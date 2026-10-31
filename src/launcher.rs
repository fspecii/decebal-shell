//! The application launcher: the Launchpad grid and the Dux list (drawn here and in `dux`), and the Windows,
//! GNOME, KDE, Xfce and BlackBerry styles (`styled`). [`style`] says which one runs, [`placement`] where it goes.

use crate::app::Button;
use crate::apps::{self, AppEntry};
use crate::gfx::{blit_scaled, color, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::launch;
use crate::text::{Fonts, Weight};
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use tiny_skia::{Color, Pixmap};

pub mod catalog;
mod dux;
pub mod placement;
pub mod style;
mod styled;

use placement::{resolve_anchor, Anchor, Invocation};
use style::LauncherStyle;

const PAD: f32 = 28.0;
const SEARCH_Y: f32 = 22.0;
const SEARCH_H: f32 = 36.0;
const GRID_Y: f32 = 84.0;
const CELL_W: f32 = 132.0;
const CELL_H: f32 = 116.0;
const ICON: u32 = 60;
const RADIUS: f32 = 14.0;

fn text_color() -> Color {
    crate::theme::th().text_c()
}
fn subtext_color() -> Color {
    crate::theme::th().ink(0.55)
}

pub struct Launcher {
    apps: Vec<AppEntry>,
    shown: Vec<usize>,
    field: TextField,
    sel: usize,
    /// First visible row (whole rows keep every cell fully inside the window).
    scroll: usize,
    /// Size of the panel (the whole window, or the card in the middle of the overlay).
    size: (u32, u32),
    /// Top-left of the panel on the surface (non-zero only in overlay mode).
    origin: (f32, f32),
    overlay: bool,
    rounded: bool,
    fonts: Option<Fonts>,
    icons: IconCache,
    dux: Option<dux::State>,
    style: LauncherStyle,
    /// How declaunch was invoked (the panel applet's position, when it says).
    inv: Invocation,
    /// The panel in surface pixels, once the screen is known.
    anchor: Option<Anchor>,
    /// X11: shown as an override-redirect popup on the monitor at `origin_root`.
    popup: bool,
    origin_root: (f32, f32),
    /// The surface or monitor size last reported.
    screen: (u32, u32),
    scale: f32,
    /// The Windows, GNOME, KDE, Xfce and BlackBerry styles.
    styled: Option<styled::Styled>,
}

/// `DECEBAL_SCALE` when set (a forced UI scale), else `scale`.
fn ui_scale(scale: f32) -> f32 {
    std::env::var("DECEBAL_SCALE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(scale).clamp(0.75, 4.0)
}

impl Launcher {
    pub fn new() -> Self {
        Self::with_apps(apps::list())
    }

    pub fn with_apps(apps: Vec<AppEntry>) -> Self {
        Self::with_style(apps, LauncherStyle::current())
    }

    /// The launcher in `style` (the Windows, GNOME, KDE, Xfce and BlackBerry styles read the user's pins too).
    pub fn with_style(apps: Vec<AppEntry>, style: LauncherStyle) -> Self {
        let styled = match style {
            LauncherStyle::Default | LauncherStyle::Dux => None,
            _ => Some(styled::Styled::new(style, catalog::Catalog::load(apps.clone()))),
        };
        Self::build(if styled.is_some() { Vec::new() } else { apps }, style, styled)
    }

    fn build(apps: Vec<AppEntry>, style: LauncherStyle, styled: Option<styled::Styled>) -> Self {
        let shown = apps::filter(&apps, "");
        let mut out = Self {
            apps,
            shown,
            field: TextField::default(),
            sel: 0,
            scroll: 0,
            size: (1000, 680),
            origin: (0.0, 0.0),
            overlay: false,
            rounded: true,
            fonts: Fonts::load(),
            icons: IconCache::default(),
            dux: None,
            style,
            inv: Invocation::default(),
            anchor: None,
            popup: false,
            origin_root: (0.0, 0.0),
            screen: (0, 0),
            scale: ui_scale(1.0),
            styled,
        };
        if out.is_dux() {
            out.size = (620, 620);
            out.prepare_dux();
        }
        out
    }

    /// Where the panel's launcher button is (see [`placement`]); without one each style opens in its usual place.
    pub fn with_invocation(mut self, inv: Invocation) -> Self {
        self.inv = inv;
        self
    }

    pub fn style(&self) -> LauncherStyle {
        self.style
    }

    /// Resolves the panel anchor against the monitor the launcher is on (`screen` in surface pixels).
    fn locate(&mut self, screen: (u32, u32)) {
        self.anchor = resolve_anchor(&self.inv, self.origin_root, (screen.0 as f32, screen.1 as f32));
    }

    /// The card of the Launchpad / Dux looks for a surface `screen` big.
    fn classic_card(&self, screen: (u32, u32)) -> placement::Rect {
        placement::placement(self.style, self.anchor, (screen.0 as f32, screen.1 as f32), self.scale)
    }

    fn cols(&self) -> usize {
        if self.is_dux() {
            return 1;
        }
        (((self.size.0 as f32 - 2.0 * PAD) / CELL_W).floor() as usize).max(1)
    }

    fn visible_rows(&self) -> usize {
        if self.is_dux() {
            return self.dux.as_ref().map_or(1, |state| state.layout.rows);
        }
        (((self.size.1 as f32 - GRID_Y - PAD) / CELL_H).floor() as usize).max(1)
    }

    fn total_rows(&self) -> usize {
        self.shown.len().div_ceil(self.cols())
    }

    /// Left edge of the centred grid.
    fn grid_x(&self) -> f32 {
        let used = self.cols() as f32 * CELL_W;
        ((self.size.0 as f32 - used) / 2.0).max(0.0)
    }

    fn cell_rect(&self, i: usize) -> (f32, f32, f32, f32) {
        if self.is_dux() {
            return self.dux.as_ref().map_or((0.0, 0.0, 0.0, 0.0), |state| state.layout.cell(i, self.scroll));
        }
        let (c, r) = (i % self.cols(), i / self.cols());
        (self.grid_x() + c as f32 * CELL_W, GRID_Y + (r as f32 - self.scroll as f32) * CELL_H, CELL_W, CELL_H)
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        if self.is_dux() {
            return self.dux.as_ref().and_then(|state| state.layout.hit(x, y, self.scroll, self.shown.len()));
        }
        if y < GRID_Y {
            return None;
        }
        let gx = self.grid_x();
        if x < gx || x >= gx + self.cols() as f32 * CELL_W {
            return None;
        }
        let (c, r) = (((x - gx) / CELL_W) as usize, ((y - GRID_Y) / CELL_H) as usize + self.scroll);
        let i = r * self.cols() + c;
        (r < self.scroll + self.visible_rows() && i < self.shown.len()).then_some(i)
    }

    fn clamp_scroll(&mut self) {
        let max = self.total_rows().saturating_sub(self.visible_rows());
        self.scroll = self.scroll.min(max);
    }

    fn reveal_selection(&mut self) {
        let row = self.sel / self.cols();
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + self.visible_rows() {
            self.scroll = row + 1 - self.visible_rows();
        }
        self.clamp_scroll();
        self.prepare_dux();
    }

    fn refilter(&mut self) {
        self.shown = apps::filter(&self.apps, self.field.text());
        self.sel = 0;
        self.scroll = 0;
        self.prepare_dux();
    }

    fn launch_selected(&mut self) -> Response {
        match self.shown.get(self.sel).and_then(|&i| self.apps.get(i)) {
            Some(a) if launch::spawn_app(a) => Response::act(Action::Close),
            _ => Response::NONE,
        }
    }

    fn move_sel(&mut self, delta: isize) -> Response {
        if self.shown.is_empty() {
            return Response::NONE;
        }
        let n = self.shown.len() as isize;
        self.sel = (self.sel as isize + delta).clamp(0, n - 1) as usize;
        self.reveal_selection();
        Response::REDRAW
    }

    fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, w))
    }

    fn fit(&self, s: &str, px: f32, max_w: f32) -> String {
        if self.measure(s, px, Weight::Regular) <= max_w {
            return s.to_string();
        }
        let mut t: String = s.to_string();
        while !t.is_empty() && self.measure(&format!("{t}…"), px, Weight::Regular) > max_w {
            t.pop();
        }
        format!("{}…", t.trim_end())
    }

    #[allow(clippy::too_many_arguments)] // a drawing helper: target, text, position, font size, weight, colour
    fn text_at(&mut self, pm: &mut Pixmap, s: &str, x: f32, cy: f32, px: f32, weight: Weight, c: Color) {
        if let Some(f) = &mut self.fonts {
            f.draw(pm, s, x, cy - px * 0.62, px, weight, c);
        }
    }
}

impl Launcher {
    fn draw_panel(&mut self, pm: &mut Pixmap) {
        if self.is_dux() {
            self.draw_dux(pm);
            return;
        }
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        pm.fill(Color::TRANSPARENT);
        let radius = if self.rounded || self.overlay { RADIUS * crate::theme::th().radius / 10.0 } else { 0.0 };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, crate::theme::th().overlay_c());

        // Search field.
        let (sx, sw) = (PAD, w - 2.0 * PAD);
        fill_rounded(pm, sx, SEARCH_Y, sw, SEARCH_H, 9.0, crate::theme::th().ink(0.08));
        stroke_rounded(pm, sx + 1.0, SEARCH_Y + 1.0, sw - 2.0, SEARCH_H - 2.0, 8.0, crate::theme::th().accent.color(0.8), 2.0);
        let cy = SEARCH_Y + SEARCH_H / 2.0;
        if self.field.text().is_empty() {
            self.text_at(pm, "Search apps", sx + 14.0, cy, 14.0, Weight::Regular, subtext_color());
        } else {
            let t = self.field.text().to_string();
            self.text_at(pm, &t, sx + 14.0, cy, 14.0, Weight::Regular, text_color());
            let cx = (sx + 14.0 + self.measure(self.field.before_cursor(), 14.0, Weight::Regular)).round();
            crate::gfx::fill_rect(pm, cx, cy - 9.0, 1.0, 18.0, text_color());
        }

        // Grid.
        let first = self.scroll * self.cols();
        let last = ((self.scroll + self.visible_rows()) * self.cols()).min(self.shown.len());
        for pos in first..last {
            let (x, y, cw, ch) = self.cell_rect(pos);
            if pos == self.sel {
                fill_rounded(pm, x + 4.0, y + 2.0, cw - 8.0, ch - 4.0, 12.0, crate::theme::th().ink(0.12));
            }
            let Some(app) = self.shown.get(pos).and_then(|&i| self.apps.get(i)).cloned() else { continue };
            let icon = self.icons.get(&app.icon, ICON);
            blit_scaled(pm, icon, (x + (cw - ICON as f32) / 2.0).round(), (y + 12.0).round(), ICON as f32, ICON as f32, 1.0);
            let label = self.fit(&app.name, 12.0, cw - 16.0);
            let tw = self.measure(&label, 12.0, Weight::Regular);
            self.text_at(pm, &label, x + (cw - tw) / 2.0, y + 12.0 + ICON as f32 + 18.0, 12.0, Weight::Regular, text_color());
        }
        if self.shown.is_empty() {
            let msg = "No matching apps";
            let tw = self.measure(msg, 14.0, Weight::Regular);
            self.text_at(pm, msg, (w - tw) / 2.0, GRID_Y + 40.0, 14.0, Weight::Regular, subtext_color());
        }
        stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), crate::theme::th().ink(0.12), 1.0);
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }

    fn motion(&mut self, x: f32, y: f32) -> Response {
        match self.hit(x, y) {
            Some(i) if i != self.sel => {
                self.sel = i;
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    fn click(&mut self, x: f32, y: f32) -> Response {
        if self.is_dux() && (12.0..60.0).contains(&y) && x >= self.size.0 as f32 - 52.0 && x < self.size.0 as f32 - 12.0 && !self.field.text().is_empty() {
            self.field.clear();
            self.refilter();
            return Response::REDRAW;
        }
        match self.hit(x, y) {
            Some(i) => {
                self.sel = i;
                self.launch_selected()
            }
            None => Response::NONE,
        }
    }
}

impl Default for Launcher {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowApp for Launcher {
    fn title(&self) -> String {
        if self.styled.is_some() {
            return self.style.title().into();
        }
        if self.is_dux() {
            "Launcher".into()
        } else {
            "Launchpad".into()
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        match &self.styled {
            Some(st) => st.natural_size(self.size),
            None => self.size,
        }
    }

    fn set_popup(&mut self, popup: bool) {
        self.popup = popup;
    }

    fn popup_monitor(&self) -> crate::window_app::MonitorHint {
        // --output first, else the monitor under the applet, else the panels' monitor
        crate::window_app::MonitorHint { output: self.inv.output.clone(), point: self.inv.point.map(|(x, y)| (x.round() as i32, y.round() as i32)) }
    }

    fn set_popup_origin(&mut self, x: i32, y: i32) {
        self.origin_root = (x as f32, y as f32);
    }

    fn set_popup_alpha(&mut self, alpha: bool) {
        if let Some(st) = self.styled.as_mut() {
            st.alpha = alpha;
        }
    }

    fn popup_fullscreen(&self) -> bool {
        self.styled.is_some()
    }

    fn popup_translucent(&self) -> bool {
        self.styled.is_some()
    }

    fn set_scale(&mut self, scale: f32) {
        self.scale = ui_scale(scale);
        if let Some(st) = self.styled.as_mut() {
            st.scale = self.scale;
        }
    }

    fn content_size(&self) -> Option<(u32, u32)> {
        // An X11 Launchpad / Dux popup opened from a panel takes the card size placement() gives it.
        (self.styled.is_none() && self.popup && self.anchor.is_some()).then(|| {
            let r = self.classic_card(self.screen);
            (r.w as u32, r.h as u32)
        })
    }

    fn popup_anchor(&self) -> Option<crate::window_app::PopupAnchor> {
        (self.styled.is_none() && self.popup && self.anchor.is_some()).then(|| {
            // PopupAnchor's top-edge form places any rectangle that keeps the 8 px margin exactly.
            let r = self.classic_card(self.screen);
            crate::window_app::PopupAnchor { top: r.y as i32, center_x: Some((r.x + r.w / 2.0) as i32), edge: crate::window_app::Edge::Top }
        })
    }

    fn app_id(&self) -> &'static str {
        "declaunch"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn set_overlay(&mut self, overlay: bool) {
        self.overlay = overlay;
        if let Some(st) = self.styled.as_mut() {
            st.alpha = true;
        }
    }

    fn set_waker(&mut self, waker: Waker) {
        if let Some(st) = self.styled.as_mut() {
            st.set_waker(waker.clone());
        }
        crate::theme::live::start_watcher(move || waker());
    }

    fn popup_centered(&self) -> bool {
        self.is_dux()
    }

    fn set_screen(&mut self, w: u32, h: u32) {
        self.screen = (w, h);
        self.locate((w, h));
        if self.is_dux() {
            self.size = (w.saturating_sub(100).clamp(1, 620), h.saturating_sub(100).clamp(1, 620));
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if let Some(st) = self.styled.as_mut() {
            // A full-screen surface (Wayland overlay, X11 popup): the card goes where placement() says.
            st.full = self.overlay || self.popup;
            if st.full {
                self.screen = (w, h);
                self.anchor = resolve_anchor(&self.inv, self.origin_root, (w as f32, h as f32));
            }
            st.anchor = self.anchor;
            st.scale = self.scale;
            st.resize(w, h);
            return;
        }
        if self.overlay {
            self.screen = (w, h);
            self.locate((w, h));
            let r = self.classic_card((w, h));
            self.size = (r.w as u32, r.h as u32);
            self.origin = (r.x, r.y);
        } else {
            self.size = (w, h);
            self.origin = (0.0, 0.0);
        }
        self.prepare_dux();
        self.clamp_scroll();
        self.reveal_selection();
    }

    fn set_rounded(&mut self, rounded: bool) {
        self.rounded = rounded;
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        if let Some(st) = self.styled.as_mut() {
            return st.draw(pm);
        }
        if !self.overlay {
            self.draw_panel(pm);
            return;
        }
        // Dim everything behind (the compositor blurs it when a layer rule asks for it), card in the middle.
        pm.fill(color(0, 0, 0, 0.35));
        if self.is_dux() {
            let buffer = self.dux.as_mut().and_then(|state| state.buffer.take());
            let buffer = buffer.filter(|card| card.width() == self.size.0 && card.height() == self.size.1);
            if let Some(mut card) = buffer.or_else(|| Pixmap::new(self.size.0, self.size.1)) {
                self.draw_dux(&mut card);
                pm.draw_pixmap(self.origin.0 as i32, self.origin.1 as i32, card.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
                if let Some(state) = self.dux.as_mut() {
                    state.buffer = Some(card);
                }
            }
            return;
        }
        if let Some(mut card) = Pixmap::new(self.size.0, self.size.1) {
            self.draw_panel(&mut card);
            pm.draw_pixmap(self.origin.0 as i32, self.origin.1 as i32, card.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        if let Some(st) = self.styled.as_mut() {
            return st.motion(x, y);
        }
        self.motion(x - self.origin.0, y - self.origin.1)
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if let Some(st) = self.styled.as_mut() {
            return st.button(x, y, button, pressed);
        }
        if !pressed || button != Button::Left {
            return Response::NONE;
        }
        let (px, py) = (x - self.origin.0, y - self.origin.1);
        if self.overlay && (px < 0.0 || py < 0.0 || px >= self.size.0 as f32 || py >= self.size.1 as f32) {
            // A click on the dimmed background dismisses the Launchpad.
            return Response::act(Action::Close);
        }
        self.click(px, py)
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> Response {
        if let Some(st) = self.styled.as_mut() {
            return st.scroll(x, y, dy);
        }
        if self.is_dux() {
            let (x, y) = (x - self.origin.0, y - self.origin.1);
            if x < 12.0 || x >= self.size.0 as f32 - 12.0 || y < 100.0 || y >= self.size.1 as f32 - 32.0 {
                return Response::NONE;
            }
        }
        let before = self.scroll;
        if dy > 0.0 {
            self.scroll += 1;
        } else if dy < 0.0 {
            self.scroll = self.scroll.saturating_sub(1);
        }
        self.clamp_scroll();
        self.prepare_dux();
        if self.scroll != before {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        if let Some(st) = self.styled.as_mut() {
            return st.key(ev);
        }
        let cols = self.cols() as isize;
        match ev.key {
            Key::Escape => Response::act(Action::Close),
            Key::Enter => self.launch_selected(),
            Key::Left if self.is_dux() => {
                self.field.left();
                Response::REDRAW
            }
            Key::Right if self.is_dux() => {
                self.field.right();
                Response::REDRAW
            }
            Key::Tab if self.is_dux() => Response::NONE,
            Key::Left => self.move_sel(-1),
            Key::Right | Key::Tab => self.move_sel(1),
            Key::Up => self.move_sel(-cols),
            Key::Down => self.move_sel(cols),
            Key::PageUp => self.move_sel(-(cols * self.visible_rows() as isize)),
            Key::PageDown => self.move_sel(cols * self.visible_rows() as isize),
            Key::Backspace => {
                self.field.backspace();
                self.refilter();
                Response::REDRAW
            }
            Key::Delete => {
                self.field.delete();
                self.refilter();
                Response::REDRAW
            }
            Key::Home => {
                self.field.home();
                Response::REDRAW
            }
            Key::End => {
                self.field.end();
                Response::REDRAW
            }
            Key::Char if !ev.ctrl && !ev.alt && !ev.text.is_empty() => {
                self.field.insert(&ev.text);
                self.refilter();
                Response::REDRAW
            }
            Key::Char => Response::NONE,
        }
    }

    fn tick(&mut self) -> Response {
        if let Some(st) = self.styled.as_mut() {
            let answer = st.tick();
            if crate::theme::take_changed() {
                st.relayout();
                return Response::REDRAW;
            }
            return answer;
        }
        if crate::theme::take_changed() || (self.is_dux() && self.dux.as_ref().is_some_and(dux::State::stale)) {
            self.prepare_dux();
            self.clamp_scroll();
            self.reveal_selection();
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppEntry {
        AppEntry { name: name.into(), exec: "true".into(), ..Default::default() }
    }

    fn key(k: Key, text: &str) -> KeyEvent {
        KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift: false, alt: false }
    }

    fn launcher(n: usize) -> Launcher {
        let apps = (0..n).map(|i| app(&format!("App {i:03}"))).collect();
        let mut l = Launcher::with_apps(apps);
        l.fonts = None;
        l
    }

    #[test]
    fn typing_filters_and_resets_selection() {
        let mut l = Launcher::with_apps(vec![app("Files"), app("Firefox"), app("Terminal")]);
        l.fonts = None;
        l.key(&key(Key::Char, "t"));
        assert_eq!(l.shown, apps::filter(&l.apps, "t"));
        assert_eq!(l.shown.first(), Some(&2), "the name starting with t comes first");
        l.key(&key(Key::Char, "z"));
        assert!(l.shown.is_empty());
        l.key(&key(Key::Backspace, ""));
        assert!(!l.shown.is_empty());
        assert_eq!(l.sel, 0);
    }

    #[test]
    fn arrows_move_by_cells_and_rows_and_scroll_into_view() {
        let mut l = launcher(200);
        l.resize(1000, 680);
        let cols = l.cols();
        l.key(&key(Key::Down, ""));
        assert_eq!(l.sel, cols);
        for _ in 0..20 {
            l.key(&key(Key::Down, ""));
        }
        assert!(l.scroll > 0, "selection scrolled into view");
        let (_, y, _, h) = l.cell_rect(l.sel);
        assert!(y >= GRID_Y && y + h <= l.size.1 as f32);
        let before = l.sel;
        l.key(&key(Key::Left, ""));
        assert_eq!(l.sel, before - 1);
    }

    #[test]
    fn selection_stays_within_bounds() {
        let mut l = launcher(5);
        l.key(&key(Key::Up, ""));
        assert_eq!(l.sel, 0);
        for _ in 0..30 {
            l.key(&key(Key::Right, ""));
        }
        assert_eq!(l.sel, 4);
    }

    #[test]
    fn clicking_a_cell_launches_and_closes() {
        let mut l = launcher(3);
        let (x, y, w, h) = l.cell_rect(1);
        let r = l.pointer_button(x + w / 2.0, y + h / 2.0, Button::Left, true);
        assert_eq!(r.action, Some(Action::Close));
        assert_eq!(l.sel, 1);
        assert_eq!(l.pointer_button(5.0, 5.0, Button::Left, true), Response::NONE);
    }

    #[test]
    fn escape_closes_and_empty_enter_does_nothing() {
        let mut l = launcher(0);
        assert_eq!(l.key(&key(Key::Escape, "")).action, Some(Action::Close));
        assert_eq!(l.key(&key(Key::Enter, "")), Response::NONE);
    }

    #[test]
    fn overlay_centres_a_card_and_background_clicks_close() {
        let mut l = launcher(30);
        l.set_overlay(true);
        l.resize(3440, 1440);
        assert_eq!(l.size, (1100, 720));
        assert_eq!(l.origin, ((3440.0 - 1100.0) / 2.0, (1440.0 - 720.0) / 2.0));
        assert_eq!(l.pointer_button(5.0, 5.0, Button::Left, true).action, Some(Action::Close));
        let (x, y, w, h) = l.cell_rect(0);
        let r = l.pointer_button(l.origin.0 + x + w / 2.0, l.origin.1 + y + h / 2.0, Button::Left, true);
        assert_eq!(r.action, Some(Action::Close), "a cell click launches and closes");
        let mut pm = Pixmap::new(3440, 1440).unwrap();
        l.draw(&mut pm);
        assert!(pm.pixel(5, 5).unwrap().alpha() > 0, "background is dimmed");
    }

    #[test]
    fn small_screens_shrink_the_card() {
        let mut l = launcher(3);
        l.set_overlay(true);
        l.resize(800, 600);
        assert_eq!(l.size, (720, 480));
    }

    #[test]
    fn draws_a_grid_without_fonts() {
        let mut l = launcher(40);
        let mut pm = Pixmap::new(1000, 680).unwrap();
        l.draw(&mut pm);
        assert!(pm.pixel(500, 300).unwrap().alpha() > 0);
    }

    fn styled_launcher(style: LauncherStyle, n: usize) -> Launcher {
        let mut l = Launcher::build(Vec::new(), style, Some(styled::tests::styled(style, n)));
        l.fonts = None;
        l
    }

    #[test]
    fn styled_launchers_cover_the_output_and_put_the_card_where_placement_says() {
        for style in styled::tests::STYLES {
            for (scale, screen) in [(1.0, (1920, 1080)), (2.0, (1024, 600))] {
                let inv = match placement::parse_args("toggle --edge left --anchor 20,500 --thickness 40".split(' ').map(String::from)) {
                    placement::Parsed::Run(i) => i,
                    other => panic!("{other:?}"),
                };
                let mut l = styled_launcher(style, 40).with_invocation(inv);
                // the X11 order: scale, popup, monitor, then the full-monitor window
                l.set_scale(scale);
                l.set_popup(true);
                l.set_screen(screen.0, screen.1);
                assert!(l.popup_fullscreen() && l.popup_translucent(), "{style:?}");
                l.set_popup_alpha(false);
                l.resize(screen.0, screen.1);
                let a = l.anchor.expect("the panel anchor");
                assert_eq!((a.edge, a.thickness), (placement::Edge::Left, 40.0));
                let want = placement::placement(style, Some(a), (screen.0 as f32, screen.1 as f32), scale);
                let st = l.styled.as_ref().unwrap();
                assert_eq!(st.card, want, "{style:?}");
                assert!(!st.alpha, "no compositor: the card is painted opaque");
                assert_eq!(l.pointer_button(5.0, screen.1 as f32 / 2.0, Button::Left, true).action, Some(Action::Close), "{style:?}: a click on the panel strip closes");
                assert_eq!(l.key(&key(Key::Escape, "")).action, Some(Action::Close));
            }
        }
    }

    #[test]
    fn the_x11_monitor_follows_output_then_anchor_and_anchors_become_monitor_local() {
        let inv = match placement::parse_args("--output HDMI-1 --edge bottom --anchor 2880,1060 --thickness 40".split(' ').map(String::from)) {
            placement::Parsed::Run(i) => i,
            other => panic!("{other:?}"),
        };
        let mut l = styled_launcher(LauncherStyle::Kde, 10).with_invocation(inv);
        assert_eq!(l.popup_monitor(), crate::window_app::MonitorHint { output: Some("HDMI-1".into()), point: Some((2880, 1060)) });
        // the backend opened on the monitor at x = 1920: the applet is 960 px into it
        l.set_popup_origin(1920, 0);
        l.set_popup(true);
        l.set_screen(1920, 1080);
        l.resize(1920, 1080);
        assert_eq!(l.anchor.unwrap().point, Some((960.0, 1060.0)));
        assert_eq!(Launcher::build(Vec::new(), LauncherStyle::Default, None).popup_monitor(), crate::window_app::MonitorHint::default());
    }

    #[test]
    fn x11_launchpad_and_dux_popups_follow_the_panel_anchor() {
        use crate::window_app::PopupAnchor;
        for style in [LauncherStyle::Default, LauncherStyle::Dux] {
            for (edge, point) in [("bottom", "960,1060"), ("top", "100,14"), ("left", "16,900"), ("right", "1904,40")] {
                let inv = match placement::parse_args(["--edge", edge, "--anchor", point, "--thickness", "36"].map(String::from)) {
                    placement::Parsed::Run(i) => i,
                    other => panic!("{other:?}"),
                };
                let mut l = Launcher::build((0..20).map(|i| app(&format!("App {i}"))).collect(), style, None).with_invocation(inv);
                l.fonts = None;
                l.set_popup(true);
                l.set_screen(1920, 1080);
                let want = placement::placement(style, l.anchor, (1920.0, 1080.0), 1.0);
                assert_eq!(l.content_size(), Some((want.w as u32, want.h as u32)), "{style:?} {edge}");
                let a: PopupAnchor = l.popup_anchor().unwrap();
                // the X11 backend's own arithmetic puts the window exactly on the card
                assert_eq!((a.left(1920, want.w as i32), a.y(1080, want.h as i32)), (want.x as i32, want.y as i32), "{style:?} {edge}");
            }
            // no panel: the original placement is untouched
            let mut l = Launcher::build(vec![app("A")], style, None);
            l.set_popup(true);
            l.set_screen(1920, 1080);
            assert_eq!((l.content_size(), l.popup_anchor()), (None, None));
        }
    }

    /// `THEME_SHOTS=dir cargo test --lib -- --ignored launchpad_in_three_themes` writes the Launchpad in three themes.
    #[test]
    #[ignore]
    fn launchpad_in_three_themes() {
        use crate::theme::{builtin, model::Selection};
        let Some(dir) = std::env::var_os("THEME_SHOTS") else { return };
        for (id, dark, tag) in [("macos", true, "dark"), ("macos", false, "light"), ("windows11", true, "windows")] {
            crate::theme::live::publish_theme(builtin::def(id).unwrap().resolve(id, dark, &Selection::default()));
            let mut l = launcher(40);
            let mut pm = Pixmap::new(1000, 680).unwrap();
            pm.fill(tiny_skia::Color::from_rgba8(70, 96, 122, 255));
            l.draw(&mut pm);
            pm.save_png(std::path::Path::new(&dir).join(format!("13-{tag}-launchpad.png"))).unwrap();
        }
    }
}
