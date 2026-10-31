//! One window per app: launching it again closes the running one (a toggle), like the original launchers.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;

/// `file` inside `$XDG_RUNTIME_DIR` (the temp dir when that is unset): where instances keep their lock and mailbox files.
pub fn runtime_file(file: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(std::env::temp_dir);
    dir.join(file)
}

fn lock_path(name: &str) -> PathBuf {
    runtime_file(&format!("{name}.lock"))
}

/// The pid of a still-running instance of the pre-rename program (`macbar` for `decbar`) that holds its old lock, if any.
/// A session that was started before the update still runs the old programs; the new ones must not start a second copy.
fn legacy_holder(name: &str) -> Option<i32> {
    let path = lock_path(&crate::compat::legacy_lock_name(name)?);
    let mut file = OpenOptions::new().read(true).open(path).ok()?;
    // SAFETY: flock on a descriptor we own; it only takes or refuses an advisory lock (released when `file` drops).
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return None;
    }
    let mut pid = String::new();
    let _ = file.read_to_string(&mut pid);
    Some(pid.trim().parse::<i32>().unwrap_or(0))
}

/// `Some(guard)` when this is the only instance (keep the guard alive for the whole run). When another
/// instance already holds the lock it is asked to quit (SIGTERM) and `None` is returned: just exit.
pub fn acquire(name: &str) -> Option<File> {
    acquire_signalling(name, libc::SIGTERM)
}

/// Single-instance toggle for ephemeral launchers. Call before publishing the lock.
/// A second invocation sends SIGUSR1, which the holder treats as a successful
/// close. SIGTERM and crashes keep their ordinary status, so launch monitoring
/// still reports genuine failures. The launcher stores no unsaved editor state.
/// Unlike a blocked/sigwait signal, this disposition is reset on exec: launched
/// applications inherit neither a blocked signal nor our close handler.
/// This does not replace [`acquire`] for applications with a different lifecycle.
pub fn acquire_clean_toggle(name: &str) -> Option<File> {
    acquire_clean(name, Verb::Toggle)
}

/// What a launcher invocation asks of the singleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Verb {
    /// Close a running instance, otherwise open (a bare invocation).
    #[default]
    Toggle,
    /// Open unless one is already running (that one is left alone).
    Open,
    /// Close a running instance; never opens one.
    Close,
}

/// [`acquire_clean_toggle`] with an explicit verb: `Some(guard)` when this process should open the launcher.
/// Every request, whatever its verb, runs under [`launcher_request_guard`], so overlapping invocations are
/// ordered and their close signals cannot coalesce. A close keeps that order until the closing instance has
/// released its lock (bounded); `open` on a running launcher only probes it and returns at once.
pub fn acquire_clean(name: &str, verb: Verb) -> Option<File> {
    // Serialize requests, not the lifetime of the popup. Standard signals may
    // coalesce while a holder exits; another request must wait for that exit.
    let _request = launcher_request_guard(name)?;
    if verb == Verb::Close {
        if signal(name, libc::SIGUSR1) {
            wait_launcher_release(name);
        }
        return None;
    }
    if !install_clean_close() {
        return None;
    }
    // Signal 0 only probes the holder: `open` leaves a running launcher as it is.
    let guard = acquire_signalling(name, if verb == Verb::Open { 0 } else { libc::SIGUSR1 });
    if guard.is_none() && verb == Verb::Toggle {
        wait_launcher_release(name);
    }
    guard
}

/// A separate CLOEXEC lock orders short-lived launcher requests. Never retain
/// this guard in the UI or share it with the long-lived singleton lock.
fn launcher_request_guard(name: &str) -> Option<File> {
    let path = runtime_file(&format!("{name}.request.lock"));
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).ok()?;
    loop {
        // SAFETY: flock only locks our own open descriptor. Interrupted waits
        // retry; an unavailable runtime directory keeps the existing no-op path.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Some(file);
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return None;
        }
    }
}

/// Keep the request lock until the old holder has actually released its lock.
/// The bounded wait never turns a failed close into a second open instance.
fn wait_launcher_release(name: &str) {
    let mut paths = vec![lock_path(name)];
    if let Some(legacy) = crate::compat::legacy_lock_name(name) {
        paths.push(lock_path(&legacy));
    }
    let files: Vec<_> = paths.iter().filter_map(|path| OpenOptions::new().read(true).open(path).ok()).collect();
    for _ in 0..400 {
        // SAFETY: descriptors belong to us. Successful temporary locks release
        // when the guard goes out of scope, before a later request can proceed.
        if files.iter().all(|file| unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Installs the SIGUSR1 close disposition of [`acquire_clean_toggle`]; false when that failed.
fn install_clean_close() -> bool {
    extern "C" fn close_launcher(_: libc::c_int) {
        // SAFETY: _exit is async-signal-safe. These ephemeral launchers have no
        // editor draft to flush; the OS releases their windows, grabs and locks.
        unsafe { libc::_exit(0) };
    }
    // SAFETY: install the process's dedicated toggle disposition before any
    // other launcher threads or its discoverable singleton lock exist.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = close_launcher as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(libc::SIGUSR1, &action, std::ptr::null_mut()) != 0 {
            return false;
        }
        // A parent may reserve this signal too. Unblock only our dedicated
        // toggle after its handler exists, still before publishing the lock.
        let mut toggle: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut toggle);
        libc::sigaddset(&mut toggle, libc::SIGUSR1);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &toggle, std::ptr::null_mut()) == 0
    }
}

/// `Some(guard)` when no other instance holds the lock `name`; otherwise `None` and the running instance is left alone.
/// For daemons a supervisor restarts (decbar and its bottom half): a second copy simply exits instead of replacing the first.
pub fn hold(name: &str) -> Option<File> {
    if legacy_holder(name).is_some() {
        return None;
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path(name)).ok()?;
    // SAFETY: flock on a descriptor we own; it only takes or refuses an advisory lock.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return None;
    }
    let mut file = file;
    let _ = file.set_len(0);
    let _ = write!(file, "{}", std::process::id());
    Some(file)
}

/// Like [`acquire`], but a running instance gets `sig` instead of SIGTERM (decswitch: SIGUSR1 = next, SIGUSR2 = previous).
/// The caller should have blocked `sig` (see [`block_signals`]) before this, so a signal that arrives early is held, not fatal.
pub fn acquire_signalling(name: &str, sig: i32) -> Option<File> {
    if let Some(pid) = legacy_holder(name) {
        if pid > 1 && pid as u32 != std::process::id() {
            // SAFETY: plain kill(2) with the pid the old instance recorded.
            unsafe { libc::kill(pid, sig) };
        }
        return None;
    }
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path(name)).ok()?;
    // SAFETY: flock on a descriptor we own; it only takes or refuses an advisory lock.
    let got = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if got {
        let _ = file.set_len(0);
        let _ = file.seek(SeekFrom::Start(0));
        let _ = write!(file, "{}", std::process::id());
        return Some(file);
    }
    let mut pid = String::new();
    let _ = file.read_to_string(&mut pid);
    if let Ok(pid) = pid.trim().parse::<i32>() {
        if pid > 1 && pid as u32 != std::process::id() {
            // SAFETY: plain kill(2) with the pid the running instance recorded.
            unsafe { libc::kill(pid, sig) };
        }
    }
    None
}

/// Sends `sig` to the running instance that holds the lock `name`; false when none runs (or it is this process).
pub fn signal(name: &str, sig: i32) -> bool {
    let Ok(mut file) = OpenOptions::new().read(true).open(lock_path(name)) else { return false };
    // SAFETY: flock on a descriptor we own; getting the lock means nobody holds it (it is released when `file` drops).
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return false;
    }
    let mut pid = String::new();
    let _ = file.read_to_string(&mut pid);
    match pid.trim().parse::<i32>() {
        // SAFETY: plain kill(2) with the pid the running instance recorded.
        Ok(pid) if pid > 1 && pid as u32 != std::process::id() => unsafe { libc::kill(pid, sig) == 0 },
        _ => false,
    }
}

/// Blocks `sigs` in the calling thread (and so in every thread it spawns later); a thread then collects them with [`wait_signal`].
pub fn block_signals(sigs: &[i32]) {
    // SAFETY: builds a signal set on the stack and applies it with pthread_sigmask; no handlers are touched.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in sigs {
            libc::sigaddset(&mut set, *s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Blocks until one of `sigs` (which must be blocked) is delivered and returns it.
pub fn wait_signal(sigs: &[i32]) -> Option<i32> {
    // SAFETY: sigwait on a set built here; it only reads the set and writes `got`.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in sigs {
            libc::sigaddset(&mut set, *s);
        }
        let mut got: libc::c_int = 0;
        (libc::sigwait(&set, &mut got) == 0).then_some(got)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_running_pre_rename_instance_blocks_the_new_program() {
        // `decclip-<pid>` has no legacy twin; use a real program name with a pid-specific role so nothing else is affected.
        let role = format!("decbar-legacytest{}", std::process::id());
        let legacy = format!("macbar-legacytest{}", std::process::id());
        assert_eq!(crate::compat::legacy_lock_name(&role).as_deref(), Some(legacy.as_str()));
        assert!(super::hold(&role).map(drop).is_some(), "free while the old program is not running");
        let old = super::hold(&legacy).expect("the old program takes its lock");
        assert!(super::hold(&role).is_none(), "the new program does not start a second copy");
        // a toggle app signals the old instance (here: ourselves, which is skipped) and exits
        assert!(super::acquire_signalling(&role, libc::SIGUSR2).is_none());
        drop(old);
        assert!(super::hold(&role).is_some(), "free again once the old one is gone");
        let _ = std::fs::remove_file(super::lock_path(&role));
        let _ = std::fs::remove_file(super::lock_path(&legacy));
    }

    #[test]
    fn hold_is_one_lock_per_role_and_never_signals_the_holder() {
        let base = format!("decebal-test-hold-{}", std::process::id());
        let (main, bottom) = (format!("{base}-decbar"), format!("{base}-decbar-bottom"));
        let try_hold = |n: &str| {
            for _ in 0..50 {
                if let Some(f) = super::hold(n) {
                    return Some(f);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            None
        };
        let a = try_hold(&main).expect("the main role is free");
        assert!(super::hold(&main).is_none(), "a second main bar is refused");
        let b = try_hold(&bottom).expect("the bottom role has a lock of its own");
        assert!(super::hold(&bottom).is_none());
        assert_eq!(std::fs::read_to_string(super::lock_path(&main)).unwrap(), std::process::id().to_string(), "the holder's pid is recorded");
        drop(a);
        let a2 = try_hold(&main);
        assert!(a2.is_some(), "released with the guard");
        drop((a2, b));
        let _ = std::fs::remove_file(super::lock_path(&main));
        let _ = std::fs::remove_file(super::lock_path(&bottom));
    }

    #[test]
    fn close_never_opens_and_open_probes_without_signalling() {
        let name = format!("decebal-test-verbs-{}", std::process::id());
        assert!(super::acquire_clean(&name, super::Verb::Close).is_none(), "close with nothing running opens nothing");
        assert!(super::hold(&name).is_some(), "and leaves no holder behind");
        // the request guard is released when the request returns
        let request = super::launcher_request_guard(&name).expect("free again");
        drop(request);
        let _ = std::fs::remove_file(super::lock_path(&name));
        let _ = std::fs::remove_file(super::runtime_file(&format!("{name}.request.lock")));
    }

    #[test]
    fn second_acquire_is_refused_while_the_first_is_held() {
        let name = format!("decebal-test-{}", std::process::id());
        // Another test may have a child between fork and exec that still shares the descriptor for a moment.
        let mut first = None;
        for _ in 0..50 {
            first = super::acquire(&name);
            if first.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(first.is_some());
        // The same process asking again would signal itself; the pid check keeps that a no-op.
        assert!(super::acquire(&name).is_none());
        drop(first);
        // Another test may have a child between fork and exec that still shares the descriptor for a moment.
        let mut again = None;
        for _ in 0..50 {
            again = super::acquire(&name);
            if again.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(again.is_some());
        let _ = std::fs::remove_file(super::lock_path(&name));
    }
}
