//! Painting of the dashboard: card, tab bar and the three tabs.

use super::charts::{arrow, bar, image_circle, image_rounded, load_color, meters, ring, sparkline};
use super::info::{self, MONTHS};
use super::layout::{self, Hit, MediaLayout, PerfCard, Rect, Tab};
use super::mpris::{self, Player, Status};
use super::sysinfo::{self, HISTORY};
use super::Dash;
use crate::gfx::{color, fill_circle, fill_rect, fill_rounded, stroke_rounded};
use crate::menubar::clock;
use crate::menubar::popup::month_grid;
use crate::text::{Role, Weight};
use crate::theme::Theme;
use std::time::Instant;
use tiny_skia::{Color, Pixmap};

const WEEKDAY_HEADS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

impl Dash {
    pub(super) fn media_layout(&self) -> MediaLayout {
        layout::media_layout(self.players.len(), self.active().is_some_and(|p| p.volume.is_some()))
    }

    // ------------------------------------------------------------ helpers

    fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(px * 0.55 * s.chars().count() as f32, |f| f.measure(s, px, w))
    }

    /// Draws `s` with the top of its line box at `top`; returns the width.
    #[allow(clippy::too_many_arguments)] // a drawing helper: target, text, position, size, weight, colour
    fn text(&mut self, pm: &mut Pixmap, s: &str, x: f32, top: f32, px: f32, w: Weight, c: Color) -> f32 {
        self.text_in(Role::Ui, pm, s, x, top, px, w, c)
    }

    /// [`Self::text`] in the font of `role` (numbers are monospace, the clock a display face).
    #[allow(clippy::too_many_arguments)]
    fn text_in(&mut self, role: Role, pm: &mut Pixmap, s: &str, x: f32, top: f32, px: f32, w: Weight, c: Color) -> f32 {
        self.fonts.as_mut().map_or(0.0, |f| f.draw_as(role, pm, s, x.round(), top.round(), px, w, c))
    }

    #[allow(clippy::too_many_arguments)]
    fn text_c_in(&mut self, role: Role, pm: &mut Pixmap, s: &str, cx: f32, top: f32, px: f32, w: Weight, c: Color) {
        let width = self.fonts.as_ref().map_or(px * 0.55 * s.chars().count() as f32, |f| f.measure_as(role, s, px, w));
        self.text_in(role, pm, s, cx - width / 2.0, top, px, w, c);
    }

    #[allow(clippy::too_many_arguments)]
    fn text_c(&mut self, pm: &mut Pixmap, s: &str, cx: f32, top: f32, px: f32, w: Weight, c: Color) {
        let width = self.measure(s, px, w);
        self.text(pm, s, cx - width / 2.0, top, px, w, c);
    }

    #[allow(clippy::too_many_arguments)]
    fn text_r(&mut self, pm: &mut Pixmap, s: &str, right: f32, top: f32, px: f32, w: Weight, c: Color) {
        let width = self.measure(s, px, w);
        self.text(pm, s, right - width, top, px, w, c);
    }

    /// `s` cut with an ellipsis so it fits `max` pixels.
    fn fit(&self, s: &str, px: f32, w: Weight, max: f32) -> String {
        if self.measure(s, px, w) <= max {
            return s.to_string();
        }
        let mut chars: Vec<char> = s.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let cand: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
            if self.measure(&cand, px, w) <= max {
                return cand;
            }
        }
        String::new()
    }

    /// Top of a line of `px` text centred vertically in a box starting at `y` that is `h` high.
    fn mid(y: f32, h: f32, px: f32) -> f32 {
        y + (h - px * 1.25) / 2.0
    }

    fn symbol(&mut self, pm: &mut Pixmap, name: &str, r: Rect, rgb: (u8, u8, u8), alpha: f32) {
        let px = (r.w * 2.0).ceil() as u32;
        if let Some(sym) = self.symbols.get(name, rgb, px) {
            crate::gfx::blit_scaled(pm, sym, r.x, r.y, r.w, r.h, alpha);
        }
    }

    fn surface(pm: &mut Pixmap, th: &Theme, r: Rect, hover: bool) {
        fill_rounded(pm, r.x, r.y, r.w, r.h, th.r(1.0), th.ink(if hover { 0.11 } else { 0.06 }));
    }

    // ------------------------------------------------------------ the card

    pub(super) fn paint(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        pm.fill(Color::TRANSPARENT);
        let radius = th.r(1.8);
        let base = if self.alpha { th.overlay_c() } else { th.overlay.color(1.0) };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, base);
        self.draw_tabs(pm, &th);
        let now = Instant::now();
        match self.tab {
            Tab::Overview => self.draw_overview(pm, &th, now),
            Tab::Media => self.draw_media(pm, &th, now),
            Tab::Performance => self.draw_performance(pm, &th),
        }
        stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), th.outline_c(), 1.0);
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }

    fn draw_tabs(&mut self, pm: &mut Pixmap, th: &Theme) {
        for (i, t) in Tab::ALL.iter().enumerate() {
            let r = layout::tab_rect(i);
            let selected = *t == self.tab;
            if selected {
                fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, th.ink(0.14));
            } else if self.hover == Some(Hit::Tab(*t)) {
                fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, th.ink(0.07));
            }
            let (px, weight) = (13.0, if selected { Weight::SemiBold } else { Weight::Medium });
            let c = if selected { th.text_c() } else { th.sub() };
            let top = Self::mid(r.y, r.h, px);
            self.text_c(pm, t.label(), r.cx(), top, px, weight, c);
        }
        fill_rect(pm, layout::PAD, layout::TABS_H - 6.0, layout::CARD_W - 2.0 * layout::PAD, 1.0, th.sep());
    }

    // ------------------------------------------------------------ overview

    fn draw_overview(&mut self, pm: &mut Pixmap, th: &Theme, now: Instant) {
        let l = layout::overview_layout();
        let c = layout::content();

        // User card.
        if let Some(av) = &self.avatar {
            image_circle(pm, av, l.avatar);
        } else {
            let (r, g, b) = info::tile_color(&self.user.login);
            fill_circle(pm, l.avatar.cx(), l.avatar.cy(), l.avatar.w / 2.0, color(r, g, b, 1.0));
            let initial = self.user.initial();
            self.text_c(pm, &initial, l.avatar.cx(), Self::mid(l.avatar.y, l.avatar.h, 26.0), 26.0, Weight::SemiBold, color(255, 255, 255, 1.0));
        }
        let name = self.fit(&self.user.display.clone(), 16.0, Weight::SemiBold, l.name.w);
        self.text(pm, &name, l.name.x, l.name.y, 16.0, Weight::SemiBold, th.text_c());
        let handle = self.fit(&self.user.handle(), 12.0, Weight::Regular, l.name.w);
        self.text(pm, &handle, l.name.x, l.name.y + 21.0, 12.0, Weight::Regular, th.sub());
        if let Some(up) = self.cur.uptime {
            self.text(pm, &sysinfo::format_uptime(up), l.name.x, l.name.y + 37.0, 12.0, Weight::Regular, th.faint());
        }

        // Clock and date.
        if let Some(tm) = clock::now() {
            let face = info::clock_face(tm.hour, tm.min, self.hour12);
            self.text_in(Role::Display, pm, &face, l.time.x - 1.0, l.time.y - 2.0, 42.0, Weight::Bold, th.text_c());
            self.text(pm, &info::long_date(tm.wday, tm.mday, tm.mon), l.date.x, l.date.y, 14.0, Weight::Medium, th.sub());
        }

        // Weather.
        Self::surface(pm, th, l.weather, false);
        match self.wx.clone() {
            Some(wx) => {
                let sym = crate::menubar::weather::symbol_for(wx.code, wx.night);
                self.symbol(pm, sym, Rect::new(l.weather.x + 14.0, l.weather.cy() - 15.0, 30.0, 30.0), th.text.t(), 1.0);
                self.text(pm, &format!("{}°", wx.temp_c), l.weather.x + 60.0, l.weather.y + 8.0, 26.0, Weight::SemiBold, th.text_c());
                self.text(pm, info::condition_label(wx.code, wx.night), l.weather.x + 60.0, l.weather.y + 38.0, 12.0, Weight::Regular, th.sub());
                let fresh = crate::menubar::weather::is_fresh(&wx, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()));
                if !fresh {
                    self.text_r(pm, "cached", l.weather.right() - 12.0, l.weather.y + 8.0, 10.0, Weight::Regular, th.faint());
                }
            }
            None => {
                let msg = if self.wx_loading.load(std::sync::atomic::Ordering::Relaxed) { "Fetching weather…" } else { "Weather unavailable" };
                self.text_c(pm, msg, l.weather.cx(), Self::mid(l.weather.y, l.weather.h, 13.0), 13.0, Weight::Regular, th.faint());
            }
        }

        // Month calendar.
        self.draw_calendar(pm, th, l.calendar);

        fill_rect(pm, c.x, l.divider_y, c.w, 1.0, th.sep());

        // Resource rings.
        let rings: [(&str, Option<f32>); 3] =
            [("CPU", Some(self.cur.cpu)), ("Memory", (self.cur.mem.total_kb > 0).then_some(self.cur.mem.used_frac())), ("Disk", self.cur.disk.map(|d| d.used_frac()))];
        for (r, (label, frac)) in l.rings.iter().zip(rings) {
            let f = frac.unwrap_or(0.0);
            let cy = r.y + 31.0;
            ring(pm, r.cx(), cy, 26.0, 6.0, f, th.ink(0.12), load_color(th, f));
            let pct = frac.map_or("--".to_string(), |f| format!("{:.0}%", f * 100.0));
            self.text_c(pm, &pct, r.cx(), cy - 9.0, 14.0, Weight::SemiBold, th.text_c());
            self.text_c(pm, label, r.cx(), r.y + 66.0, 11.0, Weight::Medium, th.sub());
        }

        // Now playing.
        let hover = self.hover == Some(Hit::NowPlaying);
        Self::surface(pm, th, l.playing, hover);
        let active = self.active().cloned();
        match active {
            Some(p) => {
                let thumb = Rect::new(l.playing.x + 12.0, l.playing.cy() - 29.0, 58.0, 58.0);
                self.draw_cover(pm, th, thumb, 8.0, &p, 22.0);
                let tx = thumb.right() + 12.0;
                let max_w = l.playing.right() - tx - 12.0;
                let title = if p.meta.title.is_empty() { "Unknown title".to_string() } else { p.meta.title.clone() };
                let title = self.fit(&title, 13.0, Weight::SemiBold, max_w);
                self.text(pm, &title, tx, thumb.y + 4.0, 13.0, Weight::SemiBold, th.text_c());
                let artist = self.fit(&p.meta.artist, 12.0, Weight::Regular, max_w);
                self.text(pm, &artist, tx, thumb.y + 24.0, 12.0, Weight::Regular, th.sub());
                let state = match p.status {
                    Status::Playing => "Playing",
                    Status::Paused => "Paused",
                    Status::Stopped => "Stopped",
                };
                let line = self.fit(&format!("{state} · {}", p.label()), 11.0, Weight::Regular, max_w);
                self.text(pm, &line, tx, thumb.y + 42.0, 11.0, Weight::Regular, th.faint());
            }
            None => {
                self.text_c(pm, "Nothing playing", l.playing.cx(), Self::mid(l.playing.y, l.playing.h, 13.0), 13.0, Weight::Regular, th.faint());
            }
        }
        let _ = now;
    }

    fn draw_calendar(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect) {
        let Some((year, mon0, today)) = clock::today() else { return };
        self.text(pm, &format!("{} {}", MONTHS[mon0 as usize % 12], year), r.x, r.y + 1.0, 14.0, Weight::Bold, th.text_c());
        let cell_w = r.w / 7.0;
        for (i, d) in WEEKDAY_HEADS.iter().enumerate() {
            self.text_c(pm, d, r.x + cell_w * (i as f32 + 0.5), r.y + 26.0, 11.0, Weight::Medium, th.ink(if i >= 5 { 0.35 } else { 0.5 }));
        }
        let grid = month_grid(year, mon0);
        for (row, cells) in grid.iter().enumerate() {
            for (col, d) in cells.iter().enumerate() {
                let Some(d) = d else { continue };
                let cx = r.x + cell_w * (col as f32 + 0.5);
                let cy = r.y + 56.0 + row as f32 * 25.0;
                let is_today = *d == today;
                if is_today {
                    fill_circle(pm, cx, cy, 12.0, th.accent_c());
                }
                let c = if is_today { th.accent_text_c() } else { th.ink(if col >= 5 { 0.55 } else { 1.0 }) };
                self.text_c(pm, &d.to_string(), cx, cy - 8.0, 12.0, if is_today { Weight::Bold } else { Weight::Regular }, c);
            }
        }
    }

    /// Cover art in `r`, or a coloured tile with the player's or track's initial.
    fn draw_cover(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect, radius: f32, p: &Player, letter_px: f32) {
        if let Some(art) = &self.art.pixmap {
            image_rounded(pm, art, r, radius);
            return;
        }
        let seed = if p.meta.album.is_empty() { p.label() } else { p.meta.album.clone() };
        let (cr, cg, cb) = info::tile_color(&seed);
        fill_rounded(pm, r.x, r.y, r.w, r.h, radius, color(cr, cg, cb, 0.9));
        let letter: String = p.meta.title.chars().chain(p.label().chars()).find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "♪".into());
        self.text_c(pm, &letter, r.cx(), Self::mid(r.y, r.h, letter_px), letter_px, Weight::SemiBold, color(255, 255, 255, 0.95));
        let _ = th;
    }

    // ------------------------------------------------------------ media

    fn draw_media(&mut self, pm: &mut Pixmap, th: &Theme, now: Instant) {
        let l = self.media_layout();
        let c = layout::content();
        let Some(p) = self.active().cloned() else {
            let msg = if self.bus_up { "No media playing" } else { "No session bus" };
            let sub = if self.bus_up { "Start a player that supports MPRIS" } else { "MPRIS needs a D-Bus session" };
            let cy = c.y + c.h / 2.0;
            let r = Rect::new(c.cx() - 24.0, cy - 64.0, 48.0, 48.0);
            fill_circle(pm, r.cx(), r.cy(), 28.0, th.ink(0.07));
            self.symbol(pm, "play", Rect::new(r.x + 10.0, r.y + 10.0, 28.0, 28.0), th.text.t(), 0.35);
            self.text_c(pm, msg, c.cx(), cy + 2.0, 16.0, Weight::SemiBold, th.text_c());
            self.text_c(pm, sub, c.cx(), cy + 26.0, 12.0, Weight::Regular, th.sub());
            return;
        };

        // Cover with a soft shadow, then the text column.
        crate::gfx::soft_shadow(pm, l.art.x, l.art.y + 4.0, l.art.w, l.art.h, 14.0, 8.0, 0.30);
        self.draw_cover(pm, th, l.art, 14.0, &p, 84.0);
        let title = if p.meta.title.is_empty() { "Unknown title".to_string() } else { p.meta.title.clone() };
        let title = self.fit(&title, 20.0, Weight::SemiBold, l.title.w);
        self.text(pm, &title, l.title.x, l.title.y, 20.0, Weight::SemiBold, th.text_c());
        let artist = if p.meta.artist.is_empty() { "Unknown artist".to_string() } else { p.meta.artist.clone() };
        let artist = self.fit(&artist, 14.0, Weight::Medium, l.artist.w);
        self.text(pm, &artist, l.artist.x, l.artist.y, 14.0, Weight::Medium, th.ink(0.85));
        if !p.meta.album.is_empty() {
            let album = self.fit(&p.meta.album, 12.0, Weight::Regular, l.album.w);
            self.text(pm, &album, l.album.x, l.album.y, 12.0, Weight::Regular, th.sub());
        }

        // Progress.
        let drag_seek = self.drag == Some(super::Drag::Seek);
        let frac = match (drag_seek, self.drag_value) {
            (true, Some(f)) => f,
            _ => p.progress(now),
        };
        bar(pm, l.bar, frac, th.ink(0.15), th.accent_c());
        let hover_bar = drag_seek || self.hover == Some(Hit::Seek(0.0));
        if p.meta.length_us.is_some() && (hover_bar || p.status == Status::Playing) {
            fill_circle(pm, l.bar.x + l.bar.w * frac.clamp(0.0, 1.0), l.bar.cy(), if hover_bar { 6.0 } else { 4.5 }, th.text_c());
        }
        let pos = match (drag_seek, self.drag_value, p.meta.length_us) {
            (true, Some(f), Some(len)) => (f64::from(f) * len as f64) as i64,
            _ => p.position_at(now),
        };
        self.text(pm, &mpris::format_time(pos), l.time_l.x, l.time_l.y, 11.0, Weight::Regular, th.sub());
        let total = p.meta.length_us.map_or("--:--".to_string(), mpris::format_time);
        self.text_r(pm, &total, l.time_r.right(), l.time_r.y, 11.0, Weight::Regular, th.sub());

        // Transport buttons.
        let controls: [(Rect, &str, bool, Hit); 3] = [
            (l.prev, "backward", p.can_control && p.can_prev, Hit::Prev),
            (l.play, if p.status == Status::Playing { "pause" } else { "play" }, p.can_control, Hit::PlayPause),
            (l.next, "forward", p.can_control && p.can_next, Hit::Next),
        ];
        for (r, icon, enabled, hit) in controls {
            let hovered = self.hover == Some(hit) && enabled;
            if hit == Hit::PlayPause {
                fill_circle(pm, r.cx(), r.cy(), r.w / 2.0, if hovered { th.accent.color(1.0) } else { th.ink(0.14) });
            } else if hovered {
                fill_circle(pm, r.cx(), r.cy(), r.w / 2.0, th.ink(0.10));
            }
            let size = if hit == Hit::PlayPause { 24.0 } else { 20.0 };
            let rgb = if hit == Hit::PlayPause && hovered { th.accent_text.t() } else { th.text.t() };
            self.symbol(pm, icon, Rect::new(r.cx() - size / 2.0, r.cy() - size / 2.0, size, size), rgb, if enabled { 1.0 } else { 0.3 });
        }

        // Volume.
        if let Some(v) = p.volume {
            let shown = match (self.drag == Some(super::Drag::Volume), self.drag_value) {
                (true, Some(f)) => f64::from(f),
                _ => v,
            };
            let icon = match shown {
                x if x <= 0.001 => "speaker-0",
                x if x < 0.34 => "speaker-1",
                x if x < 0.67 => "speaker-2",
                _ => "speaker-3",
            };
            self.symbol(pm, icon, l.vol_icon, th.text.t(), 0.8);
            bar(pm, l.vol_bar, shown as f32, th.ink(0.15), th.ink(0.75));
            let active = self.drag == Some(super::Drag::Volume) || self.hover == Some(Hit::Volume(0.0));
            fill_circle(pm, l.vol_bar.x + l.vol_bar.w * (shown as f32).clamp(0.0, 1.0), l.vol_bar.cy(), if active { 6.0 } else { 4.5 }, th.text_c());
            self.text_r(pm, &format!("{:.0}%", shown * 100.0), l.vol_hit.right(), l.vol_bar.y + 14.0, 11.0, Weight::Regular, th.faint());
        }

        // Player picker (several players) or the one player's name.
        if l.chips.is_empty() {
            let who = self.fit(&p.label(), 11.0, Weight::Regular, l.who.w);
            self.text(pm, &who, l.who.x, l.who.y + 3.0, 11.0, Weight::Regular, th.faint());
        } else {
            let players = self.players.clone();
            for (i, (r, pl)) in l.chips.iter().zip(players.iter()).enumerate() {
                let sel = pl.bus == p.bus;
                let hovered = self.hover == Some(Hit::Chip(i));
                let bg = if sel {
                    th.accent_c()
                } else if hovered {
                    th.ink(0.14)
                } else {
                    th.ink(0.07)
                };
                fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, bg);
                let label = self.fit(&pl.label(), 12.0, Weight::Medium, r.w - 20.0);
                let c = if sel { th.accent_text_c() } else { th.text_c() };
                self.text_c(pm, &label, r.cx(), Self::mid(r.y, r.h, 12.0), 12.0, Weight::Medium, c);
            }
        }
    }

    // ------------------------------------------------------------ performance

    fn card_title(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect, title: &str) {
        self.text(pm, title, r.x + 14.0, r.y + 10.0, 11.0, Weight::Bold, th.sub());
    }

    fn draw_performance(&mut self, pm: &mut Pixmap, th: &Theme) {
        let gpu = self.cur.gpu.clone();
        for (card, r) in layout::perf_layout(gpu.is_some()) {
            Self::surface(pm, th, r, false);
            match card {
                PerfCard::Cpu => self.draw_cpu(pm, th, r),
                PerfCard::Network => self.draw_network(pm, th, r),
                PerfCard::Memory => self.draw_memory(pm, th, r),
                PerfCard::Storage => self.draw_storage(pm, th, r),
                PerfCard::Gpu => {
                    if let Some(g) = &gpu {
                        self.draw_gpu(pm, th, r, g);
                    }
                }
            }
        }
    }

    fn gauge(&mut self, pm: &mut Pixmap, th: &Theme, cx: f32, cy: f32, radius: f32, frac: Option<f32>) {
        let f = frac.unwrap_or(0.0);
        ring(pm, cx, cy, radius, radius * 0.24, f, th.ink(0.12), load_color(th, f));
        let pct = frac.map_or("--".to_string(), |f| format!("{:.0}%", f * 100.0));
        let px = (radius * 0.52).clamp(11.0, 16.0);
        self.text_c_in(Role::Mono, pm, &pct, cx, cy - px * 0.62, px, Weight::SemiBold, th.text_c());
    }

    fn draw_cpu(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect) {
        self.card_title(pm, th, r, "CPU");
        let s = self.cur.clone();
        self.gauge(pm, th, r.x + 52.0, r.y + 66.0, 31.0, Some(s.cpu));
        let tx = r.x + 108.0;
        let temp = s.cpu_temp.map_or("--".to_string(), |t| format!("{t:.0}°C"));
        self.text_in(Role::Mono, pm, &temp, tx, r.y + 34.0, 18.0, Weight::SemiBold, th.text_c());
        self.text(pm, "Temperature", tx, r.y + 56.0, 11.0, Weight::Regular, th.sub());
        let load = s.load1.map_or("--".to_string(), |l| format!("{l:.2}"));
        self.text_in(Role::Mono, pm, &load, tx, r.y + 76.0, 18.0, Weight::SemiBold, th.text_c());
        self.text(pm, &format!("Load, {} threads", s.cores.len()), tx, r.y + 98.0, 11.0, Weight::Regular, th.sub());
        meters(pm, Rect::new(r.x + 14.0, r.y + 118.0, r.w - 28.0, 30.0), &s.cores, th.ink(0.10), th.accent_c());
        sparkline(pm, Rect::new(r.x + 14.0, r.y + 158.0, r.w - 28.0, 32.0), &s.cpu_hist, 1.0, HISTORY, th.accent_c(), true);
    }

    fn draw_network(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect) {
        self.card_title(pm, th, r, "NETWORK");
        let s = self.cur.clone();
        let (down, up) = (th.accent_c(), th.ok.color(1.0));
        let half = (r.w - 28.0) / 2.0;
        for (i, (label, rate, c)) in [("Down", s.rx, down), ("Up", s.tx, up)].into_iter().enumerate() {
            let x = r.x + 14.0 + i as f32 * half;
            arrow(pm, x + 5.0, r.y + 38.0, 9.0, i == 1, c);
            self.text_in(Role::Mono, pm, &sysinfo::human_rate(rate), x + 16.0, r.y + 28.0, 14.0, Weight::SemiBold, th.text_c());
            self.text(pm, label, x + 16.0, r.y + 46.0, 10.0, Weight::Regular, th.sub());
        }
        let area = Rect::new(r.x + 14.0, r.y + 64.0, r.w - 28.0, r.h - 64.0 - 10.0);
        let top = s.rx_hist.iter().chain(&s.tx_hist).copied().fold(0.0f32, f32::max).max(50_000.0);
        sparkline(pm, area, &s.rx_hist, top, HISTORY, down, true);
        sparkline(pm, area, &s.tx_hist, top, HISTORY, up, false);
    }

    /// Vertical shift of the small cards' contents: clear of the title, and centred when the card is taller than the 96 px design.
    fn spread(r: Rect) -> f32 {
        4.0 + ((r.h - 96.0) / 2.0).max(0.0)
    }

    fn draw_memory(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect) {
        self.card_title(pm, th, r, "MEMORY");
        let (m, dy) = (self.cur.mem, Self::spread(r));
        let known = m.total_kb > 0;
        self.gauge(pm, th, r.x + 40.0, r.y + 46.0 + dy, 21.0, known.then(|| m.used_frac()));
        let tx = r.x + 78.0;
        self.text_in(Role::Mono, pm, &sysinfo::human_kb(m.used_kb()), tx, r.y + 24.0 + dy, 16.0, Weight::SemiBold, th.text_c());
        self.text(pm, &format!("of {}", sysinfo::human_kb(m.total_kb)), tx, r.y + 44.0 + dy, 11.0, Weight::Regular, th.sub());
        let swap = if m.swap_total_kb == 0 { "No swap".to_string() } else { format!("Swap {} / {}", sysinfo::human_kb(m.swap_used_kb()), sysinfo::human_kb(m.swap_total_kb)) };
        let swap = self.fit(&swap, 11.0, Weight::Regular, r.right() - tx - 10.0);
        self.text(pm, &swap, tx, r.y + 62.0 + dy, 11.0, Weight::Regular, th.sub());
        bar(pm, Rect::new(tx, r.y + 80.0 + dy, r.right() - tx - 14.0, 4.0), m.swap_frac(), th.ink(0.12), load_color(th, m.swap_frac()));
    }

    fn draw_storage(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect) {
        self.card_title(pm, th, r, "STORAGE  /");
        let dy = Self::spread(r);
        let disk = self.cur.disk;
        self.gauge(pm, th, r.x + 40.0, r.y + 46.0 + dy, 21.0, disk.map(|d| d.used_frac()));
        let tx = r.x + 78.0;
        if let Some(d) = disk {
            self.text_in(Role::Mono, pm, &sysinfo::human_bytes(d.used), tx, r.y + 24.0 + dy, 16.0, Weight::SemiBold, th.text_c());
            self.text(pm, &format!("of {}", sysinfo::human_bytes(d.total)), tx, r.y + 44.0 + dy, 11.0, Weight::Regular, th.sub());
            self.text(pm, &format!("{} free", sysinfo::human_bytes(d.total.saturating_sub(d.used))), tx, r.y + 62.0 + dy, 11.0, Weight::Regular, th.sub());
            bar(pm, Rect::new(tx, r.y + 80.0 + dy, r.right() - tx - 14.0, 4.0), d.used_frac(), th.ink(0.12), load_color(th, d.used_frac()));
        } else {
            self.text(pm, "Unavailable", tx, r.y + 30.0 + dy, 14.0, Weight::Regular, th.faint());
        }
    }

    fn draw_gpu(&mut self, pm: &mut Pixmap, th: &Theme, r: Rect, g: &sysinfo::Gpu) {
        self.card_title(pm, th, r, "GPU");
        let name = self.fit(&g.name.replace("NVIDIA ", "").replace("GeForce ", ""), 11.0, Weight::Regular, r.w - 70.0);
        self.text_r(pm, &name, r.right() - 14.0, r.y + 10.0, 11.0, Weight::Regular, th.faint());
        self.gauge(pm, th, r.x + 40.0, r.y + 58.0, 25.0, g.util.map(|u| u / 100.0));
        let tx = r.x + 82.0;
        let temp = g.temp.map_or("--".to_string(), |t| format!("{t:.0}°C"));
        self.text_in(Role::Mono, pm, &temp, tx, r.y + 30.0, 18.0, Weight::SemiBold, th.text_c());
        self.text(pm, "Temperature", tx, r.y + 52.0, 11.0, Weight::Regular, th.sub());
        let vram = match (g.mem_used_mib, g.mem_total_mib) {
            (Some(u), Some(t)) => format!("VRAM {} / {}", sysinfo::human_bytes(u << 20), sysinfo::human_bytes(t << 20)),
            _ => "VRAM --".to_string(),
        };
        let vram = self.fit(&vram, 11.0, Weight::Regular, r.right() - tx - 10.0);
        self.text(pm, &vram, tx, r.y + 72.0, 11.0, Weight::Regular, th.sub());
        let f = g.mem_frac().unwrap_or(0.0);
        bar(pm, Rect::new(tx, r.y + 92.0, r.right() - tx - 14.0, 4.0), f, th.ink(0.12), load_color(th, f));
    }
}
