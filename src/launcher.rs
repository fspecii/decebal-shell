//! The Launchpad: a searchable grid of installed applications in a floating window.

use crate::app::Button;
use crate::apps::{self, AppEntry};
use crate::gfx::{blit_scaled, color, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::launch;
use crate::text::{Fonts, Weight};
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use tiny_skia::{Color, Pixmap};

const PAD: f32 = 28.0;
const SEARCH_Y: f32 = 22.0;
const SEARCH_H: f32 = 36.0;
const GRID_Y: f32 = 84.0;
const CELL_W: f32 = 132.0;
const CELL_H: f32 = 116.0;
const ICON: u32 = 60;
const RADIUS: f32 = 14.0;
const PANEL_MAX_W: u32 = 1100;
const PANEL_MAX_H: u32 = 720;

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
}

impl Launcher {
    pub fn new() -> Self {
        Self::with_apps(apps::list())
    }

    pub fn with_apps(apps: Vec<AppEntry>) -> Self {
        let shown = apps::filter(&apps, "");
        Self { apps, shown, field: TextField::default(), sel: 0, scroll: 0, size: (1000, 680), origin: (0.0, 0.0), overlay: false, rounded: true, fonts: Fonts::load(), icons: IconCache::default() }
    }

    fn cols(&self) -> usize {
        (((self.size.0 as f32 - 2.0 * PAD) / CELL_W).floor() as usize).max(1)
    }

    fn visible_rows(&self) -> usize {
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
        let (c, r) = (i % self.cols(), i / self.cols());
        (self.grid_x() + c as f32 * CELL_W, GRID_Y + (r as f32 - self.scroll as f32) * CELL_H, CELL_W, CELL_H)
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
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
    }

    fn refilter(&mut self) {
        self.shown = apps::filter(&self.apps, self.field.text());
        self.sel = 0;
        self.scroll = 0;
    }

    fn launch_selected(&mut self) -> Response {
        match self.shown.get(self.sel).and_then(|&i| self.apps.get(i)) {
            Some(a) if launch::spawn_exec(&a.exec) => Response::act(Action::Close),
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
        "Launchpad".into()
    }

    fn initial_size(&self) -> (u32, u32) {
        self.size
    }

    fn app_id(&self) -> &'static str {
        "maclaunch"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn set_overlay(&mut self, overlay: bool) {
        self.overlay = overlay;
    }

    fn set_waker(&mut self, waker: Waker) {
        crate::theme::live::start_watcher(move || waker());
    }

    fn set_screen(&mut self, _: u32, _: u32) {}

    fn resize(&mut self, w: u32, h: u32) {
        if self.overlay {
            self.size = ((w.saturating_sub(80)).clamp(300, PANEL_MAX_W), (h.saturating_sub(120)).clamp(300, PANEL_MAX_H));
            self.origin = ((w as f32 - self.size.0 as f32) / 2.0, (h as f32 - self.size.1 as f32) / 2.0);
        } else {
            self.size = (w, h);
            self.origin = (0.0, 0.0);
        }
        self.clamp_scroll();
        self.reveal_selection();
    }

    fn set_rounded(&mut self, rounded: bool) {
        self.rounded = rounded;
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        if !self.overlay {
            self.draw_panel(pm);
            return;
        }
        // Dim everything behind (the compositor blurs it when a layer rule asks for it), card in the middle.
        pm.fill(color(0, 0, 0, 0.35));
        if let Some(mut card) = Pixmap::new(self.size.0, self.size.1) {
            self.draw_panel(&mut card);
            pm.draw_pixmap(self.origin.0 as i32, self.origin.1 as i32, card.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        self.motion(x - self.origin.0, y - self.origin.1)
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
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

    fn scroll(&mut self, _: f32, _: f32, dy: f32) -> Response {
        let before = self.scroll;
        if dy > 0.0 {
            self.scroll += 1;
        } else if dy < 0.0 {
            self.scroll = self.scroll.saturating_sub(1);
        }
        self.clamp_scroll();
        if self.scroll != before {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        let cols = self.cols() as isize;
        match ev.key {
            Key::Escape => Response::act(Action::Close),
            Key::Enter => self.launch_selected(),
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
        if crate::theme::take_changed() {
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
        AppEntry { name: name.into(), exec: "true".into(), icon: String::new() }
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
        assert_eq!(l.size, (PANEL_MAX_W, PANEL_MAX_H));
        assert_eq!(l.origin, ((3440.0 - PANEL_MAX_W as f32) / 2.0, (1440.0 - PANEL_MAX_H as f32) / 2.0));
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
}
