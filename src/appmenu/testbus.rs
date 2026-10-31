//! Test-only private bus and a scriptable exporter/registrar peer. The bus is a `dbus-daemon` with
//! its own config (no service directories, so nothing is ever activated), socket and HOME under a
//! fresh temp dir; nothing here reads the session environment.

use super::bus::{self, Bus};
use crate::dbus::{self, Message, Value, Writer};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct PrivateBus {
    dir: PathBuf,
    child: Child,
    pub address: String,
}

impl PrivateBus {
    pub fn start() -> PrivateBus {
        let dir = std::env::temp_dir().join(format!("decebal-appmenu-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("bus");
        let conf = dir.join("bus.conf");
        std::fs::write(
            &conf,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\
<busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth>\
<policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
                sock.display()
            ),
        )
        .unwrap();
        let child = spawn_daemon(&dir, &conf, &sock);
        PrivateBus { address: format!("unix:path={}", sock.display()), dir, child }
    }

    /// Kills the daemon, leaving the address free for [`PrivateBus::restart`].
    pub fn child_kill_for_restart(&mut self) -> std::io::Result<()> {
        self.child.kill()?;
        self.child.wait()?;
        let _ = std::fs::remove_file(self.dir.join("bus"));
        Ok(())
    }

    pub fn restart(&mut self) {
        self.child = spawn_daemon(&self.dir, &self.dir.join("bus.conf"), &self.dir.join("bus"));
    }
}

fn spawn_daemon(dir: &std::path::Path, conf: &std::path::Path, sock: &std::path::Path) -> Child {
    let child = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", conf.display()))
        .args(["--nofork", "--nopidfile"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbus-daemon is required for the private-bus app-menu tests");
    let end = Instant::now() + Duration::from_secs(5);
    while !sock.exists() {
        assert!(Instant::now() < end, "private dbus-daemon did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    child
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A layout node for the fake exporter.
#[derive(Clone, Debug)]
pub struct N {
    pub id: i32,
    pub props: Vec<(&'static str, P)>,
    pub children: Vec<N>,
}

#[derive(Clone, Debug)]
pub enum P {
    S(&'static str),
    B(bool),
    I(i32),
    Keys(Vec<Vec<&'static str>>),
}

pub fn n(id: i32, props: Vec<(&'static str, P)>, children: Vec<N>) -> N {
    N { id, props, children }
}

fn write_prop(w: &mut Writer, v: &P) {
    match v {
        P::S(s) => {
            w.sig("s");
            w.string(s);
        }
        P::B(b) => {
            w.sig("b");
            w.u32(u32::from(*b));
        }
        P::I(i) => {
            w.sig("i");
            w.u32(*i as u32);
        }
        P::Keys(k) => {
            w.sig("aas");
            w.array(4, |w| {
                for alt in k {
                    w.array(4, |w| alt.iter().for_each(|s| w.string(s)));
                }
            });
        }
    }
}

fn write_props(w: &mut Writer, props: &[(&'static str, P)]) {
    w.array(8, |w| {
        for (k, v) in props {
            w.align(8);
            w.string(k);
            write_prop(w, v);
        }
    });
}

pub fn write_node(w: &mut Writer, node: &N) {
    w.align(8);
    w.u32(node.id as u32);
    write_props(w, &node.props);
    w.array(1, |w| {
        for c in &node.children {
            w.sig("(ia{sv}av)");
            write_node(w, c);
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// `GetLayout` answers with the wrong signature.
    WrongSig,
    /// `GetLayout` answers with a body far over the worker's reply cap.
    Huge,
    /// `GetLayout` is never answered.
    Silent,
    /// `GetLayout` is answered after a delay.
    Slow(Duration),
}

/// Calls the fake received, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rec {
    pub member: String,
    pub id: i32,
    pub event: String,
    pub timestamp: u32,
    pub sender: String,
}

enum Ctl {
    Call(Message, Sender<Message>),
    Send(Message),
    Layout(N),
    Mode(Mode),
    /// Serve the registrar interface at its path (after owning the name via `call`).
    ServeRegistrar(HashMap<u32, (String, String)>),
    Quit,
}

pub struct Fake {
    tx: Sender<Ctl>,
    pub log: Arc<Mutex<Vec<Rec>>>,
    pub unique: String,
    thread: Option<JoinHandle<()>>,
}

pub const MENU_PATH: &str = "/MenuBar/1";

impl Fake {
    pub fn connect(address: &str, layout: N) -> Fake {
        let bus = Bus::connect(address, Instant::now() + Duration::from_secs(2), bus::Cancel::never(), 64 << 20, Duration::from_secs(2)).unwrap();
        let unique = bus.unique.clone();
        let (tx, rx) = mpsc::channel();
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        let thread = std::thread::spawn(move || serve(bus, rx, l, layout));
        Fake { tx, log, unique, thread: Some(thread) }
    }

    pub fn call(&self, m: Message) -> Message {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Ctl::Call(m, tx)).unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("fake call timed out")
    }

    pub fn send(&self, m: Message) {
        self.tx.send(Ctl::Send(m)).unwrap();
    }

    pub fn register(&self, window: u32, path: &str) -> Message {
        let mut w = Writer::default();
        w.u32(window);
        w.string(path);
        self.call(bus::call(super::registrar::NAME, super::registrar::PATH, super::registrar::IFACE, "RegisterWindow", "uo", w.buf))
    }

    pub fn request_name(&self, name: &str, flags: u32) -> u32 {
        let mut w = Writer::default();
        w.string(name);
        w.u32(flags);
        let r = self.call(bus::call(bus::BUS, bus::BUS_PATH, bus::BUS, "RequestName", "su", w.buf));
        r.args().unwrap()[0].as_u32().unwrap()
    }

    pub fn serve_registrar(&self, table: HashMap<u32, (String, String)>) {
        self.tx.send(Ctl::ServeRegistrar(table)).unwrap();
    }

    pub fn set_layout(&self, layout: N) {
        self.tx.send(Ctl::Layout(layout)).unwrap();
    }

    pub fn set_mode(&self, mode: Mode) {
        self.tx.send(Ctl::Mode(mode)).unwrap();
    }

    pub fn layout_updated(&self, revision: u32) {
        let mut w = Writer::default();
        w.u32(revision);
        w.u32(0);
        self.send(Message::signal(MENU_PATH, super::DBUSMENU, "LayoutUpdated", "ui", w.buf));
    }

    pub fn props_updated(&self, id: i32, props: Vec<(&'static str, P)>, removed: Vec<&'static str>) {
        let mut w = Writer::default();
        w.array(8, |w| {
            w.align(8);
            w.u32(id as u32);
            write_props(w, &props);
        });
        w.array(8, |w| {
            w.align(8);
            w.u32(id as u32);
            w.array(4, |w| removed.iter().for_each(|s| w.string(s)));
        });
        self.send(Message::signal(MENU_PATH, super::DBUSMENU, "ItemsPropertiesUpdated", "a(ia{sv})a(ias)", w.buf));
    }

    pub fn records(&self, member: &str) -> Vec<Rec> {
        self.log.lock().unwrap().iter().filter(|r| r.member == member).cloned().collect()
    }

    pub fn wait_records(&self, member: &str, count: usize) -> Vec<Rec> {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let r = self.records(member);
            if r.len() >= count {
                return r;
            }
            assert!(Instant::now() < end, "waited for {count} {member} calls, have {r:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Disconnects from the bus (as if the application exited).
    pub fn quit(mut self) {
        let _ = self.tx.send(Ctl::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.tx.send(Ctl::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve(mut bus: Bus, rx: Receiver<Ctl>, log: Arc<Mutex<Vec<Rec>>>, mut layout: N) {
    let mut waiting: HashMap<u32, Sender<Message>> = HashMap::new();
    let mut delayed: Vec<(Instant, Message)> = Vec::new();
    let mut mode = Mode::Normal;
    let mut revision = 1u32;
    let mut registrar: Option<HashMap<u32, (String, String)>> = None;
    loop {
        loop {
            match rx.try_recv() {
                Ok(Ctl::Call(m, reply)) => {
                    if let Ok(s) = bus.send(&m) {
                        waiting.insert(s, reply);
                    }
                }
                Ok(Ctl::Send(m)) => {
                    let _ = bus.send(&m);
                }
                Ok(Ctl::Layout(l)) => {
                    layout = l;
                    revision += 1;
                }
                Ok(Ctl::Mode(m)) => mode = m,
                Ok(Ctl::ServeRegistrar(t)) => registrar = Some(t),
                Ok(Ctl::Quit) | Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        let now = Instant::now();
        delayed.retain(|(t, m)| {
            if *t <= now {
                let _ = bus.send(m);
                false
            } else {
                true
            }
        });
        if !matches!(bus.fill(), Ok(true)) {
            return;
        }
        while let Ok(Some(m)) = bus.next_message() {
            if m.kind == dbus::RETURN || m.kind == dbus::ERROR {
                if let Some(tx) = waiting.remove(&m.reply_serial) {
                    let _ = tx.send(m);
                }
                continue;
            }
            if m.kind != dbus::CALL {
                continue;
            }
            let a = m.args().unwrap_or_default();
            let int = |i: usize| match a.get(i) {
                Some(Value::I32(v)) => *v,
                Some(Value::U32(v)) => *v as i32,
                _ => -1,
            };
            if let Some(table) = registrar.as_mut().filter(|_| m.path == super::registrar::PATH) {
                let mut w = Writer::default();
                let mut sig = "";
                match m.member.as_str() {
                    "GetMenuForWindow" => {
                        log.lock().unwrap().push(Rec { member: "GetMenuForWindow".into(), id: int(0), event: String::new(), timestamp: 0, sender: m.sender.clone() });
                        let (s, p) = table.get(&(int(0) as u32)).cloned().unwrap_or_else(|| (String::new(), "/".into()));
                        w.string(&s);
                        w.string(&p);
                        sig = "so";
                    }
                    "RegisterWindow" => {
                        let path = a.get(1).and_then(Value::as_str).unwrap_or("/").to_string();
                        table.insert(int(0) as u32, (m.sender.clone(), path.clone()));
                        let mut s = Writer::default();
                        s.u32(int(0) as u32);
                        s.string(&m.sender);
                        s.string(&path);
                        let _ = bus.send(&Message::signal(super::registrar::PATH, super::registrar::IFACE, "WindowRegistered", "uso", s.buf));
                    }
                    _ => {}
                }
                let _ = bus.send(&Message::reply_to(&m, sig, w.buf));
                continue;
            }
            match (m.path.as_str(), m.member.as_str()) {
                (MENU_PATH, "GetLayout") => {
                    log.lock().unwrap().push(Rec { member: "GetLayout".into(), id: int(0), event: String::new(), timestamp: 0, sender: m.sender.clone() });
                    let reply = match mode {
                        Mode::Silent => continue,
                        Mode::WrongSig => {
                            let mut w = Writer::default();
                            w.string("not a layout");
                            Message::reply_to(&m, "s", w.buf)
                        }
                        Mode::Huge => {
                            let mut w = Writer::default();
                            w.u32(revision);
                            let label: &'static str = Box::leak("x".repeat(1 << 20).into_boxed_str());
                            write_node(&mut w, &n(0, vec![], (1..6).map(|i| n(i, vec![("label", P::S(label))], vec![])).collect()));
                            Message::reply_to(&m, "u(ia{sv}av)", w.buf)
                        }
                        Mode::Normal | Mode::Slow(_) => {
                            let mut w = Writer::default();
                            w.u32(revision);
                            write_node(&mut w, &layout);
                            Message::reply_to(&m, "u(ia{sv}av)", w.buf)
                        }
                    };
                    if let Mode::Slow(d) = mode {
                        delayed.push((Instant::now() + d, reply));
                    } else {
                        let _ = bus.send(&reply);
                    }
                }
                (MENU_PATH, "Event") => {
                    let ts = match a.get(3) {
                        Some(Value::U32(t)) => *t,
                        _ => 0,
                    };
                    let event = a.get(1).and_then(Value::as_str).unwrap_or("").to_string();
                    log.lock().unwrap().push(Rec { member: "Event".into(), id: int(0), event, timestamp: ts, sender: m.sender.clone() });
                    let _ = bus.send(&Message::reply_to(&m, "", Vec::new()));
                }
                (MENU_PATH, "AboutToShow") => {
                    log.lock().unwrap().push(Rec { member: "AboutToShow".into(), id: int(0), event: String::new(), timestamp: 0, sender: m.sender.clone() });
                    let mut w = Writer::default();
                    w.u32(1);
                    let _ = bus.send(&Message::reply_to(&m, "b", w.buf));
                }
                _ => {
                    let _ = bus.send(&Message::error_to(&m, "org.freedesktop.DBus.Error.UnknownObject", "fake"));
                }
            }
        }
        let _ = crate::backend::poll_readable(&[bus.fd()], Some(Duration::from_millis(5)));
    }
}
