//! System-side panes of macsettings (sound, network, bluetooth, displays, input, power, ...).
//!
//! Architecture
//! * every pane is `parse (pure) -> State`, `load(&dyn Runner) -> State`, `items(&Hub) -> Vec<Item>`, `apply(&mut Hub, id, val)`;
//! * external tools run only through the [`run::Runner`] trait, only on worker threads ([`Shared::spawn`]);
//! * results come back as [`Done`] values in a queue drained by `Model::sys_poll` from the UI tick;
//! * a poller thread refreshes the *visible* pane's state every few seconds and sleeps (parked) when the
//!   window is hidden or the pane needs no polling;
//! * settings that must survive a login go to [`session::Session`] (`session-settings.json` + `.sh`).

pub mod about;
pub mod access;
pub mod bluetooth;
pub mod confirm;
pub mod datetime;
pub mod defaults;
pub mod display;
pub mod glyphs;
pub mod input;
pub mod network;
pub mod notifications;
pub mod power;
pub mod registry;
pub mod run;
pub mod session;
pub mod sound;

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Row};
use confirm::Confirm;
use run::{Cmd, RealRunner, Runner};
use session::Session;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dom {
    Sound,
    Network,
    Bluetooth,
    Display,
    Input,
    Power,
    DateTime,
    Defaults,
    Users,
    Storage,
    /// File-only panes: no loaded state.
    Privacy,
    Access,
}

impl Dom {
    /// Pane id -> domain.
    pub fn of_pane(id: &str) -> Option<Dom> {
        Some(match id {
            "sound" => Dom::Sound,
            "network" => Dom::Network,
            "bluetooth" => Dom::Bluetooth,
            "displays" => Dom::Display,
            "inputdev" => Dom::Input,
            "power" => Dom::Power,
            "datetime" => Dom::DateTime,
            "defaults" => Dom::Defaults,
            "users" => Dom::Users,
            "storage" => Dom::Storage,
            _ => return None,
        })
    }
    pub fn has_data(self) -> bool {
        !matches!(self, Dom::Privacy | Dom::Access)
    }
    /// How often the visible pane is refreshed (None = only when opened or after an action).
    pub fn poll_every(self) -> Option<Duration> {
        let s = match self {
            Dom::Sound => 3,
            Dom::Network => 6,
            Dom::Bluetooth => 5,
            Dom::Display => 10,
            Dom::Power => 5,
            Dom::DateTime => 30,
            _ => return None,
        };
        Some(Duration::from_secs(s))
    }
}

pub enum Data {
    Sound(sound::State),
    Net(network::State),
    Bt(bluetooth::State),
    Disp(display::State),
    Input(input::State),
    Power(power::State),
    Dt(datetime::State),
    Defaults(defaults::State),
    Users(about::Users),
    Storage(about::Storage),
}

pub struct UndoRec {
    pub label: String,
    pub dom: Dom,
    pub cmds: Vec<Cmd>,
    pub session: Option<Session>,
}

#[derive(Default)]
pub struct Done {
    pub data: Option<Data>,
    pub msg: Option<(String, bool)>,
    pub refresh: Option<Dom>,
    pub undo: Option<UndoRec>,
    /// The undo itself finished: forget the record.
    pub clear_undo: bool,
    /// The session file now holds this (display layout kept).
    pub session: Option<Session>,
    /// An action failed before the session file was written: reload it from disk.
    pub session_reload: bool,
}

/// A change to run: commands (stop at the first failure), an optional session-file update, how to undo.
#[derive(Debug, Clone, Default)]
pub struct Action {
    pub dom: Option<Dom>,
    pub label: String,
    pub ok_msg: Option<String>,
    pub cmds: Vec<Cmd>,
    pub undo: Vec<Cmd>,
    pub session: Option<Session>,
    /// What the session looked like before (for undo of the persisted part).
    pub session_before: Option<Session>,
    pub refresh: bool,
    pub is_undo: bool,
    /// Turn polkit refusals into a readable hint.
    pub explain: bool,
}

impl Action {
    pub fn new(dom: Dom, label: impl Into<String>, cmds: Vec<Cmd>) -> Action {
        Action { dom: Some(dom), label: label.into(), cmds, refresh: true, ..Action::default() }
    }
    pub fn undo(mut self, u: Vec<Cmd>) -> Action {
        self.undo = u;
        self
    }
    pub fn session(mut self, before: &Session, after: Session) -> Action {
        self.session_before = Some(before.clone());
        self.session = Some(after);
        self
    }
    pub fn msg(mut self, m: impl Into<String>) -> Action {
        self.ok_msg = Some(m.into());
        self
    }
}

/// Runs an action (blocking; call from a worker) with a private "latest session" (tests).
pub fn exec_action(r: &dyn Runner, dir: &std::path::Path, a: &Action) -> Done {
    let latest = Mutex::new(a.session.clone().unwrap_or_default());
    exec_action_with(r, dir, &latest, a)
}

/// The real executor. Session saves always write the *latest* in-memory session, so quick successive
/// changes from different worker threads cannot overwrite each other with stale snapshots.
pub fn exec_action_with(r: &dyn Runner, dir: &std::path::Path, latest: &Mutex<Session>, a: &Action) -> Done {
    let mut d = Done { refresh: a.dom.filter(|d| a.refresh && d.has_data()), ..Done::default() };
    for c in &a.cmds {
        let out = r.run(c);
        if !out.ok {
            let why = if a.explain { datetime::explain(&out) } else { None }.unwrap_or_else(|| out.error_text(&c.prog));
            d.msg = Some((format!("{}: {}", a.label, why), true));
            d.session_reload = a.session.is_some();
            return d;
        }
    }
    if a.session.is_some() {
        let res = latest.lock().unwrap().save(dir);
        if let Err(e) = res {
            d.msg = Some((format!("{}: could not save for next login: {e}", a.label), true));
            d.session_reload = true;
            return d;
        }
    }
    d.msg = a.ok_msg.clone().map(|m| (m, false));
    if a.is_undo {
        d.clear_undo = true;
    } else if a.dom.is_some() && (!a.undo.is_empty() || a.session_before.is_some()) {
        d.undo = Some(UndoRec { label: a.label.clone(), dom: a.dom.expect("dom"), cmds: a.undo.clone(), session: a.session_before.clone() });
    }
    d
}

pub struct Shared {
    pub runner: Arc<dyn Runner>,
    pub dir: PathBuf,
    results: Mutex<VecDeque<Done>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    inflight: Mutex<HashSet<Dom>>,
    active: Mutex<Option<Dom>>,
    pub visible: AtomicBool,
    pub confirm: Mutex<Confirm>,
    /// Latest session settings (see `exec_action_with`).
    pub session: Mutex<Session>,
    poller: Mutex<Option<std::thread::Thread>>,
}

impl Shared {
    pub fn spawn(self: &Arc<Self>, f: impl FnOnce(&dyn Runner) -> Done + Send + 'static) {
        let me = self.clone();
        let _ = std::thread::Builder::new().name("macsettings-job".into()).stack_size(512 * 1024).spawn(move || {
            let d = f(me.runner.as_ref());
            me.results.lock().unwrap().push_back(d);
            (me.wake)();
        });
    }

    pub fn refresh(self: &Arc<Self>, dom: Dom) {
        if !dom.has_data() || !self.inflight.lock().unwrap().insert(dom) {
            return;
        }
        let me = self.clone();
        self.spawn(move |r| {
            let data = load(dom, r);
            me.inflight.lock().unwrap().remove(&dom);
            Done { data, ..Done::default() }
        });
    }

    pub(crate) fn wake_poller(&self) {
        if let Some(t) = self.poller.lock().unwrap().as_ref() {
            t.unpark();
        }
    }
}

pub fn load(dom: Dom, r: &dyn Runner) -> Option<Data> {
    Some(match dom {
        Dom::Sound => Data::Sound(sound::load(r)),
        Dom::Network => Data::Net(network::load(r)),
        Dom::Bluetooth => Data::Bt(bluetooth::load(r)),
        Dom::Display => Data::Disp(display::load(r)),
        Dom::Input => Data::Input(input::load(r)),
        Dom::Power => Data::Power(power::load(r)),
        Dom::DateTime => Data::Dt(datetime::load(r)),
        Dom::Defaults => Data::Defaults(defaults::load(r)),
        Dom::Users => Data::Users(about::load_users()),
        Dom::Storage => Data::Storage(about::load_storage(r)),
        Dom::Privacy | Dom::Access => return None,
    })
}

/// One loaded domain: data plus bookkeeping.
pub struct Slot<T> {
    pub data: Option<T>,
    pub at: Option<Instant>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot { data: None, at: None }
    }
}

pub struct Hub {
    pub sh: Arc<Shared>,
    pub sound: Slot<sound::State>,
    pub net: Slot<network::State>,
    pub bt: Slot<bluetooth::State>,
    pub disp: Slot<display::State>,
    pub input: Slot<input::State>,
    pub power: Slot<power::State>,
    pub dt: Slot<datetime::State>,
    pub defaults: Slot<defaults::State>,
    pub users: Slot<about::Users>,
    pub storage: Slot<about::Storage>,
    pub notif: notifications::NotifCfg,
    pub session: Session,
    pub undo: Option<UndoRec>,
    /// Set when something changed that needs a repaint.
    pub dirty: bool,
    /// Messages produced on the UI thread (validation), shown on the next drain.
    pub msgs: Vec<(String, bool)>,
    pub ui_net: network::Ui,
    pub ui_bt: bluetooth::Ui,
    pub ui_not: notifications::Ui,
    pub ui_users: about::UsersUi,
    pub ui_access: access::Ui,
}

impl Hub {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Hub {
        Hub::with(Arc::new(RealRunner::default()), session::dir(), wake)
    }

    pub fn with(runner: Arc<dyn Runner>, dir: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Hub {
        let session = Session::load(&dir);
        let sh = Arc::new(Shared {
            runner,
            dir: dir.clone(),
            results: Mutex::new(VecDeque::new()),
            wake,
            inflight: Mutex::new(HashSet::new()),
            active: Mutex::new(None),
            visible: AtomicBool::new(true),
            confirm: Mutex::new(Confirm::default()),
            session: Mutex::new(session.clone()),
            poller: Mutex::new(None),
        });
        Hub {
            sh,
            sound: Slot::default(),
            net: Slot::default(),
            bt: Slot::default(),
            disp: Slot::default(),
            input: Slot::default(),
            power: Slot::default(),
            dt: Slot::default(),
            defaults: Slot::default(),
            users: Slot::default(),
            storage: Slot::default(),
            notif: notifications::NotifCfg::load(&dir),
            session,
            undo: None,
            dirty: false,
            msgs: Vec::new(),
            ui_net: Default::default(),
            ui_bt: Default::default(),
            ui_not: Default::default(),
            ui_users: Default::default(),
            ui_access: Default::default(),
        }
    }

    /// Called whenever a system pane is (re)drawn: remember which pane polls, load it on first view.
    pub fn activate(&mut self, pane_id: &str) {
        let dom = Dom::of_pane(pane_id);
        let changed = {
            let mut a = self.sh.active.lock().unwrap();
            let c = *a != dom;
            *a = dom;
            c
        };
        if changed && pane_id == "notifications" {
            self.notif = notifications::NotifCfg::load(&self.sh.dir);
        }
        if changed && matches!(pane_id, "privacy" | "accessibility") {
            self.session = Session::load(&self.sh.dir);
        }
        let Some(dom) = dom else { return };
        if changed || self.slot_empty(dom) {
            self.ensure_poller();
            self.sh.refresh(dom);
            self.sh.wake_poller();
        }
    }

    fn slot_empty(&self, dom: Dom) -> bool {
        match dom {
            Dom::Sound => self.sound.data.is_none(),
            Dom::Network => self.net.data.is_none(),
            Dom::Bluetooth => self.bt.data.is_none(),
            Dom::Display => self.disp.data.is_none(),
            Dom::Input => self.input.data.is_none(),
            Dom::Power => self.power.data.is_none(),
            Dom::DateTime => self.dt.data.is_none(),
            Dom::Defaults => self.defaults.data.is_none(),
            Dom::Users => self.users.data.is_none(),
            Dom::Storage => self.storage.data.is_none(),
            Dom::Privacy | Dom::Access => false,
        }
    }

    pub fn set_visible(&mut self, v: bool) {
        let was = self.sh.visible.swap(v, Ordering::Relaxed);
        if v && !was {
            self.sh.wake_poller();
        }
    }

    fn ensure_poller(&mut self) {
        let mut g = self.sh.poller.lock().unwrap();
        if g.is_some() {
            return;
        }
        let sh = self.sh.clone();
        let h = std::thread::Builder::new().name("macsettings-poll".into()).stack_size(256 * 1024).spawn(move || poll_loop(sh));
        if let Ok(h) = h {
            *g = Some(h.thread().clone());
        }
    }

    pub fn run(&mut self, a: Action) {
        let (dir, sh) = (self.sh.dir.clone(), self.sh.clone());
        self.sh.spawn(move |r| exec_action_with(r, &dir, &sh.session, &a));
    }

    /// Like `run`, and also persists `s` as the new session settings (applied again at login).
    pub fn run_session(&mut self, mut a: Action, s: Session) {
        a.session_before = Some(self.session.clone());
        a.session = Some(s.clone());
        *self.sh.session.lock().unwrap() = s.clone();
        self.session = s;
        self.run(a);
    }

    pub fn say(&mut self, text: impl Into<String>, error: bool) {
        self.msgs.push((text.into(), error));
        self.dirty = true;
    }

    pub fn undo_last(&mut self) {
        if let Some(u) = self.undo.take() {
            let mut a = Action::new(u.dom, format!("Undo {}", u.label), u.cmds);
            a.is_undo = true;
            a.ok_msg = Some(format!("Undone: {}", u.label));
            match u.session {
                Some(s) => self.run_session(a, s),
                None => self.run(a),
            }
        }
    }

    /// Applies finished jobs. Returns (anything changed, messages to show).
    pub fn drain(&mut self) -> (bool, Vec<(String, bool)>) {
        let mut changed = std::mem::take(&mut self.dirty);
        let mut msgs = std::mem::take(&mut self.msgs);
        if !msgs.is_empty() {
            changed = true;
        }
        loop {
            let d = self.sh.results.lock().unwrap().pop_front();
            let Some(d) = d else { break };
            changed = true;
            let now = Some(Instant::now());
            match d.data {
                Some(Data::Sound(s)) => self.sound = Slot { data: Some(s), at: now },
                Some(Data::Net(s)) => {
                    self.ui_net.scanning = false;
                    self.net = Slot { data: Some(s), at: now }
                }
                Some(Data::Bt(s)) => {
                    self.ui_bt.scanning = false;
                    self.bt = Slot { data: Some(s), at: now }
                }
                Some(Data::Disp(s)) => self.disp = Slot { data: Some(s), at: now },
                Some(Data::Input(s)) => self.input = Slot { data: Some(s), at: now },
                Some(Data::Power(s)) => self.power = Slot { data: Some(s), at: now },
                Some(Data::Dt(s)) => self.dt = Slot { data: Some(s), at: now },
                Some(Data::Defaults(s)) => self.defaults = Slot { data: Some(s), at: now },
                Some(Data::Users(s)) => self.users = Slot { data: Some(s), at: now },
                Some(Data::Storage(s)) => self.storage = Slot { data: Some(s), at: now },
                None => {}
            }
            if let Some(m) = d.msg {
                msgs.push(m);
            }
            if d.clear_undo {
                self.undo = None;
            }
            if let Some(u) = d.undo {
                self.undo = Some(u);
            }
            if let Some(s) = d.session {
                *self.sh.session.lock().unwrap() = s.clone();
                self.session = s;
            }
            if d.session_reload {
                self.session = Session::load(&self.sh.dir);
                *self.sh.session.lock().unwrap() = self.session.clone();
            }
            if let Some(dom) = d.refresh {
                self.ui_net.scanning &= dom != Dom::Network;
                self.sh.refresh(dom);
            }
        }
        if self.sh.confirm.lock().unwrap().active() {
            changed = true;
        }
        (changed, msgs)
    }
}

fn poll_loop(sh: Arc<Shared>) {
    let mut last: Option<(Dom, Instant)> = None;
    loop {
        let dom = *sh.active.lock().unwrap();
        let visible = sh.visible.load(Ordering::Relaxed);
        let pending = sh.confirm.lock().unwrap().active();
        if dom.filter(|_| visible).is_none_or(|d| d.poll_every().is_none()) && !pending {
            std::thread::park_timeout(Duration::from_secs(600));
            continue;
        }
        if pending {
            let step = sh.confirm.lock().unwrap().poll(Instant::now());
            if let confirm::Step::Expired(p) = step {
                display_revert(&sh, p);
            }
            (sh.wake)();
        }
        if let (Some(d), true) = (dom, visible) {
            if let Some(every) = d.poll_every() {
                let due = last.is_none_or(|(ld, at)| ld != d || at.elapsed() >= every);
                if due {
                    last = Some((d, Instant::now()));
                    sh.refresh(d);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn display_revert(sh: &Arc<Shared>, p: confirm::Pending) {
    display::display_revert(sh, p)
}

// ---------------------------------------------------------------- glue into Model

impl Model {
    /// Called from `prepare` (every paint of a pane).
    pub fn sys_prepare(&mut self, pane_id: &str) {
        self.sys.activate(pane_id);
    }

    /// Called from the UI tick: applies finished jobs, shows their messages. True = repaint.
    pub fn sys_poll(&mut self) -> bool {
        let (changed, msgs) = self.sys.drain();
        for (t, e) in msgs {
            self.say(t, e);
        }
        changed
    }

    /// Window shown / hidden: polling stops while hidden.
    pub fn sys_visible(&mut self, v: bool) {
        self.sys.set_visible(v);
    }

    fn clock_prefs(&self) -> (bool, String) {
        (self.shell["clockHour12"].as_bool().unwrap_or(false), self.shell["clockDate"].as_str().unwrap_or("weekday-day-month").to_string())
    }

    /// Items of the Displays pane that come from the system side (the rdwm part is appended by the caller).
    pub fn sys_display_items(&self) -> Vec<Item> {
        let scale = self.value("decoration", "scale").and_then(|v| match v {
            super::tomlcfg::Val::Float(f) => Some(f),
            super::tomlcfg::Val::Int(i) => Some(i as f64),
            _ => None,
        });
        let mut v = display::items(&self.sys, self.store.cfg.get("decoration", "scale").and(scale));
        if let Some(u) = &self.sys.undo {
            if u.dom == Dom::Display {
                v.insert(0, undo_card(u));
            }
        }
        v
    }

    /// Items of a system pane, or None when `id` is not one.
    pub fn sys_items(&self, id: &str) -> Option<Vec<Item>> {
        let h = &self.sys;
        let mut v = match id {
            "sound" => sound::items(h),
            "network" => network::items(h),
            "bluetooth" => bluetooth::items(h),
            "inputdev" => input::items(h),
            "power" => power::items(h),
            "defaults" => defaults::items(h),
            "notifications" => notifications::items(h),
            "datetime" => {
                let (h12, fmt) = self.clock_prefs();
                datetime::items(h, h12, &fmt)
            }
            "users" => about::users_items(h),
            "storage" => about::storage_items(h, self.rt.version.as_deref().filter(|_| self.rt.running)),
            "privacy" => access::privacy_items(h),
            "accessibility" => access::access_items(h),
            _ => return None,
        };
        if let Some(u) = &self.sys.undo {
            if Dom::of_pane(id) == Some(u.dom) {
                v.insert(0, undo_card(u));
            }
        }
        Some(v)
    }

    pub fn sys_apply(&mut self, id: &str, v: CtlVal) {
        if id == "s:undo" {
            self.sys.undo_last();
            return;
        }
        let mut it = id.splitn(4, ':');
        let (_, dom, act, arg) = (it.next(), it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        match (dom, act) {
            ("snd", _) => sound::apply(&mut self.sys, act, arg, v),
            ("net", _) => network::apply(&mut self.sys, act, arg, v),
            ("bt", _) => bluetooth::apply(&mut self.sys, act, arg, v),
            ("disp", "scale") => self.apply_scale(v),
            ("disp", _) => display::apply(&mut self.sys, act, arg, v),
            ("inp", _) => input::apply(&mut self.sys, act, arg, v),
            ("pwr", _) => power::apply(&mut self.sys, act, arg, v),
            ("def", _) => defaults::apply(&mut self.sys, act, arg, v),
            ("not", _) => notifications::apply(&mut self.sys, act, arg, v),
            ("usr", _) => about::users_apply(&mut self.sys, act, v),
            ("prv", _) => access::privacy_apply(&mut self.sys, act, v),
            ("acc", _) => access::access_apply(&mut self.sys, act, v),
            ("dt", "hour12") => {
                if let CtlVal::Bool(b) = v {
                    self.edit_json("shell", "clockHour12", Some(serde_json::json!(b)));
                }
            }
            ("dt", "datefmt") => {
                if let CtlVal::Str(s) = v {
                    if datetime::date_formats().iter().any(|(_, x)| *x == s) {
                        self.edit_json("shell", "clockDate", Some(serde_json::json!(s)));
                    } else {
                        self.say("Unknown date format", true);
                    }
                }
            }
            ("dt", _) => datetime::apply(&mut self.sys, act, arg, v),
            ("sto", "check") => self.check_config(),
            _ => {}
        }
    }

    fn apply_scale(&mut self, v: CtlVal) {
        let CtlVal::Str(s) = v else { return };
        if s == "auto" {
            self.edit(|c| {
                c.remove("decoration", "scale");
                Ok(())
            });
            if let Err(e) = display::apply_scale_dpi(&mut self.sys, None) {
                self.say(e, true);
            }
            return;
        }
        let Ok(p) = s.parse::<u32>() else { return self.say("Invalid scale", true) };
        if !(50..=400).contains(&p) {
            return self.say("Scale must be between 50 % and 400 %", true);
        }
        let f = p as f64 / 100.0;
        if self.edit(|c| c.set("decoration", "scale", &super::tomlcfg::Val::Float(f))) {
            if let Err(e) = display::apply_scale_dpi(&mut self.sys, Some(p)) {
                self.say(e, true);
            }
        }
    }

    /// "Check config": rdwm's own parser over the current file, off the UI thread.
    fn check_config(&mut self) {
        let ctl = self.paths.rdwmctl.to_string_lossy().into_owned();
        let file = self.paths.rdwm_config.to_string_lossy().into_owned();
        self.sys.sh.spawn(move |r| {
            let o = r.run(&Cmd::owned(&ctl, vec!["check-config".into(), file]).timeout(10));
            let msg = if o.ok {
                ("Configuration is valid".to_string(), false)
            } else if o.missing {
                ("rdwmctl is not installed, so the file cannot be checked".to_string(), true)
            } else {
                (format!("Configuration problem: {}", o.error_text("rdwmctl")), true)
            };
            Done { msg: Some(msg), ..Done::default() }
        });
    }
}

fn undo_card(u: &UndoRec) -> Item {
    Item::Card(vec![Row::new(format!("Last change: {}", u.label)).ctl(Ctl::new("s:undo", Kind::Button { label: "Undo".into(), danger: false }))])
}
