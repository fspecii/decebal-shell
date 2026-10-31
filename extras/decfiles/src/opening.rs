//! Opening one local file with a registered application. Typed GIO only: no shell, no
//! command line is ever built, file contents are never executed and permissions are never
//! changed. The target is a `GFile` made from the exact Unix path (non-UTF8 names survive
//! as percent-encoded URI bytes). Every step is asynchronous and cancellable, and every
//! request has an identity, so a late completion can never act for a newer request.
use gio::prelude::*;
use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// Default registered application, or a user-chosen one (Open With).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Default,
    Chooser,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Regular,
    Directory,
    Other,
}

/// What a metadata query (symlinks followed, nothing opened except GIO's own type sniff) says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub kind: Kind,
    pub content_type: String,
    pub mode: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Executable,
    DesktopEntry,
    Folder,
    Special,
}

impl Refusal {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Executable => "refused-executable",
            Self::DesktopEntry => "refused-desktop-entry",
            Self::Folder => "refused-folder",
            Self::Special => "refused-special",
        }
    }

    pub fn message(self, name: &str) -> String {
        match self {
            Self::Executable => format!("“{name}” is an executable file. Files never runs programs, so it was not opened. Use Open With… to view it in an application you choose."),
            Self::DesktopEntry => format!("“{name}” is a launcher (.desktop) file. Files never runs launchers, so it was not opened. Use Open With… to view it in an application you choose."),
            Self::Folder => format!("“{name}” is now a folder, not a file. Refresh the folder view."),
            Self::Special => format!("“{name}” is not a regular file (device, pipe or socket), so it was not opened."),
        }
    }
}

const PROGRAM_TYPES: &[&str] = &["application/x-executable", "application/x-pie-executable", "application/x-sharedlib", "application/vnd.appimage"];

pub fn is_desktop_entry(path: &Path, content_type: &str) -> bool {
    content_type == "application/x-desktop" || path.file_name().is_some_and(|name| name.as_bytes().to_ascii_lowercase().ends_with(b".desktop"))
}

/// Any execute bit (of the symlink target), or a program content type.
pub fn is_executable(probe: &Probe) -> bool {
    probe.mode & 0o111 != 0 || PROGRAM_TYPES.contains(&probe.content_type.as_str())
}

/// Default activation never hands programs or launchers to anything. Open With is the user's
/// explicit choice of an application, so executables and launchers may be *viewed* that way.
/// Folders, devices, pipes and sockets are never launched.
pub fn judge(path: &Path, probe: &Probe, purpose: Purpose) -> Result<(), Refusal> {
    match probe.kind {
        Kind::Directory => Err(Refusal::Folder),
        Kind::Other => Err(Refusal::Special),
        Kind::Regular if purpose == Purpose::Chooser => Ok(()),
        Kind::Regular if is_desktop_entry(path, &probe.content_type) => Err(Refusal::DesktopEntry),
        Kind::Regular if is_executable(probe) => Err(Refusal::Executable),
        Kind::Regular => Ok(()),
    }
}

impl Probe {
    fn from_info(info: &gio::FileInfo) -> Self {
        let kind = match info.file_type() {
            gio::FileType::Regular => Kind::Regular,
            gio::FileType::Directory => Kind::Directory,
            _ => Kind::Other,
        };
        let mode = if info.has_attribute("unix::mode") { info.attribute_uint32("unix::mode") } else { 0 };
        Self { kind, content_type: info.content_type().map(|value| value.to_string()).unwrap_or_default(), mode }
    }
}

/// Asynchronous metadata query for the exact path. A dangling link or missing file is an error.
pub fn probe(path: &Path, cancel: &gio::Cancellable, done: impl FnOnce(Result<Probe, glib::Error>) + 'static) {
    gio::File::for_path(path).query_info_async("standard::type,standard::content-type,unix::mode", gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT, Some(cancel), move |result| {
        done(result.map(|info| Probe::from_info(&info)))
    });
}

/// The registered default handler for a content type, looked up off the UI thread.
pub fn default_app(content_type: &str, cancel: &gio::Cancellable, done: impl FnOnce(Result<gio::AppInfo, glib::Error>) + 'static) {
    gio::AppInfo::default_for_type_async(content_type, false, Some(cancel), done);
}

/// Launch `app` with exactly this file. GIO substitutes the path into the application's
/// Exec line as one argument; there is no shell and no string interpolation here.
/// This never records the app as a default or "last used" handler.
pub fn launch_with<P: IsA<gio::AppLaunchContext>>(app: &gio::AppInfo, path: &Path, context: Option<&P>, cancel: &gio::Cancellable, done: impl FnOnce(Result<(), glib::Error>) + 'static) {
    let uri = gio::File::for_path(path).uri();
    app.launch_uris_async(&[uri.as_str()], context, Some(cancel), done);
}

/// Where a request was made. A completion for any other scope is stale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub folder: Option<PathBuf>,
    pub tab: usize,
    pub search: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Probing,
    Choosing,
    Launching,
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub id: u64,
    pub path: PathBuf,
    pub purpose: Purpose,
    pub scope: Scope,
    pub stage: Stage,
    pub cancel: gio::Cancellable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub id: u64,
    pub path: PathBuf,
    pub purpose: Purpose,
    pub status: &'static str,
    pub message: String,
}

/// At most one open request is current. Callers cancel the returned `Cancellable` after
/// releasing their borrow of the tracker.
#[derive(Default)]
pub struct Tracker {
    last_id: u64,
    pending: Option<Pending>,
    last: Option<Record>,
}

impl Tracker {
    pub fn begin(&mut self, path: PathBuf, purpose: Purpose, scope: Scope) -> (Pending, Option<gio::Cancellable>) {
        let replaced = self.cancel("Superseded by a newer open request.");
        self.last_id = self.last_id.wrapping_add(1);
        let pending = Pending { id: self.last_id, path, purpose, scope, stage: Stage::Probing, cancel: gio::Cancellable::new() };
        self.pending = Some(pending.clone());
        (pending, replaced)
    }

    pub fn current(&self, id: u64) -> Option<&Pending> {
        self.pending.as_ref().filter(|pending| pending.id == id)
    }

    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    pub fn last(&self) -> Option<&Record> {
        self.last.as_ref()
    }

    pub fn advance(&mut self, id: u64, stage: Stage) -> bool {
        match self.pending.as_mut().filter(|pending| pending.id == id) {
            Some(pending) => {
                pending.stage = stage;
                true
            }
            None => false,
        }
    }

    /// Complete the current request. False (and nothing recorded) when `id` is stale.
    pub fn finish(&mut self, id: u64, status: &'static str, message: String) -> bool {
        if self.current(id).is_none() {
            return false;
        }
        let pending = self.pending.take().unwrap();
        self.last = Some(Record { id, path: pending.path, purpose: pending.purpose, status, message });
        true
    }

    /// Drop the current request (if any); the caller must cancel the returned token.
    pub fn cancel(&mut self, message: &str) -> Option<gio::Cancellable> {
        let pending = self.pending.take()?;
        self.last = Some(Record { id: pending.id, path: pending.path, purpose: pending.purpose, status: "cancelled", message: message.into() });
        Some(pending.cancel)
    }

    /// A selection that is no longer the request's exact target supersedes the request.
    pub fn cancel_unless_target(&mut self, selected: Option<&Path>, message: &str) -> Option<gio::Cancellable> {
        if self.pending.as_ref().is_some_and(|pending| Some(pending.path.as_path()) != selected) {
            self.cancel(message)
        } else {
            None
        }
    }

    pub fn evidence(&self) -> serde_json::Value {
        let describe = |id: u64, path: &Path, purpose: Purpose| serde_json::json!({"id":id, "path":path.to_string_lossy(), "path_bytes":path.as_os_str().as_bytes(), "purpose":format!("{purpose:?}")});
        let pending = self.pending.as_ref().map(|pending| {
            let mut value = describe(pending.id, &pending.path, pending.purpose);
            value["stage"] = format!("{:?}", pending.stage).into();
            value
        });
        let last = self.last.as_ref().map(|record| {
            let mut value = describe(record.id, &record.path, record.purpose);
            value["status"] = record.status.into();
            value["message"] = record.message.clone().into();
            value
        });
        serde_json::json!({"pending":pending, "last":last, "requests":self.last_id})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::PermissionsExt},
        rc::Rc,
    };

    fn scope() -> Scope {
        Scope { folder: Some("/base".into()), tab: 0, search: None }
    }
    fn probe_of(kind: Kind, content_type: &str, mode: u32) -> Probe {
        Probe { kind, content_type: content_type.into(), mode }
    }

    #[test]
    fn default_activation_refuses_programs_launchers_and_non_files_but_chooser_may_view_them() {
        let text = probe_of(Kind::Regular, "text/plain", 0o100644);
        assert_eq!(judge(Path::new("/b/a.txt"), &text, Purpose::Default), Ok(()));
        let script = probe_of(Kind::Regular, "text/x-shellscript", 0o100755);
        assert_eq!(judge(Path::new("/b/run.sh"), &script, Purpose::Default), Err(Refusal::Executable));
        assert_eq!(judge(Path::new("/b/run.sh"), &script, Purpose::Chooser), Ok(()));
        let elf = probe_of(Kind::Regular, "application/x-pie-executable", 0o100644);
        assert_eq!(judge(Path::new("/b/tool"), &elf, Purpose::Default), Err(Refusal::Executable));
        // Not executable by mode or type: a data file that merely has a script type opens normally.
        let data_script = probe_of(Kind::Regular, "text/x-shellscript", 0o100644);
        assert_eq!(judge(Path::new("/b/run.sh"), &data_script, Purpose::Default), Ok(()));
        let desktop = probe_of(Kind::Regular, "application/x-desktop", 0o100644);
        assert_eq!(judge(Path::new("/b/app.desktop"), &desktop, Purpose::Default), Err(Refusal::DesktopEntry));
        assert_eq!(judge(Path::new("/b/app.desktop"), &desktop, Purpose::Chooser), Ok(()));
        // The name alone is enough, whatever type sniffing said, and the check is case-insensitive.
        assert_eq!(judge(Path::new("/b/APP.Desktop"), &text, Purpose::Default), Err(Refusal::DesktopEntry));
        for purpose in [Purpose::Default, Purpose::Chooser] {
            assert_eq!(judge(Path::new("/b/dir"), &probe_of(Kind::Directory, "inode/directory", 0o40755), purpose), Err(Refusal::Folder));
            assert_eq!(judge(Path::new("/b/pipe"), &probe_of(Kind::Other, "inode/fifo", 0o10644), purpose), Err(Refusal::Special));
        }
        assert!(Refusal::Executable.message("tool").contains("never runs programs"));
        assert!(Refusal::DesktopEntry.message("a.desktop").contains("Open With"));
    }

    #[test]
    fn desktop_entry_names_are_matched_on_raw_bytes() {
        let raw = PathBuf::from(OsString::from_vec(b"/b/\xff.DESKTOP".to_vec()));
        assert!(is_desktop_entry(&raw, ""));
        assert!(!is_desktop_entry(Path::new("/b/desktop"), "text/plain"));
        assert!(!is_desktop_entry(Path::new("/b/a.desktop.txt"), "text/plain"));
    }

    #[test]
    fn requests_have_identity_and_stale_completions_are_ignored() {
        let mut tracker = Tracker::default();
        let (first, replaced) = tracker.begin("/base/a".into(), Purpose::Default, scope());
        assert!(replaced.is_none());
        let (second, replaced) = tracker.begin("/base/b".into(), Purpose::Chooser, scope());
        let replaced = replaced.expect("the first request is superseded");
        assert!(second.id != first.id);
        replaced.cancel();
        assert!(first.cancel.is_cancelled() && !second.cancel.is_cancelled());
        assert_eq!(tracker.last().map(|record| (record.id, record.status)), Some((first.id, "cancelled")));
        // A late completion for the first request changes nothing and records nothing.
        assert!(tracker.current(first.id).is_none());
        assert!(!tracker.finish(first.id, "launched", "late".into()));
        assert!(!tracker.advance(first.id, Stage::Launching));
        assert_eq!(tracker.pending().map(|pending| pending.id), Some(second.id));
        assert!(tracker.advance(second.id, Stage::Choosing));
        assert_eq!(tracker.pending().map(|pending| pending.stage), Some(Stage::Choosing));
        assert!(tracker.finish(second.id, "launched", "done".into()));
        assert!(tracker.pending().is_none());
        assert!(!tracker.finish(second.id, "launched", "twice".into()));
        assert_eq!(tracker.last().map(|record| record.message.as_str()), Some("done"));
    }

    #[test]
    fn a_changed_selection_cancels_only_when_it_is_not_the_exact_target() {
        let mut tracker = Tracker::default();
        let raw = PathBuf::from(OsString::from_vec(b"/base/raw-\xff".to_vec()));
        let (pending, _) = tracker.begin(raw.clone(), Purpose::Default, scope());
        // Re-selecting the same target (a refresh re-render) keeps the request.
        assert!(tracker.cancel_unless_target(Some(&raw), "changed").is_none());
        assert!(tracker.pending().is_some());
        // A lossy look-alike is a different file.
        let token = tracker.cancel_unless_target(Some(Path::new("/base/raw-\u{fffd}")), "changed");
        assert!(token.is_some());
        assert!(!pending.cancel.is_cancelled(), "the caller cancels the returned token");
        token.unwrap().cancel();
        assert!(pending.cancel.is_cancelled());
        assert!(tracker.pending().is_none());
        tracker.begin(raw, Purpose::Default, scope());
        assert!(tracker.cancel_unless_target(None, "cleared").is_some());
        assert_eq!(tracker.last().map(|record| record.message.as_str()), Some("cleared"));
        assert!(tracker.cancel("nothing pending").is_none());
    }

    #[test]
    fn evidence_keeps_exact_path_bytes() {
        let mut tracker = Tracker::default();
        let raw = PathBuf::from(OsString::from_vec(b"/base/\xff".to_vec()));
        tracker.begin(raw.clone(), Purpose::Chooser, scope());
        let value = tracker.evidence();
        assert_eq!(value["pending"]["path_bytes"], serde_json::json!(raw.as_os_str().as_bytes()));
        assert_eq!(value["pending"]["stage"], "Probing");
        assert!(serde_json::to_string(&value).is_ok());
    }

    /// Runs `start` on a private main context until its callback delivers a value.
    fn await_result<T: 'static>(start: impl FnOnce(Box<dyn FnOnce(T)>)) -> Option<T> {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let main_loop = glib::MainLoop::new(Some(&context), false);
                let slot = Rc::new(RefCell::new(None));
                let (out, quit) = (slot.clone(), main_loop.clone());
                start(Box::new(move |value| {
                    out.replace(Some(value));
                    quit.quit();
                }));
                main_loop.run();
                let value = slot.borrow_mut().take();
                value
            })
            .unwrap()
    }

    fn query(path: &Path, cancelled: bool) -> Result<Probe, glib::Error> {
        let path = path.to_path_buf();
        await_result(move |done| {
            let cancel = gio::Cancellable::new();
            if cancelled {
                cancel.cancel();
            }
            probe(&path, &cancel, done);
        })
        .expect("probe callback")
    }

    #[test]
    fn probe_classifies_files_folders_links_pipes_and_failures_without_opening_specials() {
        let root = std::env::temp_dir().join(format!("decfiles-open-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("folder")).unwrap();
        std::fs::write(root.join("plain.txt"), b"text").unwrap();
        let tool = root.join("tool.sh");
        std::fs::write(&tool, b"#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&tool, root.join("link-to-tool")).unwrap();
        std::os::unix::fs::symlink(root.join("missing"), root.join("dangling")).unwrap();
        let fifo = root.join("pipe");
        let c_fifo = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o600) }, 0);
        let raw = root.join(OsString::from_vec(b"raw-\xff.txt".to_vec()));
        std::fs::write(&raw, b"raw").unwrap();

        let plain = query(&root.join("plain.txt"), false).unwrap();
        assert_eq!(plain.kind, Kind::Regular);
        assert_eq!(plain.mode & 0o111, 0);
        assert_eq!(judge(&root.join("plain.txt"), &plain, Purpose::Default), Ok(()));
        let script = query(&tool, false).unwrap();
        assert!(is_executable(&script));
        let link = query(&root.join("link-to-tool"), false).unwrap();
        assert_eq!(judge(&root.join("link-to-tool"), &link, Purpose::Default), Err(Refusal::Executable), "a link to a program is a program");
        assert_eq!(query(&root.join("folder"), false).unwrap().kind, Kind::Directory);
        assert_eq!(query(&fifo, false).unwrap().kind, Kind::Other);
        assert_eq!(query(&raw, false).unwrap().kind, Kind::Regular, "non-UTF8 names are addressed by exact bytes");
        assert!(query(&root.join("missing"), false).is_err());
        assert!(query(&root.join("dangling"), false).is_err());
        let cancelled = query(&root.join("plain.txt"), true).unwrap_err();
        assert!(cancelled.matches(gio::IOErrorEnum::Cancelled));
        // The probe changed nothing.
        assert_eq!(std::fs::metadata(&tool).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(std::fs::metadata(root.join("plain.txt")).unwrap().permissions().mode() & 0o111, 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn launch(app: &gio::AppInfo, path: &Path) -> Result<(), glib::Error> {
        let (app, path) = (app.clone(), path.to_path_buf());
        await_result(move |done| {
            launch_with(&app, &path, None::<&gio::AppLaunchContext>, &gio::Cancellable::new(), done);
        })
        .expect("launch callback")
    }

    #[test]
    fn launch_hands_the_exact_path_to_the_application_as_one_argument_without_a_shell() {
        let root = std::env::temp_dir().join(format!("decfiles-open-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = root.join("files");
        std::fs::create_dir_all(&files).unwrap();
        let recorder = root.join("record.sh");
        // Data for /bin/sh: records every argument NUL-terminated, then publishes atomically.
        std::fs::write(&recorder, "out=$1\nshift\n: > \"$out.tmp\"\nfor a in \"$@\"; do printf '%s\\0' \"$a\" >> \"$out.tmp\"; done\nmv \"$out.tmp\" \"$out\"\n").unwrap();
        let out = root.join("argv.out");
        let desktop = root.join("recorder.desktop");
        std::fs::write(&desktop, format!("[Desktop Entry]\nType=Application\nName=Decfiles test recorder\nExec=/bin/sh {} {} %f\nTerminal=false\n", recorder.display(), out.display())).unwrap();
        let app: gio::AppInfo = gio::DesktopAppInfo::from_filename(&desktop).expect("desktop entry parses").upcast();
        let names: Vec<Vec<u8>> = vec![
            b"plain name.txt".to_vec(),
            b"  leading and trailing  ".to_vec(),
            b"meta $(touch decfiles-open-pwned-1) ; touch decfiles-open-pwned-2 `touch decfiles-open-pwned-3` 'q' \"d\" & | * ? %f %U ~ # \\ .txt".to_vec(),
            b"new\nline.txt".to_vec(),
            b"raw-\xff\xfe-not-utf8.txt".to_vec(),
            "ünï çödé #%.txt".as_bytes().to_vec(),
        ];
        for name in names {
            let path = files.join(OsString::from_vec(name));
            std::fs::write(&path, b"x").unwrap();
            let _ = std::fs::remove_file(&out);
            launch(&app, &path).unwrap_or_else(|error| panic!("launch of {path:?} failed: {error}"));
            let mut recorded = None;
            for _ in 0..200 {
                if let Ok(bytes) = std::fs::read(&out) {
                    recorded = Some(bytes);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let mut expected = path.as_os_str().as_bytes().to_vec();
            expected.push(0);
            assert_eq!(recorded.as_deref(), Some(expected.as_slice()), "argument for {path:?}");
        }
        for sentinel in ["decfiles-open-pwned-1", "decfiles-open-pwned-2", "decfiles-open-pwned-3"] {
            assert!(!Path::new(sentinel).exists() && !files.join(sentinel).exists() && !root.join(sentinel).exists(), "{sentinel} must not exist: no shell saw the name");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_program_is_a_reported_launch_failure() {
        let root = std::env::temp_dir().join(format!("decfiles-open-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let desktop = root.join("missing.desktop");
        std::fs::write(&desktop, format!("[Desktop Entry]\nType=Application\nName=Missing\nExec={}/no-such-program %f\n", root.display())).unwrap();
        let app: gio::AppInfo = gio::DesktopAppInfo::from_filename(&desktop).expect("desktop entry parses").upcast();
        let file = root.join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        let error = launch(&app, &file).expect_err("launching a missing program must fail");
        assert!(error.message().contains("no-such-program"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_content_type_without_a_registered_handler_is_an_error_not_a_guess() {
        let error = await_result(|done| default_app("application/x-decfiles-no-such-type", &gio::Cancellable::new(), done)).expect("callback");
        assert!(error.is_err());
    }
}
