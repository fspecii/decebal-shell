//! The Windows-style taskbar mode of [`DockApp`]: input handling, state and the glue between the pure
//! model in [`crate::taskbar`], the tray popups of the embedded [`MacBar`] status engine and the
//! backend. Everything that differs between the two dock styles lives here; `dock_app.rs` only
//! dispatches to these methods when `dock.style` is `"windows"`.

use super::*;
use crate::config::{DockStyle, WinPosition, WinTheme};
use crate::menubar::clock;
use crate::menubar::{MacBar, Tray};
use crate::taskbar::draw::{Colors, Frame, Painter};
use crate::taskbar::{self, click_action, Hit, Inputs, Layout, Metrics, TaskButton, TaskClick};

/// Space kept above a popup inside the surface.
const POP_TOP: f32 = 6.0;
/// Wheel notches move the task row by this many pixels (scaled).
const SCROLL_STEP: f32 = 48.0;

pub(super) struct WinState {
    pub painter: Painter,
    pub status: MacBar,
    pub status_started: bool,
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
    /// Test hook (`MACDOCK_DEBUG_LAYOUT=file`): zone centres in surface coordinates after each draw.
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
            status: MacBar::status_engine(),
            status_started: false,
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
            debug: std::env::var_os("MACDOCK_DEBUG_LAYOUT").map(|p| (PathBuf::from(p), String::new())),
        }
    }
}

/// Time and date lines of the clock: `14:05` over `27/10/2026`.
pub fn clock_lines() -> (String, String) {
    let time = clock::now().map(|t| format!("{:02}:{:02}", t.hour, t.min)).unwrap_or_default();
    let date = clock::today().map(|(y, m, d)| format!("{d:02}/{:02}/{y}", m + 1)).unwrap_or_default();
    (time, date)
}

/// What opens when the search entry is clicked: `MACSHELL_SPOTLIGHT` or macspot.
pub fn search_argv() -> Vec<String> {
    match std::env::var("MACSHELL_SPOTLIGHT") {
        Ok(cmd) if !cmd.is_empty() => launch::exec_argv(&cmd),
        _ => vec![launch::sibling_or_path("macspot")],
    }
}

/// Surface spec of the taskbar for these settings and metrics.
pub fn taskbar_spec(s: &config::DockSettings, m: &Metrics) -> SurfaceSpec {
    let anchor = if s.windows_position == WinPosition::Top { Anchor::Top } else { Anchor::Bottom };
    SurfaceSpec { anchor, margin: 0, reserve: m.bar_h.ceil() as i32, stretch: true, namespace: "macshell-dock" }
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

    fn on_top(&self) -> bool {
        self.dock.settings.windows_position == WinPosition::Top
    }

    /// Creates the taskbar state the first time the Windows style is needed and starts the status engine.
    pub(super) fn win_init(&mut self) {
        if self.win.is_none() {
            let scale = *self.scale.get_or_insert_with(crate::backend::ui_scale);
            self.win = Some(Box::new(WinState::new(&self.dock.settings, scale)));
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
        w.m = Metrics::new(scale, s.windows_align);
        let light = match s.windows_theme {
            WinTheme::Auto => w.status.status_state().light,
            WinTheme::Dark => false,
            WinTheme::Light => true,
        };
        w.colors = Colors::pick(light);
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
        if self.on_top() {
            0.0
        } else {
            (self.surface.1 as f32 - self.win_bar_h()).max(0.0)
        }
    }

    /// Strip edge a popup hangs from (the side facing the desktop).
    fn win_edge(&self) -> f32 {
        if self.on_top() {
            self.win_bar_h()
        } else {
            self.win_strip_top()
        }
    }

    fn win_width(&self) -> f32 {
        self.surface.0.max(1) as f32
    }

    pub(super) fn win_layout(&self) -> Option<Layout> {
        let w = self.win.as_ref()?;
        let s = &self.dock.settings;
        let kinds = w.status.status_kinds();
        Some(taskbar::layout(
            &w.m,
            &Inputs {
                width: self.win_width(),
                align: s.windows_align,
                labels: s.windows_labels,
                search: s.windows_search,
                buttons: w.buttons.len(),
                tray: &kinds,
                clock_text_w: w.clock_w,
                scroll: w.scroll,
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
        let bar = self.win_bar_h();
        let mut extra = 0.0f32;
        if let Some(m) = &self.menu {
            extra = extra.max(m.menu.size().1 + MENU_GAP + POP_TOP);
        }
        if let Some((_, h)) = self.win.as_ref().and_then(|w| w.status.status_size()) {
            extra = extra.max(h + crate::menubar::STATUS_GAP + POP_TOP);
        }
        (0, (bar + extra).ceil() as u32)
    }

    pub(super) fn win_spec(&self) -> Option<SurfaceSpec> {
        let m = self.win.as_ref().map(|w| w.m).unwrap_or_else(|| Metrics::new(self.scale.unwrap_or(1.0), self.dock.settings.windows_align));
        Some(taskbar_spec(&self.dock.settings, &m))
    }

    fn win_menu_bottom(&self, menu_h: f32) -> f32 {
        menu_bottom_at(self.on_top(), self.win_bar_h(), self.surface.1 as f32, menu_h)
    }

    /// Places the open status popup against the strip.
    fn win_place_popup(&mut self) {
        let (edge, up, width) = (self.win_edge(), !self.on_top(), self.win_width());
        if let Some(w) = self.win.as_mut() {
            w.status.status_place(edge, up, width);
        }
    }

    pub(super) fn win_input_rects(&self) -> Vec<InputRect> {
        let (w, bar) = (self.surface.0 as i32, self.win_bar_h());
        let top = self.win_strip_top();
        let mut rects = vec![InputRect { x: 0, y: top as i32, w, h: bar as i32 }];
        if let Some(m) = &self.menu {
            let d = m.menu.placed(w as f32, self.win_menu_bottom(m.menu.size().1));
            rects.push(InputRect { x: d.x.floor() as i32, y: d.y.floor() as i32, w: d.w.ceil() as i32 + 1, h: d.h.ceil() as i32 });
        }
        if let Some(r) = self.win.as_ref().and_then(|w| w.status.status_rect_at(self.win_edge(), !self.on_top(), self.win_width())) {
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
        let top = self.on_top();
        let alpha = (1.0 - self.dock.settings.transparency * 0.25).clamp(0.6, 1.0);
        let Some(w) = self.win.as_mut() else { return };
        let snap = w.status.status_state();
        let layout_name = w.status.status_layout();
        let open = w.status.status_open_kind().map(|k| if k == Tray::Clock { Hit::Clock } else { Hit::Tray(k) });
        let hover = w.hover;
        let pressed = w.pressed.map(|p| p.0);
        let drag = w.drag.as_ref().and_then(|(id, x)| w.buttons.iter().position(|b| &b.app_id == id).map(|i| (i, *x)));
        let frame = Frame {
            m: &w.m,
            c: &w.colors,
            layout: &layout,
            buttons: &w.buttons,
            hover,
            pressed,
            open,
            tray: &snap,
            layout_name: layout_name.as_deref(),
            time: &w.time,
            date: &w.date,
            show_search_label: self.dock.settings.windows_search == config::WinSearch::Box,
            alpha,
            top,
            drag,
        };
        if let Some(strip) = w.painter.draw_strip(&frame) {
            pm.draw_pixmap(0, y0 as i32, strip.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
        w.status.status_draw(pm);
        self.win_dump(&layout, y0);
        let bottom = self.menu.as_ref().map(|m| self.win_menu_bottom(m.menu.size().1));
        if let (Some(m), Some(bottom)) = (&mut self.menu, bottom) {
            let d = m.menu.placed(pm.width() as f32, bottom);
            if let Some(f) = &mut m.fonts {
                m.menu.draw(pm, &d, f);
                f.trim();
            }
        }
    }

    fn win_dump(&mut self, l: &Layout, y0: f32) {
        let Some(w) = self.win.as_mut() else { return };
        let Some((path, last)) = w.debug.as_mut() else { return };
        let cy = y0 + w.m.bar_h / 2.0;
        let mut t = format!("bar {} {} {}\nstart {} {cy}\n", l.width, y0, w.m.bar_h, l.start.cx());
        if let Some(s) = l.search {
            t += &format!("search {} {cy}\n", s.cx());
        }
        for (b, r) in w.buttons.iter().zip(&l.tasks) {
            t += &format!("task {} {} {cy} {}\n", b.app_id, r.cx(), u8::from(b.active));
        }
        for (k, r) in &l.tray {
            t += &format!("tray {k:?} {} {cy}\n", r.cx());
        }
        t += &format!("clock {} {cy}\ndesktop {} {cy}\nstrut {}\n", l.clock.cx(), l.desktop.cx(), w.m.bar_h);
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
        let mut changed = self.poll_config();
        if self.win.is_none() {
            self.win_init();
            changed = true;
        }
        changed |= self.win_sync_buttons();
        let Some(w) = self.win.as_mut() else { return Tick::default() };
        changed |= w.status.status_tick();
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
        l.hit(x, y - self.win_strip_top(), self.win_bar_h())
    }

    pub(super) fn win_motion(&mut self, x: f32, y: f32) -> bool {
        // A menu or popup being open takes precedence.
        let (top, bar, sh) = (self.on_top(), self.win_bar_h(), self.surface.1 as f32);
        if let Some(m) = &mut self.menu {
            let d = m.menu.placed(self.surface.0 as f32, menu_bottom_at(top, bar, sh, m.menu.size().1));
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
            w.drag = Some((id, x));
            self.win_drag_over(x);
            return true;
        }
        if let Some((Hit::Task(i), px)) = w.pressed {
            let moved = (x - px).abs() > DRAG_THRESHOLD;
            let app = w.buttons.get(i).map(|b| b.app_id.clone());
            if let (true, Some(app)) = (moved, app) {
                if self.pinned.contains(&app) {
                    if let Some(w) = self.win.as_mut() {
                        w.drag = Some((app, x));
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
        let (sw, _) = self.surface;
        let (top, bar, sh) = (self.on_top(), self.win_bar_h(), self.surface.1 as f32);
        if let Some(m) = &mut self.menu {
            let d = m.menu.placed(sw as f32, menu_bottom_at(top, bar, sh, m.menu.size().1));
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
                    w.pressed = Some((h, x));
                }
                true
            }
            (Button::Middle, Some(Hit::Task(i))) => {
                if let Some(id) = self.win.as_ref().and_then(|w| w.buttons.get(i)).map(|b| b.app_id.clone()) {
                    self.launch_item(&id);
                }
                false
            }
            (Button::Middle, Some(Hit::Tray(Tray::Audio))) => self.win.as_mut().is_some_and(|w| w.status.status_mute()),
            (Button::Right, Some(Hit::Task(i))) => {
                self.win_open_task_menu(i);
                true
            }
            (Button::Right, None) => {
                self.win_open_dock_menu(x);
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
            let (top, bar, sh) = (self.on_top(), self.win_bar_h(), self.surface.1 as f32);
            let out = match &mut self.menu {
                Some(m) => {
                    let d = m.menu.placed(self.surface.0 as f32, menu_bottom_at(top, bar, sh, m.menu.size().1));
                    m.menu.release(&d, x, y)
                }
                None => Outcome::None,
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
            self.win_click(down, x);
        }
        true
    }

    // ---------------------------------------------------------------- actions

    fn win_click(&mut self, hit: Hit, _x: f32) {
        match hit {
            Hit::Start => {
                if !self.effects.launch("launchpad") {
                    eprintln!("macdock: could not open the launcher");
                }
            }
            Hit::Search => {
                if !self.effects.spawn(&search_argv()) {
                    eprintln!("macdock: could not start the search");
                }
            }
            Hit::Task(i) => self.win_click_task(i),
            Hit::Tray(kind) => {
                self.win_place_popup();
                let cx = self.win_layout().and_then(|l| l.rect_of(hit)).map(|r| r.cx());
                if let (Some(w), Some(cx)) = (self.win.as_mut(), cx) {
                    w.status.status_toggle(kind, cx);
                }
                self.win_place_popup();
            }
            Hit::Clock => {
                let right = self.win_layout().map(|l| l.clock.x + l.clock.w);
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
            eprintln!("macdock: cannot save {}: {e}", path.display());
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
        let look = |s: &config::DockSettings| (s.style, s.windows_align, s.windows_labels, s.windows_search, s.windows_position, s.windows_theme);
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
                self.effects.spawn(&[launch::sibling_or_path("macsettings"), "desktop".into()]);
            }
            _ => return false,
        }
        true
    }

    pub(super) fn win_wants_keyboard(&self) -> bool {
        self.menu.is_some() || self.win.as_ref().is_some_and(|w| w.status.status_open_kind().is_some())
    }

    pub(super) fn win_escape(&mut self) -> bool {
        let closed_menu = self.close_menu();
        let closed_popup = self.win.as_mut().is_some_and(|w| w.status.status_close());
        closed_menu || closed_popup
    }
}
