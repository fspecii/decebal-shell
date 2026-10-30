//! Private, bounded history I/O. Directory-relative descriptors prevent following
//! swapped/symlinked history files; no error includes notification content.
use super::{HistoryItem, MAX_HISTORY};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd, RawFd},
    os::unix::fs::MetadataExt,
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};
const LIMIT: usize = 1 << 20;
static NEXT: AtomicU64 = AtomicU64::new(1);
pub(super) type Result<T> = std::result::Result<T, String>;
fn error(context: &str, e: std::io::Error) -> String {
    format!("{context} ({})", e.kind())
}
fn c(s: &std::ffi::OsStr) -> Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(s.as_bytes()).map_err(|_| "History path is invalid".into())
}
fn open_at(fd: RawFd, name: &CString, flags: i32, mode: u32) -> std::io::Result<File> {
    let n = unsafe { libc::openat(fd, name.as_ptr(), flags | libc::O_CLOEXEC | libc::O_NOFOLLOW, mode as libc::mode_t) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(n) })
    }
}
fn private_file(f: &File) -> Result<()> {
    let m = f.metadata().map_err(|e| error("Cannot inspect history file", e))?;
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.nlink() != 1 {
        return Err("History file must be a regular private file owned by you".into());
    }
    if unsafe { libc::fchmod(f.as_raw_fd(), (m.mode() & 0o600) as libc::mode_t) } != 0 {
        return Err(error("Cannot protect history file", std::io::Error::last_os_error()));
    }
    Ok(())
}
pub(super) struct Store {
    dir: File,
    name: CString,
    _lock: File,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err("History path must be absolute".into());
        }
        let parent = path.parent().ok_or("History path has no directory")?;
        let mut dir = File::open("/").map_err(|e| error("Cannot open history root", e))?;
        for part in parent.components() {
            let Component::Normal(part) = part else {
                if part == Component::RootDir {
                    continue;
                }
                return Err("History path must not traverse directories".into());
            };
            let name = c(part)?;
            let next = match open_at(dir.as_raw_fd(), &name, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                        let e = std::io::Error::last_os_error();
                        if e.kind() != std::io::ErrorKind::AlreadyExists {
                            return Err(error("Cannot create history directory", e));
                        }
                    } else {
                        dir.sync_all().map_err(|e| error("Cannot sync new history directory", e))?;
                    }
                    open_at(dir.as_raw_fd(), &name, libc::O_RDONLY | libc::O_DIRECTORY, 0).map_err(|e| error("Cannot open history directory safely", e))?
                }
                Err(e) => return Err(error("Cannot open history directory safely", e)),
            };
            dir = next;
        }
        let metadata = dir.metadata().map_err(|e| error("Cannot inspect history directory", e))?;
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("History directory must be owned by you".into());
        }
        if unsafe { libc::fchmod(dir.as_raw_fd(), (metadata.mode() & 0o700) as libc::mode_t) } != 0 {
            return Err(error("Cannot protect history directory", std::io::Error::last_os_error()));
        }
        let lock_name = CString::new("notification-history.lock").unwrap();
        let lock = open_at(dir.as_raw_fd(), &lock_name, libc::O_RDWR | libc::O_CREAT | libc::O_NONBLOCK, 0o600).map_err(|e| error("Cannot open history lock safely", e))?;
        private_file(&lock)?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(error("Cannot lock history", std::io::Error::last_os_error()));
        }
        Ok(Self { dir, name: c(path.file_name().ok_or("History filename is missing")?)?, _lock: lock })
    }
    pub fn read(&self) -> Result<Vec<HistoryItem>> {
        let mut f = match open_at(self.dir.as_raw_fd(), &self.name, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(error("Cannot read history safely; existing file preserved", e)),
        };
        private_file(&f)?;
        if f.metadata().map_err(|e| error("Cannot inspect history size", e))?.len() > LIMIT as u64 {
            return Err("History exceeds the 1 MiB limit; existing file preserved".into());
        }
        let mut data = Vec::new();
        Read::by_ref(&mut f).take(LIMIT as u64 + 1).read_to_end(&mut data).map_err(|e| error("Cannot read history; existing file preserved", e))?;
        if data.len() > LIMIT {
            return Err("History exceeds the 1 MiB limit; existing file preserved".into());
        }
        let items: Vec<HistoryItem> = serde_json::from_slice(&data).map_err(|_| "History is not valid JSON; existing file preserved".to_string())?;
        if items.len() > MAX_HISTORY {
            return Err("History contains too many records; existing file preserved".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        if items.iter().any(|i| !ids.insert(i.id)) {
            return Err("History contains duplicate IDs; existing file preserved".into());
        }
        Ok(items)
    }
    pub fn write(&self, items: &[HistoryItem]) -> Result<()> {
        let data = serde_json::to_vec(items).map_err(|_| "Cannot encode notification history".to_string())?;
        if data.len() > LIMIT {
            return Err("History exceeds the 1 MiB limit; changes remain in memory".into());
        }
        // Validate again under the same advisory lock before replacing an existing file.
        self.read()?;
        // Respect an explicitly read-only existing file instead of using rename
        // to bypass its owner's write permission.
        match open_at(self.dir.as_raw_fd(), &self.name, libc::O_WRONLY | libc::O_NONBLOCK, 0) {
            Ok(file) => private_file(&file)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(error("Cannot update read-only history; existing file preserved", e)),
        }
        let name = CString::new(format!(".notification-history-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))).unwrap();
        let result = (|| {
            let mut file = open_at(self.dir.as_raw_fd(), &name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600).map_err(|e| error("Cannot create history temporary file", e))?;
            file.write_all(&data).map_err(|e| error("Cannot write notification history", e))?;
            file.sync_all().map_err(|e| error("Cannot sync notification history", e))?;
            if unsafe { libc::renameat(self.dir.as_raw_fd(), name.as_ptr(), self.dir.as_raw_fd(), self.name.as_ptr()) } != 0 {
                return Err(error("Cannot replace notification history", std::io::Error::last_os_error()));
            }
            self.dir.sync_all().map_err(|e| error("Cannot sync history directory", e))
        })();
        if result.is_err() {
            unsafe { libc::unlinkat(self.dir.as_raw_fd(), name.as_ptr(), 0) };
        }
        result
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // flock belongs to the open-file description. A concurrent helper spawn can inherit
        // it between fork and exec even with CLOEXEC, so closing our descriptor alone can
        // temporarily extend the lock. Explicitly release it when this transaction ends.
        let _ = unsafe { libc::flock(self._lock.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transaction_unlocks_even_while_an_inherited_description_remains_open() {
        let directory = std::env::temp_dir().join(format!("decebal-history-lock-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let path = directory.join("notification-history.json");
        let store = Store::open(&path).unwrap();
        // dup and fork share the same flock lifetime; a duplicate deterministically
        // models a child that has not reached exec/closed its inherited descriptor.
        let inherited = store._lock.try_clone().unwrap();
        drop(store);
        let next = Store::open(&path).expect("finished transaction must release its lock before helper exec");
        drop(inherited);
        drop(next);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
