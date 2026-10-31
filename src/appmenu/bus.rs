//! The menu worker's own bus connection: an explicit address (never an implicit fallback when a
//! caller passed one), bounded buffering, and helpers for the few shapes this module marshals.
//! Framing and value parsing are [`crate::dbus`]'s.
//!
//! Nothing here blocks without a bound. Connecting (every address candidate, the socket connect
//! itself, the EXTERNAL exchange and `Hello`) shares one absolute deadline; each send has one
//! absolute end. Both are cut short by a [`Cancel`] (the worker's stop flag, noticed through its
//! wake socket), and a send that stopped half-way leaves the connection marked broken.

use crate::dbus::{Message, Writer};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `NO_AUTO_START`: calls never activate services from the bus's service directories.
pub const FLAG_NO_AUTO_START: u8 = 0x2;
pub const FLAG_NO_REPLY_EXPECTED: u8 = 0x1;

pub const BUS: &str = "org.freedesktop.DBus";
pub const BUS_PATH: &str = "/org/freedesktop/DBus";

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn timed_out(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, what.to_string())
}

/// The error a [`Cancel`] produces; see [`is_cancelled`].
#[derive(Debug)]
struct Stopped;
impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stopped")
    }
}
impl std::error::Error for Stopped {}

pub fn is_cancelled(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|i| i.is::<Stopped>())
}

/// Ends a bus wait early once `stop` is set. `wake` (a clone of the worker's wake socket) makes the
/// wait notice it without polling; bytes read from it are only wake-ups (the worker re-reads its
/// focus slot and command queue on every turn), and its EOF (the handle dropped) also cancels.
pub struct Cancel {
    wake: Option<UnixStream>,
    stop: Arc<AtomicBool>,
}

impl Cancel {
    pub fn new(wake: Option<UnixStream>, stop: Arc<AtomicBool>) -> Cancel {
        Cancel { wake, stop }
    }

    /// Bounded by deadlines only.
    pub fn never() -> Cancel {
        Cancel { wake: None, stop: Arc::new(AtomicBool::new(false)) }
    }

    fn check(&self) -> io::Result<()> {
        if self.stop.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, Stopped));
        }
        Ok(())
    }

    /// Reads the pending wake bytes; EOF counts as a stop.
    fn drain(&self) -> io::Result<()> {
        let Some(mut w) = self.wake.as_ref() else { return Ok(()) };
        let mut buf = [0u8; 64];
        loop {
            match w.read(&mut buf) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::Interrupted, Stopped)),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Ok(()),
            }
        }
    }

    /// Waits until `fd` reports `events` (or an error/hangup), `end` passes, or a stop. `fd < 0`
    /// waits for the deadline or a stop only.
    fn wait(&self, fd: RawFd, events: libc::c_short, end: Instant) -> io::Result<()> {
        loop {
            self.check()?;
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(timed_out("bus deadline passed"));
            }
            let wake = self.wake.as_ref().map_or(-1, |w| w.as_raw_fd());
            let mut p = [libc::pollfd { fd, events, revents: 0 }, libc::pollfd { fd: wake, events: libc::POLLIN, revents: 0 }];
            // Round up so a sub-millisecond remainder still sleeps instead of spinning.
            let ms = left.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32;
            // SAFETY: `p` is a valid array of two pollfd for the duration of the call.
            let r = unsafe { libc::poll(p.as_mut_ptr(), 2, ms) };
            if r < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if p[1].revents != 0 {
                self.drain()?;
            }
            if fd >= 0 && p[0].revents & (events | libc::POLLERR | libc::POLLHUP) != 0 {
                return Ok(());
            }
            if fd < 0 && r == 0 {
                return Ok(());
            }
        }
    }
}

fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// One unix socket from a D-Bus address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Path(Vec<u8>),
    Abstract(Vec<u8>),
}

/// Unix socket candidates of a D-Bus address string (`unix:path=…;unix:abstract=…`).
pub fn socket_addrs(address: &str) -> Vec<Target> {
    let mut out = Vec::new();
    for cand in address.split(';') {
        let Some(rest) = cand.strip_prefix("unix:") else { continue };
        for kv in rest.split(',') {
            if let Some(p) = kv.strip_prefix("path=") {
                out.push(Target::Path(unescape(p)));
            } else if let Some(p) = kv.strip_prefix("abstract=") {
                out.push(Target::Abstract(unescape(p)));
            }
        }
    }
    out
}

/// Connects without blocking: a full listen backlog (`EAGAIN`) is retried until `end`.
fn connect_target(t: &Target, end: Instant, cancel: &Cancel) -> io::Result<UnixStream> {
    // SAFETY: plain socket(2); the result is checked before use and owned right away.
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh descriptor nothing else owns.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: sockaddr_un is plain old data; all-zero is a valid (empty) value.
    let mut sa: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    sa.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let (bytes, extra): (Vec<u8>, usize) = match t {
        Target::Path(p) if !p.is_empty() && !p.contains(&0) => (p.clone(), 1),
        Target::Abstract(n) => ([&[0u8][..], n].concat(), 0),
        Target::Path(_) => return Err(bad("invalid socket path")),
    };
    if bytes.len() + extra > sa.sun_path.len() {
        return Err(bad("socket address too long"));
    }
    for (d, s) in sa.sun_path.iter_mut().zip(&bytes) {
        *d = *s as libc::c_char;
    }
    let len = (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + extra) as libc::socklen_t;
    loop {
        cancel.check()?;
        // SAFETY: `sa` is a valid sockaddr_un and `len` does not exceed its size.
        let r = unsafe { libc::connect(fd.as_raw_fd(), (&sa as *const libc::sockaddr_un).cast(), len) };
        if r == 0 {
            return Ok(UnixStream::from(fd));
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) => {}
            // Listener backlog full: nothing is pending, so try again shortly.
            Some(libc::EAGAIN) => {
                if Instant::now() >= end {
                    return Err(timed_out("bus listener backlog stayed full"));
                }
                cancel.wait(-1, 0, (Instant::now() + Duration::from_millis(10)).min(end))?;
            }
            Some(libc::EINPROGRESS) => {
                cancel.wait(fd.as_raw_fd(), libc::POLLOUT, end)?;
                let mut err: libc::c_int = 0;
                let mut l = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
                // SAFETY: SO_ERROR writes one c_int into `err`; `l` holds its size.
                unsafe { libc::getsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_ERROR, (&mut err as *mut libc::c_int).cast(), &mut l) };
                if err != 0 {
                    return Err(io::Error::from_raw_os_error(err));
                }
                return Ok(UnixStream::from(fd));
            }
            _ => return Err(e),
        }
    }
}

fn write_until(sock: &mut UnixStream, bytes: &[u8], end: Instant, cancel: &Cancel) -> io::Result<()> {
    let mut off = 0;
    while off < bytes.len() {
        match sock.write(&bytes[off..]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => off += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => cancel.wait(sock.as_raw_fd(), libc::POLLOUT, end)?,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Reads at least one byte (into `out`, at most `max` in total), waiting until `end`.
fn read_until(sock: &mut UnixStream, out: &mut Vec<u8>, max: usize, end: Instant, cancel: &Cancel) -> io::Result<()> {
    let mut tmp = [0u8; 4096];
    loop {
        let room = max.saturating_sub(out.len()).min(tmp.len());
        if room == 0 {
            return Err(bad("bus peer sent too much during the handshake"));
        }
        match sock.read(&mut tmp[..room]) {
            Ok(0) => return Err(bad("bus closed during the handshake")),
            Ok(n) => {
                out.extend_from_slice(&tmp[..n]);
                return Ok(());
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => cancel.wait(sock.as_raw_fd(), libc::POLLIN, end)?,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

pub struct Bus {
    sock: UnixStream,
    inbuf: Vec<u8>,
    serial: u32,
    max_buffered: usize,
    write_timeout: Duration,
    cancel: Cancel,
    /// A write stopped half-way: the stream is no longer framed and must not be used again.
    broken: bool,
    /// Unique name assigned by the bus (`:1.42`).
    pub unique: String,
}

impl Bus {
    /// Connects, authenticates (EXTERNAL) and says `Hello`, all before `end` (one deadline for every
    /// candidate and every step) unless `cancel` stops it first. The socket is non-blocking
    /// throughout; later sends get `write_timeout` each and the same `cancel`.
    pub fn connect(address: &str, end: Instant, cancel: Cancel, max_buffered: usize, write_timeout: Duration) -> io::Result<Bus> {
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no usable unix socket in the bus address");
        for t in socket_addrs(address) {
            match connect_target(&t, end, &cancel) {
                Ok(s) => return Self::handshake(s, end, cancel, max_buffered, write_timeout),
                Err(e) if is_cancelled(&e) || e.kind() == io::ErrorKind::TimedOut => return Err(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn handshake(mut sock: UnixStream, end: Instant, cancel: Cancel, max_buffered: usize, write_timeout: Duration) -> io::Result<Bus> {
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() }.to_string();
        let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
        write_until(&mut sock, format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes(), end, &cancel)?;
        let mut line = Vec::new();
        while !line.windows(2).any(|w| w == b"\r\n") {
            read_until(&mut sock, &mut line, 512, end, &cancel)?;
        }
        if !line.starts_with(b"OK") {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "bus refused authentication"));
        }
        // Nothing may follow the OK line before BEGIN.
        if !line.ends_with(b"\r\n") {
            return Err(bad("unexpected data after the auth reply"));
        }
        write_until(&mut sock, b"BEGIN\r\n", end, &cancel)?;
        let mut bus = Bus { sock, inbuf: Vec::new(), serial: 1, max_buffered, write_timeout, cancel, broken: false, unique: String::new() };
        let serial = 1;
        let hello = Message::call(BUS, BUS_PATH, BUS, "Hello", "", Vec::new()).encode(serial);
        write_until(&mut bus.sock, &hello, end, &bus.cancel)?;
        loop {
            while let Some(m) = bus.next_message()? {
                if m.reply_serial == serial {
                    if m.kind != crate::dbus::RETURN {
                        return Err(bad("Hello refused"));
                    }
                    bus.unique = m.args()?.first().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    return Ok(bus);
                }
            }
            let mut chunk = Vec::new();
            read_until(&mut bus.sock, &mut chunk, 4096, end, &bus.cancel)?;
            bus.push(&chunk)?;
        }
    }

    fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.inbuf.len() + bytes.len() > self.max_buffered {
            return Err(bad("bus peer flooded the connection"));
        }
        self.inbuf.extend_from_slice(bytes);
        Ok(())
    }

    pub fn fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }

    /// Sends a message within one absolute `write_timeout`, unless stopped; returns the serial.
    pub fn send(&mut self, m: &Message) -> io::Result<u32> {
        let end = Instant::now() + self.write_timeout;
        self.send_by(m, end, true)
    }

    /// Sends with its own short budget that a stop does not cut (the goodbye `ReleaseName`).
    pub fn send_final(&mut self, m: &Message, budget: Duration) -> io::Result<u32> {
        self.send_by(m, Instant::now() + budget, false)
    }

    fn send_by(&mut self, m: &Message, end: Instant, cancellable: bool) -> io::Result<u32> {
        if self.broken {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "bus connection broken by an unfinished write"));
        }
        self.serial = self.serial.wrapping_add(1).max(1);
        let bytes = m.encode(self.serial);
        let never;
        let cancel = if cancellable {
            &self.cancel
        } else {
            never = Cancel::never();
            &never
        };
        if let Err(e) = write_until(&mut self.sock, &bytes, end, cancel) {
            self.broken = true;
            return Err(e);
        }
        Ok(self.serial)
    }

    /// Reads what is available. `Ok(false)` means the bus closed the connection.
    pub fn fill(&mut self) -> io::Result<bool> {
        if self.broken {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "bus connection broken by an unfinished write"));
        }
        let mut tmp = [0u8; 16384];
        loop {
            match self.sock.read(&mut tmp) {
                Ok(0) => return Ok(false),
                Ok(n) => self.push(&tmp[..n])?,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(true),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    pub fn next_message(&mut self) -> io::Result<Option<Message>> {
        match Message::decode(&self.inbuf)? {
            Some((m, used)) => {
                self.inbuf.drain(..used);
                Ok(Some(m))
            }
            None => Ok(None),
        }
    }
}

/// A method call that never auto-starts a service.
pub fn call(dest: &str, path: &str, iface: &str, member: &str, sig: &str, body: Vec<u8>) -> Message {
    let mut m = Message::call(dest, path, iface, member, sig, body);
    m.flags |= FLAG_NO_AUTO_START;
    m
}

pub fn strings(v: &[&str]) -> Vec<u8> {
    let mut w = Writer::default();
    for s in v {
        w.string(s);
    }
    w.buf
}

pub fn i32(w: &mut Writer, v: i32) {
    w.u32(v as u32);
}

/// D-Bus object path syntax.
pub fn valid_path(p: &str) -> bool {
    p.starts_with('/') && p.len() <= 255 && (p == "/" || p[1..].split('/').all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')))
}

/// Unique (`:1.42`) or well-known (`org.example.App`) bus name syntax.
pub fn valid_name(n: &str) -> bool {
    if n.is_empty() || n.len() > 255 {
        return false;
    }
    let unique = n.starts_with(':');
    let body = if unique { &n[1..] } else { n };
    let parts: Vec<&str> = body.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') && (unique || !p.as_bytes()[0].is_ascii_digit()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert!(valid_name(":1.42") && valid_name("org.kde.foo-bar") && valid_name(":1.2.3"));
        assert!(!valid_name("") && !valid_name("nodots") && !valid_name("org..x") && !valid_name("org.1x") && !valid_name("a.b/c"));
        assert!(valid_path("/") && valid_path("/MenuBar/1") && valid_path("/com/canonical/menu/4A0000E"));
        assert!(!valid_path("") && !valid_path("MenuBar") && !valid_path("/a//b") && !valid_path("/a-b") && !valid_path("/trailing/"));
    }

    #[test]
    fn addresses_parse_paths_and_abstract_names_only() {
        assert_eq!(socket_addrs("unix:path=/tmp/a%20b").len(), 1);
        assert_eq!(socket_addrs("tcp:host=localhost,port=1;unix:abstract=x").len(), 1);
        assert!(socket_addrs("").is_empty() && socket_addrs("tcp:host=x").is_empty());
    }

    use std::os::unix::net::UnixListener;
    use std::sync::atomic::AtomicU32;

    static N: AtomicU32 = AtomicU32::new(0);

    /// A fresh private directory (removed on drop) for one fake peer socket.
    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new() -> Dir {
            let d = std::env::temp_dir().join(format!("decebal-appmenu-bus-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Dir(d)
        }
        fn sock(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn addr(p: &std::path::Path) -> String {
        format!("unix:path={}", p.display())
    }

    fn end(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    #[derive(Clone, Copy)]
    enum Peer {
        /// Accepts and never says anything.
        Silent,
        /// Sends an auth reply one byte at a time, never finishing the line.
        TrickleAuth,
        /// Completes auth, then trickles a Hello reply that never completes.
        TrickleHello,
        /// Completes auth and Hello, then reads 1 KiB every 20 ms.
        SlowReader,
    }

    /// Runs `peer` for every connection on `path` until the test ends (threads are detached).
    fn serve(path: &std::path::Path, peer: Peer) {
        let l = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            for c in l.incoming() {
                let Ok(mut c) = c else { return };
                std::thread::spawn(move || {
                    let _ = c.set_read_timeout(Some(Duration::from_secs(20)));
                    let read_line = |c: &mut UnixStream| {
                        let mut line = Vec::new();
                        let mut b = [0u8; 1];
                        while !line.ends_with(b"\r\n") {
                            if c.read(&mut b).ok()? == 0 {
                                return None;
                            }
                            line.push(b[0]);
                        }
                        Some(line)
                    };
                    match peer {
                        Peer::Silent => std::thread::sleep(Duration::from_secs(20)),
                        Peer::TrickleAuth => {
                            read_line(&mut c);
                            for b in b"OK 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".iter().cycle().take(400) {
                                if c.write_all(&[*b]).is_err() {
                                    return;
                                }
                                std::thread::sleep(Duration::from_millis(20));
                            }
                        }
                        Peer::TrickleHello | Peer::SlowReader => {
                            read_line(&mut c);
                            let _ = c.write_all(b"OK 0123456789abcdef0123456789abcdef\r\n");
                            read_line(&mut c);
                            let mut buf = Vec::new();
                            let mut tmp = [0u8; 4096];
                            let hello = loop {
                                if let Ok(Some((m, _))) = Message::decode(&buf) {
                                    break m;
                                }
                                match c.read(&mut tmp) {
                                    Ok(0) | Err(_) => return,
                                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                                }
                            };
                            let mut w = Writer::default();
                            w.string(":1.99");
                            let reply = Message::reply_to(&hello, "s", w.buf).encode(1);
                            if let Peer::TrickleHello = peer {
                                for b in &reply[..reply.len() - 1] {
                                    if c.write_all(&[*b]).is_err() {
                                        return;
                                    }
                                    std::thread::sleep(Duration::from_millis(20));
                                }
                                std::thread::sleep(Duration::from_secs(20));
                            } else {
                                let _ = c.write_all(&reply);
                                let mut sink = [0u8; 1024];
                                loop {
                                    std::thread::sleep(Duration::from_millis(20));
                                    if matches!(c.read(&mut sink), Ok(0) | Err(_)) {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                });
            }
        });
    }

    /// A listener with a zero backlog that never accepts, filled until connects get `EAGAIN`.
    pub(crate) fn full_backlog(path: &std::path::Path) -> (OwnedFd, Vec<OwnedFd>) {
        let mk = || {
            // SAFETY: plain socket(2), checked and owned at once.
            let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0) };
            assert!(fd >= 0);
            // SAFETY: fresh descriptor.
            unsafe { OwnedFd::from_raw_fd(fd) }
        };
        // SAFETY: zeroed sockaddr_un is valid; the path fits (asserted).
        let mut sa: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        sa.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let p = path.as_os_str().as_encoded_bytes();
        assert!(p.len() < sa.sun_path.len());
        for (d, s) in sa.sun_path.iter_mut().zip(p) {
            *d = *s as libc::c_char;
        }
        let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
        let l = mk();
        // SAFETY: valid sockaddr and descriptor.
        unsafe {
            assert_eq!(libc::bind(l.as_raw_fd(), (&sa as *const libc::sockaddr_un).cast(), len), 0);
            assert_eq!(libc::listen(l.as_raw_fd(), 0), 0);
        }
        let mut pending = Vec::new();
        for _ in 0..16 {
            let c = mk();
            // SAFETY: valid sockaddr and descriptor.
            let r = unsafe { libc::connect(c.as_raw_fd(), (&sa as *const libc::sockaddr_un).cast(), len) };
            if r != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EAGAIN) {
                return (l, pending);
            }
            pending.push(c);
        }
        panic!("the zero backlog never filled");
    }

    /// A cancel whose stop flag `stopper` sets after `after`, waking the wait through the socket.
    fn stop_after(after: Duration) -> Cancel {
        let (a, b) = UnixStream::pair().unwrap();
        a.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            s.store(true, Ordering::SeqCst);
            let _ = (&b).write(&[1]);
            std::thread::sleep(Duration::from_secs(30));
            drop(b);
        });
        Cancel::new(Some(a), stop)
    }

    #[test]
    fn missing_socket_is_an_error_not_a_hang() {
        let t = Instant::now();
        assert!(Bus::connect("unix:path=/nonexistent/decebal-appmenu/bus", end(200), Cancel::never(), 1 << 20, Duration::from_secs(1)).is_err());
        assert!(Bus::connect("", end(200), Cancel::never(), 1 << 20, Duration::from_secs(1)).is_err());
        assert!(t.elapsed() < Duration::from_millis(150));
    }

    #[test]
    fn one_deadline_covers_every_candidate_and_the_whole_handshake() {
        let d = Dir::new();
        let (a, b) = (d.sock("a"), d.sock("b"));
        serve(&a, Peer::Silent);
        serve(&b, Peer::Silent);
        let t = Instant::now();
        let e = Bus::connect(&format!("{};{}", addr(&a), addr(&b)), end(300), Cancel::never(), 1 << 20, Duration::from_secs(1)).err().unwrap();
        let took = t.elapsed();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut, "{e}");
        assert!(took >= Duration::from_millis(290) && took < Duration::from_millis(600), "{took:?}: not one budget per candidate");
    }

    #[test]
    fn a_trickling_auth_reply_cannot_extend_the_deadline() {
        let d = Dir::new();
        let p = d.sock("bus");
        serve(&p, Peer::TrickleAuth);
        let t = Instant::now();
        let e = Bus::connect(&addr(&p), end(300), Cancel::never(), 1 << 20, Duration::from_secs(1)).err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut, "{e}");
        assert!(t.elapsed() < Duration::from_millis(600), "{:?} (each byte used to restart the timeout)", t.elapsed());
    }

    #[test]
    fn a_trickling_hello_reply_cannot_extend_the_deadline() {
        let d = Dir::new();
        let p = d.sock("bus");
        serve(&p, Peer::TrickleHello);
        let t = Instant::now();
        let e = Bus::connect(&addr(&p), end(300), Cancel::never(), 1 << 20, Duration::from_secs(1)).err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut, "{e}");
        assert!(t.elapsed() < Duration::from_millis(600), "{:?}", t.elapsed());
    }

    #[test]
    fn a_full_listen_backlog_is_retried_without_blocking_until_the_deadline() {
        let d = Dir::new();
        let p = d.sock("bus");
        let _held = full_backlog(&p);
        let t = Instant::now();
        let e = Bus::connect(&addr(&p), end(300), Cancel::never(), 1 << 20, Duration::from_secs(1)).err().unwrap();
        let took = t.elapsed();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut, "{e}");
        assert!(took >= Duration::from_millis(290) && took < Duration::from_millis(600), "{took:?}");
    }

    #[test]
    fn a_stop_cuts_connect_short_at_every_step() {
        let d = Dir::new();
        let (silent, trickle, hello, backlog) = (d.sock("silent"), d.sock("trickle"), d.sock("hello"), d.sock("backlog"));
        serve(&silent, Peer::Silent);
        serve(&trickle, Peer::TrickleAuth);
        serve(&hello, Peer::TrickleHello);
        let _held = full_backlog(&backlog);
        for p in [&silent, &trickle, &hello, &backlog] {
            let t = Instant::now();
            let e = Bus::connect(&addr(p), end(30_000), stop_after(Duration::from_millis(150)), 1 << 20, Duration::from_secs(1)).err().unwrap();
            let took = t.elapsed();
            assert!(is_cancelled(&e), "{}: {e}", p.display());
            assert!(took >= Duration::from_millis(140) && took < Duration::from_millis(500), "{}: {took:?}", p.display());
        }
    }

    #[test]
    fn a_send_has_one_absolute_end_and_breaks_the_connection_when_cut() {
        let d = Dir::new();
        let p = d.sock("bus");
        serve(&p, Peer::SlowReader);
        let mut bus = Bus::connect(&addr(&p), end(2000), Cancel::never(), 1 << 20, Duration::from_millis(300)).unwrap();
        assert_eq!(bus.unique, ":1.99");
        // 4 MiB against a peer draining ~50 KiB/s: each WouldBlock used to restart a 2 s wait.
        let big = Message::call(BUS, BUS_PATH, BUS, "Ping", "ay", {
            let mut w = Writer::default();
            w.array(1, |w| w.buf.extend(std::iter::repeat_n(0u8, 4 << 20)));
            w.buf
        });
        let t = Instant::now();
        let e = bus.send(&big).err().unwrap();
        let took = t.elapsed();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut, "{e}");
        assert!(took >= Duration::from_millis(290) && took < Duration::from_millis(600), "{took:?}");
        assert_eq!(bus.send(&Message::call(BUS, BUS_PATH, BUS, "Ping", "", vec![])).err().unwrap().kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(bus.fill().err().unwrap().kind(), io::ErrorKind::BrokenPipe, "the half-written stream is never reused");

        // The same send is cut short by a stop, and the goodbye send keeps its own small budget.
        let mut bus = Bus::connect(&addr(&p), end(2000), stop_after(Duration::from_millis(150)), 1 << 20, Duration::from_secs(30)).unwrap();
        let t = Instant::now();
        let e = bus.send(&big).err().unwrap();
        assert!(is_cancelled(&e), "{e}");
        assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
        let mut bus = Bus::connect(&addr(&p), end(2000), Cancel::never(), 1 << 20, Duration::from_secs(30)).unwrap();
        let t = Instant::now();
        assert!(bus.send_final(&big, Duration::from_millis(100)).is_err());
        assert!(t.elapsed() < Duration::from_millis(400), "{:?}", t.elapsed());
    }
}
