//! A minimal D-Bus wire client: just enough to own a name on the session bus, answer method calls and
//! emit signals (used by `decnotify`), without libdbus or an async runtime.
//!
//! Scope: little/big-endian parsing of every basic type plus arrays, structs, dict entries and variants;
//! little-endian marshalling; EXTERNAL authentication over a unix socket. No fd passing.

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::time::{Duration, Instant};

/// Messages larger than this are refused (the protocol allows 128 MiB; notifications never need that).
const MAX_MESSAGE: usize = 16 << 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    U8(u8),
    Bool(bool),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F64(f64),
    Str(String),
    Path(String),
    Sig(String),
    /// `ay`, kept as raw bytes (image data).
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Struct(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    Variant(Box<Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Path(s) | Value::Sig(s) => Some(s),
            Value::Variant(v) => v.as_str(),
            _ => None,
        }
    }
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::U32(v) => Some(*v),
            Value::Variant(v) => v.as_u32(),
            _ => None,
        }
    }
    /// Any integer type as i64 (hints arrive as byte, int or uint depending on the client).
    pub fn as_int(&self) -> Option<i64> {
        Some(match self {
            Value::U8(v) => i64::from(*v),
            Value::I16(v) => i64::from(*v),
            Value::U16(v) => i64::from(*v),
            Value::I32(v) => i64::from(*v),
            Value::U32(v) => i64::from(*v),
            Value::I64(v) => *v,
            Value::U64(v) => i64::try_from(*v).ok()?,
            Value::Variant(v) => return v.as_int(),
            _ => return None,
        })
    }
    /// Any number as f64 (a player may send a volume as an integer).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(v) => Some(*v),
            Value::Variant(v) => v.as_f64(),
            other => other.as_int().map(|i| i as f64),
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Variant(v) => v.as_bool(),
            _ => None,
        }
    }
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn align_of(c: u8) -> usize {
    match c {
        b'y' | b'g' | b'v' => 1,
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b's' | b'o' | b'a' => 4,
        _ => 8, // x t d ( {
    }
}

/// End index (exclusive) of the single complete type starting at `i`.
fn type_end(sig: &[u8], i: usize) -> io::Result<usize> {
    let c = *sig.get(i).ok_or_else(|| bad("truncated signature"))?;
    match c {
        b'a' => type_end(sig, i + 1),
        b'(' | b'{' => {
            let close = if c == b'(' { b')' } else { b'}' };
            let mut j = i + 1;
            while *sig.get(j).ok_or_else(|| bad("unterminated container"))? != close {
                j = type_end(sig, j)?;
            }
            Ok(j + 1)
        }
        _ => Ok(i + 1),
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pub pos: usize,
    le: bool,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8], le: bool) -> Self {
        Self { buf, pos: 0, le }
    }

    fn align(&mut self, n: usize) -> io::Result<()> {
        self.pos = self.pos.div_ceil(n) * n;
        if self.pos > self.buf.len() {
            return Err(bad("alignment past end"));
        }
        Ok(())
    }

    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.buf.len()).ok_or_else(|| bad("truncated message"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn num<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        self.align(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(self.take(N)?);
        if !self.le {
            a.reverse();
        }
        Ok(a)
    }

    pub fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.num::<4>()?))
    }

    fn string(&mut self, long: bool) -> io::Result<String> {
        let n = if long { self.u32()? as usize } else { self.take(1)?[0] as usize };
        let s = String::from_utf8_lossy(self.take(n)?).into_owned();
        if self.take(1)?[0] != 0 {
            return Err(bad("string not NUL terminated"));
        }
        Ok(s)
    }

    /// Reads the value(s) described by `sig` (a full signature, possibly several types).
    pub fn values(&mut self, sig: &str) -> io::Result<Vec<Value>> {
        let s = sig.as_bytes();
        let mut i = 0;
        let mut out = Vec::new();
        while i < s.len() {
            out.push(self.value(s, i)?);
            i = type_end(s, i)?;
        }
        Ok(out)
    }

    fn value(&mut self, s: &[u8], i: usize) -> io::Result<Value> {
        let c = *s.get(i).ok_or_else(|| bad("truncated signature"))?;
        Ok(match c {
            b'y' => Value::U8(self.take(1)?[0]),
            b'b' => Value::Bool(self.u32()? != 0),
            b'n' => Value::I16(i16::from_le_bytes(self.num::<2>()?)),
            b'q' => Value::U16(u16::from_le_bytes(self.num::<2>()?)),
            b'i' => Value::I32(i32::from_le_bytes(self.num::<4>()?)),
            b'u' => Value::U32(self.u32()?),
            b'x' => Value::I64(i64::from_le_bytes(self.num::<8>()?)),
            b't' => Value::U64(u64::from_le_bytes(self.num::<8>()?)),
            b'd' => Value::F64(f64::from_le_bytes(self.num::<8>()?)),
            b's' => Value::Str(self.string(true)?),
            b'o' => Value::Path(self.string(true)?),
            b'g' => Value::Sig(self.string(false)?),
            b'v' => {
                let sig = self.string(false)?;
                let sb = sig.as_bytes();
                if sb.is_empty() || type_end(sb, 0)? != sb.len() {
                    return Err(bad("variant signature must be one complete type"));
                }
                Value::Variant(Box::new(self.value(sb, 0)?))
            }
            b'a' => {
                let len = self.u32()? as usize;
                let elem = i + 1;
                let ec = *s.get(elem).ok_or_else(|| bad("array without element type"))?;
                self.align(align_of(ec))?;
                let end = self.pos.checked_add(len).filter(|e| *e <= self.buf.len()).ok_or_else(|| bad("array past end"))?;
                if ec == b'y' {
                    return Ok(Value::Bytes(self.take(len)?.to_vec()));
                }
                if ec == b'{' {
                    let mut dict = Vec::new();
                    while self.pos < end {
                        self.align(8)?;
                        let k = self.value(s, elem + 1)?;
                        let v = self.value(s, type_end(s, elem + 1)?)?;
                        dict.push((k, v));
                    }
                    return Ok(Value::Dict(dict));
                }
                let mut items = Vec::new();
                while self.pos < end {
                    items.push(self.value(s, elem)?);
                }
                Value::Array(items)
            }
            b'(' => {
                self.align(8)?;
                let mut fields = Vec::new();
                let mut j = i + 1;
                while *s.get(j).ok_or_else(|| bad("unterminated struct"))? != b')' {
                    fields.push(self.value(s, j)?);
                    j = type_end(s, j)?;
                }
                Value::Struct(fields)
            }
            _ => return Err(bad("unsupported type in signature")),
        })
    }
}

/// Little-endian marshaller.
#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn align(&mut self, n: usize) {
        while self.buf.len() % n != 0 {
            self.buf.push(0);
        }
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.align(4);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.align(8);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.align(8);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn string(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }
    pub fn sig(&mut self, s: &str) {
        self.buf.push(s.len() as u8);
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }
    /// Writes an array: `f` appends the elements, whose alignment is `elem_align`.
    pub fn array(&mut self, elem_align: usize, f: impl FnOnce(&mut Writer)) {
        self.align(4);
        let len_at = self.buf.len();
        self.buf.extend_from_slice(&[0; 4]);
        self.align(elem_align);
        let start = self.buf.len();
        f(self);
        let len = (self.buf.len() - start) as u32;
        self.buf[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
    }
}

pub const CALL: u8 = 1;
pub const RETURN: u8 = 2;
pub const ERROR: u8 = 3;
pub const SIGNAL: u8 = 4;

/// Header flag: the caller does not want a reply.
pub const NO_REPLY_EXPECTED: u8 = 0x1;
/// Header flag: do not start the destination's service just for this message (watch it appear instead).
pub const NO_AUTO_START: u8 = 0x2;

#[derive(Debug, Clone, Default)]
pub struct Message {
    pub kind: u8,
    pub flags: u8,
    pub serial: u32,
    pub path: String,
    pub iface: String,
    pub member: String,
    pub error: String,
    pub reply_serial: u32,
    pub dest: String,
    pub sender: String,
    pub sig: String,
    pub body: Vec<u8>,
    pub le: bool,
}

impl Message {
    pub fn args(&self) -> io::Result<Vec<Value>> {
        Reader::new(&self.body, self.le).values(&self.sig)
    }

    pub fn call(dest: &str, path: &str, iface: &str, member: &str, sig: &str, body: Vec<u8>) -> Message {
        Message { kind: CALL, dest: dest.into(), path: path.into(), iface: iface.into(), member: member.into(), sig: sig.into(), body, le: true, ..Default::default() }
    }

    pub fn signal(path: &str, iface: &str, member: &str, sig: &str, body: Vec<u8>) -> Message {
        Message { kind: SIGNAL, path: path.into(), iface: iface.into(), member: member.into(), sig: sig.into(), body, le: true, ..Default::default() }
    }

    pub fn reply_to(call: &Message, sig: &str, body: Vec<u8>) -> Message {
        Message { kind: RETURN, flags: 1, reply_serial: call.serial, dest: call.sender.clone(), sig: sig.into(), body, le: true, ..Default::default() }
    }

    pub fn error_to(call: &Message, name: &str, text: &str) -> Message {
        let mut w = Writer::default();
        w.string(text);
        Message { kind: ERROR, flags: 1, reply_serial: call.serial, dest: call.sender.clone(), error: name.into(), sig: "s".into(), body: w.buf, le: true, ..Default::default() }
    }

    /// Wire bytes of this message (always little-endian), with the given serial.
    pub fn encode(&self, serial: u32) -> Vec<u8> {
        let mut h = Writer::default();
        h.u8(b'l');
        h.u8(self.kind);
        h.u8(self.flags);
        h.u8(1);
        h.u32(self.body.len() as u32);
        h.u32(serial);
        h.array(8, |w| {
            let mut field = |code: u8, sig: &str, s: &str| {
                if s.is_empty() {
                    return;
                }
                w.align(8);
                w.u8(code);
                w.sig(sig);
                if sig == "g" {
                    w.sig(s);
                } else {
                    w.string(s);
                }
            };
            field(1, "o", &self.path);
            field(2, "s", &self.iface);
            field(3, "s", &self.member);
            field(4, "s", &self.error);
            field(6, "s", &self.dest);
            field(8, "g", &self.sig);
            if self.reply_serial != 0 {
                w.align(8);
                w.u8(5);
                w.sig("u");
                w.u32(self.reply_serial);
            }
        });
        h.align(8);
        h.buf.extend_from_slice(&self.body);
        h.buf
    }

    /// Parses one complete message from the start of `buf`; returns it and the bytes consumed,
    /// or `None` while more data is needed.
    pub fn decode(buf: &[u8]) -> io::Result<Option<(Message, usize)>> {
        if buf.len() < 16 {
            return Ok(None);
        }
        let le = match buf[0] {
            b'l' => true,
            b'B' => false,
            _ => return Err(bad("bad endianness marker")),
        };
        let rd = |o: usize| {
            let b: [u8; 4] = buf[o..o + 4].try_into().unwrap_or([0; 4]);
            if le {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let (body_len, serial, fields_len) = (rd(4) as usize, rd(8), rd(12) as usize);
        let header_len = (16 + fields_len).div_ceil(8) * 8;
        let total = header_len.checked_add(body_len).filter(|t| *t <= MAX_MESSAGE).ok_or_else(|| bad("message too large"))?;
        if buf.len() < total {
            return Ok(None);
        }
        let mut m = Message { kind: buf[1], flags: buf[2], serial, le, ..Default::default() };
        let mut r = Reader::new(&buf[..16 + fields_len], le);
        r.pos = 16;
        while r.pos < 16 + fields_len {
            r.align(8)?;
            let code = r.take(1)?[0];
            let sig = r.string(false)?;
            let v = r.values(&sig)?.into_iter().next().ok_or_else(|| bad("empty header field"))?;
            match (code, v) {
                (1, Value::Path(s)) => m.path = s,
                (2, Value::Str(s)) => m.iface = s,
                (3, Value::Str(s)) => m.member = s,
                (4, Value::Str(s)) => m.error = s,
                (5, Value::U32(n)) => m.reply_serial = n,
                (6, Value::Str(s)) => m.dest = s,
                (7, Value::Str(s)) => m.sender = s,
                (8, Value::Sig(s)) => m.sig = s,
                _ => {}
            }
        }
        m.body = buf[header_len..total].to_vec();
        Ok(Some((m, total)))
    }
}

/// `Value`s that are only ever sent by us, as a typed builder for common reply shapes.
pub fn body_u32(v: u32) -> Vec<u8> {
    let mut w = Writer::default();
    w.u32(v);
    w.buf
}

pub fn body_strings(items: &[&str]) -> Vec<u8> {
    let mut w = Writer::default();
    w.array(4, |w| items.iter().for_each(|s| w.string(s)));
    w.buf
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

/// Socket addresses of a D-Bus address string (`unix:path=...;unix:abstract=...,guid=...`), in order.
/// Transports other than `unix:` and server-only keys (`dir`, `tmpdir`) are skipped. Never reads the environment.
pub fn parse_address(addr: &str) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for cand in addr.split(';') {
        let Some(rest) = cand.trim().strip_prefix("unix:") else { continue };
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

/// The session bus address this process was started with (`$DBUS_SESSION_BUS_ADDRESS`, else `$XDG_RUNTIME_DIR/bus`).
/// Read once by callers that then keep it immutable (accessibility never rewrites process environment variables).
pub fn session_address() -> String {
    match std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        Ok(a) if !a.trim().is_empty() => a,
        _ => {
            let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
            format!("unix:path={dir}/bus")
        }
    }
}

/// Session bus address candidates: `$DBUS_SESSION_BUS_ADDRESS`, else `$XDG_RUNTIME_DIR/bus`.
fn socket_addrs() -> Vec<SocketAddr> {
    let mut out = Vec::new();
    let raw = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_default();
    for cand in raw.split(';') {
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
    if out.is_empty() {
        let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        if let Ok(a) = SocketAddr::from_pathname(format!("{dir}/bus")) {
            out.push(a);
        }
    }
    out
}

pub struct Conn {
    sock: UnixStream,
    inbuf: Vec<u8>,
    serial: u32,
    /// Unique name assigned by the bus (`:1.42`).
    pub unique: String,
    /// Complete messages that arrived while waiting for a specific reply (never dropped; see [`Conn::next_message`]).
    backlog: std::collections::VecDeque<Message>,
}

/// What a cancellable connect does when its wake descriptor becomes readable: `true` = give up now.
pub type Cancel<'a> = (RawFd, &'a mut dyn FnMut() -> bool);

/// Waits until `fd` is readable (or `deadline`); a readable `cancel` descriptor runs its callback, which may abort.
fn wait_in(fd: RawFd, cancel: &mut Option<Cancel>, deadline: Instant) -> io::Result<()> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "bus handshake timed out"));
        }
        let mut fds = vec![fd];
        if let Some((c, _)) = cancel.as_ref() {
            fds.push(*c);
        }
        let ready = crate::backend::poll_readable(&fds, Some(left))?;
        if ready.get(1).copied().unwrap_or(false) {
            if let Some((_, f)) = cancel.as_mut() {
                if f() {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "bus connection cancelled"));
                }
            }
        }
        if ready[0] {
            return Ok(());
        }
    }
}

impl Conn {
    /// Connects to the bus at `addr` (a D-Bus address string), authenticates and says Hello, without touching the
    /// process environment. Blocks the calling thread for at most 5 s: call it from a worker, never a UI thread.
    pub fn connect_address(addr: &str) -> io::Result<Conn> {
        Self::connect_address_cancellable(addr, None, Duration::from_secs(5))
    }

    /// [`Conn::connect_address`] on a non-blocking socket: waits in `poll` on the socket and on an optional cancel
    /// descriptor, up to `timeout` in total. Messages that arrive before the Hello reply stay queued (nothing is dropped).
    pub fn connect_address_cancellable(addr: &str, mut cancel: Option<Cancel>, timeout: Duration) -> io::Result<Conn> {
        let deadline = Instant::now() + timeout;
        let mut last = io::Error::new(io::ErrorKind::NotFound, format!("no usable unix socket in bus address {addr:?}"));
        for sa in parse_address(addr) {
            let sock = match UnixStream::connect_addr(&sa) {
                Ok(s) => s,
                Err(e) => {
                    last = e;
                    continue;
                }
            };
            match Self::handshake_nonblocking(sock, &mut cancel, deadline) {
                Ok(c) => return Ok(c),
                Err(e) if e.kind() == io::ErrorKind::Interrupted || e.kind() == io::ErrorKind::TimedOut => return Err(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn handshake_nonblocking(sock: UnixStream, cancel: &mut Option<Cancel>, deadline: Instant) -> io::Result<Conn> {
        sock.set_nonblocking(true)?;
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() }.to_string();
        let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
        let mut c = Conn { sock, inbuf: Vec::new(), serial: 0, unique: String::new(), backlog: Default::default() };
        c.write_all_nb(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let mut line = Vec::new();
        while !line.ends_with(b"\r\n") {
            let mut byte = [0u8; 1];
            match c.sock.read(&mut byte) {
                Ok(0) => return Err(bad("bus closed during authentication")),
                Ok(_) => line.push(byte[0]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => wait_in(c.fd(), cancel, deadline)?,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
            if line.len() > 512 {
                return Err(bad("oversized auth reply"));
            }
        }
        if !line.starts_with(b"OK") {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("bus refused authentication: {}", String::from_utf8_lossy(&line).trim())));
        }
        c.write_all_nb(b"BEGIN\r\n")?;
        let serial = c.send(&Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", "", Vec::new()))?;
        let mut early = Vec::new();
        let reply = loop {
            let mut found = None;
            while let Some(m) = c.next_message()? {
                if m.reply_serial == serial && (m.kind == RETURN || m.kind == ERROR) {
                    found = Some(m);
                    break;
                }
                early.push(m);
            }
            if let Some(m) = found {
                break m;
            }
            if !c.fill()? {
                return Err(bad("bus closed before Hello"));
            }
            if c.inbuf.is_empty() || Message::decode(&c.inbuf)?.is_none() {
                wait_in(c.fd(), cancel, deadline)?;
            }
        };
        if reply.kind == ERROR {
            return Err(io::Error::other(format!("Hello failed: {}", reply.error)));
        }
        c.unique = reply.args()?.first().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        // Anything that arrived ahead of the reply is delivered first, in order.
        c.backlog.extend(early);
        Ok(c)
    }

    fn write_all_nb(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            match self.sock.write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => bytes = &bytes[n..],
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // A peer that stops reading for 2 s is an error, not a reason to spin forever.
                    if !crate::backend::poll_writable(self.fd(), Duration::from_secs(2))? {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "bus socket not writable for 2 s"));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Bytes or messages waiting in the input buffers (a complete message may be queued without the socket being readable).
    pub fn buffered(&self) -> usize {
        self.inbuf.len() + self.backlog.len()
    }

    /// Connects to the session bus, authenticates and says Hello. Blocking; the socket is then non-blocking.
    pub fn session() -> io::Result<Conn> {
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no session bus address");
        for addr in socket_addrs() {
            match UnixStream::connect_addr(&addr) {
                Ok(s) => return Self::handshake(s),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn handshake(mut sock: UnixStream) -> io::Result<Conn> {
        sock.set_read_timeout(Some(Duration::from_secs(5)))?;
        sock.set_write_timeout(Some(Duration::from_secs(5)))?;
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() }.to_string();
        let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
        sock.write_all(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while !line.ends_with(b"\r\n") {
            if sock.read(&mut byte)? == 0 {
                return Err(bad("bus closed during authentication"));
            }
            line.push(byte[0]);
            if line.len() > 512 {
                return Err(bad("oversized auth reply"));
            }
        }
        if !line.starts_with(b"OK") {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("bus refused authentication: {}", String::from_utf8_lossy(&line).trim())));
        }
        sock.write_all(b"BEGIN\r\n")?;
        let mut c = Conn { sock, inbuf: Vec::new(), serial: 0, unique: String::new(), backlog: Default::default() };
        let reply = c.call_blocking(Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", "", Vec::new()))?;
        c.unique = reply.args()?.first().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        Ok(c)
    }

    pub fn fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }

    pub fn set_nonblocking(&self, nb: bool) -> io::Result<()> {
        self.sock.set_nonblocking(nb)
    }

    /// Sends a message; returns the serial used.
    pub fn send(&mut self, m: &Message) -> io::Result<u32> {
        self.serial = self.serial.wrapping_add(1).max(1);
        let bytes = m.encode(self.serial);
        let mut off = 0;
        while off < bytes.len() {
            match self.sock.write(&bytes[off..]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => off += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // A peer that stops reading for 2 s is an error, not a reason to spin forever.
                    if !crate::backend::poll_writable(self.fd(), Duration::from_secs(2))? {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "bus socket not writable for 2 s"));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(self.serial)
    }

    /// Reads whatever is available (non-blocking socket); `Ok(false)` means the bus closed the connection.
    pub fn fill(&mut self) -> io::Result<bool> {
        let mut tmp = [0u8; 8192];
        loop {
            match self.sock.read(&mut tmp) {
                Ok(0) => return Ok(false),
                Ok(n) => self.inbuf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(true),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Next complete buffered message, if any.
    pub fn next_message(&mut self) -> io::Result<Option<Message>> {
        if let Some(m) = self.backlog.pop_front() {
            return Ok(Some(m));
        }
        match Message::decode(&self.inbuf)? {
            Some((m, used)) => {
                self.inbuf.drain(..used);
                Ok(Some(m))
            }
            None => Ok(None),
        }
    }

    /// Sends `m` and waits (up to the socket timeout) for its reply; other traffic is dropped.
    /// Only for start-up and one-shot CLI use, before the socket goes non-blocking.
    pub fn call_blocking(&mut self, m: Message) -> io::Result<Message> {
        let serial = self.send(&m)?;
        loop {
            while let Some(r) = self.next_message()? {
                if r.reply_serial == serial && (r.kind == RETURN || r.kind == ERROR) {
                    if r.kind == ERROR {
                        let text = r.args().ok().and_then(|a| a.first().and_then(|v| v.as_str().map(str::to_string))).unwrap_or_default();
                        return Err(io::Error::other(format!("{}: {}", r.error, text)));
                    }
                    return Ok(r);
                }
            }
            let mut tmp = [0u8; 8192];
            let n = self.sock.read(&mut tmp)?;
            if n == 0 {
                return Err(bad("bus closed"));
            }
            self.inbuf.extend_from_slice(&tmp[..n]);
        }
    }

    /// `RequestName` with DO_NOT_QUEUE; true when this connection now owns `name`.
    pub fn request_name(&mut self, name: &str, replace_existing: bool) -> io::Result<bool> {
        let mut w = Writer::default();
        w.string(name);
        // 0x4 = DBUS_NAME_FLAG_DO_NOT_QUEUE, 0x2 = REPLACE_EXISTING
        w.u32(if replace_existing { 0x6 } else { 0x4 });
        let r = self.call_blocking(Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", "su", w.buf))?;
        Ok(matches!(r.args()?.first().and_then(Value::as_u32), Some(1) | Some(4)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_roundtrips_through_the_wire_format() {
        let mut w = Writer::default();
        w.string("app");
        w.u32(7);
        w.array(4, |w| {
            w.string("default");
            w.string("Open");
        });
        let m = Message::call("org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "Notify", "suas", w.buf);
        let bytes = m.encode(42);
        assert!(Message::decode(&bytes[..bytes.len() - 1]).unwrap().is_none(), "partial data needs more");
        let (d, used) = Message::decode(&bytes).unwrap().unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!((d.serial, d.member.as_str(), d.sig.as_str()), (42, "Notify", "suas"));
        assert_eq!(d.args().unwrap(), vec![Value::Str("app".into()), Value::U32(7), Value::Array(vec![Value::Str("default".into()), Value::Str("Open".into())])]);
    }

    #[test]
    fn parses_hints_dict_with_variants_and_image_struct() {
        // a{sv}: urgency=byte 2, image-data=(iiibiiay)
        let mut w = Writer::default();
        w.array(8, |w| {
            w.align(8);
            w.string("urgency");
            w.sig("y");
            w.u8(2);
            w.align(8);
            w.string("image-data");
            w.sig("(iiibiiay)");
            w.align(8);
            for v in [1u32, 1, 4] {
                w.u32(v);
            }
            w.u32(1);
            w.u32(8);
            w.u32(4);
            w.array(1, |w| w.buf.extend_from_slice(&[1, 2, 3, 4]));
        });
        let vals = Reader::new(&w.buf, true).values("a{sv}").unwrap();
        let Value::Dict(d) = &vals[0] else { panic!("not a dict") };
        assert_eq!(d[0].0.as_str(), Some("urgency"));
        assert_eq!(d[0].1.as_int(), Some(2));
        let Value::Variant(img) = &d[1].1 else { panic!() };
        let Value::Struct(f) = &**img else { panic!() };
        assert_eq!(f.len(), 7);
        assert_eq!(f[6], Value::Bytes(vec![1, 2, 3, 4]));
    }

    #[test]
    fn truncated_or_hostile_input_is_an_error_not_a_panic() {
        let m = Message::call("a.b", "/x", "a.b", "M", "s", {
            let mut w = Writer::default();
            w.string("hello");
            w.buf
        });
        let bytes = m.encode(1);
        let mut cut = bytes.clone();
        cut.truncate(bytes.len() - 3);
        // Declared lengths now exceed the body: decode waits for more data rather than panicking.
        assert!(Message::decode(&cut).unwrap().is_none());
        let mut evil = bytes.clone();
        evil[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Message::decode(&evil).is_err());
        assert!(Reader::new(&[0xff, 0xff, 0xff, 0x7f], true).values("as").is_err());
        assert!(Reader::new(&[1], true).values("(").is_err());
    }

    #[test]
    fn replies_and_errors_address_the_caller() {
        let mut call = Message::call("x", "/", "i", "m", "", vec![]);
        call.serial = 9;
        call.sender = ":1.5".into();
        let r = Message::reply_to(&call, "u", body_u32(3));
        let (d, _) = Message::decode(&r.encode(1)).unwrap().unwrap();
        assert_eq!((d.kind, d.reply_serial, d.dest.as_str()), (RETURN, 9, ":1.5"));
        let e = Message::error_to(&call, "org.freedesktop.DBus.Error.UnknownMethod", "nope");
        let (d, _) = Message::decode(&e.encode(2)).unwrap().unwrap();
        assert_eq!((d.kind, d.error.as_str()), (ERROR, "org.freedesktop.DBus.Error.UnknownMethod"));
    }

    /// A minimal bus: accepts one client, says OK to EXTERNAL auth, then hands the connection to `serve`.
    fn fake_bus(serve: impl FnOnce(&mut UnixStream) + Send + 'static) -> (String, std::thread::JoinHandle<()>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("decebal-dbus-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bus");
        let _ = std::fs::remove_file(&path);
        let l = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut line = Vec::new();
            let mut b = [0u8; 1];
            while !line.ends_with(b"\r\n") {
                s.read_exact(&mut b).unwrap();
                line.push(b[0]);
            }
            assert!(line.starts_with(b"\0AUTH EXTERNAL "));
            s.write_all(b"OK 0123456789abcdef\r\n").unwrap();
            serve(&mut s);
        });
        (format!("unix:path={},guid=0123", path.display()), t, dir)
    }

    fn read_msg(s: &mut UnixStream, buf: &mut Vec<u8>) -> Message {
        loop {
            if let Some((m, used)) = Message::decode(buf).unwrap() {
                buf.drain(..used);
                return m;
            }
            let mut tmp = [0u8; 4096];
            let n = s.read(&mut tmp).unwrap();
            assert!(n > 0, "client closed");
            buf.extend_from_slice(&tmp[..n]);
        }
    }

    #[test]
    fn parses_addresses_without_touching_the_environment() {
        assert_eq!(parse_address("unix:path=/run/a%20b,guid=1;tcp:host=x;unix:abstract=/tmp/dbus-x").len(), 2);
        assert!(parse_address("tcp:host=localhost,port=1").is_empty());
        assert!(parse_address("").is_empty());
    }

    #[test]
    fn connect_address_keeps_traffic_that_arrives_before_the_hello_reply() {
        let (addr, t, dir) = fake_bus(|s| {
            let mut buf = Vec::new();
            let mut begin = [0u8; 7];
            s.read_exact(&mut begin).unwrap();
            assert_eq!(&begin, b"BEGIN\r\n");
            let hello = read_msg(s, &mut buf);
            assert_eq!(hello.member, "Hello");
            // A signal and a method call race ahead of the reply; neither may be lost.
            s.write_all(&Message::signal("/x", "a.b", "Early", "", vec![]).encode(1)).unwrap();
            let mut call = Message::call(":1.5", "/y", "a.b", "Ping", "", vec![]);
            call.sender = ":1.9".into();
            s.write_all(&call.encode(2)).unwrap();
            let mut w = Writer::default();
            w.string(":1.5");
            let mut r = Message::reply_to(&hello, "s", w.buf);
            r.reply_serial = hello.serial;
            s.write_all(&r.encode(3)).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        let mut c = Conn::connect_address(&addr).unwrap();
        assert_eq!(c.unique, ":1.5");
        assert!(c.buffered() > 0);
        assert_eq!(c.next_message().unwrap().unwrap().member, "Early");
        assert_eq!(c.next_message().unwrap().unwrap().member, "Ping");
        t.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cancelled_connect_returns_at_once() {
        let (addr, t, dir) = fake_bus(|s| {
            // Never answers Hello.
            let mut b = [0u8; 256];
            let _ = s.read(&mut b);
            std::thread::sleep(Duration::from_millis(500));
        });
        let (rx, tx) = UnixStream::pair().unwrap();
        let start = Instant::now();
        let poke = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            (&tx).write_all(&[1]).unwrap();
            tx
        });
        let mut cancel = || true;
        let r = Conn::connect_address_cancellable(&addr, Some((rx.as_raw_fd(), &mut cancel)), Duration::from_secs(5));
        assert_eq!(r.err().map(|e| e.kind()), Some(io::ErrorKind::Interrupted));
        assert!(start.elapsed() < Duration::from_secs(1));
        let _ = poke.join();
        t.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn address_unescaping() {
        assert_eq!(unescape("/run/user/1000/a%20b"), "/run/user/1000/a b");
        assert_eq!(unescape("plain%"), "plain%");
    }
}
