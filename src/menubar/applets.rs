//! The bar's side of [`crate::panel`]: which applets this decbar shows right now, in its renderer's order, and what
//! pressing one does. Every renderer (the classic strip, the floating bar, the pill rail, the Dux band and the taskbar's
//! status engine) goes through here, so an applet behaves the same whatever draws it; each renderer only paints it.

use super::popup::{Act, Icon, Row};
use super::*;
#[cfg(test)]
use crate::panel::AppletSet;
use crate::panel::{AppletId as A, Edge, Effective, Renderer};

#[cfg(test)]
thread_local! {
    /// Tests choose the applets and the edge instead of reading the developer's settings.
    pub(super) static TEST_APPLETS: std::cell::Cell<Option<AppletSet>> = const { std::cell::Cell::new(None) };
    pub(super) static TEST_EDGE: std::cell::Cell<Option<Edge>> = const { std::cell::Cell::new(None) };
    pub(super) static TEST_EXPLICIT: std::cell::Cell<AppletSet> = const { std::cell::Cell::new(AppletSet::EMPTY) };
    /// The Dux renderer without publishing a process-wide theme.
    pub(super) static TEST_DUX: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A media player for the media applet.
    pub(super) static TEST_PLAYER: std::cell::RefCell<Option<crate::dash::mpris::Player>> = const { std::cell::RefCell::new(None) };
}

/// Whether the sibling program `name` is installed (asked once per process).
pub(super) fn have_program(name: &'static str) -> bool {
    if cfg!(test) {
        return true;
    }
    static CACHE: Mutex<Vec<(&'static str, bool)>> = Mutex::new(Vec::new());
    let Ok(mut cache) = CACHE.lock() else { return true };
    if let Some((_, v)) = cache.iter().find(|(n, _)| *n == name) {
        return *v;
    }
    let path = crate::launch::sibling_or_path(name);
    let v = std::path::Path::new(&path).is_file() || sys::have(name);
    cache.push((name, v));
    v
}

/// Starts a Decebal application program (unit tests only record it).
fn launch_app(argv: &[&str]) {
    if cfg!(test) {
        return vbar::launch_ui(argv.iter().map(|s| s.to_string()).collect());
    }
    sys::spawn_application(argv);
}

/// The applet a bar item belongs to (`None` for the overflow cell).
pub(super) fn applet_of(kind: Tray) -> Option<A> {
    Some(match kind {
        Tray::Indicator(_) | Tray::Indicators => A::Tray,
        Tray::Workspaces => A::Workspaces,
        Tray::Layout => A::Layout,
        Tray::Weather => A::Weather,
        Tray::Awake => A::StayAwake,
        Tray::Agents => A::Agents,
        Tray::Battery => A::Battery,
        Tray::Bluetooth => A::Bluetooth,
        Tray::Audio => A::Volume,
        Tray::Network => A::Network,
        Tray::Dash => A::Dashboard,
        Tray::Spotlight => A::Search,
        Tray::Control => A::ControlCenter,
        Tray::Clock => A::Clock,
        Tray::Logo => A::Launcher,
        Tray::Window => A::WindowTitle,
        Tray::Power => A::Power,
        Tray::Lock => A::LockKeys,
        Tray::AppMenu => A::AppMenu,
        Tray::Media => A::Media,
        Tray::Notifications => A::Notifications,
        Tray::Clipboard => A::Clipboard,
        Tray::Cpu => A::Cpu,
        Tray::Memory => A::Memory,
        Tray::Brightness => A::Brightness,
        Tray::Task(_) => A::Tasks,
        Tray::ShowDesktop => A::ShowDesktop,
        Tray::More => return None,
    })
}

/// The one bar item of a single-cell applet (task buttons and the tray have several).
pub(super) fn single(id: A) -> Option<Tray> {
    Some(match id {
        A::Launcher => Tray::Logo,
        A::AppMenu => Tray::AppMenu,
        A::Workspaces => Tray::Workspaces,
        A::WindowTitle => Tray::Window,
        A::Layout => Tray::Layout,
        A::Media => Tray::Media,
        A::Clock => Tray::Clock,
        A::Weather => Tray::Weather,
        A::Clipboard => Tray::Clipboard,
        A::Cpu => Tray::Cpu,
        A::Memory => Tray::Memory,
        A::Notifications => Tray::Notifications,
        A::StayAwake => Tray::Awake,
        A::Agents => Tray::Agents,
        A::LockKeys => Tray::Lock,
        A::Volume => Tray::Audio,
        A::Network => Tray::Network,
        A::Bluetooth => Tray::Bluetooth,
        A::Battery => Tray::Battery,
        A::Brightness => Tray::Brightness,
        A::Search => Tray::Spotlight,
        A::Dashboard => Tray::Dash,
        A::ControlCenter => Tray::Control,
        A::Power => Tray::Power,
        A::ShowDesktop => Tray::ShowDesktop,
        A::Tasks | A::Tray => return None,
    })
}

/// The classic strip's right-hand items, left to right (the menus and the launcher sit at the left edge). The order of
/// the items it always had is kept; the others join where a menu bar puts them.
pub(super) const CLASSIC_ORDER: &[A] = &[
    A::WindowTitle,
    A::Tasks,
    A::Tray,
    A::Workspaces,
    A::Layout,
    A::Media,
    A::Weather,
    A::Cpu,
    A::Memory,
    A::StayAwake,
    A::Agents,
    A::LockKeys,
    A::Battery,
    A::Bluetooth,
    A::Volume,
    A::Brightness,
    A::Network,
    A::Clipboard,
    A::Notifications,
    A::Dashboard,
    A::Search,
    A::ControlCenter,
    A::Clock,
    A::Power,
    A::ShowDesktop,
];

/// The rail's groups, in reading order: single head pills, the tray pill (actions), the status pill (state) and the
/// single tail pills. Workspaces, tasks and the focused window have pills of their own between head and tray.
pub(super) const RAIL_HEAD: &[A] = &[A::Launcher, A::AppMenu];
pub(super) const RAIL_TRAY: &[A] = &[A::Layout, A::Media, A::StayAwake, A::Agents, A::Clipboard, A::Notifications, A::Search, A::Dashboard, A::ControlCenter];
pub(super) const RAIL_STATUS: &[A] = &[A::LockKeys, A::Weather, A::Cpu, A::Memory, A::Volume, A::Brightness, A::Network, A::Bluetooth, A::Battery];
pub(super) const RAIL_TAIL: &[A] = &[A::Clock, A::Power, A::ShowDesktop];

/// The Dux group (0 start, 1 centre, 2 end) an applet joins when it is enabled but not in the theme's item lists.
pub(super) fn dux_group(id: A) -> usize {
    match id {
        A::Launcher | A::AppMenu | A::Workspaces | A::WindowTitle | A::Tasks => 0,
        A::Media | A::Clock | A::Weather => 1,
        _ => 2,
    }
}

/// One task button of the bar: the app and the window a click focuses.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Task {
    pub app_id: String,
    pub title: String,
    /// (window, focused) of every window of the app, the order clicks walk through.
    pub windows: Vec<(u64, bool)>,
}

impl Task {
    pub fn key(&self) -> u64 {
        self.windows.first().map_or(0, |w| w.0)
    }
    pub fn active(&self) -> bool {
        self.windows.iter().any(|w| w.1)
    }
}

/// Groups windows by application in window-list order (one button per app, as the taskbar's combined mode).
pub(super) fn group_tasks(windows: &[crate::wm::Window]) -> Vec<Task> {
    let mut out: Vec<Task> = Vec::new();
    for w in windows.iter().filter(|w| !w.app_id.is_empty()) {
        match out.iter_mut().find(|t| crate::wm::app_matches(&t.app_id, &w.app_id)) {
            Some(t) => t.windows.push((w.id, w.active)),
            None => out.push(Task { app_id: w.app_id.clone(), title: w.title.clone(), windows: vec![(w.id, w.active)] }),
        }
        if w.active {
            if let Some(t) = out.iter_mut().find(|t| crate::wm::app_matches(&t.app_id, &w.app_id)) {
                t.title = w.title.clone();
            }
        }
    }
    out
}

impl DecBar {
    /// This bar's effective panel: renderer, edge and enabled applets (`decbar --position` moves it).
    pub(super) fn panel(&self) -> Effective {
        let th = crate::theme::th();
        #[allow(unused_mut)]
        let mut p = if self.embedded {
            crate::panel::resolve(&crate::panel::Inputs {
                renderer: Renderer::Taskbar,
                user: &th.panel_user,
                theme: &th.panel_theme,
                legacy_position: None,
                legacy_workspaces: None,
                dux: None,
                dock_user_edge: None,
            })
        } else {
            th.bar_panel()
        };
        #[cfg(test)]
        {
            if !self.embedded {
                if TEST_VERTICAL.with(|f| f.get()) {
                    let (edge, split) = Edge::of_position(TEST_POSITION.with(|p| p.get()));
                    p = Effective { renderer: Renderer::Pill, edge, split, applets: Renderer::Pill.defaults(), explicit: AppletSet::EMPTY };
                } else if TEST_FLOATING.with(|f| f.get()) {
                    p.renderer = Renderer::Floating;
                } else if TEST_DUX.with(|f| f.get()) {
                    p.renderer = Renderer::Dux;
                    p.applets = crate::panel::dux_items(&crate::config::BarLayout::default());
                }
            }
            if let Some(a) = TEST_APPLETS.with(|t| t.get()) {
                p.applets = a;
            }
            p.explicit = TEST_EXPLICIT.with(|t| t.get());
            if let Some(e) = TEST_EDGE.with(|t| t.get()) {
                p.edge = e;
                p.split = false;
            }
        }
        if let (Some(over), false) = (self.position_override, self.embedded) {
            let (edge, split) = Edge::of_position(over);
            p.edge = edge;
            p.split = split && p.renderer.supports_split();
        }
        p
    }

    pub(super) fn shows(&self, id: A) -> bool {
        self.panel().shows(id)
    }

    /// Whether an enabled applet has something to show on this machine right now (see [`crate::panel::Need`]).
    pub(super) fn applet_ready(&self, id: A, p: &Effective, snap: &Snapshot) -> bool {
        match id {
            A::Launcher | A::AppMenu | A::Clock | A::Search | A::Dashboard | A::ShowDesktop | A::Cpu | A::Memory => true,
            A::Workspaces => self.show_workspaces && !self.bar_workspaces().is_empty(),
            A::WindowTitle => self.v_win.is_some(),
            A::Tasks => !self.tasks.is_empty(),
            A::Layout => self.layout_state().is_some(),
            A::Media => self.dux_player().is_some(),
            A::Weather => self.shared.weather.lock().is_ok_and(|w| w.is_some()),
            A::Tray => self.indicators.items.iter().any(|i| i.status != crate::sni::Status::Passive),
            A::Clipboard => have_program("decclip"),
            A::Notifications => have_program("decnotify"),
            A::Power => have_program("decsession"),
            A::StayAwake => snap.awake || p.explicit.contains(id),
            A::Agents => symbols::agents_available(),
            A::LockKeys => self.locks.caps() || self.locks.num() || p.explicit.contains(id),
            A::Volume => snap.volume.is_some(),
            A::Network => snap.net.is_some(),
            A::Bluetooth => snap.bluetooth.is_some(),
            A::Battery => snap.battery.is_some(),
            A::Brightness => snap.brightness.is_some(),
            A::ControlCenter => crate::theme::th().control_center.enabled,
        }
    }

    /// The workspaces this bar shows: rdwm's tags when it reports them, else the window manager's desktops.
    pub(super) fn bar_workspaces(&self) -> Vec<Workspace> {
        self.v_workspaces()
    }

    /// The items of `order` that are on and ready, expanded (the tray to its application cells, limited to `tray_slots`;
    /// the task applet to one cell per application).
    pub(super) fn applet_items(&self, order: &[A], tray_slots: usize) -> Vec<Tray> {
        let p = self.panel();
        let snap = self.snapshot();
        let mut out = Vec::new();
        for id in order.iter().copied().filter(|id| p.shows(*id) && self.applet_ready(*id, &p, &snap)) {
            match id {
                A::Tray => out.extend(self.indicator_kinds(tray_slots)),
                A::Tasks => out.extend(self.tasks.iter().map(|t| Tray::Task(t.key()))),
                other => out.extend(single(other)),
            }
        }
        out
    }

    /// Re-reads the window list for the task buttons (only while the applet is on). True when it changed.
    pub(super) fn refresh_tasks(&mut self) -> bool {
        let next = if self.shows(A::Tasks) || self.shows(A::ShowDesktop) { group_tasks(&self.active.windows()) } else { Vec::new() };
        if next == self.tasks {
            return false;
        }
        self.tasks = next;
        true
    }

    pub(super) fn task(&self, key: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.key() == key)
    }

    /// Show desktop: minimise every window, the next press brings them back (as the taskbar's sliver).
    pub(super) fn show_desktop(&mut self) {
        let windows = self.active.windows();
        if windows.iter().any(|w| w.active) || self.desk_hidden.is_empty() {
            self.desk_hidden = windows.iter().map(|w| w.id).collect();
            for w in &windows {
                self.active.minimize_window(w.id);
            }
        } else {
            for id in std::mem::take(&mut self.desk_hidden) {
                self.active.activate_window(id);
            }
        }
    }

    /// A left click on a task button: focus it, walk through its windows, or minimise the focused one.
    pub(super) fn task_click(&mut self, key: u64) {
        let Some(t) = self.task(key) else { return };
        match crate::taskbar::click_action(&t.windows) {
            crate::taskbar::TaskClick::Activate(id) => self.active.activate_window(id),
            crate::taskbar::TaskClick::Minimize(id) => self.active.minimize_window(id),
            crate::taskbar::TaskClick::Launch => {}
        }
    }

    /// The launcher applet's program and where it opens: `declaunch` beside the applet (see docs/universal-panels.md).
    pub(super) fn open_launcher(&self, at: Option<crate::panel::frame::LaunchAnchor>) {
        let mut argv = vec![crate::launch::sibling_or_path("declaunch"), "toggle".to_string()];
        if let Some(a) = at {
            argv.extend(a.args());
        }
        vbar::launch_ui(argv);
    }

    /// Where a program opened from the surface rectangle `r` should hang: the panel's edge, the rectangle's centre in
    /// global pixels and the panel's thickness.
    pub(super) fn anchor_of(&self, r: (f32, f32, f32, f32)) -> crate::panel::frame::LaunchAnchor {
        let edge = self.panel().edge;
        let thickness = crate::theme::th().bar_thickness(edge);
        crate::panel::frame::LaunchAnchor { edge, x: self.indicator_origin.0 as f32 + r.0 + r.2 / 2.0, y: self.indicator_origin.1 as f32 + r.1 + r.3 / 2.0, thickness }
    }

    /// What a press on a bar item does, for every renderer. `r` is the item's rectangle on the surface. Items whose
    /// behaviour is the renderer's own (workspace cells, application indicators, the clock of the rail) are handled
    /// there first; this is the shared rest. True when something happened.
    pub(super) fn applet_press(&mut self, kind: Tray, r: (f32, f32, f32, f32), button: Button) -> bool {
        match button {
            Button::Middle => {
                if kind == Tray::Audio {
                    self.toggle_mute_now();
                    if matches!(self.open, Open::Popup(Tray::Audio)) {
                        self.rebuild_popup();
                    }
                    return true;
                }
                return false;
            }
            Button::Right => {
                return match kind {
                    Tray::Awake | Tray::Task(_) => {
                        self.toggle_popup(kind);
                        true
                    }
                    _ => false,
                };
            }
            Button::Left => {}
        }
        let span = Span { x: r.0, w: r.2 };
        match kind {
            Tray::Logo => {
                self.close();
                if self.panel().renderer == Renderer::Pill {
                    // Caelestia's OS icon opens its search-first launcher
                    self.spotlight();
                } else {
                    let at = self.anchor_of(r);
                    self.open_launcher(Some(at));
                }
            }
            Tray::Spotlight => {
                self.close();
                self.spotlight();
            }
            Tray::Dash => {
                self.close();
                self.dashboard(span);
            }
            Tray::Control => {
                let was = self.open == Open::Control;
                self.close();
                if !was {
                    self.open_control();
                }
            }
            Tray::Power => {
                self.close();
                vbar::launch_ui(vec![crate::launch::sibling_or_path("decsession")]);
            }
            Tray::Awake => {
                self.close();
                if self.snapshot().awake {
                    self.awake_off();
                } else {
                    awake::set(true);
                    self.update_snapshot(|s| s.awake = true);
                }
            }
            Tray::Agents | Tray::Cpu | Tray::Memory => {
                self.close();
                let along = if self.panel().edge.is_vertical() { r.1 + r.3 / 2.0 } else { r.0 + r.2 / 2.0 };
                let edge = self.panel().edge;
                let gap = crate::theme::th().bar_thickness(edge) as i32 + 10;
                self.process_monitor(vec!["--edge".into(), edge.as_str().into(), "--top".into(), gap.to_string(), "--x".into(), (along.round() as i32).to_string()]);
            }
            Tray::Notifications => {
                self.close();
                launch_app(&["decnotify", "history"]);
            }
            Tray::Clipboard => {
                self.close();
                launch_app(&["decclip"]);
            }
            Tray::ShowDesktop => {
                self.close();
                self.show_desktop();
            }
            Tray::Task(key) => {
                self.close();
                self.task_click(key);
            }
            Tray::Lock => return false,
            Tray::Workspaces => return false,
            Tray::Layout
            | Tray::Weather
            | Tray::Battery
            | Tray::Bluetooth
            | Tray::Audio
            | Tray::Network
            | Tray::Clock
            | Tray::Window
            | Tray::AppMenu
            | Tray::Media
            | Tray::Brightness
            | Tray::More
            | Tray::Indicators
            | Tray::Indicator(_) => self.toggle_popup(kind),
        }
        true
    }

    /// Opens the popup of `kind`, or closes it when it is the one open.
    pub(super) fn toggle_popup(&mut self, kind: Tray) {
        if self.open == Open::Popup(kind) {
            self.close();
        } else {
            if kind == Tray::AppMenu {
                self.app_menu_page = None;
            }
            self.open_popup(kind);
        }
    }

    /// The wheel over an applet, for every renderer. True when something changed.
    pub(super) fn applet_scroll(&mut self, kind: Tray, dy: f32) -> bool {
        let up = if dy > 0.0 { -1.0 } else { 1.0 };
        match kind {
            Tray::Indicator(key) => self.indicator_scroll(key, dy),
            Tray::Audio => {
                let Some(v) = self.snapshot().volume else { return false };
                let cur = if v.muted { 0.0 } else { v.level };
                self.set_volume_now((cur + up * 0.05).clamp(0.0, 1.0));
                if matches!(self.open, Open::Popup(Tray::Audio)) {
                    self.rebuild_popup();
                }
                true
            }
            Tray::Brightness => {
                let Some(b) = self.snapshot().brightness else { return false };
                let pct = (b as i32 + (up * 5.0) as i32).clamp(0, 100) as u32;
                self.update_snapshot(|s| s.brightness = Some(pct));
                sys::set_brightness(pct);
                if matches!(self.open, Open::Popup(Tray::Brightness)) {
                    self.rebuild_popup();
                }
                true
            }
            Tray::Layout => {
                rdwm::send_action(if dy > 0.0 { "cyclelayout" } else { "cyclelayout_back" }, &[]);
                false
            }
            Tray::Workspaces => {
                rdwm::send_action(if dy > 0.0 { "viewnext" } else { "viewprev" }, &[]);
                false
            }
            _ => false,
        }
    }

    /// Popup rows of the applets added with the registry (the others are in [`DecBar::build_popup`]).
    pub(super) fn applet_rows(&self, kind: Tray) -> Option<(Vec<Row>, f32, bool)> {
        match kind {
            Tray::AppMenu => Some((self.app_menu_rows(), 260.0, true)),
            Tray::Media => {
                let player = self.dux_player()?;
                let playing = player.status == crate::dash::mpris::Status::Playing;
                let mut rows = vec![Row::header("Now Playing").detail(&player.identity)];
                rows.push(Row::text(if player.meta.title.is_empty() { "Unknown track" } else { &player.meta.title }));
                if !player.meta.artist.is_empty() {
                    rows.push(Row::text(&player.meta.artist));
                }
                rows.push(Row::sep());
                rows.push(Row::item("Previous", Act::MediaPrev).icon(Icon::Symbol("backward")).enabled(player.can_prev));
                rows.push(Row::item(if playing { "Pause" } else { "Play" }, Act::MediaToggle).icon(Icon::Symbol(if playing { "pause" } else { "play" })).enabled(player.can_control));
                rows.push(Row::item("Next", Act::MediaNext).icon(Icon::Symbol("forward")).enabled(player.can_next));
                if have_program("decdash") {
                    rows.push(Row::sep());
                    rows.push(Row::item("Media controls…", Act::MediaOpen));
                }
                Some((rows, 280.0, false))
            }
            Tray::Brightness => {
                let b = self.snapshot().brightness?;
                Some((vec![Row::header("Display").detail(&format!("{b}%")), Row::slider(b as f32 / 100.0, "sun-small", Act::BrightnessSlider)], 260.0, false))
            }
            Tray::Task(key) => {
                let t = self.task(key)?;
                let mut rows = vec![Row::header(&active::display_name(&t.app_id))];
                if !t.title.is_empty() {
                    rows.push(Row::text(&t.title));
                }
                rows.push(Row::sep());
                let first = t.windows.first().map_or(0, |w| w.0);
                rows.push(Row::item("Bring to front", Act::TaskFocus(first)));
                rows.push(Row::item("Minimise", Act::TaskMinimize(first)));
                rows.push(Row::item(if t.windows.len() > 1 { "Close window" } else { "Close" }, Act::TaskClose(first)));
                Some((rows, 260.0, false))
            }
            Tray::More => {
                let mut rows = vec![Row::header("More")];
                for k in &self.hidden_items() {
                    let title = match k {
                        Tray::Task(key) => self.task(*key).map(|t| active::display_name(&t.app_id)).unwrap_or_default(),
                        Tray::Indicator(_) | Tray::Indicators => "Applications".to_string(),
                        k => applet_of(*k).map_or("", A::title).to_string(),
                    };
                    rows.push(Row::item(&title, Act::OpenApplet(*k)).icon(Icon::Symbol(classic_symbol(*k))));
                }
                Some((rows, 240.0, true))
            }
            _ => None,
        }
    }

    /// The folded application menus: the list of menus, or one menu's items with a way back.
    fn app_menu_rows(&self) -> Vec<Row> {
        match self.app_menu_page.and_then(|i| self.menus.get(i).map(|m| (i, m))) {
            None => {
                let mut rows = vec![Row::header(&self.app_name)];
                for (i, m) in self.menus.iter().enumerate() {
                    let title = if m.icon { "System".to_string() } else { m.title.clone() };
                    rows.push(Row::item(&title, Act::MenuOpen(i)).detail("›"));
                }
                rows
            }
            Some((i, m)) => {
                let mut rows = vec![Row::item("‹ Menus", Act::MenuBack), Row::header(if m.icon { "System" } else { &m.title })];
                for (j, it) in m.items.iter().enumerate() {
                    if it.separator {
                        rows.push(Row::sep());
                        continue;
                    }
                    let label = if self.confirm == Some(it.action) { format!("{}?", it.label.trim_end_matches('…')) } else { it.label.clone() };
                    rows.push(Row::item(&label, Act::MenuRun(i, j)).detail(it.shortcut).enabled(self.runner.available(it.action)));
                }
                rows
            }
        }
    }

    /// Runs the popup actions added with the registry. True when handled.
    pub(super) fn run_applet_act(&mut self, act: &Act) -> bool {
        match act {
            Act::MenuOpen(i) => self.app_menu_page = Some(*i),
            Act::MenuBack => self.app_menu_page = None,
            Act::MenuRun(i, j) => {
                let page = self.app_menu_page;
                if self.run_menu_item(*i, *j) && self.confirm.is_some() {
                    // the destructive item asks again; its popup stays open on the same page
                    self.app_menu_page = page;
                }
            }
            Act::MediaPrev | Act::MediaToggle | Act::MediaNext => {
                if let (Some(worker), Some(player)) = (&self.dux_mpris, self.dux_player()) {
                    use crate::dash::mpris::Cmd;
                    worker.send(match act {
                        Act::MediaPrev => Cmd::Previous(player.bus),
                        Act::MediaNext => Cmd::Next(player.bus),
                        _ => Cmd::PlayPause(player.bus),
                    });
                } else {
                    sys::media_command(match act {
                        Act::MediaPrev => "previous",
                        Act::MediaNext => "next",
                        _ => "play-pause",
                    });
                }
            }
            Act::MediaOpen => {
                self.close();
                vbar::launch_ui(vec![crate::launch::sibling_or_path("decdash"), "media".into()]);
            }
            Act::TaskFocus(id) => {
                self.close();
                self.active.activate_window(*id);
            }
            Act::TaskMinimize(id) => {
                self.close();
                self.active.minimize_window(*id);
            }
            Act::TaskClose(id) => {
                self.close();
                self.active.close_window(*id);
            }
            Act::OpenApplet(kind) => {
                let kind = *kind;
                let rect = self.item_rect(Tray::More).unwrap_or((0.0, 0.0, 0.0, 0.0));
                self.close();
                self.more_target = Some(kind);
                self.applet_press(kind, rect, Button::Left);
            }
            _ => return false,
        }
        true
    }

    /// Where an item is drawn on the surface, whatever the renderer (a hidden item: the overflow cell).
    pub(super) fn item_rect(&self, kind: Tray) -> Option<(f32, f32, f32, f32)> {
        if self.pill_bar() {
            let lay = self.v_layout();
            return lay.item(kind).or_else(|| lay.item(Tray::More)).map(|r| (r.x, r.y, r.w, r.h));
        }
        let top = self.strip_top();
        let h = self.bar_bottom();
        if self.dux() {
            let lay = self.dux_layout();
            let find = |k: Tray| lay.items.iter().find(|i| i.kind == segmented::Kind::Tray(k)).map(|i| (i.x, i.y + top, i.w, i.h));
            return find(kind).or_else(|| find(Tray::More));
        }
        self.tray().into_iter().find(|(k, _)| *k == kind).or_else(|| self.tray().into_iter().find(|(k, _)| *k == Tray::More)).map(|(_, s)| (s.x, top, s.w, h))
    }

    /// The background work the enabled applets need runs; work nobody needs any more stops. Shared sources stay while
    /// any consumer remains (the volume subscription also serves the Control Center's sound tile). Cheap when nothing
    /// changes; called from the watchers' start and every tick.
    pub(super) fn sync_workers(&mut self) {
        let Some(waker) = self.waker.clone() else { return };
        let p = self.panel();
        let cc = crate::theme::th().control_center.enabled && p.shows(A::ControlCenter);
        // volume changes made elsewhere (keys, other apps) show up without polling
        let want_volume = (p.shows(A::Volume) || cc) && !cfg!(test);
        if !want_volume {
            self.volume_watch = None;
        } else if self.volume_watch.is_none() {
            let (shared, w) = (Arc::clone(&self.shared), waker.clone());
            self.volume_watch = Some(sys::watch_volume_until(move || {
                let recently_set = shared.vol_set_at.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < Duration::from_millis(500));
                if recently_set {
                    return;
                }
                if let Some((v, name)) = sys::probe_volume() {
                    if let Ok(mut s) = shared.snap.lock() {
                        s.volume = Some(v);
                        s.sink = name;
                    }
                    shared.changed.store(true, Ordering::SeqCst);
                    w.wake();
                }
            }));
        }
        // rdwm's layout and tags: the layout applet and the rail's workspace pills
        let want_layout = (p.shows(A::Layout) || p.shows(A::Workspaces)) && !cfg!(test);
        if !want_layout {
            if let Some(stop) = self.layout_stop.take() {
                stop.store(true, Ordering::SeqCst);
                if let Ok(mut l) = self.shared.layout.lock() {
                    *l = None;
                }
            }
        } else if self.layout_stop.is_none() {
            let stop = Arc::new(AtomicBool::new(false));
            let (w, sh) = (waker, Arc::clone(&self.shared));
            rdwm::spawn_watcher_until(
                Arc::clone(&self.shared.layout),
                move || {
                    sh.changed.store(true, Ordering::SeqCst);
                    w.wake();
                },
                Arc::clone(&stop),
                true,
            );
            self.layout_stop = Some(stop);
        }
        self.ensure_lock_watcher();
        self.ensure_indicator_host();
    }

    /// What the minute refresh must probe: nothing when no applet (and no Control Center button) shows system state.
    pub(super) fn snapshot_wanted(&self) -> bool {
        let p = self.panel();
        [A::Volume, A::Network, A::Bluetooth, A::Battery, A::Brightness, A::StayAwake, A::ControlCenter, A::AppMenu].iter().any(|id| p.shows(*id))
    }
}

/// The SF-style symbol of an item on the classic bar, the rail and the overflow list.
pub(super) fn classic_symbol(kind: Tray) -> &'static str {
    match kind {
        Tray::Logo => "grid",
        Tray::AppMenu => "menu",
        Tray::Media => "play",
        Tray::Notifications => "bell",
        Tray::Clipboard => "clipboard",
        Tray::Cpu | Tray::Memory | Tray::Dash => "chart",
        Tray::Brightness => "sun-small",
        Tray::ShowDesktop => "desktop",
        Tray::Power => "power",
        Tray::Lock => "lock",
        Tray::Awake => "cup",
        Tray::Agents => "agents",
        Tray::Spotlight => "magnifyingglass",
        Tray::Control => "switches",
        Tray::Audio => "speaker-2",
        Tray::Network => "wifi-3",
        Tray::Bluetooth => "bluetooth",
        Tray::Battery => "power",
        Tray::Weather => "weather-cloud",
        Tray::Layout => "grid",
        Tray::Workspaces => "apps",
        Tray::Window => "apps",
        Tray::Clock => "refresh",
        Tray::Task(_) | Tray::Indicator(_) | Tray::Indicators | Tray::More => "apps",
    }
}

/// The Material symbol of an item on the Dux bar.
pub(super) fn material_symbol(kind: Tray) -> &'static str {
    match kind {
        Tray::Logo => "apps",
        Tray::AppMenu => "menu",
        Tray::Media => "play",
        Tray::Notifications => "bell",
        Tray::Clipboard => "clipboard",
        Tray::Cpu => "memory",
        Tray::Memory => "developer_board",
        Tray::Dash => "dashboard",
        Tray::Brightness => "sun",
        Tray::ShowDesktop => "desktop",
        Tray::Power => "power",
        Tray::Lock => "lock",
        Tray::Awake => "cup",
        Tray::Agents => "memory",
        Tray::Spotlight => "magnifyingglass",
        Tray::Control => "switches",
        Tray::Audio => "speaker-3",
        Tray::Network => "wifi-3",
        Tray::Bluetooth => "bluetooth",
        Tray::Battery => "battery",
        Tray::Weather => "weather-cloud",
        Tray::Layout => "layout",
        _ => "apps",
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::panel::AppletSet;

    pub const STYLES: [Renderer; 4] = [Renderer::MenuBar, Renderer::Floating, Renderer::Pill, Renderer::Dux];

    struct FakeWm;
    impl active::ActiveWindow for FakeWm {
        fn active_app_name(&self) -> Option<String> {
            Some("foot".into())
        }
        fn active_title(&self) -> Option<String> {
            Some("~/src".into())
        }
        fn workspaces(&self) -> Vec<Workspace> {
            (0..5).map(|i| Workspace { name: (i + 1).to_string(), active: i == 0, occupied: i < 3, urgent: false }).collect()
        }
        fn windows(&self) -> Vec<crate::wm::Window> {
            vec![
                crate::wm::Window { id: 11, app_id: "foot".into(), title: "~/src".into(), active: true, desktop: Some(0) },
                crate::wm::Window { id: 12, app_id: "firefox".into(), title: "Docs".into(), active: false, desktop: Some(0) },
            ]
        }
    }

    fn indicator(key: u64) -> crate::sni::Item {
        let mut pm = Pixmap::new(8, 8).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(240, 60, 90, 255));
        crate::sni::Item {
            key,
            title: format!("App {key}"),
            tooltip: String::new(),
            status: crate::sni::Status::Active,
            icon_name: String::new(),
            icon: Some(std::sync::Arc::new(pm)),
            item_is_menu: false,
            menu: None,
            menu_loading: false,
            menu_error: None,
        }
    }

    /// Resets every renderer switch of this thread.
    pub fn reset() {
        TEST_VERTICAL.with(|f| f.set(false));
        TEST_FLOATING.with(|f| f.set(false));
        TEST_DUX.with(|f| f.set(false));
        TEST_APPLETS.with(|t| t.set(None));
        TEST_EXPLICIT.with(|t| t.set(AppletSet::EMPTY));
        TEST_EDGE.with(|t| t.set(None));
        TEST_PLAYER.with(|p| *p.borrow_mut() = None);
    }

    /// A bar of `style` on `edge` showing `applets`, with every data source an applet can need filled in, on a surface
    /// of `len` pixels along the edge.
    pub fn fixture(style: Renderer, edge: Edge, applets: AppletSet, len: u32) -> DecBar {
        reset();
        match style {
            Renderer::Pill => {
                TEST_VERTICAL.with(|f| f.set(true));
                TEST_POSITION.with(|p| p.set(edge.position()));
            }
            Renderer::Floating => TEST_FLOATING.with(|f| f.set(true)),
            Renderer::Dux => TEST_DUX.with(|f| f.set(true)),
            _ => {}
        }
        TEST_EDGE.with(|t| t.set(Some(edge)));
        TEST_APPLETS.with(|t| t.set(Some(applets)));
        TEST_EXPLICIT.with(|t| t.set(applets));
        let mut player = crate::dash::mpris::Player::new("org.mpris.MediaPlayer2.fixture", Instant::now());
        player.identity = "Fixture".into();
        player.status = crate::dash::mpris::Status::Playing;
        player.meta.title = "Track".into();
        player.meta.artist = "Artist".into();
        (player.can_control, player.can_next, player.can_prev) = (true, true, true);
        TEST_PLAYER.with(|p| *p.borrow_mut() = Some(player));
        let mut bar = DecBar::new(Box::new(FakeWm)).with_workspaces(true);
        bar.update_snapshot(|s| {
            *s = Snapshot {
                net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: String::new() }),
                bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
                volume: Some(sys::Volume { level: 0.5, muted: false }),
                battery: Some(sys::Battery { percent: 80, charging: false, status: "Discharging".into() }),
                brightness: Some(60),
                awake: false,
                ..Snapshot::default()
            }
        });
        *bar.shared.weather.lock().unwrap() = Some(sample_weather());
        *bar.shared.layout.lock().unwrap() = Some(rdwm::LayoutState { name: "tile".into(), symbol: "[]=".into(), tags: Vec::new() });
        *bar.dux_perf.lock().unwrap() = Some((Some(0.25), Some(0.5)));
        bar.indicators.items = vec![indicator(1)];
        bar.clock_text = "Fri 30 Oct  14:07".into();
        bar.last_minute = Some((14, 7));
        bar.v_refresh_window();
        bar.refresh_tasks();
        let (w, h) = match bar.place() {
            Some(p) if p.edge.is_side() => (p.surface_size().0, len),
            Some(p) => (len, p.surface_size().1),
            None => (len, bar.size().1),
        };
        (bar.width, bar.height) = (w, h);
        bar
    }

    /// Every drawn item with its surface rectangle.
    pub fn items(bar: &DecBar) -> Vec<(Tray, (f32, f32, f32, f32))> {
        if bar.pill_bar() {
            return bar.v_layout().items.into_iter().map(|(k, r)| (k, (r.x, r.y, r.w, r.h))).collect();
        }
        let top = bar.strip_top();
        if bar.dux() {
            return bar
                .dux_layout()
                .items
                .into_iter()
                .filter_map(|i| {
                    let k = match i.kind {
                        segmented::Kind::Tray(t) => t,
                        segmented::Kind::Media => Tray::Media,
                        segmented::Kind::Notifications => Tray::Notifications,
                        segmented::Kind::Clipboard => Tray::Clipboard,
                        segmented::Kind::Cpu => Tray::Cpu,
                        segmented::Kind::Memory => Tray::Memory,
                        _ => return None,
                    };
                    Some((k, (i.x, i.y + top, i.w, i.h)))
                })
                .collect();
        }
        let mut out: Vec<_> = bar.tray().into_iter().map(|(k, s)| (k, (s.x, top, s.w, bar.bar_bottom()))).collect();
        if bar.shows(A::AppMenu) {
            out.extend(bar.spans().into_iter().filter(|s| s.x + s.w <= bar.right_start()).map(|s| (Tray::AppMenu, (s.x, top, s.w, bar.bar_bottom()))));
        }
        out
    }

    /// The strip of the bar on its surface.
    fn strip(bar: &DecBar) -> (f32, f32, f32, f32) {
        if bar.pill_bar() {
            let s = bar.v_geo().strip();
            return (s.x, s.y, s.w, s.h);
        }
        (0.0, bar.strip_top(), bar.width as f32, bar.bar_bottom())
    }

    fn inside(r: (f32, f32, f32, f32), s: (f32, f32, f32, f32)) -> bool {
        r.0 >= s.0 - 0.5 && r.1 >= s.1 - 0.5 && r.0 + r.2 <= s.0 + s.2 + 0.5 && r.1 + r.3 <= s.1 + s.3 + 0.5
    }

    fn overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
        a.0 + 0.5 < b.0 + b.2 && b.0 + 0.5 < a.0 + a.2 && a.1 + 0.5 < b.1 + b.3 && b.1 + 0.5 < a.1 + a.3
    }

    fn shown_applets(bar: &DecBar) -> AppletSet {
        let mut set = AppletSet::EMPTY;
        for (k, _) in items(bar) {
            if let Some(id) = applet_of(k) {
                set.set(id, true);
            }
        }
        for k in bar.hidden_items() {
            if let Some(id) = applet_of(k) {
                set.set(id, true);
            }
        }
        set
    }

    #[test]
    fn every_style_shows_every_applet_alone_on_every_edge() {
        for style in STYLES {
            for edge in Edge::ALL {
                for id in A::all() {
                    let bar = fixture(style, edge, AppletSet::of(&[id]), 1280);
                    let got = shown_applets(&bar);
                    assert_eq!(got, AppletSet::of(&[id]), "{} on {} with only {}", style.key(), edge.as_str(), id.key());
                    // alone it always fits: it is a target of its own, not in the overflow list
                    assert!(items(&bar).iter().any(|(k, _)| applet_of(*k) == Some(id)), "{} on {}: {} drawn", style.key(), edge.as_str(), id.key());
                }
            }
        }
        reset();
    }

    #[test]
    fn all_on_never_overlaps_and_keeps_every_applet_reachable() {
        for style in STYLES {
            for edge in Edge::ALL {
                for len in [1920, 1024, 600] {
                    let bar = fixture(style, edge, AppletSet::all(), len);
                    let lay = items(&bar);
                    let s = strip(&bar);
                    for (i, (k, r)) in lay.iter().enumerate() {
                        assert!(inside(*r, s), "{} {} {len}: {k:?} {r:?} outside the strip {s:?}", style.key(), edge.as_str());
                        for (k2, r2) in &lay[i + 1..] {
                            if *k == Tray::AppMenu && *k2 == Tray::AppMenu {
                                continue;
                            }
                            assert!(!overlap(*r, *r2), "{} {} {len}: {k:?} {r:?} overlaps {k2:?} {r2:?}", style.key(), edge.as_str());
                        }
                    }
                    // what does not fit is in the overflow list, never lost
                    assert_eq!(shown_applets(&bar), AppletSet::all(), "{} {} {len}", style.key(), edge.as_str());
                    if !bar.hidden_items().is_empty() {
                        assert!(lay.iter().any(|(k, _)| *k == Tray::More), "{} {} {len}: overflow cell", style.key(), edge.as_str());
                    }
                }
            }
        }
        reset();
    }

    #[test]
    fn all_off_leaves_an_empty_strip() {
        for style in STYLES {
            for edge in Edge::ALL {
                let bar = fixture(style, edge, AppletSet::EMPTY, 1280);
                assert!(items(&bar).is_empty(), "{} {}: {:?}", style.key(), edge.as_str(), items(&bar));
            }
        }
        reset();
    }

    #[test]
    fn side_edges_are_rails_and_top_bottom_keep_each_styles_own_strip() {
        for style in STYLES {
            for edge in Edge::ALL {
                let bar = fixture(style, edge, Renderer::MenuBar.defaults(), 1280);
                let rail = bar.pill_bar();
                assert_eq!(rail, style == Renderer::Pill || edge.is_vertical(), "{} {}", style.key(), edge.as_str());
                let spec = bar.surface_spec().unwrap();
                assert_eq!(spec.anchor, edge.anchor(), "{} {}", style.key(), edge.as_str());
                assert!(spec.reserve > 0);
                let s = strip(&bar);
                let touches = match edge {
                    Edge::Top => s.1 == 0.0,
                    Edge::Bottom => (s.1 + s.3 - bar.height as f32).abs() < 1.0,
                    Edge::Left => s.0 == 0.0,
                    Edge::Right => (s.0 + s.2 - bar.width as f32).abs() < 1.0,
                };
                assert!(touches, "{} {}: strip {s:?} on {}x{}", style.key(), edge.as_str(), bar.width, bar.height);
            }
        }
        reset();
    }

    /// Opens what an item opens and checks the popup is on the surface, on the screen side, clear of the strip.
    #[test]
    fn popups_open_beside_their_applet_towards_the_screen() {
        for style in STYLES {
            for edge in Edge::ALL {
                let mut bar = fixture(style, edge, AppletSet::all(), 1600);
                let s = strip(&bar);
                for kind in [Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery, Tray::Weather, Tray::Layout, Tray::Media, Tray::Brightness, Tray::AppMenu, Tray::Window] {
                    bar.close();
                    bar.open_popup(kind);
                    // the backend gives the surface the size it asks for
                    let (sw, sh) = bar.size();
                    let (w, h) = (if sw == 0 { bar.width } else { sw }, if sh == 0 { bar.height } else { sh });
                    bar.resized(w, h);
                    let s = strip(&bar);
                    let Some(p) = bar.popup.clone() else {
                        // the Dux bar has no window popup (its item opens the dashboard), the rail no application menu list
                        continue;
                    };
                    assert_eq!(bar.open, Open::Popup(kind));
                    let (w, h) = (bar.width as f32, bar.height as f32);
                    assert!(p.x >= 0.0 && p.y >= 0.0 && p.x + p.w <= w + 0.5 && p.y + p.h <= h + 0.5, "{} {} {kind:?}: {:?} on {w}x{h}", style.key(), edge.as_str(), (p.x, p.y, p.w, p.h));
                    assert!(!overlap((p.x, p.y, p.w, p.h), s), "{} {} {kind:?}: popup covers the strip", style.key(), edge.as_str());
                    // the input region reaches from the strip to the popup
                    let rects = bar.input_rects();
                    let covered = |x: f32, y: f32| rects.iter().any(|r| x >= r.x as f32 && x < (r.x + r.w) as f32 && y >= r.y as f32 && y < (r.y + r.h) as f32);
                    assert!(covered(p.x + p.w / 2.0, p.y + p.h / 2.0), "{} {} {kind:?}: popup not in the input region", style.key(), edge.as_str());
                }
                // the Control Center hangs from its button on the screen side too
                bar.close();
                bar.open_control();
                let (ox, oy) = bar.panel_origin();
                let cw = bar.cc_width();
                let ch = bar.cc_visible_height();
                assert!(!overlap((ox, oy, cw, ch), s), "{} {}: control center covers the strip", style.key(), edge.as_str());
            }
        }
        reset();
    }

    /// Pressing each applet does its job: popups open, launchers start their program at the applet.
    #[test]
    fn pointer_presses_reach_every_applet() {
        for style in STYLES {
            for edge in Edge::ALL {
                for id in A::all() {
                    let mut bar = fixture(style, edge, AppletSet::of(&[id]), 1600);
                    let Some((kind, r)) = items(&bar).into_iter().find(|(k, _)| applet_of(*k) == Some(id) && !matches!(k, Tray::Workspaces)) else { continue };
                    vbar::LAUNCHED.with(|l| l.borrow_mut().clear());
                    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
                    bar.pointer_button(cx, cy, Button::Left);
                    let launched = vbar::LAUNCHED.with(|l| l.borrow().clone());
                    let did = bar.open != Open::None || !launched.is_empty();
                    // state toggles, task focus and show desktop act on the session, not on the bar
                    let silent = matches!(kind, Tray::Lock | Tray::Awake | Tray::Task(_) | Tray::ShowDesktop | Tray::Indicator(_));
                    assert!(did || silent, "{} {} {}: press on {kind:?} did nothing", style.key(), edge.as_str(), id.key());
                    if id == A::Launcher && style != Renderer::Pill {
                        let argv = launched.iter().find(|a| a.first().is_some_and(|p| p.ends_with("declaunch"))).expect("declaunch");
                        let anchor = crate::panel::frame::LaunchAnchor::parse(argv).expect("anchor flags");
                        assert_eq!(anchor.edge, edge);
                    }
                }
            }
        }
        reset();
    }

    #[test]
    fn every_frame_draws_without_panicking_in_both_modes() {
        for style in STYLES {
            for edge in Edge::ALL {
                for set in [AppletSet::all(), AppletSet::EMPTY, style.defaults()] {
                    let mut bar = fixture(style, edge, set, 1280);
                    let mut pm = Pixmap::new(bar.width, bar.height.max(1)).unwrap();
                    bar.draw(&mut pm);
                    bar.open_popup(Tray::Audio);
                    bar.draw(&mut pm);
                }
            }
        }
        reset();
    }

    #[test]
    fn explicit_state_applets_stay_as_controls() {
        let bar = fixture(Renderer::MenuBar, Edge::Top, AppletSet::of(&[A::StayAwake, A::LockKeys]), 1280);
        let kinds: Vec<_> = items(&bar).into_iter().map(|(k, _)| k).collect();
        assert!(kinds.contains(&Tray::Awake) && kinds.contains(&Tray::Lock), "{kinds:?}");
        // without the explicit choice they appear only while on, as before
        TEST_EXPLICIT.with(|t| t.set(AppletSet::EMPTY));
        let kinds: Vec<_> = items(&bar).into_iter().map(|(k, _)| k).collect();
        assert!(!kinds.contains(&Tray::Awake) && !kinds.contains(&Tray::Lock), "{kinds:?}");
        reset();
    }

    #[test]
    fn application_menu_folds_into_one_button_and_walks_into_each_menu() {
        let mut bar = fixture(Renderer::MenuBar, Edge::Left, AppletSet::of(&[A::AppMenu]), 900);
        bar.toggle_popup(Tray::AppMenu);
        let rows = bar.popup.clone().unwrap().rows;
        let open = rows.iter().find(|r| matches!(r.act, popup::Act::MenuOpen(2))).expect("third menu").act.clone();
        bar.run_act(open);
        let rows = bar.popup.clone().unwrap().rows;
        assert!(rows.iter().any(|r| r.act == popup::Act::MenuBack));
        assert!(rows.iter().any(|r| matches!(r.act, popup::Act::MenuRun(2, _))));
        reset();
    }

    #[test]
    fn task_buttons_group_windows_by_application() {
        let tasks = group_tasks(&FakeWm.windows());
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].active() && !tasks[1].active());
    }
}
