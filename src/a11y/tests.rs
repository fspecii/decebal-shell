//! Bridge + host against an in-process fake bus that also plays the registry and an assistive technology.

use super::atspi::{path_of, ACCESSIBLE, ACTION, APPLICATION, ROOT_PATH, SOCKET};
use super::bridge::{Config, Mode, Phase};
use super::host::Host;
use super::tree::{ActionDef, NodeId, Role, Spec, State, TreeBuilder};
use super::{ActionError, Op, Target};
use crate::dbus::{Message, Value, Writer, CALL, ERROR, RETURN, SIGNAL};
use crate::window_app::{Key, KeyEvent, Response, Waker, WindowApp};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

struct Peer {
    s: UnixStream,
    buf: Vec<u8>,
    serial: u32,
    /// Messages read while looking for something else.
    seen: Vec<Message>,
}

impl Peer {
    fn accept(l: &UnixListener) -> Peer {
        l.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let (mut s, _) = loop {
            match l.accept() {
                Ok(connection) => break connection,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "bridge did not connect to private bus");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("private bus accept: {e}"),
            }
        };
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        let t0 = Instant::now();
        while !line.ends_with(b"\r\n") {
            match s.read(&mut b) {
                Ok(1) => line.push(b[0]),
                _ => assert!(t0.elapsed() < Duration::from_secs(3), "no auth"),
            }
        }
        s.write_all(b"OK 00112233445566778899aabbccddeeff\r\n").unwrap();
        let mut p = Peer { s, buf: Vec::new(), serial: 100, seen: Vec::new() };
        // BEGIN then Hello.
        let t0 = Instant::now();
        while !p.buf.starts_with(b"BEGIN\r\n") {
            p.fill();
            assert!(t0.elapsed() < Duration::from_secs(3), "no BEGIN");
        }
        p.buf.drain(..7);
        let hello = p.expect(|m| m.member == "Hello");
        let mut w = Writer::default();
        w.string(":1.42");
        p.reply(&hello, "s", w.buf);
        p
    }

    fn fill(&mut self) {
        let mut tmp = [0u8; 8192];
        match self.s.read(&mut tmp) {
            Ok(0) => {}
            Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
            Err(_) => {}
        }
    }

    fn next(&mut self, within: Duration) -> Option<Message> {
        let t0 = Instant::now();
        loop {
            if let Some((m, used)) = Message::decode(&self.buf).unwrap() {
                self.buf.drain(..used);
                return Some(m);
            }
            if t0.elapsed() > within {
                return None;
            }
            self.fill();
        }
    }

    fn expect(&mut self, f: impl Fn(&Message) -> bool) -> Message {
        if let Some(i) = self.seen.iter().position(&f) {
            return self.seen.remove(i);
        }
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(3) {
            if let Some(m) = self.next(Duration::from_millis(50)) {
                if f(&m) {
                    return m;
                }
                if m.kind == CALL && m.member == "AddMatch" {
                    self.reply(&m, "", vec![]);
                    continue;
                }
                self.seen.push(m);
            }
        }
        panic!("expected message not received; saw {:?}", self.seen.iter().map(|m| (m.kind, m.member.clone(), m.error.clone())).collect::<Vec<_>>());
    }

    fn send(&mut self, mut m: Message) -> u32 {
        self.serial += 1;
        if m.sender.is_empty() {
            m.sender = ":1.0".into();
        }
        self.s.write_all(&m.encode(self.serial)).unwrap();
        self.serial
    }

    fn reply(&mut self, call: &Message, sig: &str, body: Vec<u8>) {
        let r = Message::reply_to(call, sig, body);
        self.send(r);
    }

    fn call(&mut self, path: &str, iface: &str, member: &str, sig: &str, body: Vec<u8>) -> Message {
        let serial = self.send(Message::call(":1.42", path, iface, member, sig, body));
        self.expect(move |m| (m.kind == RETURN || m.kind == ERROR) && m.reply_serial == serial)
    }

    fn call_async(&mut self, path: &str, iface: &str, member: &str, sig: &str, body: Vec<u8>) -> u32 {
        self.send(Message::call(":1.42", path, iface, member, sig, body))
    }

    fn reply_to_serial(&mut self, serial: u32) -> Message {
        self.expect(move |m| (m.kind == RETURN || m.kind == ERROR) && m.reply_serial == serial)
    }

    /// Plays the registry: sets our Id during Embed (as at-spi2-registryd does), then answers it.
    fn registry_embed(&mut self) {
        let embed = self.expect(|m| m.kind == CALL && m.iface == SOCKET && m.member == "Embed");
        let mut w = Writer::default();
        w.string(APPLICATION);
        w.string("Id");
        w.sig("i");
        w.u32(7);
        let set = self.call(ROOT_PATH, "org.freedesktop.DBus.Properties", "Set", "ssv", w.buf);
        assert_eq!(set.kind, RETURN, "Id set while Embed is pending must be answered (no deadlock)");
        let mut w = Writer::default();
        w.align(8);
        w.string(":1.0");
        w.string(ROOT_PATH);
        self.reply(&embed, "(so)", w.buf);
    }
}

#[derive(Default)]
struct Fake {
    on: bool,
    flips: u32,
    external: bool,
}

impl WindowApp for Fake {
    fn title(&self) -> String {
        "Fake".into()
    }
    fn initial_size(&self) -> (u32, u32) {
        (100, 100)
    }
    fn set_waker(&mut self, _: Waker) {}
    fn set_screen(&mut self, _: u32, _: u32) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn set_rounded(&mut self, _: bool) {}
    fn draw(&mut self, _: &mut Pixmap) {}
    fn pointer_motion(&mut self, _: f32, _: f32) -> Response {
        Response::NONE
    }
    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }
    fn pointer_button(&mut self, _: f32, _: f32, _: crate::app::Button, _: bool) -> Response {
        Response::NONE
    }
    fn scroll(&mut self, _: f32, _: f32, _: f32) -> Response {
        Response::NONE
    }
    fn key(&mut self, _: &KeyEvent) -> Response {
        let _ = Key::Tab;
        Response::NONE
    }
    fn tick(&mut self) -> Response {
        Response::NONE
    }
    fn a11y_name(&self) -> Option<String> {
        Some("Fake".into())
    }
    fn a11y_tree(&mut self, b: &mut TreeBuilder) {
        if self.external {
            self.external = false;
            self.on = !self.on;
        }
        let f = b.add(NodeId::ROOT, Spec::new("frame", Role::Frame, "Fake").bounds(Some((0, 0, 100, 100)))).unwrap();
        b.add(f, Spec::new("sw", Role::Switch, "Wi-Fi").state(State::Checked, self.on).state(State::Focusable, true).bounds(Some((1, 2, 3, 4))).action(ActionDef::new("toggle", "Toggle"))).unwrap();
        b.add(f, Spec::new("off", Role::PushButton, "Disabled").enabled(false).action(ActionDef::new("click", "Press"))).unwrap();
    }
    fn a11y_action(&mut self, t: &Target) -> Result<Response, ActionError> {
        match (t.key(), t.op) {
            ("sw", Op::Action("toggle")) => {
                self.on = !self.on;
                self.flips += 1;
                Ok(Response::REDRAW)
            }
            _ => Err(ActionError::Unsupported("fake".into())),
        }
    }
}

fn setup(queue: usize, timeout: Duration) -> (UnixListener, std::path::PathBuf, Host, Receiver<()>, Fake) {
    let dir = std::env::temp_dir().join(format!("decebal-a11y-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a11y-bus");
    let l = UnixListener::bind(&path).unwrap();
    let cfg = Config {
        app_name: "Fake".into(),
        session_address: None,
        explicit_address: Some(format!("unix:path={}", path.display())),
        mode: Mode::On,
        locale: "C".into(),
        toolkit_version: "test".into(),
        queue,
        reply_timeout: timeout,
    };
    let (tx, rx) = channel();
    let waker: Waker = Arc::new(move || {
        let _ = tx.send(());
    });
    let host = Host::with_config(cfg, waker);
    (l, dir, host, rx, Fake::default())
}

/// One UI iteration: wake handling, then a paint (as the backends do).
fn pump(host: &mut Host, app: &mut Fake, rx: &Receiver<()>, wait: Duration) -> bool {
    let woke = rx.recv_timeout(wait).is_ok();
    while rx.try_recv().is_ok() {}
    let r = host.service(app);
    if woke || r.redraw {
        host.after_draw(app);
    }
    woke
}

fn int(v: u32) -> Vec<u8> {
    let mut w = Writer::default();
    w.u32(v);
    w.buf
}

#[test]
fn forced_mode_discovers_the_bus_even_when_status_is_disabled() {
    let dir = std::env::temp_dir().join(format!("decebal-a11y-forced-{}-{:?}", std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).unwrap();
    let session_path = dir.join("session");
    let a11y_path = dir.join("a11y");
    let session = UnixListener::bind(&session_path).unwrap();
    let a11y = UnixListener::bind(&a11y_path).unwrap();
    let bridge = super::bridge::Bridge::start(
        Config {
            app_name: "Forced test".into(),
            session_address: Some(format!("unix:path={}", session_path.display())),
            explicit_address: None,
            mode: Mode::On,
            locale: "C".into(),
            toolkit_version: "test".into(),
            queue: 4,
            reply_timeout: Duration::from_secs(2),
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let mut bus = Peer::accept(&session);
    let status = bus.expect(|m| m.member == "Get");
    let mut w = Writer::default();
    w.sig("b");
    w.u32(0);
    bus.reply(&status, "v", w.buf);
    let get_address = bus.expect(|m| m.member == "GetAddress");
    let mut w = Writer::default();
    w.string(&format!("unix:path={}", a11y_path.display()));
    bus.reply(&get_address, "s", w.buf);
    let _accessibility_bus = Peer::accept(&a11y);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !bridge.wants_tree() {
        assert!(Instant::now() < deadline, "forced mode did not connect: {:?}", bridge.status());
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(bridge);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn action_debug_output_never_contains_edited_text() {
    for op in [Op::SetText("private-password-123".into()), Op::InsertText { offset: 3, text: "private-password-123".into() }] {
        let debug = format!("{op:?}");
        assert!(debug.contains("redacted"));
        assert!(!debug.contains("private-password-123"));
    }
}

fn wait_phase(host: &Host, p: Phase) {
    let t0 = Instant::now();
    while host.bridge().unwrap().status().phase != p {
        assert!(t0.elapsed() < Duration::from_secs(3), "phase {:?} not reached: {:?}", p, host.bridge().unwrap().status());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn registers_serves_and_dispatches_through_the_ui_thread() {
    let (l, dir, mut host, rx, mut app) = setup(4, Duration::from_secs(3));
    let mut bus = Peer::accept(&l);
    // The bridge asks for a tree before embedding; the UI answers on its next wake.
    assert!(pump(&mut host, &mut app, &rx, Duration::from_secs(2)));
    assert!(host.active());
    bus.registry_embed();
    wait_phase(&host, Phase::Serving);
    let st = host.bridge().unwrap().status();
    assert_eq!((st.app_id, st.unique.as_str()), (7, ":1.42"));

    // Reads are answered by the worker alone (no UI pump here).
    let kids = bus.call(ROOT_PATH, ACCESSIBLE, "GetChildren", "", vec![]);
    let Value::Array(k) = &kids.args().unwrap()[0] else { panic!() };
    assert_eq!(k.len(), 1);
    let tree = host.tree().unwrap().clone();
    let sw = path_of(tree.by_key("sw").unwrap().id);
    let off = path_of(tree.by_key("off").unwrap().id);

    // An action: deferred to the UI, applied, and answered only after the new tree (and its events) went out.
    let serial = bus.call_async(&sw, ACTION, "DoAction", "i", int(0));
    let t0 = Instant::now();
    while app.flips == 0 {
        pump(&mut host, &mut app, &rx, Duration::from_millis(20));
        assert!(t0.elapsed() < Duration::from_secs(3));
    }
    let mut got_event_first = false;
    loop {
        let m = bus.next(Duration::from_secs(2)).expect("reply");
        if m.kind == SIGNAL && m.member == "StateChanged" {
            let a = m.args().unwrap();
            if a[0].as_str() == Some("checked") && a[1].as_int() == Some(1) {
                got_event_first = true;
            }
            continue;
        }
        if m.kind == RETURN && m.reply_serial == serial {
            assert_eq!(m.args().unwrap()[0], Value::Bool(true));
            break;
        }
    }
    assert!(got_event_first, "state-changed:checked precedes the DoAction reply");

    // Disabled control.
    let r = bus.call_async(&off, ACTION, "DoAction", "i", int(0));
    pump(&mut host, &mut app, &rx, Duration::from_millis(500));
    let e = bus.reply_to_serial(r);
    assert_eq!(e.error, "org.decebal.Accessibility.Error.Disabled");

    // Stale: the switch changes (an external edit) after the AT's request was stamped, before the UI saw it.
    app.external = true;
    let r = bus.call_async(&sw, ACTION, "DoAction", "i", int(0));
    // Repaint first (the external change becomes a new revision), then service the queued request.
    std::thread::sleep(Duration::from_millis(50));
    host.after_draw(&mut app);
    pump(&mut host, &mut app, &rx, Duration::from_millis(500));
    let e = bus.reply_to_serial(r);
    assert_eq!(e.error, "org.decebal.Accessibility.Error.Stale", "{:?}", e.args());
    assert_eq!(app.flips, 1, "a stale request never reaches the app");

    // Unknown action index: rejected by the worker, no UI round trip.
    let e = bus.call(&sw, ACTION, "DoAction", "i", int(5));
    assert_eq!(e.error, "org.freedesktop.DBus.Error.InvalidArgs");

    // Stop: unembeds and joins promptly.
    let t0 = Instant::now();
    drop(host);
    assert!(t0.elapsed() < Duration::from_secs(1));
    bus.expect(|m| m.kind == CALL && m.member == "Unembed");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn queue_pressure_and_timeouts_answer_instead_of_hanging() {
    let (l, dir, mut host, rx, mut app) = setup(2, Duration::from_millis(300));
    let mut bus = Peer::accept(&l);
    pump(&mut host, &mut app, &rx, Duration::from_secs(2));
    bus.registry_embed();
    wait_phase(&host, Phase::Serving);
    let sw = path_of(host.tree().unwrap().by_key("sw").unwrap().id);
    // The UI is "busy" (not pumped): two requests queue, the rest are refused at once.
    let serials: Vec<u32> = (0..4).map(|_| bus.call_async(&sw, ACTION, "DoAction", "i", int(0))).collect();
    let busy3 = bus.reply_to_serial(serials[2]);
    let busy4 = bus.reply_to_serial(serials[3]);
    assert_eq!((busy3.error.as_str(), busy4.error.as_str()), ("org.freedesktop.DBus.Error.LimitsExceeded", "org.freedesktop.DBus.Error.LimitsExceeded"));
    // Still not pumped: the queued two time out with a clear error.
    let t1 = bus.reply_to_serial(serials[0]);
    let t2 = bus.reply_to_serial(serials[1]);
    assert_eq!((t1.error.as_str(), t2.error.as_str()), ("org.decebal.Accessibility.Error.Timeout", "org.decebal.Accessibility.Error.Timeout"));
    let st = host.bridge().unwrap().status();
    assert_eq!((st.busy, st.timeouts), (2, 2));
    // Draining the stale queue afterwards must not apply them twice or panic.
    for _ in 0..3 {
        pump(&mut host, &mut app, &rx, Duration::from_millis(30));
    }
    assert_eq!(app.flips, 0, "requests the AT was told timed out are dropped, never applied late");
    drop(host);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reconnects_after_the_accessibility_bus_goes_away() {
    let (l, dir, mut host, rx, mut app) = setup(4, Duration::from_secs(2));
    let mut bus = Peer::accept(&l);
    pump(&mut host, &mut app, &rx, Duration::from_secs(2));
    bus.registry_embed();
    wait_phase(&host, Phase::Serving);
    drop(bus);
    // The worker notices EOF, stops wanting trees, and retries after its back-off.
    let t0 = Instant::now();
    while host.active() {
        assert!(t0.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut bus = Peer::accept(&l);
    assert!(pump(&mut host, &mut app, &rx, Duration::from_secs(2)), "a new registration wakes the UI");
    bus.registry_embed();
    wait_phase(&host, Phase::Serving);
    let st = host.bridge().unwrap().status();
    assert_eq!((st.connects, st.embeds), (2, 2));
    // The fresh registration serves the full tree again.
    let kids = bus.call(&path_of(host.tree().unwrap().by_key("frame").unwrap().id), ACCESSIBLE, "GetChildren", "", vec![]);
    let Value::Array(k) = &kids.args().unwrap()[0] else { panic!() };
    assert_eq!(k.len(), 2);
    drop(host);
    let _ = std::fs::remove_dir_all(dir);
}
