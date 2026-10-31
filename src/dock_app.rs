//! Glue between [`Dock`] and a display backend: pointer handling, click-to-launch, the right-click
//! context menu, drag-to-reorder and live reload of the pinned list.

use crate::app::{App, Button, InputRect, Key, NavKey, Tick, Waker};
use crate::backend::SurfaceSpec;
use crate::config::{self, edit, Stamp};
use crate::dock::{Dock, DockItem, Drag};
use crate::dock_menu::{self, Action, Ctx, DockMenu, Outcome, TEXT_PX};
use crate::launch;
use crate::text::{Fonts, Weight};
use crate::wm::{app_matches, display_name, Tracker};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

#[cfg(test)]
mod dpi_tests;
mod keyboard;
mod windows;

/// Width of `s` in menu text in logical pixels. At a display scale the glyphs are rasterised at the surface size, so they are measured
/// there and divided back: the menu's rows are as wide as the text drawn in them.
fn measure_logical(f: &Fonts, s: &str) -> f32 {
    let k = crate::gfx::render_scale();
    f.measure(s, TEXT_PX * k, Weight::Regular) / k
}

/// Side effects of the dock (starting programs), replaceable in tests.
pub trait Effects {
    /// Starts the app with desktop id `id` (or the built-in Launchpad / Trash).
    fn launch(&self, id: &str) -> bool;
    /// Runs a command detached.
    fn spawn(&self, argv: &[String]) -> bool;
}

struct System;

impl Effects for System {
    fn launch(&self, id: &str) -> bool {
        launch::launch(id)
    }

    fn spawn(&self, argv: &[String]) -> bool {
        launch::spawn_argv(argv)
    }
}

/// How often the config files are checked for external edits (a `stat` each).
const POLL_EVERY: Duration = Duration::from_millis(500);
/// Pointer travel before a press turns into a drag.
const DRAG_THRESHOLD: f32 = 8.0;
/// Gap between the menu and the top of the dock strip.
const MENU_GAP: f32 = 8.0;
/// Room kept above the menu inside the surface.
const MENU_TOP: f32 = 6.0;

/// y of the menu's bottom edge on a surface `h` pixels tall (just above the dock strip).
fn menu_bottom(h: u32, panel_height: f32) -> f32 {
    h as f32 - panel_height - MENU_GAP
}

/// Config files watched for external edits (the settings app writes them).
struct Watch {
    pinned_path: PathBuf,
    pinned_stamp: Stamp,
    settings_path: PathBuf,
    settings_stamp: Stamp,
    last_poll: Instant,
    every: Duration,
}

struct MenuState {
    menu: DockMenu,
    fonts: Option<Fonts>,
}

/// A left press that may become a click or a drag.
struct Press {
    index: usize,
    x: f32,
}

pub struct DockApp {
    pub dock: Dock,
    /// Pinned ids in dock order (what is saved).
    pinned: Vec<String>,
    /// Real surface size, as last reported by the backend.
    surface: (u32, u32),
    surface_origin: (i32, i32),
    tracker: Option<Tracker>,
    track_windows: bool,
    waker: Option<Waker>,
    effects: Box<dyn Effects>,
    watch: Option<Watch>,
    /// Theme generation last drawn (see `theme::generation`).
    theme_gen: u64,
    menu: Option<MenuState>,
    press: Option<Press>,
    /// window app id -> desktop id (None = no installed app), so `.desktop` files are scanned once.
    desktops: HashMap<String, Option<String>>,
    animating: bool,
    /// State of the Windows-style taskbar; created the first time that style is shown.
    win: Option<Box<windows::WinState>>,
    /// DPI scale for the taskbar, read once.
    scale: Option<f32>,
    /// Lets the dock close itself while `dock.enabled` (decebal.json) says it is off; see [`DockApp::with_gate`].
    gate: std::cell::RefCell<Option<crate::menubar::gate::Gate>>,
    /// `decdock --focus` (SIGUSR2): the request flag and the slot for the waker; see [`DockApp::with_focus_signal`].
    focus_signal: Option<&'static crate::menubar::FocusSignal>,
    /// Keyboard mode of the macOS dock (`decdock --focus`); the taskbar keeps its own in [`windows::WinState`].
    kb: Option<keyboard::Kb>,
}

impl DockApp {
    pub fn new(dock: Dock) -> Self {
        let surface = dock.canvas_size();
        let pinned = dock.items.iter().filter(|i| i.pinned).map(|i| i.id.clone()).collect();
        Self {
            dock,
            pinned,
            surface,
            tracker: None,
            track_windows: false,
            waker: None,
            effects: Box::new(System),
            watch: None,
            theme_gen: crate::theme::generation(),
            menu: None,
            press: None,
            desktops: HashMap::new(),
            animating: false,
            win: None,
            surface_origin: (0, 0),
            scale: None,
            gate: std::cell::RefCell::new(None),
            focus_signal: None,
            kb: None,
        }
    }

    /// Lets `decdock --focus` (SIGUSR2 to this process) start the taskbar's keyboard mode, like `decbar --focus` does
    /// for the bar. Call before any other thread starts: the signal is blocked here and collected by a thread of its own.
    pub fn with_focus_signal(mut self) -> Self {
        self.focus_signal = Some(crate::menubar::DecBar::init_focus_signal());
        self
    }

    /// Lets the dock close itself (the backend returns) while `dock.enabled` in `decebal.json` says it is off, which under
    /// `"auto"` is the case with a theme that has a vertical bar (Caelestia has no dock). `closed` is set when that happens.
    pub fn with_gate(self, closed: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        *self.gate.borrow_mut() = Some(crate::menubar::gate::Gate::with_check(closed, Box::new(crate::menubar::gate::dock_visible_now)));
        self
    }

    /// Native static gallery frame. Sample pins only; no trackers, watchers or event loop.
    pub fn preview_frame(width: u32) -> Result<Pixmap, String> {
        let dock = Dock::new(vec![DockItem::pinned("launchpad", false), DockItem::pinned("trash", false)], config::dock_settings());
        let mut app = Self::new(dock).with_scale(1.0);
        let (w, h) = if let Some(win) = app.win.as_mut() {
            win.time = "14:07".into();
            win.date = "23/10/2026".into();
            (width, win.m.bar_h.ceil() as u32)
        } else {
            app.dock.canvas_size()
        };
        app.resized(w, h);
        let mut pm = Pixmap::new(w, h).ok_or("dock preview canvas")?;
        app.draw(&mut pm);
        Ok(pm)
    }

    /// The dock or taskbar placed on its edge of a `width` x `height` desktop, as the backend would place it (the
    /// gallery's preview). Static sample pins; no trackers, watchers or event loop.
    pub fn preview_desktop(width: u32, height: u32) -> Result<Pixmap, String> {
        Self::preview_desktop_with(config::dock_settings(), width, height)
    }

    /// [`DockApp::preview_desktop`] for given dock settings.
    pub fn preview_desktop_with(settings: config::DockSettings, width: u32, height: u32) -> Result<Pixmap, String> {
        let dock = Dock::new(vec![DockItem::pinned("launchpad", false), DockItem::pinned("trash", false)], settings);
        let mut app = Self::new(dock).with_scale(1.0);
        let th = crate::theme::th();
        // a bar that is not shown (a taskbar theme's `enabled = "auto"`) takes no room from the strip's edge
        let insets = if config::shell_settings().bar_visible(app.dock.settings.style) { th.bar_insets() } else { crate::theme::Insets::default() };
        let mut desk = Pixmap::new(width, height).ok_or("dock preview canvas")?;
        let (w, h, x, y) = if let Some(win) = app.win.as_mut() {
            win.time = "14:07".into();
            win.date = "23/10/2026".into();
            win.status.status_fill_sample();
            // fixed sample tasks (a file manager pinned, a focused browser, a running terminal): the preview cache is not
            // keyed on installed apps, and launchpad and trash are never task buttons
            win.buttons = [("org.gnome.Nautilus", "", true, false), ("firefox", "Mozilla Firefox", true, true), ("foot", "Terminal", false, false)]
                .into_iter()
                .enumerate()
                .map(|(i, (id, title, pinned, active))| crate::taskbar::TaskButton {
                    app_id: id.into(),
                    window: (i > 0).then_some(i as u64),
                    title: title.into(),
                    pinned,
                    running: i > 0,
                    active,
                    windows: (i > 0) as usize,
                    ..Default::default()
                })
                .collect();
            let bar = (win.m.bar_h + win.m.float_gap).ceil() as u32;
            match app.win_vertical_edge() {
                Some(true) => (bar, height, width as i32 - bar as i32 - insets.right, 0),
                Some(false) => (bar, height, insets.left, 0),
                None if app.win_panel_edge_top() => (width, bar, 0, crate::theme::bar_bottom().ceil() as i32),
                None => (width, bar, 0, height as i32 - bar as i32 - insets.bottom),
            }
        } else {
            let (w, h) = app.dock.canvas_size();
            let spec = app.surface_spec().ok_or("dock spec")?;
            let (cx, cy) = ((width as i32 - w as i32) / 2, (height as i32 - h as i32) / 2);
            let (x, y) = match spec.anchor {
                crate::backend::Anchor::Bottom => (cx, height as i32 - h as i32 - spec.margin),
                crate::backend::Anchor::Top => (cx, spec.margin),
                crate::backend::Anchor::Left => (spec.margin, cy),
                crate::backend::Anchor::Right => (width as i32 - w as i32 - spec.margin, cy),
            };
            (w, h, x, y)
        };
        app.resized(w, h);
        let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or("dock preview canvas")?;
        app.draw(&mut pm);
        desk.draw_pixmap(x, y, pm.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        Ok(desk)
    }

    /// Forces the taskbar's DPI scale (tests; normally `DECEBAL_SCALE` or `Xft.dpi`).
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = Some(scale);
        if self.is_windows() {
            self.win_init();
        }
        self
    }

    /// Show a running dot under every pinned app that has an open window (tracker starts with the event loop).
    pub fn track_windows(mut self) -> Self {
        self.track_windows = true;
        self
    }

    /// Re-reads the pinned list and dock settings whenever the files in [`config::config_dir`] change.
    pub fn watch_config(self) -> Self {
        let dir = config::config_dir();
        self.watch_files(dir.join(config::PINNED_FILE), dir.join(config::SETTINGS_FILE))
    }

    /// Like [`DockApp::watch_config`] with explicit paths.
    pub fn watch_files(mut self, pinned_path: PathBuf, settings_path: PathBuf) -> Self {
        self.watch = Some(Watch { pinned_stamp: Stamp::of(&pinned_path), settings_stamp: Stamp::of(&settings_path), pinned_path, settings_path, last_poll: Instant::now(), every: POLL_EVERY });
        self
    }

    pub fn with_effects(mut self, effects: Box<dyn Effects>) -> Self {
        self.effects = effects;
        self
    }

    fn wake(&self) {
        if let Some(w) = &self.waker {
            w.wake();
        }
    }

    // ------------------------------------------------------------------ items

    fn desktop_id(&mut self, app_id: &str) -> Option<String> {
        self.desktops.entry(app_id.to_lowercase()).or_insert_with(|| launch::resolve_desktop_id(app_id)).clone()
    }

    /// Rebuilds the dock items from the pinned list and the open windows; true when they changed.
    /// Running apps that are not pinned appear before the Trash, like on macOS.
    fn sync_items(&mut self) -> bool {
        if self.dock.drag.is_some() {
            return false;
        }
        let running_ids: Vec<String> = self.tracker.as_ref().map(|t| t.snapshot().windows.into_iter().map(|w| w.app_id).collect()).unwrap_or_default();
        let have_tracker = self.tracker.is_some();
        let old = std::mem::take(&mut self.dock.items);
        let mut items: Vec<DockItem> = self
            .pinned
            .iter()
            .map(|id| {
                let running = if have_tracker { running_ids.iter().any(|a| app_matches(id, a)) } else { old.iter().any(|o| &o.id == id && o.running) };
                DockItem::pinned(id, running)
            })
            .collect();
        let mut extra: Vec<DockItem> = Vec::new();
        for app in &running_ids {
            if items.iter().chain(&extra).any(|i| app_matches(&i.id, app)) {
                continue;
            }
            if let Some(id) = self.desktop_id(app) {
                if !items.iter().chain(&extra).any(|i| i.id == id || app_matches(&i.id, &id)) {
                    extra.push(DockItem { id, running: true, pinned: false });
                }
            }
        }
        let at = if items.last().is_some_and(|i| i.id == "trash") { items.len() - 1 } else { items.len() };
        items.splice(at..at, extra);
        let changed = items != old;
        self.dock.items = items;
        if changed {
            self.press = None; // indices in a pending press may be stale
        }
        changed
    }

    // ------------------------------------------------------------------ config

    fn save_pinned(&mut self) {
        let Some(w) = &mut self.watch else { return };
        match config::write_pinned(&w.pinned_path, &self.pinned) {
            Ok(()) => w.pinned_stamp = Stamp::of(&w.pinned_path),
            Err(e) => eprintln!("decdock: cannot save {}: {e}", w.pinned_path.display()),
        }
    }

    /// Checks the watched files (at most every `every`); true when something changed on screen.
    fn poll_config(&mut self) -> bool {
        let Some(w) = &mut self.watch else { return false };
        if w.last_poll.elapsed() < w.every {
            return false;
        }
        w.last_poll = Instant::now();
        let mut changed = false;
        let stamp = Stamp::of(&w.pinned_path);
        if stamp != w.pinned_stamp {
            w.pinned_stamp = stamp;
            if let Some(p) = std::fs::read_to_string(&w.pinned_path).ok().and_then(|t| serde_json::from_str::<config::Pinned>(&t).ok()) {
                let list = p.or_default(&|id| launch::find_exec(id).is_some());
                if list != self.pinned && self.dock.drag.is_none() {
                    self.pinned = list;
                    changed = true;
                }
            } else if !w.pinned_path.exists() {
                let list = config::default_pinned(&|id| launch::find_exec(id).is_some());
                if list != self.pinned {
                    self.pinned = list;
                    changed = true;
                }
            }
        }
        let stamp = Stamp::of(&w.settings_path);
        if stamp != w.settings_stamp {
            w.settings_stamp = stamp;
            // A file that does not parse (half-written by another tool) keeps the current look; a missing one resets it.
            let parsed = match std::fs::read_to_string(&w.settings_path) {
                Ok(t) => serde_json::from_str::<config::DockSettings>(&t).ok(),
                Err(_) => Some(config::DockSettings::default()),
            };
            if let Some(s) = parsed {
                if self.apply_settings(s) {
                    changed = true;
                }
            }
        }
        changed
    }

    // ------------------------------------------------------------------ clicks

    /// Focuses the app's window when it already runs (like macOS); false when a launch is needed.
    fn activate_running(&self, id: &str) -> bool {
        let Some(win) = self.tracker.as_ref().and_then(|t| t.window_for(id)) else { return false };
        if let Some(t) = &self.tracker {
            t.activate(win.id);
        }
        true
    }

    fn launch_item(&mut self, id: &str) {
        if !self.effects.launch(id) {
            eprintln!("decdock: could not launch {id}");
            return;
        }
        if id != "launchpad" && id != "trash" && !self.is_windows() {
            self.dock.start_bounce(id);
            self.wake();
        }
    }

    /// Plain left click: focus a running app, otherwise start it.
    fn click_item(&mut self, i: usize) {
        let Some(item) = self.dock.items.get(i) else { return };
        let id = item.id.clone();
        if matches!(id.as_str(), "launchpad" | "trash") || !self.activate_running(&id) {
            self.launch_item(&id);
        }
    }

    // -------------------------------------------------------------------- menu

    fn close_menu(&mut self) -> bool {
        self.press = None;
        self.menu.take().is_some()
    }

    /// Length of the surface along the dock's edge.
    fn dock_len(&self) -> f32 {
        if self.dock.edge().is_vertical() {
            self.surface.1 as f32
        } else {
            self.surface.0 as f32
        }
    }

    /// The coordinate along the dock's edge of a surface point (x of the bottom dock, y of a side one).
    fn along(&self, x: f32, y: f32) -> f32 {
        self.dock.to_frame(self.surface.0 as f32, self.surface.1 as f32, x, y).0
    }

    /// Where the open context menu of the macOS dock is drawn: over a bottom dock, under a top one, beside a side one
    /// (centred on its icon along the edge), on the surface.
    fn mac_menu(&self, m: &DockMenu) -> crate::menubar::menus::Dropdown {
        use crate::panel::Edge;
        let (w, h) = (self.surface.0 as f32, self.surface.1 as f32);
        let ph = self.dock.panel_height();
        match self.dock.edge() {
            Edge::Bottom => m.placed(w, menu_bottom(self.surface.1, ph)),
            Edge::Top => m.placed(w, ph + MENU_GAP + m.size().1),
            edge => {
                let mut d = m.placed(f32::MAX / 8.0, 0.0);
                d.x = if edge == Edge::Right { (w - ph - MENU_GAP - d.w).max(0.0) } else { ph + MENU_GAP };
                let y = (m.anchor_x - d.h / 2.0).clamp(MENU_TOP, (h - d.h - MENU_TOP).max(MENU_TOP));
                d.shift_y(y - d.y);
                d
            }
        }
    }

    fn open_menu(&mut self, i: usize) {
        let Some(item) = self.dock.items.get(i) else { return };
        let layout = self.dock.layout(self.dock_len());
        let Some(cell) = layout.cells.get(i) else { return };
        let windows = self.tracker.as_ref().map(|t| t.windows_for(&item.id)).unwrap_or_default();
        let ctx = Ctx {
            id: item.id.clone(),
            display_name: display_name(&item.id),
            running: item.running,
            pinned: item.pinned,
            windows: windows.into_iter().map(|w| (w.id, w.title)).collect(),
            has_desktop: launch::find_desktop_file(&item.id).is_some(),
        };
        let fonts = Fonts::load();
        let measure = |s: &str| fonts.as_ref().map_or(s.chars().count() as f32 * 7.0, |f| measure_logical(f, s));
        let mut entries = dock_menu::build_entries(&ctx);
        entries.extend(dock_menu::style_entries());
        let menu = DockMenu::new(&item.id, entries, cell.x + cell.w / 2.0, &measure);
        if let Some(f) = &fonts {
            f.trim();
        }
        self.dock.hover_x = None;
        self.menu = Some(MenuState { menu, fonts });
        self.wake();
    }

    /// The macOS dock's own menu (style switch, settings), opened on the panel's empty parts.
    fn open_dock_menu(&mut self, x: f32) {
        let s = &self.dock.settings;
        let entries = dock_menu::build_entries_dock(s.style, s.windows_align, s.windows_labels);
        let fonts = Fonts::load();
        let measure = |s: &str| fonts.as_ref().map_or(s.chars().count() as f32 * 7.0, |f| measure_logical(f, s));
        let menu = DockMenu::new("dock", entries, x, &measure);
        if let Some(f) = &fonts {
            f.trim();
        }
        self.dock.hover_x = None;
        self.menu = Some(MenuState { menu, fonts });
        self.wake();
    }

    fn run_action(&mut self, item_id: &str, action: Action) {
        match action {
            Action::Focus(win) => {
                if let Some(t) = &self.tracker {
                    t.activate(win);
                }
            }
            Action::Open => self.launch_item(item_id),
            Action::KeepInDock => {
                edit::add(&mut self.pinned, item_id);
                self.save_pinned();
                self.sync_items();
            }
            Action::RemoveFromDock => {
                if edit::remove(&mut self.pinned, item_id) {
                    self.save_pinned();
                    self.sync_items();
                }
            }
            Action::ShowInFinder => {
                if let Some(dir) = launch::find_desktop_file(item_id).and_then(|p| p.parent().map(|d| d.to_path_buf())) {
                    self.effects.spawn(&launch::open_argv(&dir.to_string_lossy()));
                }
            }
            Action::Hide => {
                if let Some(t) = &self.tracker {
                    for w in t.windows_for(item_id) {
                        t.minimize(w.id);
                    }
                }
            }
            Action::Quit => {
                if let Some(t) = &self.tracker {
                    for w in t.windows_for(item_id) {
                        t.close(w.id);
                    }
                }
            }
            Action::EmptyTrash => {
                self.effects.spawn(&launch::empty_trash_argv());
            }
            other => {
                self.win_run_action(other);
            }
        }
    }

    fn finish_menu(&mut self, out: Outcome) -> bool {
        match out {
            Outcome::None => false,
            Outcome::Redraw => true,
            Outcome::Close => self.close_menu(),
            Outcome::Run(a) => {
                let id = self.menu.as_ref().map(|m| m.menu.item_id.clone()).unwrap_or_default();
                self.close_menu();
                self.run_action(&id, a);
                true
            }
        }
    }

    // -------------------------------------------------------------------- drag

    fn draggable(&self, i: usize) -> bool {
        self.dock.items.get(i).is_some_and(|it| it.pinned && !it.is_builtin())
    }

    /// Moves the dragged item over the neighbour under the pointer.
    fn drag_to(&mut self, x: f32) {
        let Some(d) = self.dock.drag else { return };
        let layout = self.dock.layout(self.dock_len());
        let target = layout.cells.iter().position(|c| x >= c.x - 3.0 && x <= c.x + c.w + 3.0);
        let mut index = d.index;
        if let Some(t) = target {
            if t != d.index && self.draggable(t) {
                let it = self.dock.items.remove(d.index);
                self.dock.items.insert(t, it);
                index = t;
            }
        }
        self.dock.drag = Some(Drag { index, x });
    }

    fn end_drag(&mut self) {
        if self.dock.drag.take().is_none() {
            return;
        }
        let order: Vec<String> = self.dock.items.iter().filter(|i| i.pinned).map(|i| i.id.clone()).collect();
        if order != self.pinned {
            self.pinned = order;
            self.save_pinned();
        }
    }
}

impl App for DockApp {
    fn size(&self) -> (u32, u32) {
        if self.is_windows() && self.win.is_some() {
            return self.win_size();
        }
        let (w, h) = self.dock.canvas_size();
        match &self.menu {
            // a side dock's menu opens beside it, the others' over / under it
            Some(m) if self.dock.edge().is_vertical() => {
                let need = (m.menu.size().0 + self.dock.panel_height() + MENU_GAP + MENU_TOP).ceil() as u32;
                (w.max(need), h.max((m.menu.size().1 + 2.0 * MENU_TOP).ceil() as u32))
            }
            Some(m) => {
                let need = (m.menu.size().1 + self.dock.panel_height() + MENU_GAP + MENU_TOP).ceil() as u32;
                (w, h.max(need))
            }
            None => (w, h),
        }
    }

    fn resized(&mut self, w: u32, h: u32) {
        self.surface = (w, h);
    }

    /// The macOS dock is drawn at the display scale by the backend; the taskbar applies the scale to itself.
    fn backend_scaled(&self) -> bool {
        !self.is_windows()
    }

    fn output_size(&mut self, w: u32, h: u32) {
        self.dock.output = Some((w as f32, h as f32));
    }

    fn surface_origin(&mut self, x: i32, y: i32) {
        self.surface_origin = (x, y);
        if let Some(win) = &mut self.win {
            win.status.status_surface_origin(x, y);
        }
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        if self.is_windows() && self.win.is_some() {
            return self.win_draw(pm);
        }
        self.dock.focus = self.kb.as_ref().map(|k| k.index);
        self.dock.draw(pm);
        let placed = self.menu.as_ref().map(|m| self.mac_menu(&m.menu));
        if let (Some(m), Some(d)) = (&mut self.menu, placed) {
            if let Some(f) = &mut m.fonts {
                m.menu.draw(pm, &d, f);
                f.trim();
            }
        }
    }

    fn glass_regions(&self) -> Vec<crate::glass::Region> {
        let mut regions = Vec::new();
        if let Some(w) = self.win.as_ref().filter(|_| self.is_windows()) {
            let r = self.win_strip_rect();
            regions.push(crate::glass::Region { x: r.0, y: r.1, w: r.2, h: r.3, radius: 0.0 });
            let _ = w;
            regions.extend(self.win.as_ref().map(|w| w.status.status_glass_regions()).unwrap_or_default());
        } else {
            regions.push(self.dock.glass_region(self.surface.0 as f32, self.surface.1 as f32));
        }
        if let Some(menu) = &self.menu {
            let placed = if self.is_windows() && self.win.is_some() { self.win_menu_dropdown(&menu.menu) } else { self.mac_menu(&menu.menu) };
            regions.push(crate::glass::Region { x: placed.x, y: placed.y, w: placed.w, h: placed.h, radius: crate::theme::th().radius });
        }
        regions
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> bool {
        if self.is_windows() {
            return self.win_motion(x, y);
        }
        let along = self.along(x, y);
        if let Some(p) = &self.press {
            if self.dock.drag.is_none() && (along - p.x).abs() > DRAG_THRESHOLD && self.draggable(p.index) && self.menu.is_none() {
                self.dock.drag = Some(Drag { index: p.index, x: along });
                self.dock.hover_x = None;
                self.wake();
            }
        }
        if self.dock.drag.is_some() {
            self.drag_to(along);
            self.wake();
            return true;
        }
        let placed = self.menu.as_ref().map(|m| self.mac_menu(&m.menu));
        if let (Some(m), Some(d)) = (&mut self.menu, placed) {
            if d.contains(x, y) {
                return m.menu.pointer_moved(&d, x, y);
            }
            let cleared = m.menu.pointer_moved(&d, -1.0, -1.0);
            return cleared;
        }
        // While the keyboard mode holds the pointer grab, motion anywhere on the screen reaches the dock: only motion over its strip hovers
        let over = self.input_rects().first().is_none_or(|r| x >= r.x as f32 && x < (r.x + r.w) as f32 && y >= r.y as f32 && y < (r.y + r.h) as f32);
        if !over {
            let was = self.dock.hover_x.take().is_some();
            if was {
                self.wake();
            }
            return was;
        }
        let changed = self.dock.hover_x != Some(along);
        self.dock.hover_x = Some(along);
        if changed {
            self.wake(); // start the easing animation
        }
        changed
    }

    fn pointer_leave(&mut self) -> bool {
        if self.is_windows() {
            return self.win_leave();
        }
        if self.menu.is_some() {
            // Only reached without a pointer grab (Wayland): moving away closes the menu.
            return self.close_menu();
        }
        let was = self.dock.hover_x.take().is_some();
        if was {
            self.wake();
        }
        was
    }

    fn set_waker(&mut self, waker: Waker) {
        if self.track_windows && self.tracker.is_none() {
            let w = waker.clone();
            self.tracker = Some(Tracker::start(move || w.wake()));
        }
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w.wake());
        if let Some((_, slot)) = self.focus_signal {
            if let Ok(mut s) = slot.lock() {
                *s = Some(waker.clone());
            }
        }
        self.waker = Some(waker);
        if self.is_windows() {
            self.win_init();
        }
    }

    fn surface_spec(&self) -> Option<SurfaceSpec> {
        if self.is_windows() {
            return self.win_spec();
        }
        // a dock kept next to the Caelestia pill bar floats above it when that runs along the bottom (`bottom`, `split`)
        // the dock floats 13 px off its edge, and further when the bar uses the same edge (it never covers it)
        let th = crate::theme::th();
        let insets = th.bar_insets();
        let edge = self.dock.edge();
        let bar = match edge {
            crate::panel::Edge::Bottom => insets.bottom,
            crate::panel::Edge::Top => crate::theme::bar_bottom().ceil() as i32,
            crate::panel::Edge::Left => insets.left,
            crate::panel::Edge::Right => insets.right,
        };
        Some(SurfaceSpec { anchor: edge.anchor(), margin: 13 + bar, reserve: self.dock.panel_height().ceil() as i32, stretch: false, namespace: "decebal-dock" })
    }

    fn quit_requested(&self) -> bool {
        let mut gate = self.gate.borrow_mut();
        match gate.as_mut() {
            Some(g) => {
                g.poll();
                g.closed()
            }
            None => false,
        }
    }

    fn tick(&mut self) -> Tick {
        let mut changed = self.poll_config();
        let theme_gen = crate::theme::generation();
        if theme_gen != self.theme_gen {
            // repaint with the new colours; settings keys the file leaves out default to the theme's values
            self.theme_gen = theme_gen;
            if let Some(w) = &self.watch {
                // Defaults can depend on theme tokens; use the normal settings
                // transition path so a simultaneous style change stays coherent.
                let settings = std::fs::read_to_string(&w.settings_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
                self.apply_settings(settings);
            }
            if self.is_windows() {
                self.win_apply_settings();
            }
            changed = true;
        }
        let focus = self.focus_signal.is_some_and(|(flag, _)| flag.swap(false, std::sync::atomic::Ordering::SeqCst));
        if self.is_windows() {
            self.kb = None;
            let mut tick = self.win_tick();
            tick.redraw |= changed || focus && self.win_focus_start();
            return tick;
        }
        changed |= self.sync_items();
        changed |= self.kb_sync(focus);
        let moving = self.dock.step(self.dock_len());
        let redraw = changed || moving || self.animating;
        self.animating = moving;
        let mut next = moving.then(|| Duration::from_millis(16));
        if let Some(w) = &self.watch {
            let due = w.every.saturating_sub(w.last_poll.elapsed()).max(Duration::from_millis(20));
            next = Some(next.map_or(due, |n| n.min(due)));
        }
        Tick { redraw, next }
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        if self.is_windows() {
            return self.win_button(x, y, button);
        }
        // the pointer ends keyboard mode
        let left_keyboard = self.kb.take().is_some();
        let (w, h) = self.surface;
        let along = self.along(x, y);
        let placed = self.menu.as_ref().map(|m| self.mac_menu(&m.menu));
        if let (Some(m), Some(d)) = (&mut self.menu, placed) {
            if button == Button::Left && m.menu.press(&d, x, y) {
                return left_keyboard;
            }
            if d.contains(x, y) {
                return left_keyboard; // other buttons on the menu do nothing
            }
            // A click anywhere else dismisses the menu; a right click on another icon opens its menu.
            self.close_menu();
            if button == Button::Right {
                if let Some(i) = self.dock.hit_test(w as f32, h as f32, x, y) {
                    self.open_menu(i);
                }
            }
            return true;
        }
        let hit = self.dock.hit_test(w as f32, h as f32, x, y);
        let changed = match (button, hit) {
            (Button::Left, Some(i)) => {
                self.press = Some(Press { index: i, x: along });
                false
            }
            (Button::Right, Some(i)) => {
                self.open_menu(i);
                true
            }
            (Button::Right, None) => {
                // The dock's own menu: right click on the panel between or around the icons.
                let (px, py, pw, ph) = self.dock.panel_rect(w as f32, h as f32);
                if x >= px && x <= px + pw && y >= py && y <= py + ph {
                    self.open_dock_menu(along);
                    return true;
                }
                false
            }
            (Button::Middle, Some(i)) => {
                // Middle click always starts a new instance.
                let id = self.dock.items[i].id.clone();
                self.launch_item(&id);
                false
            }
            _ => false,
        };
        left_keyboard || changed
    }

    fn pointer_release(&mut self, x: f32, y: f32, button: Button) -> bool {
        if self.is_windows() {
            return self.win_release(x, y, button);
        }
        if button != Button::Left {
            return false;
        }
        let (w, h) = self.surface;
        if self.menu.is_some() {
            let placed = self.menu.as_ref().map(|m| self.mac_menu(&m.menu));
            let out = match (&mut self.menu, placed) {
                (Some(m), Some(d)) => m.menu.release(&d, x, y),
                _ => Outcome::None,
            };
            return self.finish_menu(out);
        }
        if self.dock.drag.is_some() {
            self.end_drag();
            self.press = None;
            self.wake();
            return true;
        }
        if let Some(p) = self.press.take() {
            if self.dock.hit_test(w as f32, h as f32, x, y) == Some(p.index) {
                self.click_item(p.index);
            }
        }
        false
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        self.is_windows() && self.win_scroll(x, y, dy)
    }

    fn key(&mut self, key: Key) -> bool {
        if self.is_windows() {
            // Escape closes a menu or popup first, then leaves keyboard mode
            return matches!(key, Key::Escape) && (self.win_escape() || self.win_focus_end());
        }
        match key {
            Key::Escape => self.kb_escape(),
        }
    }

    fn home_end(&mut self, end: bool) -> bool {
        !self.is_windows() && self.kb_home_end(end)
    }

    fn nav_key(&mut self, key: NavKey) -> bool {
        if self.menu.is_none() && self.is_windows() {
            return self.win_nav_key(key);
        }
        if self.menu.is_none() && self.kb.is_some() {
            return self.kb_nav_key(key);
        }
        let out = match (&mut self.menu, key) {
            (Some(m), NavKey::Up) => return m.menu.step(-1),
            (Some(m), NavKey::Down) => return m.menu.step(1),
            (Some(m), NavKey::Enter) => m.menu.activate_hovered(),
            (Some(_), NavKey::Left | NavKey::Right) | (None, _) => return false,
        };
        self.kb_menu_enter(out)
    }

    fn wants_keyboard(&self) -> bool {
        if self.is_windows() {
            return self.win_wants_keyboard();
        }
        self.menu.is_some() || self.kb.is_some()
    }

    fn wants_pointer_grab(&self) -> bool {
        if self.is_windows() {
            return self.win_wants_keyboard();
        }
        self.menu.is_some() || self.kb.is_some()
    }

    fn input_rect(&self) -> Option<InputRect> {
        self.input_rects().into_iter().next()
    }

    fn input_rects(&self) -> Vec<InputRect> {
        if self.is_windows() && self.win.is_some() {
            return self.win_input_rects();
        }
        let (w, h) = self.surface;
        let strip = self.dock.panel_height() + 8.0;
        let len = if self.dock.edge().is_vertical() { h } else { w } as f32;
        let (rx, ry, rw, rh) = self.dock.map(w as f32, h as f32, 0.0, 0.0, len, strip);
        let mut rects = vec![InputRect { x: rx.floor() as i32, y: ry.floor() as i32, w: rw.ceil() as i32, h: rh.ceil() as i32 }];
        if let Some(m) = &self.menu {
            let d = self.mac_menu(&m.menu);
            rects.push(InputRect { x: d.x.floor() as i32, y: d.y.floor() as i32, w: d.w.ceil() as i32 + 1, h: d.h.ceil() as i32 });
        }
        rects
    }

    fn idle(&mut self) -> Option<Duration> {
        if let Some(m) = &self.menu {
            if let Some(f) = &m.fonts {
                f.trim();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Anchor;
    use crate::config::DockSettings;
    use crate::wm::{Control, Window};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    /// Tests that point `XDG_DATA_HOME` at a private directory must not overlap.
    static ENV: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct Calls {
        activated: Vec<u64>,
        minimized: Vec<u64>,
        closed: Vec<u64>,
    }

    struct Rec(Arc<Mutex<Calls>>);
    impl Control for Rec {
        fn activate(&self, id: u64) {
            self.0.lock().unwrap().activated.push(id);
        }
        fn switch_workspace(&self, _: usize) {}
        fn minimize(&self, id: u64) {
            self.0.lock().unwrap().minimized.push(id);
        }
        fn close(&self, id: u64) {
            self.0.lock().unwrap().closed.push(id);
        }
    }

    #[derive(Default)]
    struct Spawned {
        launched: Vec<String>,
        argv: Vec<Vec<String>>,
    }

    struct Fx(Rc<RefCell<Spawned>>);
    impl Effects for Fx {
        fn launch(&self, id: &str) -> bool {
            self.0.borrow_mut().launched.push(id.to_string());
            true
        }
        fn spawn(&self, argv: &[String]) -> bool {
            self.0.borrow_mut().argv.push(argv.to_vec());
            true
        }
    }

    struct Rig {
        app: DockApp,
        calls: Arc<Mutex<Calls>>,
        fx: Rc<RefCell<Spawned>>,
        dir: PathBuf,
    }

    #[test]
    fn windows_taskbar_recolours_on_theme_generation_without_restarting() {
        const CHILD: &str = "DECEBAL_TASKBAR_GENERATION_TEST_CHILD";
        // Publishing a process-global theme must not interfere with parallel tests.
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "dock_app::tests::windows_taskbar_recolours_on_theme_generation_without_restarting", "--nocapture"])
                .env(CHILD, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let mut rig = rig("windows-generation", &[], vec![]);
        std::fs::write(rig.dir.join("settings.json"), r#"{"style":"windows"}"#).unwrap();
        rig.app.tick();
        let original = std::ptr::from_ref(rig.app.win.as_deref().unwrap());
        let src = crate::theme::live::Source { shell_json: rig.dir.join("decebal.json"), themes_dir: rig.dir.join("themes") };
        for palette in ["catppuccin", "nord", "gruvbox"] {
            let selection = crate::theme::Selection { name: "windows11".into(), palette: Some(palette.into()), mode: crate::theme::Mode::Dark, ..Default::default() };
            let theme = src.theme_for("windows11", &selection, true).unwrap();
            let expected = crate::taskbar::draw::Colors::from_theme(&theme);
            crate::theme::live::publish_theme(theme);
            assert!(rig.app.tick().redraw);
            assert_eq!(rig.app.win.as_ref().unwrap().colors, expected);
            assert_eq!(std::ptr::from_ref(rig.app.win.as_deref().unwrap()), original);
        }
        for style in ["mac", "windows"] {
            std::fs::write(rig.dir.join("settings.json"), format!(r#"{{"style":"{style}"}}"#)).unwrap();
            assert!(rig.app.tick().redraw);
            assert_eq!(rig.app.is_windows(), style == "windows");
        }
    }

    fn win(id: u64, app: &str, title: &str) -> Window {
        Window { id, app_id: app.into(), title: title.into(), active: false, desktop: None, ..Default::default() }
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decebal-dockapp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A dock with these pinned ids and these windows, saving into a private temp directory.
    fn rig(name: &str, pinned: &[&str], windows: Vec<Window>) -> Rig {
        let dir = tmp_dir(name);
        let items = pinned.iter().map(|id| DockItem::pinned(id, false)).collect();
        let calls = Arc::new(Mutex::new(Calls::default()));
        let fx = Rc::new(RefCell::new(Spawned::default()));
        let tracker = Tracker::default();
        tracker.set_control(Arc::new(Rec(calls.clone())));
        tracker.replace(windows, vec![]);
        let mut app = DockApp::new(Dock::new(items, DockSettings::default())).with_effects(Box::new(Fx(fx.clone()))).watch_files(dir.join("pinned.json"), dir.join("settings.json"));
        app.tracker = Some(tracker);
        app.watch.as_mut().unwrap().every = Duration::ZERO;
        app.tick();
        Rig { app, calls, fx, dir }
    }

    impl Rig {
        fn centre(&self, i: usize) -> (f32, f32) {
            let l = self.app.dock.layout(self.app.surface.0 as f32);
            (l.cells[i].x + l.cells[i].w / 2.0, self.app.surface.1 as f32 - 20.0)
        }

        fn click(&mut self, i: usize, b: Button) {
            let (x, y) = self.centre(i);
            self.app.pointer_button(x, y, b);
            self.app.pointer_release(x, y, b);
        }

        /// The backend would resize the surface after `size()` changed.
        fn settle(&mut self) {
            let (w, h) = self.app.size();
            self.app.resized(w, h);
        }

        fn open_menu(&mut self, i: usize) {
            self.click(i, Button::Right);
            self.settle();
        }

        fn menu_labels(&self) -> Vec<String> {
            self.app.menu.as_ref().map(|m| m.menu.entries.iter().map(|e| e.label.clone()).collect()).unwrap_or_default()
        }

        fn row_point(&self, label: &str) -> (f32, f32) {
            let m = &self.app.menu.as_ref().unwrap().menu;
            let d = self.app.mac_menu(m);
            let i = m.entries.iter().position(|e| e.label == label).unwrap_or_else(|| panic!("no {label}"));
            (d.x + 30.0, d.rows[i].y + 5.0)
        }

        fn choose(&mut self, label: &str) {
            let (x, y) = self.row_point(label);
            self.app.pointer_motion(x, y);
            self.app.pointer_button(x, y, Button::Left);
            self.app.pointer_release(x, y, Button::Left);
            self.settle();
        }

        fn saved(&self) -> Vec<String> {
            let t = std::fs::read_to_string(self.dir.join("pinned.json")).unwrap();
            serde_json::from_str::<config::Pinned>(&t).unwrap().ordered()
        }
    }

    fn ids(app: &DockApp) -> Vec<String> {
        app.dock.items.iter().map(|i| i.id.clone()).collect()
    }

    #[test]
    fn clicking_a_running_app_focuses_its_window() {
        let mut r = rig("focus", &["launchpad", "foot", "firefox", "trash"], vec![win(7, "foot", ""), win(9, "firefox", "")]);
        assert!(r.app.activate_running("firefox"));
        r.click(2, Button::Left);
        assert_eq!(r.calls.lock().unwrap().activated, vec![9, 9]);
        assert!(r.fx.borrow().launched.is_empty(), "no second instance");
    }

    #[test]
    fn clicking_a_closed_app_launches_and_bounces() {
        let mut r = rig("launch", &["launchpad", "foot", "trash"], vec![]);
        r.click(1, Button::Left);
        assert_eq!(r.fx.borrow().launched, vec!["foot"]);
        assert!(r.app.dock.bouncing());
        assert!(r.calls.lock().unwrap().activated.is_empty());
        assert!(!DockApp::new(Dock::new(vec![], DockSettings::default())).activate_running("foot"), "no tracker");
    }

    #[test]
    fn launchpad_and_trash_launch_without_bounce() {
        let mut r = rig("builtin", &["launchpad", "foot", "trash"], vec![]);
        r.click(0, Button::Left);
        r.click(2, Button::Left);
        assert_eq!(r.fx.borrow().launched, vec!["launchpad", "trash"]);
        assert!(!r.app.dock.bouncing());
    }

    #[test]
    fn middle_click_starts_a_new_instance_even_when_running() {
        let mut r = rig("middle", &["launchpad", "foot", "trash"], vec![win(7, "foot", "")]);
        r.click(1, Button::Middle);
        assert_eq!(r.fx.borrow().launched, vec!["foot"]);
        assert!(r.calls.lock().unwrap().activated.is_empty());
    }

    #[test]
    fn running_dots_follow_the_windows() {
        let r = rig("dots", &["launchpad", "foot", "firefox", "trash"], vec![win(7, "foot", "")]);
        let running: Vec<bool> = r.app.dock.items.iter().map(|i| i.running).collect();
        assert_eq!(running, vec![false, true, false, false]);
    }

    #[test]
    fn right_click_opens_a_menu_that_grows_the_surface_but_not_the_input_outside() {
        let mut r = rig("open", &["launchpad", "foot", "trash"], vec![win(7, "foot", "Shell")]);
        let base = r.app.size();
        r.open_menu(1);
        assert!(r.app.menu.is_some() && r.app.wants_keyboard() && r.app.wants_pointer_grab());
        assert!(r.app.size().1 > base.1, "room for the menu");
        assert_eq!(r.app.size().0, base.0, "width (and so the strut span) is unchanged");
        assert_eq!(r.menu_labels()[0], "Shell");
        let rects = r.app.input_rects();
        assert_eq!(rects.len(), 2, "dock strip and menu only");
        assert!(rects[1].y + rects[1].h <= rects[0].y + 1, "menu sits above the strip");
        assert!(rects[1].y >= 0);
        r.app.key(Key::Escape);
        assert!(r.app.menu.is_none());
        assert_eq!(r.app.size(), base);
        assert_eq!(r.app.input_rects().len(), 1);
    }

    #[test]
    fn clicking_outside_closes_and_does_not_activate_anything() {
        let mut r = rig("outside", &["launchpad", "foot", "trash"], vec![]);
        r.open_menu(1);
        assert!(r.app.pointer_button(5.0, 5.0, Button::Left));
        assert!(r.app.menu.is_none());
        assert!(r.fx.borrow().launched.is_empty());
        // far outside the surface (pointer grab reports such coordinates)
        r.open_menu(1);
        r.app.pointer_button(-300.0, 4000.0, Button::Left);
        assert!(r.app.menu.is_none());
        // a left click on another icon only dismisses
        r.open_menu(1);
        let (x, y) = r.centre(0);
        r.app.pointer_button(x, y, Button::Left);
        r.app.pointer_release(x, y, Button::Left);
        assert!(r.app.menu.is_none() && r.fx.borrow().launched.is_empty());
    }

    #[test]
    fn right_clicking_another_icon_switches_menus() {
        let mut r = rig("switch", &["launchpad", "foot", "trash"], vec![]);
        r.open_menu(1);
        assert_eq!(r.menu_labels()[0], "Open");
        r.open_menu(2);
        assert!(r.menu_labels().contains(&"Empty Trash…".to_string()));
    }

    #[test]
    fn leaving_the_surface_closes_the_menu_without_a_grab() {
        let mut r = rig("leave", &["launchpad", "foot", "trash"], vec![]);
        r.open_menu(1);
        assert!(r.app.pointer_leave());
        assert!(r.app.menu.is_none());
    }

    #[test]
    fn keyboard_navigation_runs_the_entry() {
        let mut r = rig("kbd", &["launchpad", "foot", "trash"], vec![]);
        r.open_menu(1);
        assert!(r.app.nav_key(NavKey::Down));
        assert!(r.app.nav_key(NavKey::Enter));
        assert!(r.app.menu.is_none());
        assert_eq!(r.fx.borrow().launched, vec!["foot"]);
        assert!(!r.app.nav_key(NavKey::Down), "ignored without a menu");
    }

    #[test]
    fn window_entries_focus_that_window() {
        let mut r = rig("windows", &["launchpad", "foot", "trash"], vec![win(7, "foot", "one"), win(8, "foot", "two")]);
        r.open_menu(1);
        r.choose("two");
        assert_eq!(r.calls.lock().unwrap().activated, vec![8]);
        assert!(r.app.menu.is_none());
    }

    #[test]
    fn hide_and_quit_reach_every_window_of_the_app() {
        let mut r = rig("hideq", &["launchpad", "foot", "trash"], vec![win(7, "foot", "one"), win(8, "foot", "two"), win(9, "other", "x")]);
        r.open_menu(1);
        r.choose("Hide");
        r.open_menu(1);
        r.choose("Quit");
        let c = r.calls.lock().unwrap();
        assert_eq!(c.minimized, vec![7, 8]);
        assert_eq!(c.closed, vec![7, 8]);
    }

    #[test]
    fn remove_and_keep_in_dock_persist_atomically() {
        let mut r = rig("pins", &["launchpad", "foot", "kitty", "trash"], vec![]);
        r.open_menu(1);
        r.choose("Remove from Dock");
        assert_eq!(ids(&r.app), vec!["launchpad", "kitty", "trash"]);
        assert_eq!(r.saved(), vec!["launchpad", "kitty", "trash"]);
        assert_eq!(std::fs::read_dir(&r.dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains("tmp")).count(), 0);
    }

    #[test]
    fn empty_trash_asks_twice_and_runs_the_command() {
        let mut r = rig("trash", &["launchpad", "foot", "trash"], vec![]);
        r.open_menu(2);
        r.choose("Empty Trash…");
        assert!(r.app.menu.is_some(), "first click only arms it");
        assert!(r.fx.borrow().argv.is_empty());
        r.choose("Empty Trash…");
        assert!(r.app.menu.is_none());
        assert_eq!(r.fx.borrow().argv, vec![launch::empty_trash_argv()]);
    }

    #[test]
    fn external_edits_show_up_without_restart() {
        let mut r = rig("reload", &["launchpad", "foot", "trash"], vec![]);
        let f = r.dir.join("pinned.json");
        config::write_pinned(&f, &["launchpad".into(), "kitty".into(), "foot".into(), "trash".into()]).unwrap();
        let t = r.app.tick();
        assert!(t.redraw);
        assert_eq!(ids(&r.app), vec!["launchpad", "kitty", "foot", "trash"]);
        // a half-written / invalid file keeps the current dock
        std::fs::write(&f, "{ \"pinned\": [").unwrap();
        r.app.tick();
        assert_eq!(ids(&r.app), vec!["launchpad", "kitty", "foot", "trash"]);
        std::fs::write(&f, r#"{"pinned": ["a"]}"#).unwrap();
        r.app.tick();
        assert_eq!(ids(&r.app), vec!["a"]);
        // the next poll asks to be woken again soon, never busy
        let t = r.app.tick();
        assert!(t.next.is_some_and(|d| d >= Duration::from_millis(20)));
    }

    #[test]
    fn own_writes_do_not_trigger_a_reload() {
        let mut r = rig("own", &["launchpad", "foot", "trash"], vec![]);
        r.open_menu(1);
        r.choose("Remove from Dock");
        assert!(!r.app.poll_config(), "our own save must not be reloaded as an external edit");
    }

    #[test]
    fn dock_settings_reload_too() {
        let mut r = rig("settings", &["launchpad", "foot", "trash"], vec![]);
        std::fs::write(r.dir.join("settings.json"), r#"{"magnification": 2.0}"#).unwrap();
        assert!(r.app.tick().redraw);
        assert_eq!(r.app.dock.settings.magnification, 2.0);
    }

    #[test]
    fn running_apps_that_are_not_pinned_appear_before_the_trash() {
        let _g = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let dir = tmp_dir("transient");
        std::fs::write(dir.join("kitty.desktop"), "[Desktop Entry]\nName=Kitty\nExec=kitty\nType=Application\n").unwrap();
        std::env::set_var("XDG_DATA_HOME", &dir);
        std::fs::create_dir_all(dir.join("applications")).unwrap();
        std::fs::rename(dir.join("kitty.desktop"), dir.join("applications/kitty.desktop")).unwrap();
        let mut r = rig("transient2", &["launchpad", "foot", "trash"], vec![win(1, "kitty", "k"), win(2, "mystery", "m")]);
        assert_eq!(ids(&r.app), vec!["launchpad", "foot", "kitty", "trash"], "windows without an installed app stay out");
        assert!(!r.app.dock.items[2].pinned && r.app.dock.items[2].running);
        r.open_menu(2);
        assert!(r.menu_labels().contains(&"Keep in Dock".to_string()));
        r.choose("Keep in Dock");
        assert_eq!(r.saved(), vec!["launchpad", "foot", "kitty", "trash"]);
        assert!(r.app.dock.items[2].pinned);
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn dragging_a_pinned_icon_reorders_and_saves_once() {
        let mut r = rig("drag", &["launchpad", "a", "b", "c", "trash"], vec![]);
        let (x1, y) = r.centre(1);
        let (x3, _) = r.centre(3);
        r.app.pointer_button(x1, y, Button::Left);
        r.app.pointer_motion(x1 + 2.0, y);
        assert!(r.app.dock.drag.is_none(), "below the threshold it is still a click");
        r.app.pointer_motion((x1 + x3) / 2.0, y);
        assert!(r.app.dock.drag.is_some());
        r.app.pointer_motion(x3, y);
        r.app.pointer_release(x3, y, Button::Left);
        assert!(r.app.dock.drag.is_none());
        assert_eq!(ids(&r.app), vec!["launchpad", "b", "c", "a", "trash"]);
        assert_eq!(r.saved(), vec!["launchpad", "b", "c", "a", "trash"]);
        assert!(r.fx.borrow().launched.is_empty(), "a drag is not a click");
    }

    #[test]
    fn builtin_icons_cannot_be_dragged_or_displaced() {
        let mut r = rig("dragfix", &["launchpad", "a", "trash"], vec![]);
        let (x0, y) = r.centre(0);
        r.app.pointer_button(x0, y, Button::Left);
        r.app.pointer_motion(x0 + 40.0, y);
        assert!(r.app.dock.drag.is_none());
        r.app.pointer_release(x0 + 40.0, y, Button::Left);
        let (x1, _) = r.centre(1);
        let (x2, _) = r.centre(2);
        r.app.pointer_button(x1, y, Button::Left);
        r.app.pointer_motion(x2, y);
        r.app.pointer_motion(x2 + 5.0, y);
        r.app.pointer_release(x2 + 5.0, y, Button::Left);
        assert_eq!(ids(&r.app), vec!["launchpad", "a", "trash"], "cannot move past the trash");
        assert!(!r.dir.join("pinned.json").exists(), "nothing changed, nothing written");
    }

    #[test]
    fn show_in_finder_opens_the_desktop_files_folder() {
        let _g = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let dir = tmp_dir("finder");
        std::fs::create_dir_all(dir.join("applications")).unwrap();
        std::fs::write(dir.join("applications/zzdockapp.desktop"), "[Desktop Entry]\nName=Z\nExec=z\n").unwrap();
        std::env::set_var("XDG_DATA_HOME", &dir);
        let mut r = rig("finder2", &["launchpad", "zzdockapp", "trash"], vec![]);
        r.open_menu(1);
        r.choose("Show in Finder");
        std::env::remove_var("XDG_DATA_HOME");
        let argv = r.fx.borrow().argv.clone();
        assert_eq!(argv.len(), 1);
        assert_eq!(argv[0].last().unwrap(), &dir.join("applications").to_string_lossy().into_owned());
    }

    // ------------------------------------------------------------ Windows style

    fn active_win(id: u64, app: &str, title: &str) -> Window {
        Window { active: true, ..win(id, app, title) }
    }

    /// A rig in the Windows style on a 1280 px wide surface (no display needed: the scale is fixed).
    fn win_rig(name: &str, pinned: &[&str], windows: Vec<Window>) -> Rig {
        let mut r = rig(name, pinned, windows);
        r.app.pinned = pinned.iter().map(|s| s.to_string()).collect();
        r.app.scale = Some(1.0);
        r.set_settings(r#"{"style":"windows"}"#);
        r.app.surface = (1280, 48);
        r
    }

    impl Rig {
        fn set_settings(&mut self, json: &str) {
            std::fs::write(self.dir.join("settings.json"), json).unwrap();
            // the file watcher compares size + mtime: make sure a quick rewrite is noticed
            self.app.watch.as_mut().unwrap().settings_stamp = Stamp::of(&self.dir.join("nonexistent"));
            self.app.tick();
        }

        fn zone(&self, hit: crate::taskbar::Hit) -> (f32, f32) {
            let r = self.app.win_layout().unwrap().rect_of(hit).unwrap_or_else(|| panic!("no zone {hit:?}"));
            let (x, y, w, h) = self.app.win_to_surface(r);
            (x + w / 2.0, y + h / 2.0)
        }

        fn task_of(&self, app: &str) -> usize {
            self.app.win.as_ref().unwrap().buttons.iter().position(|b| b.app_id == app).unwrap_or_else(|| panic!("no task {app}"))
        }

        fn wclick(&mut self, hit: crate::taskbar::Hit, b: Button) {
            let (x, y) = self.zone(hit);
            self.app.pointer_button(x, y, b);
            self.app.pointer_release(x, y, b);
        }
    }

    #[test]
    fn the_macos_dock_sits_on_every_edge_with_upright_icons_hits_and_menus() {
        use crate::panel::Edge;
        for edge in Edge::ALL {
            let mut r = rig(&format!("mac-{}", edge.as_str()), &["launchpad", "a", "b", "trash"], vec![]);
            r.app.scale = Some(1.0);
            r.set_settings(&format!(r#"{{"style":"mac","edge":"{}"}}"#, edge.as_str()));
            let spec = r.app.surface_spec().unwrap();
            assert_eq!((spec.anchor, spec.stretch), (edge.anchor(), false), "{edge:?}");
            r.app.surface = r.app.dock.canvas_size();
            let (w, h) = (r.app.surface.0 as f32, r.app.surface.1 as f32);
            assert_eq!(edge.is_vertical(), h > w, "{edge:?}: a side dock is tall");
            // the panel hugs the canvas side that faces the screen edge
            let (px, py, pw, ph) = r.app.dock.panel_rect(w, h);
            let hugs = match edge {
                Edge::Bottom => (py + ph - h).abs() < 0.5,
                Edge::Top => py.abs() < 0.5,
                Edge::Left => px.abs() < 0.5,
                Edge::Right => (px + pw - w).abs() < 0.5,
            };
            assert!(hugs, "{edge:?}: panel {:?} on {w}x{h}", (px, py, pw, ph));
            // every icon is hit where it is drawn, in reading order along the edge
            let len = if edge.is_vertical() { h } else { w };
            let l = r.app.dock.layout(len);
            for (i, c) in l.cells.iter().enumerate() {
                let (x, y, _, _) = r.app.dock.map(w, h, c.x + c.w / 2.0, r.app.dock.panel_height() / 2.0, 0.0, 0.0);
                assert_eq!(r.app.dock.hit_test(w, h, x, y), Some(i), "{edge:?} cell {i}");
                assert!(r.app.input_rects()[0].x as f32 <= x && x <= (r.app.input_rects()[0].x + r.app.input_rects()[0].w) as f32, "{edge:?}: input covers the icon");
            }
            let mut pm = Pixmap::new(w as u32, h as u32).unwrap();
            r.app.dock.draw(&mut pm);
            // a right click opens the item's menu on the desktop side of the dock, on the surface, clear of the panel
            let c = l.cells[1];
            let (x, y, _, _) = r.app.dock.map(w, h, c.x + c.w / 2.0, r.app.dock.panel_height() / 2.0, 0.0, 0.0);
            r.app.pointer_button(x, y, Button::Right);
            assert!(r.app.menu.is_some(), "{edge:?}: menu");
            let (sw, sh) = r.app.size();
            r.app.surface = (sw, sh);
            let d = r.app.mac_menu(&r.app.menu.as_ref().unwrap().menu);
            let (px, py, pw, ph) = r.app.dock.panel_rect(sw as f32, sh as f32);
            assert!(d.x >= 0.0 && d.y >= 0.0 && d.x + d.w <= sw as f32 + 0.5 && d.y + d.h <= sh as f32 + 0.5, "{edge:?}: menu {:?} on {sw}x{sh}", (d.x, d.y, d.w, d.h));
            assert!(!(d.x < px + pw && px < d.x + d.w && d.y < py + ph && py < d.y + d.h), "{edge:?}: the menu covers the dock");
        }
    }

    #[test]
    fn style_switches_at_runtime_through_the_settings_file() {
        let mut r = rig("style", &["launchpad", "a", "trash"], vec![]);
        r.app.scale = Some(1.0);
        let mac = r.app.surface_spec().unwrap();
        assert_eq!((mac.anchor, mac.margin, mac.reserve, mac.stretch), (Anchor::Bottom, 13 + crate::theme::th().bar_insets().bottom, 71, false));
        assert!(!r.app.is_windows());
        r.set_settings(r#"{"style":"windows"}"#);
        assert!(r.app.is_windows());
        let spec = r.app.surface_spec().unwrap();
        assert_eq!((spec.anchor, spec.margin, spec.reserve, spec.stretch), (Anchor::Bottom, 0, 48, true));
        assert_eq!(r.app.size(), (0, 48), "monitor wide, 48 px high");
        r.set_settings(r#"{"style":"windows","windowsAlign":"left"}"#);
        assert_eq!(r.app.surface_spec().unwrap().reserve, 44, "the left aligned look is a little lower");
        r.set_settings(r#"{"style":"windows","windowsPosition":"top"}"#);
        assert_eq!(r.app.surface_spec().unwrap().anchor, Anchor::Top);
        r.set_settings(r#"{"style":"mac"}"#);
        assert!(!r.app.is_windows());
        assert_eq!(r.app.surface_spec().unwrap().reserve, 71);
        assert_eq!(r.app.size(), r.app.dock.canvas_size());
        // a half written file keeps the current look, a removed one resets it
        r.set_settings(r#"{"style":"windows"}"#);
        r.set_settings("{ not json");
        assert!(r.app.is_windows());
        std::fs::remove_file(r.dir.join("settings.json")).unwrap();
        r.app.tick();
        assert!(!r.app.is_windows());
    }

    /// The taskbar on `edge` of a 1280 x 900 monitor with every applet's data.
    fn edge_rig(name: &str, edge: crate::panel::Edge, applets: Option<crate::panel::AppletSet>) -> Rig {
        crate::menubar::DecBar::status_test_applets(applets);
        let mut r = win_rig(name, &["launchpad", "foot", "beta", "trash"], vec![active_win(1, "foot", "shell"), win(2, "mpv", "movie")]);
        r.set_settings(&format!(r#"{{"style":"windows","edge":"{}"}}"#, edge.as_str()));
        r.app.win.as_mut().unwrap().status.status_fill_for_tests();
        let (w, h) = r.app.size();
        r.app.surface = if edge.is_vertical() { (w.max(1), 900) } else { (1280, h.max(1)) };
        r
    }

    fn overlaps(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
        a.0 + 0.5 < b.0 + b.2 && b.0 + 0.5 < a.0 + a.2 && a.1 + 0.5 < b.1 + b.3 && b.1 + 0.5 < a.1 + a.3
    }

    #[test]
    fn taskbar_start_passes_its_actual_global_anchor_at_every_edge_and_scale() {
        use crate::panel::{frame::LaunchAnchor, AppletSet, Edge};
        use crate::taskbar::Hit;
        for edge in Edge::ALL {
            for scale in [1.0, 1.25, 2.0] {
                let mut r = edge_rig(&format!("start-anchor-{}-{scale}", edge.as_str()), edge, Some(AppletSet::all()));
                r.app.scale = Some(scale);
                r.app.win_apply_settings();
                // Floating strips and surfaces enlarged for a popup must still report the
                // Start cell's physical position, including a monitor left of the primary.
                r.app.win.as_mut().unwrap().m.float_gap = 8.0 * scale;
                let across = (r.app.win.as_ref().unwrap().m.bar_h + 8.0 * scale + 180.0).ceil() as u32;
                r.app.surface = if edge.is_vertical() { (across, 1100) } else { (1800, across) };
                r.app.surface_origin(-1900, 137);
                let (x, y) = r.zone(Hit::Start);
                let height = r.app.win.as_ref().unwrap().m.bar_h;
                r.wclick(Hit::Start, Button::Left);
                let calls = r.fx.borrow();
                assert!(calls.launched.is_empty(), "Start must use the positioned launcher command");
                let argv = calls.argv.last().expect("Start command");
                assert!(argv[0].ends_with("declaunch") && argv[1] == "toggle");
                let at = LaunchAnchor::parse(argv).expect("all anchor flags");
                assert_eq!(at.edge, edge);
                assert!((at.x - (x - 1900.0)).abs() <= 0.51, "{edge:?} {scale}: x {at:?}, click {x},{y}");
                assert!((at.y - (y + 137.0)).abs() <= 0.51, "{edge:?} {scale}: y {at:?}, click {x},{y}");
                assert!((at.thickness - height).abs() <= 0.51, "strip thickness is scaled exactly once");
            }
        }
    }

    #[test]
    fn the_taskbar_draws_every_zone_on_every_edge_and_opens_popups_towards_the_desktop() {
        use crate::panel::Edge;
        use crate::taskbar::Hit;
        for edge in Edge::ALL {
            let mut r = edge_rig(&format!("edge-{}", edge.as_str()), edge, Some(crate::panel::AppletSet::all()));
            let spec = r.app.surface_spec().unwrap();
            assert_eq!((spec.anchor, spec.reserve), (edge.anchor(), 48), "{edge:?}");
            let strip = r.app.win_strip_rect();
            let touches = match edge {
                Edge::Top => strip.1 == 0.0,
                Edge::Bottom => (strip.1 + strip.3 - r.app.surface.1 as f32).abs() < 1.0,
                Edge::Left => strip.0 == 0.0,
                Edge::Right => (strip.0 + strip.2 - r.app.surface.0 as f32).abs() < 1.0,
            };
            assert!(touches, "{edge:?}: strip {strip:?} on {:?}", r.app.surface);
            let l = r.app.win_layout().unwrap();
            let mut zones: Vec<(Hit, crate::taskbar::Rect)> = vec![(Hit::Start, l.start), (Hit::Clock, l.clock), (Hit::Desktop, l.desktop)];
            zones.extend(l.search.map(|s| (Hit::Search, s)));
            // a crowded strip scrolls its task row: only the buttons inside the visible span are zones
            zones.extend(l.tasks.iter().enumerate().filter(|(_, t)| t.x >= l.clip.0 - 0.5 && t.x + t.w <= l.clip.1 + 0.5).map(|(i, t)| (Hit::Task(i), *t)));
            zones.extend(l.tray.iter().map(|(k, t)| (Hit::Tray(*k), *t)));
            let surf: Vec<_> = zones.iter().map(|(h, z)| (*h, r.app.win_to_surface(*z))).collect();
            for (i, (h, z)) in surf.iter().enumerate() {
                assert!(z.0 >= strip.0 - 0.5 && z.1 >= strip.1 - 0.5 && z.0 + z.2 <= strip.0 + strip.2 + 0.5 && z.1 + z.3 <= strip.1 + strip.3 + 0.5, "{edge:?} {h:?} {z:?} outside {strip:?}");
                // the pointer finds every zone where it is drawn
                assert_eq!(r.app.win_hit(z.0 + z.2 / 2.0, z.1 + z.3 / 2.0), Some(*h), "{edge:?} {h:?}");
                for (h2, z2) in &surf[i + 1..] {
                    assert!(!overlaps(*z, *z2), "{edge:?}: {h:?} overlaps {h2:?}");
                }
            }
            // every popup applet of the tray opens beside the strip, on the surface
            for (kind, _) in l.tray.iter().filter(|(k, _)| crate::menubar::DecBar::status_has_popup(*k)).cloned().collect::<Vec<_>>() {
                r.app.win.as_mut().unwrap().status.status_close();
                let z = r.app.win_to_surface(r.app.win_layout().unwrap().rect_of(Hit::Tray(kind)).unwrap());
                let (cx, cy) = (z.0 + z.2 / 2.0, z.1 + z.3 / 2.0);
                r.app.pointer_button(cx, cy, Button::Left);
                r.app.pointer_release(cx, cy, Button::Left);
                let open = r.app.win.as_ref().unwrap().status.status_open_kind();
                assert_eq!(open, Some(kind), "{edge:?}: {kind:?} opens");
                let (w, h) = r.app.size();
                r.app.surface = if edge.is_vertical() { (w.max(r.app.surface.0), 900) } else { (1280, h.max(r.app.surface.1)) };
                let strip = r.app.win_strip_rect();
                let rect = r.app.win_input_rects().into_iter().skip(1).last().expect("popup input region");
                let p = (rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32);
                assert!(!overlaps(p, strip), "{edge:?} {kind:?}: popup {p:?} covers the strip {strip:?}");
                assert!(p.0 >= 0.0 && p.1 >= 0.0 && p.0 + p.2 <= r.app.surface.0 as f32 + 1.0 && p.1 + p.3 <= r.app.surface.1 as f32 + 1.0, "{edge:?} {kind:?}: {p:?} off {:?}", r.app.surface);
            }
        }
        crate::menubar::DecBar::status_test_applets(None);
    }

    #[test]
    fn a_short_side_taskbar_moves_what_does_not_fit_into_its_overflow_list() {
        use crate::menubar::Tray;
        use crate::panel::Edge;
        let mut r = edge_rig("short-side", Edge::Left, Some(crate::panel::AppletSet::all()));
        r.app.surface = (r.app.surface.0, 600);
        let l = r.app.win_layout().unwrap();
        let kinds: Vec<Tray> = l.tray.iter().map(|(k, _)| *k).collect();
        assert!(kinds.contains(&Tray::More) && kinds.contains(&Tray::Control), "{kinds:?}");
        let mut zones = vec![l.start, l.clock, l.desktop];
        zones.extend(l.search);
        zones.extend(l.tray.iter().map(|(_, t)| *t));
        for (i, a) in zones.iter().enumerate() {
            assert!(a.x >= 0.0 && a.x + a.w <= 600.5, "{a:?} off the strip");
            for b in &zones[i + 1..] {
                assert!(!(a.x + 0.5 < b.x + b.w && b.x + 0.5 < a.x + a.w), "{a:?} overlaps {b:?}");
            }
        }
        crate::menubar::DecBar::status_test_applets(None);
    }

    #[test]
    fn a_side_taskbar_preview_draws_the_whole_strip() {
        for edge in [crate::panel::Edge::Left, crate::panel::Edge::Right] {
            let settings = DockSettings { style: config::DockStyle::Windows, edge: Some(edge), ..DockSettings::default() };
            let pm = DockApp::preview_desktop_with(settings, 1920, 1080).unwrap();
            if let Some(dir) = std::env::var_os("PANEL_PREVIEW_OUT") {
                pm.save_png(std::path::Path::new(&dir).join(format!("taskbar-{}.png", edge.as_str()))).unwrap();
            }
            // the strip is painted down the whole edge, with content near its start (Start) and its end (clock)
            let x = if edge == crate::panel::Edge::Left { 24 } else { 1920 - 24 };
            let painted = |y: u32| pm.pixel(x, y).is_some_and(|p| p.alpha() > 0);
            for y in [5, 300, 540, 900, 1075] {
                assert!(painted(y), "{edge:?}: strip painted at y={y}");
            }
        }
    }

    #[test]
    fn decdock_focus_walks_the_taskbar_by_keyboard_on_every_edge() {
        use crate::menubar::Tray;
        use crate::panel::Edge;
        use crate::taskbar::Hit;
        for edge in [Edge::Bottom, Edge::Left, Edge::Right, Edge::Top] {
            let mut r = edge_rig(&format!("kb-{}", edge.as_str()), edge, Some(crate::panel::AppletSet::all()));
            assert!(!r.app.wants_keyboard());
            // the signal of `decdock --focus`, as the collector thread leaves it
            let signal: &'static crate::menubar::FocusSignal = Box::leak(Box::new((std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)), Default::default())));
            r.app.focus_signal = Some(signal);
            assert!(r.app.tick().redraw, "{edge:?}: keyboard mode starts with a redraw");
            assert!(r.app.wants_keyboard(), "{edge:?}");
            let items = r.app.win_focus_items();
            assert_eq!(items.first(), Some(&Hit::Start));
            assert_eq!(items.last(), Some(&Hit::Desktop));
            assert!(items.iter().any(|h| matches!(h, Hit::Task(_))) && items.contains(&Hit::Clock));
            // every zone is reached by the arrows of either axis and wraps around
            let mut seen = Vec::new();
            for _ in 0..items.len() {
                seen.push(items[r.app.win.as_ref().unwrap().kb.unwrap()]);
                assert!(r.app.nav_key(if edge.is_vertical() { NavKey::Down } else { NavKey::Right }));
            }
            assert_eq!(seen, items, "{edge:?}");
            assert_eq!(r.app.win.as_ref().unwrap().kb, Some(0));
            r.app.nav_key(NavKey::Up);
            assert_eq!(r.app.win.as_ref().unwrap().kb, Some(items.len() - 1));
            // the ring is drawn on the focused zone
            let mut pm = Pixmap::new(r.app.surface.0, r.app.surface.1).unwrap();
            r.app.draw(&mut pm);
            // Enter on the sound cell opens its popup with the first row focused; the arrows walk it; Escape closes
            // it, keyboard mode stays, a second Escape leaves
            let at = items.iter().position(|h| *h == Hit::Tray(Tray::Audio)).expect("sound");
            r.app.win.as_mut().unwrap().kb = Some(at);
            assert!(r.app.nav_key(NavKey::Enter));
            assert_eq!(r.app.win.as_ref().unwrap().status.status_open_kind(), Some(Tray::Audio), "{edge:?}");
            assert!(r.app.nav_key(NavKey::Down), "{edge:?}: the popup's rows take the arrows");
            assert!(r.app.key(Key::Escape));
            assert_eq!(r.app.win.as_ref().unwrap().status.status_open_kind(), None);
            assert!(r.app.wants_keyboard());
            assert!(r.app.key(Key::Escape) && !r.app.wants_keyboard(), "{edge:?}: Escape leaves keyboard mode");
            // Enter on Start opens the launcher, which takes the focus
            signal.0.store(true, std::sync::atomic::Ordering::SeqCst);
            r.app.tick();
            assert!(r.app.nav_key(NavKey::Enter));
            let spawned = r.fx.borrow();
            let argv = spawned.argv.last().expect("Start spawns the launcher");
            assert!(argv[0].ends_with("declaunch") && argv[1] == "toggle");
            assert_eq!(crate::panel::frame::LaunchAnchor::parse(argv).unwrap().edge, edge);
            drop(spawned);
            assert!(!r.app.wants_keyboard());
            // a pointer press also ends it
            signal.0.store(true, std::sync::atomic::Ordering::SeqCst);
            r.app.tick();
            let (x, y) = (r.app.surface.0 as f32 / 2.0, r.app.surface.1 as f32 / 2.0);
            r.app.pointer_button(x, y, Button::Left);
            assert!(r.app.win.as_ref().unwrap().kb.is_none());
        }
        crate::menubar::DecBar::status_test_applets(None);
    }

    #[test]
    fn a_window_that_only_moves_or_the_desktop_that_only_resizes_repaints_the_pager() {
        use crate::panel::{plasma_style::WorkspaceForm, Edge};
        use crate::wm::{Geometry, Workspace};
        let mut r = edge_rig("pager-geometry", Edge::Bottom, Some(crate::panel::AppletSet::all()));
        r.app.win.as_mut().unwrap().m.workspace_form = WorkspaceForm::Cells;
        let tracker = r.app.tracker.clone().expect("the rig's tracker");
        let spaces: Vec<Workspace> = (0..2).map(|i| Workspace { name: (i + 1).to_string(), active: i == 0, ..Default::default() }).collect();
        let windows = |x: i32, minimized: bool| {
            vec![
                Window { id: 1, app_id: "foot".into(), title: "shell".into(), active: true, desktop: Some(0), geometry: Some(Geometry { x, y: 0, w: 400, h: 300 }), ..Default::default() },
                Window {
                    id: 2,
                    app_id: "mpv".into(),
                    title: "movie".into(),
                    desktop: Some(0),
                    minimized,
                    geometry: (!minimized).then_some(Geometry { x: 600, y: 100, w: 300, h: 200 }),
                    ..Default::default()
                },
            ]
        };
        tracker.replace_all(windows(0, false), spaces.clone(), Some((1280, 900)));
        r.app.tick();
        assert!(!r.app.tick().redraw, "a tick with nothing new does not repaint");
        // the buttons, the workspaces and the focused app are all unchanged: only a window's rectangle moved
        tracker.replace_all(windows(120, false), spaces.clone(), Some((1280, 900)));
        assert!(r.app.tick().redraw, "a moved window must repaint the pager");
        assert!(!r.app.tick().redraw, "and only once");
        // only the desktop's size changed (a root-window resize with no _NET_DESKTOP_GEOMETRY)
        tracker.replace_all(windows(120, false), spaces.clone(), Some((1920, 1080)));
        assert!(r.app.tick().redraw, "a resized desktop reshapes the pager cells");
        assert!(!r.app.tick().redraw);
        // minimising drops the window's outline
        tracker.replace_all(windows(120, true), spaces.clone(), Some((1920, 1080)));
        assert!(r.app.tick().redraw);
        // a theme without pager cells never looks at the geometry, so a moved window costs no repaint there
        r.app.win.as_mut().unwrap().m.workspace_form = WorkspaceForm::Auto;
        tracker.replace_all(windows(300, true), spaces, Some((1920, 1080)));
        assert!(!r.app.tick().redraw, "no pager, no geometry repaint");
    }

    #[test]
    fn keyboard_pager_opens_the_list_instead_of_choosing_its_middle_workspace() {
        use crate::menubar::Tray;
        use crate::panel::{plasma_style::WorkspaceForm, Edge};
        use crate::taskbar::Hit;
        for edge in Edge::ALL {
            let mut r = edge_rig(&format!("kb-pager-{}", edge.as_str()), edge, Some(crate::panel::AppletSet::all()));
            r.app.win.as_mut().unwrap().m.workspace_form = WorkspaceForm::Cells;
            let items = r.app.win_focus_items();
            let pager = items.iter().position(|h| *h == Hit::Tray(Tray::Workspaces)).expect("workspace applet");
            r.app.win.as_mut().unwrap().kb = Some(pager);
            assert!(r.app.nav_key(NavKey::Enter));
            assert_eq!(r.app.win.as_ref().unwrap().status.status_open_kind(), Some(Tray::Workspaces), "{edge:?}: keyboard users can navigate the complete workspace list");
            assert!(r.app.key(Key::Escape));
        }
        crate::menubar::DecBar::status_test_applets(None);
    }

    #[test]
    fn a_side_taskbar_grows_for_quick_settings_and_keeps_the_panel_at_its_size() {
        use crate::panel::Edge;
        use crate::taskbar::Hit;
        for (edge, scale) in [(Edge::Left, 1.0), (Edge::Right, 1.0), (Edge::Left, 1.25), (Edge::Right, 2.0)] {
            let mut r = edge_rig(&format!("qs-{}-{scale}", edge.as_str()), edge, Some(crate::panel::AppletSet::all()));
            // the taskbar at a display scale: its metrics, the engine and the room all in surface pixels
            r.app.scale = Some(scale);
            r.app.win_init();
            r.app.win.as_mut().unwrap().status.status_fill_for_tests();
            r.app.surface = (r.app.size().0.max(1), 720);
            let z = r.app.win_to_surface(r.app.win_layout().unwrap().rect_of(Hit::Tray(crate::menubar::Tray::Control)).unwrap());
            let (x, y) = (z.0 + z.2 / 2.0, z.1 + z.3 / 2.0);
            r.app.pointer_button(x, y, Button::Left);
            r.app.pointer_release(x, y, Button::Left);
            assert!(r.app.win.as_ref().unwrap().status.status_open_kind().is_some(), "{edge:?}: quick settings opened");
            // the backend: the size asked for, bounded by a 1280 x 720 output; then a frame
            for _ in 0..2 {
                let (w, _) = r.app.size();
                r.app.resized(w.min(1280), 720);
                let mut pm = Pixmap::new(r.app.surface.0, r.app.surface.1).unwrap();
                r.app.draw(&mut pm);
            }
            let st = &r.app.win.as_ref().unwrap().status;
            let (shown, wanted) = (st.status_size().unwrap(), st.status_wanted_size().unwrap());
            assert!(r.app.surface.0 as f32 > 300.0 * scale, "{edge:?} {scale}: the surface grew for the panel ({:?})", r.app.surface);
            // shown at its natural width (the wanted size adds the margins around it)
            let natural = crate::controlcenter::panel_width(crate::theme::th().control_center.style.resolve(true)) * scale;
            assert!((shown.0 - natural).abs() < 1.0, "{edge:?} {scale}: the panel keeps its width when it fits ({shown:?}, natural {natural}, wanted {wanted:?})");
        }
        crate::menubar::DecBar::status_test_applets(None);
    }

    #[test]
    fn each_taskbar_applet_alone_is_its_own_zone() {
        use crate::panel::{AppletId as A, AppletSet, Edge};
        for edge in [Edge::Bottom, Edge::Left] {
            // Agents needs a Nerd Font and is not drawn where there is none.
            for id in A::all().into_iter().filter(|id| *id != A::Agents || crate::menubar::symbols::agents_available()) {
                let r = edge_rig(&format!("alone-{}-{}", edge.as_str(), id.key()), edge, Some(AppletSet::of(&[id])));
                let l = r.app.win_layout().unwrap();
                let shown = match id {
                    A::Launcher => l.start.w > 0.0,
                    A::Search => l.search.is_some(),
                    A::Tasks => !l.tasks.is_empty(),
                    A::Clock => l.clock.w > 0.0,
                    A::ShowDesktop => l.desktop.w > 0.0,
                    _ => !l.tray.is_empty(),
                };
                assert!(shown, "{edge:?}: {} alone", id.key());
                let others = (l.start.w > 0.0) as usize + l.search.is_some() as usize + (!l.tasks.is_empty()) as usize + (l.clock.w > 0.0) as usize + (l.desktop.w > 0.0) as usize + l.tray.len();
                assert_eq!(others, 1, "{edge:?}: {} alone shows only itself", id.key());
            }
        }
        crate::menubar::DecBar::status_test_applets(None);
    }

    #[test]
    fn taskbar_buttons_start_search_and_tasks_behave_like_windows() {
        use crate::taskbar::Hit;
        let mut r = win_rig("wtask", &["launchpad", "foot", "beta", "trash"], vec![active_win(1, "foot", "shell"), win(2, "mpv", "movie")]);
        let names: Vec<String> = r.app.win.as_ref().unwrap().buttons.iter().map(|b| b.app_id.clone()).collect();
        assert_eq!(&names[..2], ["foot", "beta"], "launchpad and trash are not task buttons");
        r.wclick(Hit::Start, Button::Left);
        r.wclick(Hit::Search, Button::Left);
        assert!(r.fx.borrow().launched.is_empty());
        assert_eq!(r.fx.borrow().argv.len(), 2, "Start and search use explicit commands");
        assert!(r.fx.borrow().argv[0][0].ends_with("declaunch"));
        assert!(crate::panel::frame::LaunchAnchor::parse(&r.fx.borrow().argv[0]).is_some());
        // the focused app's button minimises, a background app's button focuses, a closed one launches
        let foot = r.task_of("foot");
        r.wclick(Hit::Task(foot), Button::Left);
        assert_eq!(r.calls.lock().unwrap().minimized, vec![1]);
        let beta = r.task_of("beta");
        r.wclick(Hit::Task(beta), Button::Left);
        assert_eq!(r.fx.borrow().launched, vec!["beta"]);
        let mpv = r.task_of("mpv");
        r.wclick(Hit::Task(mpv), Button::Left);
        assert_eq!(r.calls.lock().unwrap().activated, vec![2]);
        // middle click always starts another instance
        r.wclick(Hit::Task(foot), Button::Middle);
        assert_eq!(r.fx.borrow().launched.last().unwrap(), "foot");
        assert!(r.fx.borrow().launched.iter().filter(|l| *l == "foot").count() == 1);
    }

    #[test]
    fn taskbar_context_menu_uses_pin_and_unpin_wording_and_saves() {
        use crate::taskbar::Hit;
        let mut r = win_rig("wmenu", &["launchpad", "foot", "beta", "trash"], vec![active_win(1, "foot", "shell")]);
        let foot = r.task_of("foot");
        r.wclick(Hit::Task(foot), Button::Right);
        let labels = r.menu_labels();
        assert!(labels.contains(&"Unpin from taskbar".to_string()) && labels.contains(&"Close window".to_string()), "{labels:?}");
        assert!(r.app.size().1 > 48, "the surface grows for the menu");
        assert_eq!(r.app.input_rects().len(), 2);
        r.app.key(Key::Escape);
        assert!(r.app.menu.is_none());
        r.app.surface = (1280, 48);
        let beta = r.task_of("beta");
        r.wclick(Hit::Task(beta), Button::Right);
        assert!(r.menu_labels().contains(&"Unpin from taskbar".to_string()));
        r.app.run_action("beta", Action::RemoveFromDock);
        assert_eq!(r.app.pinned, vec!["launchpad", "foot", "trash"]);
        assert!(r.dir.join("pinned.json").exists());
        r.app.run_action("beta", Action::CloseWindow(7));
        assert_eq!(r.calls.lock().unwrap().closed, vec![7]);
    }

    #[test]
    fn dock_menu_actions_write_the_settings_and_restyle_at_once() {
        let mut r = win_rig("wstyle", &["launchpad", "foot", "trash"], vec![]);
        r.app.run_action("dock", Action::SetAlign(config::WinAlign::Left));
        assert_eq!(r.app.dock.settings.windows_align, config::WinAlign::Left);
        assert_eq!(r.app.surface_spec().unwrap().reserve, 44);
        r.app.run_action("dock", Action::SetLabels(config::WinLabels::Always));
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(r.dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!((saved["style"].as_str(), saved["windowsAlign"].as_str(), saved["windowsLabels"].as_str()), (Some("windows"), Some("left"), Some("always")));
        r.app.run_action("dock", Action::SetStyle(config::DockStyle::Mac));
        assert!(!r.app.is_windows() && r.app.menu.is_none());
        assert_eq!(r.app.surface_spec().unwrap().reserve, 71);
        r.app.run_action("dock", Action::OpenSettings);
        assert!(r.fx.borrow().argv.iter().any(|a| a.last().map(String::as_str) == Some("desktop")));
    }

    #[test]
    fn right_click_on_the_empty_bar_or_mac_panel_opens_the_dock_menu() {
        let mut r = win_rig("wbg", &["launchpad", "foot", "trash"], vec![]);
        r.app.pointer_button(40.0, 20.0, Button::Right);
        let labels = r.menu_labels();
        assert!(labels.iter().any(|l| l == "Dock Style: macOS") && labels.iter().any(|l| l == "Taskbar Alignment: Left"), "{labels:?}");
        r.app.run_action("dock", Action::SetStyle(config::DockStyle::Mac));
        r.app.close_menu();
        r.set_settings(r#"{"style":"mac"}"#);
        r.app.surface = r.app.dock.canvas_size();
        let l = r.app.dock.layout(r.app.surface.0 as f32);
        r.app.pointer_button(l.panel_x + 2.0, r.app.surface.1 as f32 - 30.0, Button::Right);
        assert!(r.menu_labels().iter().any(|l| l == "Dock Style: Windows"), "{:?}", r.menu_labels());
    }

    #[test]
    fn dragging_a_pinned_button_reorders_and_saves_on_release() {
        use crate::taskbar::Hit;
        let mut r = win_rig("wdrag", &["launchpad", "a", "b", "c", "trash"], vec![]);
        let (ax, y) = r.zone(Hit::Task(r.task_of("a")));
        let (cx, _) = r.zone(Hit::Task(r.task_of("c")));
        r.app.pointer_button(ax, y, Button::Left);
        r.app.pointer_motion(ax + 4.0, y);
        assert!(r.app.win.as_ref().unwrap().drag.is_none(), "below the drag threshold");
        r.app.pointer_motion(ax + 20.0, y);
        assert!(r.app.win.as_ref().unwrap().drag.is_some());
        r.app.pointer_motion(cx, y);
        assert_eq!(r.app.pinned, vec!["launchpad", "b", "c", "a", "trash"]);
        assert!(!r.dir.join("pinned.json").exists(), "saved on release, not while dragging");
        r.app.pointer_release(cx, y, Button::Left);
        assert!(r.dir.join("pinned.json").exists());
        assert!(r.fx.borrow().launched.is_empty(), "a drag is not a click");
    }

    #[test]
    fn many_windows_shrink_then_scroll_and_clipped_buttons_cannot_be_hit() {
        use crate::taskbar::Hit;
        let ids: Vec<String> = (0..60).map(|i| format!("app{i}")).collect();
        let pinned: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut r = win_rig("wscroll", &pinned, vec![]);
        r.app.surface = (900, 48);
        let l = r.app.win_layout().unwrap();
        assert!(l.scroll_max > 0.0 && l.task_w == r.app.win.as_ref().unwrap().m.icon_min);
        let first = l.tasks[0].cx();
        let last = l.tasks[59].cx();
        assert!(r.app.win_hit(first, 20.0 + r.app.win_strip_top()).is_some());
        assert_eq!(r.app.win_hit(last, 20.0 + r.app.win_strip_top()), None);
        // the wheel works over any visible task button
        let over_a_task = |r: &Rig| {
            let l = r.app.win_layout().unwrap();
            l.tasks.iter().map(|t| t.cx()).find(|cx| *cx > l.clip.0 + 4.0 && *cx < l.clip.1 - 4.0).unwrap()
        };
        for _ in 0..200 {
            let x = over_a_task(&r);
            r.app.scroll(x, 20.0, 1.0);
        }
        let l2 = r.app.win_layout().unwrap();
        assert!((l2.scroll_max - r.app.win.as_ref().unwrap().scroll).abs() < 0.5, "clamped to the end");
        assert!(matches!(r.app.win_hit(l2.tasks[59].cx(), 20.0 + r.app.win_strip_top()), Some(Hit::Task(59))));
        let x = over_a_task(&r);
        r.app.scroll(x, 20.0, -1.0);
        assert!(r.app.win.as_ref().unwrap().scroll < l2.scroll_max);
    }

    #[test]
    fn show_desktop_minimises_everything_then_restores_it() {
        use crate::taskbar::Hit;
        let mut r = win_rig("wdesk", &["launchpad", "trash"], vec![active_win(1, "foot", "a"), win(2, "mpv", "b")]);
        r.wclick(Hit::Desktop, Button::Left);
        assert_eq!(r.calls.lock().unwrap().minimized, vec![1, 2]);
        // nothing has focus any more: the second click brings them back
        r.app.tracker.as_ref().unwrap().replace(vec![win(1, "foot", "a"), win(2, "mpv", "b")], vec![]);
        r.wclick(Hit::Desktop, Button::Left);
        assert_eq!(r.calls.lock().unwrap().activated, vec![1, 2]);
    }

    #[test]
    fn strip_input_follows_the_edge_and_titled_mode_makes_a_button_per_window() {
        let mut r = win_rig("wedge", &["launchpad", "foot", "trash"], vec![win(1, "foot", "one"), win(2, "foot", "two")]);
        r.app.surface = (1280, 300);
        let rects = r.app.input_rects();
        assert_eq!((rects[0].y, rects[0].h, rects[0].w), (252, 48, 1280), "bottom strip");
        r.set_settings(r#"{"style":"windows","windowsPosition":"top"}"#);
        assert_eq!(r.app.input_rects()[0].y, 0);
        r.set_settings(r#"{"style":"windows","windowsLabels":"always"}"#);
        let w = r.app.win.as_ref().unwrap();
        assert_eq!(w.buttons.iter().filter(|b| b.app_id == "foot").count(), 2, "one button per window");
        assert_eq!(w.buttons[1].title, "two");
    }
}
