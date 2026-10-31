//! The AT-SPI bridge worker: one thread that owns the session-bus watch and the accessibility-bus connection.
//!
//! * Lazy: it only watches `org.a11y.Status.IsEnabled` on the session bus (never auto-starting the launcher) and
//!   connects to the accessibility bus while that is true. Until then the UI thread builds no trees at all.
//! * Asynchronous: the registry `Embed` handshake, `GetAddress` and every reply are serial-tracked messages in
//!   the worker's `poll` loop; nothing waits on the UI thread and no unrelated traffic is ever dropped.
//! * Bounded: one coalescing tree slot (latest snapshot wins), an action queue of [`Config::queue`] requests
//!   (a full queue answers `LimitsExceeded` at once), a reply channel that can hold every pending request, and a
//!   deadline after which an unanswered request fails instead of hanging the AT.
//! * Idle-free: the thread sleeps in `poll` on its wake pipe and bus sockets; it only uses a timeout while a
//!   reconnect is scheduled or a request is pending.
//! * Stoppable: dropping [`Bridge`] sets the stop flag, wakes the thread (also out of a connect in progress), and
//!   joins it; the thread answers pending calls, unembeds, and closes its sockets before it exits.

use super::action::{ActionError, Request};
use super::atspi::{self, Ctx, Outcome, ReplyKind};
use super::tree::{Tree, TreeBuilder};
use crate::dbus::{Conn, Message, Value, Writer, ERROR, NO_AUTO_START, RETURN, SIGNAL};
use crate::window_app::Waker;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// `DECEBAL_A11Y`: `0`/`off` never starts the bridge, `1`/`on` connects without waiting for `org.a11y.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub app_name: String,
    /// The session bus, read once from the environment (never rewritten).
    pub session_address: Option<String>,
    /// `AT_SPI_BUS_ADDRESS`: an accessibility bus given explicitly (used instead of `org.a11y.Bus.GetAddress`).
    pub explicit_address: Option<String>,
    pub mode: Mode,
    pub locale: String,
    pub toolkit_version: String,
    /// Most action requests waiting for the UI thread.
    pub queue: usize,
    /// How long a request may wait for the UI thread before it fails.
    pub reply_timeout: Duration,
}

impl Config {
    /// Reads the environment once (on the caller's thread); the bridge keeps the result immutable.
    pub fn from_env(app_name: &str) -> Config {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let mode = match var("DECEBAL_A11Y").as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("0" | "off" | "no" | "false") => Mode::Off,
            Some("1" | "on" | "yes" | "true") => Mode::On,
            _ => Mode::Auto,
        };
        let locale = var("LC_ALL").or_else(|| var("LC_MESSAGES")).or_else(|| var("LANG")).unwrap_or_else(|| "C".into());
        Config {
            app_name: app_name.to_string(),
            session_address: Some(crate::dbus::session_address()),
            explicit_address: var("AT_SPI_BUS_ADDRESS"),
            mode,
            locale,
            toolkit_version: env!("CARGO_PKG_VERSION").to_string(),
            queue: 32,
            reply_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Watching `org.a11y.Status` (accessibility off, or the launcher not running).
    #[default]
    Watching,
    /// No session bus and no explicit accessibility bus: nothing to do until stopped.
    NoSession,
    /// Connected to the accessibility bus, waiting for the first tree or the registry's Embed reply.
    Registering,
    /// Embedded in the registry and serving.
    Serving,
    /// Waiting to retry the accessibility bus.
    Backoff,
    Stopped,
}

/// Counters for diagnostics and the native tests (`DECEBAL_A11Y_STATUS` writes them as JSON).
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub phase: Phase,
    pub enabled: bool,
    pub connects: u32,
    pub embeds: u32,
    pub app_id: i32,
    pub unique: String,
    pub trees: u64,
    pub revision: u64,
    pub nodes: usize,
    pub calls: u64,
    pub requests: u64,
    pub busy: u64,
    pub timeouts: u64,
    pub events: u64,
    pub last_error: Option<String>,
}

struct Shared {
    slot: Mutex<Option<Arc<Tree>>>,
    want: AtomicBool,
    generation: AtomicU64,
    stop: AtomicBool,
    status: Mutex<Status>,
    ui_wake: Waker,
}

type Reply = (u64, Result<(), ActionError>);

/// The UI thread's handle on the worker.
pub struct Bridge {
    shared: Arc<Shared>,
    wake: UnixStream,
    requests: Receiver<Request>,
    replies: SyncSender<Reply>,
    thread: Option<JoinHandle<()>>,
}

impl Bridge {
    pub fn start(cfg: Config, ui_wake: Waker) -> io::Result<Bridge> {
        let (wake_rx, wake_tx) = UnixStream::pair()?;
        wake_rx.set_nonblocking(true)?;
        wake_tx.set_nonblocking(true)?;
        let shared =
            Arc::new(Shared { slot: Mutex::new(None), want: AtomicBool::new(false), generation: AtomicU64::new(0), stop: AtomicBool::new(false), status: Mutex::new(Status::default()), ui_wake });
        let (req_tx, req_rx) = sync_channel(cfg.queue.max(1));
        let (rep_tx, rep_rx) = sync_channel(cfg.queue.max(1) * 2);
        let sh = shared.clone();
        let thread = std::thread::Builder::new().name("a11y-bridge".into()).spawn(move || {
            let mut w = Worker::new(cfg, sh.clone(), wake_rx, req_tx, rep_rx);
            w.run();
            sh.status.lock().unwrap().phase = Phase::Stopped;
        })?;
        Ok(Bridge { shared, wake: wake_tx, requests: req_rx, replies: rep_tx, thread: Some(thread) })
    }

    /// An AT is listening: build and [`publish`](Self::publish) trees.
    pub fn wants_tree(&self) -> bool {
        self.shared.want.load(Ordering::Acquire)
    }

    /// Bumps on every (re)registration; a new value means "publish a tree now, even if nothing changed".
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    /// Hands the worker a new snapshot. A snapshot it has not picked up yet is replaced (coalesced).
    pub fn publish(&self, tree: Arc<Tree>) {
        *self.shared.slot.lock().unwrap() = Some(tree);
        self.poke();
    }

    pub fn next_request(&self) -> Option<Request> {
        self.requests.try_recv().ok()
    }

    pub fn reply(&self, id: u64, result: Result<(), ActionError>) {
        // The channel holds twice the request queue, so this only fails if the worker is gone.
        let _ = self.replies.try_send((id, result));
        self.poke();
    }

    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }

    fn poke(&self) {
        // A full pipe already holds a pending wake-up.
        let _ = (&self.wake).write(&[1]);
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.poke();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Pending {
    call: Message,
    kind: ReplyKind,
    deadline: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionCall {
    IsEnabled,
    GetAddress,
}

struct A11y {
    conn: Conn,
    embed_serial: Option<u32>,
    embedded: bool,
    /// Send Embed once the first tree arrived, or at this instant at the latest.
    embed_at: Option<Instant>,
}

struct Worker {
    cfg: Config,
    shared: Arc<Shared>,
    wake: UnixStream,
    requests: SyncSender<Request>,
    replies: Receiver<Reply>,
    session: Option<Conn>,
    session_calls: HashMap<u32, SessionCall>,
    enabled: bool,
    a11y: Option<A11y>,
    ctx: Ctx,
    serving: Arc<Tree>,
    pending: HashMap<u64, Pending>,
    next_id: u64,
    retry_at: Option<Instant>,
    backoff: Duration,
}

const BACKOFF_MIN: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

fn drain(s: &UnixStream) {
    let mut b = [0u8; 64];
    while matches!((&*s).read(&mut b), Ok(n) if n > 0) {}
}

fn add_match(rule: &str) -> Message {
    let mut w = Writer::default();
    w.string(rule);
    Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "AddMatch", "s", w.buf)
}

impl Worker {
    fn new(cfg: Config, shared: Arc<Shared>, wake: UnixStream, requests: SyncSender<Request>, replies: Receiver<Reply>) -> Worker {
        let mut ids = super::tree::Ids::default();
        let serving = Arc::new(TreeBuilder::new(&mut ids, &cfg.app_name).finish());
        let ctx = Ctx { unique: String::new(), parent: None, app_id: -1, locale: cfg.locale.clone(), toolkit_version: cfg.toolkit_version.clone() };
        Worker {
            cfg,
            shared,
            wake,
            requests,
            replies,
            session: None,
            session_calls: HashMap::new(),
            enabled: false,
            a11y: None,
            ctx,
            serving,
            pending: HashMap::new(),
            next_id: 1,
            retry_at: None,
            backoff: BACKOFF_MIN,
        }
    }

    fn stopping(&self) -> bool {
        self.shared.stop.load(Ordering::Acquire)
    }

    fn status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.shared.status.lock().unwrap());
    }

    fn note_error(&self, e: impl std::fmt::Display) {
        let text = e.to_string();
        crate::log_warn!("accessibility bridge: {text}");
        self.status(|s| s.last_error = Some(text));
    }

    /// Connects with the wake pipe as the cancel descriptor: a stop request aborts the handshake.
    fn connect(&self, addr: &str) -> io::Result<Conn> {
        let shared = self.shared.clone();
        let wake = self.wake.try_clone()?;
        let mut on_wake = move || {
            drain(&wake);
            shared.stop.load(Ordering::Acquire)
        };
        Conn::connect_address_cancellable(addr, Some((self.wake.as_raw_fd(), &mut on_wake)), Duration::from_secs(5))
    }

    fn run(&mut self) {
        if self.cfg.mode == Mode::Off {
            return;
        }
        if let Some(addr) = self.cfg.session_address.clone().filter(|_| self.cfg.mode == Mode::Auto) {
            match self.connect(&addr) {
                Ok(c) => {
                    self.session = Some(c);
                    self.watch_status();
                }
                Err(e) if self.stopping() => {
                    let _ = e;
                    return;
                }
                Err(e) => self.note_error(format!("session bus unavailable ({e}); accessibility follows AT_SPI_BUS_ADDRESS only")),
            }
        }
        if self.session.is_none() {
            if self.cfg.mode == Mode::On || self.cfg.explicit_address.is_some() {
                self.set_enabled(true);
            } else {
                self.status(|s| s.phase = Phase::NoSession);
            }
        }
        while !self.stopping() {
            if let Err(e) = self.step() {
                self.note_error(e);
            }
        }
        self.shutdown();
    }

    fn watch_status(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let rules = [
            "type='signal',sender='org.a11y.Bus',path='/org/a11y/bus',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0='org.a11y.Status'",
            "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='org.a11y.Bus'",
        ];
        for r in rules {
            let _ = s.send(&add_match(r));
        }
        self.query_enabled();
    }

    fn query_enabled(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let mut w = Writer::default();
        w.string("org.a11y.Status");
        w.string("IsEnabled");
        let mut m = Message::call("org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Properties", "Get", "ss", w.buf);
        // Never start the bus launcher ourselves: if it is not running, NameOwnerChanged tells us when it is.
        m.flags |= NO_AUTO_START;
        if let Ok(serial) = s.send(&m) {
            self.session_calls.insert(serial, SessionCall::IsEnabled);
        }
    }

    fn set_enabled(&mut self, on: bool) {
        let on = on || self.cfg.mode == Mode::On;
        self.status(|s| s.enabled = on);
        if on == self.enabled {
            return;
        }
        self.enabled = on;
        if on {
            self.backoff = BACKOFF_MIN;
            self.begin_connect();
        } else {
            self.retry_at = None;
            self.disconnect("accessibility was turned off");
            self.status(|s| s.phase = Phase::Watching);
        }
    }

    fn begin_connect(&mut self) {
        if self.a11y.is_some() || !self.enabled {
            return;
        }
        if let Some(addr) = self.cfg.explicit_address.clone() {
            self.connect_a11y(&addr);
            return;
        }
        let Some(s) = self.session.as_mut() else { return };
        let mut m = Message::call("org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", "", Vec::new());
        m.flags |= NO_AUTO_START;
        match s.send(&m) {
            Ok(serial) => {
                self.session_calls.insert(serial, SessionCall::GetAddress);
            }
            Err(e) => {
                self.note_error(format!("GetAddress: {e}"));
                self.schedule_retry();
            }
        }
    }

    fn connect_a11y(&mut self, addr: &str) {
        match self.connect(addr) {
            Ok(mut c) => {
                let _ = c.send(&add_match("type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='org.a11y.atspi.Registry'"));
                self.ctx.unique = c.unique.clone();
                self.ctx.parent = None;
                self.ctx.app_id = -1;
                self.a11y = Some(A11y { conn: c, embed_serial: None, embedded: false, embed_at: Some(Instant::now() + Duration::from_millis(500)) });
                self.retry_at = None;
                let unique = self.ctx.unique.clone();
                self.status(|s| {
                    s.connects += 1;
                    s.phase = Phase::Registering;
                    s.unique = unique;
                });
                // Ask the UI for a tree first so the registry's first look already sees the window.
                *self.shared.slot.lock().unwrap() = None;
                self.shared.want.store(true, Ordering::Release);
                self.shared.generation.fetch_add(1, Ordering::AcqRel);
                (self.shared.ui_wake)();
            }
            Err(e) if self.stopping() => {
                let _ = e;
            }
            Err(e) => {
                self.note_error(format!("accessibility bus {addr}: {e}"));
                self.schedule_retry();
            }
        }
    }

    fn schedule_retry(&mut self) {
        if !self.enabled || self.stopping() {
            return;
        }
        self.retry_at = Some(Instant::now() + self.backoff);
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
        self.status(|s| s.phase = Phase::Backoff);
    }

    /// Drops the accessibility connection; pending calls cannot be answered on it any more.
    fn disconnect(&mut self, why: &str) {
        self.shared.want.store(false, Ordering::Release);
        self.pending.clear();
        if let Some(mut a) = self.a11y.take() {
            if a.embedded {
                let _ = a.conn.send(&atspi::unembed_call(&a.conn.unique));
            }
            crate::log_info!("accessibility bridge: disconnected ({why})");
        }
        // Start from an empty application again so the next registration sends a complete first view.
        let mut ids = super::tree::Ids::default();
        self.serving = Arc::new(TreeBuilder::new(&mut ids, &self.cfg.app_name).finish());
    }

    fn timeout(&self) -> Option<Duration> {
        let now = Instant::now();
        let mut t: Option<Instant> = self.retry_at;
        if let Some(a) = &self.a11y {
            if let Some(e) = a.embed_at {
                t = Some(t.map_or(e, |x| x.min(e)));
            }
        }
        for p in self.pending.values() {
            t = Some(t.map_or(p.deadline, |x| x.min(p.deadline)));
        }
        let buffered = self.session.as_ref().is_some_and(|c| c.buffered() > 0) || self.a11y.as_ref().is_some_and(|a| a.conn.buffered() > 0);
        if buffered {
            return Some(Duration::ZERO);
        }
        t.map(|t| t.saturating_duration_since(now))
    }

    fn step(&mut self) -> io::Result<()> {
        let mut fds: Vec<RawFd> = vec![self.wake.as_raw_fd()];
        let si = self.session.as_ref().map(|c| {
            fds.push(c.fd());
            fds.len() - 1
        });
        let ai = self.a11y.as_ref().map(|a| {
            fds.push(a.conn.fd());
            fds.len() - 1
        });
        let ready = crate::backend::poll_readable(&fds, self.timeout())?;
        if ready[0] {
            drain(&self.wake);
        }
        if self.stopping() {
            return Ok(());
        }
        if si.is_some_and(|i| ready[i]) || self.session.as_ref().is_some_and(|c| c.buffered() > 0) {
            self.read_session();
        }
        if ai.is_some_and(|i| ready[i]) || self.a11y.as_ref().is_some_and(|a| a.conn.buffered() > 0) {
            self.read_a11y();
        }
        self.take_tree();
        self.take_replies();
        self.timers();
        Ok(())
    }

    fn read_session(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let mut msgs = Vec::new();
        let open = s.fill().unwrap_or(false);
        while let Ok(Some(m)) = s.next_message() {
            msgs.push(m);
        }
        for m in msgs {
            self.on_session(m);
        }
        if !open {
            // The session is ending. Keep any accessibility connection until it closes too.
            self.note_error("session bus closed");
            self.session = None;
            self.session_calls.clear();
        }
    }

    fn on_session(&mut self, m: Message) {
        match m.kind {
            RETURN | ERROR => match self.session_calls.remove(&m.reply_serial) {
                Some(SessionCall::IsEnabled) => {
                    let on = m.kind == RETURN && m.args().ok().and_then(|a| a.first().and_then(Value::as_bool)).unwrap_or(false);
                    // An error (launcher not running) with an explicit bus address still counts as enabled.
                    let on = on || (m.kind == ERROR && self.cfg.explicit_address.is_some());
                    self.set_enabled(on);
                }
                Some(SessionCall::GetAddress) => {
                    let addr = (m.kind == RETURN).then(|| m.args().ok().and_then(|a| a.first().and_then(|v| v.as_str().map(str::to_string)))).flatten();
                    match addr.filter(|a| !a.is_empty()) {
                        Some(a) if self.enabled => self.connect_a11y(&a),
                        Some(_) => {}
                        None => {
                            self.note_error(format!("GetAddress failed: {}", m.error));
                            self.schedule_retry();
                        }
                    }
                }
                None => {}
            },
            SIGNAL if m.member == "PropertiesChanged" => {
                let Ok(args) = m.args() else { return };
                if args.first().and_then(Value::as_str) != Some("org.a11y.Status") {
                    return;
                }
                let mut enabled = None;
                if let Some(Value::Dict(d)) = args.get(1) {
                    for (k, v) in d {
                        if matches!(k.as_str(), Some("IsEnabled") | Some("ScreenReaderEnabled")) {
                            enabled = Some(enabled.unwrap_or(false) || v.as_bool().unwrap_or(false));
                        }
                    }
                }
                let invalidated = matches!(args.get(2), Some(Value::Array(a)) if a.iter().any(|v| v.as_str() == Some("IsEnabled")));
                match enabled {
                    Some(on) => self.set_enabled(on),
                    None if invalidated => self.query_enabled(),
                    None => {}
                }
            }
            SIGNAL if m.member == "NameOwnerChanged" => {
                let Ok(args) = m.args() else { return };
                if args.first().and_then(Value::as_str) == Some("org.a11y.Bus") && args.get(2).and_then(Value::as_str).is_some_and(|n| !n.is_empty()) {
                    self.query_enabled();
                }
            }
            _ => {}
        }
    }

    fn read_a11y(&mut self) {
        let Some(a) = self.a11y.as_mut() else { return };
        let open = a.conn.fill().unwrap_or(false);
        let mut msgs = Vec::new();
        loop {
            match a.conn.next_message() {
                Ok(Some(m)) => msgs.push(m),
                Ok(None) => break,
                Err(e) => {
                    self.note_error(format!("accessibility bus sent a malformed message: {e}"));
                    self.lost();
                    return;
                }
            }
        }
        for m in msgs {
            self.on_a11y(m);
        }
        if !open {
            self.lost();
        }
    }

    /// The accessibility bus went away (launcher restart, bus crash): recover while still enabled.
    fn lost(&mut self) {
        self.disconnect("accessibility bus closed");
        self.backoff = BACKOFF_MIN;
        self.schedule_retry();
    }

    fn send(&mut self, m: &Message) {
        let failed = match self.a11y.as_mut() {
            Some(a) => a.conn.send(m).is_err(),
            None => false,
        };
        if failed {
            self.lost();
        }
    }

    fn on_a11y(&mut self, m: Message) {
        match m.kind {
            crate::dbus::CALL => {
                self.status(|s| s.calls += 1);
                match atspi::handle(&self.ctx, &self.serving, &m) {
                    Outcome::Reply(r) => self.send(&r),
                    Outcome::None => {}
                    Outcome::SetAppId(id, ok) => {
                        self.ctx.app_id = id;
                        self.status(|s| s.app_id = id);
                        if ok.kind != 0 {
                            self.send(&ok);
                        }
                    }
                    Outcome::Defer { node, op, kind } => self.defer(m, node, op, kind),
                }
            }
            RETURN | ERROR => {
                let Some(a) = self.a11y.as_mut() else { return };
                if a.embed_serial == Some(m.reply_serial) {
                    a.embed_serial = None;
                    if m.kind == RETURN {
                        if let Some(Value::Struct(f)) = m.args().ok().and_then(|v| v.into_iter().next()) {
                            if let (Some(bus), Some(path)) = (f.first().and_then(Value::as_str), f.get(1).and_then(Value::as_str)) {
                                self.ctx.parent = Some((bus.to_string(), path.to_string()));
                            }
                        }
                        a.embedded = true;
                        self.backoff = BACKOFF_MIN;
                        let rev = self.serving.revision;
                        self.status(|s| {
                            s.embeds += 1;
                            s.phase = Phase::Serving;
                            s.revision = rev;
                        });
                        crate::log_info!("accessibility bridge: embedded as {} (app id {})", self.ctx.unique, self.ctx.app_id);
                    } else {
                        self.note_error(format!("registry refused Embed: {}", m.error));
                        // The registry may not be up yet; NameOwnerChanged re-embeds when it is.
                    }
                }
            }
            SIGNAL if m.member == "NameOwnerChanged" => {
                let Ok(args) = m.args() else { return };
                if args.first().and_then(Value::as_str) == Some("org.a11y.atspi.Registry") && args.get(2).and_then(Value::as_str).is_some_and(|n| !n.is_empty()) {
                    // A (re)started registry: register again.
                    if let Some(a) = self.a11y.as_mut() {
                        a.embedded = false;
                        a.embed_serial = None;
                        a.embed_at = Some(Instant::now());
                    }
                }
            }
            _ => {}
        }
    }

    fn defer(&mut self, call: Message, node: super::tree::NodeId, op: super::action::Op, kind: ReplyKind) {
        let no_reply = call.flags & crate::dbus::NO_REPLY_EXPECTED != 0;
        if self.pending.len() >= self.cfg.queue {
            self.status(|s| s.busy += 1);
            if !no_reply {
                self.send(&atspi::action_error(&call, &ActionError::Busy));
            }
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let deadline = Instant::now() + self.cfg.reply_timeout;
        let req = Request { id, revision: self.serving.revision, node, op, expires: Some(deadline) };
        match self.requests.try_send(req) {
            Ok(()) => {
                self.status(|s| s.requests += 1);
                self.pending.insert(id, Pending { call, kind, deadline });
                (self.shared.ui_wake)();
            }
            Err(TrySendError::Full(_)) => {
                self.status(|s| s.busy += 1);
                if !no_reply {
                    self.send(&atspi::action_error(&call, &ActionError::Busy));
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                if !no_reply {
                    self.send(&atspi::action_error(&call, &ActionError::Gone));
                }
            }
        }
    }

    fn take_tree(&mut self) {
        let Some(new) = self.shared.slot.lock().unwrap().take() else { return };
        if self.a11y.is_none() {
            return;
        }
        let embedded = self.a11y.as_ref().is_some_and(|a| a.embedded);
        let msgs = if embedded { atspi::events(&self.ctx, &self.serving, &new) } else { Vec::new() };
        let (rev, nodes, n_ev) = (new.revision, new.len(), msgs.len() as u64);
        self.serving = new;
        for m in &msgs {
            self.send(m);
        }
        self.status(|s| {
            s.trees += 1;
            s.revision = rev;
            s.nodes = nodes;
            s.events += n_ev;
        });
        if let Some(a) = self.a11y.as_mut() {
            if !a.embedded && a.embed_serial.is_none() {
                a.embed_at = Some(Instant::now());
            }
        }
        self.timers();
    }

    fn take_replies(&mut self) {
        loop {
            match self.replies.try_recv() {
                Ok((id, result)) => {
                    let Some(p) = self.pending.remove(&id) else { continue };
                    if p.call.flags & crate::dbus::NO_REPLY_EXPECTED != 0 {
                        continue;
                    }
                    let msg = match result {
                        Ok(()) => atspi::deferred_ok(&p.call, p.kind),
                        Err(e) => atspi::action_error(&p.call, &e),
                    };
                    self.send(&msg);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn timers(&mut self) {
        let now = Instant::now();
        if self.retry_at.is_some_and(|t| t <= now) {
            self.retry_at = None;
            self.begin_connect();
        }
        let due = self.a11y.as_ref().is_some_and(|a| !a.embedded && a.embed_serial.is_none() && a.embed_at.is_some_and(|t| t <= now));
        if due {
            let unique = self.ctx.unique.clone();
            let sent = self.a11y.as_mut().map(|a| {
                a.embed_at = None;
                a.conn.send(&atspi::embed_call(&unique)).map(|serial| a.embed_serial = Some(serial)).is_ok()
            });
            if sent == Some(false) {
                self.lost();
                return;
            }
        }
        let late: Vec<u64> = self.pending.iter().filter(|(_, p)| p.deadline <= now).map(|(k, _)| *k).collect();
        for id in late {
            if let Some(p) = self.pending.remove(&id) {
                self.status(|s| s.timeouts += 1);
                if p.call.flags & crate::dbus::NO_REPLY_EXPECTED == 0 {
                    let e = Message::error_to(&p.call, "org.decebal.Accessibility.Error.Timeout", "the window did not handle the request in time (it may be busy or closing)");
                    self.send(&e);
                }
            }
        }
    }

    fn shutdown(&mut self) {
        let pending: Vec<Pending> = self.pending.drain().map(|(_, p)| p).collect();
        for p in pending {
            if p.call.flags & crate::dbus::NO_REPLY_EXPECTED == 0 {
                self.send(&atspi::action_error(&p.call, &ActionError::Gone));
            }
        }
        if let Some(mut a) = self.a11y.take() {
            if a.embedded {
                let _ = a.conn.send(&atspi::unembed_call(&a.conn.unique));
            }
        }
        self.shared.want.store(false, Ordering::Release);
        self.session = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: Mode) -> Config {
        Config { app_name: "Test".into(), session_address: None, explicit_address: None, mode, locale: "C".into(), toolkit_version: "t".into(), queue: 2, reply_timeout: Duration::from_millis(200) }
    }

    #[test]
    fn without_any_bus_the_worker_idles_and_stops_promptly() {
        let woke = Arc::new(AtomicU64::new(0));
        let w2 = woke.clone();
        let b = Bridge::start(
            cfg(Mode::Auto),
            Arc::new(move || {
                w2.fetch_add(1, Ordering::Relaxed);
            }),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(b.status().phase, Phase::NoSession);
        assert!(!b.wants_tree());
        let t0 = Instant::now();
        drop(b);
        assert!(t0.elapsed() < Duration::from_secs(1), "stop must not wait for a timeout");
        assert_eq!(woke.load(Ordering::Relaxed), 0, "no UI wake-ups while nothing listens");
    }

    #[test]
    fn off_mode_never_connects() {
        let b = Bridge::start(Config { session_address: Some("unix:path=/nonexistent/bus".into()), ..cfg(Mode::Off) }, Arc::new(|| {})).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(!b.wants_tree());
        drop(b);
    }
}
