//! The menu worker's own bus connection: an explicit address (never an implicit fallback when a
//! caller passed one), bounded buffering, and helpers for the few shapes this module marshals.
//! Framing and value parsing are [`crate::dbus`]'s.

use crate::dbus::{Message, Writer};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::time::{Duration, Instant};

/// `NO_AUTO_START`: calls never activate services from the bus's service directories.
pub const FLAG_NO_AUTO_START: u8 = 0x2;
pub const FLAG_NO_REPLY_EXPECTED: u8 = 0x1;

pub const BUS: &str = "org.freedesktop.DBus";
pub const BUS_PATH: &str = "/org/freedesktop/DBus";

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn unescape(s: &str) -> String {
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
    String::from_utf8_lossy(&out).into_owned()
}

/// Unix socket candidates of a D-Bus address string (`unix:path=…;unix:abstract=…`).
pub fn socket_addrs(address: &str) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for cand in address.split(';') {
        let Some(rest) = cand.strip_prefix("unix:") else { continue };
        for kv in rest.split(',') {
            if let Some(p) = kv.strip_prefix("path=") {
                if let Ok(a) = SocketAddr::from_pathname(unescape(p)) {
                    out.push(a);
                }
            } else if let Some(p) = kv.strip_prefix("abstract=") {
                if let Ok(a) = SocketAddr::from_abstract_name(unescape(p).as_bytes()) {
                    out.push(a);
                }
            }
        }
    }
    out
}

pub struct Bus {
    sock: UnixStream,
    inbuf: Vec<u8>,
    serial: u32,
    max_buffered: usize,
    /// Unique name assigned by the bus (`:1.42`).
    pub unique: String,
}

impl Bus {
    /// Connects, authenticates (EXTERNAL) and says `Hello`, spending at most `timeout`.
    /// The socket is non-blocking afterwards.
    pub fn connect(address: &str, timeout: Duration, max_buffered: usize) -> io::Result<Bus> {
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no usable unix socket in the bus address");
        for addr in socket_addrs(address) {
            match UnixStream::connect_addr(&addr) {
                Ok(s) => return Self::handshake(s, timeout, max_buffered),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn handshake(mut sock: UnixStream, timeout: Duration, max_buffered: usize) -> io::Result<Bus> {
        let deadline = Instant::now() + timeout;
        let left = || deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        sock.set_read_timeout(Some(left()))?;
        sock.set_write_timeout(Some(left()))?;
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() }.to_string();
        let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
        sock.write_all(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while !line.ends_with(b"\r\n") {
            sock.set_read_timeout(Some(left()))?;
            if sock.read(&mut byte)? == 0 {
                return Err(bad("bus closed during authentication"));
            }
            line.push(byte[0]);
            if line.len() > 512 {
                return Err(bad("oversized auth reply"));
            }
        }
        if !line.starts_with(b"OK") {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "bus refused authentication"));
        }
        sock.write_all(b"BEGIN\r\n")?;
        let mut bus = Bus { sock, inbuf: Vec::new(), serial: 0, max_buffered, unique: String::new() };
        let serial = bus.send(&Message::call(BUS, BUS_PATH, BUS, "Hello", "", Vec::new()))?;
        loop {
            while let Some(m) = bus.next_message()? {
                if m.reply_serial == serial {
                    if m.kind != crate::dbus::RETURN {
                        return Err(bad("Hello refused"));
                    }
                    bus.unique = m.args()?.first().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    bus.sock.set_read_timeout(None)?;
                    bus.sock.set_write_timeout(None)?;
                    bus.sock.set_nonblocking(true)?;
                    return Ok(bus);
                }
            }
            bus.sock.set_read_timeout(Some(left()))?;
            let mut tmp = [0u8; 4096];
            let n = bus.sock.read(&mut tmp)?;
            if n == 0 {
                return Err(bad("bus closed before Hello"));
            }
            bus.push(&tmp[..n])?;
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

    /// Sends a message; returns the serial used. Writes are bounded by a 2 s wait for buffer space.
    pub fn send(&mut self, m: &Message) -> io::Result<u32> {
        self.serial = self.serial.wrapping_add(1).max(1);
        let bytes = m.encode(self.serial);
        let mut off = 0;
        while off < bytes.len() {
            match self.sock.write(&bytes[off..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => off += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if !crate::backend::poll_writable(self.fd(), Duration::from_secs(2))? {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "bus not accepting writes"));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(self.serial)
    }

    /// Reads what is available. `Ok(false)` means the bus closed the connection.
    pub fn fill(&mut self) -> io::Result<bool> {
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
mod tests {
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

    #[test]
    fn missing_socket_is_an_error_not_a_hang() {
        let t = std::time::Instant::now();
        assert!(Bus::connect("unix:path=/nonexistent/decebal-appmenu/bus", Duration::from_millis(200), 1 << 20).is_err());
        assert!(Bus::connect("", Duration::from_millis(200), 1 << 20).is_err());
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn handshake_times_out_against_a_silent_peer() {
        let dir = std::env::temp_dir().join(format!("decebal-appmenu-silent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bus");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let t = std::time::Instant::now();
        let r = Bus::connect(&format!("unix:path={}", path.display()), Duration::from_millis(300), 1 << 20);
        assert!(r.is_err());
        assert!(t.elapsed() < Duration::from_secs(2), "bounded by the connect timeout");
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
