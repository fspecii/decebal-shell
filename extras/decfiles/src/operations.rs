//! Single-item operations. Callers bound concurrency; no shell commands or delete fallback.
use gio::prelude::*;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Action {
    CreateFolder { parent: PathBuf, name: String },
    Rename { source: PathBuf, name: String },
    Trash { source: PathBuf },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailureKind {
    InvalidInput,
    Exists,
    PermissionDenied,
    Cancelled,
    Unsupported,
    Other,
}

#[derive(Debug)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    /// Cancellation can race a GIO filesystem commit. Never promise rollback.
    pub may_have_changed: bool,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub parent: PathBuf,
    pub selected: Option<PathBuf>,
    pub message: String,
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Enter a name.".into());
    }
    if name == "." || name == ".." {
        return Err("Choose a name other than \".\" or \"..\".".into());
    }
    if name.contains('/') || name.contains('\0') {
        return Err("A name cannot contain / or a NUL character.".into());
    }
    Ok(())
}

fn valid_parent(parent: &Path) -> bool {
    parent.is_absolute() && !parent.components().any(|part| matches!(part, Component::ParentDir))
}

impl Action {
    pub fn validate(&self) -> Result<(), Failure> {
        let invalid = |message| Failure { kind: FailureKind::InvalidInput, message, may_have_changed: false };
        let parent = match self {
            Self::CreateFolder { parent, name } => {
                validate_name(name).map_err(invalid)?;
                parent.as_path()
            }
            Self::Rename { source, name } => {
                validate_name(name).map_err(invalid)?;
                let parent = source.parent().filter(|_| source.file_name().is_some()).ok_or_else(|| invalid("Select an item inside a folder.".into()))?;
                if parent.join(name) == *source {
                    return Err(invalid("Enter a different name.".into()));
                }
                parent
            }
            Self::Trash { source } => source.parent().filter(|_| source.file_name().is_some()).ok_or_else(|| invalid("The file-system root cannot be moved to Trash.".into()))?,
        };
        if !valid_parent(parent) {
            return Err(invalid("Operations require an absolute, resolved parent folder.".into()));
        }
        Ok(())
    }

    pub fn parent(&self) -> PathBuf {
        match self {
            Self::CreateFolder { parent, .. } => parent.clone(),
            Self::Rename { source, .. } | Self::Trash { source } => source.parent().unwrap_or(Path::new("/")).to_path_buf(),
        }
    }

    fn outcome(&self) -> Outcome {
        let parent = self.parent();
        match self {
            Self::CreateFolder { name, .. } => Outcome { selected: Some(parent.join(name)), parent, message: "Folder created.".into() },
            Self::Rename { name, .. } => Outcome { selected: Some(parent.join(name)), parent, message: "Item renamed.".into() },
            Self::Trash { .. } => Outcome { selected: None, parent, message: "Item moved to Trash.".into() },
        }
    }
}

fn cancelled_before_start() -> Failure {
    Failure { kind: FailureKind::Cancelled, message: "Cancelled before the operation started. Nothing was changed.".into(), may_have_changed: false }
}

fn gio_failure(error: glib::Error) -> Failure {
    let kind = match error.kind::<gio::IOErrorEnum>() {
        Some(gio::IOErrorEnum::Exists) => FailureKind::Exists,
        Some(gio::IOErrorEnum::PermissionDenied) => FailureKind::PermissionDenied,
        Some(gio::IOErrorEnum::Cancelled) => FailureKind::Cancelled,
        Some(gio::IOErrorEnum::NotSupported) => FailureKind::Unsupported,
        _ => FailureKind::Other,
    };
    let may_have_changed = kind == FailureKind::Cancelled;
    let message = match &kind {
        FailureKind::Cancelled => "Cancelled. The change may already have finished. Refreshing the folder so you can check it.".into(),
        FailureKind::Exists => "An item with that name already exists. Choose another name.".into(),
        FailureKind::PermissionDenied => "You don't have permission to change items in this folder.".into(),
        FailureKind::Unsupported => "This operation is not supported for this item.".into(),
        _ => error.to_string(),
    };
    Failure { kind, message, may_have_changed }
}

/// Typed GIO async mkdir/trash; atomic Linux no-replace rename on GIO's worker pool.
/// A returned success is never replaced by a late cancellation after rename commits.
pub fn run(action: Action, cancel: &gio::Cancellable, done: impl FnOnce(Result<Outcome, Failure>) + 'static) {
    if let Err(error) = action.validate() {
        done(Err(error));
        return;
    }
    if cancel.is_cancelled() {
        done(Err(cancelled_before_start()));
        return;
    }
    #[cfg(feature = "fixture-delay")]
    if let Some(delay) = std::env::var("DECFILES_FIXTURE_DELAY_MS").ok().and_then(|value| value.parse::<u64>().ok()).filter(|delay| *delay > 0) {
        let cancel = cancel.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(delay.min(30_000)), move || run_now(action, &cancel, done));
        return;
    }
    run_now(action, cancel, done);
}

fn run_now(action: Action, cancel: &gio::Cancellable, done: impl FnOnce(Result<Outcome, Failure>) + 'static) {
    if cancel.is_cancelled() {
        done(Err(cancelled_before_start()));
        return;
    }
    let outcome = action.outcome();
    match action {
        Action::CreateFolder { parent, name } => {
            gio::File::for_path(parent.join(name)).make_directory_async(glib::Priority::DEFAULT, Some(cancel), move |result| done(result.map(|()| outcome).map_err(gio_failure)));
        }
        Action::Trash { source } => {
            // Do not resolve the selected leaf: this must trash a symlink, not its target.
            gio::File::for_path(source).trash_async(glib::Priority::DEFAULT, Some(cancel), move |result| done(result.map(|()| outcome).map_err(gio_failure)));
        }
        Action::Rename { source, name } => {
            let cancel = cancel.clone();
            glib::MainContext::ref_thread_default().spawn_local(async move {
                let result = gio::spawn_blocking(move || rename_noreplace(&source, &name, &cancel)).await;
                done(match result {
                    Ok(result) => result.map(|()| outcome),
                    Err(_) => Err(Failure { kind: FailureKind::Other, message: "The rename worker stopped unexpectedly. Check the folder before retrying.".into(), may_have_changed: true }),
                });
            });
        }
    }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(source: &Path, name: &str, cancel: &gio::Cancellable) -> Result<(), Failure> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::ffi::OsStrExt,
        },
    };
    let parent = source.parent().unwrap();
    let parent_c = CString::new(parent.as_os_str().as_bytes()).map_err(|_| Failure { kind: FailureKind::InvalidInput, message: "Invalid parent path.".into(), may_have_changed: false })?;
    let leaf = CString::new(source.file_name().unwrap().as_bytes()).map_err(|_| Failure { kind: FailureKind::InvalidInput, message: "Invalid selected name.".into(), may_have_changed: false })?;
    let destination = CString::new(name).map_err(|_| Failure { kind: FailureKind::InvalidInput, message: "Invalid destination name.".into(), may_have_changed: false })?;
    if cancel.is_cancelled() {
        return Err(cancelled_before_start());
    }
    // SAFETY: all strings are NUL-terminated and valid for these calls. The opened
    // directory FD is owned once and reused for both leaf names; renameat2 does not
    // follow the source leaf symlink. No preflight/overwrite fallback exists.
    let raw = unsafe { libc::open(parent_c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
    if raw < 0 {
        return Err(io_failure(std::io::Error::last_os_error()));
    }
    let directory = unsafe { OwnedFd::from_raw_fd(raw) };
    if cancel.is_cancelled() {
        return Err(cancelled_before_start());
    }
    let result = unsafe { libc::renameat2(directory.as_raw_fd(), leaf.as_ptr(), directory.as_raw_fd(), destination.as_ptr(), libc::RENAME_NOREPLACE) };
    if result != 0 {
        return Err(io_failure(std::io::Error::last_os_error()));
    }
    // The atomic commit happened. A cancellation arriving now cannot undo it or
    // replace a known success with a misleading "cancelled, nothing changed".
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn rename_noreplace(_: &Path, _: &str, _: &gio::Cancellable) -> Result<(), Failure> {
    Err(Failure { kind: FailureKind::Unsupported, message: "Atomic no-overwrite rename requires Linux renameat2 support in this prototype.".into(), may_have_changed: false })
}

#[cfg(target_os = "linux")]
fn io_failure(error: std::io::Error) -> Failure {
    let kind = match error.raw_os_error() {
        Some(libc::EEXIST | libc::ENOTEMPTY) => FailureKind::Exists,
        Some(libc::EPERM | libc::EACCES | libc::EROFS) => FailureKind::PermissionDenied,
        Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP) => FailureKind::Unsupported,
        _ => FailureKind::Other,
    };
    let message = match &kind {
        FailureKind::Exists => "An item with that name already exists. Choose another name.".into(),
        FailureKind::PermissionDenied if error.raw_os_error() == Some(libc::EROFS) => "This folder is read-only.".into(),
        FailureKind::PermissionDenied => "You don't have permission to change items in this folder.".into(),
        FailureKind::Unsupported => "Safe renaming is not supported by this system or filesystem. The item was not renamed.".into(),
        _ => error.to_string(),
    };
    Failure { kind, message, may_have_changed: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    fn perform(action: Action, pre_cancel: bool) -> Result<Outcome, Failure> {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let loop_ = glib::MainLoop::new(Some(&context), false);
                let result = Rc::new(RefCell::new(None));
                let cancel = gio::Cancellable::new();
                if pre_cancel {
                    cancel.cancel();
                }
                let output = result.clone();
                let finished = loop_.clone();
                run(action, &cancel, move |value| {
                    output.replace(Some(value));
                    finished.quit();
                });
                if result.borrow().is_none() {
                    loop_.run();
                }
                let value = result.borrow_mut().take().unwrap();
                value
            })
            .unwrap()
    }

    #[test]
    fn validates_leaf_names_without_trimming_literal_unix_names() {
        for name in ["", ".", "..", "a/b", "a\0b"] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        for name in ["folder space", " # %.txt", "ü", "-leading"] {
            assert!(validate_name(name).is_ok());
        }
    }

    #[test]
    fn gio_cancellation_does_not_promise_an_uncommitted_operation() {
        let error = gio_failure(glib::Error::new(gio::IOErrorEnum::Cancelled, "cancelled"));
        assert_eq!(error.kind, FailureKind::Cancelled);
        assert!(error.may_have_changed);
        assert!(!error.message.contains("Nothing was changed"));
    }

    #[test]
    fn committed_rename_survives_cancellation_before_ui_completion_dispatch() {
        use std::time::{Duration, Instant};
        let parent = PathBuf::from("/var/tmp").join(format!("decfiles-late-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&parent).unwrap();
        let source = parent.join("source");
        let destination = parent.join("destination");
        std::fs::write(&source, b"committed bytes").unwrap();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let result = Rc::new(RefCell::new(None));
                let output = result.clone();
                let loop_ = glib::MainLoop::new(Some(&context), false);
                let finished = loop_.clone();
                let cancel = gio::Cancellable::new();
                run(Action::Rename { source: source.clone(), name: "destination".into() }, &cancel, move |value| {
                    output.replace(Some(value));
                    finished.quit();
                });
                // Start the async task, then leave the UI context paused until the worker
                // actually commits. Cancel before dispatching its completion callback.
                context.iteration(false);
                let until = Instant::now() + Duration::from_secs(5);
                while !destination.exists() {
                    assert!(Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(result.borrow().is_none());
                cancel.cancel();
                loop_.run();
                assert!(result.borrow_mut().take().unwrap().is_ok());
            })
            .unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&destination).unwrap(), b"committed bytes");
        std::fs::remove_dir_all(parent).unwrap();
    }

    // A fresh subprocess fixes HOME/XDG before GLib initializes cached trash paths.
    // Parent test runs never redirect process-global HOME around unrelated tests.
    #[test]
    fn isolated_operations_preserve_bytes_links_and_no_overwrite() {
        use std::{
            os::unix::{
                ffi::OsStringExt,
                fs::{symlink, PermissionsExt},
            },
            sync::{Arc, Barrier},
        };
        const CHILD: &str = "DECFILES_OPERATIONS_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let base = PathBuf::from("/var/tmp").join(format!("decfiles-operations-test-{}", std::process::id()));
            std::fs::create_dir_all(base.join("home")).unwrap();
            std::fs::create_dir_all(base.join("data")).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["operations::tests::isolated_operations_preserve_bytes_links_and_no_overwrite", "--exact", "--nocapture"])
                .env(CHILD, &base)
                .env("HOME", base.join("home"))
                .env("XDG_DATA_HOME", base.join("data"))
                .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}/no-bus", base.display()))
                .env("GIO_USE_VFS", "local")
                .status()
                .unwrap();
            assert!(status.success());
            std::fs::remove_dir_all(base).unwrap();
            return;
        }
        let root = PathBuf::from(std::env::var_os(CHILD).unwrap());
        assert!(root.starts_with("/var/tmp"));
        let parent = root.join("home/fixtures");
        std::fs::create_dir_all(&parent).unwrap();
        let create = |name: &str| Action::CreateFolder { parent: parent.clone(), name: name.into() };
        let rename = |source: PathBuf, name: &str| Action::Rename { source, name: name.into() };
        perform(create("created"), false).unwrap();
        assert!(parent.join("created").is_dir());
        let source = parent.join("source");
        let target = parent.join("existing");
        std::fs::write(&source, b"source bytes").unwrap();
        std::fs::write(&target, b"existing bytes").unwrap();
        assert_eq!(perform(rename(source.clone(), "existing"), false).unwrap_err().kind, FailureKind::Exists);
        assert_eq!(std::fs::read(&source).unwrap(), b"source bytes");
        assert_eq!(std::fs::read(&target).unwrap(), b"existing bytes");
        assert_eq!(perform(create("existing"), false).unwrap_err().kind, FailureKind::Exists);
        perform(rename(source.clone(), "renamed"), false).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(parent.join("renamed")).unwrap(), b"source bytes");
        let external = root.join("target");
        std::fs::write(&external, b"do not alter target").unwrap();
        let link = parent.join("link");
        symlink(&external, &link).unwrap();
        perform(rename(link.clone(), "renamed-link"), false).unwrap();
        let renamed_link = parent.join("renamed-link");
        assert!(std::fs::symlink_metadata(&renamed_link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_link(&renamed_link).unwrap(), external);
        assert_eq!(std::fs::read(&external).unwrap(), b"do not alter target");
        let dangling = parent.join("dangling");
        symlink(root.join("no-target"), &dangling).unwrap();
        assert_eq!(perform(rename(parent.join("renamed"), "dangling"), false).unwrap_err().kind, FailureKind::Exists);
        assert!(std::fs::symlink_metadata(&dangling).unwrap().file_type().is_symlink());
        let raw = parent.join(std::ffi::OsString::from_vec(b"raw-\xff".to_vec()));
        std::fs::write(&raw, b"raw filename bytes").unwrap();
        perform(rename(raw.clone(), "raw-renamed"), false).unwrap();
        assert!(!raw.exists());
        assert_eq!(std::fs::read(parent.join("raw-renamed")).unwrap(), b"raw filename bytes");
        for action in [create("cancelled"), rename(parent.join("renamed"), "cancelled-rename"), Action::Trash { source: parent.join("existing") }] {
            let error = perform(action, true).unwrap_err();
            assert_eq!(error.kind, FailureKind::Cancelled);
            assert!(!error.may_have_changed);
        }
        assert!(!parent.join("cancelled").exists());
        assert!(parent.join("renamed").exists());
        assert!(target.exists());
        let denied = parent.join("denied");
        std::fs::create_dir(&denied).unwrap();
        std::fs::write(denied.join("item"), b"protected").unwrap();
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(perform(Action::CreateFolder { parent: denied.clone(), name: "no".into() }, false).unwrap_err().kind, FailureKind::PermissionDenied);
        assert_eq!(perform(rename(denied.join("item"), "no"), false).unwrap_err().kind, FailureKind::PermissionDenied);
        assert!(perform(Action::Trash { source: denied.join("item") }, false).is_err());
        assert_eq!(std::fs::read(denied.join("item")).unwrap(), b"protected");
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Two live contenders for the same destination: exactly one commit; loser survives.
        for index in 0..32 {
            let a = parent.join(format!("race-a-{index}"));
            let b = parent.join(format!("race-b-{index}"));
            let dest_name = format!("race-dest-{index}");
            std::fs::write(&a, b"A").unwrap();
            std::fs::write(&b, b"B").unwrap();
            let barrier = Arc::new(Barrier::new(3));
            let handles: Vec<_> = [a.clone(), b.clone()]
                .into_iter()
                .map(|path| {
                    let barrier = barrier.clone();
                    let name = dest_name.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        rename_noreplace(&path, &name, &gio::Cancellable::new())
                    })
                })
                .collect();
            barrier.wait();
            let results: Vec<_> = handles.into_iter().map(|thread| thread.join().unwrap()).collect();
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert_eq!(results.iter().filter_map(|result| result.as_ref().err()).next().unwrap().kind, FailureKind::Exists);
            let dest = std::fs::read(parent.join(dest_name)).unwrap();
            if dest == b"A" {
                assert!(!a.exists());
                assert_eq!(std::fs::read(&b).unwrap(), b"B");
            } else {
                assert_eq!(dest, b"B");
                assert!(!b.exists());
                assert_eq!(std::fs::read(&a).unwrap(), b"A");
            }
        }
        let trash_file = parent.join("trash # %.txt");
        std::fs::write(&trash_file, b"trash bytes").unwrap();
        perform(Action::Trash { source: trash_file.clone() }, false).unwrap();
        assert!(!trash_file.exists());
        perform(Action::Trash { source: renamed_link.clone() }, false).unwrap();
        assert!(std::fs::symlink_metadata(&renamed_link).is_err());
        assert_eq!(std::fs::read(&external).unwrap(), b"do not alter target");
        let raw_trash = parent.join(std::ffi::OsString::from_vec(b"trash-raw-\xff".to_vec()));
        std::fs::write(&raw_trash, b"raw trash bytes").unwrap();
        perform(Action::Trash { source: raw_trash.clone() }, false).unwrap();
        assert!(!raw_trash.exists());
        let trash = root.join("data/Trash");
        let infos: Vec<_> = std::fs::read_dir(trash.join("info")).unwrap().map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap()).collect();
        assert_eq!(infos.len(), 3);
        assert!(infos.iter().all(|info| info.contains("[Trash Info]") && info.contains("DeletionDate=")));
        assert!(infos.iter().any(|info| info.contains("trash%20%23%20%25.txt")));
        assert!(infos.iter().any(|info| info.contains("trash-raw-%FF")));
        let files: Vec<_> = std::fs::read_dir(trash.join("files")).unwrap().map(|entry| entry.unwrap().path()).collect();
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|path| std::fs::symlink_metadata(path).unwrap().file_type().is_symlink() && std::fs::read_link(path).unwrap() == external));
        assert!(files.iter().any(|path| std::fs::read(path).is_ok_and(|bytes| bytes == b"trash bytes")));
        assert!(files.iter().any(|path| std::fs::read(path).is_ok_and(|bytes| bytes == b"raw trash bytes")));
    }
}
