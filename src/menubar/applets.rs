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

/// The taskbar's tray, left to right (Start, search, the task buttons, the clock and show desktop are its own zones).
/// The items it always had keep their order.
pub(super) const TASKBAR_TRAY: &[A] = &[
    A::AppMenu,
    A::WindowTitle,
    A::Workspaces,
    A::Layout,
    A::Media,
    A::Weather,
    A::Cpu,
    A::Memory,
    A::StayAwake,
    A::Agents,
    A::LockKeys,
    A::Clipboard,
    A::Notifications,
    A::Dashboard,
    A::Bluetooth,
    A::Network,
    A::Volume,
    A::Brightness,
    A::Battery,
    A::Tray,
    A::ControlCenter,
    A::Power,
];

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
        #[cfg(not(test))]
        {
            let generation = crate::theme::generation();
            if let Some((g, p)) = self.panel_cache.get().filter(|(g, _)| *g == generation) {
                let _ = g;
                return p;
            }
            let p = self.resolve_panel();
            self.panel_cache.set(Some((generation, p)));
            p
        }
        #[cfg(test)]
        self.resolve_panel()
    }

    fn resolve_panel(&self) -> Effective {
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
        // the contract is in surface (root) pixels: the logical rectangle times the render scale
        let k = crate::gfx::render_scale();
        let thickness = crate::theme::th().bar_thickness(edge) * k;
        crate::panel::frame::LaunchAnchor { edge, x: self.indicator_origin.0 as f32 + (r.0 + r.2 / 2.0) * k, y: self.indicator_origin.1 as f32 + (r.1 + r.3 / 2.0) * k, thickness }
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
            Tray::Workspaces => {
                let ws = self.bar_workspaces();
                if ws.is_empty() {
                    return None;
                }
                let mut rows = vec![Row::header("Workspaces")];
                for (i, w) in ws.iter().enumerate() {
                    let name = if w.name.is_empty() { (i + 1).to_string() } else { w.name.clone() };
                    rows.push(Row::item(&format!("Workspace {name}"), Act::Workspace(i)).checked(w.active).detail(if w.urgent {
                        "!"
                    } else if w.occupied {
                        "•"
                    } else {
                        ""
                    }));
                }
                Some((rows, 220.0, true))
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
                    rows.push(Row::item(&label, Act::MenuRun(i, j)).detail(&self.menu_item_shortcut(i, j)).enabled(self.menu_item_available(i, j)));
                }
                rows
            }
        }
    }

    /// Runs the popup actions added with the registry. True when handled.
    pub(super) fn run_applet_act(&mut self, act: &Act) -> bool {
        match act {
            Act::Workspace(i) => {
                self.close();
                self.v_switch_workspace(*i);
            }
            Act::MenuOpen(i) => {
                self.menu_opened(*i);
                self.app_menu_page = Some(*i);
            }
            Act::MenuBack => {
                self.menu_closed();
                self.app_menu_page = None;
            }
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
        // the focused window's exported menus: only while the app_menu applet is shown
        if !cfg!(test) {
            let w = waker.clone();
            self.app_menu.set_enabled(p.shows(A::AppMenu), crate::appmenu::Config::session, || exported::notifier(Some(w)));
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

    /// What the minute refresh probes: each kind of state only while an applet shows it. The Control Center takes a
    /// full snapshot when it opens and while it is open, so its button alone needs nothing between; the application
    /// menu's Apple menu names the current appearance. Hosts (the taskbar) keep their sound / network / battery icons.
    pub(super) fn probes(&self) -> Probes {
        let p = self.panel();
        Probes {
            net: p.shows(A::Network),
            volume: p.shows(A::Volume),
            battery: p.shows(A::Battery),
            bluetooth: p.shows(A::Bluetooth),
            brightness: p.shows(A::Brightness),
            awake: p.shows(A::StayAwake),
            light: p.shows(A::AppMenu),
        }
    }
}

impl DecBar {
    /// Fixed sample data for static previews (labelled as such where shown): what a typical laptop reports. No worker
    /// starts and nothing is probed.
    pub(super) fn fill_sample(&mut self) {
        self.update_snapshot(|s| {
            *s = Snapshot {
                net: Some(sys::Net { wifi_enabled: true, wifi_name: "Network".into(), wired_name: String::new() }),
                bluetooth: Some(sys::Bluetooth { powered: true, connected: 0 }),
                volume: Some(sys::Volume { level: 0.6, muted: false }),
                battery: Some(sys::Battery { percent: 76, charging: false, status: "Discharging".into() }),
                brightness: Some(70),
                ..Snapshot::default()
            }
        });
        if let Ok(mut w) = self.shared.weather.lock() {
            *w = Some(sample_weather());
        }
        if let Ok(mut p) = self.dux_perf.lock() {
            *p = Some((Some(0.18), Some(0.42)));
        }
        // previews have no window manager to ask: four sample workspaces, the first current
        self.workspaces = (0..4).map(|i| Workspace { name: (i + 1).to_string(), active: i == 0, occupied: i < 2, urgent: false }).collect();
        self.show_workspaces = true;
        self.v_win = Some(("org.gnome.Nautilus".into(), "Files".into()));
        self.v_win_len = self.fonts.as_ref().map_or(40.0, |f| f.measure("Files", TEXT_PX, Weight::Medium));
        self.tasks = group_tasks(&[
            crate::wm::Window { id: 1, app_id: "org.gnome.Nautilus".into(), title: "Files".into(), active: true, desktop: Some(0) },
            crate::wm::Window { id: 2, app_id: "firefox".into(), title: "Web".into(), active: false, desktop: Some(0) },
        ]);
    }
}

/// A process's keyboard-mode request (`decbar --focus`, `decdock --focus`: SIGUSR2) and the waker of its event loop.
pub type FocusSignal = (Arc<AtomicBool>, Arc<Mutex<Option<Waker>>>);

/// Something the keyboard can focus on a panel: an applet's item, or one of the classic bar's menu titles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Focus {
    Item(Tray),
    Menu(usize),
}

impl DecBar {
    /// What the keyboard walks through, in reading order, with each item's rectangle on the surface.
    pub(super) fn focus_items(&self) -> Vec<(Focus, (f32, f32, f32, f32))> {
        if self.pill_bar() {
            return self.v_layout().items.into_iter().map(|(k, r)| (Focus::Item(k), (r.x, r.y, r.w, r.h))).collect();
        }
        let top = self.strip_top();
        if self.dux() {
            return self
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
                        // the transport buttons are reached through the media popup
                        _ => return None,
                    };
                    Some((Focus::Item(k), (i.x, i.y + top, i.w, i.h)))
                })
                .collect();
        }
        let h = self.bar_bottom();
        let limit = self.right_start();
        let mut out: Vec<(Focus, (f32, f32, f32, f32))> = self.left_items().into_iter().map(|(k, s)| (Focus::Item(k), (s.x, top, s.w, h))).collect();
        out.extend(self.spans().into_iter().enumerate().filter(|(_, s)| s.x + s.w <= limit).map(|(i, s)| (Focus::Menu(i), (s.x, top, s.w, h))));
        out.extend(self.tray().into_iter().filter(|(k, _)| *k != Tray::Logo).map(|(k, s)| (Focus::Item(k), (s.x, top, s.w, h))));
        out
    }

    /// Keyboard mode starts when `decbar --focus` asked for it (the signal thread set the flag). True when it did.
    pub(super) fn take_focus_request(&mut self) -> bool {
        if !self.focus_request.swap(false, Ordering::SeqCst) {
            return false;
        }
        self.close();
        self.kb_focus = (!self.focus_items().is_empty()).then_some(0);
        true
    }

    /// Lets `decbar --focus` (SIGUSR2 to this bar) start keyboard mode. Call before any other thread starts: the signal
    /// is blocked here and collected by a thread of its own.
    pub fn with_focus_signal(mut self) -> Self {
        let (flag, waker) = Self::init_focus_signal();
        self.focus_request = Arc::clone(flag);
        self.focus_waker = Arc::clone(waker);
        self
    }

    /// Blocks SIGUSR2 and starts its collector, once per process (the bar is built again each time it comes back on
    /// screen). Call before other threads start.
    pub fn init_focus_signal() -> &'static FocusSignal {
        static SIGNAL: std::sync::OnceLock<FocusSignal> = std::sync::OnceLock::new();
        SIGNAL.get_or_init(|| {
            crate::single::block_signals(&[libc::SIGUSR2]);
            let shared: FocusSignal = (Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
            let (flag, waker) = (Arc::clone(&shared.0), Arc::clone(&shared.1));
            let _ = std::thread::Builder::new().name("bar-focus".into()).spawn(move || {
                while crate::single::wait_signal(&[libc::SIGUSR2]) == Some(libc::SIGUSR2) {
                    flag.store(true, Ordering::SeqCst);
                    if let Some(w) = waker.lock().ok().and_then(|w| w.clone()) {
                        w.wake();
                    }
                }
            });
            shared
        })
    }

    /// Starts keyboard mode directly (tests).
    #[cfg(test)]
    pub(super) fn focus_panel(&mut self) {
        self.focus_request.store(true, Ordering::SeqCst);
        self.take_focus_request();
    }

    /// Enter on a focused item does what a click on it does, through the same typed actions.
    fn activate_focus(&mut self, f: Focus, r: (f32, f32, f32, f32)) -> bool {
        let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
        match f {
            Focus::Menu(i) => {
                self.close();
                self.open_menu(i);
                self.hover_row = self.menus.get(i).and_then(|m| (0..m.items.len()).find(|&j| self.menu_item_available(i, j)));
                true
            }
            Focus::Item(Tray::Workspaces) => {
                self.toggle_popup(Tray::Workspaces);
                true
            }
            Focus::Item(Tray::Indicator(key)) => match self.indicator_press(key, Button::Left, cx.round() as i32, cy.round() as i32) {
                indicators::IndicatorPress::Menu => {
                    self.open_popup(Tray::Indicator(key));
                    true
                }
                indicators::IndicatorPress::Handled => true,
                indicators::IndicatorPress::Ignored => false,
            },
            Focus::Item(kind) if self.pill_bar() => self.v_press(kind, None, cx, cy),
            Focus::Item(kind @ (Tray::Clock | Tray::Window)) if self.dux() => {
                // the Dux clock and window open the dashboard, as their clicks do
                self.close();
                let _ = kind;
                self.dashboard(Span { x: r.0, w: r.2 });
                true
            }
            Focus::Item(kind) => self.applet_press(kind, r, Button::Left),
        }
    }

    /// The panel's keyboard: arrows between applets, Enter to activate; inside a popup or menu the arrows move between
    /// its rows (Left goes back in the application menu, Left/Right step a slider or the calendar). `None`: not the
    /// panel's key (the Control Center routes its own).
    pub(super) fn panel_nav(&mut self, key: crate::app::NavKey) -> Option<bool> {
        use crate::app::NavKey as K;
        match self.open {
            Open::Control => None,
            Open::Popup(kind) => {
                if let Some(c) = self.calendar.as_mut() {
                    return Some(match key {
                        K::Left | K::Up => {
                            c.shift(-1);
                            true
                        }
                        K::Right | K::Down => {
                            c.shift(1);
                            true
                        }
                        K::Enter => false,
                    });
                }
                let p = self.popup.as_ref()?;
                let rows: Vec<usize> = (0..p.rows.len()).filter(|i| p.rows[*i].interactive()).collect();
                if rows.is_empty() {
                    return Some(false);
                }
                let at = self.popup_hover.and_then(|h| rows.iter().position(|r| *r == h));
                let row = self.popup_hover.and_then(|h| p.rows.get(h)).cloned();
                match key {
                    K::Up | K::Down => {
                        let step: i32 = if key == K::Up { -1 } else { 1 };
                        let next = at.map_or(if step > 0 { 0 } else { rows.len() - 1 }, |i| (i as i32 + step).rem_euclid(rows.len() as i32) as usize);
                        self.popup_hover = Some(rows[next]);
                        Some(true)
                    }
                    K::Left | K::Right => {
                        let up = if key == K::Right { -1.0 } else { 1.0 };
                        match row.map(|r| r.act) {
                            Some(Act::VolumeSlider) => Some(self.applet_scroll(Tray::Audio, up)),
                            Some(Act::BrightnessSlider) => Some(self.applet_scroll(Tray::Brightness, up)),
                            _ if kind == Tray::AppMenu && key == K::Left && self.app_menu_page.is_some() => {
                                self.run_act(Act::MenuBack);
                                Some(true)
                            }
                            _ => Some(false),
                        }
                    }
                    K::Enter => {
                        let Some(row) = row else { return Some(false) };
                        if matches!(row.kind, popup::Kind::Slider(_)) {
                            return Some(false);
                        }
                        self.run_act(row.act);
                        // a new page (the application menu's): start on its first row
                        if let Some(p) = self.popup.as_ref() {
                            if self.popup_hover.is_none_or(|h| h >= p.rows.len() || !p.rows[h].interactive()) {
                                self.popup_hover = (0..p.rows.len()).find(|i| p.rows[*i].interactive());
                            }
                        }
                        Some(true)
                    }
                }
            }
            Open::Menu(i) => {
                let Some(menu) = self.menus.get(i) else { return Some(false) };
                let rows: Vec<usize> = (0..menu.items.len()).filter(|j| !menu.items[*j].separator).collect();
                match key {
                    K::Up | K::Down => {
                        let at = self.hover_row.and_then(|h| rows.iter().position(|r| *r == h));
                        let step: i32 = if key == K::Up { -1 } else { 1 };
                        let next = at.map_or(0, |a| (a as i32 + step).rem_euclid(rows.len().max(1) as i32) as usize);
                        self.hover_row = rows.get(next).copied();
                        Some(true)
                    }
                    K::Left | K::Right => {
                        let n = self.menus.len() as i32;
                        let next = (i as i32 + if key == K::Left { -1 } else { 1 }).rem_euclid(n.max(1)) as usize;
                        self.open_menu(next);
                        self.hover_row = self.menus.get(next).and_then(|m| m.items.iter().position(|it| !it.separator));
                        Some(true)
                    }
                    K::Enter => Some(self.hover_row.is_some_and(|row| self.run_menu_item(i, row))),
                }
            }
            Open::None => {
                let i = self.kb_focus?;
                let items = self.focus_items();
                if items.is_empty() {
                    self.kb_focus = None;
                    return Some(false);
                }
                let i = i.min(items.len() - 1);
                match key {
                    K::Left | K::Up => self.kb_focus = Some((i + items.len() - 1) % items.len()),
                    K::Right | K::Down => self.kb_focus = Some((i + 1) % items.len()),
                    K::Enter => {
                        let (f, r) = items[i];
                        let done = self.activate_focus(f, r);
                        if let Some(p) = self.popup.as_ref() {
                            self.popup_hover = (0..p.rows.len()).find(|j| p.rows[*j].interactive());
                        }
                        return Some(done);
                    }
                }
                Some(true)
            }
        }
    }

    /// The focus ring around the focused item (the keyboard's place on the panel).
    pub(super) fn draw_focus_ring(&mut self, pm: &mut Pixmap) {
        let Some(i) = self.kb_focus else { return };
        let Some((_, r)) = self.focus_items().get(i).copied() else { return };
        let th = crate::theme::th();
        stroke_rounded(pm, r.0 + 1.0, r.1 + 1.0, (r.2 - 2.0).max(1.0), (r.3 - 2.0).max(1.0), 6.0_f32.min(r.2.min(r.3) / 2.0), th.accent.color(1.0), 2.0);
    }
}

/// What a rendered logo depends on.
pub(super) type LogoKey = (crate::panel::branding::Logo, u32, (u8, u8, u8), bool);

impl DecBar {
    /// Draws the theme's `[branding] logo` centred on (`cx`, `cy`); false when the theme has none (the style draws its
    /// own) or it cannot be loaded.
    pub(super) fn draw_brand(&mut self, pm: &mut Pixmap, size: f32, cx: f32, cy: f32, ink: (u8, u8, u8)) -> bool {
        let b = branding();
        if matches!(b.logo, crate::panel::branding::Logo::Style | crate::panel::branding::Logo::None) {
            return false;
        }
        let px = (size * 2.0).ceil() as u32;
        let key: LogoKey = (b.logo.clone(), px, ink, b.tint);
        if self.brand_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            // an explicit logo that cannot be had (no such icon, a missing or corrupt image) is the neutral one
            let img = render_logo(&b.logo, px, ink, &mut self.symbols)
                .map(|mut img| {
                    if b.tint {
                        tint(&mut img, ink);
                    }
                    img
                })
                .or_else(|| render_logo(&crate::panel::branding::NEUTRAL, px, ink, &mut self.symbols));
            self.brand_cache = Some((key, img));
        }
        let Some((_, Some(img))) = &self.brand_cache else { return false };
        crate::gfx::blit_scaled(pm, img, (cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size, 1.0);
        true
    }
}

/// Renders a logo `px` square (`None` when it cannot be had: a missing icon, an unreadable image).
pub(super) fn render_logo(logo: &crate::panel::branding::Logo, px: u32, ink: (u8, u8, u8), symbols: &mut SymbolCache) -> Option<Pixmap> {
    use crate::panel::branding::Logo;
    match logo {
        Logo::Style | Logo::None => None,
        Logo::Builtin("windows") => {
            // four squares, as the taskbar's Start button
            let mut pm = Pixmap::new(px, px)?;
            let q = px as f32 * 0.44;
            let g = px as f32 * 0.08;
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                crate::gfx::fill_rect(&mut pm, px as f32 * 0.02 + dx * (q + g), px as f32 * 0.02 + dy * (q + g), q, q, color(ink.0, ink.1, ink.2, 1.0));
            }
            Some(pm)
        }
        Logo::Builtin("os") => symbols.get(&vbar::logo_symbol(), ink, px).cloned(),
        Logo::Builtin(name) => symbols.get(name, ink, px).cloned(),
        Logo::Icon(name) => crate::icons::load_named(name, px),
        Logo::File(path) => {
            let (w, h) = image::image_dimensions(path).ok()?;
            if w == 0 || h == 0 || w > 4096 || h > 4096 {
                return None;
            }
            Some(crate::dash::info::square_pixmap(&image::open(path).ok()?.to_rgba8(), px))
        }
    }
}

/// Repaints a premultiplied image in `ink`, keeping its coverage.
fn tint(img: &mut Pixmap, ink: (u8, u8, u8)) {
    for p in img.data_mut().chunks_exact_mut(4) {
        let a = p[3] as u16;
        p[0] = (ink.0 as u16 * a / 255) as u8;
        p[1] = (ink.1 as u16 * a / 255) as u8;
        p[2] = (ink.2 as u16 * a / 255) as u8;
    }
}

/// The theme's `[branding] logo` rendered `px` square in `ink` (tinted when the theme asks), for hosts that draw their
/// own launcher button (the taskbar's Start). `None`: the theme has no logo, the host draws its own.
pub fn theme_logo(px: u32, ink: (u8, u8, u8), symbols: &mut SymbolCache) -> Option<Pixmap> {
    let b = branding();
    if b.logo == crate::panel::branding::Logo::Style {
        return None;
    }
    // `none` and anything that cannot be had: the neutral glyph, never the host's trademark logo
    match render_logo(&b.logo, px, ink, symbols) {
        Some(mut img) => {
            if b.tint {
                tint(&mut img, ink);
            }
            Some(img)
        }
        None => render_logo(&crate::panel::branding::NEUTRAL, px, ink, symbols),
    }
}

#[cfg(test)]
thread_local! {
    /// Tests set a theme's branding without publishing a process-wide theme.
    pub(super) static TEST_BRANDING: std::cell::RefCell<Option<crate::panel::branding::Branding>> = const { std::cell::RefCell::new(None) };
}

/// The theme's `[branding]`.
pub(super) fn branding() -> crate::panel::branding::Branding {
    #[cfg(test)]
    if let Some(b) = TEST_BRANDING.with(|b| b.borrow().clone()) {
        return b;
    }
    crate::theme::th().branding.clone()
}

/// The SF-style symbol of an item on the classic bar, the rail and the overflow list.
pub fn classic_symbol(kind: Tray) -> &'static str {
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
            Some(p) if p.edge.is_side() => (bar.v_surface_size(p).0, len),
            Some(p) => (len, bar.v_surface_size(p).1),
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
    fn the_keyboard_reaches_every_applet_and_walks_its_popup() {
        use crate::app::{App, Key, NavKey};
        for style in STYLES {
            for edge in Edge::ALL {
                let mut bar = fixture(style, edge, AppletSet::all(), 1600);
                assert!(!bar.wants_keyboard(), "the bar never takes the keyboard by itself");
                bar.focus_panel();
                assert!(bar.wants_keyboard() && bar.kb_focus == Some(0), "{} {}", style.key(), edge.as_str());
                let n = bar.focus_items().len();
                assert!(n > 0);
                let mut seen = AppletSet::EMPTY;
                for i in 0..n {
                    assert_eq!(bar.kb_focus, Some(i));
                    match bar.focus_items()[i].0 {
                        Focus::Item(k) => {
                            if let Some(id) = applet_of(k) {
                                seen.set(id, true);
                            }
                        }
                        Focus::Menu(_) => seen.set(A::AppMenu, true),
                    }
                    bar.nav_key(NavKey::Right);
                }
                assert_eq!(bar.kb_focus, Some(0), "it wraps");
                bar.nav_key(NavKey::Left);
                assert_eq!(bar.kb_focus, Some(n - 1));
                // every applet drawn is a keyboard stop (the overflow cell stands for the rest)
                let drawn = items(&bar).iter().filter_map(|(k, _)| applet_of(*k)).fold(AppletSet::EMPTY, |s, id| s.with(id, true));
                assert_eq!(seen.intersect(drawn), drawn, "{} {}", style.key(), edge.as_str());
                // Enter on a popup applet opens it with the first row focused, the arrows walk it, Escape closes it and then leaves
                let at = bar.focus_items().iter().position(|(f, _)| *f == Focus::Item(Tray::Audio)).expect("sound");
                bar.kb_focus = Some(at);
                assert!(bar.nav_key(NavKey::Enter));
                assert_eq!(bar.open, Open::Popup(Tray::Audio), "{} {}", style.key(), edge.as_str());
                let first = bar.popup_hover.expect("a row is focused");
                bar.nav_key(NavKey::Down);
                assert_ne!(bar.popup_hover, Some(first));
                bar.nav_key(NavKey::Up);
                assert_eq!(bar.popup_hover, Some(first));
                assert!(bar.key(Key::Escape) && bar.open == Open::None && bar.kb_focus.is_some());
                assert!(bar.key(Key::Escape) && bar.kb_focus.is_none() && !bar.wants_keyboard());
                let mut pm = Pixmap::new(bar.width, bar.height.max(1)).unwrap();
                bar.draw(&mut pm);
            }
        }
        reset();
    }

    #[test]
    fn the_keyboard_opens_menus_and_runs_their_items_on_the_classic_bar() {
        use crate::app::{App, NavKey};
        let mut bar = fixture(Renderer::MenuBar, Edge::Bottom, AppletSet::of(&[A::AppMenu, A::Clock]), 1600);
        // menu rows are focusable only when their actions can run, which must not depend on the test machine
        bar.runner = super::super::menus::Runner::available_for_tests();
        bar.focus_panel();
        let at = bar.focus_items().iter().position(|(f, _)| matches!(f, Focus::Menu(1))).expect("the app menu title");
        bar.kb_focus = Some(at);
        bar.nav_key(NavKey::Enter);
        assert_eq!(bar.open, Open::Menu(1));
        assert!(bar.hover_row.is_some());
        let d = bar.dropdown(1).unwrap();
        assert!(d.y + d.h <= bar.strip_top() + 0.5, "a bottom bar's menu opens upwards");
        bar.nav_key(NavKey::Right);
        assert_eq!(bar.open, Open::Menu(2), "Right moves to the next menu");
        bar.nav_key(NavKey::Down);
        assert!(bar.hover_row.is_some());
        reset();
    }

    /// A theme with the GNOME quick-settings look (it scrolls) at text scale `scale`, for this test thread only.
    fn gnome_cc(scale: f32) -> crate::theme::live::ThreadTheme {
        use crate::controlcenter::settings::{Settings, Style, Tiles};
        let mut t = crate::theme::builtin::def("nord").unwrap().resolve("nord", true, &crate::theme::model::Selection::default());
        t.control_center = Settings { style: Style::Gnome, tiles: Tiles::ALL, ..Settings::default() };
        t.font_scale = scale;
        t.popup_alpha = 1.0;
        crate::theme::live::thread_theme(t)
    }

    /// The panel's walk of a short host: every control reached by the keyboard lies inside the visible part, the wheel
    /// scrolls and clamps at both ends, and the pointer reaches the control drawn under it (not the one at the same
    /// place before the scroll).
    fn walk_clipped_panel(bar: &mut DecBar, what: &str) {
        use crate::app::{App, NavKey};
        let visible = bar.cc_visible_height();
        assert!(bar.cc_clipped() && visible < bar.cc_height, "{what}: a short host clips the panel ({visible} of {})", bar.cc_height);
        let order = controlcenter::targets_in_order(&bar.cc_items);
        assert!(order.len() > 10, "{what}");
        for target in &order {
            assert!(bar.nav_key(NavKey::Down), "{what}: Down moves the focus");
            assert_eq!(bar.cc_viewport.focus, Some(*target), "{what}");
            let r = bar.cc_items.iter().find(|i| i.target == Some(*target)).unwrap().r;
            let scroll = bar.cc_viewport.scroll;
            assert!(r.y - scroll >= -0.5 && r.y + r.h - scroll <= visible + 0.5, "{what}: {target:?} revealed (y {} scroll {scroll} of {visible})", r.y);
        }
        assert!(bar.cc_viewport.scroll > 0.0, "{what}: the last control scrolled the panel");
        let last = *order.last().unwrap();
        // the pointer over the last control, now on screen, hovers it
        let (ox, oy) = bar.panel_origin();
        let r = bar.cc_items.iter().find(|i| i.target == Some(last)).unwrap().r;
        let (px, py) = (ox + r.x + r.w / 2.0, oy + r.y - bar.cc_viewport.scroll + r.h / 2.0);
        assert!(py < oy + visible, "{what}");
        if bar.embedded {
            bar.status_motion(px, py);
        } else {
            bar.pointer_motion(px, py);
        }
        assert_eq!(bar.cc_hover, Some(last), "{what}: the pointer reaches the scrolled control");
        // the wheel over a non-slider part scrolls back and clamps at the top, then down and clamps at the end
        let (wx, wy) = (ox + 6.0, oy + 6.0);
        for _ in 0..80 {
            bar.cc_scroll(wx, wy, -1.0);
        }
        assert_eq!(bar.cc_viewport.scroll, 0.0, "{what}");
        for _ in 0..80 {
            bar.cc_scroll(wx, wy, 1.0);
        }
        assert!((bar.cc_viewport.scroll - (bar.cc_height - visible)).abs() < 0.5, "{what}: clamped at the end ({})", bar.cc_viewport.scroll);
    }

    #[test]
    fn the_rail_clips_and_scrolls_a_tall_control_center_at_200_percent() {
        let _theme = gnome_cc(2.0);
        for (edge, len) in [(Edge::Left, 520), (Edge::Right, 480), (Edge::Bottom, 1280)] {
            let mut bar = fixture(Renderer::Pill, edge, AppletSet::all(), len);
            bar.relayout_control();
            bar.open = Open::Control;
            assert_eq!(bar.cc_width(), controlcenter::panel_width(bar.cc_style()), "the GNOME panel keeps its own width beside a rail");
            let room = bar.v_cc_room();
            let (ox, oy) = bar.panel_origin();
            let visible = bar.cc_visible_height();
            assert!(visible <= room + 0.5 && oy >= 0.0 && oy + visible <= bar.height as f32 + 0.5, "{edge:?}: panel {oy}+{visible} in a {} surface", bar.height);
            assert!(ox >= 0.0 && ox + bar.cc_width() <= bar.width as f32 + 0.5, "{edge:?}: panel x {ox}");
            // the input region is the visible panel (and, for a bottom rail, the way down to the strip), not the clipped rest
            let rects = bar.v_input_rects();
            let fits = |r: &crate::app::InputRect| match edge {
                Edge::Bottom => (r.y as f32 - oy.floor()).abs() <= 1.0,
                _ => ((r.y + r.h) as f32 - (oy + visible)).abs() <= 1.0,
            };
            assert!(rects.iter().skip(1).any(fits), "{edge:?}: {rects:?}");
            if bar.cc_height > room {
                walk_clipped_panel(&mut bar, &format!("rail {edge:?}"));
                // nothing is drawn past the visible panel
                let mut pm = Pixmap::new(bar.width, bar.height).unwrap();
                crate::app::App::draw(&mut bar, &mut pm);
                let below = (oy + visible + 2.0) as u32;
                if below < bar.height {
                    assert_eq!(pm.pixel((ox + 30.0) as u32, below).unwrap().alpha(), 0, "{edge:?}: clipped below the panel");
                }
            }
        }
        reset();
    }

    #[test]
    fn the_taskbar_engine_clips_and_scrolls_a_tall_control_center_at_200_percent() {
        let _theme = gnome_cc(2.0);
        // a bottom taskbar on a 480 px tall surface, and a left one 460 px long
        for vertical in [false, true] {
            let mut bar = DecBar::status_engine();
            bar.update_snapshot(|s| {
                *s = Snapshot {
                    net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: "Wired".into() }),
                    bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
                    volume: Some(sys::Volume { level: 0.5, muted: false }),
                    sink: "Headset".into(),
                    brightness: Some(40),
                    night_shift: Some(false),
                    dnd: Some(false),
                    media: Some(sys::Media { playing: true, title: "Song".into(), artist: "Artist".into() }),
                    battery: Some(sys::Battery { percent: 82, charging: false, status: "Discharging".into() }),
                    util: Some(sys::Utils::default()),
                    lock: true,
                    power: true,
                    ..Snapshot::default()
                }
            });
            bar.status_set_surface_height(480.0);
            assert!(bar.status_toggle(Tray::Control, if vertical { 400.0 } else { 1500.0 }));
            if vertical {
                bar.status_place_side(48.0, false, 460.0);
            } else {
                bar.status_place(480.0 - 48.0, true, 1600.0);
            }
            let what = if vertical { "left taskbar" } else { "bottom taskbar" };
            let (ox, oy) = bar.panel_origin();
            let visible = bar.cc_visible_height();
            let r = bar.status_rect().unwrap();
            assert!(oy >= 0.0 && (r.h as f32 - visible).abs() <= 1.0, "{what}: input region {r:?} is the visible panel {visible}");
            if vertical {
                assert!(oy + visible <= 460.0 && ox >= 48.0, "{what}");
            } else {
                assert!(oy + visible <= 480.0 - 48.0, "{what}: clear of the strip");
            }
            walk_clipped_panel(&mut bar, what);
            // the host's keyboard reaches the panel through the engine
            assert!(bar.status_nav_key(crate::app::NavKey::Up));
            let mut pm = Pixmap::new(1600, 480).unwrap();
            bar.status_draw(&mut pm);
        }
    }

    #[test]
    fn an_explicit_logo_that_cannot_be_had_is_the_neutral_glyph_never_a_trademark() {
        use crate::panel::branding::{Branding, Logo, NEUTRAL};
        let dir = std::env::temp_dir().join(format!("decebal-brand-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let corrupt = dir.join("corrupt.png");
        std::fs::write(&corrupt, b"not a png at all").unwrap();
        let ink = (230, 230, 230);
        let mut symbols = SymbolCache::default();
        let grid = render_logo(&NEUTRAL, 32, ink, &mut symbols).expect("the neutral glyph always renders");
        for logo in [Logo::Icon("decebal-no-such-icon-anywhere".into()), Logo::File(dir.join("missing.png")), Logo::File(corrupt.clone())] {
            TEST_BRANDING.with(|b| *b.borrow_mut() = Some(Branding { logo: logo.clone(), launcher_label: Some("Activities".into()), ..Branding::default() }));
            // the taskbar's Start: the neutral glyph, not the four squares
            let start = theme_logo(32, ink, &mut symbols).expect("a logo");
            assert_eq!(start.data(), grid.data(), "{logo:?}: Start shows the neutral glyph");
            // the bars: drawn (so the style's Apple / OS logo is not), with the theme's label kept
            let mut bar = fixture(Renderer::MenuBar, Edge::Top, AppletSet::of(&[A::Launcher, A::AppMenu]), 1280);
            let mut pm = Pixmap::new(64, 64).unwrap();
            assert!(bar.draw_brand(&mut pm, 16.0, 32.0, 32.0, ink), "{logo:?}: drawn instead of the style's logo");
            assert!(pm.data().chunks(4).any(|p| p[3] > 0), "{logo:?}: visible");
            assert_eq!(branding().launcher_label.as_deref(), Some("Activities"));
        }
        // without [branding] logo the style keeps its own (Apple on the menu bar, the four squares on the taskbar)
        TEST_BRANDING.with(|b| *b.borrow_mut() = Some(Branding::default()));
        assert!(theme_logo(32, ink, &mut symbols).is_none());
        let mut bar = fixture(Renderer::MenuBar, Edge::Top, AppletSet::of(&[A::AppMenu]), 1280);
        let mut pm = Pixmap::new(64, 64).unwrap();
        assert!(!bar.draw_brand(&mut pm, 16.0, 32.0, 32.0, ink));
        TEST_BRANDING.with(|b| *b.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
        reset();
    }

    #[test]
    fn every_style_and_edge_works_at_125_and_200_percent() {
        use crate::app::App;
        for scale in [1.25f32, 2.0] {
            for style in STYLES {
                for edge in Edge::ALL {
                    let bar = fixture(style, edge, AppletSet::all(), 1280);
                    let (lw, lh) = (bar.width, bar.height);
                    let logical: Vec<_> = items(&bar);
                    let spec = bar.surface_spec().unwrap();
                    let mut sc = crate::backend::scaled::Scaled::new(bar, scale);
                    let (pw, ph) = (((lw as f32) * scale).round() as u32, ((lh as f32) * scale).round() as u32);
                    sc.resized(pw, ph);
                    let sspec = sc.surface_spec().unwrap();
                    assert_eq!(sspec.reserve, (spec.reserve as f32 * scale).ceil() as i32, "{} {} {scale}: the strut scales", style.key(), edge.as_str());
                    let mut pm = Pixmap::new(pw, ph).unwrap();
                    sc.draw(&mut pm);
                    assert!(pm.data().chunks(4).any(|p| p[3] > 0));
                    // the sound applet, pressed where the surface shows it, opens its popup
                    let (_, r) = logical.iter().find(|(k, _)| *k == Tray::Audio).copied().expect("sound");
                    sc.pointer_button((r.0 + r.2 / 2.0) * scale, (r.1 + r.3 / 2.0) * scale, Button::Left);
                    assert_eq!(sc.inner().open, Open::Popup(Tray::Audio), "{} {} {scale}", style.key(), edge.as_str());
                    // the input region covers the popup in surface pixels
                    let p = sc.inner().popup.clone().unwrap();
                    let (cx, cy) = ((p.x + p.w / 2.0) * scale, (p.y + p.h / 2.0) * scale);
                    assert!(sc.input_rects().iter().any(|i| cx >= i.x as f32 && cx < (i.x + i.w) as f32 && cy >= i.y as f32 && cy < (i.y + i.h) as f32));
                    sc.draw(&mut pm);
                }
            }
        }
        reset();
    }

    #[test]
    fn task_buttons_group_windows_by_application() {
        let tasks = group_tasks(&FakeWm.windows());
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].active() && !tasks[1].active());
    }
}
