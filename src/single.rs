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

/// `Some(guard)` when this is the only instance (keep the guard alive for the whole run). When another
/// instance already holds the lock it is asked to quit (SIGTERM) and `None` is returned: just exit.
pub fn acquire(name: &str) -> Option<File> {
    acquire_signalling(name, libc::SIGTERM)
}

/// `Some(guard)` when no other instance holds the lock `name`; otherwise `None` and the running instance is left alone.
/// For daemons a supervisor restarts (macbar and its bottom half): a second copy simply exits instead of replacing the first.
pub fn hold(name: &str) -> Option<File> {
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

/// Like [`acquire`], but a running instance gets `sig` instead of SIGTERM (macswitch: SIGUSR1 = next, SIGUSR2 = previous).
/// The caller should have blocked `sig` (see [`block_signals`]) before this, so a signal that arrives early is held, not fatal.
pub fn acquire_signalling(name: &str, sig: i32) -> Option<File> {
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
    fn hold_is_one_lock_per_role_and_never_signals_the_holder() {
        let base = format!("macshell-test-hold-{}", std::process::id());
        let (main, bottom) = (format!("{base}-macbar"), format!("{base}-macbar-bottom"));
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
    fn second_acquire_is_refused_while_the_first_is_held() {
        let name = format!("macshell-test-{}", std::process::id());
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
