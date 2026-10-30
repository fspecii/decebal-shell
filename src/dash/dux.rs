//! Native Dux dashboard composition. Reference: upstream DDash screenshot and
//! DashMetrics: six 128px columns, 8px gaps, 96px rows, stacked clock, calendar,
//! narrow vertical resource meters and portrait media transport. No image reads
//! or system probes occur in this module: it consumes Dash's worker snapshots.
use super::*;
use crate::gfx::{fill_circle, fill_rect, fill_rounded, stroke_rounded};
use crate::text::{Role, Weight};
use crate::window_app::Key;
use layout::Rect;
use tiny_skia::Color;

// Comfortable-density values from the pinned DDash/DashMetrics.qml and
// ControlCenter/CcMetrics.qml. Header chrome remains outside the card grid.
const GRID_COLUMNS: f32 = 6.0;
const GRID_COLUMN: f32 = 128.0;
const GRID_GAP: f32 = 8.0;
const GRID_ROW: f32 = 96.0;
const GRID_PAD: f32 = 12.0;
const GRID_TOP: f32 = 92.0;
const GRID_WIDTH: f32 = GRID_COLUMNS * GRID_COLUMN + (GRID_COLUMNS - 1.0) * GRID_GAP + 2.0 * GRID_PAD;
const GRID_HEIGHT: f32 = GRID_TOP + 5.0 * GRID_ROW + 4.0 * GRID_GAP + GRID_PAD;

const PAGES: [(&str, &str); 4] = [("Overview", "dashboard"), ("Media", "music_note"), ("Weather", "wb_sunny"), ("Settings", "settings")];

#[derive(Default)]
pub(super) struct State {
    pub clock: crate::clockface::AsyncClock,
    pub screen: (u32, u32),
    pub size: (u32, u32),
    page: usize,
    scroll: f32,
    month: i32,
    hover: Option<Target>,
    focus: Option<Target>,
    seek: Option<f32>,
    volume: Option<f32>,
    unmuted_volume: Option<(String, f64)>,
}

impl State {
    pub(super) fn for_tab(tab: Tab) -> Self {
        Self {
            clock: if crate::theme::th().dashboard_style == "dux" { crate::clockface::AsyncClock::load(&crate::theme::th().fonts) } else { Default::default() },
            page: match tab {
                Tab::Overview => 0,
                Tab::Media => 1,
                Tab::Performance => 0,
            },
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Page(usize),
    PreviousMonth,
    NextMonth,
    Previous,
    Play,
    Next,
    Seek(f32),
    Volume(f32),
    Settings(&'static str),
}

/// Physical geometry grows with text. Short/narrow windows reflow and scroll;
/// enlarged text is never raster-scaled or shrunk to squeeze a fixed card.
struct Grid {
    tabs: [Rect; 4],
    clock: Rect,
    weather: Rect,
    user: Rect,
    resources: Rect,
    calendar: Rect,
    media: Rect,
    bottom: f32,
    scale: f32,
}

impl Grid {
    fn new(width: f32, scale: f32) -> Self {
        let s = scale;
        let pad = GRID_PAD * s;
        // Pinned DashMetrics: 6 × 128 columns, five 8px gaps, 12px
        // outer padding = 832px. Cells use the shared 96px row unit.
        let gap = GRID_GAP * s;
        let row = GRID_ROW * s;
        let span_h = |n: f32| n * row + (n - 1.0) * gap;
        let inner = (width - pad * 2.0).max(100.0);
        let tabs = std::array::from_fn(|i| Rect::new(pad + i as f32 * inner / 4.0, 8.0 * s, inner / 4.0, 64.0 * s));
        let y = GRID_TOP * s;
        let wide = width >= 780.0 * s;
        let (clock, weather, user, resources, calendar, media) = if wide {
            let cell = (inner - 5.0 * gap) / 6.0;
            let x = |col: f32| pad + col * (cell + gap);
            let span_w = |n: f32| n * cell + (n - 1.0) * gap;
            (
                Rect::new(x(0.0), y, span_w(1.0), span_h(2.0)),
                Rect::new(x(1.0), y, span_w(2.0), span_h(1.0)),
                Rect::new(x(3.0), y, span_w(3.0), span_h(1.0)),
                Rect::new(x(0.0), y + 2.0 * (row + gap), span_w(1.0), span_h(3.0)),
                Rect::new(x(1.0), y + row + gap, span_w(4.0), span_h(4.0)),
                Rect::new(x(5.0), y + row + gap, span_w(1.0), span_h(4.0)),
            )
        } else {
            // Accessibility adaptation: keep the row unit and enlarged text,
            // reflow to two columns and scroll rather than shrinking content.
            let half = (inner - gap) / 2.0;
            (
                Rect::new(pad, y, half, span_h(2.0)),
                Rect::new(pad + half + gap, y, half, span_h(2.0)),
                Rect::new(pad, y + 2.0 * (row + gap), inner, span_h(1.0)),
                Rect::new(pad, y + 7.0 * (row + gap), half, span_h(3.0)),
                Rect::new(pad, y + 3.0 * (row + gap), inner, span_h(4.0)),
                Rect::new(pad + half + gap, y + 7.0 * (row + gap), half, span_h(4.0)),
            )
        };
        let bottom = resources.bottom().max(media.bottom()) + pad;
        Self { tabs, clock, weather, user, resources, calendar, media, bottom, scale: s }
    }
    fn shift(&self, r: Rect, scroll: f32) -> Rect {
        Rect::new(r.x, r.y - scroll, r.w, r.h)
    }
}

impl Dash {
    pub(super) fn dux_enabled(&self) -> bool {
        crate::theme::th().dashboard_style == "dux" && self.tab != Tab::Performance
    }
    pub(super) fn dux_size(&self) -> (u32, u32) {
        let s = crate::theme::live::font_scale().max(1.0);
        let (sw, sh) = self.dux.screen;
        (
            ((GRID_WIDTH * s) as u32).min(if sw > 0 { sw.saturating_sub(32).max(320) } else { 1680 }),
            ((GRID_HEIGHT * s) as u32).min(if sh > 0 { sh.saturating_sub(default_top().max(0) as u32 + 24).max(240) } else { 1216 }),
        )
    }
    fn dux_grid(&self) -> Grid {
        let width = if self.dux.size.0 > 0 { self.dux.size.0 } else { self.dux_size().0 };
        Grid::new(width as f32, crate::theme::live::font_scale().max(1.0))
    }
    /// Prepare the current minute and geometry outside paint. Only time-string
    /// changes reuse the same digit bitmaps; theme changes explicitly reload.
    pub(super) fn prepare_dux_clock(&mut self) -> bool {
        if !self.dux_enabled() || !self.dux.clock.enabled() {
            return false;
        }
        let Some(tm) = crate::menubar::clock::now() else {
            return false;
        };
        let g = self.dux_grid();
        let hour = if self.hour12 {
            if tm.hour % 12 == 0 {
                12
            } else {
                tm.hour % 12
            }
        } else {
            tm.hour
        };
        self.dux.clock.request(crate::clockface::Input {
            width: (g.clock.w - 32.0 * g.scale).max(1.0),
            height: (g.clock.h - 32.0 * g.scale).max(1.0),
            hours: format!("{hour:02}"),
            minutes: format!("{:02}", tm.min),
            date: info::long_date(tm.wday, tm.mday, tm.mon),
            scale: g.scale,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn dtext(&mut self, pm: &mut Pixmap, value: &str, r: Rect, size: f32, weight: Weight, ink: Color, centered: bool) {
        let Some(fonts) = self.fonts.as_mut() else { return };
        let value = crate::controlcenter::fit(value, r.w, size, weight, fonts);
        let w = fonts.measure(&value, size, weight);
        fonts.draw(pm, &value, if centered { r.cx() - w / 2.0 } else { r.x }, r.y, size, weight, ink);
    }
    fn dicon(&mut self, pm: &mut Pixmap, name: &str, r: Rect, accent: bool) {
        let th = crate::theme::th();
        if let Some(icon) = self.symbols.get_material(name, if accent { th.accent.t() } else { th.text.t() }, r.w.ceil().max(1.0) as u32) {
            crate::gfx::blit_scaled(pm, icon, r.x, r.y, r.w, r.h, 1.0);
        }
    }
    fn dcard(pm: &mut Pixmap, r: Rect) {
        fill_rounded(pm, r.x, r.y, r.w, r.h, 12.0, crate::theme::th().surface.color(1.0));
    }
    pub(super) fn dux_paint(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let g = self.dux_grid();
        let s = g.scale;
        self.dux.scroll = self.dux.scroll.clamp(0.0, (self.dux_bottom(&g) - pm.height() as f32).max(0.0));
        pm.fill(Color::TRANSPARENT);
        fill_rounded(pm, 0.0, 0.0, pm.width() as f32, pm.height() as f32, 28.0, th.overlay.color(1.0));
        if self.dux.page == 0 {
            let clock = g.shift(g.clock, self.dux.scroll);
            Self::dcard(pm, clock);
            if self.dux.clock.enabled() {
                let inset = 16.0 * s;
                if !self.dux.clock.paint(pm, (clock.x + inset, clock.y + inset), th.accent_c(), th.sub()) {
                    // Pending preparation: native vector dots need no font lookup.
                    for i in 0..3 {
                        fill_circle(pm, clock.cx() + (i as f32 - 1.0) * 8.0, clock.cy(), 2.0, th.sub());
                    }
                }
            } else {
                self.dux_legacy_clock(pm, clock, s);
            }
            self.dux_weather(pm, g.shift(g.weather, self.dux.scroll), s);
            let user = g.shift(g.user, self.dux.scroll);
            Self::dcard(pm, user);
            let avatar = Rect::new(user.x + 16.0 * s, user.y + 24.0 * s, 48.0 * s, 48.0 * s);
            if let Some(image) = &self.avatar {
                super::charts::image_circle(pm, image, avatar);
            } else {
                fill_circle(pm, avatar.cx(), avatar.cy(), avatar.w / 2.0, th.accent_c());
                self.dtext(pm, &self.user.initial(), Rect::new(avatar.x, avatar.y + 18.0 * s, avatar.w, 40.0 * s), 28.0, Weight::Medium, th.accent_text_c(), true);
            }
            let tx = avatar.right() + 16.0 * s;
            self.dtext(pm, &self.user.display.clone(), Rect::new(tx, user.y + 10.0 * s, user.right() - tx - 12.0 * s, 30.0 * s), 18.0, Weight::Medium, th.text_c(), false);
            self.dtext(pm, &self.user.host.clone(), Rect::new(tx, user.y + 39.0 * s, user.right() - tx - 12.0 * s, 24.0 * s), 12.0, Weight::Regular, th.sub(), false);
            let uptime = self.cur.uptime.map(sysinfo::format_uptime).unwrap_or_else(|| "Uptime unavailable".into());
            self.dtext(pm, &uptime, Rect::new(tx, user.y + 64.0 * s, user.right() - tx - 12.0 * s, 24.0 * s), 12.0, Weight::Regular, th.sub(), false);
            self.dux_calendar(pm, g.shift(g.calendar, self.dux.scroll), s);
            self.dux_resources(pm, g.shift(g.resources, self.dux.scroll), s);
            self.dux_media(pm, g.shift(g.media, self.dux.scroll), s, false);
        } else {
            let r = Rect::new(12.0 * s, 92.0 * s - self.dux.scroll, pm.width() as f32 - 24.0 * s, 490.0 * s);
            match self.dux.page {
                1 => self.dux_media(pm, r, s, true),
                2 => self.dux_weather(pm, r, s),
                _ => {}
            }
        }
        // Fixed opaque navigation chrome covers scrolled cards; no backing pixmap allocation.
        fill_rounded(pm, 0.0, 0.0, pm.width() as f32, 84.0 * s, 28.0, th.overlay.color(1.0));
        fill_rect(pm, 0.0, 40.0 * s, pm.width() as f32, 44.0 * s, th.overlay.color(1.0));
        for (i, (label, icon)) in PAGES.iter().enumerate() {
            let r = g.tabs[i];
            let selected = self.dux.page == i;
            self.dicon(pm, icon, Rect::new(r.cx() - 11.0 * s, r.y + 4.0 * s, 22.0 * s, 22.0 * s), selected);
            self.dtext(pm, label, Rect::new(r.x, r.y + 37.0 * s, r.w, 26.0 * s), 14.0, Weight::Medium, if selected { th.accent_c() } else { th.text_c() }, true);
            if selected {
                fill_rounded(pm, r.cx() - 36.0 * s, r.bottom() + 3.0 * s, 72.0 * s, 3.0 * s, 1.5 * s, th.accent_c());
            }
            if self.dux.hover == Some(if i == 3 { Target::Settings("appearance") } else { Target::Page(i) }) {
                fill_rounded(pm, r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 12.0, th.ink(0.06));
            }
            if self.dux.focus == Some(if i == 3 { Target::Settings("appearance") } else { Target::Page(i) }) {
                stroke_rounded(pm, r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 12.0, th.accent_c(), 2.0);
            }
        }
        if let Some(target) = self.dux.focus {
            if let Some((_, r)) = self.dux_targets().into_iter().find(|(t, _)| *t == target) {
                if r.y >= 84.0 * s {
                    stroke_rounded(pm, r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 12.0, th.accent_c(), 2.0);
                }
            }
        }
        if th.outline > 0.0 {
            stroke_rounded(pm, 0.5, 0.5, pm.width() as f32 - 1.0, pm.height() as f32 - 1.0, 27.5, th.outline_c(), 1.0);
        }
    }
    /// Preserve the deployed four-font package until its separately authored
    /// fifth-font activation. This is the unchanged production0.2 clock path.
    fn dux_legacy_clock(&mut self, pm: &mut Pixmap, clock: Rect, s: f32) {
        let th = crate::theme::th();
        if let Some(tm) = crate::menubar::clock::now() {
            let hour = if self.hour12 {
                if tm.hour % 12 == 0 {
                    12
                } else {
                    tm.hour % 12
                }
            } else {
                tm.hour
            };
            for (i, value) in [format!("{hour:02}"), format!("{:02}", tm.min)].iter().enumerate() {
                let r = Rect::new(clock.x + 8.0 * s, clock.y + (26.0 + i as f32 * 66.0) * s, clock.w - 16.0 * s, 64.0 * s);
                let Some(fonts) = self.fonts.as_mut() else { continue };
                let w = fonts.measure_as(Role::Display, value, 54.0, Weight::Medium);
                fonts.draw_as(Role::Display, pm, value, r.cx() - w / 2.0, r.y, 54.0, Weight::Medium, th.accent_c());
            }
            self.dtext(pm, &info::long_date(tm.wday, tm.mday, tm.mon), Rect::new(clock.x + 8.0 * s, clock.bottom() - 38.0 * s, clock.w - 16.0 * s, 24.0 * s), 11.0, Weight::Regular, th.sub(), true);
        }
    }
    fn dux_weather(&mut self, pm: &mut Pixmap, r: Rect, s: f32) {
        Self::dcard(pm, r);
        let th = crate::theme::th();
        if let Some(wx) = self.wx.clone() {
            let icon = weather::symbol_for(wx.code, wx.night);
            let compact = r.w < 240.0 * s && self.dux.page == 0;
            self.dicon(pm, icon, Rect::new(if compact { r.cx() - 24.0 * s } else { r.x + 24.0 * s }, r.y + 24.0 * s, 48.0 * s, 48.0 * s), true);
            let x = if compact { r.x + 12.0 * s } else { r.x + 84.0 * s };
            self.dtext(
                pm,
                &format!("{}°C", wx.temp_c),
                Rect::new(x, r.y + if compact { 90.0 * s } else { 22.0 * s }, (r.right() - x - 12.0 * s).max(0.0), 40.0 * s),
                28.0,
                Weight::Regular,
                th.text_c(),
                compact,
            );
            self.dtext(
                pm,
                info::condition_label(wx.code, wx.night),
                Rect::new(x, r.y + if compact { 146.0 * s } else { 65.0 * s }, (r.right() - x - 12.0 * s).max(0.0), 28.0 * s),
                12.0,
                Weight::Regular,
                th.sub(),
                compact,
            );
            if !weather::is_fresh(&wx, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())) {
                self.dtext(pm, "Cached", Rect::new(r.right() - 64.0 * s, r.y + 4.0 * s, 56.0 * s, 18.0 * s), 10.0, Weight::Regular, th.sub(), true);
            }
            if self.dux.page == 2 {
                self.dtext(pm, &wx.location, Rect::new(r.x + 24.0 * s, r.y + 120.0 * s, r.w - 48.0 * s, 28.0 * s), 14.0, Weight::Medium, th.text_c(), false);
                let values = [("Feels like", wx.feels_c.map(|v| format!("{v}°C"))), ("Humidity", wx.humidity.map(|v| format!("{v}%"))), ("Wind", wx.wind_kmh.map(|v| format!("{v} km/h")))];
                let width = (r.w - 64.0 * s) / 3.0;
                for (i, (label, value)) in values.iter().enumerate() {
                    let x = r.x + 24.0 * s + i as f32 * (width + 8.0 * s);
                    fill_rounded(pm, x, r.y + 172.0 * s, width, 76.0 * s, 12.0, th.ink(0.06));
                    self.dtext(pm, label, Rect::new(x + 8.0 * s, r.y + 184.0 * s, width - 16.0 * s, 24.0 * s), 11.0, Weight::Regular, th.sub(), false);
                    self.dtext(pm, value.as_deref().unwrap_or("Unavailable"), Rect::new(x + 8.0 * s, r.y + 212.0 * s, width - 16.0 * s, 28.0 * s), 14.0, Weight::Medium, th.text_c(), false);
                }
                self.dtext(pm, "Forecast", Rect::new(r.x + 24.0 * s, r.y + 276.0 * s, r.w - 48.0 * s, 28.0 * s), 16.0, Weight::Medium, th.text_c(), false);
                if wx.days.is_empty() {
                    self.dtext(pm, "Forecast unavailable", Rect::new(r.x + 24.0 * s, r.y + 320.0 * s, r.w - 48.0 * s, 32.0 * s), 13.0, Weight::Regular, th.sub(), false);
                } else {
                    for (i, day) in wx.days.iter().take(3).enumerate() {
                        let x = r.x + 24.0 * s + i as f32 * (width + 8.0 * s);
                        fill_rounded(pm, x, r.y + 318.0 * s, width, 140.0 * s, 12.0, th.ink(0.06));
                        self.dtext(pm, &day.date, Rect::new(x + 8.0 * s, r.y + 330.0 * s, width - 16.0 * s, 28.0 * s), 11.0, Weight::Medium, th.sub(), true);
                        self.dicon(pm, weather::symbol_for(day.code, false), Rect::new(x + width / 2.0 - 16.0 * s, r.y + 368.0 * s, 32.0 * s, 32.0 * s), true);
                        self.dtext(pm, &format!("{}° / {}°", day.high_c, day.low_c), Rect::new(x + 8.0 * s, r.y + 418.0 * s, width - 16.0 * s, 28.0 * s), 14.0, Weight::Medium, th.text_c(), true);
                    }
                }
            }
        } else {
            self.dicon(pm, "sun-small", Rect::new(r.cx() - 20.0 * s, r.y + 20.0 * s, 40.0 * s, 40.0 * s), true);
            self.dtext(pm, "Weather unavailable", Rect::new(r.x + 12.0 * s, r.y + 62.0 * s, r.w - 24.0 * s, 26.0 * s), 13.0, Weight::Regular, th.sub(), true);
        }
    }
    fn dux_calendar(&mut self, pm: &mut Pixmap, r: Rect, s: f32) {
        Self::dcard(pm, r);
        let th = crate::theme::th();
        let Some((year, month, today)) = crate::menubar::clock::today() else { return };
        let total = year * 12 + month as i32 + self.dux.month;
        let year = total.div_euclid(12);
        let month = total.rem_euclid(12) as u32;
        self.dtext(pm, &format!("{} {}", info::MONTHS[month as usize], year), Rect::new(r.x + 38.0 * s, r.y + 20.0 * s, r.w - 76.0 * s, 24.0 * s), 14.0, Weight::Medium, th.text_c(), true);
        self.dtext(pm, "‹", Rect::new(r.x + 12.0 * s, r.y + 18.0 * s, 24.0 * s, 32.0 * s), 18.0, Weight::Regular, th.accent_c(), true);
        self.dtext(pm, "›", Rect::new(r.right() - 36.0 * s, r.y + 18.0 * s, 24.0 * s, 32.0 * s), 18.0, Weight::Regular, th.accent_c(), true);
        let cell = (r.w - 24.0 * s) / 7.0;
        for (i, label) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].iter().enumerate() {
            self.dtext(pm, label, Rect::new(r.x + 12.0 * s + i as f32 * cell, r.y + 60.0 * s, cell, 24.0 * s), 12.0, Weight::Regular, th.sub(), true);
        }
        for (row, days) in crate::menubar::popup::month_grid(year, month).iter().enumerate() {
            for (col, day) in days.iter().enumerate() {
                if let Some(day) = day {
                    let x = r.x + 12.0 * s + col as f32 * cell;
                    let y = r.y + (104.0 + row as f32 * 42.0) * s;
                    if self.dux.month == 0 && *day == today {
                        fill_circle(pm, x + cell / 2.0, y + 10.0 * s, 18.0 * s, th.accent.color(0.18));
                    }
                    self.dtext(pm, &day.to_string(), Rect::new(x, y, cell, 24.0 * s), 12.0, Weight::Regular, if self.dux.month == 0 && *day == today { th.accent_c() } else { th.text_c() }, true);
                }
            }
        }
    }
    fn dux_resources(&mut self, pm: &mut Pixmap, r: Rect, s: f32) {
        Self::dcard(pm, r);
        let th = crate::theme::th();
        let cell = (r.w - 24.0 * s) / 3.0;
        let values = [
            ("memory", self.cur.primed.then_some(self.cur.cpu), self.cur.primed.then(|| format!("{:.0}%", self.cur.cpu * 100.0))),
            ("device_thermostat", self.cur.cpu_temp.map(|v| v / 100.0), self.cur.cpu_temp.map(|v| format!("{v:.0}°"))),
            ("developer_board", (self.cur.mem.total_kb > 0).then_some(self.cur.mem.used_frac()), (self.cur.mem.total_kb > 0).then(|| format!("{:.0}%", self.cur.mem.used_frac() * 100.0))),
        ];
        for (i, (label, value, shown)) in values.into_iter().enumerate() {
            let x = r.x + 12.0 * s + cell * (i as f32 + 0.5);
            let y = r.y + 24.0 * s;
            let height = r.h - 100.0 * s;
            fill_rounded(pm, x - 5.0 * s, y, 10.0 * s, height, 5.0 * s, th.ink(0.12));
            if let Some(value) = value {
                let used = height * value.clamp(0.0, 1.0);
                fill_rounded(pm, x - 5.0 * s, y + height - used, 10.0 * s, used.max(3.0 * s), 5.0 * s, th.accent_c());
            }
            let value = shown.unwrap_or_else(|| "--".into());
            self.dtext(pm, &value, Rect::new(x - cell / 2.0, r.bottom() - 66.0 * s, cell, 24.0 * s), 10.0, Weight::Medium, th.sub(), true);
            self.dicon(pm, label, Rect::new(x - 9.0 * s, r.bottom() - 34.0 * s, 18.0 * s, 18.0 * s), true);
        }
    }
    fn dux_media(&mut self, pm: &mut Pixmap, r: Rect, s: f32, large: bool) {
        Self::dcard(pm, r);
        let th = crate::theme::th();
        let active = self.active().cloned();
        let Some(p) = active else {
            self.dicon(pm, "play", Rect::new(r.cx() - 20.0 * s, r.y + 40.0 * s, 40.0 * s, 40.0 * s), true);
            self.dtext(pm, "No media playing", Rect::new(r.x + 12.0 * s, r.y + 100.0 * s, r.w - 24.0 * s, 40.0 * s), 12.0, Weight::Regular, th.sub(), true);
            return;
        };
        let art_size = (if large { 200.0 * s } else { 96.0 * s }).min(r.w - 32.0 * s);
        let art = Rect::new(r.cx() - art_size / 2.0, r.y + 24.0 * s, art_size, art_size);
        if let Some(image) = &self.art.pixmap {
            super::charts::image_rounded(pm, image, art, art_size * 0.3);
        } else {
            fill_rounded(pm, art.x, art.y, art.w, art.h, art_size * 0.3, th.ink(0.08));
            self.dicon(pm, "play", Rect::new(art.cx() - 16.0 * s, art.cy() - 16.0 * s, 32.0 * s, 32.0 * s), true);
        }
        let title = if p.meta.title.is_empty() { "Unknown title" } else { &p.meta.title };
        let metadata_y = art.bottom() + 20.0 * s;
        self.dtext(pm, title, Rect::new(r.x + 12.0 * s, metadata_y, r.w - 24.0 * s, 28.0 * s), 13.0, Weight::Medium, th.text_c(), true);
        self.dtext(pm, &p.meta.artist, Rect::new(r.x + 12.0 * s, metadata_y + 28.0 * s, r.w - 24.0 * s, 28.0 * s), 12.0, Weight::Regular, th.sub(), true);
        let bar = media_seek(r, s);
        fill_rounded(pm, bar.x, bar.y, bar.w, bar.h, bar.h / 2.0, th.ink(0.12));
        let progress = self.dux.seek.unwrap_or_else(|| p.progress(Instant::now()));
        fill_rounded(pm, bar.x, bar.y, bar.w * progress, bar.h, bar.h / 2.0, th.accent_c());
        if large {
            if let Some(value) = p.volume {
                let value = self.dux.volume.unwrap_or(value as f32);
                let bar = media_volume(r, s);
                fill_rounded(pm, bar.x, bar.y, bar.w, bar.h, bar.h / 2.0, th.ink(0.12));
                fill_rounded(pm, bar.x, bar.y, bar.w * value.clamp(0.0, 1.0), bar.h, bar.h / 2.0, th.accent_c());
                self.dicon(pm, "speaker-2", Rect::new(r.x + 20.0 * s, bar.y - 8.0 * s, 20.0 * s, 20.0 * s), true);
            }
        }
        for (target, icon, b) in media_controls(r, s, p.status == Status::Playing) {
            let on = target == Target::Play;
            if on {
                fill_circle(pm, b.cx(), b.cy(), b.w / 2.0, th.accent_c());
            }
            if on {
                if let Some(image) = self.symbols.get_material(icon, th.accent_text.t(), (16.0 * s).ceil() as u32) {
                    crate::gfx::blit_scaled(pm, image, b.cx() - 8.0 * s, b.cy() - 8.0 * s, 16.0 * s, 16.0 * s, 1.0);
                }
            } else {
                self.dicon(pm, icon, Rect::new(b.cx() - 8.0 * s, b.cy() - 8.0 * s, 16.0 * s, 16.0 * s), true);
            }
            if self.dux.focus == Some(target) {
                stroke_rounded(pm, b.x, b.y, b.w, b.h, b.w / 2.0, th.accent_c(), 2.0);
            }
        }
    }
    fn dux_targets(&self) -> Vec<(Target, Rect)> {
        let g = self.dux_grid();
        let s = g.scale;
        let mut targets: Vec<_> = g.tabs.iter().enumerate().map(|(i, r)| (if i == 3 { Target::Settings("appearance") } else { Target::Page(i) }, *r)).collect();
        if self.dux.page == 0 {
            let r = g.shift(g.calendar, self.dux.scroll);
            targets.push((Target::PreviousMonth, Rect::new(r.x + 8.0 * s, r.y + 12.0 * s, 32.0 * s, 36.0 * s)));
            targets.push((Target::NextMonth, Rect::new(r.right() - 40.0 * s, r.y + 12.0 * s, 32.0 * s, 36.0 * s)));
        }
        if self.dux.page <= 1 && self.active().is_some() {
            let r = if self.dux.page == 0 { g.shift(g.media, self.dux.scroll) } else { Rect::new(12.0 * s, 92.0 * s - self.dux.scroll, self.dux_grid_width() - 24.0 * s, 490.0 * s) };
            for (t, _, r) in media_controls(r, s, false) {
                targets.push((t, r));
            }
            if self.active().is_some_and(|p| p.can_seek && p.meta.length_us.is_some()) {
                targets.push((Target::Seek(0.0), media_seek(r, s).inflate(8.0 * s)));
            }
        }

        if self.dux.page == 1 && self.active().is_some_and(|p| p.volume.is_some()) {
            let s = g.scale;
            let r = Rect::new(12.0 * s, 92.0 * s - self.dux.scroll, self.dux_grid_width() - 24.0 * s, 490.0 * s);
            targets.push((Target::Volume(0.0), media_volume(r, s).inflate(8.0 * s)));
        }
        targets
    }
    fn dux_bottom(&self, grid: &Grid) -> f32 {
        if self.dux.page == 0 {
            grid.bottom
        } else {
            594.0 * grid.scale
        }
    }
    fn dux_grid_width(&self) -> f32 {
        if self.dux.size.0 > 0 {
            self.dux.size.0 as f32
        } else {
            self.dux_size().0 as f32
        }
    }
    fn dux_hit(&self, x: f32, y: f32) -> Option<Target> {
        let s = self.dux_grid().scale;
        self.dux_targets().into_iter().filter(|(t, _)| (matches!(t, Target::Page(_)) || matches!(t, Target::Settings("appearance"))) || y >= 84.0 * s).find(|(_, r)| r.contains(x, y)).map(|(t, r)| {
            if matches!(t, Target::Volume(_)) {
                Target::Volume(((x - r.x) / r.w).clamp(0.0, 1.0))
            } else if matches!(t, Target::Seek(_)) {
                Target::Seek(((x - r.x) / r.w).clamp(0.0, 1.0))
            } else {
                t
            }
        })
    }
    fn dux_activate(&mut self, target: Target) -> Response {
        match target {
            Target::Page(page) => {
                self.dux.page = page;
                self.dux.focus = self.dux.focus.and_then(|target| matches!(target, Target::Page(_)).then_some(Target::Page(page)));
                self.dux.hover = None;
                self.dux.scroll = 0.0;
                self.dux.seek = None;
                self.dux.volume = None;
                self.tab = if page == 1 { Tab::Media } else { Tab::Overview };
                if let Some(m) = &self.mpris {
                    m.send(Cmd::Polling(page <= 1));
                }
            }
            Target::PreviousMonth => self.dux.month -= 1,
            Target::NextMonth => self.dux.month += 1,
            Target::Play => self.command(Cmd::PlayPause),
            Target::Previous => {
                if self.active().is_some_and(|p| p.can_prev) {
                    self.command(Cmd::Previous);
                }
            }
            Target::Next => {
                if self.active().is_some_and(|p| p.can_next) {
                    self.command(Cmd::Next);
                }
            }
            Target::Volume(value) => {
                if let (Some(m), Some(p)) = (&self.mpris, self.active()) {
                    if p.volume.is_some() {
                        m.send(Cmd::Volume(p.bus.clone(), value.clamp(0.0, 1.0) as f64));
                    }
                }
            }
            Target::Settings(route) => {
                if !cfg!(test) {
                    crate::launch::spawn_argv(&[crate::launch::sibling_or_path("decsettings"), route.into()]);
                }
                return Response::act(Action::Close);
            }
            Target::Seek(frac) => {
                if let (Some(m), Some(p)) = (&self.mpris, self.active()) {
                    if p.can_seek {
                        if let Some(length) = p.meta.length_us {
                            m.send(Cmd::Seek(p.bus.clone(), (length as f64 * frac as f64) as i64));
                        }
                    }
                }
            }
        }
        Response::REDRAW
    }
    fn dux_seek_at(&self, x: f32) -> f32 {
        let g = self.dux_grid();
        let s = g.scale;
        let r = if self.dux.page == 0 { g.shift(g.media, self.dux.scroll) } else { Rect::new(12.0 * s, 92.0 * s - self.dux.scroll, self.dux_grid_width() - 24.0 * s, 490.0 * s) };
        let bar = media_seek(r, s);
        ((x - bar.x) / bar.w.max(1.0)).clamp(0.0, 1.0)
    }
    fn dux_volume_at(&self, x: f32) -> f32 {
        let s = self.dux_grid().scale;
        let r = Rect::new(12.0 * s, 92.0 * s - self.dux.scroll, self.dux_grid_width() - 24.0 * s, 490.0 * s);
        let bar = media_volume(r, s);
        ((x - bar.x) / bar.w.max(1.0)).clamp(0.0, 1.0)
    }
    pub(super) fn dux_leave(&mut self) -> Response {
        if self.dux.hover.take().is_some() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
    pub(super) fn dux_motion(&mut self, x: f32, y: f32) -> Response {
        if self.dux.seek.is_some() {
            self.dux.seek = Some(self.dux_seek_at(x));
            return Response::REDRAW;
        }

        if self.dux.volume.is_some() {
            self.dux.volume = Some(self.dux_volume_at(x));
            return Response::REDRAW;
        }
        let hit = self.dux_hit(x, y);
        if hit != self.dux.hover {
            self.dux.hover = hit;
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
    pub(super) fn dux_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if button == Button::Left {
            if !pressed {
                if self.dux.seek.take().is_some() {
                    return self.dux_activate(Target::Seek(self.dux_seek_at(x)));
                }
                if self.dux.volume.take().is_some() {
                    return self.dux_activate(Target::Volume(self.dux_volume_at(x)));
                }
            } else if let Some(target) = self.dux_hit(x, y) {
                if matches!(target, Target::Volume(_)) {
                    self.dux.volume = Some(self.dux_volume_at(x));
                    return Response::REDRAW;
                }
                if matches!(target, Target::Seek(_)) {
                    self.dux.seek = Some(self.dux_seek_at(x));
                    return Response::REDRAW;
                }
                return self.dux_activate(target);
            }
        }
        Response::NONE
    }
    pub(super) fn dux_scroll(&mut self, dy: f32) -> Response {
        let height = if self.dux.size.1 > 0 { self.dux.size.1 } else { self.dux_size().1 } as f32;
        let g = self.dux_grid();
        self.dux.scroll = (self.dux.scroll + dy * 40.0).clamp(0.0, (self.dux_bottom(&g) - height).max(0.0));
        Response::REDRAW
    }
    fn dux_adjust_seek(&mut self, seconds: i64) -> Response {
        let Some(player) = self.active().filter(|p| p.can_seek) else { return Response::NONE };
        let Some(length) = player.meta.length_us.filter(|n| *n > 0) else { return Response::NONE };
        let position = player.position_us.saturating_add(seconds.saturating_mul(1_000_000)).clamp(0, length);
        self.dux_activate(Target::Seek(position as f32 / length as f32))
    }

    // The Media page owns source player shortcuts when no individual control
    // has keyboard focus. Overview keeps page navigation and explicit controls.
    fn dux_media_key(&mut self, ev: &KeyEvent) -> Option<Response> {
        if self.dux.page != 1 || self.dux.focus.is_some() || ev.ctrl || ev.alt || self.active().is_none() {
            return None;
        }
        Some(match (ev.key, ev.ch) {
            (Key::Left, _) | (Key::Char, Some('h')) => self.dux_adjust_seek(-5),
            (Key::Right, _) | (Key::Char, Some('l')) => self.dux_adjust_seek(5),
            (Key::Up, _) | (Key::Char, Some('k')) => self.adjust_volume(5),
            (Key::Down, _) | (Key::Char, Some('j')) => self.adjust_volume(-5),
            (Key::Char, Some(' ')) => self.dux_activate(Target::Play),
            (Key::Char, Some(digit @ '0'..='9')) => self.dux_activate(Target::Seek((digit as u8 - b'0') as f32 / 10.0)),
            (Key::Char, Some('m')) => {
                let player = self.active()?;
                let volume = player.volume?;
                let bus = player.bus.clone();
                let next = if volume > 0.0 {
                    self.dux.unmuted_volume = Some((bus, volume));
                    0.0
                } else {
                    self.dux.unmuted_volume.as_ref().filter(|(saved, _)| saved == &bus).map_or(0.5, |(_, value)| *value)
                };
                self.dux_activate(Target::Volume(next as f32))
            }
            _ => return None,
        })
    }

    pub(super) fn dux_key(&mut self, ev: &KeyEvent) -> Response {
        if let Some(response) = self.dux_media_key(ev) {
            return response;
        }
        match ev.key {
            Key::Escape => Response::act(Action::Close),
            Key::Left if matches!(self.dux.focus, Some(Target::Volume(_))) => self.adjust_volume(-5),
            Key::Right if matches!(self.dux.focus, Some(Target::Volume(_))) => self.adjust_volume(5),
            Key::Left | Key::Down if matches!(self.dux.focus, Some(Target::Seek(_))) => self.dux_adjust_seek(-5),
            Key::Right | Key::Up if matches!(self.dux.focus, Some(Target::Seek(_))) => self.dux_adjust_seek(5),
            Key::Left => self.dux_activate(Target::Page((self.dux.page + 2) % 3)),
            Key::Right => self.dux_activate(Target::Page((self.dux.page + 1) % 3)),
            Key::Up if matches!(self.dux.focus, Some(Target::Volume(_))) => self.adjust_volume(5),
            Key::Down if matches!(self.dux.focus, Some(Target::Volume(_))) => self.adjust_volume(-5),
            Key::PageDown | Key::Down => self.dux_scroll(4.0),
            Key::PageUp | Key::Up => self.dux_scroll(-4.0),
            Key::Home => {
                self.dux.scroll = 0.0;
                Response::REDRAW
            }
            Key::Tab => {
                let targets = self.dux_targets();
                let pos = targets.iter().position(|(t, _)| Some(*t) == self.dux.focus).map_or(if ev.shift { targets.len() - 1 } else { 0 }, |p| {
                    if ev.shift {
                        (p + targets.len() - 1) % targets.len()
                    } else {
                        (p + 1) % targets.len()
                    }
                });
                self.dux.focus = Some(targets[pos].0);
                if !matches!(targets[pos].0, Target::Page(_)) {
                    let rect = targets[pos].1;
                    let header = 84.0 * self.dux_grid().scale;
                    let height = if self.dux.size.1 > 0 { self.dux.size.1 } else { self.dux_size().1 } as f32;
                    if rect.y < header {
                        self.dux.scroll = (self.dux.scroll - (header - rect.y)).max(0.0);
                    } else if rect.bottom() > height {
                        self.dux.scroll += rect.bottom() - height + 12.0;
                    }
                }
                Response::REDRAW
            }
            Key::Enter => match self.dux.focus {
                Some(Target::Seek(_) | Target::Volume(_)) => Response::NONE,
                Some(target) => self.dux_activate(target),
                None => Response::NONE,
            },
            Key::Char if matches!(ev.ch, Some('1'..='3')) => self.dux_activate(Target::Page((ev.ch.unwrap() as u8 - b'1') as usize)),
            Key::Char if ev.ch == Some(' ') => match self.dux.focus {
                Some(Target::Seek(_) | Target::Volume(_)) | None => Response::NONE,
                Some(target) => self.dux_activate(target),
            },
            _ => Response::NONE,
        }
    }
}

fn media_volume(r: Rect, s: f32) -> Rect {
    Rect::new(r.x + 48.0 * s, r.bottom() - 20.0 * s, r.w - 72.0 * s, 4.0 * s)
}
fn media_seek(r: Rect, s: f32) -> Rect {
    Rect::new(r.x + 20.0 * s, r.bottom() - 112.0 * s, r.w - 40.0 * s, 3.0 * s)
}
fn media_controls(r: Rect, s: f32, playing: bool) -> [(Target, &'static str, Rect); 3] {
    let s = s.min(r.w / 128.0);
    let cy = r.bottom() - 62.0 * s;
    [
        (Target::Previous, "backward", Rect::new(r.cx() - 56.0 * s, cy - 16.0 * s, 32.0 * s, 32.0 * s)),
        (Target::Play, if playing { "pause" } else { "play" }, Rect::new(r.cx() - 20.0 * s, cy - 20.0 * s, 40.0 * s, 40.0 * s)),
        (Target::Next, "forward", Rect::new(r.cx() + 24.0 * s, cy - 16.0 * s, 32.0 * s, 32.0 * s)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_keyboard_slider_inputs_and_pointer_commits() {
        let scale = crate::theme::live::font_scale().max(1.0);
        let mut dash = Dash::new(Tab::Media, None, None);
        dash.dux.size = ((832.0 * scale) as u32, (608.0 * scale) as u32);
        let mut player = Player::new("org.mpris.MediaPlayer2.fixture", Instant::now());
        player.ready = true;
        player.status = Status::Paused;
        player.can_seek = true;
        player.volume = Some(0.67);
        player.position_us = 42_000_000;
        player.meta.length_us = Some(180_000_000);
        dash.auto = Some(player.bus.clone());
        dash.players.push(player.clone());
        let (sink, commands, _peer) = Mpris::test_sink();
        dash.mpris = Some(sink);
        let enter = KeyEvent { key: Key::Enter, text: String::new(), ch: None, ctrl: false, shift: false, alt: false };
        for target in [Target::Seek(0.0), Target::Volume(0.0)] {
            dash.dux.focus = Some(target);
            dash.dux_key(&enter);
            assert!(commands.try_recv().is_err(), "Enter on a slider must not emit a zero-valued command");
            assert_eq!(dash.players[0], player);
            assert!(dash.dux.seek.is_none() && dash.dux.volume.is_none(), "keyboard must not arm pointer interaction");
        }
        let card = Rect::new(12.0 * scale, 92.0 * scale, 832.0 * scale - 24.0 * scale, 490.0 * scale);
        let seek = media_seek(card, scale);
        dash.dux_button(seek.x + seek.w * 0.4, seek.cy(), Button::Left, true);
        dash.dux_motion(seek.x + seek.w * 0.73, seek.cy());
        assert!(commands.try_recv().is_err(), "pointer preview must not issue repeated seek commands");
        dash.dux_button(seek.x + seek.w * 0.73, seek.cy(), Button::Left, false);
        match commands.try_recv().expect("pointer seek commit") {
            Cmd::Seek(_, position) => assert!((position - 131_400_000).abs() < 32),
            other => panic!("unexpected {other:?}"),
        }
        let volume = media_volume(card, scale);
        dash.dux_button(volume.x + volume.w * 0.5, volume.cy(), Button::Left, true);
        dash.dux_motion(volume.x + volume.w * 0.71, volume.cy());
        assert!(commands.try_recv().is_err());
        dash.dux_button(volume.x + volume.w * 0.71, volume.cy(), Button::Left, false);
        match commands.try_recv().expect("pointer volume commit") {
            Cmd::Volume(_, value) => assert!((value - 0.71).abs() < 0.001),
            other => panic!("unexpected {other:?}"),
        }
    }
    #[test]
    fn comfortable_source_grid_matches_pinned_dashmetrics_equations() {
        let g = Grid::new(832.0, 1.0);
        assert_eq!(g.clock, Rect::new(12.0, 92.0, 128.0, 200.0));
        assert_eq!(g.weather, Rect::new(148.0, 92.0, 264.0, 96.0));
        assert_eq!(g.user, Rect::new(420.0, 92.0, 400.0, 96.0));
        assert_eq!(g.resources, Rect::new(12.0, 300.0, 128.0, 304.0));
        assert_eq!(g.calendar, Rect::new(148.0, 196.0, 536.0, 408.0));
        assert_eq!(g.media, Rect::new(692.0, 196.0, 128.0, 408.0));
        assert_eq!(g.bottom, 616.0);
    }

    #[test]
    fn media_page_keys_control_the_player_while_overview_requires_control_focus() {
        let mut dash = Dash::new(Tab::Media, None, None);
        dash.dux.size = (832, 300);
        let mut player = Player::new("org.mpris.MediaPlayer2.fixture", Instant::now());
        player.ready = true;
        player.volume = Some(0.5);
        player.position_us = 20_000_000;
        player.meta.length_us = Some(100_000_000);
        player.can_seek = true;
        dash.auto = Some(player.bus.clone());
        dash.players.push(player);
        let (sink, commands, _peer) = Mpris::test_sink();
        dash.mpris = Some(sink);
        let key = |key, ch| KeyEvent { key, text: String::new(), ch, ctrl: false, shift: false, alt: false };
        dash.dux_key(&key(Key::Down, None));
        assert_eq!(dash.dux.scroll, 0.0);
        assert!(matches!(commands.try_recv(), Ok(Cmd::Volume(_, value)) if (value - 0.45).abs() < 0.001));
        dash.dux_key(&key(Key::Right, None));
        assert_eq!(dash.dux.page, 1);
        assert!(matches!(commands.try_recv(), Ok(Cmd::Seek(_, value)) if (value - 25_000_000).abs() < 32));
        dash.dux_key(&key(Key::Char, Some('8')));
        assert!(matches!(commands.try_recv(), Ok(Cmd::Seek(_, value)) if (value - 80_000_000).abs() < 32));
        dash.dux_key(&key(Key::Char, Some('m')));
        assert!(matches!(commands.try_recv(), Ok(Cmd::Volume(_, value)) if value == 0.0));
        dash.players[0].volume = Some(0.0);
        dash.dux_key(&key(Key::Char, Some('m')));
        assert!(matches!(commands.try_recv(), Ok(Cmd::Volume(_, value)) if (value - 0.5).abs() < 0.001));
        dash.players[0].volume = Some(0.5);
        dash.dux_key(&key(Key::Char, Some(' ')));
        assert!(matches!(commands.try_recv(), Ok(Cmd::PlayPause(_))));
        dash.dux.page = 0;
        dash.dux_key(&key(Key::Down, None));
        assert!(dash.dux.scroll > 0.0);
        dash.dux_key(&key(Key::Char, Some(' ')));
        assert!(commands.try_recv().is_err(), "Overview must not trigger the player without focus");
        dash.dux.focus = Some(Target::NextMonth);
        dash.dux_key(&key(Key::Char, Some(' ')));
        assert_eq!(dash.dux.month, 1);
        assert!(commands.try_recv().is_err());
        dash.dux_activate(Target::Page(1));
        assert!(dash.dux.focus.is_none(), "page changes must clear controls absent from the new page");
        assert!(matches!(commands.try_recv(), Ok(Cmd::Polling(true))));
        dash.dux.focus = Some(Target::Volume(0.0));
        dash.dux_key(&key(Key::Up, None));
        assert!(matches!(commands.try_recv(), Ok(Cmd::Volume(_, v)) if (v-0.55).abs()<0.001));
        dash.dux.focus = Some(Target::Seek(0.0));
        dash.dux_key(&key(Key::Up, None));
        assert!(matches!(commands.try_recv(), Ok(Cmd::Seek(_, v)) if (v-25_000_000).abs()<32));
        dash.dux_key(&key(Key::Char, Some(' ')));
        assert!(commands.try_recv().is_err(), "Space on a slider must not commit zero");
        dash.dux.focus = Some(Target::Play);
        dash.dux_key(&key(Key::Char, Some(' ')));
        assert!(matches!(commands.try_recv(), Ok(Cmd::PlayPause(_))));
        let perf = Dash::new(Tab::Performance, None, None);
        assert!(!perf.dux_enabled(), "Performance uses the actual shared performance page, never Weather");
        assert_eq!(perf.dux.page, 0);
    }

    #[test]
    fn source_grid_and_large_text_reflow_keep_cards_separate_and_in_bounds() {
        for (width, scale) in [(840.0, 1.0), (960.0, 2.0), (480.0, 1.0), (720.0, 2.0)] {
            let g = Grid::new(width, scale);
            let cards = [g.clock, g.weather, g.user, g.resources, g.calendar, g.media];
            for (i, r) in cards.iter().enumerate() {
                assert!(r.x >= 0.0 && r.right() <= width + 0.01 && r.bottom() <= g.bottom);
                for other in cards.iter().skip(i + 1) {
                    assert!(!r.intersects(other), "{width}/{scale}: {r:?} {other:?}");
                }
            }
        }
    }

    /// Isolated because native fonts use process-global accessibility settings.
    #[test]
    #[ignore = "native Dux surfaces: run alone with DUX_SURFACE_EVIDENCE_DIR"]
    fn native_dux_surface_evidence() {
        use crate::theme::model::Mode;
        assert!(std::env::var_os("DECEBAL_CONFIG").is_some() && std::env::var_os("DECEBAL_THEMES").is_some(), "explicit private theme/config fixture required");
        let source = crate::theme::live::Source::discover();
        let selected = source.selection();
        assert!(selected.name.starts_with("dux"), "actual Dux package selection required");
        let dir = std::path::PathBuf::from(std::env::var_os("DUX_SURFACE_EVIDENCE_DIR").expect("evidence directory"));
        std::fs::create_dir_all(&dir).unwrap();
        for scale in [1.0, 2.0] {
            for dark in [true, false] {
                let mut selected = selected.clone();
                selected.mode = if dark { Mode::Dark } else { Mode::Light };
                let (th, problem) = source.build(&selected, dark);
                assert!(problem.is_none(), "{problem:?}");
                assert_eq!(th.dashboard_style, "dux");
                assert_eq!(th.fonts.primary_ui(), "Google Sans Flex");
                assert!(!th.fonts.package_faces.is_empty(), "process-private font metadata required");
                let th = crate::accessibility::Preferences { font_scale: Some(scale), ..Default::default() }.apply(th);
                crate::theme::live::publish_theme(th);
                crate::menubar::overlay::assert_slider_keyboard_inert();
                assert_keyboard_slider_inputs_and_pointer_commits();
                for available in [false, true] {
                    let mut dash = Dash::new(Tab::Overview, None, None);
                    assert!(dash.symbols.prepare_material(), "actual package Material font required");
                    dash.dux.screen = (960, 720);
                    dash.dux.size = (if scale == 1.0 { 832 } else { 928 }, 560);
                    dash.avatar = None;
                    dash.user = info::User { login: "sample".into(), display: "Native sample user".into(), host: "sample".into() };
                    dash.wx = None;
                    if available {
                        dash.cur.primed = true;
                        dash.cur.cpu = 0.37;
                        dash.cur.cpu_temp = Some(52.0);
                        dash.cur.mem = sysinfo::Mem { total_kb: 16_000_000, available_kb: 8_000_000, ..Default::default() };
                        dash.cur.uptime = Some(86400.0);
                        dash.wx = Some(Weather {
                            temp_c: 21,
                            code: 119,
                            location: "Native sample city".into(),
                            humidity: Some(60),
                            wind_kmh: Some(12),
                            feels_c: Some(20),
                            days: vec![weather::Day { date: "2026-10-30".into(), high_c: 22, low_c: 15, code: 119 }],
                            ..Default::default()
                        });
                        let mut player = Player::new("org.mpris.MediaPlayer2.sample", Instant::now());
                        player.ready = true;
                        player.status = Status::Paused;
                        player.meta.title = "Native sample track".into();
                        player.meta.artist = "Native sample artist".into();
                        player.meta.length_us = Some(180_000_000);
                        player.position_us = 30_000_000;
                        player.can_seek = true;
                        dash.auto = Some(player.bus.clone());
                        dash.players.push(player);
                    }
                    let mut snapshot = if available {
                        crate::controlcenter::Snapshot {
                            user: Some(("Native sample user".into(), "up 1 day".into())),
                            net: Some(crate::controlcenter::sys::Net { wifi_enabled: true, wifi_name: "Sample network".into(), wired_name: String::new() }),
                            bluetooth: Some(crate::controlcenter::sys::Bluetooth { powered: true, connected: 1 }),
                            volume: Some(crate::controlcenter::sys::Volume { level: 0.5, muted: false }),
                            brightness: Some(70),
                            dnd: Some(false),
                            night_shift: Some(false),
                            media: Some(crate::controlcenter::sys::Media { playing: false, title: "Native sample track".into(), artist: "Native sample artist".into() }),
                            util: Some(crate::controlcenter::sys::Utils::default()),
                            ..Default::default()
                        }
                    } else {
                        crate::controlcenter::Snapshot::default()
                    };
                    snapshot.light = !dark;
                    let style = crate::controlcenter::settings::Style::Dux;
                    let (items, height) = crate::controlcenter::layout_styled(&snapshot, &crate::controlcenter::settings::Tiles::ALL, style);
                    let mut panel = Pixmap::new(crate::controlcenter::panel_width(style) as u32, height.ceil() as u32).unwrap();
                    crate::controlcenter::draw(
                        &mut panel,
                        &crate::controlcenter::View { origin: (0.0, 0.0), items: &items, height, hover: None, style },
                        dash.fonts.as_mut().unwrap(),
                        &mut dash.symbols,
                    );
                    panel.save_png(dir.join(format!("control-{}-{}-{}.png", (scale * 100.0) as u32, if dark { "dark" } else { "light" }, if available { "sample" } else { "missing" }))).unwrap();
                    for item in items.iter().filter(|i| i.target.is_some()) {
                        assert_eq!(crate::controlcenter::hit(&items, item.r.x + item.r.w / 2.0, item.r.y + item.r.h / 2.0).map(|v| v.0), item.target);
                    }
                    for page in 0..3 {
                        dash.dux.page = page;
                        dash.dux.scroll = 0.0;
                        let mut pixels = Pixmap::new(dash.dux.size.0, dash.dux.size.1).unwrap();
                        dash.prepare_dux_clock();
                        dash.dux.clock.wait_prepared();
                        if let Some(metrics) = dash.dux.clock.metrics() {
                            let clock = dash.dux_grid().clock;
                            assert_eq!(metrics.width, clock.w - 32.0 * scale);
                            assert_eq!(metrics.height, 168.0 * scale, "preserve root's 96px row / 8px gap / 200px clock cell");
                        }
                        dash.dux_paint(&mut pixels);
                        assert!(pixels.data().chunks_exact(4).any(|p| p[3] > 0));
                        pixels
                            .save_png(dir.join(format!("dash-{}-{}-{}-page{page}.png", (scale * 100.0) as u32, if dark { "dark" } else { "light" }, if available { "sample" } else { "missing" })))
                            .unwrap();
                        if page == 0 {
                            let grid = dash.dux_grid();
                            let target = grid.shift(grid.calendar, dash.dux.scroll);
                            dash.dux_button(target.right() - 24.0 * scale, target.y + 24.0 * scale, Button::Left, true);
                            if target.y >= 84.0 * scale && target.y < 560.0 {
                                assert_eq!(dash.dux.month, 1);
                            }
                            dash.dux_scroll(100.0);
                            assert!(dash.dux.scroll > 0.0);
                            dash.prepare_dux_clock();
                            dash.dux.clock.wait_prepared();
                            dash.dux_paint(&mut pixels);
                            pixels
                                .save_png(dir.join(format!("dash-{}-{}-{}-scrolled.png", (scale * 100.0) as u32, if dark { "dark" } else { "light" }, if available { "sample" } else { "missing" })))
                                .unwrap();
                        }
                    }
                }
            }
        }
    }
}
