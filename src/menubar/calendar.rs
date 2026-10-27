//! The month calendar that drops down from the bar's clock (weeks start on Monday).

use super::symbols::SymbolCache;
use crate::gfx::{blit_scaled, color, fill_circle, fill_rounded, stroke_rounded};
use crate::text::{Fonts, Weight};
use tiny_skia::Pixmap;

pub const PANEL_W: f32 = 268.0;
const PAD: f32 = 14.0;
const HEADER_H: f32 = 46.0;
const WEEKDAY_H: f32 = 22.0;
const CELL_W: f32 = (PANEL_W - 2.0 * PAD) / 7.0;
const CELL_H: f32 = 32.0;
/// Panel height: header, weekday names, six week rows.
pub const PANEL_H: f32 = HEADER_H + WEEKDAY_H + 6.0 * CELL_H + PAD;

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

/// A displayed month: `month` is 0 = January.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Month {
    pub year: i32,
    pub month: u32,
}

impl Month {
    pub fn shifted(self, delta: i32) -> Month {
        let n = self.year * 12 + self.month as i32 + delta;
        Month { year: n.div_euclid(12), month: n.rem_euclid(12) as u32 }
    }

    pub fn title(self) -> String {
        format!("{} {}", MONTHS[(self.month % 12) as usize], self.year)
    }
}

pub fn days_in_month(m: Month) -> u32 {
    match m.month {
        1 if (m.year % 4 == 0 && m.year % 100 != 0) || m.year % 400 == 0 => 29,
        1 => 28,
        3 | 5 | 8 | 10 => 30,
        _ => 31,
    }
}

/// Day of the week with Monday = 0 (Sakamoto's method).
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 2 { year - 1 } else { year };
    let sunday_first = (y + y / 4 - y / 100 + y / 400 + T[(month % 12) as usize] + day as i32).rem_euclid(7);
    ((sunday_first + 6) % 7) as u32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub day: u32,
    /// Belongs to the shown month (not the greyed-out neighbours).
    pub current: bool,
}

/// Six rows of seven cells starting on the Monday on or before the 1st.
pub fn grid(m: Month) -> [[Cell; 7]; 6] {
    let lead = weekday(m.year, m.month, 1);
    let prev_days = days_in_month(m.shifted(-1));
    let this_days = days_in_month(m);
    let mut out = [[Cell { day: 1, current: false }; 7]; 6];
    for (i, cell) in out.iter_mut().flatten().enumerate() {
        let i = i as u32;
        *cell = if i < lead {
            Cell { day: prev_days - lead + 1 + i, current: false }
        } else if i - lead < this_days {
            Cell { day: i - lead + 1, current: true }
        } else {
            Cell { day: i - lead - this_days + 1, current: false }
        };
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Prev,
    Next,
    /// The month title: jump back to the current month.
    Today,
}

const NAV_W: f32 = 28.0;

/// What lies under the panel-relative point (header buttons only).
pub fn hit(x: f32, y: f32) -> Option<Hit> {
    if !(0.0..HEADER_H).contains(&y) || !(0.0..PANEL_W).contains(&x) {
        return None;
    }
    if x >= PANEL_W - PAD - NAV_W {
        Some(Hit::Next)
    } else if x >= PANEL_W - PAD - 2.0 * NAV_W - 4.0 {
        Some(Hit::Prev)
    } else {
        Some(Hit::Today)
    }
}

pub struct View {
    pub origin: (f32, f32),
    pub month: Month,
    /// The current day, when it is in the shown month's grid.
    pub today: Option<(i32, u32, u32)>,
    pub hover: Option<Hit>,
}

pub fn draw(pm: &mut Pixmap, v: &View, fonts: &mut Fonts, symbols: &mut SymbolCache) {
    let (ox, oy) = v.origin;
    fill_rounded(pm, ox, oy, PANEL_W, PANEL_H, 12.0, color(36, 36, 41, 0.97));
    stroke_rounded(pm, ox + 0.5, oy + 0.5, PANEL_W - 1.0, PANEL_H - 1.0, 11.5, color(255, 255, 255, 0.14), 1.0);

    let text_top = |fonts: &Fonts, px: f32, cy: f32, w: Weight| cy - fonts.ascent(px, w) / 2.0 - px * 0.1;
    let title = v.month.title();
    let ty = text_top(fonts, 16.0, oy + HEADER_H / 2.0, Weight::SemiBold);
    fonts.draw(pm, &title, ox + PAD, ty, 16.0, Weight::SemiBold, color(245, 245, 247, 1.0));
    for (hit, name, x) in [(Hit::Prev, "chevron-left", PANEL_W - PAD - 2.0 * NAV_W - 4.0), (Hit::Next, "chevron-right", PANEL_W - PAD - NAV_W)] {
        if v.hover == Some(hit) {
            fill_rounded(pm, ox + x, oy + HEADER_H / 2.0 - 13.0, NAV_W, 26.0, 6.0, color(255, 255, 255, 0.14));
        }
        if let Some(s) = symbols.get(name, (222, 222, 222), 32) {
            blit_scaled(pm, s, (ox + x + NAV_W / 2.0 - 8.0).round(), (oy + HEADER_H / 2.0 - 8.0).round(), 16.0, 16.0, 1.0);
        }
    }

    let top = oy + HEADER_H;
    for (i, d) in WEEKDAYS.iter().enumerate() {
        let w = fonts.measure(d, 11.0, Weight::Medium);
        let cx = ox + PAD + (i as f32 + 0.5) * CELL_W;
        let y = text_top(fonts, 11.0, top + WEEKDAY_H / 2.0, Weight::Medium);
        fonts.draw(pm, d, cx - w / 2.0, y, 11.0, Weight::Medium, color(255, 255, 255, 0.45));
    }
    let weekdays_bottom = top + WEEKDAY_H;
    for (r, row) in grid(v.month).iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let cx = ox + PAD + (c as f32 + 0.5) * CELL_W;
            let cy = weekdays_bottom + (r as f32 + 0.5) * CELL_H;
            let is_today = cell.current && v.today == Some((v.month.year, v.month.month, cell.day));
            if is_today {
                fill_circle(pm, cx, cy, 14.0, color(10, 132, 255, 1.0));
            }
            let s = cell.day.to_string();
            let (weight, alpha) = if is_today {
                (Weight::SemiBold, 1.0)
            } else if cell.current {
                (Weight::Regular, 0.92)
            } else {
                (Weight::Regular, 0.28)
            };
            let w = fonts.measure(&s, 13.0, weight);
            let y = text_top(fonts, 13.0, cy, weight);
            fonts.draw(pm, &s, cx - w / 2.0, y, 13.0, weight, color(255, 255, 255, alpha));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_are_monday_based() {
        assert_eq!(weekday(2026, 9, 26), 0, "26 Oct 2026 is a Monday");
        assert_eq!(weekday(2026, 0, 1), 3, "1 Jan 2026 is a Thursday");
        assert_eq!(weekday(2024, 1, 29), 3, "29 Feb 2024 is a Thursday");
    }

    #[test]
    fn month_lengths_and_leap_years() {
        assert_eq!(days_in_month(Month { year: 2024, month: 1 }), 29);
        assert_eq!(days_in_month(Month { year: 2100, month: 1 }), 28);
        assert_eq!(days_in_month(Month { year: 2000, month: 1 }), 29);
        assert_eq!(days_in_month(Month { year: 2026, month: 9 }), 31);
        assert_eq!(days_in_month(Month { year: 2026, month: 10 }), 30);
    }

    #[test]
    fn shifting_wraps_years() {
        assert_eq!(Month { year: 2026, month: 0 }.shifted(-1), Month { year: 2025, month: 11 });
        assert_eq!(Month { year: 2026, month: 11 }.shifted(1), Month { year: 2027, month: 0 });
        assert_eq!(Month { year: 2026, month: 9 }.shifted(15), Month { year: 2028, month: 0 });
    }

    #[test]
    fn grid_pads_with_neighbouring_months() {
        // October 2026 starts on a Thursday: Mon 28, Tue 29, Wed 30 September lead in.
        let g = grid(Month { year: 2026, month: 9 });
        assert_eq!([g[0][0], g[0][2]], [Cell { day: 28, current: false }, Cell { day: 30, current: false }]);
        assert_eq!(g[0][3], Cell { day: 1, current: true });
        assert_eq!(g[4][5], Cell { day: 31, current: true });
        assert_eq!(g[4][6], Cell { day: 1, current: false });
        assert_eq!(g.iter().flatten().filter(|c| c.current).count(), 31);
    }

    #[test]
    fn header_hit_targets() {
        assert_eq!(hit(PANEL_W - PAD - 5.0, 20.0), Some(Hit::Next));
        assert_eq!(hit(PANEL_W - PAD - NAV_W - 8.0, 20.0), Some(Hit::Prev));
        assert_eq!(hit(30.0, 20.0), Some(Hit::Today));
        assert_eq!(hit(30.0, 100.0), None);
    }

    #[test]
    fn draws_without_fonts_panics_never() {
        let Some(mut f) = Fonts::load() else { return };
        let mut pm = Pixmap::new(300, 320).unwrap();
        let v = View { origin: (10.0, 10.0), month: Month { year: 2026, month: 9 }, today: Some((2026, 9, 26)), hover: Some(Hit::Next) };
        draw(&mut pm, &v, &mut f, &mut SymbolCache::default());
        assert!(pm.pixel(20, 20).unwrap().alpha() > 0);
    }
}
