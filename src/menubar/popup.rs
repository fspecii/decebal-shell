//! Popups opened from the bar's status items: a list popup (layouts, sound, Wi-Fi, Bluetooth,
//! battery) and a month calendar. Both are drawn into the bar's own surface, whose input region
//! follows what is open, so they need no extra windows. This module is pure model + drawing.

use super::rdwm;
use crate::controlcenter;
use crate::gfx::{color, fill_circle, fill_rect, fill_rounded, stroke_rounded};
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform};

const TEXT_PX: f32 = 12.0;
const PAD: f32 = 5.0;

/// What a row does when clicked.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    IndicatorOpen(u64),
    IndicatorActivate(u64),
    IndicatorEvent {
        key: u64,
        id: i32,
    },
    IndicatorSubmenu(i32),
    IndicatorBack,
    IndicatorPage(isize),
    None,
    SetLayout(String),
    IncMaster(i32),
    Mfact(f32),
    ToggleGaps,
    ToggleFloat,
    /// Close / minimise the focused window (the vertical bar's window popout).
    CloseWindow,
    MinimizeWindow,
    /// Turn Stay Awake off / on for a while (`None` = until turned off), see [`super::awake`].
    AwakeOff,
    AwakeFor(Option<u64>),
    VolumeSlider,
    BrightnessSlider,
    ToggleMute,
    WifiToggle,
    WifiConnect(String),
    NetworkEditor,
    /// Fetch the weather again now (the weather popup's Refresh row).
    WeatherRefresh,
    BtToggle,
    BtDevice {
        mac: String,
        connect: bool,
    },
    /// The folded application menu: show menu `n`, back to the list of menus, run item (menu, item).
    MenuOpen(usize),
    MenuBack,
    MenuRun(usize, usize),
    /// The media applet's transport.
    MediaPrev,
    MediaToggle,
    MediaNext,
    /// Opens the dashboard's media page.
    MediaOpen,
    /// A task button's menu.
    TaskFocus(u64),
    TaskMinimize(u64),
    TaskClose(u64),
    /// A row of the overflow list: does what pressing that applet does.
    OpenApplet(super::Tray),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Icon {
    None,
    Layout(String),
    Symbol(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Item,
    Header,
    Separator,
    Text,
    /// Value 0..=1.
    Slider(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub kind: Kind,
    pub label: String,
    pub detail: String,
    pub icon: Icon,
    pub checked: bool,
    pub enabled: bool,
    pub act: Act,
}

impl Row {
    fn base(kind: Kind, label: &str) -> Row {
        Row { kind, label: label.to_string(), detail: String::new(), icon: Icon::None, checked: false, enabled: true, act: Act::None }
    }
    pub fn header(label: &str) -> Row {
        Row::base(Kind::Header, label)
    }
    pub fn sep() -> Row {
        Row::base(Kind::Separator, "")
    }
    pub fn text(label: &str) -> Row {
        Row::base(Kind::Text, label)
    }
    pub fn item(label: &str, act: Act) -> Row {
        Row { act, ..Row::base(Kind::Item, label) }
    }
    pub fn slider(value: f32, icon: &'static str, act: Act) -> Row {
        Row { act, icon: Icon::Symbol(icon), ..Row::base(Kind::Slider(value), "") }
    }
    pub fn checked(mut self, on: bool) -> Row {
        self.checked = on;
        self
    }
    pub fn detail(mut self, d: &str) -> Row {
        self.detail = d.to_string();
        self
    }
    pub fn icon(mut self, i: Icon) -> Row {
        self.icon = i;
        self
    }
    pub fn enabled(mut self, on: bool) -> Row {
        self.enabled = on;
        self
    }
    fn interactive(&self) -> bool {
        self.enabled && matches!(self.kind, Kind::Item | Kind::Slider(_)) && self.act != Act::None
    }
}

fn row_h(kind: &Kind, compact: bool) -> f32 {
    match kind {
        Kind::Item => {
            if compact {
                22.0
            } else {
                24.0
            }
        }
        Kind::Header => 24.0,
        Kind::Separator => 9.0,
        Kind::Text => 22.0,
        Kind::Slider(_) => 34.0,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Popup {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rows: Vec<Row>,
    ys: Vec<f32>,
    compact: bool,
}

impl Popup {
    /// Lays `rows` out with the popup's top-left at (`x`, `y`).
    pub fn layout(rows: Vec<Row>, x: f32, y: f32, w: f32, compact: bool) -> Popup {
        let mut ys = Vec::with_capacity(rows.len());
        let mut cy = PAD;
        for r in &rows {
            ys.push(cy);
            cy += row_h(&r.kind, compact);
        }
        Popup { x, y, w, h: cy + PAD, rows, ys, compact }
    }

    /// Row rectangle in popup-local coordinates: (y, h).
    pub fn row_span(&self, i: usize) -> (f32, f32) {
        (self.ys[i], row_h(&self.rows[i].kind, self.compact))
    }

    /// The interactive row under (px, py) in surface coordinates.
    pub fn hit(&self, px: f32, py: f32) -> Option<usize> {
        if px < self.x || px >= self.x + self.w || py < self.y || py >= self.y + self.h {
            return None;
        }
        let ly = py - self.y;
        (0..self.rows.len()).find(|&i| {
            let (y, h) = self.row_span(i);
            self.rows[i].interactive() && ly >= y && ly < y + h
        })
    }

    /// Horizontal extent of a slider row's track in surface coordinates: (x, w).
    pub fn slider_track(&self) -> (f32, f32) {
        (self.x + 14.0 + 22.0, self.w - 28.0 - 22.0)
    }

    /// Slider value for pointer x.
    pub fn slider_value(&self, px: f32) -> f32 {
        let (x, w) = self.slider_track();
        controlcenter::slider_value(px, x, w)
    }

    pub fn draw(&self, pm: &mut Pixmap, fonts: &mut Fonts, symbols: &mut SymbolCache, hover: Option<usize>) {
        fill_rounded(pm, self.x, self.y, self.w, self.h, crate::theme::th().radius, crate::theme::th().popup_c());
        stroke_rounded(pm, self.x + 0.5, self.y + 0.5, self.w - 1.0, self.h - 1.0, crate::theme::th().radius - 0.5, crate::theme::th().outline_c(), 1.0);
        for (i, r) in self.rows.iter().enumerate() {
            let (ly, h) = self.row_span(i);
            let y = self.y + ly;
            let hovered = hover == Some(i) && r.interactive() && !matches!(r.kind, Kind::Slider(_));
            let alpha = if r.enabled { 1.0 } else { 0.4 };
            let top = y + (h - TEXT_PX) / 2.0 - 1.0;
            match &r.kind {
                Kind::Separator => fill_rect(pm, self.x + 8.0, y + h / 2.0, self.w - 16.0, 1.0, crate::theme::th().ink(0.12)),
                Kind::Header => {
                    let room = if r.detail.is_empty() { self.w - 24. } else { self.w * 0.4 };
                    let shown = controlcenter::fit(&r.label, room, TEXT_PX, Weight::Bold, fonts);
                    fonts.draw(pm, &shown, self.x + 12.0, top, TEXT_PX, Weight::Bold, crate::theme::th().text_c());
                    if !r.detail.is_empty() {
                        let shown = controlcenter::fit(&r.detail, self.w * 0.55, 11.0, Weight::Regular, fonts);
                        let dw = fonts.measure(&shown, 11.0, Weight::Regular);
                        fonts.draw(pm, &shown, self.x + self.w - 12.0 - dw, top + 0.5, 11.0, Weight::Regular, crate::theme::th().ink(0.55));
                    }
                }
                Kind::Text => {
                    let shown = controlcenter::fit(&r.label, self.w - 24.0, TEXT_PX, Weight::Regular, fonts);
                    fonts.draw(pm, &shown, self.x + 12.0, top, TEXT_PX, Weight::Regular, crate::theme::th().ink(0.55));
                }
                Kind::Slider(v) => {
                    let cy = y + h / 2.0;
                    if let Icon::Symbol(name) = &r.icon {
                        if let Some(sym) = symbols.get(name, crate::theme::th().text.t(), 32) {
                            crate::gfx::blit_scaled(pm, sym, self.x + 12.0, cy - 8.0, 16.0, 16.0, 1.0);
                        }
                    }
                    let (sx, sw) = self.slider_track();
                    fill_rounded(pm, sx, cy - 3.0, sw, 6.0, 3.0, crate::theme::th().ink(0.2));
                    let fw = (sw * v.clamp(0.0, 1.0)).max(0.0);
                    if fw > 0.5 {
                        fill_rounded(pm, sx, cy - 3.0, fw.max(6.0), 6.0, 3.0, crate::theme::th().accent_c());
                    }
                    fill_circle(pm, sx + fw, cy, 8.0, crate::theme::th().text_c());
                }
                Kind::Item => {
                    if hovered {
                        fill_rounded(pm, self.x + PAD, y, self.w - 2.0 * PAD, h, crate::theme::th().r(0.5), crate::theme::th().accent_c());
                    }
                    let th = crate::theme::th();
                    let on = if hovered { th.accent_text } else { th.text };
                    let (main, sub) = if hovered { (on.a(1.0), on.a(0.85)) } else { (on.a(alpha), on.a(0.45 * alpha)) };
                    let ink = on.t();
                    if r.checked {
                        draw_check(pm, self.x + 17.0, y + h / 2.0, ink);
                    }
                    let mut lx = self.x + 28.0;
                    match &r.icon {
                        Icon::Layout(name) => {
                            draw_layout_icon(pm, name, lx, y + h / 2.0 - 7.0, 22.0, 14.0, (ink.0, ink.1, ink.2, 0.9 * alpha));
                            lx += 31.0;
                        }
                        Icon::Symbol(name) => {
                            if let Some(sym) = symbols.get(name, ink, 28) {
                                crate::gfx::blit_scaled(pm, sym, lx, y + h / 2.0 - 7.0, 14.0, 14.0, alpha);
                            }
                            lx += 22.0;
                        }
                        Icon::None => {}
                    }
                    let detail_w = if r.detail.is_empty() { 0.0 } else { fonts.measure(&r.detail, 11.0, Weight::Regular) + 8.0 };
                    let shown = controlcenter::fit(&r.label, self.x + self.w - 12.0 - detail_w - lx, TEXT_PX, Weight::Regular, fonts);
                    fonts.draw(pm, &shown, lx, top, TEXT_PX, Weight::Regular, color(main.0, main.1, main.2, main.3));
                    if !r.detail.is_empty() {
                        fonts.draw(pm, &r.detail, self.x + self.w - 12.0 - detail_w + 8.0, top + 0.5, 11.0, Weight::Regular, color(sub.0, sub.1, sub.2, sub.3));
                    }
                }
            }
        }
    }
}

fn draw_check(pm: &mut Pixmap, cx: f32, cy: f32, rgb: (u8, u8, u8)) {
    let mut pb = PathBuilder::new();
    pb.move_to(cx - 4.0, cy + 0.5);
    pb.line_to(cx - 1.2, cy + 3.5);
    pb.line_to(cx + 4.2, cy - 3.5);
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
    paint.anti_alias = true;
    let stroke = Stroke { width: 1.8, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// Draws the wireframe of layout `name` in the box (x, y, w, h).
pub fn draw_layout_icon(pm: &mut Pixmap, name: &str, x: f32, y: f32, w: f32, h: f32, rgba: (u8, u8, u8, f32)) {
    let c = color(rgba.0, rgba.1, rgba.2, rgba.3);
    // Outline of the screen, cells inside it.
    stroke_rounded(pm, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 2.0, color(rgba.0, rgba.1, rgba.2, rgba.3 * 0.55), 1.0);
    let (ix, iy, iw, ih) = (x + 2.0, y + 2.0, w - 4.0, h - 4.0);
    for (cx, cy, cw, ch) in rdwm::layout_cells(name) {
        let (x0, x1) = ((cx).max(0.0), (cx + cw).min(1.0));
        if x1 <= x0 {
            continue;
        }
        fill_rounded(pm, ix + x0 * iw, iy + cy * ih, ((x1 - x0) * iw).max(1.0), (ch * ih).max(1.0), 0.8, c);
    }
}

// ---------------------------------------------------------------- calendar

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

pub fn days_in_month(year: i32, mon0: u32) -> u32 {
    match mon0 {
        0 | 2 | 4 | 6 | 7 | 9 | 11 => 31,
        3 | 5 | 8 | 10 => 30,
        _ => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Weekday of a date, 0 = Monday ... 6 = Sunday (Sakamoto's method).
pub fn weekday_mon0(year: i32, mon0: u32, day: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if mon0 < 2 { year - 1 } else { year };
    let sun0 = (y + y / 4 - y / 100 + y / 400 + T[mon0 as usize % 12] + day as i32).rem_euclid(7);
    ((sun0 + 6) % 7) as u32
}

/// The month as 6 rows of 7 cells (Monday first); `None` outside the month.
pub fn month_grid(year: i32, mon0: u32) -> [[Option<u32>; 7]; 6] {
    let mut g = [[None; 7]; 6];
    let first = weekday_mon0(year, mon0, 1) as usize;
    for d in 1..=days_in_month(year, mon0) {
        let i = first + d as usize - 1;
        g[i / 7][i % 7] = Some(d);
    }
    g
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalHit {
    Prev,
    Next,
    Today,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Calendar {
    pub x: f32,
    pub y: f32,
    pub year: i32,
    pub mon0: u32,
    /// (year, month 0-based, day)
    pub today: (i32, u32, u32),
}

impl Calendar {
    pub const W: f32 = 252.0;
    pub const H: f32 = 62.0 + 6.0 * 28.0 + 12.0;

    pub fn new(x: f32, y: f32, today: (i32, u32, u32)) -> Self {
        Calendar { x, y, year: today.0, mon0: today.1, today }
    }

    pub fn shift(&mut self, months: i32) {
        let n = self.year * 12 + self.mon0 as i32 + months;
        self.year = n.div_euclid(12);
        self.mon0 = n.rem_euclid(12) as u32;
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + Self::W && py >= self.y && py < self.y + Self::H
    }

    /// Navigation targets: the arrows right of the title and the title itself (back to today).
    pub fn hit(&self, px: f32, py: f32) -> Option<CalHit> {
        let (lx, ly) = (px - self.x, py - self.y);
        if !(8.0..40.0).contains(&ly) {
            return None;
        }
        if (Self::W - 66.0..Self::W - 38.0).contains(&lx) {
            Some(CalHit::Prev)
        } else if (Self::W - 38.0..Self::W - 8.0).contains(&lx) {
            Some(CalHit::Next)
        } else if (8.0..Self::W - 66.0).contains(&lx) {
            Some(CalHit::Today)
        } else {
            None
        }
    }

    pub fn title(&self) -> String {
        format!("{} {}", MONTHS[self.mon0 as usize % 12], self.year)
    }

    pub fn draw(&self, pm: &mut Pixmap, fonts: &mut Fonts) {
        fill_rounded(pm, self.x, self.y, Self::W, Self::H, crate::theme::th().r(1.2), crate::theme::th().popup_c());
        stroke_rounded(pm, self.x + 0.5, self.y + 0.5, Self::W - 1.0, Self::H - 1.0, crate::theme::th().r(1.2) - 0.5, crate::theme::th().outline_c(), 1.0);
        fonts.draw(pm, &self.title(), self.x + 16.0, self.y + 15.0, 14.0, Weight::Bold, crate::theme::th().text_c());
        for (label, off) in [("‹", 52.0), ("›", 24.0)] {
            let w = fonts.measure(label, 18.0, Weight::Medium);
            fonts.draw(pm, label, self.x + Self::W - off - w / 2.0 - 4.0, self.y + 11.0, 18.0, Weight::Medium, crate::theme::th().text.color(0.85));
        }
        let cell_w = (Self::W - 24.0) / 7.0;
        for (i, d) in WEEKDAYS.iter().enumerate() {
            let w = fonts.measure(d, 11.0, Weight::Medium);
            let col = if i >= 5 { 0.35 } else { 0.5 };
            fonts.draw(pm, d, self.x + 12.0 + cell_w * (i as f32 + 0.5) - w / 2.0, self.y + 44.0, 11.0, Weight::Medium, crate::theme::th().ink(col));
        }
        let grid = month_grid(self.year, self.mon0);
        for (r, row) in grid.iter().enumerate() {
            for (c, d) in row.iter().enumerate() {
                let Some(d) = d else { continue };
                let cx = self.x + 12.0 + cell_w * (c as f32 + 0.5);
                let cy = self.y + 62.0 + r as f32 * 28.0 + 14.0;
                let is_today = (self.year, self.mon0, *d) == self.today;
                if is_today {
                    fill_circle(pm, cx, cy, 12.0, crate::theme::th().accent_c());
                }
                let s = d.to_string();
                let w = fonts.measure(&s, TEXT_PX, Weight::Regular);
                let a = if is_today || c < 5 { 1.0 } else { 0.55 };
                fonts.draw(
                    pm,
                    &s,
                    cx - w / 2.0,
                    cy - 7.0,
                    TEXT_PX,
                    if is_today { Weight::Bold } else { Weight::Regular },
                    if is_today { crate::theme::th().accent_text_c() } else { crate::theme::th().ink(a) },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_are_right() {
        // 2026-10-27 is a Tuesday, 2024-02-29 a Thursday, 2000-01-01 a Saturday, 1999-12-31 a Friday.
        assert_eq!(weekday_mon0(2026, 9, 27), 1);
        assert_eq!(weekday_mon0(2024, 1, 29), 3);
        assert_eq!(weekday_mon0(2000, 0, 1), 5);
        assert_eq!(weekday_mon0(1999, 11, 31), 4);
    }

    #[test]
    fn month_lengths_handle_leap_years() {
        assert_eq!(days_in_month(2024, 1), 29);
        assert_eq!(days_in_month(2100, 1), 28);
        assert_eq!(days_in_month(2000, 1), 29);
        assert_eq!(days_in_month(2026, 9), 31);
        assert_eq!(days_in_month(2026, 10), 30);
    }

    #[test]
    fn grid_places_days_under_their_weekday() {
        let g = month_grid(2026, 9); // October 2026 starts on a Thursday
        assert_eq!(g[0], [None, None, None, Some(1), Some(2), Some(3), Some(4)]);
        assert_eq!(g[4][1], Some(27));
        let n: usize = g.iter().flatten().flatten().count();
        assert_eq!(n, 31);
        // February 2027 starts on a Monday: exactly four full rows.
        let f = month_grid(2027, 1);
        assert_eq!(f[0][0], Some(1));
        assert_eq!(f[3][6], Some(28));
        assert!(f[4].iter().all(Option::is_none));
    }

    #[test]
    fn navigation_wraps_years() {
        let mut c = Calendar::new(0.0, 0.0, (2026, 9, 27));
        c.shift(3);
        assert_eq!((c.year, c.mon0), (2027, 0));
        c.shift(-1);
        assert_eq!((c.year, c.mon0), (2026, 11));
        c.shift(-12);
        assert_eq!((c.year, c.mon0), (2025, 11));
        assert_eq!(c.title(), "December 2025");
    }

    #[test]
    fn calendar_arrows_are_hit() {
        let c = Calendar::new(100.0, 30.0, (2026, 9, 27));
        assert_eq!(c.hit(100.0 + Calendar::W - 50.0, 50.0), Some(CalHit::Prev));
        assert_eq!(c.hit(100.0 + Calendar::W - 20.0, 50.0), Some(CalHit::Next));
        assert_eq!(c.hit(120.0, 50.0), Some(CalHit::Today));
        assert_eq!(c.hit(120.0, 120.0), None);
    }

    #[test]
    fn popup_hit_only_finds_enabled_actionable_rows() {
        let rows = vec![
            Row::header("H"),
            Row::item("a", Act::ToggleGaps),
            Row::sep(),
            Row::item("off", Act::ToggleFloat).enabled(false),
            Row::text("info"),
            Row::item("noact", Act::None),
            Row::slider(0.5, "speaker-2", Act::VolumeSlider),
        ];
        let p = Popup::layout(rows, 10.0, 20.0, 200.0, false);
        let mid = |i: usize| {
            let (y, h) = p.row_span(i);
            p.y + y + h / 2.0
        };
        assert_eq!(p.hit(50.0, mid(0)), None);
        assert_eq!(p.hit(50.0, mid(1)), Some(1));
        assert_eq!(p.hit(50.0, mid(3)), None);
        assert_eq!(p.hit(50.0, mid(4)), None);
        assert_eq!(p.hit(50.0, mid(5)), None);
        assert_eq!(p.hit(50.0, mid(6)), Some(6));
        assert_eq!(p.hit(5.0, mid(1)), None);
        assert_eq!(p.hit(50.0, p.y + p.h + 1.0), None);
    }

    #[test]
    fn slider_value_follows_the_track() {
        let p = Popup::layout(vec![Row::slider(0.0, "speaker-2", Act::VolumeSlider)], 100.0, 0.0, 260.0, false);
        let (x, w) = p.slider_track();
        assert_eq!(p.slider_value(x - 10.0), 0.0);
        assert_eq!(p.slider_value(x + w + 10.0), 1.0);
        assert!((p.slider_value(x + w / 2.0) - 0.5).abs() < 1e-5);
    }
}
