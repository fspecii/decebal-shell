//! Bounded, data-only activation and pane requests to the running Settings model.
use super::{appearance, panes::PANES};
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::{
    fs::{MetadataExt, PermissionsExt},
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, Receiver},
    Arc, Mutex,
};
use std::time::Duration;

// Serialize same-process listener replacement and inode-checked cleanup. Across
// processes the caller's Settings instance lock remains the ownership boundary.
static ENDPOINT_HANDOFF: Mutex<()> = Mutex::new(());

pub fn resolve(name: &str) -> Option<String> {
    let name = match name {
        "help" | "getting-started" => "welcome",
        n => n,
    };
    (appearance::resolve(name).is_some() || PANES.iter().any(|p| p.id == name)).then(|| name.to_owned())
}

pub fn endpoint() -> io::Result<PathBuf> {
    // Hash display names into a short Unix-socket path, without using them as filenames.
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    std::env::var_os("WAYLAND_DISPLAY").hash(&mut hash);
    std::env::var_os("DISPLAY").hash(&mut hash);
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    endpoint_in(&base, hash.finish(), Path::new("/tmp"))
}

fn endpoint_in(base: &Path, display: u64, fallback: &Path) -> io::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: geteuid has no arguments or memory accesses.
    let uid = unsafe { libc::geteuid() };
    let mut dir = base.join(format!("decsettings-{uid}"));
    let mut file = format!("{display:x}.sock");
    // Linux sockaddr_un has 108 bytes including the terminating NUL. Hashing
    // the display alone cannot shorten an unusually long runtime directory.
    if dir.join(&file).as_os_str().as_bytes().len() > 107 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        base.hash(&mut hash);
        dir = fallback.join(format!("decsettings-{uid}"));
        file = format!("{:x}-{display:x}.sock", hash.finish());
    }
    match fs::create_dir(&dir) {
        Ok(()) => fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let md = fs::symlink_metadata(&dir)?;
    if !md.is_dir() || md.uid() != uid || md.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Settings runtime directory must be owned by this user with mode 0700"));
    }
    let path = dir.join(file);
    if path.as_os_str().as_bytes().len() > 107 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Settings socket path exceeds the Unix socket limit"));
    }
    Ok(path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Activate,
    Pane(String),
}

pub fn activate(path: &Path, config: Option<&Path>) -> io::Result<()> {
    send_request(path, Request::Activate, config)
}

pub fn send(path: &Path, pane: &str) -> io::Result<()> {
    send_request(path, Request::Pane(pane.into()), None)
}

pub fn send_for_config(path: &Path, pane: &str, config: &Path) -> io::Result<()> {
    send_request(path, Request::Pane(pane.into()), Some(config))
}

fn absolute(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| if path.is_absolute() { path.to_owned() } else { std::env::current_dir().unwrap_or_default().join(path) })
}

fn send_request(path: &Path, request: Request, config: Option<&Path>) -> io::Result<()> {
    if matches!(&request, Request::Pane(pane) if pane.len() > 128 || resolve(pane).is_none()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown Settings pane"));
    }
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(Duration::from_millis(250)))?;
    let request = match request {
        Request::Activate => serde_json::json!({"activate": true, "config": config.map(absolute)}),
        Request::Pane(pane) => serde_json::json!({"pane": pane, "config": config.map(absolute)}),
    }
    .to_string();
    if request.len() > 2048 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Settings request too large"));
    }
    writeln!(stream, "{request}")?;
    let mut ack = [0; 1];
    stream.read_exact(&mut ack)?;
    if ack != *b"K" {
        return Err(io::Error::other("Settings refused request (the requested config may differ from the running window)"));
    }
    Ok(())
}

pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
    config: Option<PathBuf>,
}
impl Listener {
    /// Caller must hold the Settings instance lock before removing a stale endpoint.
    pub fn bind(path: PathBuf) -> io::Result<Self> {
        let _handoff = ENDPOINT_HANDOFF.lock().unwrap_or_else(|e| e.into_inner());
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let md = fs::symlink_metadata(&path)?;
        Ok(Self { listener, identity: (md.dev(), md.ino()), path, config: None })
    }
    pub fn for_config(mut self, config: &Path) -> Self {
        self.config = Some(absolute(config));
        self
    }

    pub fn start(self, wake: Arc<dyn Fn() + Send + Sync>) -> Requests {
        let (tx, rx) = sync_channel(16);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match self.listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(150)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(150)));
                        // Read a single bounded line; never interpret shell text or commands.
                        let mut bytes = Vec::new();
                        let deadline = std::time::Instant::now() + Duration::from_millis(150);
                        for _ in 0..2050 {
                            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                            if remaining.is_zero() {
                                break;
                            }
                            let _ = stream.set_read_timeout(Some(remaining));
                            let mut byte = [0];
                            if stream.read_exact(&mut byte).is_err() {
                                break;
                            }
                            if byte[0] == b'\n' {
                                if let Ok(request) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                    let matches_config = request.get("config").filter(|c| !c.is_null()).map(|c| c.as_str().map(PathBuf::from).as_ref() == self.config.as_ref()).unwrap_or(true);
                                    if matches_config {
                                        let action = match (request.get("activate"), request.get("pane")) {
                                            (Some(serde_json::Value::Bool(true)), None) => Some(Request::Activate),
                                            (None, Some(pane)) => pane.as_str().filter(|p| p.len() <= 128).and_then(resolve).map(Request::Pane),
                                            _ => None,
                                        };
                                        if let Some(action) = action {
                                            if tx.try_send(action).is_ok() {
                                                wake();
                                                let _ = stream.write_all(b"K");
                                            }
                                        }
                                    } else {
                                        let _ = stream.write_all(b"E");
                                    }
                                }
                                break;
                            }
                            bytes.push(byte[0]);
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        let mut descriptor = libc::pollfd { fd: self.listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                        // SAFETY: one valid pollfd backed by the owned listener; bounded wait
                        // lets shutdown complete without a busy idle timer.
                        unsafe {
                            libc::poll(&mut descriptor, 1, 500);
                        }
                    }
                    Err(_) => break,
                }
            }
            let _handoff = ENDPOINT_HANDOFF.lock().unwrap_or_else(|e| e.into_inner());
            if fs::symlink_metadata(&self.path).is_ok_and(|md| (md.dev(), md.ino()) == self.identity) {
                let _ = fs::remove_file(&self.path);
            }
        });
        Requests { rx, stop }
    }
}
pub struct Requests {
    pub rx: Receiver<Request>,
    stop: Arc<AtomicBool>,
}
impl Drop for Requests {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_runtime_paths_bind_and_keep_sessions_separate() {
        use std::os::unix::ffi::OsStrExt;
        let root = std::env::temp_dir().join(format!("settings-long-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let a = root.join("runtime".repeat(20));
        let b = root.join("other".repeat(30));
        let fallback = root.join("short");
        fs::create_dir_all(&fallback).unwrap();
        let path = endpoint_in(&a, 42, &fallback).unwrap();
        assert!(path.as_os_str().as_bytes().len() <= 107);
        assert_ne!(path, endpoint_in(&b, 42, &fallback).unwrap());
        assert_ne!(path, endpoint_in(&a, 43, &fallback).unwrap());
        assert_eq!(path, endpoint_in(&a, 42, &fallback).unwrap());
        let unicode = endpoint_in(&root.join("é".repeat(42)), 42, &fallback).unwrap();
        assert!(unicode.as_os_str().as_bytes().len() <= 107, "the socket budget counts bytes, not characters");
        assert_eq!(fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o777, 0o700);
        let listener = Listener::bind(path.clone()).unwrap().for_config(&a.join("config.toml"));
        let requests = listener.start(Arc::new(|| {}));
        send_for_config(&path, "widgets", &a.join("config.toml")).unwrap();
        assert_eq!(requests.rx.recv_timeout(Duration::from_secs(1)).unwrap(), Request::Pane("widgets".into()));
        assert!(send_for_config(&path, "widgets", &b.join("config.toml")).is_err());
        drop(requests);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn short_runtime_paths_keep_existing_endpoints_and_reject_unsafe_directories() {
        let root = std::env::temp_dir().join(format!("settings-endpoint-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = endpoint_in(&root, 42, &root).unwrap();
        assert_eq!(path.file_name().unwrap(), "2a.sock");
        assert!(path.starts_with(&root));
        fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(endpoint_in(&root, 42, &root).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_listener_cannot_remove_replacement_endpoint() {
        let dir = std::env::temp_dir().join(format!("settings-navigation-handoff-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("request.sock");
        let old = Listener::bind(path.clone()).unwrap().start(Arc::new(|| {}));
        drop(old);
        let new = Listener::bind(path.clone()).unwrap().start(Arc::new(|| {}));
        std::thread::sleep(Duration::from_millis(550));
        send(&path, "welcome").unwrap();
        assert_eq!(new.rx.recv_timeout(Duration::from_millis(500)).unwrap(), Request::Pane("welcome".into()));
        drop(new);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn private_endpoint_accepts_only_bounded_valid_panes_and_wakes() {
        let dir = std::env::temp_dir().join(format!("settings-navigation-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("request.sock");
        fs::write(&path, "stale").unwrap();
        let config = dir.join("config.toml");
        let listener = Listener::bind(path.clone()).unwrap().for_config(&config);
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = wakes.clone();
        let requests = listener.start(Arc::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        }));
        send(&path, "getting-started").unwrap();
        assert_eq!(requests.rx.recv_timeout(Duration::from_secs(1)).unwrap(), Request::Pane("welcome".into()));
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        assert!(send_for_config(&path, "welcome", &dir.join("other.toml")).is_err());
        assert!(requests.rx.try_recv().is_err());
        send_for_config(&path, "welcome", &config).unwrap();
        assert_eq!(requests.rx.recv_timeout(Duration::from_secs(1)).unwrap(), Request::Pane("welcome".into()));
        activate(&path, Some(&config)).unwrap();
        assert_eq!(requests.rx.recv_timeout(Duration::from_secs(1)).unwrap(), Request::Activate);
        assert!(activate(&path, Some(&dir.join("other.toml"))).is_err());
        assert!(requests.rx.try_recv().is_err());
        assert!(send(&path, "unknown").is_err());
        assert!(send(&path, &"x".repeat(1000)).is_err());
        let mut invalid = UnixStream::connect(&path).unwrap();
        invalid.write_all(&[b'x'; 140]).unwrap();
        drop(invalid);
        send(&path, "icons").unwrap();
        assert_eq!(requests.rx.recv_timeout(Duration::from_secs(1)).unwrap(), Request::Pane("icons".into()));
        drop(requests);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!path.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
