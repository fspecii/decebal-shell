//! The Windows-style taskbar mode of [`DockApp`]: input handling, state and the glue between the pure
//! model in [`crate::taskbar`], the tray popups of the embedded [`DecBar`] status engine and the
//! backend. Everything that differs between the two dock styles lives here; `dock_app.rs` only
//! dispatches to these methods when `dock.style` is `"windows"`.

use super::*;
use crate::config::{DockStyle, WinTheme};
use crate::menubar::clock;
use crate::menubar::{DecBar, Tray};
use crate::taskbar::draw::{Colors, Frame, Painter};
use crate::taskbar::{self, click_action, Hit, Inputs, Layout, Metrics, TaskButton, TaskClick};

/// Space kept above a popup inside the surface.
const POP_TOP: f32 = 6.0;
/// Wheel notches move the task row by this many pixels (scaled).
const SCROLL_STEP: f32 = 48.0;

pub(super) struct WinState {
    pub painter: Painter,
    pub status: DecBar,
    pub status_started: bool,
    /// The status engine reads the dock's tracker.
    pub status_tracker: bool,
    pub m: Metrics,
    pub colors: Colors,
    pub buttons: Vec<TaskButton>,
    pub hover: Option<Hit>,
    /// Left button went down on this zone at this x.
    pub pressed: Option<(Hit, f32)>,
    pub scroll: f32,
    /// Pinned app being dragged to a new place and the pointer x.
    pub drag: Option<(String, f32)>,
    pub time: String,
    pub date: String,
    pub clock_w: f32,
    pub last_minute: Option<(u32, u32)>,
    /// Windows hidden by "show desktop", restored by the next click on the sliver.
    pub hidden: Vec<u64>,
    /// What the pager last drew (desktop size, windows' geometry and state): a geometry-only change of the tracker repaints through it.
    pub pager_seen: crate::menubar::PagerSignature,
    /// Keyboard mode (`decdock --focus`): the focused zone, an index into [`DockApp::win_focus_items`].
    pub kb: Option<usize>,
    /// Test hook (`DECDOCK_DEBUG_LAYOUT=file`): zone centres in surface coordinates after each draw.
    pub debug: Option<(PathBuf, String)>,
}

impl WinState {
    fn new(settings: &config::DockSettings, scale: f32) -> WinState {
        let painter = Painter::default();
        let m = Metrics::new(scale, settings.windows_align);
        let (time, date) = clock_lines();
        let clock_w = painter.measure(&time, 12.0 * m.scale).max(painter.measure(&date, 12.0 * m.scale));
        WinState {
            painter,
            status: DecBar::status_engine(),
            status_started: false,
            status_tracker: false,
            m,
            colors: Colors::dark(),
            buttons: Vec::new(),
            hover: None,
            pressed: None,
            scroll: 0.0,
            drag: None,
            time,
            date,
            clock_w,
            last_minute: None,
            hidden: Vec::new(),
            kb: None,
            pager_seen: Default::default(),
            debug: std::env::var_os("DECDOCK_DEBUG_LAYOUT").map(|p| (PathBuf::from(p), String::new())),
        }
    }
}

/// Time and date lines of the clock: `14:05` over `27/10/2026`.
pub fn clock_lines() -> (String, String) {
    let time = clock::now().map(|t| format!("{:02}:{:02}", t.hour, t.min)).unwrap_or_default();
    let date = clock::today().map(|(y, m, d)| format!("{d:02}/{:02}/{y}", m + 1)).unwrap_or_default();
    (time, date)
}

/// The short date a side taskbar fits under the time: `27/10`.
pub fn short_date(date: &str) -> String {
    date.rsplit_once('/').map_or_else(|| date.to_string(), |(dm, _)| dm.to_string())
}

/// What opens when the search entry is clicked: `DECEBAL_SPOTLIGHT` or decspot.
pub fn search_argv() -> Vec<String> {
    match std::env::var("DECEBAL_SPOTLIGHT") {
        Ok(cmd) if !cmd.is_empty() => launch::exec_argv(&cmd),
        _ => vec![launch::sibling_or_path("decspot")],
    }
}

/// The taskbar's screen edge: the user's `edge`, the old `windowsPosition`, the theme's `[dock] position` (see
/// [`crate::panel`]).
pub fn taskbar_edge(s: &config::DockSettings) -> crate::panel::Edge {
    crate::theme::th().taskbar_panel(s).edge
}

/// Surface spec of the taskbar for these settings and metrics.
pub fn taskbar_spec(s: &config::DockSettings, m: &Metrics) -> SurfaceSpec {
    SurfaceSpec { anchor: taskbar_edge(s).anchor(), margin: 0, reserve: (m.bar_h + m.float_gap).ceil() as i32, stretch: true, namespace: "decebal-dock" }
}

/// y of an open menu's bottom edge: above a bottom strip, or below a top one.
fn menu_bottom_at(top: bool, bar_h: f32, surface_h: f32, menu_h: f32) -> f32 {
    if top {
        bar_h + MENU_GAP + menu_h
    } else {
        surface_h - bar_h - MENU_GAP
    }
}

impl DockApp {
    pub(super) fn is_windows(&self) -> bool {
        self.dock.settings.style == DockStyle::Windows
    }

    fn win_panel_edge(&self) -> crate::panel::Edge {
        taskbar_edge(&self.dock.settings)
    }

    /// For the gallery: `Some(right)` for a side taskbar, `None` for a top / bottom one.
    pub(super) fn win_vertical_edge(&self) -> Option<bool> {
        self.win_vertical().then(|| self.on_right())
    }

    pub(super) fn win_panel_edge_top(&self) -> bool {
        self.on_top()
    }

    fn on_top(&self) -> bool {
        self.win_panel_edge() == crate::panel::Edge::Top
    }

    /// The taskbar runs down a side edge (Windows 10 allowed it on the left and the right).
    pub(super) fn win_vertical(&self) -> bool {
        self.win_panel_edge().is_vertical()
    }

    fn on_right(&self) -> bool {
        self.win_panel_edge() == crate::panel::Edge::Right
    }

    /// x of the strip's left edge in the surface (a right strip sits at the surface's right end).
    fn win_strip_x(&self) -> f32 {
        if self.on_right() {
            (self.surface.0 as f32 - self.win_bar_h() - self.win_gap()).max(0.0)
        } else {
            self.win_gap()
        }
    }

    /// Where the strip's layout starts along the strip: the floating gap, or 0 for a flush strip.
    fn win_along0(&self) -> f32 {
        if self.win_vertical() {
            self.win_strip_top()
        } else {
            self.win_strip_x()
        }
    }

    /// The floating gap (`[taskbar] float_gap`): the strip keeps this far from the screen edge and from both its ends.
    fn win_gap(&self) -> f32 {
        self.win.as_ref().map_or(0.0, |w| w.m.float_gap)
    }

    /// The strip on the surface: (x, y, w, h).
    pub(super) fn win_strip_rect(&self) -> (f32, f32, f32, f32) {
        if self.win_vertical() {
            (self.win_strip_x(), self.win_strip_top(), self.win_bar_h(), self.win_width())
        } else {
            (self.win_strip_x(), self.win_strip_top(), self.win_width(), self.win_bar_h())
        }
    }

    /// A surface point in the layout's frame: (along the strip, across it from the strip's own origin).
    fn win_local(&self, x: f32, y: f32) -> (f32, f32) {
        if self.win_vertical() {
            (y - self.win_strip_top(), x - self.win_strip_x())
        } else {
            (x - self.win_strip_x(), y - self.win_strip_top())
        }
    }

    /// The coordinate along the strip of a surface point.
    fn win_along(&self, x: f32, y: f32) -> f32 {
        self.win_local(x, y).0
    }

    /// A rectangle of the layout on the surface.
    pub(super) fn win_to_surface(&self, r: crate::taskbar::Rect) -> (f32, f32, f32, f32) {
        if self.win_vertical() {
            (self.win_strip_x() + r.y, self.win_strip_top() + r.x, r.h, r.w)
        } else {
            (self.win_strip_x() + r.x, self.win_strip_top() + r.y, r.w, r.h)
        }
    }

    /// Creates the taskbar state the first time the Windows style is needed and starts the status engine.
    pub(super) fn win_init(&mut self) {
        if self.win.is_none() {
            let scale = *self.scale.get_or_insert_with(crate::backend::ui_scale);
            self.win = Some(Box::new(WinState::new(&self.dock.settings, scale)));
            if let Some(win) = self.win.as_mut() {
                win.status.status_surface_origin(self.surface_origin.0, self.surface_origin.1);
            }
        }
        // the workspace, window and application-menu applets read the dock's own window tracker
        if let (Some(w), Some(t)) = (self.win.as_mut(), self.tracker.clone()) {
            if !w.status_tracker {
                w.status_tracker = true;
                w.status.status_set_active(Box::new(crate::menubar::active::TrackerActive(t)));
            }
        }
        self.win_apply_settings();
        if let (Some(w), Some(waker)) = (self.win.as_mut(), self.waker.clone()) {
            if !w.status_started {
                w.status_started = true;
                w.status.status_start(waker);
            }
        }
        self.win_sync_buttons();
    }

    /// Recomputes metrics and colours after a settings change.
    pub(super) fn win_apply_settings(&mut self) {
        let s = self.dock.settings.clone();
        let scale = *self.scale.get_or_insert(1.0);
        let Some(w) = self.win.as_mut() else { return };
        w.m = Metrics::new(scale, s.windows_align).themed(&crate::theme::th());
        // the tray's popups, calendar and quick settings at the taskbar's scale
        w.status.status_set_scale(w.m.scale);
        // auto follows the active theme's mode; dark/light force that mode
        w.colors = match s.windows_theme {
            WinTheme::Auto => Colors::from_theme(&crate::theme::th()),
            WinTheme::Dark => Colors::pick(false),
            WinTheme::Light => Colors::pick(true),
        };
        let px = 12.0 * w.m.scale;
        w.clock_w = w.painter.measure(&w.time, px).max(w.painter.measure(&w.date, px));
        w.scroll = 0.0;
    }

    /// The dock's style options changed (file edit, menu): rebuild what depends on them.
    pub(super) fn style_changed(&mut self, old: &config::DockSettings) {
        let new = &self.dock.settings;
        self.menu = None;
        self.press = None;
        self.dock.hover_x = None;
        self.dock.drag = None;
        if let Some(w) = self.win.as_mut() {
            w.status.status_close();
            w.hover = None;
            w.pressed = None;
            w.drag = None;
        }
        if new.style == DockStyle::Windows {
            self.win_init();
        } else if old.style == DockStyle::Windows {
            self.dock.hover_x = None;
        }
    }

    fn win_bar_h(&self) -> f32 {
        self.win.as_ref().map_or(48.0, |w| w.m.bar_h)
    }

    /// y of the strip's top edge in the surface.
    pub(super) fn win_strip_top(&self) -> f32 {
        if self.on_top() || self.win_vertical() {
            self.win_gap()
        } else {
            (self.surface.1 as f32 - self.win_bar_h() - self.win_gap()).max(0.0)
        }
    }

    /// Strip edge a popup hangs from (the side facing the desktop): a y for a top/bottom strip, an x for a side one.
    fn win_edge(&self) -> f32 {
        if self.win_vertical() {
            if self.on_right() {
                self.win_strip_x()
            } else {
                self.win_bar_h() + self.win_gap()
            }
        } else if self.on_top() {
            self.win_bar_h() + self.win_gap()
        } else {
            self.win_strip_top()
        }
    }

    /// Length of the strip (its layout's width).
    fn win_width(&self) -> f32 {
        let len = if self.win_vertical() { self.surface.1 } else { self.surface.0 };
        (len as f32 - 2.0 * self.win_gap()).max(1.0)
    }

    /// The dock setting's search form, except that a theme's `[taskbar] search = "button"` turns the default box into the button.
    fn win_search_form(&self) -> config::WinSearch {
        let form = self.dock.settings.windows_search;
        let themed = self.win.as_ref().is_some_and(|w| w.m.search_form == crate::panel::plasma_style::SearchForm::Button);
        if themed && form == config::WinSearch::Box { config::WinSearch::Button } else { form }
    }

    pub(super) fn win_layout(&self) -> Option<Layout> {
        use crate::panel::AppletId as A;
        let w = self.win.as_ref()?;
        let s = &self.dock.settings;
        let vertical = self.win_vertical();
        // a side strip has icon-only buttons, a search button and a stacked clock
        let search = match (w.status.status_shows(A::Search), vertical, self.win_search_form()) {
            (false, _, _) => config::WinSearch::None,
            (true, true, config::WinSearch::Box) => config::WinSearch::Button,
            (true, _, other) => other,
        };
        let labels = if vertical { config::WinLabels::Never } else { s.windows_labels };
        let buttons = if w.status.status_shows(A::Tasks) { w.buttons.len() } else { 0 };
        let clock_w = if vertical { 2.0 * 15.0 * w.m.scale - 2.0 * w.m.clock_pad + 8.0 * w.m.scale } else { w.clock_w };
        let builtin_count = w.status.status_kinds_with_limit(0).len();
        // a theme with `[taskbar] workspaces = "cells"` draws one cell per workspace: the pager takes more room than one tray cell
        let pager = if w.m.workspace_form == crate::panel::plasma_style::WorkspaceForm::Cells && w.status.status_shows(A::Workspaces) { w.status.status_workspace_count() } else { 0 };
        let pager_shape = crate::panel::plasma_style::pager_shape(w.status.status_desktop_size());
        let pager_extra = if pager > 0 { (crate::panel::plasma_style::pager_extent(pager, w.m.icon, vertical, pager_shape) + 2.0 * (4.0 * w.m.scale).round() - w.m.tray_w).max(0.0) } else { 0.0 };
        let slots = taskbar::indicator_capacity(&w.m, self.win_width(), search, buttons, builtin_count, clock_w, pager_extra);
        // how many tray cells fit beside Start, search, one task button and the clock
        let cells = taskbar::indicator_capacity(&w.m, self.win_width(), search, buttons, 0, clock_w, pager_extra);
        let kinds = w.status.status_kinds_fit(cells, slots);
        Some(taskbar::layout(
            &w.m,
            &Inputs {
                width: self.win_width(),
                align: s.windows_align,
                labels,
                search,
                buttons,
                tray: &kinds,
                clock_text_w: clock_w,
                scroll: w.scroll,
                start: w.status.status_shows(A::Launcher),
                clock: w.status.status_shows(A::Clock),
                desktop: w.status.status_shows(A::ShowDesktop),
                pager,
                pager_shape,
                vertical,
            },
        ))
    }

    /// Rebuilds the task buttons from the pinned list and the open windows; true when they changed.
    pub(super) fn win_sync_buttons(&mut self) -> bool {
        let Some(w) = self.win.as_mut() else { return false };
        let windows = self.tracker.as_ref().map(|t| t.snapshot().windows).unwrap_or_default();
        let desktops = &mut self.desktops;
        let mut resolve = |app: &str| desktops.entry(app.to_lowercase()).or_insert_with(|| launch::resolve_desktop_id(app)).clone();
        let buttons = taskbar::build_buttons(&self.pinned, &windows, self.dock.settings.windows_labels, &mut resolve);
        if buttons == w.buttons {
            return false;
        }
        w.buttons = buttons;
        self.press = None;
        true
    }

    // ------------------------------------------------------------------ size

    pub(super) fn win_size(&self) -> (u32, u32) {
        let bar = self.win_bar_h() + self.win_gap();
        let vertical = self.win_vertical();
        let mut extra = 0.0f32;
        if let Some(m) = &self.menu {
            let (mw, mh) = m.menu.size();
            extra = extra.max(if vertical { mw } else { mh } + MENU_GAP + POP_TOP);
        }
        // the popup's wanted size: the backend keeps the surface on the output and the panel is fitted to what it got
        if let Some((pw, ph)) = self.win.as_ref().and_then(|w| w.status.status_wanted_size()) {
            extra = extra.max(if vertical { pw } else { ph } + crate::menubar::STATUS_GAP + POP_TOP);
        }
        // a side strip is as tall as the monitor and grows sideways; a top / bottom one as wide and grows up / down
        if vertical {
            ((bar + extra).ceil() as u32, 0)
        } else {
            (0, (bar + extra).ceil() as u32)
        }
    }

    pub(super) fn win_spec(&self) -> Option<SurfaceSpec> {
        let m = self.win.as_ref().map(|w| w.m).unwrap_or_else(|| Metrics::new(self.scale.unwrap_or(1.0), self.dock.settings.windows_align).themed(&crate::theme::th()));
        Some(taskbar_spec(&self.dock.settings, &m))
    }

    fn win_menu_bottom(&self, menu_h: f32) -> f32 {
        menu_bottom_at(self.on_top(), self.win_bar_h() + self.win_gap(), self.surface.1 as f32, menu_h)
    }

    /// Where the open context menu is drawn: against the strip on the desktop side; beside a side strip, centred on
    /// the item it belongs to along the strip.
    pub(super) fn win_menu_dropdown(&self, m: &DockMenu) -> crate::menubar::menus::Dropdown {
        if !self.win_vertical() {
            return m.placed(self.surface.0 as f32, self.win_menu_bottom(m.size().1));
        }
        let mut d = m.placed(f32::MAX / 8.0, 0.0);
        let len = self.surface.1 as f32;
        d.x = if self.on_right() { self.win_strip_x() - MENU_GAP - d.w } else { self.win_bar_h() + self.win_gap() + MENU_GAP };
        let y = (m.anchor_x - d.h / 2.0).clamp(4.0, (len - d.h - 4.0).max(4.0));
        d.shift_y(y - d.y);
        d
    }

    /// Places the open status popup against the strip.
    fn win_place_popup(&mut self) {
        let (edge, up, width) = (self.win_edge(), if self.win_vertical() { self.on_right() } else { !self.on_top() }, self.win_width());
        let vertical = self.win_vertical();
        let along0 = self.win_along0();
        let indicator_anchor = self.win.as_ref().and_then(|w| w.status.status_open_kind()).and_then(|kind| {
            if !matches!(kind, Tray::Indicator(_) | Tray::Indicators) {
                return None;
            }
            let layout = self.win_layout()?;
            layout.rect_of(Hit::Tray(kind)).or_else(|| layout.rect_of(Hit::Tray(Tray::Indicators))).map(|r| r.cx() + along0)
        });
        let surface = (self.surface.0 as f32, self.surface.1 as f32);
        if let Some(w) = self.win.as_mut() {
            if let Some(x) = indicator_anchor {
                w.status.status_indicator_anchor(x);
            }
            // a panel opening down from a top taskbar has the rest of the surface (the backend keeps it on the output)
            w.status.status_set_surface_size(surface.0, surface.1);
            if vertical {
                w.status.status_place_side(edge, up, width);
            } else {
                w.status.status_place(edge, up, width);
            }
        }
    }

    pub(super) fn win_input_rects(&self) -> Vec<InputRect> {
        let (len, bar) = (self.win_width() as i32, self.win_bar_h());
        let top = self.win_strip_top();
        let mut rects = if self.win_vertical() {
            vec![InputRect { x: self.win_strip_x() as i32, y: top as i32, w: bar as i32, h: len }]
        } else {
            vec![InputRect { x: self.win_strip_x() as i32, y: top as i32, w: len, h: bar as i32 }]
        };
        if let Some(m) = &self.menu {
            let d = self.win_menu_dropdown(&m.menu);
            rects.push(InputRect { x: d.x.floor() as i32, y: d.y.floor() as i32, w: d.w.ceil() as i32 + 1, h: d.h.ceil() as i32 });
        }
        let up = if self.win_vertical() { self.on_right() } else { !self.on_top() };
        if let Some(r) = self.win.as_ref().and_then(|w| w.status.status_rect_at(self.win_edge(), up, self.win_width())) {
            rects.push(r);
        }
        rects
    }

    // ------------------------------------------------------------------ draw

    pub(super) fn win_draw(&mut self, pm: &mut Pixmap) {
        crate::gfx::clear(pm);
        self.win_place_popup();
        let Some(layout) = self.win_layout() else { return };
        let y0 = self.win_strip_top();
        let x0 = self.win_strip_x();
        let top = self.on_top();
        let (vertical, right) = (self.win_vertical(), self.on_right());
        let search_form = self.win_search_form();
        let painted = if vertical { layout.transposed() } else { layout.clone() };
        let to_surface = |r: crate::taskbar::Rect| if vertical { (x0 + r.y + r.h / 2.0, y0 + r.x + r.w / 2.0) } else { (x0 + r.cx(), y0 + r.cy()) };
        // the theme's dock translucency; the Transparency setting shifts it around its 0.4 default
        let alpha = (crate::theme::th().dock_alpha - (self.dock.settings.transparency - 0.4) * 0.25).clamp(0.0, 1.0);
        let Some(w) = self.win.as_mut() else { return };
        let snap = w.status.status_state();
        let extras = w.status.status_extras();
        let short = short_date(&w.date);
        let layout_name = w.status.status_layout();
        let weather = w.status.status_weather_symbol();
        let open = w.status.status_open_kind().map(|k| if k == Tray::Clock { Hit::Clock } else { Hit::Tray(k) });
        let hover = w.hover;
        let pressed = w.pressed.map(|p| p.0);
        let drag = w.drag.as_ref().and_then(|(id, x)| w.buttons.iter().position(|b| &b.app_id == id).map(|i| (i, *x)));
        let frame = Frame {
            m: &w.m,
            c: &w.colors,
            layout: &painted,
            buttons: &w.buttons,
            hover,
            pressed,
            open,
            tray: &snap,
            weather,
            layout_name: layout_name.as_deref(),
            time: &w.time,
            date: if vertical { &short } else { &w.date },
            show_search_label: search_form == config::WinSearch::Box && !vertical,
            alpha,
            top,
            drag,
            vertical,
            right,
            extras: &extras,
        };
        if let Some(strip) = w.painter.draw_strip(&frame) {
            pm.draw_pixmap(x0 as i32, y0 as i32, strip.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
        for (kind, r) in &layout.tray {
            if let Tray::Indicator(key) = kind {
                let (cx, cy) = to_surface(*r);
                w.status.status_draw_indicator(pm, *key, cx, cy, 18.0 * w.m.scale);
            }
        }
        if let Some(r) = w.kb.and_then(|i| focus_rect(&layout, &w.buttons, i)) {
            let (x, y, rw, rh) = if vertical { (x0 + r.y, y0 + r.x, r.h, r.w) } else { (x0 + r.x, y0 + r.y, r.w, r.h) };
            let th = crate::theme::th();
            crate::gfx::stroke_rounded(pm, x + 1.0, y + 1.0, (rw - 2.0).max(1.0), (rh - 2.0).max(1.0), (4.0 * w.m.scale).min(rw.min(rh) / 2.0), th.accent.color(1.0), 2.0 * w.m.scale);
        }
        w.status.status_draw(pm);
        let indicator_rects: Vec<_> = layout
            .tray
            .iter()
            .map(|(kind, r)| {
                let (cx, cy) = to_surface(*r);
                let (w_, h_) = if vertical { (r.h, r.w) } else { (r.w, r.h) };
                (*kind, (cx - w_ / 2.0, cy - h_ / 2.0, w_, h_))
            })
            .collect();
        w.status.status_debug_indicators(&indicator_rects);
        self.win_dump(&layout, y0);
        let placed = self.menu.as_ref().map(|m| self.win_menu_dropdown(&m.menu));
        if let (Some(m), Some(d)) = (&mut self.menu, placed) {
            if let Some(f) = &mut m.fonts {
                m.menu.draw(pm, &d, f);
                f.trim();
            }
        }
    }

    /// `pagercell <index> <along>` lines: where each pager cell's centre is along the strip (the layout's own axis), for the native tests.
    fn win_dump_pager(&self, l: &Layout) -> String {
        let Some(w) = self.win.as_ref().filter(|w| w.m.workspace_form == crate::panel::plasma_style::WorkspaceForm::Cells) else { return String::new() };
        let Some(r) = l.rect_of(Hit::Tray(Tray::Workspaces)) else { return String::new() };
        let vertical = self.win_vertical();
        let shape = crate::panel::plasma_style::pager_shape(w.status.status_desktop_size());
        let cells = crate::panel::plasma_style::pager_cells(w.status.status_workspace_count(), shape.0, shape.1, r.w, w.m.icon, vertical);
        cells.iter().enumerate().map(|(i, c)| format!("pagercell {i} {}\n", self.win_along0() + r.x + if vertical { c.y + c.h / 2.0 } else { c.x + c.w / 2.0 })).collect()
    }

    fn win_dump(&mut self, l: &Layout, y0: f32) {
        let pager_lines = self.win_dump_pager(l);
        let vertical = self.win_vertical();
        let edge = self.win_panel_edge().as_str();
        let x0 = self.win_strip_x();
        // positions along the strip are surface coordinates: the layout starts after the floating gap
        let a0 = self.win_along0();
        let Some(w) = self.win.as_mut() else { return };
        let Some((path, last)) = w.debug.as_mut() else { return };
        if vertical {
            // zone centres as surface points: the strip's middle across, the layout's position along
            let cx = x0 + w.m.bar_h / 2.0;
            let mut t = format!("edge {edge}\nbar {} {x0} {}\nstart {cx} {}\n", l.width, w.m.bar_h, l.start.cx() + a0);
            if let Some(s) = l.search {
                t += &format!("search {cx} {}\n", s.cx() + a0);
            }
            for (b, r) in w.buttons.iter().zip(&l.tasks) {
                t += &format!("task {} {cx} {} {}\n", b.app_id, r.cx() + a0, u8::from(b.active));
            }
            for (k, r) in &l.tray {
                t += &format!("tray {k:?} {cx} {}\n", r.cx() + a0);
            }
            t += &format!("clock {cx} {}\ndesktop {cx} {}\nstrut {}\n", l.clock.cx() + a0, l.desktop.cx() + a0, w.m.bar_h);
            t += &pager_lines;
            if let Some((x, y, pw, ph)) = w.status.status_rect().map(|r| (r.x, r.y, r.w, r.h)) {
                t += &format!("popup {x} {y} {pw} {ph}\n");
            }
            if *last != t {
                let _ = std::fs::write(&*path, &t);
                *last = t;
            }
            return;
        }
        let cy = y0 + w.m.bar_h / 2.0;
        let mut t = format!("bar {} {} {}\nstart {} {cy}\n", l.width, y0, w.m.bar_h, l.start.cx() + a0);
        if let Some(s) = l.search {
            t += &format!("search {} {cy}\n", s.cx() + a0);
        }
        for (b, r) in w.buttons.iter().zip(&l.tasks) {
            t += &format!("task {} {} {cy} {}\n", b.app_id, r.cx() + a0, u8::from(b.active));
        }
        for (k, r) in &l.tray {
            t += &format!("tray {k:?} {} {cy}\n", r.cx() + a0);
        }
        t += &format!("clock {} {cy}\ndesktop {} {cy}\nstrut {}\n", l.clock.cx() + a0, l.desktop.cx() + a0, w.m.bar_h);
        t += &pager_lines;
        if let Some((x, y, pw, ph)) = w.status.status_rect().map(|r| (r.x, r.y, r.w, r.h)) {
            t += &format!("popup {x} {y} {pw} {ph}\n");
        }
        if *last != t {
            let _ = std::fs::write(&*path, &t);
            *last = t;
        }
    }

    // ----------------------------------------------------------------- tick

    pub(super) fn win_tick(&mut self) -> Tick {
        // Common tick has already polled settings and refreshed theme-dependent caches.
        let mut changed = false;
        if self.win.is_none() {
            self.win_init();
            changed = true;
        }
        changed |= self.win_sync_buttons();
        let Some(w) = self.win.as_mut() else { return Tick::default() };
        changed |= w.status.status_tick();
        // The pager draws windows from tracker geometry, which `win_sync_buttons` (buttons) and `status_tick` (workspaces, focused app) do
        // not compare: a window that only moved or resized wakes this tick, so look at exactly what the pager draws. It is the
        // tracker's snapshot, never a query of the display.
        if w.m.workspace_form == crate::panel::plasma_style::WorkspaceForm::Cells && w.status.status_shows(crate::panel::AppletId::Workspaces) {
            let seen = w.status.status_pager_signature();
            if seen != w.pager_seen {
                w.pager_seen = seen;
                changed = true;
            }
        }
        let mut next = Duration::from_secs(60);
        if let Some(tm) = clock::now() {
            if w.last_minute != Some((tm.hour, tm.min)) {
                w.last_minute = Some((tm.hour, tm.min));
                let (time, date) = clock_lines();
                if (time.clone(), date.clone()) != (w.time.clone(), w.date.clone()) {
                    w.time = time;
                    w.date = date;
                    let px = 12.0 * w.m.scale;
                    w.clock_w = w.painter.measure(&w.time, px).max(w.painter.measure(&w.date, px));
                    changed = true;
                }
            }
            next = clock::until_next_minute(&tm).min(next);
        }
        if let Some(f) = &w.painter.fonts {
            f.trim();
        }
        if let Some(wt) = &self.watch {
            next = next.min(wt.every.saturating_sub(wt.last_poll.elapsed()).max(Duration::from_millis(20)));
        }
        Tick { redraw: changed, next: Some(next) }
    }

    // ----------------------------------------------------------------- input

    pub(super) fn win_hit(&self, x: f32, y: f32) -> Option<Hit> {
        let l = self.win_layout()?;
        let (a, c) = self.win_local(x, y);
        l.hit(a, c, self.win_bar_h())
    }

    pub(super) fn win_motion(&mut self, x: f32, y: f32) -> bool {
        // A menu or popup being open takes precedence.
        let placed = self.menu.as_ref().map(|m| self.win_menu_dropdown(&m.menu));
        let along = self.win_along(x, y);
        if let (Some(m), Some(d)) = (&mut self.menu, placed) {
            if d.contains(x, y) {
                return m.menu.pointer_moved(&d, x, y);
            }
            return m.menu.pointer_moved(&d, -1.0, -1.0);
        }
        self.win_place_popup();
        let hit = self.win_hit(x, y);
        let Some(w) = self.win.as_mut() else { return false };
        let mut redraw = w.status.status_motion(x, y);
        if let Some((id, _)) = &w.drag {
            let id = id.clone();
            w.drag = Some((id, along));
            self.win_drag_over(along);
            return true;
        }
        if let Some((Hit::Task(i), px)) = w.pressed {
            let moved = (along - px).abs() > DRAG_THRESHOLD;
            let app = w.buttons.get(i).map(|b| b.app_id.clone());
            if let (true, Some(app)) = (moved, app) {
                if self.pinned.contains(&app) {
                    if let Some(w) = self.win.as_mut() {
                        w.drag = Some((app, along));
                        w.hover = None;
                    }
                    return true;
                }
            }
        }
        let Some(w) = self.win.as_mut() else { return false };
        if w.hover != hit {
            w.hover = hit;
            redraw = true;
        }
        redraw
    }

    /// While dragging: moves the pinned entry over the task button under the pointer.
    fn win_drag_over(&mut self, x: f32) {
        let Some(layout) = self.win_layout() else { return };
        let Some(w) = self.win.as_mut() else { return };
        let Some((dragged, _)) = w.drag.clone() else { return };
        let over = layout.tasks.iter().position(|r| x >= r.x && x < r.x + r.w && x >= layout.clip.0 && x < layout.clip.1).and_then(|i| w.buttons.get(i)).map(|b| b.app_id.clone());
        if let Some(over) = over {
            if let Some((from, to)) = taskbar::reorder_target(&self.pinned, &dragged, &over) {
                if edit::move_to(&mut self.pinned, from, to) {
                    self.win_sync_buttons();
                }
            }
        }
    }

    pub(super) fn win_leave(&mut self) -> bool {
        if self.menu.is_some() {
            return self.close_menu();
        }
        let Some(w) = self.win.as_mut() else { return false };
        if w.drag.is_some() || w.status.status_dragging() {
            return false;
        }
        let closed = w.status.status_close();
        let was = w.hover.take().is_some();
        closed || was
    }

    pub(super) fn win_scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.win.as_mut().is_some_and(|w| w.status.status_wheel(x, y, dy)) {
            self.win_place_popup();
            return true;
        }
        let hit = self.win_hit(x, y);
        let Some(layout) = self.win_layout() else { return false };
        let Some(w) = self.win.as_mut() else { return false };
        match hit {
            Some(Hit::Tray(k)) => w.status.status_scroll(k, dy),
            Some(Hit::Task(_)) | Some(Hit::Search) | Some(Hit::Start) if layout.scroll_max > 0.0 => {
                let step = SCROLL_STEP * w.m.scale;
                w.scroll = (w.scroll + dy.signum() * step).clamp(0.0, layout.scroll_max);
                true
            }
            _ => false,
        }
    }

    pub(super) fn win_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        // the pointer ends keyboard mode
        if let Some(w) = self.win.as_mut() {
            w.kb = None;
        }
        let placed = self.menu.as_ref().map(|m| self.win_menu_dropdown(&m.menu));
        let along = self.win_along(x, y);
        if let (Some(m), Some(d)) = (&mut self.menu, placed) {
            if button == Button::Left && m.menu.press(&d, x, y) {
                return false;
            }
            if d.contains(x, y) {
                return false;
            }
            self.close_menu();
            if button == Button::Right {
                if let Some(Hit::Task(i)) = self.win_hit(x, y) {
                    self.win_open_task_menu(i);
                }
            }
            return true;
        }
        self.win_place_popup();
        let hit = self.win_hit(x, y);
        // An open tray popup: a press inside it is its own; outside closes it.
        let open = self.win.as_ref().and_then(|w| w.status.status_open_kind());
        if let Some(kind) = open {
            if button == Button::Left && self.win.as_mut().is_some_and(|w| w.status.status_press(x, y)) {
                return true;
            }
            let same = match hit {
                Some(Hit::Tray(k)) => k == kind,
                Some(Hit::Clock) => kind == Tray::Clock,
                _ => false,
            };
            if let Some(w) = self.win.as_mut() {
                w.status.status_close();
            }
            if same || hit.is_none() && button == Button::Left {
                return true;
            }
        }
        match (button, hit) {
            (Button::Left, Some(h)) => {
                if let Some(w) = self.win.as_mut() {
                    w.pressed = Some((h, along));
                }
                true
            }
            (Button::Middle, Some(Hit::Task(i))) => {
                if let Some(id) = self.win.as_ref().and_then(|w| w.buttons.get(i)).map(|b| b.app_id.clone()) {
                    self.launch_item(&id);
                }
                false
            }
            (button @ (Button::Middle | Button::Right), Some(Hit::Tray(kind @ (Tray::Indicator(_) | Tray::Indicators)))) => {
                let handled = self.win.as_mut().is_some_and(|w| w.status.status_indicator_button_at(kind, button, x, y, along));
                self.win_place_popup();
                handled
            }
            (Button::Middle, Some(Hit::Tray(Tray::Audio))) => self.win.as_mut().is_some_and(|w| w.status.status_mute()),
            (Button::Right, Some(Hit::Tray(kind @ (Tray::Awake | Tray::Task(_))))) => {
                let r = self.win_layout().and_then(|l| l.rect_of(Hit::Tray(kind))).map(|r| self.win_to_surface(r)).unwrap_or((x, y, 0.0, 0.0));
                let handled = self.win.as_mut().is_some_and(|w| w.status.status_activate(kind, r, Button::Right));
                self.win_place_popup();
                handled
            }
            (Button::Right, Some(Hit::Task(i))) => {
                self.win_open_task_menu(i);
                true
            }
            (Button::Right, None) => {
                self.win_open_dock_menu(along);
                true
            }
            _ => false,
        }
    }

    pub(super) fn win_release(&mut self, x: f32, y: f32, button: Button) -> bool {
        if button != Button::Left {
            return false;
        }
        if self.menu.is_some() {
            let placed = self.menu.as_ref().map(|m| self.win_menu_dropdown(&m.menu));
            let out = match (&mut self.menu, placed) {
                (Some(m), Some(d)) => m.menu.release(&d, x, y),
                _ => Outcome::None,
            };
            return self.finish_menu(out);
        }
        let Some(w) = self.win.as_mut() else { return false };
        if w.status.status_release(x) {
            return true;
        }
        if w.drag.take().is_some() {
            w.pressed = None;
            self.save_pinned();
            return true;
        }
        let Some((down, _)) = self.win.as_mut().and_then(|w| w.pressed.take()) else { return false };
        if self.win_hit(x, y) == Some(down) {
            let along = self.win_along(x, y);
            self.win_click(down, along);
        }
        true
    }

    // ---------------------------------------------------------------- actions

    /// The workspace a click on the pager lands on: `along` is the pointer's coordinate along the strip.
    fn win_pager_cell_at(&self, along: f32) -> Option<usize> {
        let w = self.win.as_ref()?;
        if w.m.workspace_form != crate::panel::plasma_style::WorkspaceForm::Cells {
            return None;
        }
        let vertical = self.win_vertical();
        let r = self.win_layout()?.rect_of(Hit::Tray(Tray::Workspaces))?;
        let n = w.status.status_workspace_count();
        let (ps, rel) = (crate::panel::plasma_style::pager_shape(w.status.status_desktop_size()), along - r.x);
        let cells = crate::panel::plasma_style::pager_cells(n, ps.0, ps.1, r.w, w.m.icon, vertical);
        cells.iter().position(|c| {
            let (a, len) = if vertical { (c.y, c.h) } else { (c.x, c.w) };
            rel >= a && rel < a + len + 1.0
        })
    }

    fn win_click(&mut self, hit: Hit, along: f32) {
        self.win_activate(hit, along, true);
    }

    fn win_activate(&mut self, hit: Hit, along: f32, direct_pager: bool) {
        if direct_pager && hit == Hit::Tray(Tray::Workspaces) {
            if let Some(i) = self.win_pager_cell_at(along) {
                if let Some(w) = self.win.as_mut() {
                    w.status.status_switch_workspace(i);
                }
                self.wake();
                return;
            }
        }
        match hit {
            Hit::Start => {
                if !self.effects.launch("launchpad") {
                    eprintln!("decdock: could not open the launcher");
                }
            }
            Hit::Search => {
                if !self.effects.spawn(&search_argv()) {
                    eprintln!("decdock: could not start the search");
                }
            }
            Hit::Task(i) => self.win_click_task(i),
            Hit::Tray(kind) => {
                self.win_place_popup();
                let rect = self.win_layout().and_then(|l| l.rect_of(hit));
                let along0 = self.win_along0();
                let cx = rect.map(|r| r.cx() + along0);
                let surface = rect.map(|r| self.win_to_surface(r));
                if let (Some(w), Some(cx), Some(sr)) = (self.win.as_mut(), cx, surface) {
                    if matches!(kind, Tray::Indicator(_) | Tray::Indicators) {
                        w.status.status_indicator_button_at(kind, Button::Left, sr.0 + sr.2 / 2.0, sr.1 + sr.3 / 2.0, cx);
                    } else if DecBar::status_has_popup(kind) {
                        w.status.status_toggle(kind, cx);
                    } else {
                        w.status.status_activate(kind, sr, Button::Left);
                    }
                }
                self.win_place_popup();
            }
            Hit::Clock => {
                let vertical = self.win_vertical();
                let along0 = self.win_along0();
                let right = self.win_layout().map(|l| along0 + if vertical { l.clock.cx() } else { l.clock.x + l.clock.w });
                if let (Some(w), Some(right)) = (self.win.as_mut(), right) {
                    w.status.status_toggle(Tray::Clock, right);
                }
                self.win_place_popup();
            }
            Hit::Desktop => self.win_show_desktop(),
        }
        self.wake();
    }

    fn win_click_task(&mut self, i: usize) {
        let Some(b) = self.win.as_ref().and_then(|w| w.buttons.get(i)).cloned() else { return };
        let windows: Vec<(u64, bool)> = match (&self.tracker, b.window) {
            (Some(t), Some(id)) if self.dock.settings.windows_labels == config::WinLabels::Always => t.snapshot().windows.iter().filter(|w| w.id == id).map(|w| (w.id, w.active)).collect(),
            (Some(t), _) => t.windows_for(&b.app_id).iter().map(|w| (w.id, w.active)).collect(),
            _ => Vec::new(),
        };
        match click_action(&windows) {
            TaskClick::Launch => self.launch_item(&b.app_id),
            TaskClick::Activate(id) => {
                if let Some(t) = &self.tracker {
                    t.activate(id);
                }
            }
            TaskClick::Minimize(id) => {
                if let Some(t) = &self.tracker {
                    t.minimize(id);
                }
            }
        }
    }

    /// "Show desktop": minimise everything on the current workspace; the next click brings it back.
    fn win_show_desktop(&mut self) {
        let Some(t) = self.tracker.clone() else { return };
        let snap = t.snapshot();
        let ws = snap.workspaces.iter().position(|w| w.active).map(|i| i as u32);
        let here: Vec<&crate::wm::Window> = snap.windows.iter().filter(|w| w.desktop.is_none() || ws.is_none() || w.desktop == ws).collect();
        let Some(st) = self.win.as_mut() else { return };
        if here.iter().any(|w| w.active) || st.hidden.is_empty() {
            st.hidden = here.iter().map(|w| w.id).collect();
            for w in &here {
                t.minimize(w.id);
            }
        } else {
            for id in std::mem::take(&mut st.hidden) {
                t.activate(id);
            }
        }
    }

    fn win_open_task_menu(&mut self, i: usize) {
        let Some(b) = self.win.as_ref().and_then(|w| w.buttons.get(i)).cloned() else { return };
        let Some(r) = self.win_layout().and_then(|l| l.tasks.get(i).copied()) else { return };
        let windows: Vec<(u64, String)> = match (&self.tracker, b.window) {
            (Some(t), Some(id)) if self.dock.settings.windows_labels == config::WinLabels::Always => t.snapshot().windows.into_iter().filter(|w| w.id == id).map(|w| (w.id, w.title)).collect(),
            (Some(t), _) => t.windows_for(&b.app_id).into_iter().map(|w| (w.id, w.title)).collect(),
            _ => Vec::new(),
        };
        let ctx = Ctx {
            id: b.app_id.clone(),
            display_name: display_name(&b.app_id),
            running: b.running,
            pinned: self.pinned.contains(&b.app_id),
            windows,
            has_desktop: launch::find_desktop_file(&b.app_id).is_some(),
        };
        self.win_show_menu(&b.app_id, dock_menu::build_entries_windows(&ctx), r.cx());
    }

    fn win_open_dock_menu(&mut self, x: f32) {
        let s = &self.dock.settings;
        let entries = dock_menu::build_entries_dock(s.style, s.windows_align, s.windows_labels);
        self.win_show_menu("dock", entries, x);
    }

    fn win_show_menu(&mut self, item_id: &str, entries: Vec<dock_menu::Entry>, anchor_x: f32) {
        let fonts = Fonts::load();
        let measure = |s: &str| fonts.as_ref().map_or(s.chars().count() as f32 * 7.0, |f| f.measure(s, TEXT_PX, Weight::Regular));
        let menu = DockMenu::new(item_id, entries, anchor_x, &measure);
        if let Some(f) = &fonts {
            f.trim();
        }
        if let Some(w) = self.win.as_mut() {
            w.status.status_close();
            w.hover = None;
        }
        self.menu = Some(MenuState { menu, fonts });
        self.wake();
    }

    /// Writes one dock setting and applies it at once (the poll would notice the file a moment later).
    pub(super) fn set_dock_setting(&mut self, key: &str, value: serde_json::Value) {
        let path = self.watch.as_ref().map(|w| w.settings_path.clone()).unwrap_or_else(|| config::config_dir().join(config::SETTINGS_FILE));
        if let Err(e) = config::write_setting(&path, key, value) {
            eprintln!("decdock: cannot save {}: {e}", path.display());
            return;
        }
        let new = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<config::DockSettings>(&t).ok()).unwrap_or_default();
        if let Some(w) = &mut self.watch {
            w.settings_stamp = Stamp::of(&w.settings_path);
        }
        self.apply_settings(new);
    }

    /// Takes new dock settings, restyling the dock when the style options changed. True when anything changed.
    pub(super) fn apply_settings(&mut self, new: config::DockSettings) -> bool {
        if new == self.dock.settings {
            return false;
        }
        let old = std::mem::replace(&mut self.dock.settings, new);
        let look = |s: &config::DockSettings| (s.style, s.windows_align, s.windows_labels, s.windows_search, s.windows_position, s.windows_theme, s.edge);
        if look(&old) != look(&self.dock.settings) {
            self.style_changed(&old);
            if self.is_windows() {
                self.win_apply_settings();
                self.win_sync_buttons();
            }
        }
        self.wake();
        true
    }

    pub(super) fn win_run_action(&mut self, action: Action) -> bool {
        match action {
            Action::CloseWindow(id) => {
                if let Some(t) = &self.tracker {
                    t.close(id);
                }
            }
            Action::SetStyle(s) => self.set_dock_setting("style", serde_json::json!(s.as_str())),
            Action::SetAlign(a) => self.set_dock_setting("windowsAlign", serde_json::json!(a.as_str())),
            Action::SetLabels(l) => self.set_dock_setting("windowsLabels", serde_json::json!(if l == config::WinLabels::Always { "always" } else { "never" })),
            Action::OpenSettings => {
                self.effects.spawn(&[launch::sibling_or_path("decsettings"), "desktop".into()]);
            }
            _ => return false,
        }
        true
    }

    pub(super) fn win_wants_keyboard(&self) -> bool {
        self.menu.is_some() || self.win.as_ref().is_some_and(|w| w.kb.is_some() || w.status.status_open_kind().is_some())
    }

    // -------------------------------------------------------------- keyboard

    /// The zones the keyboard walks, in strip order: Start, search, every task button, the tray cells, the clock and
    /// the show-desktop sliver (whichever are on).
    pub(super) fn win_focus_items(&self) -> Vec<Hit> {
        let (Some(l), Some(w)) = (self.win_layout(), self.win.as_ref()) else { return Vec::new() };
        focus_hits(&l, w.buttons.len())
    }

    /// `decdock --focus`: keyboard mode on the first zone, popups and menus closed. False when there is nothing to focus.
    pub(super) fn win_focus_start(&mut self) -> bool {
        self.close_menu();
        let any = !self.win_focus_items().is_empty();
        let Some(w) = self.win.as_mut() else { return false };
        w.status.status_close();
        w.kb = any.then_some(0);
        any
    }

    /// Leaves keyboard mode; false when it was not on.
    pub(super) fn win_focus_end(&mut self) -> bool {
        self.win.as_mut().and_then(|w| w.kb.take()).is_some()
    }

    /// Arrows between the zones (either pair, on any edge), Enter or Space does what a click does. Inside an open tray
    /// popup or the calendar the keys are the popup's.
    pub(super) fn win_nav_key(&mut self, key: NavKey) -> bool {
        if self.win.as_ref().is_some_and(|w| w.status.status_open_kind().is_some()) {
            let handled = self.win.as_mut().is_some_and(|w| w.status.status_nav_key(key));
            self.win_place_popup();
            return handled;
        }
        let Some(i) = self.win.as_ref().and_then(|w| w.kb) else { return false };
        let items = self.win_focus_items();
        if items.is_empty() {
            return self.win_focus_end();
        }
        let i = i.min(items.len() - 1);
        let next = match key {
            NavKey::Left | NavKey::Up => (i + items.len() - 1) % items.len(),
            NavKey::Right | NavKey::Down => (i + 1) % items.len(),
            NavKey::Enter => {
                let hit = items[i];
                let along = self.win_layout().and_then(|l| l.rect_of(hit)).map_or(0.0, |r| r.cx());
                // Keyboard activation opens the workspace list, making every pager cell reachable.
                self.win_activate(hit, along, false);
                if self.win.as_ref().is_some_and(|w| w.status.status_open_kind().is_some()) {
                    // a popup opened: its first row takes the keys
                    if let Some(w) = self.win.as_mut() {
                        w.status.status_focus_first_row();
                    }
                } else if matches!(hit, Hit::Start | Hit::Search | Hit::Task(_) | Hit::Desktop) {
                    // the launcher, search or a window takes the focus from here
                    self.win_focus_end();
                }
                return true;
            }
        };
        // a task button scrolled out of the row comes into view
        if let (Hit::Task(_), Some(l)) = (items[next], self.win_layout()) {
            if let (Some(r), Some(w)) = (l.rect_of(items[next]), self.win.as_mut()) {
                if r.x < l.clip.0 {
                    w.scroll = (w.scroll - (l.clip.0 - r.x)).clamp(0.0, l.scroll_max);
                } else if r.x + r.w > l.clip.1 {
                    w.scroll = (w.scroll + (r.x + r.w - l.clip.1)).clamp(0.0, l.scroll_max);
                }
            }
        }
        if let Some(w) = self.win.as_mut() {
            w.kb = Some(next);
        }
        true
    }

    pub(super) fn win_escape(&mut self) -> bool {
        let closed_menu = self.close_menu();
        let closed_popup = self.win.as_mut().is_some_and(|w| w.status.status_close());
        closed_menu || closed_popup
    }
}

/// [`DockApp::win_focus_items`] for a layout with `buttons` task buttons.
fn focus_hits(l: &Layout, buttons: usize) -> Vec<Hit> {
    let mut out = Vec::new();
    if l.start.w > 0.0 {
        out.push(Hit::Start);
    }
    if l.search.is_some() {
        out.push(Hit::Search);
    }
    out.extend((0..buttons.min(l.tasks.len())).map(Hit::Task));
    out.extend(l.tray.iter().map(|(k, _)| Hit::Tray(*k)));
    if l.clock.w > 0.0 {
        out.push(Hit::Clock);
    }
    if l.desktop.w > 0.0 {
        out.push(Hit::Desktop);
    }
    out
}

/// The layout rectangle of keyboard zone `i`.
fn focus_rect(l: &Layout, buttons: &[TaskButton], i: usize) -> Option<taskbar::Rect> {
    focus_hits(l, buttons.len()).get(i).and_then(|h| l.rect_of(*h))
}
