//! Application state without any windowing: the config store, the other files, rdwm's runtime view,
//! and the transformation "control changed -> edit the file -> validate -> write -> reload".
//! Everything here is testable headless.

use super::io::{self, Ev, External, Paths, Queue, Runtime, Store};
use super::persist::{Done, Job, Outcome, Persister, Refused, Submitted, ThemeJob, ThemeRequest, TomlPatch};
use super::schema::{KeyMeta, Schema};
use super::tomlcfg::{Cfg, Val};
use super::ui::{parse_hex, Ctl, Kind, Opt, Row};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What a control reports.
#[derive(Debug, Clone, PartialEq)]
pub enum CtlVal {
    Bool(bool),
    Num(f64),
    Str(String),
    Click,
    /// Right click: back to the default (removes the key).
    Reset,
}

/// What the toast says while accepted changes wait for the disk.
pub const SAVING: &str = "Saving…";

/// An accepted change the persistence worker has not finished, keyed by its ticket in [`Model::pending`].
#[derive(Debug, Clone)]
pub enum Pending {
    Toml(TomlPatch),
    Json { file: &'static str, key: String, value: Option<Value> },
    Theme { selection: Value, done: ThemeDone },
}

/// The message a finished theme transaction shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeDone {
    /// A Theme pane control: "Theme: <id>".
    Selection,
    /// A package card: "Applied <id>…".
    Package,
    /// After the widget-layout question.
    Widgets { keep: bool, empty_desktop: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// UI selections that are not part of any file.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub pane: usize,
    /// Selected entry per editable table (`rule` -> index).
    pub table: HashMap<String, usize>,
    pub bind_filter: String,
    /// Layout whose `[layout.<name>]` options are shown.
    pub layout_opt: Option<String>,
    pub new_bind: NewBind,
    /// Which shortcut is being recorded: "k:<n>" or "new".
    pub recording: Option<String>,
    pub rec_mods: Vec<&'static str>,
    /// Appearance pane: the open tab, the open Customise section and whether the All apps tab shows its advanced options.
    pub tab: super::appearance::Tab,
    pub section: super::appearance::Section,
    pub adv: bool,
    /// A button asked to open another pane (index into `PANES`) / to scroll back to the top; the window acts on both.
    pub goto: Option<usize>,
    pub reset_scroll: bool,
}

#[derive(Debug, Clone, Default)]
pub struct NewBind {
    pub keys: String,
    pub action: String,
    pub args: String,
}

pub struct Model {
    pub paths: Paths,
    pub store: Store,
    pub schema: Schema,
    pub rt: Runtime,
    pub dock: Value,
    pub pinned: Value,
    pub shell: Value,
    pub sel: Selection,
    pub notice: Option<Notice>,
    wallpaper_follow_error_seen: Option<String>,
    /// The file on disk is currently not valid TOML (mid-edit by someone else).
    pub file_error: Option<String>,
    /// Set whenever something visible changed.
    pub dirty: bool,
    pub queue: Queue,
    pub wake: Arc<dyn Fn() + Send + Sync>,
    pub apps: Option<Vec<(String, String)>>,
    pub walls: Option<Vec<std::path::PathBuf>>,
    /// Wallpaper > Wallhaven tab (starts its workers only when opened).
    pub wh: super::wallhaven::Wh,
    pub lw: crate::livewall::pane::State,
    pub wallpaper_palette_name: String,
    pub widget_choice: Option<super::widgetchoice::Pending>,
    /// The open "Create widget" form of the Widgets pane (nothing is saved until Create).
    pub widget_builder: Option<super::widgetbuilder::Draft>,
    /// Keys the builders asked for / found missing in the schema (consistency test).
    pub used: RefCell<Vec<(String, String)>>,
    pub missing: RefCell<Vec<(String, String)>>,
    /// Wallpaper change made through rdwmctl that the file does not know.
    pub synced_at: Option<Instant>,
    /// System panes (sound, network, ...): see `system/mod.rs`.
    pub sys: super::system::Hub,
    /// The Configuration pane (text editor + presets): see `configpane.rs`.
    pub cfg: super::configpane::State,
    /// The one owner of durable writes (see `persist.rs`); dropping the model drains it.
    pub persist: Persister,
    /// Accepted changes not yet committed, by ticket (submission order). They are shown on top of the saved files.
    pub pending: BTreeMap<u64, Pending>,
    /// A file change arrived while our own writes were in flight: look again once they are done.
    recheck_files: bool,
}

pub fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

impl Model {
    pub fn new(paths: Paths, schema: Schema, queue: Queue, wake: Arc<dyn Fn() + Send + Sync>) -> Model {
        let store = Store::load(&paths.rdwm_config, &paths.rdwmctl);
        let (queue_for_worker, wake_for_worker) = (queue.clone(), wake.clone());
        let mut m = Model {
            dock: io::json_load(&paths.dock_settings()),
            pinned: io::json_load(&paths.dock_pinned()),
            shell: io::json_load(&paths.shell_json()),
            sys: super::system::Hub::new(wake.clone()),
            cfg: super::configpane::State::new(crate::preset::Env::from_paths(&paths), wake.clone()),
            paths,
            store,
            schema,
            rt: Runtime::default(),
            sel: Selection::default(),
            notice: None,
            wallpaper_follow_error_seen: None,
            file_error: None,
            dirty: true,
            queue,
            wake,
            apps: None,
            walls: None,
            wh: super::wallhaven::Wh::default(),
            lw: crate::livewall::pane::State::default(),
            wallpaper_palette_name: "Wallpaper colours".into(),
            widget_choice: None,
            widget_builder: None,
            used: RefCell::new(Vec::new()),
            missing: RefCell::new(Vec::new()),
            synced_at: None,
            // Unit tests drive the model without a window or event loop: the same jobs run as they are submitted.
            persist: if cfg!(test) { Persister::inline() } else { Persister::spawn(queue_for_worker, wake_for_worker) },
            pending: BTreeMap::new(),
            recheck_files: false,
        };
        m.rt = io::poll_runtime(&m.paths.socket);
        m.refresh_wallpaper_follow_notice();
        m
    }

    /// The Wallpaper tab of the Appearance pane is on screen.
    pub fn wallpaper_open(&self) -> bool {
        super::panes::PANES.get(self.sel.pane).is_some_and(|p| p.id == "wallpaper")
    }

    pub fn watched_files(&self) -> Vec<std::path::PathBuf> {
        let env = crate::theme::system::Env::discover();
        vec![
            self.paths.rdwm_config.clone(),
            self.paths.dock_settings(),
            self.paths.dock_pinned(),
            self.paths.shell_json(),
            env.state_dir.join("pending"),
            env.state_dir.join("last-result.json"),
            crate::theme::wallpaper_follow::status_path(&self.paths),
        ]
    }

    // ------------------------------------------------------------ notices

    pub fn say(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            crate::log_warn!("notice (error): {text}");
        } else {
            crate::log_info!("notice: {text}");
        }
        self.notice = Some(Notice { text, error, at: Instant::now() });
        self.dirty = true;
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(3300));
            wake();
        });
    }

    /// Drops an expired notice; true when the screen needs a repaint.
    pub fn expire_notice(&mut self) -> bool {
        match &self.notice {
            Some(n) if n.text == SAVING && self.saving() => false,
            Some(n) if !n.error && n.at.elapsed() > Duration::from_secs(3) => {
                self.notice = None;
                true
            }
            _ => false,
        }
    }

    // ------------------------------------------------------------ sync

    /// A watched file changed: re-read everything that changed. Never loses UI state: controls are
    /// rebuilt from the documents on the next paint, selections and edit buffers live elsewhere.
    pub fn on_files_changed(&mut self) {
        if self.saving() {
            // Our own writes are landing; what changed behind them is checked once they are done.
            self.recheck_files = true;
            return;
        }
        self.recheck_files = false;
        match self.store.check_external() {
            External::Reloaded => {
                self.file_error = None;
                self.rebuild_cfg();
                self.say("Reloaded from disk", false);
            }
            External::Invalid(e) => self.file_error = Some(e),
            External::Unchanged => {
                if self.file_error.is_some() && std::fs::read_to_string(&self.store.path).map(|t| Cfg::parse(&t).is_ok()).unwrap_or(false) {
                    self.file_error = None;
                }
            }
        }
        let (d, p, s) = (io::json_load(&self.paths.dock_settings()), io::json_load(&self.paths.dock_pinned()), io::json_load(&self.paths.shell_json()));
        if d != self.dock || p != self.pinned || s != self.shell {
            self.dock = d;
            self.pinned = p;
            self.shell = s;
            self.say("Reloaded from disk", false);
        }
        self.refresh_wallpaper_follow_notice();
        self.dirty = true;
    }

    fn refresh_wallpaper_follow_notice(&mut self) {
        let state = crate::theme::wallpaper_follow::status(&self.paths);
        if crate::theme::wallpaper_follow::enabled(&self.paths) && state.error && !state.generation.is_empty() && self.wallpaper_follow_error_seen.as_deref() != Some(state.generation.as_str()) {
            self.wallpaper_follow_error_seen = Some(state.generation);
            self.say(state.message, true);
        }
    }

    pub fn on_event(&mut self, ev: Ev) {
        match ev {
            Ev::FilesChanged => {
                crate::log_debug!("watched config file changed on disk");
                self.on_files_changed()
            }
            Ev::Runtime(rt) => {
                if rt.observed < self.rt.observed {
                    return;
                }
                let changed = self.rt != *rt;
                let before = self.rt.state["wallpaper"]["image"].as_str().map(String::from);
                self.rt = *rt;
                self.dirty |= changed;
                // the wallpaper was changed behind our back (rdwmctl, a script): "Match wallpaper" follows it
                let now = self.rt.state["wallpaper"]["image"].as_str().map(String::from);
                if let (Some(img), true) = (now.as_deref(), now != before) {
                    super::wallpapercolors::follow(self, img);
                }
            }
            Ev::Persisted(done) => self.on_persisted(*done),
            Ev::Reload(r) => {
                match r {
                    Ok(()) => {}
                    Err(e) => self.say(format!("rdwm rejected the config: {e}"), true),
                }
                self.dirty = true;
            }
        }
    }

    /// Tells rdwm to re-read the file (on a thread: the UI never waits for the socket).
    pub fn reload_rdwm(&self) {
        if !self.rt.running {
            return;
        }
        let (sock, queue, wake) = (self.paths.socket.clone(), self.queue.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let r = io::ipc_call(&sock, &json!({"cmd":"reload"})).map(|_| ());
            queue.lock().unwrap().push_back(Ev::Reload(r));
            // refresh the runtime view right away (new effective config, wallpaper, ...)
            let rt = io::poll_runtime(&sock);
            io::queue_runtime(&queue, rt);
            wake();
        });
    }

    // ------------------------------------------------------------ editing

    /// Applies `f` to a copy of the shown document and hands the keys it changed to the persistence worker, which
    /// validates them like rdwm, writes atomically and then rdwm reloads. True when the change was accepted (it is
    /// shown at once, as pending); a refused or failed change is reported in the notice and taken back.
    pub fn edit(&mut self, f: impl FnOnce(&mut Cfg) -> Result<(), String>) -> bool {
        let mut cfg = self.store.cfg.clone();
        if let Err(e) = f(&mut cfg) {
            self.say(format!("Not saved: {e}"), true);
            return false;
        }
        let patch = match TomlPatch::between(&self.store.cfg.text(), &cfg.text()) {
            Ok(p) if p.is_empty() => return false,
            Ok(p) => p,
            Err(e) => {
                self.say(format!("Not saved: {e}"), true);
                return false;
            }
        };
        let job = Job::Toml { path: self.store.path.clone(), rdwmctl: self.store.rdwmctl.clone(), patch: patch.clone(), epoch: self.store.epoch, base: self.store.saved_stamp() };
        let Some(seq) = self.submit(job, Pending::Toml(patch)) else { return false };
        if self.pending.contains_key(&seq) {
            self.store.cfg = cfg;
        }
        self.dirty = true;
        true
    }

    pub fn edit_json(&mut self, file: &str, key: &str, v: Option<Value>) {
        let (file, path) = match file {
            "dock" => ("dock", self.paths.dock_settings()),
            "pinned" => ("pinned", self.paths.dock_pinned()),
            _ => ("shell", self.paths.shell_json()),
        };
        let job = Job::Json { path, key: key.to_string(), value: v.clone(), reconcile: Some(self.paths.shell_json()) };
        if self.submit(job, Pending::Json { file, key: key.to_string(), value: v.clone() }).is_some_and(|seq| self.pending.contains_key(&seq)) {
            set_key(self.json_mut(file), key, v);
        }
        self.dirty = true;
    }

    /// Runs a theme package transaction in the persistence worker (planned there, against the files as the earlier
    /// jobs leave them). The new selection is shown at once; the theme's other files follow when it committed.
    pub fn apply_theme(&mut self, src: crate::theme::live::Source, paths: Paths, request: ThemeRequest, style: Option<crate::theme::style::Style>, done: ThemeDone) -> bool {
        let selection = match &request {
            ThemeRequest::Selection(s) | ThemeRequest::Widgets { selection: s, .. } => s.clone(),
            ThemeRequest::Package { name, base } => crate::theme::model::Selection { name: name.clone(), ..base.clone() },
        };
        let selection = selection.to_json();
        let Some(seq) = self.submit(Job::Theme(Box::new(ThemeJob { src, paths, request, style })), Pending::Theme { selection: selection.clone(), done }) else { return false };
        if self.pending.contains_key(&seq) {
            set_key(&mut self.shell, "theme", Some(selection));
        }
        self.dirty = true;
        true
    }

    /// The theme selection new Theme controls start from: the newest one accepted, else the saved one.
    pub fn theme_selection(&self) -> crate::theme::model::Selection {
        let shown = self.pending.values().rev().find_map(|p| match p {
            Pending::Theme { selection, .. } => Some(selection),
            Pending::Json { file: "shell", key, value: Some(v) } if key == "theme" => Some(v),
            _ => None,
        });
        match shown {
            Some(v) => crate::theme::model::Selection::from_json(v),
            None => super::themepane::source(self).selection(),
        }
    }

    /// Accepted changes are still on their way to the disk.
    pub fn saving(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Headless tools (no event loop): run the writes on the calling thread as they are submitted.
    pub fn persist_inline(&mut self) {
        debug_assert!(self.pending.is_empty());
        self.persist = Persister::inline();
    }

    /// The ticket of an accepted job (its result may already have been handled in inline mode).
    fn submit(&mut self, job: Job, pending: Pending) -> Option<u64> {
        match self.persist.submit(job) {
            Ok(s) => {
                if let Submitted::Merged { replaced, .. } = s {
                    self.pending.remove(&replaced);
                }
                self.pending.insert(s.seq(), pending);
                self.notice = Some(Notice { text: SAVING.into(), error: false, at: Instant::now() });
                self.dirty = true;
                for d in self.persist.take_inline() {
                    self.on_persisted(d);
                }
                Some(s.seq())
            }
            Err(Refused::Full) => {
                self.say("Not saved: earlier changes are still being written to disk; try again in a moment", true);
                None
            }
            Err(Refused::Closed) => {
                self.say("Not saved: Settings is closing", true);
                None
            }
        }
    }

    fn json_mut(&mut self, file: &str) -> &mut Value {
        match file {
            "dock" => &mut self.dock,
            "pinned" => &mut self.pinned,
            _ => &mut self.shell,
        }
    }

    /// The shown document of a JSON file: `saved` plus the accepted changes not committed yet.
    fn json_shown(&self, file: &str, saved: Value) -> Value {
        let mut doc = saved;
        for p in self.pending.values() {
            match p {
                Pending::Json { file: f, key, value } if *f == file => set_key(&mut doc, key, value.clone()),
                Pending::Theme { selection, .. } if file == "shell" => set_key(&mut doc, "theme", Some(selection.clone())),
                _ => {}
            }
        }
        doc
    }

    /// The shown rdwm document: the saved text plus the pending patches, in order.
    fn rebuild_cfg(&mut self) {
        let mut text = self.store.text.clone();
        for p in self.pending.values() {
            if let Pending::Toml(patch) = p {
                match patch.apply_to(&text) {
                    Ok(t) => text = t,
                    Err(e) => crate::log_debug!("pending change not shown: {e}"),
                }
            }
        }
        if let Ok(cfg) = Cfg::parse(&text) {
            self.store.cfg = cfg;
        }
    }

    fn reload_json_shown(&mut self) {
        self.shell = self.json_shown("shell", io::json_load(&self.paths.shell_json()));
        self.dock = self.json_shown("dock", io::json_load(&self.paths.dock_settings()));
        self.pinned = self.json_shown("pinned", io::json_load(&self.paths.dock_pinned()));
    }

    /// A persistence job finished (results arrive in submission order).
    pub fn on_persisted(&mut self, d: Done) {
        let Some(p) = self.pending.remove(&d.seq) else {
            crate::log_warn!("persist: result for unknown change #{}", d.seq);
            return;
        };
        let idle = self.pending.is_empty();
        let toml_left = self.pending.values().any(|p| matches!(p, Pending::Toml(_) | Pending::Theme { .. }));
        self.dirty = true;
        match (p, d.outcome) {
            (_, Outcome::Toml(Ok(saved))) => {
                self.store.adopt(saved.text, saved.stamp);
                self.rebuild_cfg();
                self.file_error = None;
                if saved.changed && !toml_left {
                    self.reload_rdwm();
                }
                if idle && self.notice.as_ref().is_none_or(|n| !n.error) {
                    self.say(if self.rt.running { "Saved and applied" } else { "Saved (rdwm is not running)" }, false);
                }
            }
            (_, Outcome::Toml(Err(f))) => {
                self.rebuild_cfg();
                if f.conflict {
                    // pick up their version first, so the error stays the last word
                    self.recheck_files = true;
                    if idle {
                        self.on_files_changed();
                    }
                }
                self.say(format!("Not saved: {}", f.message), true);
            }
            (Pending::Json { file, .. }, Outcome::Json(r)) => {
                let saved = match &r {
                    Ok(doc) => doc.clone(),
                    Err(_) => io::json_load(&d.path),
                };
                *self.json_mut(file) = self.json_shown(file, saved);
                match r {
                    Ok(_) if idle && self.notice.as_ref().is_none_or(|n| !n.error) => self.say("Saved", false),
                    Ok(_) => {}
                    Err(e) => self.say(format!("Not saved: {e}"), true),
                }
            }
            (Pending::Theme { done, .. }, Outcome::Theme(r)) => {
                self.store = Store::load(&self.store.path, &self.store.rdwmctl.clone());
                self.rebuild_cfg();
                self.reload_json_shown();
                match r {
                    Ok(a) => {
                        self.file_error = None;
                        let text = match done {
                            ThemeDone::Selection => format!("Theme: {}{}", a.theme_id, if a.notes.is_empty() { String::new() } else { format!(" ({})", a.notes.join("; ")) }),
                            ThemeDone::Package => format!(
                                "Applied {}{}. `decthemectl theme undo` goes back.",
                                a.theme_id,
                                if a.notes.is_empty() { String::new() } else { format!(" ({} setting{} not available yet)", a.notes.len(), if a.notes.len() == 1 { "" } else { "s" }) }
                            ),
                            ThemeDone::Widgets { keep, empty_desktop } => format!(
                                "Applied {} with {}",
                                a.theme_id,
                                if keep {
                                    "your widget layout"
                                } else if empty_desktop {
                                    "the empty desktop layout"
                                } else {
                                    "the theme's widgets"
                                }
                            ),
                        };
                        let text = match (d.style, done) {
                            (Some(st), ThemeDone::Selection) => format!("Shell style: {}", st.title()),
                            _ => text,
                        };
                        if idle || !self.notice.as_ref().is_some_and(|n| n.error) {
                            self.say(text, false);
                        }
                    }
                    Err(e) => self.say(format!("Not applied: {e}"), true),
                }
            }
            (p, o) => crate::log_warn!("persist: mismatched result {o:?} for {p:?}"),
        }
        if idle && self.recheck_files {
            self.on_files_changed();
        }
    }

    /// Tests: the real worker thread, with `hook` run before each of its fsyncs.
    #[cfg(test)]
    pub fn use_worker_with_sync_hook(&mut self, hook: io::fault::SyncHook) {
        assert!(self.pending.is_empty());
        self.persist = Persister::spawn_with_sync_hook(self.queue.clone(), self.wake.clone(), hook);
    }

    /// Tests: waits until the worker finished everything submitted and handles the results.
    #[cfg(test)]
    pub fn settle(&mut self) {
        assert!(self.persist.wait_idle(Duration::from_secs(30)), "persistence worker did not finish");
        let evs: Vec<Ev> = self.queue.lock().unwrap().drain(..).collect();
        for ev in evs {
            self.on_event(ev);
        }
    }

    // ------------------------------------------------------------ value access

    pub fn meta(&self, section: &str, key: &str) -> Option<&KeyMeta> {
        self.used.borrow_mut().push((section.to_string(), key.to_string()));
        let m = self.schema.get(section, key);
        if m.is_none() {
            self.missing.borrow_mut().push((section.to_string(), key.to_string()));
        }
        m
    }

    /// File value, else the schema default.
    pub fn value(&self, section: &str, key: &str) -> Option<Val> {
        if let Some(v) = self.store.cfg.get(section, key) {
            return Some(v);
        }
        if section == "decoration" && key != "style" {
            let style = self.string("decoration", "style").unwrap_or_else(|| "macos".into());
            if let Some(value) = super::decorations::default_value(&style, key) {
                return Some(value);
            }
        }
        let d = &self.schema.get(section, key)?.default;
        match d {
            Value::Bool(b) => Some(Val::Bool(*b)),
            Value::Number(n) => n.as_i64().map(Val::Int).or_else(|| n.as_f64().map(Val::Float)),
            Value::String(s) => Some(Val::Str(s.clone())),
            Value::Array(a) => Some(Val::List(a.iter().filter_map(|x| x.as_str().map(String::from)).collect())),
            _ => None,
        }
    }

    pub fn flag(&self, section: &str, key: &str) -> bool {
        // [snap] enabled defaults to "on when titlebars are on"
        if (section, key) == ("snap", "enabled") && !self.store.cfg.has("snap", "enabled") {
            return self.flag("decoration", "enabled");
        }
        self.value(section, key).and_then(|v| v.as_bool()).unwrap_or(false)
    }

    pub fn num(&self, section: &str, key: &str) -> f64 {
        self.value(section, key).and_then(|v| v.as_f64()).unwrap_or(0.0)
    }

    pub fn string(&self, section: &str, key: &str) -> Option<String> {
        self.value(section, key).and_then(|v| v.as_str().map(String::from))
    }

    pub fn list(&self, section: &str, key: &str) -> Vec<String> {
        match self.value(section, key) {
            Some(Val::List(l)) => l,
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------ row builders (schema driven)

    pub fn r_toggle(&self, sec: &str, key: &str, label: &str, detail: &str) -> Option<Row> {
        self.meta(sec, key)?;
        Some(Row::new(label).detail(detail).ctl(Ctl::new(rid(sec, key), Kind::Toggle(self.flag(sec, key)))))
    }

    pub fn r_slider(&self, sec: &str, key: &str, label: &str, detail: &str, step: f64, unit: &str, cap_max: Option<f64>) -> Option<Row> {
        let m = self.meta(sec, key)?;
        let (min, max) = (m.min.unwrap_or(0.0), cap_max.unwrap_or(m.max.unwrap_or(100.0)).min(m.max.unwrap_or(f64::MAX)));
        let v = self.num(sec, key).clamp(min, max);
        Some(Row::new(label).detail(detail).ctl(Ctl::new(rid(sec, key), Kind::Slider { v, min, max, step, unit: unit.into() })))
    }

    pub fn r_stepper(&self, sec: &str, key: &str, label: &str, detail: &str, step: f64, unit: &str, cap_max: Option<f64>) -> Option<Row> {
        let m = self.meta(sec, key)?;
        let (min, max) = (m.min.unwrap_or(0.0), cap_max.unwrap_or(m.max.unwrap_or(1e6)).min(m.max.unwrap_or(f64::MAX)));
        Some(Row::new(label).detail(detail).ctl(Ctl::new(rid(sec, key), Kind::Stepper { v: self.num(sec, key), min, max, step, unit: unit.into() })))
    }

    fn enum_opts(m: &KeyMeta, labels: Option<&[(&str, &str)]>, unset: Option<&str>) -> Vec<Opt> {
        let mut opts: Vec<Opt> = match labels {
            Some(l) => l.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            None => m.choices.iter().map(|c| (cap(c), c.clone())).collect(),
        };
        if let Some(u) = unset {
            opts.insert(0, (u.to_string(), String::new()));
        }
        opts
    }

    pub fn r_seg(&self, sec: &str, key: &str, label: &str, detail: &str, labels: Option<&[(&str, &str)]>) -> Option<Row> {
        let m = self.meta(sec, key)?;
        let opts = Self::enum_opts(m, labels, None);
        let cur = self.string(sec, key);
        let sel = opts.iter().position(|o| Some(&o.1) == cur.as_ref());
        Some(Row::new(label).detail(detail).ctl(Ctl::new(rid(sec, key), Kind::Seg(opts, sel))))
    }

    pub fn r_popup(&self, sec: &str, key: &str, label: &str, detail: &str, labels: Option<&[(&str, &str)]>, unset: Option<&str>) -> Option<Row> {
        let m = self.meta(sec, key)?;
        let opts = Self::enum_opts(m, labels, unset);
        let cur = self.string(sec, key).unwrap_or_default();
        let sel = opts.iter().position(|o| o.1 == cur);
        Some(Row::new(label).detail(detail).ctl(Ctl::new(rid(sec, key), Kind::Popup(opts, sel))))
    }

    pub fn r_color(&self, sec: &str, key: &str, label: &str, detail: &str) -> Option<Row> {
        self.meta(sec, key)?;
        let hex = self.string(sec, key).unwrap_or_default();
        Some(Row::new(label).detail(detail).ctl(Ctl::new(rid(sec, key), Kind::Color(hex))))
    }

    pub fn r_text(&self, sec: &str, key: &str, label: &str, detail: &str, hint: &str) -> Option<Row> {
        self.meta(sec, key)?;
        let val = match self.value(sec, key) {
            Some(Val::List(l)) => l.join(", "),
            Some(Val::Str(s)) => s,
            _ => String::new(),
        };
        Some(Row::new(label).detail(detail).ctl(Ctl::new(rid(sec, key), Kind::Text { val, hint: hint.into() })))
    }

    // ------------------------------------------------------------ applying a control change

    pub fn apply(&mut self, id: &str, v: CtlVal) {
        let parts: Vec<&str> = id.splitn(3, ':').collect();
        match parts.as_slice() {
            ["r", sec, key] => self.apply_key(sec, key, v),
            ["j", file, key] => self.apply_json(file, key, v),
            _ if id.starts_with("s:") => self.sys_apply(id, v),
            _ => super::panes::apply_special(self, id, v),
        }
    }

    fn apply_key(&mut self, sec: &str, key: &str, v: CtlVal) {
        if sec == "wallpaper" {
            if let Err(error) = crate::theme::wallpaper_follow::cancel(&self.paths) {
                self.say(error, true);
                return;
            }
        }
        let Some(m) = self.schema.get(sec, key).cloned() else {
            self.say(format!("Unknown setting {sec}.{key}"), true);
            return;
        };
        let (sec, key) = (sec.to_string(), key.to_string());
        if v == CtlVal::Reset {
            self.edit(|c| {
                c.remove(&sec, &key);
                Ok(())
            });
            return;
        }
        let val = match to_val(&m, v) {
            Ok(Some(val)) => val,
            Ok(None) => {
                self.edit(|c| {
                    c.remove(&sec, &key);
                    Ok(())
                });
                return;
            }
            Err(e) => {
                self.say(format!("{}: {e}", m.key), true);
                return;
            }
        };
        if sec == "decoration" && key == "style" {
            if let Val::Str(style) = &val {
                self.edit(|c| super::decorations::choose(c, style));
                return;
            }
        }
        self.edit(|c| c.set(&sec, &key, &val));
    }

    fn apply_json(&mut self, file: &str, key: &str, v: CtlVal) {
        let value = match (file, key, v) {
            (_, _, CtlVal::Reset) => None,
            ("shell", "workspaces", CtlVal::Str(s)) => Some(match s.as_str() {
                "on" => json!(true),
                "off" => json!(false),
                _ => json!("auto"),
            }),
            ("dock", _, CtlVal::Num(n)) => Some(json!((n * 1e6).round() / 1e6)),
            (_, _, CtlVal::Bool(b)) => Some(json!(b)),
            (_, _, CtlVal::Str(s)) => Some(json!(s)),
            _ => return,
        };
        self.edit_json(file, key, value);
    }
}

/// Sets (or with `None` removes) a top-level key of a JSON object, keeping the other keys and their order.
fn set_key(doc: &mut Value, key: &str, v: Option<Value>) {
    match v {
        Some(v) => {
            if !doc.is_object() {
                *doc = Value::Object(Default::default());
            }
            doc[key] = v;
        }
        None => {
            if let Some(o) = doc.as_object_mut() {
                o.shift_remove(key);
            }
        }
    }
}

pub fn rid(sec: &str, key: &str) -> String {
    format!("r:{sec}:{key}")
}

/// Converts a control value to a TOML value for `m`, validating type and range. `Ok(None)` = unset.
pub fn to_val(m: &KeyMeta, v: CtlVal) -> Result<Option<Val>, String> {
    let range = |n: f64| -> Result<(), String> {
        match (m.min, m.max) {
            (Some(lo), _) if n < lo => Err(format!("must be at least {lo}")),
            (_, Some(hi)) if n > hi => Err(format!("must be at most {hi}")),
            _ => Ok(()),
        }
    };
    Ok(Some(match (m.ty.as_str(), v) {
        ("bool", CtlVal::Bool(b)) => Val::Bool(b),
        ("int", CtlVal::Num(n)) => {
            range(n.round())?;
            Val::Int(n.round() as i64)
        }
        ("float", CtlVal::Num(n)) => {
            range(n)?;
            Val::Float(n)
        }
        ("enum", CtlVal::Str(s)) => {
            if s.is_empty() {
                return Ok(None);
            }
            if !m.choices.contains(&s) {
                return Err(format!("`{s}` is not one of {}", m.choices.join(", ")));
            }
            Val::Str(s)
        }
        ("color", CtlVal::Str(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(None);
            }
            let hex = t.trim_start_matches('#');
            if !(hex.len() == 6 || hex.len() == 8 && m.key == "preview_color") || parse_hex(hex).is_none() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("colour must look like #rrggbb".into());
            }
            Val::Str(format!("#{}", hex.to_lowercase()))
        }
        ("string", CtlVal::Str(s)) => {
            if s.trim().is_empty() {
                return Ok(None);
            }
            Val::Str(s)
        }
        ("list", CtlVal::Str(s)) => {
            let items: Vec<String> = s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            if items.is_empty() {
                return Ok(None);
            }
            if let Some(hi) = m.max {
                if items.len() as f64 > hi {
                    return Err(format!("at most {hi} entries"));
                }
            }
            Val::List(items)
        }
        (ty, v) => return Err(format!("cannot set a {ty} setting from {v:?}")),
    }))
}

#[cfg(test)]
pub mod tests_support {
    use super::*;
    use std::sync::Mutex;

    pub fn model_with(text: &str) -> (Model, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("decsettings-model-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        let cfg = d.join("config.toml");
        std::fs::write(&cfg, text).unwrap();
        let paths = Paths { rdwm_config: cfg.clone(), shell_dir: d.join("shell"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("no-such-rdwm.sock") };
        let q: Queue = Arc::new(Mutex::new(Default::default()));
        let m = Model::new(paths, Schema::snapshot(), q, Arc::new(|| {}));
        (m, cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::model_with;
    use super::*;

    #[test]
    fn toggling_writes_the_file_and_keeps_everything_else() {
        let (mut m, cfg) = model_with("# mine\n[general]\nborder_px = 2   # thin\nsmart_gaps = false\nfoo_future = 1\n");
        m.apply("r:general:smart_gaps", CtlVal::Bool(true));
        let t = std::fs::read_to_string(&cfg).unwrap();
        assert_eq!(t, "# mine\n[general]\nborder_px = 2   # thin\nsmart_gaps = true\nfoo_future = 1\n");
        assert!(std::fs::metadata(cfg.with_file_name("config.toml.bak-decsettings")).is_ok());
        assert!(m.flag("general", "smart_gaps"));
    }

    #[test]
    fn every_setter_kind() {
        let (mut m, cfg) = model_with("");
        m.apply("r:general:gap_inner", CtlVal::Num(7.4));
        m.apply("r:general:mfact", CtlVal::Num(0.62));
        m.apply("r:decoration:style", CtlVal::Str("classic".into()));
        m.apply("r:colors:focused", CtlVal::Str("#AABBCC".into()));
        m.apply("r:general:tags", CtlVal::Str("web, code ,".into()));
        m.apply("r:wallpaper:image", CtlVal::Str("/tmp/a.png".into()));
        m.apply("r:snap:enabled", CtlVal::Bool(true));
        let t = std::fs::read_to_string(&cfg).unwrap();
        for want in ["gap_inner = 7", "mfact = 0.62", "style = \"classic\"", "focused = \"#aabbcc\"", "tags = [\"web\", \"code\"]", "image = \"/tmp/a.png\"", "[snap]\nenabled = true"] {
            assert!(t.contains(want), "missing {want:?} in\n{t}");
        }
        // empty / reset removes the key again
        m.apply("r:wallpaper:image", CtlVal::Str(String::new()));
        m.apply("r:general:gap_inner", CtlVal::Reset);
        let t = std::fs::read_to_string(&cfg).unwrap();
        assert!(!t.contains("image =") && !t.contains("gap_inner"), "{t}");
    }

    #[test]
    fn invalid_input_is_rejected_and_nothing_is_written() {
        let (mut m, cfg) = model_with("[general]\nborder_px = 1\n");
        let before = std::fs::read_to_string(&cfg).unwrap();
        for (id, v) in [
            ("r:general:border_px", CtlVal::Num(9999.0)),
            ("r:general:border_px", CtlVal::Num(-3.0)),
            ("r:general:mfact", CtlVal::Num(2.0)),
            ("r:decoration:style", CtlVal::Str("fancy".into())),
            ("r:colors:focused", CtlVal::Str("red".into())),
            ("r:colors:focused", CtlVal::Str("#12345".into())),
            ("r:general:smart_gaps", CtlVal::Num(1.0)),
            ("r:general:nosuch", CtlVal::Bool(true)),
        ] {
            m.apply(id, v);
            assert!(m.notice.as_ref().is_some_and(|n| n.error), "{id}");
            m.notice = None;
        }
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), before);
    }

    #[test]
    fn external_edit_updates_the_model_and_says_so() {
        let (mut m, cfg) = model_with("[general]\nborder_px = 1\n");
        std::thread::sleep(Duration::from_millis(15));
        std::fs::write(&cfg, "[general]\nborder_px = 6\n").unwrap();
        m.on_files_changed();
        assert_eq!(m.num("general", "border_px"), 6.0);
        assert_eq!(m.notice.as_ref().unwrap().text, "Reloaded from disk");
        // a half-written file keeps the previous values and flags the error
        std::fs::write(&cfg, "[general]\nborder_px =").unwrap();
        m.on_files_changed();
        assert!(m.file_error.is_some());
        assert_eq!(m.num("general", "border_px"), 6.0);
        std::fs::write(&cfg, "[general]\nborder_px = 8\n").unwrap();
        m.on_files_changed();
        assert!(m.file_error.is_none());
        assert_eq!(m.num("general", "border_px"), 8.0);
    }

    #[test]
    fn snap_enabled_follows_decorations_until_set() {
        let (mut m, _) = model_with("[decoration]\nenabled = true\n");
        assert!(m.flag("snap", "enabled"));
        m.apply("r:snap:enabled", CtlVal::Bool(false));
        assert!(!m.flag("snap", "enabled"));
    }

    #[test]
    fn defaults_come_from_the_schema() {
        let (m, _) = model_with("");
        assert_eq!(m.num("general", "mfact"), 0.55);
        assert_eq!(m.string("decoration", "style").as_deref(), Some("macos"));
        assert_eq!(m.list("general", "layouts"), ["tile", "floating", "monocle"]);
    }

    #[test]
    fn json_settings_keep_unknown_keys() {
        let (mut m, _) = model_with("");
        std::fs::write(m.paths.dock_settings(), "{\"dockSide\":\"bottom\",\"magnification\":1.4}").unwrap();
        m.dock = io::json_load(&m.paths.dock_settings());
        m.apply("j:dock:magnification", CtlVal::Num(1.8));
        let v = io::json_load(&m.paths.dock_settings());
        assert_eq!(v["magnification"], 1.8);
        assert_eq!(v["dockSide"], "bottom");
        m.apply("j:shell:workspaces", CtlVal::Str("off".into()));
        assert_eq!(io::json_load(&m.paths.shell_json())["workspaces"], false);
    }

    #[test]
    fn dock_style_row_writes_the_style_word_decdock_reads() {
        let (mut m, _) = model_with("");
        m.apply("j:dock:style", CtlVal::Str("windows".into()));
        let v = io::json_load(&m.paths.dock_settings());
        assert_eq!(v["style"], "windows");
        let parsed: crate::config::DockSettings = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.style, crate::config::DockStyle::Windows);
        m.apply("j:dock:style", CtlVal::Reset);
        assert!(io::json_load(&m.paths.dock_settings()).get("style").is_none(), "reset removes the key");
    }

    #[test]
    fn hot_corner_rows_write_the_section_dechot_reads() {
        use crate::hot::{Action, Corner, HotConfig};
        let (mut m, _) = model_with("");
        std::fs::write(m.paths.shell_json(), "{\"workspaces\":true}").unwrap();
        m.apply("hc:top_right", CtlVal::Str("spotlight".into()));
        m.apply("hc:bottom_left", CtlVal::Str("custom:notify-send hi".into()));
        m.apply("hc:dwell_ms", CtlVal::Num(225.0));
        let v = io::json_load(&m.paths.shell_json());
        assert_eq!(v["workspaces"], true, "other keys stay");
        let cfg = HotConfig::from_json(&v);
        assert_eq!(*cfg.action(Corner::TopRight), Action::Spotlight);
        assert_eq!(*cfg.action(Corner::BottomLeft), Action::Custom("notify-send hi".into()));
        assert_eq!(*cfg.action(Corner::TopLeft), Action::None);
        assert_eq!(cfg.dwell_ms, 225);
        assert_eq!(m.shell, v, "the model's copy follows");
        // the pane shows what was written, a custom command included
        let rows = m.hot_corner_rows();
        assert_eq!(rows.len(), 5);
        let sel = |i: usize| match &rows[i].ctls[0].kind {
            Kind::Popup(o, Some(s)) => o[*s].1.clone(),
            _ => String::new(),
        };
        assert_eq!(sel(1), "spotlight");
        assert_eq!(sel(2), "custom:notify-send hi");
        assert_eq!(sel(0), "none");
        m.apply("hc:top_left", CtlVal::Str("teleport".into()));
        assert!(m.notice.as_ref().is_some_and(|n| n.error), "an unknown action is refused");
    }
}

#[cfg(test)]
mod wallpaper_follow_notice_tests {
    #[test]
    fn async_photo_failure_is_a_persistent_notice_once_per_request() {
        let (mut m, _) = super::tests_support::model_with("[general]\nborder_px=1\n");
        super::io::json_set(&m.paths.shell_json(), "wallpaper", Some(serde_json::json!({"followTheme":true}))).unwrap();
        let path = crate::theme::wallpaper_follow::status_path(&m.paths);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let fail = |generation: &str| {
            std::fs::write(
                &path,
                serde_json::json!({"generation":generation,"message":"Theme applied; wallpaper unchanged: Wallhaven is unavailable","pending":false,"error":true,"pid":null}).to_string(),
            )
            .unwrap()
        };
        fail("first");
        m.on_files_changed();
        assert!(m.notice.as_ref().is_some_and(|n| n.error && n.text.contains("Theme applied; wallpaper unchanged")));
        let at = m.notice.as_ref().unwrap().at;
        m.on_files_changed();
        assert_eq!(m.notice.as_ref().unwrap().at, at);
        m.notice = None;
        m.on_files_changed();
        assert!(m.notice.is_none(), "dismissed errors must not keep forcing a new notice/scroll");
        fail("second");
        m.on_files_changed();
        assert!(m.notice.as_ref().is_some_and(|n| n.error));
    }
}
