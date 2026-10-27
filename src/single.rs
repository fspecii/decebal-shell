//! One window per app: launching it again closes the running one (a toggle), like the original launchers.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;

fn lock_path(name: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("{name}.lock"))
}

/// `Some(guard)` when this is the only instance (keep the guard alive for the whole run). When another
/// instance already holds the lock it is asked to quit (SIGTERM) and `None` is returned: just exit.
pub fn acquire(name: &str) -> Option<File> {
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
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
    None
}

#[cfg(test)]
mod tests {
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
