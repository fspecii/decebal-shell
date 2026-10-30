//! decnotify: a freedesktop notification daemon (org.freedesktop.Notifications) with macOS-style banners.
//!
//! The bus side lives here (method dispatch, signals, the event loop); drawing and input are in [`ui`].
//! The daemon exits quietly when another daemon (mako, dunst, ...) already owns the name, so it can be
//! started unconditionally from an X11 session script.

pub mod model;
pub mod sound;
#[cfg(feature = "x11")]
pub mod ui;

pub use model::{Config, History};

use crate::dbus::{self, Conn, Message, Value, Writer};
use model::{parse_notify, HistoryItem};

pub const BUS_NAME: &str = "org.freedesktop.Notifications";
pub const PATH: &str = "/org/freedesktop/Notifications";
/// decebal's own interface on the same object (history panel, do-not-disturb), driven by `decnotify`.
pub const EXT_IFACE: &str = "org.decebal.Notifications";

const INTROSPECT: &str = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.freedesktop.DBus.Introspectable"><method name="Introspect"><arg direction="out" type="s"/></method></interface>
  <interface name="org.freedesktop.DBus.Peer"><method name="Ping"/><method name="GetMachineId"><arg direction="out" type="s"/></method></interface>
  <interface name="org.freedesktop.Notifications">
    <method name="GetCapabilities"><arg direction="out" type="as"/></method>
    <method name="Notify">
      <arg direction="in" type="s" name="app_name"/><arg direction="in" type="u" name="replaces_id"/><arg direction="in" type="s" name="app_icon"/>
      <arg direction="in" type="s" name="summary"/><arg direction="in" type="s" name="body"/><arg direction="in" type="as" name="actions"/>
      <arg direction="in" type="a{sv}" name="hints"/><arg direction="in" type="i" name="expire_timeout"/><arg direction="out" type="u" name="id"/>
    </method>
    <method name="CloseNotification"><arg direction="in" type="u" name="id"/></method>
    <method name="GetServerInformation"><arg direction="out" type="s"/><arg direction="out" type="s"/><arg direction="out" type="s"/><arg direction="out" type="s"/></method>
    <signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
    <signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
  </interface>
  <interface name="org.decebal.Notifications">
    <method name="ToggleHistory"/><method name="ShowHistory"/><method name="ClearHistory"/>
    <method name="RetryHistory"/>
    <method name="GetHistoryStatus"><arg direction="out" type="s"/><arg direction="out" type="b"/><arg direction="out" type="b"/></method>
    <method name="GetDnd"><arg direction="out" type="b"/></method><method name="SetDnd"><arg direction="in" type="b"/></method>
  </interface>
</node>"#;

/// State shared between the bus side and the UI: settings and the persisted history.
pub struct Shared {
    pub cfg: Config,
    pub history: History,
}

/// What a bus call asks the UI to do, decided by [`dispatch`] (kept separate so it can be tested without X).
#[derive(Debug, PartialEq)]
pub enum Effect {
    Show(model::Notification),
    Sound(sound::Request),
    Close(u32),
    ToggleHistory,
    ShowHistory,
}

pub struct Daemon {
    next_id: u32,
}

fn bool_body(b: bool) -> Vec<u8> {
    let mut w = Writer::default();
    w.u32(u32::from(b));
    w.buf
}

impl Daemon {
    pub fn new() -> Daemon {
        Daemon { next_id: 1 }
    }

    fn sync_history_ids(&mut self, history: &History) {
        self.next_id = self.next_id.max(history.items.iter().map(|i| i.id).max().unwrap_or(0).saturating_add(1)).max(1);
    }
    fn fresh_id(&mut self, history: &History) -> u32 {
        loop {
            let id = self.next_id.max(1);
            self.next_id = id.checked_add(1).unwrap_or(1);
            if !history.items.iter().any(|item| item.id == id) {
                return id;
            }
        }
    }

    /// Answers one method call: the reply to send (None for no-reply calls) and the UI effects.
    pub fn dispatch(&mut self, m: &Message, sh: &mut Shared) -> (Option<Message>, Vec<Effect>) {
        self.sync_history_ids(&sh.history);
        let mut fx = Vec::new();
        let reply = match (m.iface.as_str(), m.member.as_str()) {
            (BUS_NAME, "Notify") => match m.args().ok().as_deref().and_then(parse_notify) {
                Some(mut n) => {
                    sh.cfg.refresh();
                    let live = n.replaces != 0 && n.replaces < self.next_id;
                    n.id = if live { n.replaces } else { self.fresh_id(&sh.history) };
                    let id = n.id;
                    if !sh.cfg.is_muted(&n.app, &n.entry) {
                        if !n.transient {
                            sh.history.push(HistoryItem::from(&n));
                        }
                        if sh.cfg.sounds && !sh.cfg.dnd && n.replaces == 0 {
                            if let Some(request) = m.args().ok().and_then(|a| sound::request(&a, n.lifetime(sh.cfg.banner_seconds))) {
                                fx.push(Effect::Sound(request));
                            }
                        }
                        if !sh.cfg.dnd || n.urgency >= 2 {
                            fx.push(Effect::Show(n));
                        }
                    }
                    Message::reply_to(m, "u", dbus::body_u32(id))
                }
                None => Message::error_to(m, "org.freedesktop.DBus.Error.InvalidArgs", "Notify expects (susssasa{sv}i)"),
            },
            (BUS_NAME, "CloseNotification") => {
                if let Some(Value::U32(id)) = m.args().ok().and_then(|a| a.into_iter().next()) {
                    sh.history.remove(id);
                    fx.push(Effect::Close(id));
                }
                Message::reply_to(m, "", Vec::new())
            }
            (BUS_NAME, "GetCapabilities") => {
                let mut caps = vec!["actions", "body", "icon-static"];
                if sound::backend().is_some() {
                    caps.push("sound");
                }
                Message::reply_to(m, "as", dbus::body_strings(&caps))
            }
            (BUS_NAME, "GetServerInformation") => {
                let mut w = Writer::default();
                for s in ["decnotify", "decebal", env!("CARGO_PKG_VERSION"), "1.2"] {
                    w.string(s);
                }
                Message::reply_to(m, "ssss", w.buf)
            }
            ("org.freedesktop.DBus.Introspectable", "Introspect") => {
                let mut w = Writer::default();
                w.string(INTROSPECT);
                Message::reply_to(m, "s", w.buf)
            }
            ("org.freedesktop.DBus.Peer", "Ping") => Message::reply_to(m, "", Vec::new()),
            ("org.freedesktop.DBus.Peer", "GetMachineId") => {
                let id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
                let mut w = Writer::default();
                w.string(id.trim());
                Message::reply_to(m, "s", w.buf)
            }
            (EXT_IFACE, "ToggleHistory") => {
                fx.push(Effect::ToggleHistory);
                Message::reply_to(m, "", Vec::new())
            }
            (EXT_IFACE, "ShowHistory") => {
                fx.push(Effect::ShowHistory);
                Message::reply_to(m, "", Vec::new())
            }
            (EXT_IFACE, "ClearHistory") => match sh.history.clear() {
                Ok(()) => Message::reply_to(m, "", Vec::new()),
                Err(e) => Message::error_to(m, "org.freedesktop.DBus.Error.Failed", &e),
            },
            (EXT_IFACE, "RetryHistory") => match sh.history.retry() {
                Ok(()) => {
                    self.sync_history_ids(&sh.history);
                    Message::reply_to(m, "", Vec::new())
                }
                Err(e) => Message::error_to(m, "org.freedesktop.DBus.Error.Failed", &e),
            },
            (EXT_IFACE, "GetHistoryStatus") => {
                let mut w = Writer::default();
                w.string(sh.history.error().unwrap_or(""));
                w.u32(u32::from(sh.history.pending()));
                w.u32(u32::from(sh.history.clear_pending()));
                Message::reply_to(m, "sbb", w.buf)
            }
            (EXT_IFACE, "GetDnd") => {
                sh.cfg.refresh();
                Message::reply_to(m, "b", bool_body(sh.cfg.dnd))
            }
            (EXT_IFACE, "SetDnd") => match m.args().ok().and_then(|a| a.first().and_then(Value::as_bool)) {
                Some(on) => match Config::save_dnd(on) {
                    Ok(()) => {
                        sh.cfg.dnd = on;
                        Message::reply_to(m, "", Vec::new())
                    }
                    Err(e) => Message::error_to(m, "org.freedesktop.DBus.Error.Failed", &e.to_string()),
                },
                None => Message::error_to(m, "org.freedesktop.DBus.Error.InvalidArgs", "SetDnd expects a boolean"),
            },
            _ => Message::error_to(m, "org.freedesktop.DBus.Error.UnknownMethod", &format!("no method {}.{}", m.iface, m.member)),
        };
        // Push/retry/remove may merge durable entries that were unavailable at startup.
        self.sync_history_ids(&sh.history);
        // flags bit 0 = NO_REPLY_EXPECTED
        let reply = if m.flags & 1 != 0 { None } else { Some(reply) };
        (reply, fx)
    }
}

impl Default for Daemon {
    fn default() -> Self {
        Self::new()
    }
}

fn signal_closed(id: u32, reason: u32) -> Message {
    let mut w = Writer::default();
    w.u32(id);
    w.u32(reason);
    Message::signal(PATH, BUS_NAME, "NotificationClosed", "uu", w.buf)
}

fn signal_action(id: u32, key: &str) -> Message {
    let mut w = Writer::default();
    w.u32(id);
    w.string(key);
    Message::signal(PATH, BUS_NAME, "ActionInvoked", "us", w.buf)
}

/// Result of trying to become the notification daemon.
pub enum Start {
    Ran,
    /// Another daemon owns the name; nothing was started.
    NameTaken,
}

/// Runs the daemon until the bus or the X server goes away.
#[cfg(feature = "x11")]
pub fn run_daemon() -> Result<Start, String> {
    use std::time::Instant;
    let mut conn = Conn::session().map_err(|e| format!("session bus: {e}"))?;
    if !conn.request_name(BUS_NAME, false).map_err(|e| e.to_string())? {
        return Ok(Start::NameTaken);
    }
    let mut ui = ui::Ui::connect()?;
    conn.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut sh = Shared { cfg: Config::load(), history: History::load() };
    let mut daemon = Daemon::new();
    let mut sounds: Option<sound::Player> = None;
    // Start id numbering above anything persisted so a restart never reuses a visible id.
    daemon.sync_history_ids(&sh.history);

    loop {
        let timeout = ui.next_deadline().map(|d| d.saturating_duration_since(Instant::now()));
        let ready = crate::backend::poll_readable(&[conn.fd(), ui.fd(), ui.theme_fd()], timeout).map_err(|e| e.to_string())?;
        let mut events = Vec::new();
        if ready[2] {
            ui.refresh_theme(&sh);
        }
        if ready[0] {
            if !conn.fill().map_err(|e| e.to_string())? {
                return Ok(Start::Ran); // bus went away: the session is ending
            }
            while let Some(msg) = conn.next_message().map_err(|e| e.to_string())? {
                if msg.kind != dbus::CALL {
                    if msg.kind == dbus::SIGNAL && msg.member == "NameLost" {
                        return Ok(Start::Ran);
                    }
                    continue;
                }
                let (reply, effects) = daemon.dispatch(&msg, &mut sh);
                daemon.sync_history_ids(&sh.history);
                if !sh.cfg.sounds || sh.cfg.dnd {
                    sounds.take();
                }
                if matches!(msg.member.as_str(), "Notify" | "CloseNotification" | "ClearHistory" | "RetryHistory") {
                    ui.refresh_history(&sh);
                }
                if let Some(r) = reply {
                    conn.send(&r).map_err(|e| e.to_string())?;
                }
                for fx in effects {
                    match fx {
                        Effect::Show(n) => ui.show(n, sh.cfg.banner_seconds),
                        Effect::Sound(request) => sounds.get_or_insert_with(sound::Player::new).submit(request),
                        Effect::Close(id) => events.extend(ui.close(id, 3)),
                        Effect::ToggleHistory => events.extend(ui.toggle_history(&sh)),
                        Effect::ShowHistory if !ui.history_open() => events.extend(ui.toggle_history(&sh)),
                        Effect::ShowHistory => {}
                    }
                }
            }
        }
        events.extend(ui.pump(&mut sh)?);
        if !sh.cfg.sounds || sh.cfg.dnd {
            sounds.take();
        }
        daemon.sync_history_ids(&sh.history);
        events.extend(ui.expire(Instant::now()));
        for ev in events {
            let sig = match ev {
                ui::UiEvent::Closed(id, reason) => signal_closed(id, reason),
                ui::UiEvent::Action(id, key) => signal_action(id, &key),
            };
            conn.send(&sig).map_err(|e| e.to_string())?;
        }
        crate::mem::trim();
    }
}

// ---------------------------------------------------------------- CLI side

/// Calls a method of our extension interface on whoever owns the notification name.
pub fn ext_call(member: &str, sig: &str, body: Vec<u8>) -> Result<Message, String> {
    let mut conn = Conn::session().map_err(|e| format!("session bus: {e}"))?;
    conn.call_blocking(Message::call(BUS_NAME, PATH, EXT_IFACE, member, sig, body)).map_err(|e| e.to_string())
}

/// `Some(dnd)` when a decnotify daemon is running, `None` for any other daemon or none at all.
pub fn running_dnd() -> Option<bool> {
    ext_call("GetDnd", "", Vec::new()).ok()?.args().ok()?.first().and_then(Value::as_bool)
}

/// Sends a notification through the bus like `notify-send` would.
pub fn send_test(summary: &str, body: &str) -> Result<u32, String> {
    send_with("decnotify", "dialog-information", summary, body, true)
}

/// Sends a notification from `app` (with a theme icon name) through the bus; the daemon's default timeout applies.
pub fn send(app: &str, icon: &str, summary: &str, body: &str) -> Result<u32, String> {
    send_with(app, icon, summary, body, false)
}

fn send_with(app: &str, icon: &str, summary: &str, body: &str, default_action: bool) -> Result<u32, String> {
    let mut conn = Conn::session().map_err(|e| format!("session bus: {e}"))?;
    let mut w = Writer::default();
    w.string(app);
    w.u32(0);
    w.string(icon);
    w.string(summary);
    w.string(body);
    w.array(4, |w| {
        if default_action {
            w.string("default");
            w.string("Open");
        }
    });
    w.array(8, |_| {});
    w.u32(u32::MAX); // -1: server default
    let r = conn.call_blocking(Message::call(BUS_NAME, PATH, BUS_NAME, "Notify", "susssasa{sv}i", w.buf)).map_err(|e| e.to_string())?;
    r.args().map_err(|e| e.to_string())?.first().and_then(Value::as_u32).ok_or_else(|| "bad reply".into())
}

/// Sets do-not-disturb on the running decnotify daemon, or in its settings file when none runs.
pub fn set_dnd(on: bool) -> Result<(), String> {
    if running_dnd().is_some() {
        let mut w = Writer::default();
        w.u32(u32::from(on));
        ext_call("SetDnd", "b", w.buf).map(|_| ())
    } else {
        Config::save_dnd(on).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notify_call(app: &str, replaces: u32, urgency: u8, transient: bool) -> Message {
        let mut w = Writer::default();
        w.string(app);
        w.u32(replaces);
        w.string("");
        w.string("Hello");
        w.string("World");
        w.array(4, |_| {});
        w.array(8, |w| {
            w.align(8);
            w.string("urgency");
            w.sig("y");
            w.u8(urgency);
            w.align(8);
            w.string("transient");
            w.sig("b");
            w.u32(u32::from(transient));
        });
        w.u32(u32::MAX);
        let mut m = Message::call(BUS_NAME, PATH, BUS_NAME, "Notify", "susssasa{sv}i", w.buf);
        m.serial = 5;
        m.sender = ":1.9".into();
        m
    }

    fn shared(cfg: Config) -> Shared {
        Shared { cfg, history: History::in_memory() }
    }

    fn reply_id(r: Option<Message>) -> u32 {
        r.unwrap().args().unwrap()[0].as_u32().unwrap()
    }

    #[test]
    fn notify_assigns_ids_shows_and_records_history() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::default()));
        let (r, fx) = d.dispatch(&notify_call("a", 0, 1, false), &mut sh);
        assert_eq!(reply_id(r), 1);
        assert!(matches!(fx.as_slice(), [Effect::Show(n)] if n.id == 1 && n.summary == "Hello"));
        assert_eq!(sh.history.items.len(), 1);
        let (r, _) = d.dispatch(&notify_call("a", 0, 1, false), &mut sh);
        assert_eq!(reply_id(r), 2);
    }

    #[test]
    fn replaces_id_reuses_the_id_and_unknown_replaces_gets_a_new_one() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::default()));
        d.dispatch(&notify_call("a", 0, 1, false), &mut sh);
        let (r, fx) = d.dispatch(&notify_call("a", 1, 1, false), &mut sh);
        assert_eq!(reply_id(r), 1);
        assert!(matches!(fx.as_slice(), [Effect::Show(n)] if n.id == 1));
        assert_eq!(sh.history.items.len(), 1, "replacement updates the history entry");
        let (r, _) = d.dispatch(&notify_call("a", 99, 1, false), &mut sh);
        assert_eq!(reply_id(r), 2);
    }

    #[test]
    fn dnd_hides_banners_but_keeps_history_and_lets_critical_through() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::parse("dnd = true")));
        let (r, fx) = d.dispatch(&notify_call("a", 0, 1, false), &mut sh);
        assert_eq!(reply_id(r), 1);
        assert!(fx.is_empty());
        assert_eq!(sh.history.items.len(), 1);
        let (_, fx) = d.dispatch(&notify_call("a", 0, 2, false), &mut sh);
        assert_eq!(fx.len(), 1, "critical bypasses do-not-disturb");
    }

    #[test]
    fn muted_apps_and_transient_notifications_skip_history() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::parse("muted_apps = [\"Spam\"]")));
        let (r, fx) = d.dispatch(&notify_call("spam", 0, 1, false), &mut sh);
        assert_eq!(reply_id(r), 1, "muted apps still get an id");
        assert!(fx.is_empty() && sh.history.items.is_empty());
        let (_, fx) = d.dispatch(&notify_call("ok", 0, 1, true), &mut sh);
        assert_eq!(fx.len(), 1);
        assert!(sh.history.items.is_empty());
    }

    #[test]
    fn server_info_and_capabilities_roundtrip() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::default()));
        let mut m = Message::call(BUS_NAME, PATH, BUS_NAME, "GetServerInformation", "", vec![]);
        m.serial = 1;
        m.sender = ":1.2".into();
        let (r, _) = d.dispatch(&m, &mut sh);
        let (wire, _) = Message::decode(&r.unwrap().encode(7)).unwrap().unwrap();
        assert_eq!(wire.sig, "ssss");
        assert_eq!(wire.args().unwrap()[0].as_str(), Some("decnotify"));
        m.member = "GetCapabilities".into();
        let (r, _) = d.dispatch(&m, &mut sh);
        let (wire, _) = Message::decode(&r.unwrap().encode(8)).unwrap().unwrap();
        let Value::Array(caps) = &wire.args().unwrap()[0] else { panic!() };
        assert!(caps.iter().any(|c| c.as_str() == Some("actions")));
    }

    #[test]
    fn recovered_history_ids_are_reserved_before_and_after_dispatch() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::default()));
        let (_, effects) = d.dispatch(&notify_call("a", 0, 1, false), &mut sh);
        let Effect::Show(n) = &effects[0] else { panic!() };
        let mut recovered = HistoryItem::from(n);
        recovered.id = 700;
        sh.history.push(recovered.clone());
        let (reply, _) = d.dispatch(&notify_call("b", 0, 1, false), &mut sh);
        assert_eq!(reply_id(reply), 701);
        recovered.id = u32::MAX;
        sh.history.push(recovered);
        let (reply, _) = d.dispatch(&notify_call("c", 0, 1, false), &mut sh);
        let id = reply_id(reply);
        assert_ne!(id, 0);
        assert_ne!(id, u32::MAX);
        assert_eq!(sh.history.items.iter().filter(|i| i.id == id).count(), 1);
    }
    #[test]
    fn history_status_and_retry_have_typed_replies() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::default()));
        let m = Message::call(BUS_NAME, PATH, EXT_IFACE, "GetHistoryStatus", "", vec![]);
        let (reply, _) = d.dispatch(&m, &mut sh);
        let reply = reply.unwrap();
        assert_eq!(reply.sig, "sbb");
        assert_eq!(reply.args().unwrap(), vec![Value::Str(String::new()), Value::Bool(false), Value::Bool(false)]);
        for method in ["RetryHistory", "ClearHistory"] {
            let m = Message::call(BUS_NAME, PATH, EXT_IFACE, method, "", vec![]);
            assert_eq!(d.dispatch(&m, &mut sh).0.unwrap().kind, dbus::RETURN);
        }
    }

    #[test]
    fn close_and_unknown_and_no_reply() {
        let (mut d, mut sh) = (Daemon::new(), shared(Config::default()));
        d.dispatch(&notify_call("a", 0, 1, false), &mut sh);
        let mut w = Writer::default();
        w.u32(1);
        let mut m = Message::call(BUS_NAME, PATH, BUS_NAME, "CloseNotification", "u", w.buf);
        m.serial = 3;
        m.sender = ":1.2".into();
        let (r, fx) = d.dispatch(&m, &mut sh);
        assert!(r.is_some());
        assert_eq!(fx, vec![Effect::Close(1)]);
        assert!(sh.history.items.is_empty());
        m.member = "Nope".into();
        let (r, _) = d.dispatch(&m, &mut sh);
        assert_eq!(r.unwrap().kind, dbus::ERROR);
        m.flags = 1;
        assert!(d.dispatch(&m, &mut sh).0.is_none());
    }
}
