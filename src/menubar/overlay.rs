//! The Control Center as a program of its own: `decbar --control-center` (also `decutil control-center`, and the hot-corner action
//! `control-center`). It is how every theme gets the panel, whatever bar it has: a shortcut or a hot corner opens it under the
//! menu bar, above the taskbar, beside a pill bar, or in a screen corner when there is no bar at all.
//!
//! One transparent surface along the top or bottom edge of the monitor; only the panel takes input. The panel is the same code
//! the menu bar, the pill bar and the Windows taskbar use (an embedded [`DecBar`] status engine), restyled by the theme. It closes on
//! Escape, on a click outside, when the pointer leaves it, and when a tile that opens another program has been pressed.

use super::*;
use crate::backend::{Anchor, SurfaceSpec};
use crate::controlcenter::settings::{Placement, GAP as EDGE_GAP};

/// Tallest the panel gets (every tile group present); the surface is made this tall so it never has to resize.
pub const MAX_PANEL_H: f32 = 560.0;
/// How long the pointer may stay off the panel before it closes, once it has been on it.
const LEAVE_CLOSE: Duration = Duration::from_millis(900);
/// Longest wait for the first data before the panel is drawn with what there is.
const READY_WAIT: Duration = Duration::from_millis(1500);

pub struct Overlay {
    bar: DecBar,
    place: Placement,
    surface: (u32, u32),
    material_seen: u64,
    started: Instant,
    ready: bool,
    entered: bool,
    left_at: Option<Instant>,
    quit: bool,
    /// Test hook (`DECBAR_DEBUG_PANEL=file`): where the panel and the surface were put, rewritten after each draw.
    debug: Option<(std::path::PathBuf, String)>,
}

impl Overlay {
    pub fn new(place: Placement, taskbar: bool) -> Overlay {
        let mut bar = DecBar::status_engine();
        bar.embed.taskbar = taskbar;
        Overlay {
            bar,
            place,
            surface: (0, 0),
            material_seen: 0,
            started: Instant::now(),
            ready: false,
            entered: false,
            left_at: None,
            quit: false,
            debug: std::env::var_os("DECBAR_DEBUG_PANEL").map(|p| (p.into(), String::new())),
        }
    }

    /// Height of the surface: the clearance from the screen edge plus the tallest panel.
    pub fn surface_height(place: &Placement) -> u32 {
        (place.inset_y + MAX_PANEL_H + EDGE_GAP).ceil() as u32
    }

    /// The surface for a placement: along the top or bottom edge, transparent, reserving nothing.
    pub fn spec(place: &Placement) -> SurfaceSpec {
        SurfaceSpec { anchor: if place.is_bottom() { Anchor::Bottom } else { Anchor::Top }, margin: 0, reserve: 0, stretch: true, namespace: "decebal-control-center" }
    }

    /// Opens the panel (taking a full snapshot) and starts the theme watcher.
    fn open(&mut self, waker: Waker) {
        self.bar.waker = Some(waker.clone());
        crate::theme::live::start_watcher(move || waker.wake());
        if self.dux() {
            self.bar.symbols.prepare_material();
            self.material_seen = crate::theme::generation();
        }
        self.bar.embed.pinned = Some((0.0, 0.0));
        self.bar.open_control();
        self.sync();
    }

    /// Fits the panel to the room the placement leaves on the surface as it is now (the common viewport: GNOME reflows,
    /// every look is clipped, scrolls and if need be zoomed), then puts it in its corner.
    fn sync(&mut self) {
        let (w, h) = (self.surface.0 as f32, self.surface.1 as f32);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.bar.embed.screen_w = w;
        self.bar.status_set_room((w - self.place.inset_x - EDGE_GAP).max(1.0), (h - self.place.inset_y - EDGE_GAP).max(1.0));
        let origin = self.place.origin(w, h, self.bar.cc_width(), self.bar.cc_visible_height());
        self.bar.embed.pinned = Some(origin);
        self.bar.embed.origin = origin;
    }

    fn open_now(&self) -> bool {
        self.bar.open == Open::Control
    }

    fn close(&mut self) -> bool {
        self.quit = true;
        self.bar.close()
    }

    fn dux(&self) -> bool {
        self.bar.cc_style() == controlcenter::settings::Style::Dux
    }

    fn viewport_rect(&self) -> Option<InputRect> {
        self.bar.status_rect()
    }

    /// The embedded engine (tests).
    #[cfg(test)]
    pub(super) fn bar_mut(&mut self) -> &mut DecBar {
        &mut self.bar
    }

    fn inside(&self, x: f32, y: f32) -> bool {
        self.viewport_rect().is_some_and(|r| x >= r.x as f32 && x < (r.x + r.w) as f32 && y >= r.y as f32 && y < (r.y + r.h) as f32)
    }
}

impl App for Overlay {
    fn size(&self) -> (u32, u32) {
        // as tall as the whole panel needs; the backend keeps the surface on the output and the panel is fitted to what it got
        (0, (self.place.inset_y + self.bar.cc_height.max(1.0) + EDGE_GAP).ceil().max(1.0) as u32)
    }

    fn surface_spec(&self) -> Option<SurfaceSpec> {
        Some(Overlay::spec(&self.place))
    }

    fn resized(&mut self, w: u32, h: u32) {
        self.surface = (w, h);
        self.sync();
    }

    fn set_waker(&mut self, waker: Waker) {
        self.open(waker);
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        crate::gfx::clear(pm);
        if !self.ready {
            return;
        }
        self.surface = crate::gfx::logical_size(pm);
        self.sync();
        self.bar.status_draw(pm);
        if let (Some(r), Some((path, last))) = (self.viewport_rect(), self.debug.as_mut()) {
            let (scroll, z) = (self.bar.cc_viewport.scroll, self.bar.cc_zoom());
            let mut text =
                format!("panel {} {} {} {}\nsurface {} {}\nscroll {} content {}\nzoom {} width {}\n", r.x, r.y, r.w, r.h, pm.width(), pm.height(), scroll, self.bar.cc_height, z, self.bar.cc_w);
            // every control once (its first rectangle) in surface coordinates, and the keyboard focus: what the native acceptance driver clicks
            let (ox, oy) = self.bar.embed.origin;
            let mut seen = Vec::new();
            for item in &self.bar.cc_items {
                if let Some(target) = item.target.filter(|t| !seen.contains(t)) {
                    seen.push(target);
                    text += &format!("target {target:?} {} {} {} {}\n", ox + item.r.x * z, oy + (item.r.y - scroll) * z, item.r.w * z, item.r.h * z);
                }
            }
            text += &format!("focus {}\n", self.bar.cc_viewport.focus.map_or("none".to_string(), |t| format!("{t:?}")));
            text += &format!("hover {}\n", self.bar.cc_hover.map_or("none".to_string(), |t| format!("{t:?}")));
            if *last != text {
                let _ = std::fs::write(&*path, &text);
                *last = text;
            }
        }
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> bool {
        self.entered = true;
        self.left_at = None;
        self.sync();
        self.bar.cc_viewport.focus = None;
        self.bar.status_motion(x, y)
    }

    fn pointer_leave(&mut self) -> bool {
        if self.entered && !self.bar.status_dragging() {
            self.left_at = Some(Instant::now());
        }
        self.bar.cc_hover.take().is_some()
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        self.left_at = None;
        self.bar.cc_viewport.focus = None;
        self.sync();
        let inside = self.inside(x, y);
        if button == Button::Left && inside && self.bar.status_press(x, y) {
            if !self.open_now() {
                self.quit = true;
            }
            return true;
        }
        if !inside {
            return self.close();
        }
        false
    }

    fn pointer_release(&mut self, x: f32, _y: f32, button: Button) -> bool {
        button == Button::Left && self.bar.status_release(x)
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        self.sync();
        // the panel's own wheel: a slider under the pointer steps, elsewhere the clipped panel scrolls
        self.bar.status_wheel(x, y, dy)
    }

    fn key(&mut self, key: Key) -> bool {
        match key {
            Key::Escape => self.close(),
        }
    }

    fn nav_key(&mut self, key: crate::app::NavKey) -> bool {
        let handled = self.bar.status_nav_key(key);
        if !self.open_now() {
            self.quit = true;
        }
        self.sync();
        handled
    }

    fn glass_regions(&self) -> Vec<crate::glass::Region> {
        if !self.dux() {
            return Vec::new();
        }
        self.viewport_rect().map(|r| vec![crate::glass::Region { x: r.x as f32, y: r.y as f32, w: r.w as f32, h: r.h as f32, radius: 28.0 }]).unwrap_or_default()
    }

    fn wants_keyboard(&self) -> bool {
        self.ready
    }

    fn wants_pointer_grab(&self) -> bool {
        self.ready
    }

    fn input_rects(&self) -> Vec<InputRect> {
        if !self.ready {
            return Vec::new();
        }
        self.viewport_rect().into_iter().collect()
    }

    fn quit_requested(&self) -> bool {
        self.quit || !self.open_now()
    }

    fn tick_in(&self) -> Option<Duration> {
        self.left_at.map(|t| LEAVE_CLOSE.saturating_sub(t.elapsed()))
    }

    fn tick(&mut self) -> Tick {
        let mut redraw = self.bar.status_tick();
        if self.dux() && self.material_seen != crate::theme::generation() {
            self.bar.symbols.prepare_material();
            self.material_seen = crate::theme::generation();
            redraw = true;
        }
        if !self.ready && (redraw || self.started.elapsed() >= READY_WAIT) {
            self.ready = true;
            redraw = true;
        }
        if redraw {
            self.sync();
        }
        let mut next = if self.ready { Duration::from_secs(30) } else { READY_WAIT.saturating_sub(self.started.elapsed()).max(Duration::from_millis(20)) };
        if let Some(t) = self.left_at {
            if t.elapsed() >= LEAVE_CLOSE {
                redraw |= self.close();
            } else {
                next = next.min(LEAVE_CLOSE - t.elapsed());
            }
        }
        Tick { redraw, next: Some(next) }
    }
}

/// The native surface evidence test runs alone with an actual Dux theme. This
/// verifies keyboard behavior against cached device state and recorded setters,
/// without touching a live device or starting a worker.
#[cfg(test)]
pub(crate) fn assert_slider_keyboard_inert() {
    struct RestoreDry(bool);
    impl Drop for RestoreDry {
        fn drop(&mut self) {
            sys::set_dry_run(self.0);
        }
    }
    let _restore = RestoreDry(sys::is_dry_run());
    sys::set_dry_run(true);
    sys::take_log();
    let place = crate::controlcenter::settings::place(&crate::controlcenter::settings::Host::Nothing, crate::controlcenter::settings::Corner::Auto);
    let mut overlay = Overlay::new(place, false);
    *overlay.bar.shared.snap.lock().unwrap() = Snapshot { volume: Some(sys::Volume { level: 0.67, muted: false }), brightness: Some(73), ..Snapshot::default() };
    overlay.bar.open_control();
    overlay.resized(960, 720);
    assert!(overlay.dux() && overlay.open_now());
    overlay.bar.cc_viewport.focus = None;
    overlay.bar.cc_hover = Some(Target::Screenshot);
    assert!(!overlay.nav_key(crate::app::NavKey::Enter), "Enter without explicit keyboard focus must be inert");
    assert!(sys::take_log().is_empty());
    for target in [Target::Volume, Target::Brightness] {
        assert!(overlay.bar.cc_items.iter().any(|item| item.target == Some(target)));
        overlay.bar.cc_viewport.focus = Some(target);
        overlay.nav_key(crate::app::NavKey::Enter);
        assert!(overlay.bar.cc_drag.is_none(), "keyboard must not arm pointer drag");
        let state = overlay.bar.snapshot();
        assert_eq!(state.volume.as_ref().unwrap().level, 0.67);
        assert_eq!(state.brightness, Some(73));
        assert!(sys::take_log().is_empty(), "keyboard must not issue midpoint setters");
    }
    let mut bar = DecBar::new(Box::new(super::active::StaticApp("Native fixture".into())));
    *bar.shared.snap.lock().unwrap() = Snapshot { volume: Some(sys::Volume { level: 0.67, muted: false }), brightness: Some(73), ..Snapshot::default() };
    bar.open_control();
    bar.resized(960, 720);
    assert!(bar.dux() && bar.open == Open::Control);
    bar.cc_hover = Some(Target::Screenshot);
    bar.cc_viewport.focus = None;
    assert!(!bar.nav_key(crate::app::NavKey::Enter), "stale pointer hover must never become a keyboard command");
    assert!(sys::take_log().is_empty());
    bar.pointer_leave();
    assert!(bar.cc_hover.is_none());
    bar.dux_hover = Some(super::segmented::Kind::Tray(Tray::Logo));
    bar.pointer_motion(500.0, bar.bar_bottom() + 20.0);
    assert!(bar.dux_hover.is_none(), "moving into panel clears bar hover");
    struct TitleFixture(std::rc::Rc<std::cell::RefCell<String>>);
    impl active::ActiveWindow for TitleFixture {
        fn active_app_name(&self) -> Option<String> {
            Some("Same app".into())
        }
        fn active_title(&self) -> Option<String> {
            Some(self.0.borrow().clone())
        }
    }
    let title = std::rc::Rc::new(std::cell::RefCell::new("First title".into()));
    let mut title_bar = DecBar::new(Box::new(TitleFixture(title.clone())));
    assert!(title_bar.dux_tick());
    assert!(!title_bar.dux_tick());
    *title.borrow_mut() = "Second title".into();
    assert!(title_bar.dux_tick(), "title-only changes must request an actual repaint");
    assert!(!title_bar.dux_tick());
    for target in [Target::Volume, Target::Brightness] {
        assert!(bar.cc_items.iter().any(|item| item.target == Some(target)));
        bar.cc_viewport.focus = Some(target);
        bar.nav_key(crate::app::NavKey::Enter);
        assert!(bar.cc_drag.is_none(), "mainbar keyboard must not arm pointer drag");
        let state = bar.snapshot();
        assert_eq!(state.volume.as_ref().unwrap().level, 0.67);
        assert_eq!(state.brightness, Some(73));
        assert!(sys::take_log().is_empty(), "mainbar keyboard must not issue midpoint setters");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BarPosition;
    use crate::controlcenter::settings::{place, Corner, Host};

    fn up(host: Host, want: Corner, (w, h): (u32, u32)) -> Overlay {
        let p = place(&host, want);
        let mut o = Overlay::new(p, host.is_taskbar());
        o.bar.open_control();
        o.resized(w, Overlay::surface_height(&p).min(h));
        o.ready = true;
        o
    }

    #[test]
    fn the_tallest_panel_fits_the_surface() {
        let full = Snapshot {
            net: Some(sys::Net { wifi_enabled: true, wifi_name: "x".into(), wired_name: "y".into() }),
            bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
            volume: Some(sys::Volume { level: 0.5, muted: false }),
            brightness: Some(50),
            night_shift: Some(false),
            dnd: Some(false),
            media: Some(sys::Media { playing: true, title: "t".into(), artist: "a".into() }),
            util: Some(sys::Utils::default()),
            ..Snapshot::default()
        };
        let (_, h) = controlcenter::layout(&full);
        assert!(h <= MAX_PANEL_H, "the panel is {h} tall, the surface room {MAX_PANEL_H}");
        const { assert!(MAX_PANEL_H + 2.0 * EDGE_GAP + crate::theme::PILL_BAR_THICKNESS <= vbar::POPOUT_H + vbar::THICK) };
    }

    #[test]
    fn the_panel_sits_in_its_corner_clear_of_the_bar_on_every_host() {
        let screen = (1600, 1000);
        for (host, want, right, bottom) in [
            (Host::MenuBar { thickness: 26.0 }, Corner::Auto, true, false),
            (Host::Taskbar { top: false, thickness: 48.0 }, Corner::Auto, true, true),
            (Host::Taskbar { top: true, thickness: 48.0 }, Corner::Auto, true, false),
            (Host::Pill { position: BarPosition::Left, thickness: 52.0 }, Corner::Auto, false, false),
            (Host::Pill { position: BarPosition::Right, thickness: 52.0 }, Corner::Auto, true, false),
            (Host::Pill { position: BarPosition::Top, thickness: 52.0 }, Corner::Auto, true, false),
            (Host::Pill { position: BarPosition::Bottom, thickness: 52.0 }, Corner::Auto, true, true),
            (Host::Pill { position: BarPosition::Split, thickness: 52.0 }, Corner::Auto, true, true),
            (Host::Nothing, Corner::Auto, true, false),
            (Host::Nothing, Corner::BottomLeft, false, true),
        ] {
            let o = up(host, want, screen);
            let r = o.bar.status_rect().unwrap();
            let (x, y, w, h) = (r.x, r.y, r.w, r.h);
            let (sw, sh) = (o.surface.0 as i32, o.surface.1 as i32);
            assert!(x >= 0 && y >= 0 && x + w <= sw && y + h <= sh, "{host:?}: {r:?} outside {sw}x{sh}");
            assert_eq!(x + w == sw - EDGE_GAP as i32 || x + w == sw - (EDGE_GAP + 52.0) as i32, right, "{host:?}: right edge {r:?}");
            assert_eq!(y + h > sh - 120, bottom, "{host:?}: bottom? {r:?} in {sh}");
            assert_eq!(o.input_rects().len(), 1, "only the panel takes input");
        }
    }

    #[test]
    fn it_closes_on_escape_outside_clicks_and_the_pointer_leaving() {
        let mut o = up(Host::MenuBar { thickness: 26.0 }, Corner::Auto, (1600, 1000));
        assert!(!o.quit_requested());
        assert!(o.pointer_button(2.0, 2.0, Button::Left), "a click outside closes");
        assert!(o.quit_requested());
        let mut o = up(Host::Nothing, Corner::Auto, (1600, 1000));
        assert!(o.key(Key::Escape) && o.quit_requested());
        let mut o = up(Host::Nothing, Corner::Auto, (1600, 1000));
        o.pointer_leave();
        assert!(o.left_at.is_none(), "a pointer that never was over the panel does not start the close timer");
        let r = o.bar.status_rect().unwrap();
        o.pointer_motion(r.x as f32 + 10.0, r.y as f32 + 10.0);
        o.pointer_leave();
        assert!(o.left_at.is_some() && !o.quit_requested());
        o.left_at = Some(Instant::now() - LEAVE_CLOSE);
        o.tick();
        assert!(o.quit_requested());
    }

    #[test]
    fn a_tile_that_opens_another_program_ends_the_overlay() {
        crate::controlcenter::sys::set_dry_run(true);
        let mut o = up(Host::Nothing, Corner::Auto, (1600, 1000));
        let it = o.bar.cc_items.iter().find(|i| i.target == Some(Target::Settings)).cloned().expect("Settings tile");
        let r = o.bar.status_rect().unwrap();
        let (px, py) = (r.x as f32 + it.r.x + it.r.w / 2.0, r.y as f32 + it.r.y + it.r.h / 2.0);
        assert!(o.pointer_button(px, py, Button::Left));
        assert!(o.quit_requested(), "Settings closes the panel");
        crate::controlcenter::sys::set_dry_run(false);
    }
}
