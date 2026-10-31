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
    scroll: f32,
    focus: Option<Target>,
    buffer: Option<Pixmap>,
    mask: Option<tiny_skia::Mask>,
    material_seen: u64,
    viewport_items: Vec<controlcenter::Item>,
    viewport_source: Vec<controlcenter::Item>,
    viewport_stamp: (f32, u32),
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
            scroll: 0.0,
            focus: None,
            buffer: None,
            mask: None,
            material_seen: 0,
            viewport_items: Vec::new(),
            viewport_source: Vec::new(),
            viewport_stamp: (-1.0, 0),
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

    /// Puts the panel where the placement says, for the surface as it is now.
    fn sync(&mut self) {
        let (w, h) = (self.surface.0 as f32, self.surface.1 as f32);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let origin = self.place.origin(w, h, self.bar.cc_width(), self.viewport_height());
        self.scroll = self.scroll.clamp(0.0, (self.bar.cc_height - self.viewport_height()).max(0.0));
        self.bar.embed.pinned = Some(origin);
        self.bar.embed.origin = origin;
        self.bar.embed.screen_w = w;
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

    /// Whether this look gets the clipped scrolling viewport and keyboard focus (Dux and GNOME).
    fn viewport(&self) -> bool {
        self.bar.cc_style().scrolls()
    }

    fn viewport_height(&self) -> f32 {
        if self.viewport() {
            self.bar.cc_height.min((self.surface.1 as f32 - self.place.inset_y - EDGE_GAP).max(1.0))
        } else {
            self.bar.cc_height
        }
    }

    fn viewport_rect(&self) -> Option<InputRect> {
        let mut rect = self.bar.status_rect()?;
        if self.viewport() {
            rect.h = self.viewport_height().ceil() as i32;
        }
        Some(rect)
    }

    /// Reuse the viewport allocation and corner mask; only cached snapshots are
    /// painted. The buffer clips both native glyphs and slider hit decoration.
    fn draw_viewport(&mut self, pm: &mut Pixmap) {
        let width = self.bar.cc_width().ceil() as u32;
        let height = self.viewport_height().ceil().max(1.0) as u32;
        if self.buffer.as_ref().is_none_or(|b| b.width() != width || b.height() != height) {
            let radius = controlcenter::look(self.bar.cc_style()).0;
            self.buffer = Pixmap::new(width, height);
            self.mask = tiny_skia::Mask::new(width, height).and_then(|mut mask| {
                let path = crate::gfx::rounded_rect(0.0, 0.0, width as f32, height as f32, radius)?;
                mask.fill_path(&path, tiny_skia::FillRule::Winding, true, tiny_skia::Transform::identity());
                Some(mask)
            });
        }
        let Some(buffer) = self.buffer.as_mut() else { return };
        crate::gfx::clear(buffer);
        let scale = crate::theme::live::font_scale().max(1.0);
        if self.viewport_source != self.bar.cc_items || self.viewport_stamp != (self.scroll, height) {
            self.viewport_source.clone_from(&self.bar.cc_items);
            self.viewport_items.clear();
            self.viewport_items.extend(self.bar.cc_items.iter().filter(|it| it.r.y + it.r.h * scale >= self.scroll && it.r.y <= self.scroll + height as f32).cloned().map(|mut it| {
                it.r.y -= self.scroll;
                it
            }));
            self.viewport_stamp = (self.scroll, height);
        }
        let items = &self.viewport_items;
        let style = self.bar.cc_style();
        if let Some(fonts) = self.bar.fonts.as_mut() {
            controlcenter::draw_focused(buffer, &controlcenter::View { origin: (0.0, 0.0), items, height: height as f32, hover: self.bar.cc_hover, style }, self.focus, fonts, &mut self.bar.symbols);
        }
        if self.bar.cc_height > height as f32 {
            let track = height as f32 - 32.0;
            let thumb = (track * height as f32 / self.bar.cc_height).max(24.0);
            let y = 16.0 + (track - thumb) * self.scroll / (self.bar.cc_height - height as f32);
            fill_rounded(buffer, width as f32 - 7.0, y, 3.0, thumb, 1.5, crate::theme::th().text_secondary.color(0.5));
        }
        if let Some(mask) = &self.mask {
            buffer.apply_mask(mask);
        }
        let (x, y) = self.bar.embed.origin;
        crate::gfx::blit_scaled(pm, buffer, x, y, width as f32, height as f32, 1.0);
    }
}

impl App for Overlay {
    fn size(&self) -> (u32, u32) {
        (0, if self.viewport() { (self.place.inset_y + self.bar.cc_height + EDGE_GAP).ceil().max(1.0) as u32 } else { Overlay::surface_height(&self.place) })
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
        self.surface = (pm.width(), pm.height());
        self.sync();
        if self.viewport() {
            self.draw_viewport(pm);
        } else {
            self.bar.status_draw(pm);
        }
        if let (Some(r), Some((path, last))) = (self.viewport_rect(), self.debug.as_mut()) {
            let mut text = format!("panel {} {} {} {}\nsurface {} {}\nscroll {} content {}\n", r.x, r.y, r.w, r.h, pm.width(), pm.height(), self.scroll, self.bar.cc_height);
            // every control once (its first rectangle) in surface coordinates, and the keyboard focus: what the native acceptance driver clicks
            let (ox, oy) = self.bar.embed.origin;
            let mut seen = Vec::new();
            for item in &self.bar.cc_items {
                if let Some(target) = item.target.filter(|t| !seen.contains(t)) {
                    seen.push(target);
                    text += &format!("target {target:?} {} {} {} {}\n", ox + item.r.x, oy + item.r.y - self.scroll, item.r.w, item.r.h);
                }
            }
            text += &format!("focus {}\n", self.focus.map_or("none".to_string(), |t| format!("{t:?}")));
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
        self.focus = None;
        self.bar.status_motion(x, if self.viewport() { y + self.scroll } else { y })
    }

    fn pointer_leave(&mut self) -> bool {
        if self.entered && !self.bar.status_dragging() {
            self.left_at = Some(Instant::now());
        }
        self.bar.cc_hover.take().is_some()
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        self.left_at = None;
        self.focus = None;
        self.sync();
        let inside = self.viewport_rect().is_some_and(|r| x >= r.x as f32 && x < (r.x + r.w) as f32 && y >= r.y as f32 && y < (r.y + r.h) as f32);
        if button == Button::Left && inside && self.bar.status_press(x, if self.viewport() { y + self.scroll } else { y }) {
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
        if self.viewport() {
            self.focus = None;
            self.bar.cc_hover = None;
            if self.viewport_rect().is_some_and(|r| x >= r.x as f32 && x < (r.x + r.w) as f32 && y >= r.y as f32 && y < (r.y + r.h) as f32) {
                let (ox, oy) = self.bar.embed.origin;
                if controlcenter::hit(&self.bar.cc_items, x - ox, y - oy + self.scroll).is_some_and(|(target, _)| matches!(target, Target::Volume | Target::Brightness)) {
                    return self.bar.status_wheel(x, y + self.scroll, dy);
                }
                self.scroll = (self.scroll + dy * ccviewport::SCROLL_STEP).clamp(0.0, (self.bar.cc_height - self.viewport_height()).max(0.0));
                return true;
            }
            return false;
        }
        self.bar.status_wheel(x, y, dy)
    }

    fn key(&mut self, key: Key) -> bool {
        match key {
            Key::Escape => self.close(),
        }
    }

    fn nav_key(&mut self, key: crate::app::NavKey) -> bool {
        if !self.viewport() {
            return false;
        }
        let mut targets = Vec::new();
        for item in &self.bar.cc_items {
            if let Some(target) = item.target {
                if !targets.iter().any(|(seen, _)| *seen == target) {
                    targets.push((target, item.r));
                }
            }
        }
        if targets.is_empty() {
            return false;
        }
        match key {
            crate::app::NavKey::Up | crate::app::NavKey::Down => {
                let step = if key == crate::app::NavKey::Up { -1 } else { 1 };
                let index = targets.iter().position(|(target, _)| Some(*target) == self.focus).map_or(0, |i| (i as i32 + step).rem_euclid(targets.len() as i32) as usize);
                self.focus = Some(targets[index].0);
                self.bar.cc_hover = Some(targets[index].0);
                let r = targets[index].1;
                if r.y < self.scroll {
                    self.scroll = r.y.max(0.0);
                } else if r.y + r.h > self.scroll + self.viewport_height() {
                    self.scroll = r.y + r.h - self.viewport_height();
                }
                self.sync();
                true
            }
            crate::app::NavKey::Enter => {
                let active = match self.focus {
                    Some(wanted) => targets.iter().find(|(target, _)| *target == wanted),
                    None => None,
                };
                let Some(&(target, rect)) = active else { return false };
                if matches!(target, Target::Volume | Target::Brightness) {
                    return false;
                }
                self.bar.activate(target, rect.x, rect.w, rect.x + rect.w / 2.0);
                if !self.open_now() {
                    self.quit = true;
                }
                self.sync();
                true
            }
            crate::app::NavKey::Left | crate::app::NavKey::Right => false,
        }
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
    overlay.focus = None;
    overlay.bar.cc_hover = Some(Target::Screenshot);
    assert!(!overlay.nav_key(crate::app::NavKey::Enter), "Enter without explicit keyboard focus must be inert");
    assert!(sys::take_log().is_empty());
    for target in [Target::Volume, Target::Brightness] {
        assert!(overlay.bar.cc_items.iter().any(|item| item.target == Some(target)));
        overlay.focus = Some(target);
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
