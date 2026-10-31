//! One inotify worker per window watching only the current folder. The watch is added
//! off the UI thread (it performs a path lookup); changes are reported latest-only
//! through a wake socket, like the path resolver. Bounded: one thread, one watch.
use std::{
    ffi::CString,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, net::UnixStream},
    },
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

const MASK: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_MODIFY
    | libc::IN_ATTRIB
    | libc::IN_CLOSE_WRITE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_ONLYDIR;

#[derive(Default)]
struct State {
    wanted: Option<(u64, PathBuf)>,
    watching: Option<u64>,
    changed: Option<u64>,
    stopped: bool,
}

pub struct Monitor {
    state: Arc<Mutex<State>>,
    control: Mutex<UnixStream>,
}

impl Monitor {
    /// Returns the monitor and the UI wake socket (readable when a change is pending).
    pub fn new() -> io::Result<(Self, UnixStream)> {
        let (wake_reader, mut wake_writer) = UnixStream::pair()?;
        let (control, mut control_reader) = UnixStream::pair()?;
        for stream in [&wake_reader, &wake_writer, &control, &control_reader] {
            stream.set_nonblocking(true)?;
        }
        // SAFETY: inotify_init1 returns a new descriptor that is owned exactly once.
        let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let inotify = unsafe { OwnedFd::from_raw_fd(raw) };
        let state = Arc::new(Mutex::new(State::default()));
        let worker = state.clone();
        thread::Builder::new().name("decfiles-monitor".into()).spawn(move || {
            let mut watch: Option<i32> = None;
            let mut buffer = vec![0u8; 16 * 1024];
            loop {
                let mut fds = [libc::pollfd { fd: inotify.as_raw_fd(), events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: control_reader.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
                // SAFETY: two valid pollfd entries.
                if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    return;
                }
                if fds[1].revents != 0 {
                    let mut drain = [0u8; 64];
                    while matches!(control_reader.read(&mut drain), Ok(n) if n > 0) {}
                    let wanted = {
                        let mut state = worker.lock().unwrap_or_else(|error| error.into_inner());
                        if state.stopped {
                            return;
                        }
                        state.wanted.take()
                    };
                    if let Some((revision, path)) = wanted {
                        if let Some(old) = watch.take() {
                            // SAFETY: removing our own watch descriptor.
                            unsafe { libc::inotify_rm_watch(inotify.as_raw_fd(), old) };
                        }
                        let added = CString::new(path.as_os_str().as_bytes()).ok().map(|path| unsafe { libc::inotify_add_watch(inotify.as_raw_fd(), path.as_ptr(), MASK) }).filter(|wd| *wd >= 0);
                        watch = added;
                        // Discard events queued for the previous folder.
                        while unsafe { libc::read(inotify.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) } > 0 {}
                        worker.lock().unwrap_or_else(|error| error.into_inner()).watching = added.map(|_| revision);
                    }
                }
                if fds[0].revents != 0 {
                    let mut relevant = false;
                    loop {
                        // SAFETY: bounded read into our buffer.
                        let count = unsafe { libc::read(inotify.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
                        if count <= 0 {
                            break;
                        }
                        let mut offset = 0;
                        while offset + std::mem::size_of::<libc::inotify_event>() <= count as usize {
                            // SAFETY: the kernel writes whole events; read_unaligned avoids alignment assumptions.
                            let event: libc::inotify_event = unsafe { std::ptr::read_unaligned(buffer.as_ptr().add(offset).cast()) };
                            if Some(event.wd) == watch || event.mask & libc::IN_Q_OVERFLOW != 0 {
                                relevant = true;
                            }
                            offset += std::mem::size_of::<libc::inotify_event>() + event.len as usize;
                        }
                    }
                    if relevant {
                        let mut state = worker.lock().unwrap_or_else(|error| error.into_inner());
                        if state.stopped {
                            return;
                        }
                        if let Some(revision) = state.watching {
                            state.changed = Some(revision);
                            drop(state);
                            // Latest-only; a full wake socket is already readable.
                            let _ = wake_writer.write(&[1]);
                        }
                    }
                }
            }
        })?;
        Ok((Self { state, control: Mutex::new(control) }, wake_reader))
    }

    /// Watch `path` (a resolved folder) for `revision`, replacing the previous watch.
    pub fn watch(&self, revision: u64, path: PathBuf) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.stopped {
            return;
        }
        state.wanted = Some((revision, path));
        state.changed = None;
        state.watching = None;
        drop(state);
        let _ = self.control.lock().unwrap_or_else(|error| error.into_inner()).write(&[1]);
    }

    /// The revision whose folder changed since the last call, if any.
    pub fn take_change(&self) -> Option<u64> {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).changed.take()
    }

    pub fn watching(&self) -> Option<u64> {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).watching
    }

    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.stopped = true;
        state.wanted = None;
        drop(state);
        let _ = self.control.lock().unwrap_or_else(|error| error.into_inner()).write(&[1]);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn until(mut check: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if check() {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn reports_changes_only_for_the_current_watch_revision() {
        let base = PathBuf::from("/var/tmp").join(format!("decfiles-monitor-{}", std::process::id()));
        let (first, second) = (base.join("first"), base.join("second"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let (monitor, mut wake) = Monitor::new().unwrap();
        monitor.watch(1, first.clone());
        assert!(until(|| monitor.watching() == Some(1)));
        std::fs::write(first.join("created"), b"x").unwrap();
        assert!(until(|| monitor.take_change() == Some(1)), "create in watched folder");
        let mut byte = [0u8; 8];
        assert!(wake.read(&mut byte).unwrap() > 0, "wake socket readable");
        std::fs::rename(first.join("created"), first.join("renamed")).unwrap();
        assert!(until(|| monitor.take_change() == Some(1)), "rename in watched folder");
        monitor.watch(2, second.clone());
        assert!(until(|| monitor.watching() == Some(2)));
        std::fs::remove_file(first.join("renamed")).unwrap();
        thread::sleep(Duration::from_millis(200));
        assert_eq!(monitor.take_change(), None, "the previous folder is no longer watched");
        std::fs::create_dir(second.join("child")).unwrap();
        assert!(until(|| monitor.take_change() == Some(2)));
        // A missing folder produces no watch and no change reports.
        monitor.watch(3, base.join("missing"));
        thread::sleep(Duration::from_millis(100));
        assert_eq!(monitor.watching(), None);
        monitor.stop();
        std::fs::remove_dir_all(base).unwrap();
    }
}
