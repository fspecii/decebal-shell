//! The bar's status engine for other hosts: the Windows-style taskbar of decdock embeds a [`DecBar`]
//! that never draws its own surface. It supplies the same data (layout, sound, Wi-Fi, Bluetooth, battery),
//! the same popups and the calendar, so a fix to any of them reaches both. The host draws the tray icons
//! itself, places the popup next to the item it opened from ([`DecBar::status_place`]) and forwards input.
//!
//! Everything here is a thin, additive layer over the private parts of `DecBar` and `status.rs`.

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
    edge: f32,
    up: bool,
    pub(super) screen_w: f32,
    /// Top-left of the open Control Center panel (surface coordinates), set by [`DecBar::status_place`].
    pub(super) origin: (f32, f32),
    /// Where an overlay host wants the panel (top-left); when set it wins over the item it opened from.
    pub(super) pinned: Option<(f32, f32)>,
    /// The host is the Windows-style taskbar: `control_center.style = "auto"` paints the Windows flyout look.
    pub(super) taskbar: bool,
}

impl DecBar {
    /// A bar that only serves as a status engine: no menus, no weather, no focused-window tracking.
    pub fn status_engine() -> DecBar {
        let mut bar = DecBar::new(Box::new(StaticApp(String::new())));
        bar.embedded = true;
        bar.embed.taskbar = true;
        bar.was_place = None;
        bar
    }

    /// Starts the data sources; `waker` makes the host's loop call [`DecBar::status_tick`].
    pub fn status_start(&mut self, waker: Waker) {
        self.start_watchers(waker);
        self.spawn_snapshot(false);
    }

    /// The status items that exist on this machine, in tray order (left to right).
    pub fn status_kinds(&self) -> Vec<Tray> {
        let snap = self.snapshot();
        let mut out = Vec::new();
        if self.layout_state().is_some() {
            out.push(Tray::Layout);
        }
        if snap.bluetooth.is_some() {
            out.push(Tray::Bluetooth);
        }
        if snap.net.is_some() {
            out.push(Tray::Network);
        }
        if snap.volume.is_some() {
            out.push(Tray::Audio);
        }
        if snap.battery.is_some() {
            out.push(Tray::Battery);
        }
        // the quick-settings button, like Windows 11's: last in the tray group, next to the clock
        if crate::theme::th().control_center.enabled {
            out.push(Tray::Control);
        }
        out
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
            self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
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
                _ => {}
            }
        }
        self.open = Open::Popup(kind);
        self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
        true
    }

    /// Extent of the open popup: (width, height), or None when none is open.
    pub fn status_size(&self) -> Option<(f32, f32)> {
        if self.open == Open::Control {
            return Some((controlcenter::PANEL_W, self.cc_height));
        }
        self.popup.as_ref().map(|p| (p.w, p.h)).or(self.calendar.as_ref().map(|_| (Calendar::W, Calendar::H)))
    }

    /// Positions the open popup against the strip edge at `edge` (`up`: the strip is at the bottom and the
    /// popup opens above it) inside a surface `screen_w` wide. Call before drawing or hit-testing.
    pub fn status_place(&mut self, edge: f32, up: bool, screen_w: f32) {
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
            return Some(InputRect { x: x.floor() as i32, y: y.floor() as i32, w: controlcenter::PANEL_W.ceil() as i32, h: self.cc_height.ceil() as i32 });
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
        self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
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
            Tray::Control => false,
            Tray::Audio => {
                let Some(v) = self.snapshot().volume else { return false };
                let cur = if v.muted { 0.0 } else { v.level };
                self.set_volume_now((cur - dy.signum() * 0.05).clamp(0.0, 1.0));
                if matches!(self.open, Open::Popup(Tray::Audio)) {
                    self.rebuild_popup();
                    self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
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
        redraw |= crate::theme::take_changed();
        if self.open == Open::Control {
            if redraw && self.cc_drag.is_none() {
                if crate::theme::th().control_center.enabled {
                    self.relayout_control();
                    self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
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
            self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
        }
        self.refresh_open_popup();
        if let Some(tm) = clock::now() {
            let minute = (tm.hour, tm.min);
            if self.last_minute != Some(minute) {
                self.last_minute = Some(minute);
                self.spawn_snapshot(false);
                redraw = true;
            }
        }
        redraw
    }

    /// Calendar navigation helper for tests: the hit zone under a point.
    pub fn status_calendar_hit(&self, x: f32, y: f32) -> Option<CalHit> {
        self.calendar.as_ref().and_then(|c| c.hit(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
