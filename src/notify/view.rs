//! Frontend-neutral half of decnotify's banners and history panel: the queue, layout, deadlines, hover/press
//! semantics, the history panel's keyboard and scroll behaviour, and all painting. No display-server types appear
//! here; [`super::ui`] (X11) and [`super::wl`] (Wayland layer-shell) supply a [`Host`] that creates, places and
//! presents surfaces, and translate their native input into the calls below.

use super::model::{Image, Notification};
use crate::gfx::{self, color};
use crate::icons;
use crate::text::{Fonts, Role, Weight};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use tiny_skia::{Color, Pixmap};

/// Banners on screen at once; further ones wait in a queue.
pub const MAX_VISIBLE: usize = 5;
pub const BANNER_W: f32 = 344.0;
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;
pub const HIST_W: f32 = 380.0;
pub const HIST_ROW: f32 = 60.0;
pub const HIST_MAX_ROWS: usize = 8;

/// What the front end reports back to the D-Bus side.
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// (id, reason) with the spec's reasons: 1 expired, 2 dismissed by the user, 3 closed by a call.
    Closed(u32, u32),
    Action(u32, String),
}

/// Colours. The adaptation point for the shared theme module: build a `Palette` from whatever it exposes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: (u8, u8, u8),
    pub border: Color,
    pub title: Color,
    pub body: Color,
    pub dim: Color,
    pub button: Color,
    pub button_hover: Color,
    pub accent: (u8, u8, u8),
    pub link: Color,
}

impl Palette {
    pub fn dark() -> Palette {
        Palette {
            bg: (42, 42, 44),
            border: color(255, 255, 255, 0.12),
            title: color(255, 255, 255, 0.95),
            body: color(255, 255, 255, 0.72),
            dim: color(255, 255, 255, 0.45),
            button: color(255, 255, 255, 0.12),
            button_hover: color(255, 255, 255, 0.22),
            accent: (10, 132, 255),
            link: color(10, 132, 255, 1.0),
        }
    }

    pub fn light() -> Palette {
        Palette {
            bg: (246, 246, 248),
            border: color(0, 0, 0, 0.14),
            title: color(0, 0, 0, 0.9),
            body: color(0, 0, 0, 0.65),
            dim: color(0, 0, 0, 0.42),
            button: color(0, 0, 0, 0.07),
            button_hover: color(0, 0, 0, 0.14),
            accent: (0, 122, 255),
            link: color(0, 122, 255, 1.0),
        }
    }

    /// Read only the process-wide resolved theme cache; no I/O in rendering.
    pub fn current() -> Palette {
        Self::from_theme(&crate::theme::th())
    }

    pub fn from_theme(theme: &crate::theme::Theme) -> Palette {
        let bg = theme.popup;
        // Keep all label backgrounds on the same side of the contrast boundary.
        let shade = if bg.readable_on() == crate::theme::Rgb(0, 0, 0) { crate::theme::Rgb(255, 255, 255) } else { crate::theme::Rgb(0, 0, 0) };
        let button = shade.over(bg, 0.08);
        let hover = shade.over(bg, 0.14);
        let readable = |fg, backgrounds: &[crate::theme::Rgb]| readable_foreground(fg, backgrounds).color(1.0);
        Palette {
            bg: bg.t(),
            border: theme.border.color(1.0),
            title: readable(theme.text, &[bg, button, hover]),
            body: readable(theme.text_secondary, &[bg, button]),
            dim: readable(theme.text_faint, &[bg]),
            button: button.color(1.0),
            button_hover: hover.color(1.0),
            accent: theme.accent.t(),
            link: readable(theme.accent, &[bg]),
        }
    }

    pub fn bg_color(&self) -> Color {
        color(self.bg.0, self.bg.1, self.bg.2, 1.0)
    }

    pub fn bg_pixel(&self) -> u32 {
        (u32::from(self.bg.0) << 16) | (u32::from(self.bg.1) << 8) | u32::from(self.bg.2)
    }
}

/// Preserve theme colours where possible; tiny notification text needs 4.5:1.
fn readable_foreground(fg: crate::theme::Rgb, backgrounds: &[crate::theme::Rgb]) -> crate::theme::Rgb {
    use crate::theme::model::{contrast, Rgb};
    let score = |c| backgrounds.iter().map(|bg| contrast(c, *bg)).fold(f32::INFINITY, f32::min);
    let black = Rgb(0, 0, 0);
    let white = Rgb(255, 255, 255);
    let fallback = if score(black) >= score(white) { black } else { white };
    for step in 0..=100 {
        let candidate = fallback.over(fg, step as f32 / 100.0);
        if score(candidate) >= 4.5 {
            return candidate;
        }
    }
    fallback
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

// ------------------------------------------------------------- text helpers

pub fn ellipsize(f: &Fonts, s: &str, px: f32, wt: Weight, max_w: f32) -> String {
    ellipsize_role(f, Role::Ui, s, px, wt, max_w)
}

pub fn ellipsize_role(f: &Fonts, role: Role, s: &str, px: f32, wt: Weight, max_w: f32) -> String {
    if f.measure_as(role, s, px, wt) <= max_w {
        return s.to_string();
    }
    let mut out: Vec<char> = s.chars().collect();
    while !out.is_empty() {
        out.pop();
        let cand: String = out.iter().collect::<String>() + "\u{2026}";
        if f.measure_as(role, &cand, px, wt) <= max_w {
            return cand;
        }
    }
    String::new()
}

/// Greedy word wrap into at most `max_lines` lines; the last line is ellipsized when text is cut off.
pub fn wrap(f: &Fonts, text: &str, px: f32, wt: Weight, max_w: f32, max_lines: usize) -> Vec<String> {
    let text: String = text.chars().take(600).collect();
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let joined = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if f.measure(&joined, px, wt) <= max_w {
                line = joined;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // A single word wider than the line is broken by characters.
            let mut piece = String::new();
            for ch in word.chars() {
                piece.push(ch);
                if f.measure(&piece, px, wt) > max_w && piece.chars().count() > 1 {
                    let last = piece.pop().unwrap_or(' ');
                    lines.push(std::mem::take(&mut piece));
                    piece.push(last);
                }
            }
            line = piece;
        }
        lines.push(line);
        if lines.len() > max_lines {
            break;
        }
    }
    while lines.last().is_some_and(String::is_empty) && lines.len() > 1 {
        lines.pop();
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        let last = lines.pop().unwrap_or_default();
        lines.push(ellipsize(f, &format!("{last} \u{2026}"), px, wt, max_w));
    }
    if lines.len() == 1 && lines[0].is_empty() {
        lines.clear();
    }
    lines
}

// ------------------------------------------------------------------ icons

pub fn image_pixmap(img: &Image, size: u32) -> Option<Pixmap> {
    let mut src = Pixmap::new(img.w, img.h)?;
    for (d, s) in src.data_mut().chunks_exact_mut(4).zip(img.rgba.chunks_exact(4)) {
        let a = u32::from(s[3]);
        d[0] = (u32::from(s[0]) * a / 255) as u8;
        d[1] = (u32::from(s[1]) * a / 255) as u8;
        d[2] = (u32::from(s[2]) * a / 255) as u8;
        d[3] = s[3];
    }
    let mut out = Pixmap::new(size, size)?;
    let scale = (size as f32 / img.w as f32).min(size as f32 / img.h as f32);
    let (w, h) = (img.w as f32 * scale, img.h as f32 * scale);
    gfx::blit_scaled(&mut out, &src, (size as f32 - w) / 2.0, (size as f32 - h) / 2.0, w, h, 1.0);
    Some(out)
}

pub fn load_icon_file(path: &std::path::Path, size: u32) -> Option<Pixmap> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => icons::render_svg(path, size),
        Some("png") => icons::load_png_scaled(path, size),
        _ => None,
    }
}

/// Theme icon name, absolute path or `file://` URI; falls back to the desktop entry and app name.
pub fn lookup_icon(icon: &str, app: &str, entry: &str, size: u32) -> Option<Pixmap> {
    let direct = icon.strip_prefix("file://").unwrap_or(icon);
    if direct.starts_with('/') {
        if let Some(p) = load_icon_file(std::path::Path::new(direct), size) {
            return Some(p);
        }
    }
    let app_dash = app.to_lowercase().replace(' ', "-");
    for name in [icon, entry, app, app_dash.as_str()] {
        if name.is_empty() || name.starts_with('/') || name.contains("..") {
            continue;
        }
        if let Some(p) = icons::load_named(name, size) {
            return Some(p);
        }
    }
    None
}

pub fn tile_color(app: &str) -> (u8, u8, u8) {
    const TILES: [(u8, u8, u8); 6] = [(10, 132, 255), (48, 209, 88), (255, 159, 10), (191, 90, 242), (255, 69, 58), (100, 210, 255)];
    TILES[app.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % TILES.len()]
}

/// Draws the icon (or a lettered tile) into the `size` square at (x, y).
pub fn draw_icon(fonts: &mut Fonts, pm: &mut Pixmap, icon: Option<&Pixmap>, app: &str, x: f32, y: f32, size: f32) {
    if let Some(i) = icon {
        gfx::blit_scaled(pm, i, x, y, size, size, 1.0);
        return;
    }
    let (r, g, b) = tile_color(app);
    gfx::fill_rounded(pm, x, y, size, size, size * 0.23, color(r, g, b, 1.0));
    let letter: String = app.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "!".into());
    let px = size * 0.5 / crate::theme::live::font_scale().max(0.1);
    let w = fonts.measure(&letter, px, Weight::SemiBold);
    fonts.draw(pm, &letter, x + (size - w) / 2.0, y + size * 0.22, px, Weight::SemiBold, Color::WHITE);
}

pub fn line_height(fonts: &Fonts, role: Role, px: f32, weight: Weight, s: f32) -> f32 {
    (fonts.ascent_as(role, px, weight) + px * crate::theme::live::font_scale() * 0.3 + 2.0 * s).ceil()
}

// ---------------------------------------------------------------- geometry

/// Where the banners may go and how large the UI is, in the front end's own pixels: device pixels on X11
/// (`s` = Xft dpi / 96) and on Wayland (`s` = the output's buffer scale, so a 1.5x output lays out at 1.5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub s: f32,
    /// The monitor (or the part of an output the compositor leaves free) the stack lives on.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Top edge of the first banner (absolute, below the bar).
    pub top: i32,
    /// Gap between the banners and the right edge of that area.
    pub right_gap: i32,
}

impl Metrics {
    pub fn px(&self, v: f32) -> f32 {
        v * self.s
    }

    pub fn margin(&self) -> i32 {
        self.px(MARGIN).round() as i32
    }

    pub fn gap(&self) -> i32 {
        self.px(GAP).round() as i32
    }

    pub fn available_height(&self) -> f32 {
        (self.y + self.h - self.top - self.margin()).max(1) as f32
    }

    pub fn surface_width(&self, base: f32) -> f32 {
        let growth = (crate::theme::live::font_scale() - 1.0).max(0.0);
        (base * self.s * (1.0 + growth * 0.5)).min((self.w - self.right_gap - self.margin()).max(1) as f32).max(1.0)
    }

    pub fn x_for(&self, w: u32) -> i32 {
        (self.x + self.w - w as i32 - self.right_gap).max(self.x)
    }
}

/// Position of the first banner: `(top, right_gap)` for an area starting at `mon_y`, `mon_h` high, at scale `s`.
/// `bar_bottom` is the bar's top-edge extent in logical pixels, `pill` says the Caelestia pill bar is in use and
/// `right_inset` is that bar's width on the right edge.
pub fn stack_origin(s: f32, mon_y: i32, mon_h: i32, bar_bottom: f32, pill: bool, right_inset: i32) -> (i32, i32) {
    // below the classic bar; with the Caelestia pill bar below it when it runs along the top (`top`, `split`), else near the edge
    let below = if pill { 12.0 + bar_bottom / s } else { bar_bottom + 8.0 };
    let top = mon_y + ((below * s).round() as i32).min((mon_h - 1).max(0));
    (top, (MARGIN * s).round() as i32 + right_inset)
}

// ----------------------------------------------------------------- banners

pub struct BannerLayout {
    pub w: u32,
    pub h: u32,
    pub title: String,
    pub body: Vec<String>,
    pub btns: Vec<(R, String, String)>,
    pub close: R,
    pub icon_at: R,
    pub text_x: f32,
    pub title_y: f32,
    pub body_y: f32,
    pub body_line: f32,
}

/// Lays `n` out as a banner under `m`: the same numbers on every front end.
pub fn layout_banner(fonts: &Fonts, m: &Metrics, n: &Notification) -> BannerLayout {
    let s = m.s;
    let w = m.surface_width(BANNER_W);
    let pad = (14.0 * s).min(w * 0.08);
    let icon = (36.0 * s).min(w * 0.16);
    let tx = pad + icon + 10.0 * s;
    let tw = (w - tx - pad).max(1.0);
    let title_line = line_height(fonts, Role::Title, 13.0 * s, Weight::SemiBold, s);
    let body_line = line_height(fonts, Role::Ui, 12.0 * s, Weight::Regular, s);
    let close_size = (18.0 * s * crate::theme::live::font_scale().max(1.0)).min(tw * 0.35);
    let stamp_width = fonts.measure("now", 11.0 * s, Weight::Regular).max(close_size) + 8.0 * s;
    let title = ellipsize_role(fonts, Role::Title, &n.summary, 13.0 * s, Weight::SemiBold, (tw - stamp_width).max(1.0));
    let title_y = pad.min(12.0 * s);
    let body_y = title_y + title_line + 4.0 * s;
    let acts: Vec<_> = n.actions.iter().filter(|(k, l)| k != "default" && !l.is_empty()).take(3).collect();
    let button_h = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s) + 10.0 * s;
    let required_w: f32 = acts.iter().map(|(_, label)| fonts.measure(label, 12.0 * s, Weight::Medium) + 16.0 * s).sum::<f32>() + acts.len().saturating_sub(1) as f32 * 6.0 * s;
    let vertical_h = acts.len() as f32 * button_h + acts.len().saturating_sub(1) as f32 * 6.0 * s;
    let max_h = m.available_height();
    let stacked = required_w > tw && body_y + vertical_h + 8.0 * s + pad <= max_h;
    let actions_h = if acts.is_empty() {
        0.0
    } else if stacked {
        vertical_h + 8.0 * s
    } else {
        button_h + 8.0 * s
    };
    let max_lines = ((max_h - body_y - actions_h - pad) / body_line).floor().clamp(0.0, 3.0) as usize;
    let body = if max_lines == 0 { Vec::new() } else { wrap(fonts, &n.body, 12.0 * s, Weight::Regular, tw, max_lines) };
    let bottom = (body_y + body_line * body.len() as f32).max(pad + icon);
    let mut btns = Vec::new();
    let mut h = bottom + pad;
    if !acts.is_empty() {
        let by = bottom + 8.0 * s;
        let bw = if stacked { tw } else { ((tw - 6.0 * s * acts.len().saturating_sub(1) as f32) / acts.len() as f32).max(1.0) };
        for (i, (key, label)) in acts.iter().enumerate() {
            let r = R { x: tx + if stacked { 0.0 } else { i as f32 * (bw + 6.0 * s) }, y: by + if stacked { i as f32 * (button_h + 6.0 * s) } else { 0.0 }, w: bw, h: button_h };
            btns.push((r, ellipsize(fonts, label, 12.0 * s, Weight::Medium, (bw - 12.0 * s).max(1.0)), (*key).clone()));
        }
        h = by + if stacked { vertical_h } else { button_h } + pad;
    }
    BannerLayout {
        w: w.round().max(1.0) as u32,
        h: h.ceil().min(max_h).max(1.0) as u32,
        title,
        body,
        btns,
        close: R { x: (w - pad - close_size).max(0.0), y: title_y, w: close_size, h: close_size },
        icon_at: R { x: pad, y: pad, w: icon, h: icon },
        text_x: tx,
        title_y,
        body_y,
        body_line,
    }
}

pub struct Banner<W> {
    pub n: Notification,
    pub win: W,
    pub w: u32,
    pub h: u32,
    pub x: i32,
    pub y: i32,
    pub deadline: Option<Instant>,
    pub hovered: bool,
    pub hover_btn: Option<usize>,
    pub hover_close: bool,
    pub icon: Option<Pixmap>,
    pub title: String,
    pub body: Vec<String>,
    pub btns: Vec<(R, String, String)>,
    pub close: R,
    pub icon_at: R,
    pub text_x: f32,
    pub title_y: f32,
    pub body_y: f32,
    pub body_line: f32,
}

impl<W> Banner<W> {
    fn set_layout(&mut self, layout: BannerLayout) {
        self.w = layout.w;
        self.h = layout.h;
        self.title = layout.title;
        self.body = layout.body;
        self.btns = layout.btns;
        self.close = layout.close;
        self.icon_at = layout.icon_at;
        self.text_x = layout.text_x;
        self.title_y = layout.title_y;
        self.body_y = layout.body_y;
        self.body_line = layout.body_line;
        self.hover_btn = None;
        self.hover_close = false;
    }
}

/// A displaced visible notification retains its absolute deadline. A new queued
/// notification starts its lifetime only when it first becomes visible.
pub struct Pending {
    pub n: Notification,
    pub started: Option<Option<Instant>>,
}

/// A mouse button as the stack understands it (each front end maps its native codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Btn {
    Left,
    Middle,
    Right,
}

/// What the banner stack needs from the display server. Pixels are the front end's (see [`Metrics`]).
pub trait Host {
    type Win: Copy + PartialEq + Into<u64>;
    fn metrics(&self) -> Metrics;
    /// Lays `n` out under the current metrics (a front end may snap the size to what its surfaces can be).
    fn layout(&mut self, n: &Notification) -> BannerLayout;
    fn icon(&mut self, n: &Notification) -> Option<Pixmap>;
    /// A hidden surface for a banner laid out as `layout`, at (`x`, `y`); `None` when none can be made.
    fn create(&mut self, layout: &BannerLayout, x: i32, y: i32) -> Option<Self::Win>;
    /// Shows the surface (after its first paint).
    fn map(&mut self, win: Self::Win);
    fn destroy(&mut self, win: Self::Win);
    fn resize(&mut self, win: Self::Win, w: u32, h: u32);
    fn place(&mut self, win: Self::Win, x: i32, y: i32);
    /// Paints the banner into its surface (and shapes its input to the visible outline).
    fn paint(&mut self, b: &Banner<Self::Win>);
    fn flush(&mut self);
}

/// The visible banners (newest first, at most [`MAX_VISIBLE`]) and the queue behind them, with their deadlines.
/// Every rule the X11 daemon had lives here so each front end follows it: replacement keeps ids and retained
/// deadlines, a queued entry's lifetime starts when it is first displayed, hover pauses and leaving resumes with a
/// short grace period, a displaced hovered banner re-queues with that grace period.
pub struct Stack<W> {
    pub banners: Vec<Banner<W>>,
    pub pending: VecDeque<Pending>,
    pub default_secs: u32,
    /// Theme generation reported in the debug file.
    pub generation: u64,
}

impl<W: Copy + PartialEq + Into<u64>> Default for Stack<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Copy + PartialEq + Into<u64>> Stack<W> {
    pub fn new() -> Self {
        Stack { banners: Vec::new(), pending: VecDeque::new(), default_secs: 5, generation: 0 }
    }

    pub fn index_of(&self, win: W) -> Option<usize> {
        self.banners.iter().position(|b| b.win == win)
    }

    /// Shows `n` (or updates the banner / queued entry with the same id).
    pub fn show<H: Host<Win = W>>(&mut self, h: &mut H, n: Notification, secs: u32) {
        self.default_secs = secs;
        if let Some(i) = self.banners.iter().position(|b| b.n.id == n.id) {
            self.update_banner(h, i, n, secs);
            return;
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.n.id == n.id) {
            if p.started.is_some() {
                p.started = Some(n.deadline(secs));
            }
            p.n = n;
            self.debug(&h.metrics());
            return;
        }
        let layout = h.layout(&n);
        if self.banners.len() >= MAX_VISIBLE || !self.fits_banner(&*h, layout.h) {
            self.pending.push_back(Pending { n, started: None });
            self.debug(&h.metrics());
            return;
        }
        self.create_banner(h, n, secs);
    }

    fn fits_banner<H: Host<Win = W>>(&self, h: &H, height: u32) -> bool {
        let m = h.metrics();
        self.banners.iter().map(|b| b.h as f32 + m.px(GAP)).sum::<f32>() + height as f32 <= m.available_height()
    }

    fn apply_banner_layout<H: Host<Win = W>>(&mut self, h: &mut H, i: usize, layout: BannerLayout) {
        let b = &mut self.banners[i];
        b.set_layout(layout);
        h.resize(b.win, b.w, b.h);
    }

    fn create_banner<H: Host<Win = W>>(&mut self, h: &mut H, n: Notification, secs: u32) {
        let layout = h.layout(&n);
        let icon = h.icon(&n);
        let m = h.metrics();
        let x = m.x_for(layout.w);
        let Some(win) = h.create(&layout, x, m.top) else { return };
        let deadline = n.deadline(secs);
        let b = Banner {
            n,
            win,
            w: layout.w,
            h: layout.h,
            x,
            y: m.top,
            deadline,
            hovered: false,
            hover_btn: None,
            hover_close: false,
            icon,
            title: layout.title,
            body: layout.body,
            btns: layout.btns,
            close: layout.close,
            icon_at: layout.icon_at,
            text_x: layout.text_x,
            title_y: layout.title_y,
            body_y: layout.body_y,
            body_line: layout.body_line,
        };
        self.banners.insert(0, b);
        self.redraw(h, 0);
        h.map(win);
        self.restack(h);
    }

    fn update_banner<H: Host<Win = W>>(&mut self, h: &mut H, i: usize, n: Notification, secs: u32) {
        let layout = h.layout(&n);
        let icon = h.icon(&n);
        let b = &mut self.banners[i];
        b.deadline = n.deadline(secs);
        b.hovered = false;
        b.n = n;
        b.icon = icon;
        self.apply_banner_layout(h, i, layout);
        self.restack(h);
        for index in 0..self.banners.len() {
            self.redraw(h, index);
        }
    }

    /// Stacks the banners under the bar (newest on top), re-queueing the ones that no longer fit.
    pub fn restack<H: Host<Win = W>>(&mut self, h: &mut H) {
        let m = h.metrics();
        let mut y = m.top;
        let gap = m.gap();
        let bottom = m.top + m.available_height() as i32;
        let mut keep = 0;
        for b in &self.banners {
            if y + b.h as i32 > bottom {
                break;
            }
            keep += 1;
            y += b.h as i32 + gap;
        }
        for b in self.banners.drain(keep..).rev() {
            h.destroy(b.win);
            let deadline = if b.hovered && b.n.lifetime(self.default_secs).is_some() { Some(Instant::now() + Duration::from_secs(u64::from((self.default_secs / 2).max(2)))) } else { b.deadline };
            self.pending.push_front(Pending { n: b.n, started: Some(deadline) });
        }
        y = m.top;
        for b in &mut self.banners {
            b.y = y;
            b.x = m.x_for(b.w);
            h.place(b.win, b.x, y);
            y += b.h as i32 + gap;
        }
        h.flush();
        self.debug(&m);
    }

    pub fn redraw<H: Host<Win = W>>(&mut self, h: &mut H, i: usize) {
        h.paint(&self.banners[i]);
    }

    /// Lays every banner out again (theme, font, scale or monitor change) without touching ids, deadlines or hover.
    pub fn reflow<H: Host<Win = W>>(&mut self, h: &mut H) {
        for i in 0..self.banners.len() {
            let n = self.banners[i].n.clone();
            let layout = h.layout(&n);
            self.apply_banner_layout(h, i, layout);
        }
        self.restack(h);
        self.promote(h);
        for i in 0..self.banners.len() {
            self.redraw(h, i);
        }
        h.flush();
    }

    /// Looks the artwork of every visible banner up again (the icon theme or the scale changed).
    pub fn reicon<H: Host<Win = W>>(&mut self, h: &mut H) {
        for i in 0..self.banners.len() {
            let n = self.banners[i].n.clone();
            self.banners[i].icon = h.icon(&n);
        }
    }

    /// Removes the banner or queued entry with `id`; the event says it was closed with `reason`.
    pub fn close<H: Host<Win = W>>(&mut self, h: &mut H, id: u32, reason: u32) -> Vec<UiEvent> {
        if let Some(i) = self.banners.iter().position(|b| b.n.id == id) {
            let b = self.banners.remove(i);
            h.destroy(b.win);
            self.promote(h);
            self.restack(h);
            return vec![UiEvent::Closed(id, reason)];
        }
        if let Some(i) = self.pending.iter().position(|p| p.n.id == id) {
            self.pending.remove(i);
            self.debug(&h.metrics());
            return vec![UiEvent::Closed(id, reason)];
        }
        Vec::new()
    }

    fn promote<H: Host<Win = W>>(&mut self, h: &mut H) {
        while self.banners.len() < MAX_VISIBLE {
            let Some(next) = self.pending.front() else { break };
            let next_h = h.layout(&next.n).h;
            if !self.fits_banner(&*h, next_h) {
                break;
            }
            let pending = self.pending.pop_front().expect("front checked");
            let before = self.banners.len();
            self.create_banner(h, pending.n, self.default_secs);
            if self.banners.len() <= before {
                break;
            }
            let mut b = self.banners.remove(0);
            if let Some(deadline) = pending.started {
                b.deadline = deadline;
            }
            self.banners.push(b);
            self.restack(h);
        }
    }

    /// Drops every banner and queued entry (used when the history panel opens): each is dismissed (reason 2).
    pub fn close_all<H: Host<Win = W>>(&mut self, h: &mut H) -> Vec<UiEvent> {
        let ids: Vec<u32> = self.banners.iter().map(|b| b.n.id).chain(self.pending.iter().map(|p| p.n.id)).collect();
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.close(h, id, 2));
        }
        out
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.banners.iter().filter_map(|b| b.deadline).chain(self.pending.iter().filter_map(|p| p.started.flatten())).min()
    }

    pub fn expire<H: Host<Win = W>>(&mut self, h: &mut H, now: Instant) -> Vec<UiEvent> {
        let due: Vec<u32> = self
            .banners
            .iter()
            .filter(|b| b.deadline.is_some_and(|d| d <= now))
            .map(|b| b.n.id)
            .chain(self.pending.iter().filter(|p| p.started.flatten().is_some_and(|d| d <= now)).map(|p| p.n.id))
            .collect();
        due.into_iter().flat_map(|id| self.close(h, id, 1)).collect()
    }

    /// The pointer entered banner `i`: the countdown pauses.
    pub fn enter<H: Host<Win = W>>(&mut self, h: &mut H, i: usize) {
        self.banners[i].hovered = true;
        self.banners[i].deadline = None;
        self.redraw(h, i);
    }

    /// The pointer left banner `i`: the countdown resumes with a short grace period.
    pub fn leave<H: Host<Win = W>>(&mut self, h: &mut H, i: usize, secs: u32) {
        let b = &mut self.banners[i];
        b.hovered = false;
        b.hover_btn = None;
        b.hover_close = false;
        b.deadline = b.n.lifetime(secs).map(|_| Instant::now() + Duration::from_secs(u64::from((secs / 2).max(2))));
        self.redraw(h, i);
    }

    pub fn motion<H: Host<Win = W>>(&mut self, h: &mut H, i: usize, x: f32, y: f32) {
        let b = &mut self.banners[i];
        let btn = b.btns.iter().position(|(r, _, _)| r.contains(x, y));
        let close = b.close.contains(x, y);
        if btn != b.hover_btn || close != b.hover_close {
            b.hover_btn = btn;
            b.hover_close = close;
            self.redraw(h, i);
        }
    }

    /// A button press on banner `i`: left runs its action (`default` when no button is hit; only the close button
    /// dismisses plainly), middle dismisses everything, right dismisses it. A resident notification that ran an
    /// action stays.
    pub fn press<H: Host<Win = W>>(&mut self, h: &mut H, i: usize, button: Btn, x: f32, y: f32) -> Vec<UiEvent> {
        let b = &self.banners[i];
        let id = b.n.id;
        let mut events = Vec::new();
        match button {
            Btn::Left => {
                if b.close.contains(x, y) {
                    // plain dismissal
                } else if let Some((_, _, key)) = b.btns.iter().find(|(r, _, _)| r.contains(x, y)) {
                    events.push(UiEvent::Action(id, key.clone()));
                } else if b.n.actions.iter().any(|(k, _)| k == "default") {
                    events.push(UiEvent::Action(id, "default".into()));
                }
            }
            Btn::Middle => return self.close_all(h),
            Btn::Right => {}
        }
        let resident = self.banners[i].n.resident && !events.is_empty();
        if !resident {
            events.extend(self.close(h, id, 2));
        }
        events
    }

    /// Opt-in geometry/state evidence, without notification text or artwork.
    pub fn debug(&self, m: &Metrics) {
        let Some(path) = std::env::var_os("DECNOTIFY_DEBUG_BANNERS") else { return };
        let rect = |r: R| [r.x, r.y, r.w, r.h];
        let active: Vec<_> = self
            .banners
            .iter()
            .map(|b| {
                serde_json::json!({
                    "id": b.n.id, "window": Into::<u64>::into(b.win), "rect": [b.x, b.y, b.w as i32, b.h as i32],
                    "title_rect": [b.text_x, b.title_y, (b.close.x - b.text_x).max(0.0), b.body_y - b.title_y],
                    "body_rects": (0..b.body.len()).map(|i| [b.text_x, b.body_y + i as f32 * b.body_line, b.w as f32 - b.text_x, b.body_line]).collect::<Vec<_>>(),
                    "buttons": b.btns.iter().map(|(r,_,key)| serde_json::json!({"rect":rect(*r),"key":key})).collect::<Vec<_>>(),
                    "close_rect":rect(b.close), "deadline":b.deadline.map(|d|format!("{d:?}")), "hovered": b.hovered
                })
            })
            .collect();
        let queued: Vec<_> = self.pending.iter().map(|p| serde_json::json!({"id":p.n.id,"started":p.started.is_some(),"deadline":p.started.flatten().map(|d|format!("{d:?}"))})).collect();
        let data = serde_json::json!({"font_scale":crate::theme::live::font_scale(),"generation":self.generation,"scale":m.s,"active":active,"queued":queued});
        let _ = std::fs::write(path, data.to_string());
    }
}

// ------------------------------------------------------------------ shared drawing state

/// Fonts, palette, scale and the icon cache that every front end draws with.
pub struct Render {
    pub fonts: Fonts,
    pub pal: Palette,
    pub s: f32,
    pub icons: HashMap<String, Option<Pixmap>>,
}

impl Render {
    pub fn new(s: f32) -> Result<Render, String> {
        Ok(Render { fonts: Fonts::load().ok_or("no usable font found")?, pal: Palette::current(), s, icons: HashMap::new() })
    }

    pub fn icon_cached(&mut self, icon: &str, app: &str, entry: &str, size: u32) -> Option<Pixmap> {
        let key = format!("{icon}|{app}|{entry}|{size}");
        if let Some(hit) = self.icons.get(&key) {
            return hit.clone();
        }
        let found = lookup_icon(icon, app, entry, size);
        if self.icons.len() >= 64 {
            self.icons.clear();
        }
        self.icons.insert(key, found.clone());
        found
    }

    pub fn resolve_icon(&mut self, n: &Notification) -> Option<Pixmap> {
        let size = (36.0 * self.s).round() as u32;
        n.image.as_ref().and_then(|i| image_pixmap(i, size)).or_else(|| self.icon_cached(&n.icon, &n.app, &n.entry, size))
    }

    pub fn layout(&self, m: &Metrics, n: &Notification) -> BannerLayout {
        layout_banner(&self.fonts, m, n)
    }

    pub fn paint_banner<W>(&mut self, b: &Banner<W>) -> Pixmap {
        paint_banner(&mut self.fonts, &self.pal, self.s, b)
    }

    /// The theme (or icon pack) changed: takes the new palette and forgets cached artwork, successful and missing
    /// alike. Returns the new theme generation when anything has to be laid out and drawn again.
    pub fn retheme(&mut self, known_generation: u64) -> Option<u64> {
        let generation = crate::theme::live::generation();
        let pal = Palette::current();
        if pal == self.pal && generation == known_generation {
            return None;
        }
        self.pal = pal;
        self.icons.clear();
        Some(generation)
    }
}

/// Wakes `poll` when the theme watcher thread sees a change.
pub struct ThemeWake {
    reader: UnixStream,
}

impl ThemeWake {
    pub fn start() -> Result<ThemeWake, String> {
        let (reader, wake) = UnixStream::pair().map_err(|e| e.to_string())?;
        reader.set_nonblocking(true).map_err(|e| e.to_string())?;
        wake.set_nonblocking(true).map_err(|e| e.to_string())?;
        crate::theme::live::start_watcher(move || {
            let _ = (&wake).write(&[1]);
        });
        Ok(ThemeWake { reader })
    }

    pub fn fd(&self) -> RawFd {
        self.reader.as_raw_fd()
    }

    pub fn drain(&self) {
        let mut bytes = [0; 64];
        while (&self.reader).read(&mut bytes).is_ok_and(|n| n > 0) {}
    }
}

/// Rectangles `(x, y, w, h)` covering the pixels of `pm` whose alpha is above `cutoff`, merged vertically: the
/// outline a surface takes input on (rounded corners let clicks through to what is below).
pub fn opaque_rects(pm: &Pixmap, cutoff: u8) -> Vec<(i32, i32, u32, u32)> {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let data = pm.data();
    let mut out: Vec<(i32, i32, u32, u32)> = Vec::new();
    // indices (into `out`) of the rectangles that cover the previous row
    let mut above: Vec<usize> = Vec::new();
    for y in 0..h {
        let row = &data[y * w * 4..(y + 1) * w * 4];
        let mut here: Vec<usize> = Vec::new();
        let mut x = 0;
        while x < w {
            if row[x * 4 + 3] <= cutoff {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < w && row[x * 4 + 3] > cutoff {
                x += 1;
            }
            let (rx, rw) = (x0 as i32, (x - x0) as u32);
            let same = above.iter().copied().find(|&i| out[i].0 == rx && out[i].2 == rw);
            match same {
                Some(i) => {
                    out[i].3 += 1;
                    here.push(i);
                }
                None => {
                    out.push((rx, y as i32, rw, 1));
                    here.push(out.len() - 1);
                }
            }
        }
        above = here;
    }
    out
}

pub fn paint_banner<W>(fonts: &mut Fonts, pal: &Palette, s: f32, b: &Banner<W>) -> Pixmap {
    let mut pm = Pixmap::new(b.w, b.h).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"));
    let (w, h) = (b.w as f32, b.h as f32);
    gfx::fill_rounded(&mut pm, 0.0, 0.0, w, h, 16.0 * s, pal.bg_color());
    gfx::stroke_rounded(&mut pm, 0.5, 0.5, w - 1.0, h - 1.0, 16.0 * s, pal.border, 1.0);
    draw_icon(fonts, &mut pm, b.icon.as_ref(), &b.n.app, b.icon_at.x, b.icon_at.y, b.icon_at.w);
    let tx = b.text_x;
    fonts.draw_as(Role::Title, &mut pm, &b.title, tx, b.title_y, 13.0 * s, Weight::SemiBold, pal.title);
    for (i, line) in b.body.iter().enumerate() {
        fonts.draw(&mut pm, line, tx, b.body_y + i as f32 * b.body_line, 12.0 * s, Weight::Regular, pal.body);
    }
    if b.hovered {
        let c = b.close;
        let bg = if b.hover_close { pal.button_hover } else { pal.button };
        gfx::fill_circle(&mut pm, c.x + c.w / 2.0, c.y + c.h / 2.0, c.w / 2.0, bg);
        let t = fonts.measure("\u{d7}", 14.0 * s, Weight::Medium);
        fonts.draw(&mut pm, "\u{d7}", c.x + (c.w - t) / 2.0, c.y + (c.h - line_height(fonts, Role::Ui, 14.0 * s, Weight::Medium, s)) / 2.0, 14.0 * s, Weight::Medium, pal.title);
    } else {
        let t = fonts.measure("now", 11.0 * s, Weight::Regular);
        fonts.draw(&mut pm, "now", w - 14.0 * s - t, b.title_y, 11.0 * s, Weight::Regular, pal.dim);
    }
    for (i, (r, label, _)) in b.btns.iter().enumerate() {
        let bg = if b.hover_btn == Some(i) { pal.button_hover } else { pal.button };
        gfx::fill_rounded(&mut pm, r.x, r.y, r.w, r.h, 7.0 * s, bg);
        let t = fonts.measure(label, 12.0 * s, Weight::Medium);
        fonts.draw(&mut pm, label, r.x + (r.w - t) / 2.0, r.y + (r.h - line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s)) / 2.0, 12.0 * s, Weight::Medium, pal.title);
    }
    pm
}

pub fn accent_text(pal: &Palette) -> Color {
    let (r, g, b) = crate::theme::Rgb(pal.accent.0, pal.accent.1, pal.accent.2).readable_on().t();
    color(r, g, b, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_is_half_open() {
        let r = R { x: 1.0, y: 1.0, w: 2.0, h: 2.0 };
        assert!(r.contains(1.0, 1.0) && r.contains(2.9, 2.9) && !r.contains(3.0, 1.0));
    }

    #[test]
    fn tile_color_is_stable() {
        assert_eq!(tile_color("Mail"), tile_color("Mail"));
    }

    #[test]
    fn retry_text_is_readable_on_dark_and_light_accents() {
        let mut pal = Palette::dark();
        pal.accent = (0, 0, 0);
        assert_eq!(accent_text(&pal), Color::WHITE);
        pal.accent = (255, 255, 255);
        assert_eq!(accent_text(&pal), Color::BLACK);
    }

    #[test]
    fn wrapping_respects_width_and_line_limit() {
        let Some(f) = Fonts::load() else { return };
        let text = "The quick brown fox jumps over the lazy dog and keeps running far away from here";
        let lines = wrap(&f, text, 12.0, Weight::Regular, 120.0, 3);
        assert!(lines.len() <= 3 && lines.len() >= 2, "{lines:?}");
        assert!(lines.iter().all(|l| f.measure(l, 12.0, Weight::Regular) <= 121.0), "{lines:?}");
        assert!(lines.last().unwrap().ends_with('\u{2026}'), "cut text is ellipsized: {lines:?}");
        assert!(wrap(&f, "", 12.0, Weight::Regular, 120.0, 3).is_empty());
        let long = wrap(&f, &"x".repeat(200), 12.0, Weight::Regular, 100.0, 2);
        assert!(long.len() <= 2);
    }

    #[test]
    fn ellipsize_fits() {
        let Some(f) = Fonts::load() else { return };
        let e = ellipsize(&f, "A very long notification title that cannot fit", 13.0, Weight::SemiBold, 90.0);
        assert!(f.measure(&e, 13.0, Weight::SemiBold) <= 90.0 && e.ends_with('\u{2026}'));
        assert_eq!(ellipsize(&f, "Hi", 13.0, Weight::SemiBold, 90.0), "Hi");
    }

    #[test]
    fn resolved_theme_palette_preserves_popup_accent_and_readable_text() {
        use crate::theme::model::{contrast, Rgb, Selection};
        let rgb = |c: Color| Rgb((c.red() * 255.).round() as u8, (c.green() * 255.).round() as u8, (c.blue() * 255.).round() as u8);
        for (id, _) in crate::theme::builtin::BUILTIN {
            for dark in [true, false] {
                let t = crate::theme::builtin::def(id).unwrap().resolve(id, dark, &Selection::default());
                let p = Palette::from_theme(&t);
                assert_eq!(p.bg, t.popup.t());
                assert_eq!(p.accent, t.accent.t());
                for background in [t.popup, rgb(p.button), rgb(p.button_hover)] {
                    assert!(contrast(rgb(p.title), background) >= 4.49, "{id} dark={dark}");
                }
                for text in [p.body, p.dim, p.link] {
                    assert!(contrast(rgb(text), t.popup) >= 4.49, "{id} dark={dark}");
                    assert_eq!(text.alpha(), 1.0);
                }
            }
        }
    }

    fn pixmap_with(w: u32, h: u32, opaque: impl Fn(u32, u32) -> bool) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        for y in 0..h {
            for x in 0..w {
                if opaque(x, y) {
                    let i = ((y * w + x) * 4) as usize;
                    pm.data_mut()[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        pm
    }

    #[test]
    fn opaque_rects_follow_a_rounded_outline_and_merge_equal_rows() {
        // a 6x5 block with its four corner pixels cut off
        let pm = pixmap_with(6, 5, |x, y| !((x == 0 || x == 5) && (y == 0 || y == 4)));
        let rects = opaque_rects(&pm, 0);
        assert_eq!(rects, vec![(1, 0, 4, 1), (0, 1, 6, 3), (1, 4, 4, 1)]);
        let area: u32 = rects.iter().map(|r| r.2 * r.3).sum();
        assert_eq!(area, 26);
        assert!(opaque_rects(&Pixmap::new(4, 4).unwrap(), 0).is_empty(), "a fully transparent surface takes no input");
    }

    #[test]
    fn opaque_rects_keep_separate_runs_apart() {
        // two columns with a gap, one of them one row longer than the other
        let pm = pixmap_with(5, 4, |x, y| (x == 0 && y < 4) || (x == 4 && y < 3));
        let mut rects = opaque_rects(&pm, 0);
        rects.sort();
        assert_eq!(rects, vec![(0, 0, 1, 4), (4, 0, 1, 3)]);
    }

    #[test]
    fn origin_is_below_the_bar_and_inside_the_pill_inset() {
        // classic bar: bar_bottom + 8 logical pixels
        assert_eq!(stack_origin(1.0, 0, 1080, 26.0, false, 0), (34, 12));
        assert_eq!(stack_origin(2.0, 0, 1080, 26.0, false, 0), (68, 24));
        // pill bar: 12 below it
        assert_eq!(stack_origin(1.0, 0, 1080, 40.0, true, 0), (52, 12));
        // a right pill bar widens the gap; a monitor below the origin starts there
        assert_eq!(stack_origin(1.0, 100, 1080, 0.0, false, 56), (108, 68));
        // the compositor owns the zone: only the visual gaps remain
        assert_eq!(stack_origin(1.5, 0, 1080, 0.0, false, 0), (12, 18));
        // never below the last pixel of a tiny monitor
        assert_eq!(stack_origin(1.0, 0, 10, 100.0, false, 0), (9, 12));
    }

    fn fixture_metrics(s: f32) -> Metrics {
        let (top, right_gap) = stack_origin(s, 0, 1080, 0.0, false, 0);
        Metrics { s, x: 0, y: 0, w: 1920, h: 1080, top, right_gap }
    }

    #[test]
    fn metrics_keep_banners_inside_the_area() {
        let m = fixture_metrics(1.0);
        assert_eq!(m.margin(), 12);
        assert_eq!(m.gap(), 10);
        assert_eq!(m.available_height(), (1080 - 8 - 12) as f32);
        assert_eq!(m.x_for(344), 1920 - 344 - 12);
        let narrow = Metrics { w: 200, ..m };
        assert!(narrow.surface_width(BANNER_W) <= (200 - 12 - 12) as f32);
        assert_eq!(narrow.x_for(500), 0, "a banner wider than the area starts at its left edge");
    }

    fn note(id: u32, summary: &str) -> Notification {
        Notification {
            id,
            replaces: 0,
            app: "App".into(),
            icon: String::new(),
            summary: summary.into(),
            body: "Body".into(),
            actions: Vec::new(),
            urgency: 1,
            timeout_ms: -1,
            entry: String::new(),
            image: None,
            transient: false,
            resident: false,
        }
    }

    /// A host without a display: banners are 100 pixels high; it records what the stack asked for.
    struct Fake {
        m: Metrics,
        next: u64,
        log: Vec<String>,
        height: u32,
        fail_create: bool,
    }

    impl Fake {
        fn new() -> Fake {
            Fake { m: fixture_metrics(1.0), next: 1, log: Vec::new(), height: 100, fail_create: false }
        }
    }

    impl Host for Fake {
        type Win = u64;
        fn metrics(&self) -> Metrics {
            self.m
        }
        fn layout(&mut self, n: &Notification) -> BannerLayout {
            BannerLayout {
                w: 344,
                h: self.height,
                title: n.summary.clone(),
                body: Vec::new(),
                btns: vec![(R { x: 10.0, y: 60.0, w: 80.0, h: 24.0 }, "Reply".into(), "reply".into())],
                close: R { x: 300.0, y: 10.0, w: 18.0, h: 18.0 },
                icon_at: R::default(),
                text_x: 60.0,
                title_y: 10.0,
                body_y: 30.0,
                body_line: 16.0,
            }
        }
        fn icon(&mut self, _: &Notification) -> Option<Pixmap> {
            None
        }
        fn create(&mut self, _: &BannerLayout, x: i32, y: i32) -> Option<u64> {
            if self.fail_create {
                return None;
            }
            let id = self.next;
            self.next += 1;
            self.log.push(format!("create {id} {x},{y}"));
            Some(id)
        }
        fn map(&mut self, win: u64) {
            self.log.push(format!("map {win}"));
        }
        fn destroy(&mut self, win: u64) {
            self.log.push(format!("destroy {win}"));
        }
        fn resize(&mut self, win: u64, w: u32, h: u32) {
            self.log.push(format!("resize {win} {w}x{h}"));
        }
        fn place(&mut self, win: u64, x: i32, y: i32) {
            self.log.push(format!("place {win} {x},{y}"));
        }
        fn paint(&mut self, b: &Banner<u64>) {
            self.log.push(format!("paint {}", b.n.id));
        }
        fn flush(&mut self) {}
    }

    fn ids(s: &Stack<u64>) -> (Vec<u32>, Vec<u32>) {
        (s.banners.iter().map(|b| b.n.id).collect(), s.pending.iter().map(|p| p.n.id).collect())
    }

    #[test]
    fn at_most_five_banners_show_and_the_rest_queue_with_their_lifetime_unstarted() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        for id in 1..=7 {
            st.show(&mut h, note(id, "n"), 5);
        }
        assert_eq!(ids(&st), (vec![5, 4, 3, 2, 1], vec![6, 7]), "newest banner on top, queue in arrival order");
        assert!(st.pending.iter().all(|p| p.started.is_none()), "a queued lifetime starts when it is displayed");
        // banners are stacked under the origin with the gap between them
        let ys: Vec<i32> = st.banners.iter().map(|b| b.y).collect();
        assert_eq!(ys, vec![8, 118, 228, 338, 448]);
        // closing one promotes the head of the queue, which then gets a deadline of its own
        let ev = st.close(&mut h, 3, 3);
        assert_eq!(ev, vec![UiEvent::Closed(3, 3)]);
        assert_eq!(ids(&st).1, vec![7]);
        assert_eq!(st.banners.len(), 5);
        assert!(st.banners.iter().all(|b| b.deadline.is_some()));
    }

    #[test]
    fn replacing_an_active_or_queued_notification_keeps_its_slot_and_retained_deadline() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        for id in 1..=6 {
            st.show(&mut h, note(id, "old"), 5);
        }
        let before = ids(&st);
        st.show(&mut h, note(2, "new"), 5);
        st.show(&mut h, note(6, "queued new"), 5);
        assert_eq!(ids(&st), before);
        assert_eq!(st.banners.iter().find(|b| b.n.id == 2).unwrap().n.summary, "new");
        assert_eq!(st.pending[0].n.summary, "queued new");
        assert!(st.pending[0].started.is_none(), "replacing a queued entry does not start its clock");
        // a displaced banner retains its absolute deadline while it waits to come back
        let deadline = st.banners.iter().find(|b| b.n.id == 1).unwrap().deadline;
        assert!(deadline.is_some());
        h.height = 400;
        st.reflow(&mut h);
        assert_eq!(ids(&st), (vec![5, 4], vec![3, 2, 1, 6]), "what no longer fits goes back to the front of the queue, in order");
        let waiting = st.pending.iter().find(|p| p.n.id == 1).unwrap();
        assert_eq!(waiting.started, Some(deadline));
        assert!(st.pending.iter().find(|p| p.n.id == 6).unwrap().started.is_none());
    }

    #[test]
    fn hover_pauses_leave_resumes_with_grace_and_a_displaced_hovered_banner_requeues_with_grace() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        st.show(&mut h, note(1, "a"), 8);
        assert!(st.banners[0].deadline.is_some());
        st.enter(&mut h, 0);
        assert!(st.banners[0].hovered && st.banners[0].deadline.is_none(), "hover pauses the countdown");
        assert_eq!(st.next_deadline(), None);
        let before = Instant::now();
        st.leave(&mut h, 0, 8);
        let d = st.banners[0].deadline.expect("leaving restarts the countdown");
        assert!(d >= before + Duration::from_secs(4) && d <= Instant::now() + Duration::from_secs(4), "grace is half the banner time");
        // a banner that never expires stays that way
        let mut sticky = note(2, "sticky");
        sticky.timeout_ms = 0;
        st.show(&mut h, sticky, 8);
        let i = st.banners.iter().position(|b| b.n.id == 2).unwrap();
        st.enter(&mut h, i);
        st.leave(&mut h, i, 8);
        assert_eq!(st.banners[i].deadline, None);
        // grace is at least two seconds, however short the banner time
        let first = st.banners.iter().position(|b| b.n.id == 1).unwrap();
        st.leave(&mut h, first, 1);
        assert!(st.banners[first].deadline.unwrap() >= Instant::now() + Duration::from_secs(1));
    }

    #[test]
    fn expiry_reports_reason_one_for_visible_and_started_queue_entries_only() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        for id in 1..=6 {
            st.show(&mut h, note(id, "n"), 1);
        }
        assert!(st.expire(&mut h, Instant::now()).is_empty());
        let closed = st.expire(&mut h, Instant::now() + Duration::from_secs(2));
        // the five visible banners expire; the queued one was never displayed, so its lifetime has not started
        assert_eq!(closed.len(), 5);
        assert!(closed.iter().all(|e| matches!(e, UiEvent::Closed(_, 1))));
        assert_eq!(ids(&st), (vec![6], vec![]));
    }

    #[test]
    fn left_click_runs_an_action_then_dismisses_unless_resident() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        let mut n = note(1, "a");
        n.actions = vec![("default".into(), "Open".into())];
        st.show(&mut h, n.clone(), 5);
        // on a button: its action, then the dismissal (reason 2)
        let ev = st.press(&mut h, 0, Btn::Left, 20.0, 70.0);
        assert_eq!(ev, vec![UiEvent::Action(1, "reply".into()), UiEvent::Closed(1, 2)]);
        // on the body: the default action
        st.show(&mut h, n.clone(), 5);
        let ev = st.press(&mut h, 0, Btn::Left, 150.0, 40.0);
        assert_eq!(ev, vec![UiEvent::Action(1, "default".into()), UiEvent::Closed(1, 2)]);
        // on the close button: only the dismissal
        st.show(&mut h, n.clone(), 5);
        assert_eq!(st.press(&mut h, 0, Btn::Left, 305.0, 15.0), vec![UiEvent::Closed(1, 2)]);
        // resident: the action is reported and the banner stays
        let mut resident = n.clone();
        resident.resident = true;
        st.show(&mut h, resident, 5);
        let ev = st.press(&mut h, 0, Btn::Left, 20.0, 70.0);
        assert_eq!(ev, vec![UiEvent::Action(1, "reply".into())]);
        assert_eq!(st.banners.len(), 1);
        // right dismisses just this one, middle dismisses everything including the queue
        assert_eq!(st.press(&mut h, 0, Btn::Right, 150.0, 40.0), vec![UiEvent::Closed(1, 2)]);
        for id in 1..=7 {
            st.show(&mut h, note(id, "n"), 5);
        }
        let ev = st.press(&mut h, 0, Btn::Middle, 150.0, 40.0);
        assert_eq!(ev.len(), 7);
        assert!(ev.iter().all(|e| matches!(e, UiEvent::Closed(_, 2))));
        assert!(st.banners.is_empty() && st.pending.is_empty());
    }

    #[test]
    fn a_resident_banner_without_an_action_is_still_dismissed_by_a_click() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        let mut n = note(1, "a");
        n.resident = true;
        st.show(&mut h, n, 5);
        assert_eq!(st.press(&mut h, 0, Btn::Left, 150.0, 40.0), vec![UiEvent::Closed(1, 2)]);
    }

    #[test]
    fn a_banner_whose_surface_cannot_be_made_is_not_shown_and_not_lost_silently() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        h.fail_create = true;
        st.show(&mut h, note(1, "a"), 5);
        assert!(st.banners.is_empty());
    }

    #[test]
    fn reflow_keeps_ids_deadlines_and_hover_and_requeues_what_no_longer_fits() {
        let (mut h, mut st) = (Fake::new(), Stack::new());
        for id in 1..=3 {
            st.show(&mut h, note(id, "n"), 5);
        }
        let deadlines: Vec<_> = st.banners.iter().map(|b| b.deadline).collect();
        st.enter(&mut h, 1);
        st.reflow(&mut h);
        assert_eq!(ids(&st).0, vec![3, 2, 1]);
        assert!(st.banners[1].hovered, "a reflow does not drop the hover state");
        assert_eq!(st.banners[0].deadline, deadlines[0]);
        assert_eq!(st.banners[2].deadline, deadlines[2]);
        // a much smaller area keeps only what fits and queues the rest (oldest first back in line)
        h.m.h = 300;
        st.reflow(&mut h);
        let (shown, queued) = ids(&st);
        assert_eq!(shown.len() + queued.len(), 3, "nothing is lost");
        assert!(shown.len() < 3);
        let mut all = [shown.clone(), queued.clone()].concat();
        all.sort();
        assert_eq!(all, vec![1, 2, 3]);
        // growing it again brings them back, in the original order
        h.m.h = 1080;
        st.reflow(&mut h);
        assert_eq!(ids(&st), (vec![3, 2, 1], vec![]));
    }

    #[test]
    fn closing_an_unknown_id_reports_nothing() {
        let (mut h, mut st) = (Fake::new(), Stack::<u64>::new());
        assert!(st.close(&mut h, 9, 3).is_empty());
    }
}
