//! Activity Monitor / Task Manager for the Decebal desktop: `decmon`, also the popup the bar's agents icon opens.
//!
//! * **Processes**: apps with their child processes folded in, background processes, sortable by CPU, memory, disk,
//!   pid and name, a search box, End task (SIGTERM) and Force quit (SIGKILL, after a confirmation).
//! * **Performance**: CPU (total and per core), memory and swap, disk, network per interface, GPU, temperatures and
//!   battery power, as live graphs.
//!
//! Data comes straight from `/proc` and `/sys` ([`proc`], [`sample`]) on a worker thread that exists only while the
//! window does, once a second; nothing runs when it is closed. [`model`] groups, sorts and filters, [`act`] decides what
//! may be signalled and delivers the signal, [`ui`] draws and hit-tests. See `docs/procmon.md`.

pub mod act;
pub mod model;
pub mod proc;
pub mod sample;
mod ui;

use crate::app::Button;
use crate::icons::IconCache;
use crate::text::Fonts;
use crate::window_app::{Action, Key, KeyEvent, PopupAnchor, Response, Waker, WindowApp};
use model::{AppIndex, Group, SortKey, VRow, View};
use proc::{Proc, Scanner};
use sample::{Perf, Roots, SysSampler};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use ui::{Hit, Lay};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Processes,
    Performance,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Processes, Tab::Performance];
    pub fn label(self) -> &'static str {
        match self {
            Tab::Processes => "Processes",
            Tab::Performance => "Performance",
        }
    }
    pub fn parse(s: &str) -> Option<Tab> {
        match s {
            "processes" | "proc" | "1" => Some(Tab::Processes),
            "performance" | "perf" | "2" => Some(Tab::Performance),
            _ => None,
        }
    }
}

/// How it is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// A card hanging under the bar (override-redirect popup on X11).
    Popup(PopupAnchor),
    /// A normal resizable window.
    Window,
}

/// One scan of the system: processes, their grouping and the system-wide numbers.
#[derive(Default)]
pub struct Snapshot {
    pub procs: Vec<Proc>,
    pub groups: Vec<Group>,
    pub perf: Perf,
}

type Shared = Arc<Mutex<(u64, Arc<Snapshot>)>>;

/// What a click on End task / Force quit is waiting for.
#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub lines: Vec<String>,
    pub button: &'static str,
    pub sig: i32,
    pub targets: Vec<(u32, u64)>,
}

/// The selected row: a process, or an app group named by its main process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sel {
    pub key: (u32, u64),
    pub group: bool,
}

pub struct Monitor {
    mode: Mode,
    tab: Tab,
    alpha: bool,
    screen: (u32, u32),
    size: (u32, u32),
    fonts: Option<Fonts>,
    icons: IconCache,
    waker: Option<Waker>,
    visible: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    shared: Shared,
    seen: u64,
    snap: Arc<Snapshot>,
    view: View,
    expanded: HashSet<u32>,
    rows: Vec<VRow>,
    sel: Option<Sel>,
    scroll: f32,
    hover: Option<Hit>,
    confirm: Option<Confirm>,
    status: Option<(String, Instant, bool)>,
    perf_sel: usize,
    ctx: act::Ctx,
    root: std::path::PathBuf,
    roots: Roots,
    dump: Option<std::path::PathBuf>,
    theme_seen: u64,
}

/// Where the popup hangs by default: just under the classic bar, or under the pill bar along the top edge (as the dashboard does).
pub fn default_top() -> i32 {
    let th = crate::theme::th();
    let bottom = crate::theme::bar_bottom();
    if th.pill_bar().is_some() {
        bottom.ceil() as i32 + 10
    } else {
        bottom.ceil() as i32 + crate::dash::layout::BAR_GAP as i32
    }
}

pub const POPUP_SIZE: (u32, u32) = (780, 560);
pub const WINDOW_SIZE: (u32, u32) = (960, 660);

impl Monitor {
    pub fn new(mode: Mode, tab: Tab) -> Monitor {
        // SAFETY: getuid/getpid cannot fail.
        let (uid, pid) = unsafe { (libc::getuid(), libc::getpid() as u32) };
        let roots = Roots::default();
        Monitor::with_roots(mode, tab, roots, act::Ctx { my_uid: uid, self_pid: pid })
    }

    pub fn with_roots(mode: Mode, tab: Tab, roots: Roots, ctx: act::Ctx) -> Monitor {
        Monitor {
            mode,
            tab,
            alpha: false,
            screen: (0, 0),
            size: if matches!(mode, Mode::Popup(_)) { POPUP_SIZE } else { WINDOW_SIZE },
            fonts: Fonts::load(),
            icons: IconCache::default(),
            waker: None,
            visible: Arc::new(AtomicBool::new(true)),
            stop: Arc::new(AtomicBool::new(false)),
            shared: Arc::new(Mutex::new((0, Arc::new(Snapshot::default())))),
            seen: 0,
            snap: Arc::new(Snapshot::default()),
            view: View { my_uid: ctx.my_uid, ..View::default() },
            expanded: HashSet::new(),
            rows: Vec::new(),
            sel: None,
            scroll: 0.0,
            hover: None,
            confirm: None,
            status: None,
            perf_sel: 0,
            ctx,
            root: roots.proc.clone(),
            roots,
            dump: std::env::var_os("DECMON_DUMP").map(Into::into),
            theme_seen: crate::theme::generation(),
        }
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    fn popup(&self) -> bool {
        matches!(self.mode, Mode::Popup(_))
    }

    fn lay(&self) -> Lay {
        Lay::new(self.size.0 as f32, self.size.1 as f32, self.popup())
    }

    // ---------------------------------------------------------------- data

    /// Takes the newest scan the worker left and rebuilds the visible rows. True when something changed.
    fn refresh(&mut self) -> bool {
        let mut changed = false;
        let g = crate::theme::generation();
        if g != self.theme_seen {
            self.theme_seen = g;
            changed = true;
        }
        let fresh = self.shared.lock().ok().filter(|s| s.0 != self.seen).map(|s| (s.0, s.1.clone()));
        if let Some((gen, snap)) = fresh {
            self.seen = gen;
            // The pointer is on the list (or a question is open): the rows must not shuffle under it, so a refresh
            // keeps their order and only updates the numbers. Re-sorting waits until the pointer leaves.
            let hold = self.confirm.is_some() || matches!(self.hover, Some(Hit::Row(..) | Hit::End(_) | Hit::Force(_)));
            let pin = hold.then(|| model::pin_order(&self.snap.procs, &self.snap.groups, &self.rows));
            let hovered = self.hover.and_then(|h| match h {
                Hit::Row(i, _) | Hit::End(i) | Hit::Force(i) => self.rows.get(i).and_then(|r| model::row_pid(&self.snap.procs, &self.snap.groups, r)),
                _ => None,
            });
            self.snap = snap;
            self.rows = model::flatten_pinned(&self.snap.procs, &self.snap.groups, &self.view, &self.expanded, pin.as_ref());
            let max = self.max_scroll();
            self.scroll = self.scroll.clamp(0.0, max);
            self.dump_rows();
            // The hovered row may have moved or gone: follow it by pid.
            if let (Some(pid), Some(h)) = (hovered, self.hover) {
                let now = self.rows.iter().position(|r| model::row_pid(&self.snap.procs, &self.snap.groups, r) == Some(pid));
                self.hover = match (h, now) {
                    (Hit::Row(_, chev), Some(i)) => Some(Hit::Row(i, chev)),
                    (Hit::End(_), Some(i)) => Some(Hit::End(i)),
                    (Hit::Force(_), Some(i)) => Some(Hit::Force(i)),
                    _ => None,
                };
            }
            changed = true;
        }
        if self.status.as_ref().is_some_and(|(_, at, _)| at.elapsed() > Duration::from_secs(6)) {
            self.status = None;
            changed = true;
        }
        changed
    }

    pub fn rebuild(&mut self) {
        self.rows = model::flatten(&self.snap.procs, &self.snap.groups, &self.view, &self.expanded);
        let max = self.max_scroll();
        self.scroll = self.scroll.clamp(0.0, max);
        self.dump_rows();
    }

    /// Test hook: `DECMON_DUMP=<file>` gets the visible rows (`G|C pid name cpu% rss`) after every refresh.
    fn dump_rows(&self) {
        let Some(path) = &self.dump else { return };
        let mut out = String::new();
        for r in &self.rows {
            match *r {
                VRow::Section(s, n) => out.push_str(&format!("S {s:?} {n}\n")),
                VRow::Group(g) => {
                    let g = &self.snap.groups[g];
                    out.push_str(&format!("G {} {} {:.1} {}\n", self.snap.procs[g.members[0]].pid, g.name, g.cpu, g.rss));
                }
                VRow::Child(g, m) => {
                    let p = &self.snap.procs[self.snap.groups[g].members[m]];
                    out.push_str(&format!("C {} {} {:.1} {}\n", p.pid, p.name, p.cpu, p.rss));
                }
            }
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    fn start_worker(&mut self, waker: &Waker) {
        let (shared, visible, stop, wake, roots) = (self.shared.clone(), self.visible.clone(), self.stop.clone(), waker.clone(), self.roots.clone());
        std::thread::spawn(move || {
            let mut scanner = Scanner::new(roots.proc.clone());
            let mut sys = SysSampler::new(roots);
            let apps = AppIndex::load();
            let mut gen = 0u64;
            let mut nap = Duration::from_millis(250);
            while !stop.load(Ordering::Relaxed) {
                let now = Instant::now();
                let scan = scanner.scan(now);
                let mut perf = sys.sample(now);
                perf.processes = scan.procs.len();
                perf.threads = scan.procs.iter().map(|p| u64::from(p.threads)).sum();
                let groups = model::build_groups(&scan.procs, &apps);
                gen += 1;
                if let Ok(mut s) = shared.lock() {
                    *s = (gen, Arc::new(Snapshot { procs: scan.procs, groups, perf }));
                }
                wake();
                crate::mem::trim();
                std::thread::sleep(nap);
                nap = Duration::from_secs(1);
                while !visible.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        });
    }

    // ---------------------------------------------------------------- rows and selection

    fn row_key(&self, r: VRow) -> Option<Sel> {
        match r {
            VRow::Section(..) => None,
            VRow::Group(g) => {
                let g = &self.snap.groups[g];
                Some(Sel { key: self.snap.procs[g.members[0]].key(), group: g.members.len() > 1 })
            }
            VRow::Child(g, m) => Some(Sel { key: self.snap.procs[self.snap.groups[g].members[m]].key(), group: false }),
        }
    }

    fn selected_row(&self) -> Option<usize> {
        let s = self.sel?;
        self.rows.iter().position(|r| self.row_key(*r) == Some(s))
    }

    fn max_scroll(&self) -> f32 {
        let l = self.lay();
        (self.rows.len() as f32 * ui::ROW_H - l.list.h).max(0.0)
    }

    fn ensure_visible(&mut self, i: usize) {
        let l = self.lay();
        let (top, bottom) = (i as f32 * ui::ROW_H, (i + 1) as f32 * ui::ROW_H);
        if top < self.scroll {
            self.scroll = top;
        } else if bottom > self.scroll + l.list.h {
            self.scroll = bottom - l.list.h;
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    /// Selects the next / previous process row (sections are skipped).
    fn step(&mut self, delta: i32) -> Response {
        let idx: Vec<usize> = (0..self.rows.len()).filter(|&i| self.row_key(self.rows[i]).is_some()).collect();
        if idx.is_empty() {
            return Response::NONE;
        }
        let cur = self.selected_row().and_then(|r| idx.iter().position(|&i| i == r));
        let next = match cur {
            None => {
                if delta > 0 {
                    0
                } else {
                    idx.len() - 1
                }
            }
            Some(c) => (c as i32 + delta).clamp(0, idx.len() as i32 - 1) as usize,
        };
        self.sel = self.row_key(self.rows[idx[next]]);
        self.ensure_visible(idx[next]);
        Response::REDRAW
    }

    fn set_expanded(&mut self, row: usize, open: bool) -> bool {
        let VRow::Group(g) = self.rows[row] else { return false };
        let grp = &self.snap.groups[g];
        if grp.members.len() < 2 {
            return false;
        }
        let pid = self.snap.procs[grp.members[0]].pid;
        let changed = if open { self.expanded.insert(pid) } else { self.expanded.remove(&pid) };
        if changed {
            self.rebuild();
        }
        changed
    }

    // ---------------------------------------------------------------- ending processes

    /// The processes a selection stands for.
    fn procs_of(&self, s: Sel) -> Vec<&Proc> {
        if s.group {
            if let Some(g) = self.snap.groups.iter().find(|g| self.snap.procs[g.members[0]].key() == s.key) {
                return g.members.iter().map(|&i| &self.snap.procs[i]).collect();
            }
        }
        self.snap.procs.iter().filter(|p| p.key() == s.key).collect()
    }

    fn name_of(&self, s: Sel) -> String {
        if s.group {
            if let Some(g) = self.snap.groups.iter().find(|g| self.snap.procs[g.members[0]].key() == s.key) {
                return g.name.clone();
            }
        }
        self.procs_of(s).first().map_or_else(|| "this process".into(), |p| p.name.clone())
    }

    /// End task (`force` false: SIGTERM) or Force quit (SIGKILL) for `s`. Force quit and session-critical processes ask first.
    pub fn end(&mut self, s: Sel, force: bool) -> Response {
        let procs = self.procs_of(s);
        if procs.is_empty() {
            self.say("That process has already ended.", true);
            return Response::REDRAW;
        }
        let plan = act::plan(&procs, self.ctx);
        let (name, n) = (self.name_of(s), procs.len());
        if plan.targets.is_empty() {
            self.say(&format!("Cannot end {name}: {}", plan.refused.unwrap_or("not allowed")), true);
            return Response::REDRAW;
        }
        let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
        if force || plan.confirm.is_some() {
            let mut lines = Vec::new();
            if force {
                lines.push("Unsaved data in it will be lost.".to_string());
            }
            if n > 1 {
                lines.push(format!("This ends {} processes.", plan.targets.len()));
            }
            if let Some(m) = plan.confirm {
                lines.push(m.to_string());
            }
            if let Some(m) = plan.refused {
                lines.push(format!("Some processes are skipped: {m}"));
            }
            let verb = if force { "Force quit" } else { "End task" };
            self.confirm = Some(Confirm { title: format!("{verb} \"{name}\"?"), lines, button: if force { "Force Quit" } else { "End Task" }, sig, targets: plan.targets });
            return Response::REDRAW;
        }
        self.deliver(&name, sig, &plan.targets);
        Response::REDRAW
    }

    fn deliver(&mut self, name: &str, sig: i32, targets: &[(u32, u64)]) {
        let (mut sent, mut gone, mut err) = (0, 0, None);
        for &t in targets {
            match act::send(&self.root, t, sig) {
                Ok(()) => sent += 1,
                Err(act::SendError::Gone) => gone += 1,
                Err(act::SendError::Reused) => gone += 1,
                Err(act::SendError::Denied(e)) => err = Some(e),
            }
        }
        let what = if sig == libc::SIGKILL { "Force quit" } else { "Asked to quit" };
        match (sent, err) {
            (0, Some(e)) => self.say(&format!("Could not end {name}: {e}"), true),
            (0, None) if gone > 0 => self.say(&format!("{name} had already ended."), false),
            (n, _) => self.say(&format!("{what}: {name}{}", if n > 1 { format!(" ({n} processes)") } else { String::new() }), false),
        }
    }

    fn say(&mut self, msg: &str, error: bool) {
        self.status = Some((msg.to_string(), Instant::now(), error));
    }

    fn accept(&mut self) -> Response {
        if let Some(c) = self.confirm.take() {
            let name = c.title.split('"').nth(1).unwrap_or("process").to_string();
            self.deliver(&name, c.sig, &c.targets);
        }
        Response::REDRAW
    }

    fn open_window(&self) {
        let argv = [crate::launch::sibling_or_path("decmon")];
        crate::launch::spawn_argv(&argv);
    }

    fn set_tab(&mut self, t: Tab) -> Response {
        if t == self.tab {
            return Response::NONE;
        }
        self.tab = t;
        self.hover = None;
        Response::REDRAW
    }

    fn sort_by(&mut self, k: SortKey) {
        if self.view.sort == k {
            self.view.desc = !self.view.desc;
        } else {
            self.view.sort = k;
            self.view.desc = k.default_desc();
        }
        self.rebuild();
    }

    fn click(&mut self, hit: Hit) -> Response {
        match hit {
            Hit::Tab(t) => self.set_tab(t),
            Hit::Link => {
                self.open_window();
                Response::act(Action::Close)
            }
            Hit::Search => Response::NONE,
            Hit::Mine => {
                self.view.mine_only = !self.view.mine_only;
                self.rebuild();
                Response::REDRAW
            }
            Hit::Kernel => {
                self.view.kernel = !self.view.kernel;
                self.rebuild();
                Response::REDRAW
            }
            Hit::Sort(k) => {
                self.sort_by(k);
                Response::REDRAW
            }
            Hit::Row(i, arrow) => {
                let Some(&r) = self.rows.get(i) else { return Response::NONE };
                if let Some(s) = self.row_key(r) {
                    self.sel = Some(s);
                    if arrow {
                        let open = !matches!(self.rows.get(i + 1), Some(VRow::Child(..)));
                        self.set_expanded(i, open);
                    }
                }
                Response::REDRAW
            }
            Hit::End(i) | Hit::Force(i) => {
                let Some(s) = self.rows.get(i).and_then(|r| self.row_key(*r)) else { return Response::NONE };
                self.sel = Some(s);
                self.end(s, matches!(hit, Hit::Force(_)))
            }
            Hit::FootEnd | Hit::FootForce => match self.sel {
                Some(s) => self.end(s, hit == Hit::FootForce),
                None => Response::NONE,
            },
            Hit::ConfirmYes => self.accept(),
            Hit::ConfirmNo => {
                self.confirm = None;
                Response::REDRAW
            }
            Hit::Perf(i) => {
                self.perf_sel = i;
                Response::REDRAW
            }
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let st =
            ui::HitState { tab: self.tab, rows: self.rows.len(), scroll: self.scroll, has_sel: self.sel.is_some(), confirm: self.confirm.is_some(), perf_items: ui::perf_items(&self.snap.perf).len() };
        self.lay().hit(x, y, &st)
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl WindowApp for Monitor {
    fn title(&self) -> String {
        "Activity Monitor".into()
    }

    fn app_id(&self) -> &'static str {
        "decmon"
    }

    fn overlay(&self) -> bool {
        self.popup()
    }

    fn popup_anchor(&self) -> Option<PopupAnchor> {
        match self.mode {
            Mode::Popup(a) => Some(a),
            Mode::Window => None,
        }
    }

    fn popup_translucent(&self) -> bool {
        true
    }

    fn set_popup_alpha(&mut self, alpha: bool) {
        self.alpha = alpha;
    }

    fn input_grabbed(&mut self, mods: Option<u16>) -> Response {
        if mods.is_none() {
            eprintln!("decmon: could not grab the keyboard; click outside to close");
        }
        Response::NONE
    }

    fn initial_size(&self) -> (u32, u32) {
        self.size
    }

    fn content_size(&self) -> Option<(u32, u32)> {
        // Never bigger than the screen.
        let (sw, sh) = self.screen;
        let (w, h) = POPUP_SIZE;
        Some(if sw > 0 { (w.min(sw.saturating_sub(24)), h.min(sh.saturating_sub(80)).max(320)) } else { (w, h) })
    }

    fn set_waker(&mut self, waker: Waker) {
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w());
        self.start_worker(&waker);
        self.waker = Some(waker);
    }

    fn set_screen(&mut self, w: u32, h: u32) {
        self.screen = (w, h);
        if self.popup() {
            if let Some(s) = self.content_size() {
                self.size = s;
            }
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w.max(1), h.max(1));
        self.rebuild();
    }

    fn set_rounded(&mut self, _: bool) {}

    fn set_visible(&mut self, visible: bool) {
        self.visible.store(visible, Ordering::Relaxed);
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        self.size = (pm.width(), pm.height());
        self.paint(pm);
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        let hit = self.hit(x, y);
        if hit != self.hover {
            self.hover = hit;
            return Response::REDRAW;
        }
        Response::NONE
    }

    fn pointer_leave(&mut self) -> Response {
        if self.hover.take().is_some() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if button != Button::Left || !pressed {
            return Response::NONE;
        }
        match self.hit(x, y) {
            Some(h) => self.click(h),
            None => {
                if self.confirm.is_some() {
                    return Response::NONE;
                }
                Response::NONE
            }
        }
    }

    fn scroll(&mut self, _x: f32, _y: f32, dy: f32) -> Response {
        if self.tab != Tab::Processes || self.confirm.is_some() {
            return Response::NONE;
        }
        let old = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
        if (self.scroll - old).abs() > 0.1 {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        if self.confirm.is_some() {
            return match ev.key {
                Key::Enter => self.accept(),
                Key::Escape => {
                    self.confirm = None;
                    Response::REDRAW
                }
                _ => Response::NONE,
            };
        }
        match ev.key {
            Key::Escape => {
                if !self.view.query.is_empty() {
                    self.view.query.clear();
                    self.rebuild();
                    return Response::REDRAW;
                }
                Response::act(Action::Close)
            }
            Key::Tab => {
                let next = if self.tab == Tab::Processes { Tab::Performance } else { Tab::Processes };
                self.set_tab(next)
            }
            Key::Up | Key::Down if self.tab == Tab::Performance => {
                let n = ui::perf_items(&self.snap.perf).len().max(1);
                self.perf_sel = if ev.key == Key::Down { (self.perf_sel + 1) % n } else { (self.perf_sel + n - 1) % n };
                Response::REDRAW
            }
            _ if self.tab == Tab::Performance => Response::NONE,
            Key::Down => self.step(1),
            Key::Up => self.step(-1),
            Key::PageDown => self.step(10),
            Key::PageUp => self.step(-10),
            Key::Home => {
                self.sel = None;
                self.step(1)
            }
            Key::End => {
                self.sel = None;
                self.step(-1)
            }
            Key::Right | Key::Left => {
                if let Some(i) = self.selected_row() {
                    let open = ev.key == Key::Right;
                    if self.set_expanded(i, open) {
                        return Response::REDRAW;
                    }
                }
                Response::NONE
            }
            Key::Delete => match self.sel {
                Some(s) => self.end(s, ev.shift),
                None => Response::NONE,
            },
            Key::Backspace => {
                if self.view.query.pop().is_some() {
                    self.rebuild();
                    return Response::REDRAW;
                }
                Response::NONE
            }
            Key::Char if !ev.ctrl && !ev.alt && !ev.text.is_empty() => {
                self.view.query.push_str(&ev.text);
                self.scroll = 0.0;
                self.rebuild();
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    fn tick(&mut self) -> Response {
        if self.refresh() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::layout::Rect;

    /// A pid no machine has (above `pid_max`), so no test can ever signal a real process.
    const GHOST: u32 = 2_000_000_000;

    fn mon() -> Monitor {
        let roots = Roots { proc: "/nonexistent".into(), sys: "/nonexistent".into(), power: "/nonexistent".into(), nvidia: false };
        Monitor::with_roots(Mode::Window, Tab::Processes, roots, act::Ctx { my_uid: 1000, self_pid: 4000 })
    }

    fn p(pid: u32, ppid: u32, comm: &str, uid: u32, cpu: f32, rss: u64) -> Proc {
        Proc { pid, ppid, start: 1, uid, comm: comm.into(), name: comm.into(), cmdline: vec![comm.into()], state: 'S', cpu, rss, threads: 1, ..Proc::default() }
    }

    fn load(m: &mut Monitor, procs: Vec<Proc>) {
        let apps = AppIndex::new(vec![crate::apps::AppEntry { name: "Browser".into(), exec: "browser".into(), icon: "web-browser".into(), ..Default::default() }]);
        let groups = model::build_groups(&procs, &apps);
        m.snap = Arc::new(Snapshot { procs, groups, perf: Perf::default() });
        m.rebuild();
    }

    fn sample() -> Vec<Proc> {
        vec![
            p(1, 0, "systemd", 0, 0.0, 10),
            p(GHOST, 1, "browser", 1000, 12.0, 1 << 28),
            p(GHOST + 1, GHOST, "browser", 1000, 30.0, 1 << 27),
            p(GHOST + 2, 1, "sleeper", 1000, 0.0, 1 << 20),
            p(GHOST + 3, 1, "rootd", 0, 1.0, 1 << 20),
        ]
    }

    fn key(k: Key, text: &str, shift: bool) -> KeyEvent {
        KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift, alt: false }
    }

    fn type_str(m: &mut Monitor, s: &str) {
        for c in s.chars() {
            m.key(&key(Key::Char, &c.to_string(), false));
        }
    }

    #[test]
    fn typing_searches_escape_clears_then_closes_and_tab_switches() {
        let mut m = mon();
        load(&mut m, sample());
        let all = m.rows.len();
        type_str(&mut m, "sleep");
        assert_eq!(m.view.query, "sleep");
        assert!(m.rows.len() < all && m.rows.iter().any(|r| matches!(r, VRow::Group(_))));
        m.key(&key(Key::Backspace, "", false));
        assert_eq!(m.view.query, "slee");
        assert_eq!(m.key(&key(Key::Escape, "", false)).action, None, "the first Escape only clears the search");
        assert!(m.view.query.is_empty() && m.rows.len() == all);
        assert_eq!(m.key(&key(Key::Escape, "", false)).action, Some(Action::Close));
        assert!(m.key(&key(Key::Tab, "", false)).redraw);
        assert_eq!(m.tab(), Tab::Performance);
        m.key(&key(Key::Tab, "", false));
        assert_eq!(m.tab(), Tab::Processes);
    }

    #[test]
    fn selection_walks_the_rows_and_apps_fold_with_the_arrow_keys() {
        let mut m = mon();
        load(&mut m, sample());
        m.key(&key(Key::Down, "", false));
        let first = m.sel.unwrap();
        m.key(&key(Key::Down, "", false));
        assert_ne!(m.sel.unwrap(), first);
        // select the Browser group (the first row after the Apps header) and unfold it
        m.sel = None;
        m.key(&key(Key::Down, "", false));
        assert!(m.sel.unwrap().group, "the first row is the multi-process app");
        let n = m.rows.len();
        m.key(&key(Key::Right, "", false));
        assert_eq!(m.rows.len(), n + 2, "its two processes are listed");
        m.key(&key(Key::Left, "", false));
        assert_eq!(m.rows.len(), n);
        m.key(&key(Key::End, "", false));
        let last = m.selected_row().unwrap();
        assert_eq!(last, m.rows.len() - 1);
        m.key(&key(Key::Home, "", false));
        assert_eq!(m.selected_row(), Some(1), "row 0 is the section header");
    }

    #[test]
    fn end_task_never_touches_init_or_other_users_and_force_quit_asks() {
        let mut m = mon();
        load(&mut m, sample());
        m.sel = Some(Sel { key: (1, 1), group: false });
        m.key(&key(Key::Delete, "", false));
        assert!(m.confirm.is_none(), "PID 1 is refused outright");
        assert!(m.status.as_ref().unwrap().0.contains("Cannot end") && m.status.as_ref().unwrap().2);
        m.sel = Some(Sel { key: (GHOST + 3, 1), group: false });
        m.key(&key(Key::Delete, "", true));
        assert!(m.confirm.is_none(), "root's process is another user's");
        // Force quit of an own process: a confirmation first, Escape cancels
        m.sel = Some(Sel { key: (GHOST + 2, 1), group: false });
        m.key(&key(Key::Delete, "", true));
        let c = m.confirm.clone().expect("force quit asks");
        assert_eq!((c.sig, c.targets.clone()), (libc::SIGKILL, vec![(GHOST + 2, 1)]));
        assert!(c.title.contains("sleeper"));
        m.key(&key(Key::Escape, "", false));
        assert!(m.confirm.is_none());
        // End task for a whole app lists every process in the confirm only when something needs a warning: here not
        m.sel = Some(Sel { key: (GHOST, 1), group: true });
        m.end(m.sel.unwrap(), true);
        assert_eq!(m.confirm.as_ref().unwrap().targets.len(), 2, "the app and its helper");
        // confirming sends the signal to a pid that does not exist: reported as already ended, nothing is hit
        m.key(&key(Key::Enter, "", false));
        assert!(m.confirm.is_none() && m.status.as_ref().unwrap().0.contains("already ended"), "{:?}", m.status);
    }

    #[test]
    fn header_clicks_sort_and_chips_filter() {
        let mut m = mon();
        load(&mut m, sample());
        let l = m.lay();
        assert_eq!((m.view.sort, m.view.desc), (SortKey::Cpu, true));
        let r = l.cols[3];
        m.pointer_button(r.cx(), r.cy(), Button::Left, true);
        assert_eq!((m.view.sort, m.view.desc), (SortKey::Mem, true), "numbers start with the biggest");
        m.pointer_button(r.cx(), r.cy(), Button::Left, true);
        assert!(!m.view.desc, "a second click reverses");
        let r = l.cols[0];
        m.pointer_button(r.cx(), r.cy(), Button::Left, true);
        assert_eq!((m.view.sort, m.view.desc), (SortKey::Name, false));
        let before = m.rows.len();
        m.pointer_button(l.mine.cx(), l.mine.cy(), Button::Left, true);
        assert!(m.view.mine_only && m.rows.len() < before, "root's processes are gone");
        assert_eq!(m.pointer_button(l.mine.cx(), l.mine.cy(), Button::Right, true), Response::NONE, "only the left button");
        m.pointer_button(l.kernel.cx(), l.kernel.cy(), Button::Left, true);
        assert!(m.view.kernel);
    }

    #[test]
    fn the_wheel_scrolls_within_bounds() {
        let mut m = mon();
        let mut many = sample();
        many.extend((10..200).map(|i| p(GHOST + i, 1, &format!("p{i}"), 1000, 0.0, 1)));
        load(&mut m, many);
        assert!(m.max_scroll() > 100.0);
        assert!(m.scroll(0.0, 0.0, 60.0).redraw);
        assert_eq!(m.scroll, 60.0);
        m.scroll(0.0, 0.0, -500.0);
        assert_eq!(m.scroll, 0.0);
        m.scroll(0.0, 0.0, 1e9);
        assert_eq!(m.scroll, m.max_scroll());
        // keeping a row selected scrolls it into view
        m.sel = None;
        m.scroll = 0.0;
        for _ in 0..60 {
            m.key(&key(Key::Down, "", false));
        }
        let i = m.selected_row().unwrap() as f32;
        assert!(i * ui::ROW_H >= m.scroll && (i + 1.0) * ui::ROW_H <= m.scroll + m.lay().list.h + 0.5);
    }

    fn painted(pm: &Pixmap) -> usize {
        pm.data().chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn draws_both_tabs_the_dialog_and_the_popup_without_panicking() {
        for mode in [Mode::Window, Mode::Popup(PopupAnchor { top: 30, center_x: Some(400), edge: crate::window_app::Edge::Top })] {
            let roots = Roots { proc: "/nonexistent".into(), sys: "/nonexistent".into(), power: "/nonexistent".into(), nvidia: false };
            let mut m = Monitor::with_roots(mode, Tab::Processes, roots, act::Ctx { my_uid: 1000, self_pid: 4000 });
            load(&mut m, sample());
            let mut perf = Perf { primed: true, cores: vec![0.1, 0.9, 0.5, 0.0], cpu_hist: vec![0.1, 0.5, 0.3], ..Perf::default() };
            perf.core_hist = vec![vec![0.1, 0.2]; 4];
            perf.nets = vec![sample::NetIf { name: "eth0".into(), rx: 1e6, tx: 2e5, rx_hist: vec![1.0, 2.0], tx_hist: vec![0.5, 0.1], ..Default::default() }];
            perf.gpus = vec![sample::GpuStat {
                gpu: sample::Gpu { name: "Test GPU".into(), util: Some(30.0), mem_used: Some(1 << 30), mem_total: Some(8 << 30), temp: Some(55.0), ..Default::default() },
                util_hist: vec![0.2, 0.3],
                mem_hist: vec![0.1, 0.1],
            }];
            perf.battery = Some(sample::Battery { percent: 60, status: "Discharging".into(), watts: 8.0 });
            perf.power_hist = vec![7.0, 8.0];
            Arc::get_mut(&mut m.snap).unwrap().perf = perf;
            let (w, h) = m.size;
            for tab in Tab::ALL {
                m.tab = tab;
                for item in 0..7 {
                    m.perf_sel = item;
                    m.alpha = item % 2 == 0;
                    let mut pm = Pixmap::new(w, h).unwrap();
                    m.draw(&mut pm);
                    assert!(painted(&pm) > (w * h) as usize / 2, "{mode:?} {tab:?} {item}");
                }
            }
            m.tab = Tab::Processes;
            m.sel = Some(Sel { key: (GHOST + 2, 1), group: false });
            m.key(&key(Key::Delete, "", true));
            let mut pm = Pixmap::new(w, h).unwrap();
            m.draw(&mut pm);
            assert!(m.confirm.is_some());
            // too small a window must not panic either
            let mut small = Pixmap::new(300, 200).unwrap();
            m.confirm = None;
            m.draw(&mut small);
            m.tab = Tab::Performance;
            m.draw(&mut small);
        }
    }

    #[test]
    fn the_popup_is_anchored_and_never_bigger_than_the_screen() {
        let mut m = Monitor::with_roots(
            Mode::Popup(PopupAnchor { top: 34, center_x: Some(500), edge: crate::window_app::Edge::Top }),
            Tab::Processes,
            Roots { proc: "/nonexistent".into(), sys: "/nonexistent".into(), power: "/nonexistent".into(), nvidia: false },
            act::Ctx { my_uid: 1, self_pid: 2 },
        );
        assert_eq!(m.popup_anchor(), Some(PopupAnchor { top: 34, center_x: Some(500), edge: crate::window_app::Edge::Top }));
        assert!(m.overlay());
        assert_eq!(m.content_size(), Some(POPUP_SIZE));
        m.set_screen(700, 500);
        assert_eq!(m.content_size(), Some((676, 420)));
        let w = mon();
        assert!(!w.overlay() && w.popup_anchor().is_none());
        assert_eq!(Tab::parse("perf"), Some(Tab::Performance));
        let _ = Rect::default();
    }
}
