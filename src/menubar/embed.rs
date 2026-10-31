//! The bar's status engine for other hosts: the Windows-style taskbar of decdock embeds a [`DecBar`]
//! that never draws its own surface. It supplies the same data (layout, sound, Wi-Fi, Bluetooth, battery),
//! the same popups and the calendar, so a fix to any of them reaches both. The host draws the tray icons
//! itself, places the popup next to the item it opened from ([`DecBar::status_place`]) and forwards input.
//!
//! Everything here is a thin, additive layer over the private parts of `DecBar` and `status.rs`.

use super::indicators::IndicatorPress;
use super::popup::{CalHit, Calendar, Popup};
use super::*;
use crate::app::InputRect;
use crate::menubar::active::StaticApp;

/// Gap between a strip edge and the popup opened from it.
pub const GAP: f32 = 8.0;

/// Where the host asked for the open popup to appear.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Place {
    /// Horizontal centre of the item it belongs to (popup) or its right edge (calendar).
    x: f32,
    right_aligned: bool,
    /// y of the strip edge the popup hangs from, and whether it opens upward (strip at the bottom).
    pub(super) edge: f32,
    pub(super) up: bool,
    pub(super) screen_w: f32,
    /// The host strip runs down a side edge: `edge` is its inner x, `up` means the popup opens leftwards (a right
    /// strip), `x` is the item's centre along the strip (y) and `screen_w` the strip's length.
    pub(super) vertical: bool,
    /// Top-left of the open Control Center panel (surface coordinates), set by [`DecBar::status_place`].
    pub(super) origin: (f32, f32),
    /// Where an overlay host wants the panel (top-left); when set it wins over the item it opened from.
    pub(super) pinned: Option<(f32, f32)>,
    /// The host is the Windows-style taskbar: `control_center.style = "auto"` paints the Windows flyout look.
    pub(super) taskbar: bool,
}

impl DecBar {
    /// A bar that only serves as a status engine: no menus, no focused-window tracking.
    pub fn status_engine() -> DecBar {
        let mut bar = DecBar::new(Box::new(StaticApp(String::new())));
        bar.embedded = true;
        bar.panel_cache.set(None);
        bar.embed.taskbar = true;
        bar.was_place = None;
        bar
    }

    /// The window manager source of the host (its own tracker): the workspace, window and application-menu applets of
    /// the taskbar read it.
    pub fn status_set_active(&mut self, active: Box<dyn active::ActiveWindow>) {
        self.active = active;
        self.show_workspaces = true;
        self.refresh_app_name();
    }

    /// Whether the taskbar shows applet `id` (the shared registry and the user's choices).
    pub fn status_shows(&self, id: crate::panel::AppletId) -> bool {
        self.shows(id)
    }

    /// What a press on a tray item that is not a popup does (launchers, toggles); true when handled. `r` is the item's
    /// rectangle on the host surface.
    pub fn status_activate(&mut self, kind: Tray, r: (f32, f32, f32, f32), button: Button) -> bool {
        let done = self.applet_press(kind, r, button);
        if self.open != Open::None {
            self.status_replace();
        }
        done
    }

    /// Re-places what is open with the last placement parameters.
    fn status_replace(&mut self) {
        if self.embed.vertical {
            self.status_place_side(self.embed.edge, self.embed.up, self.embed.screen_w);
        } else {
            self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
        }
    }

    /// Items that open a popup of their own on the taskbar (the others act at once, see [`DecBar::status_activate`]).
    pub fn status_has_popup(kind: Tray) -> bool {
        matches!(
            kind,
            Tray::Layout
                | Tray::Weather
                | Tray::Bluetooth
                | Tray::Network
                | Tray::Audio
                | Tray::Battery
                | Tray::Control
                | Tray::Clock
                | Tray::AppMenu
                | Tray::Window
                | Tray::Workspaces
                | Tray::Media
                | Tray::Brightness
                | Tray::More
                | Tray::Indicators
        )
    }

    /// Starts the data sources; `waker` makes the host's loop call [`DecBar::status_tick`].
    pub fn status_start(&mut self, waker: Waker) {
        self.start_watchers(waker);
        self.spawn_snapshot(false);
        if self.weather_stale() {
            self.spawn_weather();
        }
    }

    /// The weather symbol for the tray (`weather-rain`, ...) once a reading exists.
    pub fn status_weather_symbol(&self) -> Option<&'static str> {
        let w = self.shared.weather.lock().ok()?.clone()?;
        Some(weather::symbol_for(w.code, w.night))
    }

    /// The status items that exist on this machine, in tray order (left to right).
    pub fn status_kinds(&self) -> Vec<Tray> {
        self.status_kinds_with_limit(32)
    }

    /// Fit-aware form used by taskbar hosts; the shared selector reserves its
    /// own overflow cell when some applications do not fit.
    pub fn status_kinds_with_limit(&self, indicator_slots: usize) -> Vec<Tray> {
        // the taskbar's own order; the quick-settings button, like Windows 11's, last in the tray group, next to the clock
        self.applet_items(super::applets::TASKBAR_TRAY, indicator_slots)
    }

    /// [`DecBar::status_kinds_with_limit`] for a host with room for only `cells` tray cells: what does not fit moves
    /// into an overflow cell (before quick settings), whose popup lists it.
    pub fn status_kinds_fit(&self, cells: usize, indicator_slots: usize) -> Vec<Tray> {
        let mut kinds = self.status_kinds_with_limit(indicator_slots);
        let mut hidden = Vec::new();
        if kinds.len() > cells {
            let victims = [
                Tray::Media,
                Tray::Cpu,
                Tray::Memory,
                Tray::Weather,
                Tray::Clipboard,
                Tray::Notifications,
                Tray::Dash,
                Tray::Agents,
                Tray::Brightness,
                Tray::Lock,
                Tray::Window,
                Tray::AppMenu,
                Tray::Workspaces,
                Tray::Layout,
                Tray::Awake,
                Tray::Power,
                Tray::Bluetooth,
                Tray::Battery,
                Tray::Network,
                Tray::Audio,
            ];
            for v in victims {
                if kinds.len() < cells {
                    break;
                }
                if let Some(i) = kinds.iter().position(|k| *k == v) {
                    hidden.push(kinds.remove(i));
                }
            }
            // application cells go last: the overflow list reaches them through its Applications row
            while kinds.len() >= cells.max(1) {
                let Some(i) = kinds.iter().rposition(|k| matches!(k, Tray::Indicator(_))) else { break };
                kinds.remove(i);
                if !hidden.contains(&Tray::Indicators) {
                    hidden.push(Tray::Indicators);
                }
            }
            if !hidden.is_empty() {
                let at = kinds.iter().position(|k| *k == Tray::Control).unwrap_or(kinds.len());
                kinds.insert(at, Tray::More);
            }
        }
        *self.embed_hidden.borrow_mut() = hidden;
        kinds
    }

    /// The backend reports root-space surface origin; native menus remain in
    /// local surface coordinates, while protocol activation receives root points.
    pub fn status_surface_origin(&mut self, x: i32, y: i32) {
        self.indicator_origin = (x, y);
    }

    /// Keep an open application menu attached as other tray items appear, vanish
    /// or move into overflow. Does not reopen/reload the menu or change its path.
    pub fn status_indicator_anchor(&mut self, x: f32) {
        if matches!(self.open, Open::Popup(Tray::Indicator(_) | Tray::Indicators)) {
            self.embed.x = x;
        }
    }

    /// Paint only cached worker pixels, after the taskbar paints its plates.
    pub fn status_draw_indicator(&self, pm: &mut Pixmap, key: u64, cx: f32, cy: f32, size: f32) {
        self.indicator_draw(pm, key, cx, cy, size);
    }

    /// Shared typed protocol actions; true means this was an indicator target.
    pub fn status_indicator_button(&mut self, kind: Tray, button: Button, x: f32, y: f32) -> bool {
        match kind {
            Tray::Indicator(key) => match self.indicator_press(key, button, x.round() as i32, y.round() as i32) {
                IndicatorPress::Ignored => false,
                IndicatorPress::Handled => true,
                IndicatorPress::Menu => self.status_toggle(kind, x),
            },
            Tray::Indicators if matches!(button, Button::Left | Button::Right) => self.status_toggle(kind, x),
            _ => false,
        }
    }

    pub fn status_debug_indicators(&mut self, rects: &[IndicatorRect]) {
        self.indicator_debug(rects);
    }

    /// What the taskbar's applet cells show beyond the snapshot: CPU / memory load, the current workspace, the focused
    /// window's app, whether media plays, the lock keys.
    pub fn status_extras(&self) -> crate::taskbar::draw::Extras {
        let perf = self.dux_perf.lock().ok().and_then(|p| *p);
        let ws = self.bar_workspaces();
        crate::taskbar::draw::Extras {
            cpu: perf.and_then(|p| p.0),
            memory: perf.and_then(|p| p.1),
            workspace: ws.iter().position(|w| w.active).map(|i| if ws[i].name.chars().count() <= 2 && !ws[i].name.is_empty() { ws[i].name.clone() } else { (i + 1).to_string() }),
            window_app: self.v_win.as_ref().map(|(id, _)| id.clone()),
            playing: self.dux_player().is_some_and(|p| p.status == crate::dash::mpris::Status::Playing),
            caps: self.locks.caps(),
            num: self.locks.num(),
        }
    }

    /// Latest system data (volume, network, battery, light/dark theme).
    pub fn status_state(&self) -> Snapshot {
        self.snapshot()
    }

    /// The current window layout (`tile`, `monocle`, ...) when running under rdwm.
    pub fn status_layout(&self) -> Option<String> {
        self.layout_state().map(|l| l.canonical().to_string())
    }

    /// The popup (or the calendar for [`Tray::Clock`]) that is open, if any.
    pub fn status_open_kind(&self) -> Option<Tray> {
        match self.open {
            Open::Popup(k) => Some(k),
            Open::Control => Some(Tray::Control),
            _ => None,
        }
    }

    /// Opens (or, when already open, closes) the popup for `kind`, anchored on the item centred at `x`
    /// (the calendar hangs from its right edge instead). Returns false when there is nothing to show.
    pub fn status_toggle(&mut self, kind: Tray, x: f32) -> bool {
        if self.open == Open::Popup(kind) || (kind == Tray::Control && self.open == Open::Control) {
            self.close();
            return true;
        }
        self.close();
        self.embed.x = x;
        if kind == Tray::Control {
            if !crate::theme::th().control_center.enabled {
                return false;
            }
            self.open_control();
            self.status_replace();
            return true;
        }
        self.embed.right_aligned = kind == Tray::Clock;
        if kind == Tray::Clock {
            let Some(today) = clock::today() else { return false };
            self.calendar = Some(Calendar::new(0.0, 0.0, today));
        } else {
            let Some((rows, w, compact)) = self.build_popup(kind) else { return false };
            self.popup = Some(Popup::layout(rows, 0.0, 0.0, w, compact));
            self.last_full_refresh = Instant::now();
            match kind {
                Tray::Audio => self.spawn_snapshot(true),
                Tray::Network | Tray::Bluetooth => self.spawn_lists(kind),
                Tray::Battery => self.spawn_snapshot(false),
                Tray::Weather if self.weather_stale() => self.spawn_weather(),
                _ => {}
            }
        }
        self.open = Open::Popup(kind);
        self.status_replace();
        true
    }

    /// Extent of the open popup: (width, height), or None when none is open.
    pub fn status_size(&self) -> Option<(f32, f32)> {
        if self.open == Open::Control {
            return Some((self.cc_width(), self.cc_height));
        }
        self.popup.as_ref().map(|p| (p.w, p.h)).or(self.calendar.as_ref().map(|_| (Calendar::W, Calendar::H)))
    }

    /// Positions the open popup against the strip edge at `edge` (`up`: the strip is at the bottom and the
    /// popup opens above it) inside a surface `screen_w` wide. Call before drawing or hit-testing.
    pub fn status_place(&mut self, edge: f32, up: bool, screen_w: f32) {
        self.embed.vertical = false;
        self.place_embedded(edge, up, screen_w);
    }

    /// [`DecBar::status_place`] for a strip down a side edge: `edge` is the strip's inner x, `left` opens the popup
    /// leftwards (a right-edge strip), `screen_h` is the strip's length; items are anchored by their centre y.
    pub fn status_place_side(&mut self, edge: f32, left: bool, screen_h: f32) {
        self.embed.vertical = true;
        self.place_embedded(edge, left, screen_h);
    }

    fn place_embedded(&mut self, edge: f32, up: bool, screen_w: f32) {
        self.embed.edge = edge;
        self.embed.up = up;
        self.embed.screen_w = screen_w;
        let Some((x, y, _, _)) = self.popup_geometry(edge, up, screen_w) else { return };
        self.embed.origin = (x, y);
        if let Some(p) = self.popup.as_mut() {
            p.x = x;
            p.y = y;
        }
        if let Some(c) = self.calendar.as_mut() {
            c.x = x;
            c.y = y;
        }
    }

    /// (x, y, w, h) the open popup gets for these placement parameters.
    fn popup_geometry(&self, edge: f32, up: bool, screen_w: f32) -> Option<(f32, f32, f32, f32)> {
        let (w, h) = self.status_size()?;
        if self.open == Open::Control {
            if let Some((x, y)) = self.embed.pinned {
                return Some((x, y, w, h));
            }
        }
        if self.embed.vertical {
            // beside the strip, centred on the item along it, kept on the surface
            let y = (self.embed.x - h / 2.0).clamp(4.0, (screen_w - h - 4.0).max(4.0));
            let x = if up { edge - GAP - w } else { edge + GAP };
            return Some((x, y, w, h));
        }
        if self.open == Open::Control {
            // like Windows' quick settings: against the right screen edge when its button is in the right half
            let x = if self.embed.x > screen_w / 2.0 { screen_w - w - GAP } else { self.embed.x - w / 2.0 };
            let y = if up { edge - GAP - h } else { edge + GAP };
            return Some((x.clamp(4.0, (screen_w - w - 4.0).max(4.0)), y, w, h));
        }
        let x = if self.embed.right_aligned { self.embed.x - w } else { self.embed.x - w / 2.0 };
        let x = x.clamp(4.0, (screen_w - w - 4.0).max(4.0));
        let y = if up { edge - GAP - h } else { edge + GAP };
        Some((x, y, w, h))
    }

    /// Input area of the open popup for these placement parameters, without moving it.
    pub fn status_rect_at(&self, edge: f32, up: bool, screen_w: f32) -> Option<InputRect> {
        let (x, y, w, h) = self.popup_geometry(edge, up, screen_w)?;
        Some(InputRect { x: x.floor() as i32, y: y.floor() as i32, w: w.ceil() as i32, h: h.ceil() as i32 })
    }

    pub fn status_close(&mut self) -> bool {
        self.close()
    }

    /// Input area the open popup needs (surface coordinates).
    pub fn status_rect(&self) -> Option<InputRect> {
        if self.open == Open::Control {
            let (x, y) = self.embed.origin;
            return Some(InputRect { x: x.floor() as i32, y: y.floor() as i32, w: self.cc_width().ceil() as i32, h: self.cc_height.ceil() as i32 });
        }
        let (x, y) = self.popup.as_ref().map(|p| (p.x, p.y)).or(self.calendar.as_ref().map(|c| (c.x, c.y)))?;
        let (w, h) = self.status_size()?;
        Some(InputRect { x: x.floor() as i32, y: y.floor() as i32, w: w.ceil() as i32, h: h.ceil() as i32 })
    }

    pub fn status_draw(&mut self, pm: &mut Pixmap) {
        if self.open == Open::Control {
            let style = self.cc_style();
            let view = controlcenter::View { origin: self.embed.origin, items: &self.cc_items, height: self.cc_height, hover: self.cc_hover, style };
            if let Some(f) = self.fonts.as_mut() {
                controlcenter::draw(pm, &view, f, &mut self.symbols);
                f.trim();
            }
            return;
        }
        if let Some(f) = self.fonts.as_mut() {
            if let Some(p) = &self.popup {
                p.draw(pm, f, &mut self.symbols, self.popup_hover);
            } else if let Some(c) = &self.calendar {
                c.draw(pm, f);
            }
            f.trim();
        }
    }

    /// Pointer moved over the surface; true when the popup needs a redraw.
    pub fn status_motion(&mut self, x: f32, y: f32) -> bool {
        if self.open == Open::Control {
            let (ox, oy) = self.embed.origin;
            if let Some(t) = self.cc_drag {
                if let Some(it) = self.cc_items.iter().find(|i| i.target == Some(t)) {
                    let (rx, rw) = (it.r.x, it.r.w);
                    self.slide(t, rx, rw, x - ox, false);
                    return true;
                }
            }
            let hover = controlcenter::hit(&self.cc_items, x - ox, y - oy).map(|h| h.0);
            let changed = hover != self.cc_hover;
            self.cc_hover = hover;
            return changed;
        }
        if let Some(row) = self.popup_drag {
            self.popup_slide(row, x, false);
            return true;
        }
        let hover = self.popup.as_ref().and_then(|p| p.hit(x, y));
        let changed = hover != self.popup_hover;
        self.popup_hover = hover;
        changed
    }

    /// Left press inside the popup; true when it handled the press.
    pub fn status_press(&mut self, x: f32, y: f32) -> bool {
        let inside = self.status_rect().is_some_and(|r| x >= r.x as f32 && x < (r.x + r.w) as f32 && y >= r.y as f32 && y < (r.y + r.h) as f32);
        if !inside {
            return false;
        }
        if self.open == Open::Control {
            let (ox, oy) = self.embed.origin;
            if let Some((t, r)) = controlcenter::hit(&self.cc_items, x - ox, y - oy) {
                self.activate(t, r.x, r.w, x - ox);
            }
            return true;
        }
        self.popup_press(x, y);
        self.status_replace();
        true
    }

    /// Left release (ends a slider drag); true when a redraw is needed.
    pub fn status_release(&mut self, x: f32) -> bool {
        if let Some(t) = self.cc_drag.take() {
            let (ox, _) = self.embed.origin;
            if let Some(it) = self.cc_items.iter().find(|i| i.target == Some(t)) {
                let (rx, rw) = (it.r.x, it.r.w);
                self.slide(t, rx, rw, x - ox, true);
            }
            return true;
        }
        match self.popup_drag.take() {
            Some(row) => {
                self.popup_slide(row, x, true);
                true
            }
            None => false,
        }
    }

    /// True while a slider in the popup is being dragged (the pointer may leave the popup).
    pub fn status_dragging(&self) -> bool {
        self.popup_drag.is_some() || self.cc_drag.is_some()
    }

    /// Wheel over the open Control Center panel: the display and sound sliders move in steps of 5%.
    pub fn status_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.indicator_menu_scroll(x, y, dy) {
            self.status_replace();
            return true;
        }
        if self.open != Open::Control {
            return false;
        }
        let (ox, oy) = self.embed.origin;
        let Some((t @ (Target::Brightness | Target::Volume), r)) = controlcenter::hit(&self.cc_items, x - ox, y - oy) else { return false };
        let snap = self.snapshot();
        let cur = match t {
            Target::Brightness => snap.brightness.unwrap_or(0) as f32 / 100.0,
            _ => snap.volume.map(|v| v.level).unwrap_or(0.0),
        };
        self.slide(t, r.x, r.w, r.x + (cur - dy.signum() * 0.05).clamp(0.0, 1.0) * r.w, true);
        true
    }

    /// Wheel over the status item `kind`: volume steps of 5%, layout cycling.
    pub fn status_scroll(&mut self, kind: Tray, dy: f32) -> bool {
        match kind {
            Tray::Indicator(key) => self.indicator_scroll(key, dy),
            Tray::Control => false,
            Tray::Audio => {
                let Some(v) = self.snapshot().volume else { return false };
                let cur = if v.muted { 0.0 } else { v.level };
                self.set_volume_now((cur - dy.signum() * 0.05).clamp(0.0, 1.0));
                if matches!(self.open, Open::Popup(Tray::Audio)) {
                    self.rebuild_popup();
                    self.status_replace();
                }
                true
            }
            Tray::Layout => {
                rdwm::send_action(if dy > 0.0 { "cyclelayout" } else { "cyclelayout_back" }, &[]);
                false
            }
            _ => false,
        }
    }

    /// Middle click on the sound item mutes.
    pub fn status_mute(&mut self) -> bool {
        self.toggle_mute_now();
        if matches!(self.open, Open::Popup(Tray::Audio)) {
            self.rebuild_popup();
        }
        true
    }

    /// Housekeeping on a wake-up or timer: applies fresh worker data and refreshes the open popup.
    /// True when something visible changed. Call at least once a minute.
    pub fn status_tick(&mut self) -> bool {
        let mut redraw = self.shared.changed.swap(false, Ordering::SeqCst);
        if self.shows(crate::panel::AppletId::WindowTitle) {
            redraw |= self.v_refresh_window();
        }
        redraw |= self.refresh_workspaces();
        redraw |= self.refresh_app_name();
        redraw |= self.dux_tick();
        self.sync_workers();
        redraw |= self.indicator_tick();
        redraw |= crate::theme::take_changed();
        if self.open == Open::Control {
            if redraw && self.cc_drag.is_none() {
                if crate::theme::th().control_center.enabled {
                    self.relayout_control();
                    self.status_replace();
                } else {
                    self.close();
                }
            }
            if self.cc_drag.is_none() && self.last_full_refresh.elapsed() >= PANEL_REFRESH {
                self.last_full_refresh = Instant::now();
                self.spawn_snapshot(true);
            }
        }
        if redraw && matches!(self.open, Open::Popup(_)) && self.popup_drag.is_none() {
            self.rebuild_popup();
            self.status_replace();
        }
        self.refresh_open_popup();
        if let Some(tm) = clock::now() {
            let minute = (tm.hour, tm.min);
            if self.last_minute != Some(minute) {
                self.last_minute = Some(minute);
                self.spawn_snapshot(false);
                if self.weather_stale() {
                    self.spawn_weather();
                }
                redraw = true;
            }
        }
        redraw
    }

    /// The gallery preview's fixed sample data (no workers, no probes): what a typical machine shows.
    pub fn status_fill_sample(&mut self) {
        self.fill_sample();
    }

    /// Tests of hosts: every data source an applet can need, filled with sample data (no workers).
    #[cfg(test)]
    pub fn status_fill_for_tests(&mut self) {
        self.update_snapshot(|s| {
            *s = Snapshot {
                net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: String::new() }),
                bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
                volume: Some(sys::Volume { level: 0.5, muted: false }),
                battery: Some(sys::Battery { percent: 80, charging: false, status: "Discharging".into() }),
                brightness: Some(60),
                ..Snapshot::default()
            }
        });
        *self.shared.weather.lock().unwrap() = Some(sample_weather());
        *self.shared.layout.lock().unwrap() = Some(rdwm::LayoutState { name: "tile".into(), symbol: "[]=".into(), tags: Vec::new() });
        *self.dux_perf.lock().unwrap() = Some((Some(0.25), Some(0.5)));
        self.workspaces = (0..4).map(|i| Workspace { name: (i + 1).to_string(), active: i == 0, occupied: true, urgent: false }).collect();
        self.show_workspaces = true;
        self.v_win = Some(("foot".into(), "~/src".into()));
        let mut player = crate::dash::mpris::Player::new("org.mpris.MediaPlayer2.fixture", Instant::now());
        player.status = crate::dash::mpris::Status::Playing;
        player.meta.title = "Track".into();
        (player.can_control, player.can_next, player.can_prev) = (true, true, true);
        super::applets::TEST_PLAYER.with(|p| *p.borrow_mut() = Some(player));
        let mut pm = Pixmap::new(8, 8).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(240, 60, 90, 255));
        self.indicators.items = vec![crate::sni::Item {
            key: 1,
            title: "App".into(),
            tooltip: String::new(),
            status: crate::sni::Status::Active,
            icon_name: String::new(),
            icon: Some(std::sync::Arc::new(pm)),
            item_is_menu: false,
            menu: None,
            menu_loading: false,
            menu_error: None,
        }];
    }

    /// Tests of hosts: shows exactly `applets` (thread-local, as the bar's own tests).
    #[cfg(test)]
    pub fn status_test_applets(applets: Option<crate::panel::AppletSet>) {
        super::applets::TEST_APPLETS.with(|t| t.set(applets));
        super::applets::TEST_EXPLICIT.with(|t| t.set(applets.unwrap_or_default()));
    }

    /// Calendar navigation helper for tests: the hit zone under a point.
    pub fn status_calendar_hit(&self, x: f32, y: f32) -> Option<CalHit> {
        self.calendar.as_ref().and_then(|c| c.hit(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indicator(key: u64, status: crate::sni::Status) -> crate::sni::Item {
        let mut pm = Pixmap::new(8, 8).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(240, 60, 90, 255));
        crate::sni::Item {
            key,
            title: format!("App {key}"),
            tooltip: String::new(),
            status,
            icon_name: String::new(),
            icon: Some(std::sync::Arc::new(pm)),
            item_is_menu: false,
            menu: None,
            menu_loading: false,
            menu_error: None,
        }
    }

    #[test]
    fn embedded_indicators_use_shared_visibility_fit_pixels_and_popup() {
        let mut bar = DecBar::status_engine();
        assert!(bar.indicator_host.is_none(), "constructing a status model never starts a bus worker");
        bar.indicators.items =
            vec![indicator(1, crate::sni::Status::Active), indicator(2, crate::sni::Status::Passive), indicator(3, crate::sni::Status::NeedsAttention), indicator(4, crate::sni::Status::Active)];
        let external = |kinds: Vec<Tray>| kinds.into_iter().filter(|k| matches!(k, Tray::Indicator(_) | Tray::Indicators)).collect::<Vec<_>>();
        assert_eq!(external(bar.status_kinds_with_limit(0)), vec![]);
        assert_eq!(external(bar.status_kinds_with_limit(1)), vec![Tray::Indicators]);
        assert_eq!(external(bar.status_kinds_with_limit(2)), vec![Tray::Indicator(1), Tray::Indicators]);
        assert_eq!(external(bar.status_kinds_with_limit(3)), vec![Tray::Indicator(1), Tray::Indicator(3), Tray::Indicator(4)]);
        let mut pm = Pixmap::new(80, 80).unwrap();
        bar.status_draw_indicator(&mut pm, 1, 40.0, 40.0, 18.0);
        let pixel = pm.pixel(40, 40).unwrap();
        assert_eq!((pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()), (240, 60, 90, 255));
        bar.status_place(752.0, true, 1280.0);
        assert!(bar.status_indicator_button(Tray::Indicator(1), Button::Right, 800.0, 780.0));
        assert_eq!(bar.status_open_kind(), Some(Tray::Indicator(1)));
        assert!(bar.status_rect().is_some());
        bar.status_close();
        assert!(!bar.status_indicator_button(Tray::Indicator(2), Button::Left, 800.0, 780.0), "passive icons cannot receive activation");
    }

    #[test]
    fn calendar_hangs_from_the_clock_above_a_bottom_strip_and_below_a_top_one() {
        let mut bar = DecBar::status_engine();
        assert!(bar.status_kinds().is_empty() || !bar.status_kinds().contains(&Tray::Clock));
        assert!(bar.status_open_kind().is_none() && bar.status_rect().is_none());
        bar.status_place(752.0, true, 1280.0);
        assert!(bar.status_toggle(Tray::Clock, 1270.0));
        assert_eq!(bar.status_open_kind(), Some(Tray::Clock));
        let r = bar.status_rect().unwrap();
        assert_eq!((r.w, r.h), (Calendar::W as i32, Calendar::H as i32));
        assert_eq!(r.y, (752.0 - GAP - Calendar::H).floor() as i32, "just above the strip");
        assert_eq!(r.x + r.w, 1270, "right edge on the clock");
        bar.status_place(48.0, false, 1280.0);
        assert_eq!(bar.status_rect().unwrap().y, (48.0 + GAP) as i32, "just below a top strip");
        assert_eq!(bar.status_rect_at(48.0, false, 1280.0), bar.status_rect());
        // clicking inside is ours, outside is not; a second toggle closes
        let r = bar.status_rect().unwrap();
        assert!(bar.status_press(r.x as f32 + 20.0, r.y as f32 + 20.0));
        assert!(!bar.status_press(5.0, 5.0));
        assert!(bar.status_toggle(Tray::Clock, 1270.0));
        assert!(bar.status_open_kind().is_none());
    }

    #[test]
    fn popups_are_kept_on_screen_and_closing_clears_them() {
        let mut bar = DecBar::status_engine();
        bar.status_place(752.0, true, 1280.0);
        assert!(bar.status_toggle(Tray::Clock, 100.0));
        assert_eq!(bar.status_rect().unwrap().x, 4, "clamped to the left edge");
        assert!(bar.status_close());
        assert!(bar.status_rect().is_none());
        assert!(!bar.status_toggle(Tray::Audio, 500.0), "no sound server data: nothing to show");
    }
}
