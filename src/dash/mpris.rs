//! MPRIS (`org.mpris.MediaPlayer2.*`) over decebal's own minimal D-Bus client ([`crate::dbus`]).
//!
//! * [`Model`] is a pure state machine: it is fed every message from the bus and answers with the requests to send
//!   (the tests drive it with synthesised messages).
//! * [`Mpris`] runs a [`Model`] on a worker thread (one `poll` over the bus socket and a command wake-up socket),
//!   publishes snapshots for the UI thread and wakes it.
//!
//! Players are found with `ListNames`, kept current by `NameOwnerChanged` and `PropertiesChanged`, and their position
//! (which MPRIS does not signal) is polled once a second, only while the UI asks for it.

use crate::dbus::{Conn, Message, Value, Writer, CALL, ERROR, RETURN, SIGNAL};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const OBJ: &str = "/org/mpris/MediaPlayer2";
const IFACE_ROOT: &str = "org.mpris.MediaPlayer2";
const IFACE_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const IFACE_PROPS: &str = "org.freedesktop.DBus.Properties";
const BUS: (&str, &str, &str) = ("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus");
/// How often the position is asked for while the UI wants it.
pub const POSITION_EVERY: Duration = Duration::from_secs(1);

// ---------------------------------------------------------------- data

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl Status {
    pub fn parse(s: &str) -> Status {
        match s {
            "Playing" => Status::Playing,
            "Paused" => Status::Paused,
            _ => Status::Stopped,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub title: String,
    /// All artists joined with ", ".
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub length_us: Option<i64>,
    pub track_id: Option<String>,
}

/// `Metadata` (`a{sv}`, possibly still wrapped in a variant). Players are sloppy: artist may be a plain string,
/// the length may be any integer type, and a missing title is just empty.
pub fn parse_metadata(v: &Value) -> Meta {
    let dict = match v {
        Value::Variant(inner) => return parse_metadata(inner),
        Value::Dict(d) => d,
        _ => return Meta::default(),
    };
    let get = |key: &str| dict.iter().find(|(k, _)| k.as_str() == Some(key)).map(|(_, v)| v);
    let text = |key: &str| get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let artist = match get("xesam:artist") {
        Some(Value::Variant(inner)) => artists(inner),
        Some(other) => artists(other),
        None => String::new(),
    };
    let art = text("mpris:artUrl");
    Meta {
        title: text("xesam:title"),
        artist,
        album: text("xesam:album"),
        art_url: (!art.is_empty()).then_some(art),
        length_us: get("mpris:length").and_then(|v| v.as_int().or_else(|| v.as_f64().map(|f| f as i64))).filter(|l| *l > 0),
        track_id: get("mpris:trackid").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
    }
}

fn artists(v: &Value) -> String {
    match v {
        Value::Array(items) => items.iter().filter_map(Value::as_str).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "),
        Value::Variant(inner) => artists(inner),
        other => other.as_str().unwrap_or("").to_string(),
    }
}

/// One player on the bus.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// Well-known name, e.g. `org.mpris.MediaPlayer2.spotify`.
    pub bus: String,
    /// Unique name (`:1.42`) that signals come from.
    pub owner: String,
    pub identity: String,
    pub status: Status,
    pub meta: Meta,
    pub position_us: i64,
    pub pos_at: Instant,
    pub volume: Option<f64>,
    pub can_control: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_seek: bool,
    /// The first `GetAll` has been answered.
    pub ready: bool,
}

impl Player {
    pub fn new(bus: &str, now: Instant) -> Player {
        Player {
            bus: bus.to_string(),
            owner: String::new(),
            identity: String::new(),
            status: Status::Stopped,
            meta: Meta::default(),
            position_us: 0,
            pos_at: now,
            volume: None,
            can_control: true,
            can_next: true,
            can_prev: true,
            can_seek: false,
            ready: false,
        }
    }

    /// Short name for the picker: `Identity`, else the bus name without its prefix.
    pub fn label(&self) -> String {
        if !self.identity.is_empty() {
            self.identity.clone()
        } else {
            self.bus.strip_prefix(BUS_PREFIX).unwrap_or(&self.bus).to_string()
        }
    }

    /// Position now: the last reading plus the time since it while playing, clamped to the track length.
    pub fn position_at(&self, now: Instant) -> i64 {
        let mut p = self.position_us;
        if self.status == Status::Playing {
            p += now.saturating_duration_since(self.pos_at).as_micros() as i64;
        }
        match self.meta.length_us {
            Some(l) => p.clamp(0, l),
            None => p.max(0),
        }
    }

    /// Fraction of the track played (0 when the length is unknown).
    pub fn progress(&self, now: Instant) -> f32 {
        match self.meta.length_us {
            Some(l) if l > 0 => (self.position_at(now) as f32 / l as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    /// Applies a property dictionary (`GetAll` reply or `PropertiesChanged`). Returns what changed.
    pub fn apply(&mut self, props: &[(Value, Value)], now: Instant) -> Applied {
        let mut a = Applied::default();
        for (k, v) in props {
            match k.as_str().unwrap_or("") {
                "PlaybackStatus" => {
                    let s = Status::parse(v.as_str().unwrap_or(""));
                    if s != self.status {
                        // The anchor of the interpolation moves with the state.
                        self.position_us = self.position_at(now);
                        self.pos_at = now;
                        self.status = s;
                        a.changed = true;
                        a.refetch_position = true;
                    }
                }
                "Metadata" => {
                    let m = parse_metadata(v);
                    if m != self.meta {
                        // A new track starts at 0 (the very first metadata may come with a position in the same batch).
                        if self.meta.track_id.is_some() && m.track_id != self.meta.track_id {
                            self.position_us = 0;
                            self.pos_at = now;
                        }
                        self.meta = m;
                        a.changed = true;
                        a.refetch_position = true;
                    }
                }
                "Volume" => {
                    let vol = v.as_f64().filter(|x| x.is_finite()).map(|x| x.max(0.0));
                    if vol != self.volume {
                        self.volume = vol;
                        a.changed = true;
                    }
                }
                "Position" => {
                    if let Some(p) = v.as_int() {
                        self.position_us = p;
                        self.pos_at = now;
                        a.changed = true;
                    }
                }
                "CanControl" => set(&mut self.can_control, v, &mut a),
                "CanGoNext" => set(&mut self.can_next, v, &mut a),
                "CanGoPrevious" => set(&mut self.can_prev, v, &mut a),
                "CanSeek" => set(&mut self.can_seek, v, &mut a),
                "Identity" => {
                    let id = v.as_str().unwrap_or("").to_string();
                    if id != self.identity {
                        self.identity = id;
                        a.changed = true;
                    }
                }
                _ => {}
            }
        }
        a
    }
}

fn set(slot: &mut bool, v: &Value, a: &mut Applied) {
    if let Some(b) = v.as_bool() {
        if *slot != b {
            *slot = b;
            a.changed = true;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    pub changed: bool,
    /// Status or track changed: the position anchor is stale, ask for it again.
    pub refetch_position: bool,
}

/// The player the UI should show: the one it chose if still there, else the first playing one, else the first paused, else any.
pub fn pick_active<'a>(players: &'a [Player], chosen: Option<&str>) -> Option<&'a Player> {
    chosen
        .and_then(|c| players.iter().find(|p| p.bus == c))
        .or_else(|| players.iter().find(|p| p.status == Status::Playing))
        .or_else(|| players.iter().find(|p| p.status == Status::Paused))
        .or_else(|| players.first())
}

// ---------------------------------------------------------------- requests

/// What an outstanding request was for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    ListNames,
    Owner(String),
    All { bus: String, root: bool },
    Position(String),
    Ignore,
}

pub type Request = (Message, Pending);

fn dbus_call(member: &str, sig: &str, body: Vec<u8>) -> Message {
    Message::call(BUS.0, BUS.1, BUS.2, member, sig, body)
}

fn add_match(rule: &str) -> Request {
    let mut w = Writer::default();
    w.string(rule);
    (dbus_call("AddMatch", "s", w.buf), Pending::Ignore)
}

fn get_all(bus: &str, iface: &str) -> Request {
    let mut w = Writer::default();
    w.string(iface);
    (Message::call(bus, OBJ, IFACE_PROPS, "GetAll", "s", w.buf), Pending::All { bus: bus.into(), root: iface == IFACE_ROOT })
}

fn get_position(bus: &str) -> Request {
    let mut w = Writer::default();
    w.string(IFACE_PLAYER);
    w.string("Position");
    (Message::call(bus, OBJ, IFACE_PROPS, "Get", "ss", w.buf), Pending::Position(bus.into()))
}

fn player_call(bus: &str, member: &str) -> Request {
    (Message::call(bus, OBJ, IFACE_PLAYER, member, "", Vec::new()), Pending::Ignore)
}

fn set_volume(bus: &str, v: f64) -> Request {
    let mut w = Writer::default();
    w.string(IFACE_PLAYER);
    w.string("Volume");
    w.sig("d");
    w.f64(v);
    (Message::call(bus, OBJ, IFACE_PROPS, "Set", "ssv", w.buf), Pending::Ignore)
}

fn set_position(bus: &str, track: &str, pos_us: i64) -> Request {
    let mut w = Writer::default();
    w.string(track);
    w.i64(pos_us);
    (Message::call(bus, OBJ, IFACE_PLAYER, "SetPosition", "ox", w.buf), Pending::Ignore)
}

/// What the UI can ask for.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    PlayPause(String),
    Next(String),
    Previous(String),
    Volume(String, f64),
    /// Seek to an absolute position (microseconds).
    Seek(String, i64),
    /// Poll positions once a second (the Media tab is showing).
    Polling(bool),
}

// ---------------------------------------------------------------- the state machine

#[derive(Debug, Default)]
pub struct Model {
    pub players: Vec<Player>,
    pending: HashMap<u32, Pending>,
    polling: bool,
}

impl Model {
    /// Messages to send at start-up: the match rules and the first player listing.
    pub fn start() -> Vec<Request> {
        vec![
            add_match("type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'"),
            add_match("type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/mpris/MediaPlayer2'"),
            add_match("type='signal',interface='org.mpris.MediaPlayer2.Player',member='Seeked',path='/org/mpris/MediaPlayer2'"),
            (dbus_call("ListNames", "", Vec::new()), Pending::ListNames),
        ]
    }

    /// Remembers the serial a request went out with.
    pub fn sent(&mut self, serial: u32, what: Pending) {
        if what != Pending::Ignore {
            self.pending.insert(serial, what);
        }
    }

    /// Players ready to show, in a stable order.
    pub fn snapshot(&self) -> Vec<Player> {
        let mut v: Vec<Player> = self.players.iter().filter(|p| p.ready).cloned().collect();
        v.sort_by(|a, b| a.bus.cmp(&b.bus));
        v
    }

    fn find(&mut self, bus: &str) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.bus == bus)
    }

    fn by_owner(&mut self, owner: &str) -> Option<&mut Player> {
        if owner.is_empty() {
            return None;
        }
        self.players.iter_mut().find(|p| p.owner == owner)
    }

    fn discover(&mut self, bus: &str, owner: Option<&str>, now: Instant, out: &mut Vec<Request>) {
        if !bus.starts_with(BUS_PREFIX) {
            return;
        }
        self.players.retain(|p| p.bus != bus);
        let mut p = Player::new(bus, now);
        match owner {
            Some(o) => p.owner = o.to_string(),
            None => {
                let mut w = Writer::default();
                w.string(bus);
                out.push((dbus_call("GetNameOwner", "s", w.buf), Pending::Owner(bus.into())));
            }
        }
        self.players.push(p);
        out.push(get_all(bus, IFACE_PLAYER));
        out.push(get_all(bus, IFACE_ROOT));
    }

    /// Handles one message from the bus; returns what to send in response. `changed` tells whether the snapshot differs now.
    pub fn on_message(&mut self, m: &Message, now: Instant, changed: &mut bool) -> Vec<Request> {
        let mut out = Vec::new();
        match m.kind {
            SIGNAL => self.on_signal(m, now, changed, &mut out),
            RETURN | ERROR => {
                let Some(what) = self.pending.remove(&m.reply_serial) else { return out };
                if m.kind == ERROR {
                    // A player that vanished between listing and asking, or one that refuses: forget a half-known one.
                    if let Pending::All { bus, .. } | Pending::Owner(bus) = &what {
                        let was_ready = self.players.iter().any(|p| p.bus == *bus && p.ready);
                        if !was_ready {
                            self.players.retain(|p| p.bus != *bus);
                        }
                    }
                    return out;
                }
                self.on_reply(&what, m, now, changed, &mut out);
            }
            CALL => {}
            _ => {}
        }
        out
    }

    fn on_reply(&mut self, what: &Pending, m: &Message, now: Instant, changed: &mut bool, out: &mut Vec<Request>) {
        let args = m.args().unwrap_or_default();
        match what {
            Pending::ListNames => {
                if let Some(Value::Array(names)) = args.first() {
                    for n in names.iter().filter_map(Value::as_str).filter(|n| n.starts_with(BUS_PREFIX)) {
                        if !self.players.iter().any(|p| p.bus == n) {
                            self.discover(n, None, now, out);
                        }
                    }
                }
            }
            Pending::Owner(bus) => {
                if let (Some(owner), Some(p)) = (args.first().and_then(Value::as_str), self.find(bus)) {
                    p.owner = owner.to_string();
                }
            }
            Pending::All { bus, root } => {
                if let (Some(Value::Dict(props)), Some(p)) = (args.first(), self.find(bus)) {
                    let a = p.apply(props, now);
                    if !*root {
                        p.ready = true;
                    }
                    *changed |= a.changed || p.ready;
                }
            }
            Pending::Position(bus) => {
                if let (Some(pos), Some(p)) = (args.first().and_then(Value::as_int), self.find(bus)) {
                    p.position_us = pos;
                    p.pos_at = now;
                    *changed = true;
                }
            }
            Pending::Ignore => {}
        }
    }

    fn on_signal(&mut self, m: &Message, now: Instant, changed: &mut bool, out: &mut Vec<Request>) {
        let args = m.args().unwrap_or_default();
        match (m.iface.as_str(), m.member.as_str()) {
            ("org.freedesktop.DBus", "NameOwnerChanged") => {
                let (Some(name), Some(new)) = (args.first().and_then(Value::as_str), args.get(2).and_then(Value::as_str)) else { return };
                if !name.starts_with(BUS_PREFIX) {
                    return;
                }
                if new.is_empty() {
                    let before = self.players.len();
                    self.players.retain(|p| p.bus != name);
                    *changed |= self.players.len() != before;
                } else {
                    self.discover(name, Some(new), now, out);
                }
            }
            (IFACE_PROPS, "PropertiesChanged") => {
                let Some(iface) = args.first().and_then(Value::as_str) else { return };
                let (Some(Value::Dict(props)), invalidated) = (args.get(1), args.get(2)) else { return };
                let stale = matches!(invalidated, Some(Value::Array(a)) if !a.is_empty());
                let Some(p) = self.by_owner(&m.sender) else { return };
                if iface != IFACE_PLAYER && iface != IFACE_ROOT {
                    return;
                }
                let a = p.apply(props, now);
                *changed |= a.changed;
                let bus = p.bus.clone();
                if a.refetch_position {
                    out.push(get_position(&bus));
                }
                if stale {
                    out.push(get_all(&bus, iface));
                }
            }
            (IFACE_PLAYER, "Seeked") => {
                if let (Some(pos), Some(p)) = (args.first().and_then(Value::as_int), self.by_owner(&m.sender)) {
                    p.position_us = pos;
                    p.pos_at = now;
                    *changed = true;
                }
            }
            _ => {}
        }
    }

    /// Requests for a UI command.
    pub fn on_command(&mut self, c: &Cmd, now: Instant) -> Vec<Request> {
        match c {
            Cmd::PlayPause(b) => vec![player_call(b, "PlayPause")],
            Cmd::Next(b) => vec![player_call(b, "Next")],
            Cmd::Previous(b) => vec![player_call(b, "Previous")],
            Cmd::Volume(b, v) => {
                if let Some(p) = self.find(b) {
                    p.volume = Some(*v);
                }
                vec![set_volume(b, *v)]
            }
            Cmd::Seek(b, pos) => {
                let track = self.players.iter().find(|p| p.bus == *b).and_then(|p| p.meta.track_id.clone());
                if let Some(p) = self.find(b) {
                    p.position_us = *pos;
                    p.pos_at = now;
                }
                match track {
                    Some(t) => vec![set_position(b, &t, *pos)],
                    None => Vec::new(),
                }
            }
            Cmd::Polling(on) => {
                self.polling = *on;
                if *on {
                    self.position_requests()
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub fn polling(&self) -> bool {
        self.polling
    }

    /// Position requests for every ready player (one per poll tick; a paused player's position is static, so only the playing ones).
    pub fn position_requests(&self) -> Vec<Request> {
        self.players.iter().filter(|p| p.ready && p.status == Status::Playing).map(|p| get_position(&p.bus)).collect()
    }
}

// ---------------------------------------------------------------- the worker

/// What the worker publishes for the UI.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub players: Vec<Player>,
    /// Bumped on every publish so the UI can tell whether anything changed.
    pub generation: u64,
    /// Connected to a session bus.
    pub connected: bool,
}

/// Handle on the worker thread.
pub struct Mpris {
    shared: Arc<Mutex<Snapshot>>,
    tx: Sender<Cmd>,
    wake: UnixStream,
}

impl Mpris {
    /// Starts the worker; `waker` is called (from the worker) whenever a new snapshot is published.
    pub fn start(waker: Arc<dyn Fn() + Send + Sync>) -> Mpris {
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let (tx, rx) = channel();
        let (wake, wake_rd) = UnixStream::pair().expect("socketpair");
        let _ = wake.set_nonblocking(true);
        let _ = wake_rd.set_nonblocking(true);
        let s = shared.clone();
        std::thread::spawn(move || {
            if let Err(e) = worker(&s, &rx, wake_rd, &*waker) {
                eprintln!("decdash: mpris: {e}");
            }
            if let Ok(mut g) = s.lock() {
                g.connected = false;
                g.players.clear();
                g.generation += 1;
            }
            waker();
        });
        Mpris { shared, tx, wake }
    }

    pub fn send(&self, c: Cmd) {
        if self.tx.send(c).is_ok() {
            let _ = (&self.wake).write(&[1]);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

fn worker(shared: &Mutex<Snapshot>, rx: &Receiver<Cmd>, mut wake_rd: UnixStream, waker: &dyn Fn()) -> std::io::Result<()> {
    let mut conn = Conn::session()?;
    conn.set_nonblocking(true)?;
    let mut model = Model::default();
    let mut next_poll = Instant::now() + POSITION_EVERY;
    let send_all = |conn: &mut Conn, model: &mut Model, reqs: Vec<Request>| -> std::io::Result<()> {
        for (msg, what) in reqs {
            let serial = conn.send(&msg)?;
            model.sent(serial, what);
        }
        Ok(())
    };
    send_all(&mut conn, &mut model, Model::start())?;
    if let Ok(mut g) = shared.lock() {
        g.connected = true;
        g.generation += 1;
    }
    waker();
    loop {
        let timeout = model.polling().then(|| next_poll.saturating_duration_since(Instant::now()));
        let ready = crate::backend::poll_readable(&[conn.fd(), wake_rd.as_raw_fd()], timeout)?;
        let now = Instant::now();
        let mut changed = false;
        if ready[1] {
            let mut sink = [0u8; 64];
            while matches!(wake_rd.read(&mut sink), Ok(n) if n > 0) {}
        }
        while let Ok(cmd) = rx.try_recv() {
            let reqs = model.on_command(&cmd, now);
            changed |= matches!(cmd, Cmd::Volume(..) | Cmd::Seek(..));
            send_all(&mut conn, &mut model, reqs)?;
        }
        if ready[0] && !conn.fill()? {
            return Err(std::io::Error::other("the bus closed the connection"));
        }
        while let Some(m) = conn.next_message()? {
            let reqs = model.on_message(&m, now, &mut changed);
            send_all(&mut conn, &mut model, reqs)?;
        }
        if model.polling() && now >= next_poll {
            next_poll = now + POSITION_EVERY;
            let reqs = model.position_requests();
            send_all(&mut conn, &mut model, reqs)?;
        }
        if changed {
            if let Ok(mut g) = shared.lock() {
                g.players = model.snapshot();
                g.generation += 1;
            }
            waker();
        }
    }
}

/// Parses `file://` (percent-encoded) art URLs into a path; other schemes give `None`.
pub fn file_url_path(url: &str) -> Option<std::path::PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = rest.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    Some(std::path::PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

/// `m:ss` (or `h:mm:ss`) for a duration in microseconds.
pub fn format_time(us: i64) -> String {
    let s = (us.max(0) / 1_000_000) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }
    fn var(v: Value) -> Value {
        Value::Variant(Box::new(v))
    }
    fn dict(items: Vec<(&str, Value)>) -> Vec<(Value, Value)> {
        items.into_iter().map(|(k, v)| (s(k), var(v))).collect()
    }
    fn meta_value() -> Value {
        Value::Dict(dict(vec![
            ("mpris:trackid", Value::Path("/org/test/track/1".into())),
            ("mpris:length", Value::I64(215_000_000)),
            ("mpris:artUrl", s("file:///tmp/cover%20art.png")),
            ("xesam:title", s("Blue Monday")),
            ("xesam:artist", Value::Array(vec![s("New Order"), s("Someone")])),
            ("xesam:album", s("Power, Corruption & Lies")),
        ]))
    }

    #[test]
    fn metadata_parsing() {
        let m = parse_metadata(&var(meta_value()));
        assert_eq!(m.title, "Blue Monday");
        assert_eq!(m.artist, "New Order, Someone");
        assert_eq!(m.album, "Power, Corruption & Lies");
        assert_eq!(m.art_url.as_deref(), Some("file:///tmp/cover%20art.png"));
        assert_eq!(m.length_us, Some(215_000_000));
        assert_eq!(m.track_id.as_deref(), Some("/org/test/track/1"));
    }

    #[test]
    fn sloppy_metadata_is_tolerated() {
        let m = parse_metadata(&Value::Dict(dict(vec![("xesam:artist", s("Just A String")), ("mpris:length", Value::U64(5_000_000)), ("xesam:title", s(""))])));
        assert_eq!((m.artist.as_str(), m.length_us, m.title.as_str()), ("Just A String", Some(5_000_000), ""));
        assert_eq!(m.art_url, None);
        assert_eq!(parse_metadata(&Value::Dict(dict(vec![("mpris:length", Value::F64(2e6))]))).length_us, Some(2_000_000));
        assert_eq!(parse_metadata(&Value::Dict(dict(vec![("mpris:length", Value::I64(-1))]))).length_us, None, "nonsense length");
        assert_eq!(parse_metadata(&Value::Bool(true)), Meta::default());
        assert_eq!(parse_metadata(&Value::Dict(vec![])), Meta::default());
        let multi = parse_metadata(&Value::Dict(dict(vec![("xesam:artist", Value::Array(vec![s(""), s("A")]))])));
        assert_eq!(multi.artist, "A", "empty artists are dropped");
    }

    /// Wire round trip: the bytes a real player sends (`a{sv}` inside a variant) parse into the same metadata.
    #[test]
    fn metadata_from_wire_bytes() {
        let mut w = Writer::default();
        w.sig("a{sv}");
        w.array(8, |w| {
            for (k, sig, val) in [("xesam:title", "s", "Wire Title"), ("mpris:artUrl", "s", "file:///x.png")] {
                w.align(8);
                w.string(k);
                w.sig(sig);
                w.string(val);
            }
            w.align(8);
            w.string("mpris:length");
            w.sig("x");
            w.i64(123_000_000);
            w.align(8);
            w.string("xesam:artist");
            w.sig("as");
            w.array(4, |w| w.string("Wire Artist"));
        });
        let vals = crate::dbus::Reader::new(&w.buf, true).values("v").unwrap();
        let m = parse_metadata(&vals[0]);
        assert_eq!((m.title.as_str(), m.artist.as_str(), m.length_us), ("Wire Title", "Wire Artist", Some(123_000_000)));
    }

    #[test]
    fn apply_tracks_state_and_position() {
        let t0 = Instant::now();
        let mut p = Player::new("org.mpris.MediaPlayer2.test", t0);
        let a =
            p.apply(&dict(vec![("PlaybackStatus", s("Playing")), ("Metadata", meta_value()), ("Volume", Value::F64(0.5)), ("Position", Value::I64(10_000_000)), ("CanSeek", Value::Bool(true))]), t0);
        assert!(a.changed && a.refetch_position);
        assert_eq!((p.status, p.volume, p.can_seek), (Status::Playing, Some(0.5), true));
        assert_eq!(p.position_at(t0 + Duration::from_secs(5)), 15_000_000, "interpolates while playing");
        assert_eq!(p.position_at(t0 + Duration::from_secs(500)), 215_000_000, "clamped to the length");
        assert!((p.progress(t0 + Duration::from_secs(97)) - 0.5).abs() < 0.01);
        // Pausing freezes the interpolation where it was.
        let t1 = t0 + Duration::from_secs(20);
        p.apply(&dict(vec![("PlaybackStatus", s("Paused"))]), t1);
        assert_eq!(p.position_at(t1 + Duration::from_secs(60)), 30_000_000);
        // A new track resets the position.
        let mut other = parse_metadata(&meta_value());
        other.track_id = Some("/org/test/track/2".into());
        let a = p.apply(&[(s("Metadata"), var(Value::Dict(dict(vec![("mpris:trackid", Value::Path("/org/test/track/2".into())), ("xesam:title", s("Next One"))]))))], t1);
        assert!(a.changed && a.refetch_position);
        assert_eq!(p.position_at(t1), 0);
        assert_eq!(p.meta.title, "Next One");
        let again = p.apply(&[(s("PlaybackStatus"), var(s("Paused")))], t1);
        assert!(!again.changed, "no change, nothing to do");
        assert_eq!(p.volume, Some(0.5));
    }

    #[test]
    fn labels_and_picking() {
        let t = Instant::now();
        let mut a = Player::new("org.mpris.MediaPlayer2.firefox.instance1", t);
        let mut b = Player::new("org.mpris.MediaPlayer2.spotify", t);
        assert_eq!(a.label(), "firefox.instance1");
        b.identity = "Spotify".into();
        assert_eq!(b.label(), "Spotify");
        a.status = Status::Paused;
        let ps = vec![a.clone(), b.clone()];
        assert_eq!(pick_active(&ps, None).unwrap().bus, a.bus, "paused beats stopped");
        b.status = Status::Playing;
        let ps = vec![a.clone(), b.clone()];
        assert_eq!(pick_active(&ps, None).unwrap().bus, b.bus, "playing wins");
        assert_eq!(pick_active(&ps, Some(&a.bus)).unwrap().bus, a.bus, "an explicit choice wins");
        assert_eq!(pick_active(&ps, Some("gone")).unwrap().bus, b.bus, "a vanished choice falls back");
        assert!(pick_active(&[], None).is_none());
    }

    // ---- the state machine, driven with synthesised bus traffic

    fn reply(serial: u32, sig: &str, body: Vec<u8>) -> Message {
        Message { kind: RETURN, reply_serial: serial, sig: sig.into(), body, le: true, ..Default::default() }
    }

    type Prop<'a> = (&'a str, &'a str, Box<dyn Fn(&mut Writer)>);

    fn getall_body(props: &[Prop]) -> Vec<u8> {
        let mut w = Writer::default();
        w.array(8, |w| {
            for (k, sig, f) in props {
                w.align(8);
                w.string(k);
                w.sig(sig);
                f(w);
            }
        });
        w.buf
    }

    fn player_props() -> Vec<u8> {
        getall_body(&[
            ("PlaybackStatus", "s", Box::new(|w| w.string("Playing"))),
            ("Volume", "d", Box::new(|w| w.f64(0.8))),
            ("Position", "x", Box::new(|w| w.i64(42_000_000))),
            ("CanSeek", "b", Box::new(|w| w.u32(1))),
            (
                "Metadata",
                "a{sv}",
                Box::new(|w| {
                    w.array(8, |w| {
                        w.align(8);
                        w.string("xesam:title");
                        w.sig("s");
                        w.string("Fake Song");
                        w.align(8);
                        w.string("mpris:trackid");
                        w.sig("o");
                        w.string("/fake/1");
                    })
                }),
            ),
        ])
    }

    fn name_owner_changed(name: &str, old: &str, new: &str) -> Message {
        let mut w = Writer::default();
        w.string(name);
        w.string(old);
        w.string(new);
        Message { kind: SIGNAL, iface: "org.freedesktop.DBus".into(), member: "NameOwnerChanged".into(), sig: "sss".into(), body: w.buf, le: true, ..Default::default() }
    }

    #[test]
    fn startup_requests_matches_then_lists() {
        let r = Model::start();
        assert_eq!(r.iter().filter(|(m, _)| m.member == "AddMatch").count(), 3);
        assert_eq!(r.last().unwrap().1, Pending::ListNames);
        assert_eq!(r.last().unwrap().0.member, "ListNames");
    }

    #[test]
    fn discovers_a_player_and_publishes_it_once_ready() {
        let mut model = Model::default();
        let now = Instant::now();
        let mut changed = false;
        // ListNames reply with one player and one other name.
        model.sent(1, Pending::ListNames);
        let mut w = Writer::default();
        w.array(4, |w| {
            w.string("org.freedesktop.DBus");
            w.string("org.mpris.MediaPlayer2.test");
        });
        let out = model.on_message(&reply(1, "as", w.buf), now, &mut changed);
        assert_eq!(out.iter().map(|(m, _)| m.member.as_str()).collect::<Vec<_>>(), ["GetNameOwner", "GetAll", "GetAll"]);
        assert!(model.snapshot().is_empty(), "not shown before its properties arrived");
        for (serial, (m, what)) in (10u32..).zip(out) {
            if m.member == "GetNameOwner" {
                model.sent(serial, what);
                let mut w = Writer::default();
                w.string(":1.77");
                model.on_message(&reply(serial, "s", w.buf), now, &mut changed);
            } else if m.iface == IFACE_PROPS && matches!(&what, Pending::All { root: false, .. }) {
                model.sent(serial, what);
                model.on_message(&reply(serial, "a{sv}", player_props()), now, &mut changed);
            } else {
                model.sent(serial, what);
                let root = getall_body(&[("Identity", "s", Box::new(|w| w.string("Test Player")))]);
                model.on_message(&reply(serial, "a{sv}", root), now, &mut changed);
            }
        }
        assert!(changed);
        let snap = model.snapshot();
        assert_eq!(snap.len(), 1);
        let p = &snap[0];
        assert_eq!((p.owner.as_str(), p.identity.as_str(), p.status, p.meta.title.as_str()), (":1.77", "Test Player", Status::Playing, "Fake Song"));
        assert_eq!((p.volume, p.position_us, p.can_seek, p.meta.track_id.as_deref()), (Some(0.8), 42_000_000, true, Some("/fake/1")));
    }

    #[test]
    fn name_owner_changed_adds_and_removes() {
        let mut model = Model::default();
        let now = Instant::now();
        let mut changed = false;
        let out = model.on_message(&name_owner_changed("org.mpris.MediaPlayer2.x", "", ":1.5"), now, &mut changed);
        assert_eq!(out.len(), 2, "the owner is known: just the two GetAll calls");
        assert_eq!(model.players[0].owner, ":1.5");
        for (i, (m, what)) in out.into_iter().enumerate() {
            let _ = m;
            model.sent(20 + i as u32, what);
            model.on_message(&reply(20 + i as u32, "a{sv}", if i == 0 { player_props() } else { getall_body(&[]) }), now, &mut changed);
        }
        assert_eq!(model.snapshot().len(), 1);
        changed = false;
        model.on_message(&name_owner_changed("org.mpris.MediaPlayer2.x", ":1.5", ""), now, &mut changed);
        assert!(changed && model.snapshot().is_empty(), "the player left the bus");
        // Unrelated names are ignored.
        let out = model.on_message(&name_owner_changed("org.other.Thing", "", ":1.9"), now, &mut changed);
        assert!(out.is_empty() && model.players.is_empty());
    }

    #[test]
    fn properties_changed_updates_and_asks_for_the_position() {
        let mut model = Model::default();
        let now = Instant::now();
        let mut changed = false;
        let mut p = Player::new("org.mpris.MediaPlayer2.x", now);
        p.owner = ":1.5".into();
        p.ready = true;
        model.players.push(p);
        let mut w = Writer::default();
        w.string(IFACE_PLAYER);
        w.array(8, |w| {
            w.align(8);
            w.string("PlaybackStatus");
            w.sig("s");
            w.string("Paused");
        });
        w.array(4, |_| {});
        let sig = Message { kind: SIGNAL, sender: ":1.5".into(), iface: IFACE_PROPS.into(), member: "PropertiesChanged".into(), sig: "sa{sv}as".into(), body: w.buf, le: true, ..Default::default() };
        let out = model.on_message(&sig, now, &mut changed);
        assert!(changed);
        assert_eq!(model.players[0].status, Status::Paused);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].0.member.as_str(), &out[0].1), ("Get", &Pending::Position("org.mpris.MediaPlayer2.x".into())));
        // A signal from someone else is not ours.
        let mut other = sig.clone();
        other.sender = ":1.99".into();
        changed = false;
        assert!(model.on_message(&other, now, &mut changed).is_empty() && !changed);
    }

    #[test]
    fn position_reply_and_seeked() {
        let mut model = Model::default();
        let now = Instant::now();
        let mut changed = false;
        let mut p = Player::new("org.mpris.MediaPlayer2.x", now);
        p.owner = ":1.5".into();
        p.ready = true;
        model.players.push(p);
        model.sent(5, Pending::Position("org.mpris.MediaPlayer2.x".into()));
        let mut w = Writer::default();
        w.sig("x");
        w.i64(77_000_000);
        model.on_message(&reply(5, "v", w.buf), now, &mut changed);
        assert_eq!(model.players[0].position_us, 77_000_000);
        let mut w = Writer::default();
        w.i64(5_000_000);
        let seeked = Message { kind: SIGNAL, sender: ":1.5".into(), iface: IFACE_PLAYER.into(), member: "Seeked".into(), sig: "x".into(), body: w.buf, le: true, ..Default::default() };
        model.on_message(&seeked, now, &mut changed);
        assert_eq!(model.players[0].position_us, 5_000_000);
    }

    #[test]
    fn errors_forget_half_known_players_only() {
        let mut model = Model::default();
        let now = Instant::now();
        let mut changed = false;
        model.players.push(Player::new("org.mpris.MediaPlayer2.dead", now));
        model.sent(1, Pending::All { bus: "org.mpris.MediaPlayer2.dead".into(), root: false });
        let err = Message { kind: ERROR, reply_serial: 1, error: "org.freedesktop.DBus.Error.ServiceUnknown".into(), le: true, ..Default::default() };
        model.on_message(&err, now, &mut changed);
        assert!(model.players.is_empty());
        let mut live = Player::new("org.mpris.MediaPlayer2.live", now);
        live.ready = true;
        model.players.push(live);
        model.sent(2, Pending::All { bus: "org.mpris.MediaPlayer2.live".into(), root: true });
        let err = Message { kind: ERROR, reply_serial: 2, le: true, ..Default::default() };
        model.on_message(&err, now, &mut changed);
        assert_eq!(model.players.len(), 1, "a ready player survives a failed refresh");
    }

    #[test]
    fn commands_build_the_right_calls() {
        let mut model = Model::default();
        let now = Instant::now();
        let bus = "org.mpris.MediaPlayer2.x";
        let mut p = Player::new(bus, now);
        p.ready = true;
        p.status = Status::Playing;
        p.meta.track_id = Some("/t/1".into());
        model.players.push(p);
        for (cmd, member) in [(Cmd::PlayPause(bus.into()), "PlayPause"), (Cmd::Next(bus.into()), "Next"), (Cmd::Previous(bus.into()), "Previous")] {
            let r = model.on_command(&cmd, now);
            assert_eq!((r[0].0.dest.as_str(), r[0].0.iface.as_str(), r[0].0.member.as_str(), r[0].0.path.as_str()), (bus, IFACE_PLAYER, member, OBJ));
        }
        let v = model.on_command(&Cmd::Volume(bus.into(), 0.25), now);
        assert_eq!((v[0].0.member.as_str(), v[0].0.iface.as_str(), v[0].0.sig.as_str()), ("Set", IFACE_PROPS, "ssv"));
        let args = v[0].0.args().unwrap();
        assert_eq!((args[0].as_str(), args[1].as_str(), args[2].as_f64()), (Some(IFACE_PLAYER), Some("Volume"), Some(0.25)));
        assert_eq!(model.players[0].volume, Some(0.25), "optimistic");
        let seek = model.on_command(&Cmd::Seek(bus.into(), 9_000_000), now);
        assert_eq!((seek[0].0.member.as_str(), seek[0].0.sig.as_str()), ("SetPosition", "ox"));
        let a = seek[0].0.args().unwrap();
        assert_eq!((a[0].as_str(), a[1].as_int()), (Some("/t/1"), Some(9_000_000)));
        // Without a track id there is nothing to seek in.
        model.players[0].meta.track_id = None;
        assert!(model.on_command(&Cmd::Seek(bus.into(), 1), now).is_empty());
        // Polling asks for the positions of playing players only.
        let mut paused = Player::new("org.mpris.MediaPlayer2.y", now);
        paused.ready = true;
        model.players.push(paused);
        let r = model.on_command(&Cmd::Polling(true), now);
        assert_eq!(r.len(), 1);
        assert!(model.polling());
        assert!(model.on_command(&Cmd::Polling(false), now).is_empty() && !model.polling());
    }

    #[test]
    fn art_urls_and_times() {
        assert_eq!(file_url_path("file:///tmp/cover%20art.png"), Some("/tmp/cover art.png".into()));
        assert_eq!(file_url_path("file://localhost/a/b.jpg"), Some("/a/b.jpg".into()));
        assert_eq!(file_url_path("https://x/y.jpg"), None);
        assert_eq!(file_url_path("file://relative"), None);
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(215_000_000), "3:35");
        assert_eq!(format_time(3_725_000_000), "1:02:05");
        assert_eq!(format_time(-5), "0:00");
    }
}
