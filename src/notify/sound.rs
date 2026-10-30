//! Optional, bounded notification event sounds. No shell or network input is accepted.
use crate::dbus::Value;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_PLAY: Duration = Duration::from_secs(5);
const MIN_GAP: Duration = Duration::from_millis(1000);

#[derive(Debug, PartialEq)]
pub enum Event {
    Name(String),
    File(PathBuf),
}
#[derive(Debug, PartialEq)]
pub struct Request {
    event: Event,
    expires: Instant,
}

/// A valid themed name wins over a file; suppress-sound always wins. The spec
/// defines both hints but leaves their precedence to the server.
pub fn request(args: &[Value], lifetime: Option<Duration>) -> Option<Request> {
    let Value::Dict(hints) = args.get(6)? else { return None };
    let mut name = None;
    let mut file = None;
    for (key, value) in hints {
        match key.as_str()? {
            "suppress-sound" if value.as_bool() == Some(true) => return None,
            "sound-name" => name = value.as_str(),
            "sound-file" => file = value.as_str(),
            _ => {}
        }
    }
    let event =
        if let Some(name) = name.filter(|n| n.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric) && n.len() <= 128 && n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')) {
            Event::Name(name.to_string())
        } else if let Some(file) = file.filter(|f| f.len() <= 4096 && !f.contains('\0') && Path::new(f).is_absolute()) {
            Event::File(PathBuf::from(file))
        } else if name.is_some() || file.is_some() {
            return None;
        } else {
            Event::Name("message-new-instant".into())
        };
    Some(Request { event, expires: Instant::now() + lifetime.unwrap_or(Duration::from_millis(750)).min(Duration::from_millis(750)) })
}

pub fn backend() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|p| p.join("canberra-gtk-play")).find(|p| p.is_absolute() && p.is_file() && p.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0))
}

fn audio_file(path: &Path) -> Option<File> {
    let mut file = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW).open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE || meta.len() < 12 {
        return None;
    }
    let mut head = [0; 12];
    file.read_exact(&mut head).ok()?;
    if !(head.starts_with(b"OggS") || head.starts_with(b"fLaC") || head.starts_with(b"RIFF") && &head[8..] == b"WAVE") {
        return None;
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).ok()?;
    // A descriptor pins the inode, but its owner can still grow/modify it. Copy
    // at most the budget into a sealed snapshot before an external decoder sees it.
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_FILE || bytes.len() < 12 || bytes[..12] != head {
        return None;
    }
    let fd = unsafe { libc::memfd_create(c"decebal-notification-sound".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return None;
    }
    let mut snapshot = unsafe { File::from_raw_fd(fd) };
    snapshot.write_all(&bytes).ok()?;
    snapshot.seek(SeekFrom::Start(0)).ok()?;
    if unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL) } != 0 {
        return None;
    }
    Some(snapshot)
}

/// One worker and at most one pending event. A flood is dropped rather than
/// building an audible backlog; expired pending events are never played.
pub struct Player {
    tx: Option<SyncSender<Request>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Player {
    pub fn new() -> Self {
        Self::with_backend(backend())
    }
    fn with_backend(backend: Option<PathBuf>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let Some(backend) = backend else {
            return Self { tx: None, stop, worker: None };
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut last = None;
            while !stopping.load(Ordering::Relaxed) {
                let request: Request = match rx.recv() {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let now = Instant::now();
                if now >= request.expires || last.is_some_and(|at| now.duration_since(at) < MIN_GAP) {
                    continue;
                }
                last = Some(now);
                play(&backend, request, &stopping);
            }
        });
        Self { tx: Some(tx), stop, worker: Some(worker) }
    }
    pub fn submit(&self, request: Request) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(request);
        }
    }
}
impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn play(backend: &Path, request: Request, stop: &AtomicBool) {
    let mut cmd = Command::new(backend);
    let file = match request.event {
        Event::Name(name) => {
            cmd.arg("--id").arg(name);
            None
        }
        Event::File(path) => {
            let Some(file) = audio_file(&path) else { return };
            cmd.arg("--file").arg("/proc/self/fd/3");
            let fd = file.as_raw_fd();
            // Pass the already validated inode, not a path that can be swapped
            // between validation and the optional player's open.
            unsafe {
                cmd.pre_exec(move || {
                    if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            Some(file)
        }
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
    crate::controlcenter::sys::die_with_parent(&mut cmd);
    if stop.load(Ordering::Relaxed) || Instant::now() >= request.expires {
        return;
    }
    let Ok(mut child) = cmd.spawn() else { return };
    drop(file);
    let end = Instant::now() + MAX_PLAY;
    loop {
        // WNOWAIT leaves the PID reserved until group cleanup and wait(), so a
        // finished player's PID cannot be recycled before its group is killed.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let exited = unsafe { libc::waitid(libc::P_PID, child.id(), &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) == 0 && info.si_pid() != 0 };
        if exited || stop.load(Ordering::Relaxed) || Instant::now() >= end {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hints(pairs: &[(&str, Value)]) -> Vec<Value> {
        let mut args = vec![Value::Str(String::new()); 7];
        args[6] = Value::Dict(pairs.iter().map(|(k, v)| (Value::Str((*k).into()), Value::Variant(Box::new(v.clone())))).collect());
        args
    }
    #[test]
    fn hint_policy_and_deadline() {
        let r = request(&hints(&[]), None).unwrap();
        assert_eq!(r.event, Event::Name("message-new-instant".into()));
        assert!(request(&hints(&[("suppress-sound", Value::Bool(true))]), None).is_none());
        assert_eq!(request(&hints(&[("sound-name", Value::Str("complete".into())), ("sound-file", Value::Str("/tmp/a.wav".into()))]), None).unwrap().event, Event::Name("complete".into()));
        assert!(request(&hints(&[("sound-name", Value::Str("--id=evil;cmd".into()))]), None).is_none());
        assert!(request(&hints(&[("sound-name", Value::Str("--help".into()))]), None).is_none());
        assert!(request(&hints(&[("sound-file", Value::Str("https://example.com/a.wav".into()))]), None).is_none());
        assert!(request(&hints(&[]), Some(Duration::ZERO)).unwrap().expires <= Instant::now());
    }
    #[test]
    fn optional_backend_and_drop_leave_no_worker_or_child() {
        let idle = Player::with_backend(None);
        assert!(idle.tx.is_none() && idle.worker.is_none());
        let dir = std::env::temp_dir().join(format!("notify-sound-drop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let backend = dir.join("canberra-gtk-play");
        let pidfile = dir.join("pid");
        std::fs::write(&backend, format!("#!/bin/sh\necho $$ > '{}'\nexec /usr/bin/sleep 30\n", pidfile.display())).unwrap();
        std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o700)).unwrap();
        let player = Player::with_backend(Some(backend));
        player.submit(Request { event: Event::Name("complete".into()), expires: Instant::now() + Duration::from_secs(1) });
        let end = Instant::now() + Duration::from_secs(3);
        while !pidfile.exists() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid: u32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        let start = Instant::now();
        drop(player);
        assert!(start.elapsed() < Duration::from_secs(1), "shutdown must interrupt playback");
        assert!(!Path::new(&format!("/proc/{pid}")).exists(), "the owned direct child was reaped");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn audio_files_are_regular_bounded_and_have_audio_headers() {
        let dir = std::env::temp_dir().join(format!("notify-sound-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audio");
        std::fs::write(&path, b"RIFF\x04\0\0\0WAVE").unwrap();
        let mut snapshot = audio_file(&path).unwrap();
        std::fs::write(&path, b"different now").unwrap();
        let mut bytes = Vec::new();
        snapshot.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"RIFF\x04\0\0\0WAVE", "the validated bytes must not change with the original inode");
        assert!(snapshot.write_all(b"changed").is_err(), "decoder input is sealed");
        std::fs::write(&path, b"RIFF\x04\0\0\0WAVE").unwrap();
        let link = dir.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(audio_file(&link).is_none());
        File::options().write(true).open(&path).unwrap().set_len(MAX_FILE + 1).unwrap();
        assert!(audio_file(&path).is_none());
        std::fs::write(&path, b"not an audio header").unwrap();
        assert!(audio_file(&path).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
