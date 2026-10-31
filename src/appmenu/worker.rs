//! The app-menu worker: one thread, one bus connection, no blocking calls after `Hello`.
//!
//! Every request is asynchronous and tagged with the focus epoch it was made for; replies for an
//! older epoch are dropped, so a slow exporter can never paint its menu over a newer window's.

use super::bus::{self, Bus, BUS, BUS_PATH, FLAG_NO_REPLY_EXPECTED};
use super::registrar::{self, Table};
use super::{ActivationRequest, Command, Config, Exported, Focus, MenuToken, Notify, RegistrarPolicy, RegistrarRole, Shared, State, Unavailable, DBUSMENU};
use crate::dbus::{self, Message, Value, Writer};
use std::collections::HashMap;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DBUS_ERR: &str = "org.freedesktop.DBus.Error";
/// `DBUS_NAME_FLAG_ALLOW_REPLACEMENT | DBUS_NAME_FLAG_DO_NOT_QUEUE`; never `REPLACE_EXISTING`.
const REQUEST_FLAGS: u32 = 0x1 | 0x4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    RegistrarOwner,
    RequestRegistrar,
    MenuForWindow { epoch: u64, window: u32 },
    ResolveOwner { epoch: u64 },
    Layout { epoch: u64 },
    AboutToShow { epoch: u64 },
    Click { epoch: u64 },
    Ignore,
}

struct Call {
    deadline: Instant,
    /// Replies from any other sender are ignored.
    expect: String,
    what: Pending,
}

/// The exporter currently answering for the focused window.
struct Source {
    window: u32,
    service: String,
    owner: String,
    path: String,
    tree: Option<Arc<super::MenuTree>>,
    inflight: bool,
    /// A layout change arrived while a fetch was in flight: fetch again after it.
    dirty: bool,
    refetch_at: Option<Instant>,
}

enum Exit {
    Stop,
    Lost(String),
}

struct Worker<'a> {
    cfg: &'a Config,
    shared: &'a Shared,
    notify: &'a Notify,
    rx: &'a Receiver<Command>,
    wake: &'a mut UnixStream,
    bus: Bus,
    pending: HashMap<u32, Call>,
    role: RegistrarRole,
    table: Table,
    registrar_retry: Option<Instant>,
    focus: Focus,
    epoch: u64,
    source: Option<Source>,
    state: State,
    notice: Option<String>,
    activation: Option<ActivationRequest>,
    activation_seq: u64,
    delivered: u64,
    dirty: bool,
}

pub(super) fn run(cfg: Config, shared: Arc<Shared>, rx: Receiver<Command>, mut wake: UnixStream, notify: Notify) {
    let mut backoff = Duration::from_millis(250);
    let mut focus = Focus::None;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let Some(address) = cfg.address.clone() else {
            publish(&shared, &notify, State::Unavailable(Unavailable::NoBus("no session bus address".into())), RegistrarRole::Absent, None);
            // Nothing will make an address appear: wait for stop only.
            if wait_stop(&shared, &mut wake, &rx, None) {
                break;
            }
            continue;
        };
        match Bus::connect(&address, cfg.limits.connect_timeout, cfg.limits.max_buffered) {
            Err(e) => {
                publish(&shared, &notify, State::Unavailable(Unavailable::NoBus(e.to_string())), RegistrarRole::Unknown, None);
                if wait_stop(&shared, &mut wake, &rx, Some(backoff)) {
                    break;
                }
                backoff = (backoff * 2).min(cfg.limits.max_backoff);
            }
            Ok(conn) => {
                backoff = Duration::from_millis(250);
                let mut w = Worker {
                    cfg: &cfg,
                    shared: &shared,
                    notify: &notify,
                    rx: &rx,
                    wake: &mut wake,
                    bus: conn,
                    pending: HashMap::new(),
                    role: RegistrarRole::Unknown,
                    table: Table::new(cfg.limits.max_registrations, cfg.limits.max_registrations_per_sender),
                    registrar_retry: None,
                    focus: focus.clone(),
                    epoch: 0,
                    source: None,
                    state: State::Starting,
                    notice: None,
                    activation: None,
                    activation_seq: 0,
                    delivered: 0,
                    dirty: true,
                };
                let exit = w.run();
                focus = w.focus.clone();
                drop(w);
                match exit {
                    Exit::Stop => break,
                    Exit::Lost(e) => {
                        publish(&shared, &notify, State::Unavailable(Unavailable::NoBus(format!("connection lost: {e}"))), RegistrarRole::Unknown, None);
                        if wait_stop(&shared, &mut wake, &rx, Some(backoff)) {
                            break;
                        }
                    }
                }
            }
        }
    }
    publish(&shared, &notify, State::Stopped, RegistrarRole::Unknown, None);
}

fn publish(shared: &Shared, notify: &Notify, state: State, role: RegistrarRole, notice: Option<String>) {
    {
        let mut s = shared.lock_snap();
        s.generation = s.generation.wrapping_add(1);
        s.state = state;
        s.registrar = role;
        s.notice = notice;
        s.activation_request = None;
    }
    notify();
}

/// Waits for `timeout` (forever when `None`) or a stop; commands arriving meanwhile are dropped
/// (they name menus that are not there). Returns true when the worker should exit.
fn wait_stop(shared: &Shared, wake: &mut UnixStream, rx: &Receiver<Command>, timeout: Option<Duration>) -> bool {
    let end = timeout.map(|t| Instant::now() + t);
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return true;
        }
        loop {
            match rx.try_recv() {
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return true,
            }
        }
        let left = end.map(|e| e.saturating_duration_since(Instant::now()));
        if left == Some(Duration::ZERO) {
            return false;
        }
        let _ = crate::backend::poll_readable(&[wake.as_raw_fd()], left);
        if drain(wake) {
            return true;
        }
    }
}

/// Empties the wake socket; true when the other end is gone (the handle was dropped).
fn drain(wake: &mut UnixStream) -> bool {
    let mut buf = [0u8; 64];
    loop {
        match wake.read(&mut buf) {
            Ok(0) => return true,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}

fn s_args(v: &[&str]) -> Vec<u8> {
    bus::strings(v)
}

impl Worker<'_> {
    fn run(&mut self) -> Exit {
        for rule in [
            "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged'",
            "type='signal',interface='com.canonical.AppMenu.Registrar'",
            "type='signal',interface='com.canonical.dbusmenu'",
        ] {
            self.bus_call("AddMatch", "s", s_args(&[rule]), Pending::Ignore);
        }
        self.resolve_registrar();
        if let Some(f) = self.shared.focus.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.focus = f;
        }
        self.apply_focus();
        loop {
            if self.shared.stop.load(Ordering::SeqCst) {
                self.shutdown();
                return Exit::Stop;
            }
            let focus = self.shared.focus.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(f) = focus {
                if f != self.focus {
                    self.focus = f;
                    self.apply_focus();
                }
            }
            for _ in 0..self.cfg.limits.command_queue.max(1) {
                match self.rx.try_recv() {
                    Ok(c) => self.command(c),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.shutdown();
                        return Exit::Stop;
                    }
                }
            }
            match self.bus.fill() {
                Ok(true) => {}
                Ok(false) => return Exit::Lost("the bus closed the connection".into()),
                Err(e) => return Exit::Lost(e.to_string()),
            }
            let mut backlog = false;
            for i in 0..256 {
                match self.bus.next_message() {
                    Ok(Some(m)) => self.message(m),
                    Ok(None) => break,
                    Err(e) => return Exit::Lost(e.to_string()),
                }
                backlog = i == 255;
            }
            self.timers();
            if self.dirty {
                self.dirty = false;
                {
                    let mut s = self.shared.lock_snap();
                    s.generation = s.generation.wrapping_add(1);
                    s.state = self.state.clone();
                    s.registrar = self.role.clone();
                    s.notice = self.notice.clone();
                    s.activation_request = self.activation;
                    s.events_delivered = self.delivered;
                }
                (self.notify)();
            }
            let timeout = if backlog { Some(Duration::ZERO) } else { self.next_deadline().map(|d| d.saturating_duration_since(Instant::now())) };
            match crate::backend::poll_readable(&[self.bus.fd(), self.wake.as_raw_fd()], timeout) {
                Ok(r) if r[1] && drain(self.wake) => {
                    self.shutdown();
                    return Exit::Stop;
                }
                Ok(_) => {}
                Err(e) => return Exit::Lost(e.to_string()),
            }
        }
    }

    fn shutdown(&mut self) {
        if self.role == RegistrarRole::Owned {
            let mut m = bus::call(BUS, BUS_PATH, BUS, "ReleaseName", "s", s_args(&[registrar::NAME]));
            m.flags |= FLAG_NO_REPLY_EXPECTED;
            let _ = self.bus.send(&m);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        let mut d = self.pending.values().map(|c| c.deadline).min();
        for t in [self.registrar_retry, self.source.as_ref().and_then(|s| s.refetch_at)].into_iter().flatten() {
            d = Some(d.map_or(t, |x| x.min(t)));
        }
        d
    }

    // ------------------------------------------------------------------ outgoing calls

    #[allow(clippy::too_many_arguments)]
    fn rpc(&mut self, dest: &str, path: &str, iface: &str, member: &str, sig: &str, body: Vec<u8>, expect: &str, what: Pending) -> bool {
        if self.pending.len() >= self.cfg.limits.max_pending {
            return false;
        }
        match self.bus.send(&bus::call(dest, path, iface, member, sig, body)) {
            Ok(serial) => {
                self.pending.insert(serial, Call { deadline: Instant::now() + self.cfg.limits.call_timeout, expect: expect.into(), what });
                true
            }
            Err(_) => false,
        }
    }

    fn bus_call(&mut self, member: &str, sig: &str, body: Vec<u8>, what: Pending) -> bool {
        self.rpc(BUS, BUS_PATH, BUS, member, sig, body, BUS, what)
    }

    fn signal(&mut self, member: &str, sig: &str, body: Vec<u8>) {
        let _ = self.bus.send(&Message::signal(registrar::PATH, registrar::IFACE, member, sig, body));
    }

    fn reply(&mut self, call: &Message, sig: &str, body: Vec<u8>) {
        if call.flags & FLAG_NO_REPLY_EXPECTED == 0 {
            let _ = self.bus.send(&Message::reply_to(call, sig, body));
        }
    }

    fn error(&mut self, call: &Message, name: &str, text: &str) {
        if call.flags & FLAG_NO_REPLY_EXPECTED == 0 {
            let _ = self.bus.send(&Message::error_to(call, name, text));
        }
    }

    // ------------------------------------------------------------------ registrar

    fn resolve_registrar(&mut self) {
        self.registrar_retry = None;
        if !self.bus_call("GetNameOwner", "s", s_args(&[registrar::NAME]), Pending::RegistrarOwner) {
            self.registrar_retry = Some(Instant::now() + Duration::from_secs(1));
        }
    }

    fn request_registrar(&mut self) {
        if self.cfg.policy == RegistrarPolicy::ObserveOnly {
            self.set_role(RegistrarRole::Absent);
            return;
        }
        let mut w = Writer::default();
        w.string(registrar::NAME);
        w.u32(REQUEST_FLAGS);
        if !self.bus_call("RequestName", "su", w.buf, Pending::RequestRegistrar) {
            self.registrar_retry = Some(Instant::now() + Duration::from_secs(1));
        }
    }

    fn set_role(&mut self, role: RegistrarRole) {
        if role == self.role {
            return;
        }
        if self.role == RegistrarRole::Owned {
            // Registrations were made with this process; a new owner starts from its own table.
            self.table.clear();
        }
        self.role = role;
        self.dirty = true;
        self.apply_focus();
    }

    fn registrar_owner_changed(&mut self, new: &str) {
        match (&self.role, new.is_empty()) {
            _ if new == self.bus.unique => self.set_role(RegistrarRole::Owned),
            (_, true) => {
                // The owner left (or was never there): take over the name if nobody else does.
                self.set_role(RegistrarRole::Unknown);
                self.request_registrar();
            }
            (_, false) => self.set_role(RegistrarRole::Observing { owner: new.into() }),
        }
    }

    fn registrar_call(&mut self, m: &Message) {
        let args = m.args().unwrap_or_default();
        let window = || match args.first() {
            Some(Value::U32(w)) => Some(*w),
            _ => None,
        };
        match (m.iface.as_str(), m.member.as_str(), m.sig.as_str()) {
            (registrar::IFACE, "RegisterWindow", "uo") => {
                let (Some(win), Some(path)) = (window(), args.get(1).and_then(Value::as_str)) else { return self.error(m, "org.freedesktop.DBus.Error.InvalidArgs", "expected (uo)") };
                let path = path.to_string();
                match self.table.register(win, &m.sender, &path) {
                    Ok(changed) => {
                        self.reply(m, "", Vec::new());
                        if changed {
                            let mut w = Writer::default();
                            w.u32(win);
                            w.string(&m.sender);
                            w.string(&path);
                            self.signal("WindowRegistered", "uso", w.buf);
                            if self.focused_window() == Some(win) {
                                self.epoch += 1;
                                self.set_source(win, m.sender.clone(), path);
                            }
                        }
                    }
                    Err(r) => self.error(m, r.error_name(), "registration refused"),
                }
            }
            (registrar::IFACE, "UnregisterWindow", "u") => {
                let Some(win) = window() else { return };
                let removed = self.table.unregister(win, &m.sender);
                self.reply(m, "", Vec::new());
                if removed {
                    self.window_gone(win);
                }
            }
            (registrar::IFACE, "GetMenuForWindow", "u") => {
                let Some(win) = window() else { return };
                let (service, path) = self.table.get(win).map(|r| (r.service.clone(), r.path.clone())).unwrap_or_else(|| (String::new(), "/".into()));
                let mut w = Writer::default();
                w.string(&service);
                w.string(&path);
                self.reply(m, "so", w.buf);
            }
            (registrar::IFACE, "GetMenus", "") => {
                let mut w = Writer::default();
                let rows: Vec<(u32, String, String)> = self.table.iter().map(|(win, r)| (win, r.service.clone(), r.path.clone())).collect();
                w.array(8, |w| {
                    for (win, s, p) in &rows {
                        w.align(8);
                        w.u32(*win);
                        w.string(s);
                        w.string(p);
                    }
                });
                self.reply(m, "a(uso)", w.buf);
            }
            ("org.freedesktop.DBus.Introspectable", "Introspect", "") | ("", "Introspect", "") => {
                let mut w = Writer::default();
                w.string(if m.path == "/" { "<node><node name='com'/></node>" } else { registrar::INTROSPECT });
                self.reply(m, "s", w.buf);
            }
            ("org.freedesktop.DBus.Peer", "Ping", "") => self.reply(m, "", Vec::new()),
            (registrar::IFACE, "RegisterWindow" | "UnregisterWindow" | "GetMenuForWindow" | "GetMenus", _) => self.error(m, "org.freedesktop.DBus.Error.InvalidArgs", "wrong argument types"),
            _ => self.error(m, "org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
        }
    }

    fn window_gone(&mut self, win: u32) {
        let mut w = Writer::default();
        w.u32(win);
        self.signal("WindowUnregistered", "u", w.buf);
        if self.source.as_ref().is_some_and(|s| s.window == win) {
            self.epoch += 1;
            self.source = None;
            self.set_state(State::NotExported { window: win });
        }
    }

    // ------------------------------------------------------------------ focus and menus

    fn focused_window(&self) -> Option<u32> {
        match self.focus {
            Focus::X11 { window, .. } if window != 0 => Some(window),
            _ => None,
        }
    }

    fn set_state(&mut self, s: State) {
        if s != self.state {
            self.state = s;
            self.dirty = true;
        }
    }

    fn apply_focus(&mut self) {
        self.epoch += 1;
        self.source = None;
        self.activation = None;
        match &self.focus {
            Focus::None => self.set_state(State::NoFocus),
            Focus::Wayland { .. } => self.set_state(State::Unavailable(Unavailable::Wayland)),
            Focus::X11 { window: 0, .. } => self.set_state(State::NoFocus),
            Focus::X11 { window, .. } => {
                let window = *window;
                self.lookup(window);
            }
        }
    }

    fn lookup(&mut self, window: u32) {
        match self.role.clone() {
            RegistrarRole::Owned => match self.table.get(window).cloned() {
                Some(r) => self.set_source(window, r.service, r.path),
                None => self.set_state(State::NotExported { window }),
            },
            RegistrarRole::Observing { owner } => {
                let mut w = Writer::default();
                w.u32(window);
                let epoch = self.epoch;
                if self.rpc(registrar::NAME, registrar::PATH, registrar::IFACE, "GetMenuForWindow", "u", w.buf, &owner, Pending::MenuForWindow { epoch, window }) {
                    self.set_state(State::Loading { window });
                } else {
                    self.set_state(State::Failed { window, reason: "Too many menu requests are pending.".into() });
                }
            }
            RegistrarRole::Unknown => self.set_state(State::Loading { window }),
            RegistrarRole::Absent => self.set_state(State::Unavailable(Unavailable::NoRegistrar("no process owns com.canonical.AppMenu.Registrar".into()))),
        }
    }

    fn set_source(&mut self, window: u32, service: String, path: String) {
        if service.is_empty() || path == "/" {
            self.source = None;
            return self.set_state(State::NotExported { window });
        }
        if !bus::valid_name(&service) || !bus::valid_path(&path) {
            self.source = None;
            return self.set_state(State::Failed { window, reason: "The registrar returned an invalid menu address.".into() });
        }
        let owner = if service.starts_with(':') { service.clone() } else { String::new() };
        self.source = Some(Source { window, service: service.clone(), owner: owner.clone(), path, tree: None, inflight: false, dirty: false, refetch_at: None });
        self.set_state(State::Loading { window });
        if owner.is_empty() {
            let epoch = self.epoch;
            if !self.bus_call("GetNameOwner", "s", s_args(&[&service]), Pending::ResolveOwner { epoch }) {
                self.set_state(State::Failed { window, reason: "Too many menu requests are pending.".into() });
            }
        } else {
            self.fetch_layout();
        }
    }

    fn fetch_layout(&mut self) {
        let epoch = self.epoch;
        let Some(src) = self.source.as_mut() else { return };
        src.refetch_at = None;
        if src.inflight {
            src.dirty = true;
            return;
        }
        if src.owner.is_empty() {
            return;
        }
        let (owner, path, window) = (src.owner.clone(), src.path.clone(), src.window);
        let mut w = Writer::default();
        bus::i32(&mut w, 0);
        bus::i32(&mut w, -1);
        w.array(4, |_| {});
        if self.rpc(&owner, &path, DBUSMENU, "GetLayout", "iias", w.buf, &owner, Pending::Layout { epoch }) {
            if let Some(s) = self.source.as_mut() {
                s.inflight = true;
            }
        } else if self.source.as_ref().is_some_and(|s| s.tree.is_none()) {
            self.set_state(State::Failed { window, reason: "Too many menu requests are pending.".into() });
        } else if let Some(s) = self.source.as_mut() {
            s.refetch_at = Some(Instant::now() + self.cfg.limits.layout_debounce * 4);
        }
    }

    fn schedule_refetch(&mut self) {
        let debounce = self.cfg.limits.layout_debounce;
        if let Some(s) = self.source.as_mut() {
            if s.inflight {
                s.dirty = true;
            } else if s.refetch_at.is_none() {
                s.refetch_at = Some(Instant::now() + debounce);
            }
        }
    }

    fn publish_tree(&mut self, tree: Arc<super::MenuTree>) {
        let token = MenuToken(self.epoch);
        let Some(src) = self.source.as_mut() else { return };
        src.tree = Some(tree.clone());
        let e = Exported { token, window: src.window, service: src.service.clone(), owner: src.owner.clone(), path: src.path.clone(), tree };
        self.set_state(State::Ready(Arc::new(e)));
    }

    fn command(&mut self, c: Command) {
        let (token, id) = match c {
            Command::Opened { token, id } | Command::Closed { token, id } | Command::Activate { token, id, .. } => (token, id),
        };
        if token != MenuToken(self.epoch) {
            return;
        }
        let Some(src) = self.source.as_ref() else { return };
        let Some(tree) = src.tree.clone() else { return };
        let (owner, path) = (src.owner.clone(), src.path.clone());
        let epoch = self.epoch;
        let event = |w: &mut Writer, name: &str, timestamp: u32| {
            bus::i32(w, id);
            w.string(name);
            w.sig("i");
            bus::i32(w, 0);
            w.u32(timestamp);
        };
        match c {
            Command::Opened { .. } => {
                if !tree.openable(id) {
                    return;
                }
                let mut w = Writer::default();
                event(&mut w, "opened", 0);
                self.rpc(&owner, &path, DBUSMENU, "Event", "isvu", w.buf, &owner, Pending::Ignore);
                let mut w = Writer::default();
                bus::i32(&mut w, id);
                self.rpc(&owner, &path, DBUSMENU, "AboutToShow", "i", w.buf, &owner, Pending::AboutToShow { epoch });
            }
            Command::Closed { .. } => {
                if !tree.openable(id) {
                    return;
                }
                let mut w = Writer::default();
                event(&mut w, "closed", 0);
                self.rpc(&owner, &path, DBUSMENU, "Event", "isvu", w.buf, &owner, Pending::Ignore);
            }
            Command::Activate { timestamp, .. } => {
                if !tree.activatable(id) {
                    self.notice = Some("That menu item is no longer available.".into());
                    self.dirty = true;
                    return;
                }
                let mut w = Writer::default();
                event(&mut w, "clicked", timestamp);
                if !self.rpc(&owner, &path, DBUSMENU, "Event", "isvu", w.buf, &owner, Pending::Click { epoch }) {
                    self.notice = Some("The menu command could not be sent.".into());
                    self.dirty = true;
                }
            }
        }
    }

    // ------------------------------------------------------------------ incoming

    fn message(&mut self, m: Message) {
        match m.kind {
            dbus::RETURN | dbus::ERROR => self.reply_in(m),
            dbus::SIGNAL => self.signal_in(m),
            dbus::CALL => {
                if self.role == RegistrarRole::Owned && (m.path == registrar::PATH || (m.path == "/" && m.member == "Introspect")) {
                    self.registrar_call(&m);
                } else if m.iface == "org.freedesktop.DBus.Peer" && m.member == "Ping" {
                    self.reply(&m, "", Vec::new());
                } else {
                    self.error(&m, &format!("{DBUS_ERR}.UnknownObject"), "no such object");
                }
            }
            _ => {}
        }
    }

    fn reply_in(&mut self, m: Message) {
        let Some(call) = self.pending.get(&m.reply_serial) else { return };
        if call.expect != m.sender {
            return;
        }
        let Some(call) = self.pending.remove(&m.reply_serial) else { return };
        let ok = m.kind == dbus::RETURN;
        let too_big = m.body.len() > self.cfg.limits.max_reply_bytes;
        let args = if ok && !too_big { m.args().ok() } else { None };
        match call.what {
            Pending::RegistrarOwner => match args.as_ref().and_then(|a| a.first()).and_then(Value::as_str) {
                Some(owner) if owner.starts_with(':') && owner != self.bus.unique => {
                    let owner = owner.to_string();
                    self.set_role(RegistrarRole::Observing { owner });
                }
                Some(owner) if owner == self.bus.unique => self.set_role(RegistrarRole::Owned),
                _ if m.error == format!("{DBUS_ERR}.NameHasNoOwner") => self.request_registrar(),
                _ => self.registrar_retry = Some(Instant::now() + Duration::from_secs(2)),
            },
            Pending::RequestRegistrar => match args.as_ref().and_then(|a| a.first()).and_then(Value::as_u32) {
                Some(1) | Some(4) => self.set_role(RegistrarRole::Owned),
                // Someone else got it first (or holds it): observe them instead.
                Some(_) => self.resolve_registrar(),
                None => {
                    self.notice = Some(format!("Could not serve the menu registrar: {}", if m.error.is_empty() { "malformed reply" } else { &m.error }));
                    self.set_role(RegistrarRole::Absent);
                }
            },
            Pending::MenuForWindow { epoch, window } => {
                if epoch != self.epoch {
                    return;
                }
                let pair = args.as_ref().filter(|_| m.sig == "so").and_then(|a| Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_str()?.to_string())));
                match pair {
                    Some((service, path)) => self.set_source(window, service, path),
                    // KDE-style registrars answer unknown windows with an error, others with ("", "/").
                    None if !ok => self.set_state(State::NotExported { window }),
                    None => self.set_state(State::Failed { window, reason: "The menu registrar sent a malformed reply.".into() }),
                }
            }
            Pending::ResolveOwner { epoch } => {
                if epoch != self.epoch {
                    return;
                }
                let window = self.source.as_ref().map_or(0, |s| s.window);
                match args.as_ref().and_then(|a| a.first()).and_then(Value::as_str).filter(|o| o.starts_with(':')) {
                    Some(owner) => {
                        let owner = owner.to_string();
                        if let Some(s) = self.source.as_mut() {
                            s.owner = owner;
                        }
                        self.fetch_layout();
                    }
                    None => {
                        self.source = None;
                        self.set_state(State::NotExported { window });
                    }
                }
            }
            Pending::Layout { epoch } => {
                if epoch != self.epoch {
                    return;
                }
                let Some(src) = self.source.as_mut() else { return };
                src.inflight = false;
                let window = src.window;
                let had_tree = src.tree.is_some();
                let again = std::mem::take(&mut src.dirty);
                let parsed = if too_big {
                    Err("The application's menu is too large.".to_string())
                } else if !ok {
                    Err(format!("The application's menu could not be read ({}).", m.error))
                } else if m.sig != "u(ia{sv}av)" {
                    Err("The application sent a malformed menu.".to_string())
                } else {
                    match args.as_deref().map(|a| super::MenuTree::from_layout(a, &self.cfg.limits)) {
                        Some(Ok(t)) => Ok(t),
                        Some(Err(e)) => Err(format!("The application sent a malformed menu ({e}).")),
                        None => Err("The application sent a malformed menu.".to_string()),
                    }
                };
                match parsed {
                    Ok(tree) => self.publish_tree(Arc::new(tree)),
                    Err(reason) if had_tree => {
                        self.notice = Some(reason);
                        self.dirty = true;
                    }
                    Err(reason) => self.set_state(State::Failed { window, reason }),
                }
                if again {
                    self.schedule_refetch();
                }
            }
            Pending::AboutToShow { epoch } => {
                if epoch == self.epoch && args.as_ref().and_then(|a| a.first()).and_then(Value::as_bool) == Some(true) {
                    self.schedule_refetch();
                }
            }
            Pending::Click { epoch } => {
                if epoch != self.epoch {
                    return;
                }
                if ok {
                    self.delivered += 1;
                    self.notice = None;
                } else {
                    self.notice = Some(format!("The application rejected the menu command ({}).", m.error));
                }
                self.dirty = true;
            }
            Pending::Ignore => {}
        }
    }

    fn signal_in(&mut self, m: Message) {
        let args = || m.args().unwrap_or_default();
        if m.sender == BUS && m.iface == BUS {
            match (m.member.as_str(), m.sig.as_str()) {
                ("NameOwnerChanged", "sss") => {
                    let a = args();
                    let s: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
                    if let [name, old, new] = s[..] {
                        self.owner_changed(name, old, new);
                    }
                }
                ("NameLost", "s") if self.role == RegistrarRole::Owned && args().first().and_then(Value::as_str) == Some(registrar::NAME) => {
                    self.set_role(RegistrarRole::Unknown);
                    self.resolve_registrar();
                }
                _ => {}
            }
            return;
        }
        if m.iface == registrar::IFACE && m.path == registrar::PATH {
            let RegistrarRole::Observing { owner } = &self.role else { return };
            if &m.sender != owner {
                return;
            }
            let a = args();
            let focused = self.focused_window();
            match (m.member.as_str(), m.sig.as_str()) {
                ("WindowRegistered", "uso") => {
                    if let (Some(Value::U32(w)), Some(s), Some(p)) = (a.first(), a.get(1).and_then(Value::as_str), a.get(2).and_then(Value::as_str)) {
                        if Some(*w) == focused {
                            let (w, s, p) = (*w, s.to_string(), p.to_string());
                            self.epoch += 1;
                            self.set_source(w, s, p);
                        }
                    }
                }
                ("WindowUnregistered", "u") => {
                    if let Some(Value::U32(w)) = a.first() {
                        if Some(*w) == focused && self.source.as_ref().is_some_and(|s| s.window == *w) {
                            let w = *w;
                            self.epoch += 1;
                            self.source = None;
                            self.set_state(State::NotExported { window: w });
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if m.iface == DBUSMENU {
            let Some(src) = self.source.as_ref() else { return };
            if src.owner.is_empty() || m.sender != src.owner || m.path != src.path {
                return;
            }
            match (m.member.as_str(), m.sig.as_str()) {
                ("LayoutUpdated", "ui") => self.schedule_refetch(),
                ("ItemsPropertiesUpdated", "a(ia{sv})a(ias)") => {
                    if m.body.len() > self.cfg.limits.max_reply_bytes {
                        return self.schedule_refetch();
                    }
                    let a = args();
                    match (src.inflight, src.tree.clone(), a.first(), a.get(1)) {
                        (false, Some(tree), Some(up), Some(rm)) => {
                            if let Some(next) = tree.with_updates(up, rm, &self.cfg.limits) {
                                self.publish_tree(Arc::new(next));
                            }
                        }
                        _ => self.schedule_refetch(),
                    }
                }
                ("ItemActivationRequested", "iu") => {
                    let a = args();
                    if let (Some(Value::I32(id)), Some(Value::U32(ts))) = (a.first(), a.get(1)) {
                        if src.tree.as_ref().is_some_and(|t| t.find(*id).is_some()) {
                            self.activation_seq += 1;
                            self.activation = Some(ActivationRequest { token: MenuToken(self.epoch), id: *id, timestamp: *ts, seq: self.activation_seq });
                            self.dirty = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn owner_changed(&mut self, name: &str, old: &str, new: &str) {
        if name == registrar::NAME {
            if self.role == RegistrarRole::Owned && new != self.bus.unique {
                self.set_role(RegistrarRole::Unknown);
            }
            return self.registrar_owner_changed(new);
        }
        if name.starts_with(':') && !old.is_empty() && new.is_empty() {
            if self.role == RegistrarRole::Owned {
                for win in self.table.drop_sender(name) {
                    self.window_gone(win);
                }
            }
            if let Some(src) = &self.source {
                if src.owner == name {
                    let window = src.window;
                    self.epoch += 1;
                    self.source = None;
                    self.set_state(State::NotExported { window });
                }
            }
            return;
        }
        // A well-known exporter name moved to another connection: resolve it again.
        if let Some(src) = &self.source {
            if src.service == name && !src.service.starts_with(':') && src.owner != new {
                let (w, s, p) = (src.window, src.service.clone(), src.path.clone());
                self.epoch += 1;
                self.set_source(w, s, p);
            }
        }
    }

    fn timers(&mut self) {
        let now = Instant::now();
        let expired: Vec<u32> = self.pending.iter().filter(|(_, c)| c.deadline <= now).map(|(s, _)| *s).collect();
        for serial in expired {
            let Some(call) = self.pending.remove(&serial) else { continue };
            match call.what {
                Pending::RegistrarOwner | Pending::RequestRegistrar => self.registrar_retry = Some(now + Duration::from_secs(2)),
                Pending::MenuForWindow { epoch, window } if epoch == self.epoch => {
                    self.set_state(State::Failed { window, reason: "The menu registrar did not answer.".into() });
                }
                Pending::ResolveOwner { epoch } | Pending::Layout { epoch } if epoch == self.epoch => {
                    let Some(src) = self.source.as_mut() else { continue };
                    src.inflight = false;
                    let window = src.window;
                    if src.tree.is_some() {
                        self.notice = Some("The application did not answer a menu update.".into());
                        self.dirty = true;
                    } else {
                        self.set_state(State::Failed { window, reason: "The application did not answer for its menu.".into() });
                    }
                }
                Pending::Click { epoch } if epoch == self.epoch => {
                    self.notice = Some("The application did not answer the menu command.".into());
                    self.dirty = true;
                }
                _ => {}
            }
        }
        if self.registrar_retry.is_some_and(|t| t <= now) {
            self.resolve_registrar();
        }
        if self.source.as_ref().and_then(|s| s.refetch_at).is_some_and(|t| t <= now) {
            self.fetch_layout();
        }
    }
}
