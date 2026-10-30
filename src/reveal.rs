//! One-shot file selection for `decutil reveal`, outside the launcher's lifetime.
//!
//! The request runs in a killable child so connection/authentication and reply
//! time together have a three-second budget. A timeout is ambiguous: a file
//! manager may already have handled ShowItems before its reply. We send no
//! retries and launch at most one fallback, but cannot promise one window.
use std::{
    ffi::OsStr,
    os::unix::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const REQUEST_BUDGET: Duration = Duration::from_secs(3);
pub const WORKER_ARG: &str = "--file-manager-show-items";

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// FileManager1 acknowledged the selection request.
    Selected,
    /// A containing-folder opener was launched; selection is not guaranteed.
    OpenedParent,
}

/// Encode local path bytes without resolving symlinks or interpreting URL syntax.
pub fn file_uri(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("A file URI needs an absolute path".into());
    }
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte == 0 {
            return Err("A file path cannot contain NUL".into());
        }
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(&mut uri, "%{byte:02X}").unwrap();
        }
    }
    Ok(uri)
}

struct Target {
    parent: PathBuf,
    uri: String,
    present: bool,
}
impl Target {
    fn new(path: &Path) -> Result<Self, String> {
        if path.as_os_str().is_empty() {
            return Err("Choose a file or folder to show".into());
        }
        let path = if path.is_absolute() { path.to_owned() } else { std::env::current_dir().map_err(|e| e.to_string())?.join(path) };
        let uri = file_uri(&path)?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).ok_or("This path has no containing folder")?.to_owned();
        if !parent.is_dir() {
            return Err("The containing folder no longer exists or is unavailable".into());
        }
        let present = path.symlink_metadata().is_ok();
        Ok(Self { parent, uri, present })
    }
}

fn perform(target: &Target, custom_opener: bool, request: impl FnOnce(&str) -> Result<(), String>, open: impl FnOnce(&Path) -> bool) -> Result<Outcome, String> {
    if !custom_opener && target.present && request(&target.uri).is_ok() {
        return Ok(Outcome::Selected);
    }
    if open(&target.parent) {
        Ok(Outcome::OpenedParent)
    } else {
        Err("Could not launch the containing folder".into())
    }
}

/// One-shot CLI operation. The private worker must be the current `decutil`
/// executable; callers in long-lived UI processes should spawn `decutil reveal`.
pub fn reveal(path: &Path) -> Result<Outcome, String> {
    let target = Target::new(path)?;
    let custom = std::env::var("DECEBAL_OPEN").is_ok_and(|s| !s.trim().is_empty());
    perform(&target, custom, request_bounded, |parent| {
        let fallback = parent.to_str().map(str::to_owned).or_else(|| file_uri(parent).ok());
        fallback.is_some_and(|p| crate::launch::spawn_argv(&crate::launch::open_argv(&p)))
    })
}

fn request_bounded(uri: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = exe.file_name().and_then(OsStr::to_str).unwrap_or_default();
    if !matches!(name, "decutil" | "decutil (deleted)") {
        return Err("File selection must run through decutil".into());
    }
    // /proc/self/exe still resolves the running inode after an atomic upgrade.
    let until = Instant::now() + REQUEST_BUDGET;
    let owner = unsafe { libc::getpid() };
    let mut command = Command::new("/proc/self/exe");
    command.args([WORKER_ARG, uri]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: the pre-exec closure uses only async-signal-safe libc calls. The
    // parent check closes the race where its owner exits before prctl runs.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != owner {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return if status.success() { Ok(()) } else { Err("File manager did not accept selection".into()) },
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(10)),
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match outcome {
                    Err(e) => e.to_string(),
                    _ => "File manager selection timed out".into(),
                });
            }
        }
    }
}

/// Internal one-shot worker: typed `as,s` request, no fallback or retries.
pub fn show_items(uri: &str) -> Result<(), String> {
    if !uri.starts_with("file:///") || uri.as_bytes().contains(&0) {
        return Err("Expected a local absolute file URI".into());
    }
    let mut body = crate::dbus::Writer::default();
    body.array(4, |w| w.string(uri));
    body.string("");
    let message = crate::dbus::Message::call("org.freedesktop.FileManager1", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1", "ShowItems", "ass", body.buf);
    crate::dbus::Conn::session().and_then(|mut conn| conn.call_blocking(message)).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::ffi::OsStringExt;
    #[test]
    fn uri_encodes_bytes_and_keeps_symlink_identity() {
        assert_eq!(file_uri(Path::new("/tmp/a b/é#%\"'-file")).unwrap(), "file:///tmp/a%20b/%C3%A9%23%25%22%27-file");
        assert_eq!(file_uri(Path::new("/tmp/link/../file")).unwrap(), "file:///tmp/link/../file");
        assert_eq!(file_uri(Path::new(&std::ffi::OsString::from_vec(b"/tmp/\xff".to_vec()))).unwrap(), "file:///tmp/%FF");
        assert!(file_uri(Path::new("relative")).is_err());
        assert!(file_uri(Path::new(&std::ffi::OsString::from_vec(b"/tmp/\0".to_vec()))).is_err());
    }
    #[test]
    fn selection_fallback_and_override_have_one_owner() {
        let target = Target { parent: "/tmp".into(), uri: "file:///tmp/file".into(), present: true };
        assert_eq!(perform(&target, false, |_| Ok(()), |_| panic!("no fallback after acknowledgment")), Ok(Outcome::Selected));
        let calls = Cell::new(0);
        assert_eq!(
            perform(
                &target,
                false,
                |_| Err("no service".into()),
                |p| {
                    assert_eq!(p, Path::new("/tmp"));
                    calls.set(calls.get() + 1);
                    true
                }
            ),
            Ok(Outcome::OpenedParent)
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(perform(&target, true, |_| panic!("custom opener must bypass service"), |_| true), Ok(Outcome::OpenedParent));
        let missing = Target { present: false, ..target };
        assert_eq!(perform(&missing, false, |_| panic!("vanished item must bypass service"), |_| true), Ok(Outcome::OpenedParent));
        assert!(perform(&missing, false, |_| panic!(), |_| false).is_err());
    }
    #[test]
    fn target_preserves_real_symlink_and_accepts_vanished_item() {
        let root = std::env::temp_dir().join(format!("decebal-reveal-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let link = root.join("-link #é");
        std::os::unix::fs::symlink("missing-target", &link).unwrap();
        let target = Target::new(&link).unwrap();
        assert_eq!(target.uri, file_uri(&link).unwrap());
        assert!(target.present);
        assert_eq!(target.parent, root);
        assert!(!Target::new(&root.join("vanished")).unwrap().present);
        assert!(Target::new(&root.join("missing-parent/file")).is_err());
        assert!(Target::new(Path::new("")).is_err());
        assert!(Target::new(Path::new("/")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
