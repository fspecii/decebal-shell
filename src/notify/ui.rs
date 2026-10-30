//! X11 front end of decnotify: banner windows stacked below the menu bar, and the history panel.
//!
//! Every banner is its own override-redirect window (no window manager involvement, nothing for a tiling
//! WM to tile), clipped with the Shape extension to its rounded outline so it never paints black corners
//! without a compositor. All state is event driven; with no banner on screen the process sleeps in poll(2).

use super::model::{ago, unix_now, Config, HistoryItem, Image, Notification};
use super::Shared;
use crate::backend::x11::{panel_monitor, visible_rects, Rect as MonRect};
use crate::gfx::{self, color};
use crate::icons;
use crate::text::{Fonts, Role, Weight};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use tiny_skia::{Color, Pixmap};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Banners on screen at once; further ones wait in a queue.
const MAX_VISIBLE: usize = 5;
const BANNER_W: f32 = 344.0;
const MARGIN: f32 = 12.0;
const GAP: f32 = 10.0;
const HIST_W: f32 = 380.0;
const HIST_ROW: f32 = 60.0;
const HIST_MAX_ROWS: usize = 8;

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

    fn from_theme(theme: &crate::theme::Theme) -> Palette {
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

    fn bg_color(&self) -> Color {
        color(self.bg.0, self.bg.1, self.bg.2, 1.0)
    }

    fn bg_pixel(&self) -> u32 {
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
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

// ------------------------------------------------------------- text helpers

fn ellipsize(f: &Fonts, s: &str, px: f32, wt: Weight, max_w: f32) -> String {
    ellipsize_role(f, Role::Ui, s, px, wt, max_w)
}

fn ellipsize_role(f: &Fonts, role: Role, s: &str, px: f32, wt: Weight, max_w: f32) -> String {
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
fn wrap(f: &Fonts, text: &str, px: f32, wt: Weight, max_w: f32, max_lines: usize) -> Vec<String> {
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

fn image_pixmap(img: &Image, size: u32) -> Option<Pixmap> {
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

fn load_icon_file(path: &std::path::Path, size: u32) -> Option<Pixmap> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => icons::render_svg(path, size),
        Some("png") => icons::load_png_scaled(path, size),
        _ => None,
    }
}

/// Theme icon name, absolute path or `file://` URI; falls back to the desktop entry and app name.
fn lookup_icon(icon: &str, app: &str, entry: &str, size: u32) -> Option<Pixmap> {
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

fn tile_color(app: &str) -> (u8, u8, u8) {
    const TILES: [(u8, u8, u8); 6] = [(10, 132, 255), (48, 209, 88), (255, 159, 10), (191, 90, 242), (255, 69, 58), (100, 210, 255)];
    TILES[app.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % TILES.len()]
}

/// Draws the icon (or a lettered tile) into the `size` square at (x, y).
fn draw_icon(fonts: &mut Fonts, pm: &mut Pixmap, icon: Option<&Pixmap>, app: &str, x: f32, y: f32, size: f32) {
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

// ----------------------------------------------------------------- banners

struct BannerLayout {
    w: u32,
    h: u32,
    title: String,
    body: Vec<String>,
    btns: Vec<(R, String, String)>,
    close: R,
    icon_at: R,
    text_x: f32,
    title_y: f32,
    body_y: f32,
    body_line: f32,
}

struct Banner {
    n: Notification,
    win: Window,
    w: u32,
    h: u32,
    y: i32,
    deadline: Option<Instant>,
    hovered: bool,
    hover_btn: Option<usize>,
    hover_close: bool,
    icon: Option<Pixmap>,
    title: String,
    body: Vec<String>,
    btns: Vec<(R, String, String)>,
    close: R,
    icon_at: R,
    text_x: f32,
    title_y: f32,
    body_y: f32,
    body_line: f32,
}

/// A displaced visible notification retains its absolute deadline. A new queued
/// notification starts its lifetime only when it first becomes visible.
struct Pending {
    n: Notification,
    started: Option<Option<Instant>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistFocus {
    Clear,
    Dnd,
    Retry,
    Item(u32),
}

impl HistFocus {
    fn id(self) -> String {
        match self {
            Self::Clear => "clear".into(),
            Self::Dnd => "dnd".into(),
            Self::Retry => "retry".into(),
            Self::Item(id) => format!("item:{id}"),
        }
    }
}

fn line_height(fonts: &Fonts, role: Role, px: f32, weight: Weight, s: f32) -> f32 {
    (fonts.ascent_as(role, px, weight) + px * crate::theme::live::font_scale() * 0.3 + 2.0 * s).ceil()
}

#[derive(Debug, Clone, Copy)]
struct ErrorLayout {
    card: R,
    retry: R,
    title_y: f32,
    reason_y: f32,
    help_y: f32,
}

#[derive(Debug, Clone, Copy)]
struct HistoryLayout {
    clear: R,
    dnd: R,
    error: Option<ErrorLayout>,
    title_y: f32,
    header: f32,
    row: f32,
    title_line: f32,
    body_line: f32,
}

impl HistoryLayout {
    fn new(fonts: &Fonts, s: f32, w: f32, error: bool) -> Self {
        let clear_h = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s).max(14.0 * s) + 8.0 * s;
        let clear_w = (fonts.measure("Clear", 12.0 * s, Weight::Medium) + 16.0 * s).min((w - 28.0 * s).max(1.0));
        let title_h = line_height(fonts, Role::Title, 16.0 * s, Weight::Bold, s).max(clear_h);
        let clear = R { x: (w - clear_w - 14.0 * s).max(0.0), y: 11.0 * s, w: clear_w, h: clear_h };
        let dnd_h = (line_height(fonts, Role::Ui, 12.5 * s, Weight::Medium, s) + 10.0 * s).max(30.0 * s);
        let dnd = R { x: 12.0 * s, y: 13.0 * s + title_h + 9.0 * s, w: (w - 24.0 * s).max(1.0), h: dnd_h };
        let mut header = dnd.y + dnd.h + 8.0 * s;
        let error = error.then(|| {
            let title_lh = line_height(fonts, Role::Ui, 12.0 * s, Weight::SemiBold, s);
            let reason_lh = line_height(fonts, Role::Ui, 11.0 * s, Weight::Regular, s);
            let help_lh = line_height(fonts, Role::Ui, 10.0 * s, Weight::Regular, s);
            let retry_h = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s) + 10.0 * s;
            let retry_w = (fonts.measure("Retry", 12.0 * s, Weight::Medium) + 24.0 * s).min((w - 44.0 * s).max(1.0));
            let title_y = header + 6.0 * s;
            let reason_y = title_y + title_lh + 3.0 * s;
            let retry = R { x: (w - 22.0 * s - retry_w).max(0.0), y: reason_y, w: retry_w, h: retry_h };
            let help_y = reason_y + reason_lh.max(retry_h) + 3.0 * s;
            let card = R { x: 12.0 * s, y: header, w: (w - 24.0 * s).max(1.0), h: help_y + help_lh + 6.0 * s - header };
            header = card.y + card.h + 6.0 * s;
            ErrorLayout { card, retry, title_y, reason_y, help_y }
        });
        let title_line = line_height(fonts, Role::Ui, 12.5 * s, Weight::SemiBold, s);
        let body_line = line_height(fonts, Role::Ui, 12.0 * s, Weight::Regular, s);
        let app_line = line_height(fonts, Role::Ui, 10.5 * s, Weight::Regular, s);
        let row = (16.0 * s + title_line + body_line + app_line).max(HIST_ROW * s);
        Self { clear, dnd, error, title_y: 13.0 * s, header, row, title_line, body_line }
    }

    fn row_rect(self, i: usize, w: f32) -> R {
        R { x: 0.0, y: self.header + i as f32 * self.row, w, h: self.row }
    }
}

fn shifted(mut r: R, pan: f32) -> R {
    r.y -= pan;
    r
}

struct HistWin {
    win: Window,
    w: u32,
    h: u32,
    scroll: usize,
    rows: usize,
    x: i32,
    y: i32,
    hover: Option<usize>,
    focus: Option<HistFocus>,
    ring: bool,
    pan: f32,
    content_h: f32,
    layout: HistoryLayout,
}

struct Atoms {
    wtype: Atom,
    wtype_notification: Atom,
    wm_name: Atom,
    utf8: Atom,
}

pub struct Ui {
    conn: RustConnection,
    root: Window,
    gc: Gcontext,
    depth: u8,
    mon: MonRect,
    s: f32,
    fonts: Fonts,
    pal: Palette,
    at: Atoms,
    banners: Vec<Banner>,
    pending: VecDeque<Pending>,
    hist: Option<HistWin>,
    icon_cache: HashMap<String, Option<Pixmap>>,
    default_secs: u32,
    theme_wake: UnixStream,
    theme_generation: u64,
}

fn cerr<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn xft_scale(conn: &RustConnection, root: Window) -> f32 {
    let Ok(Ok(r)) = conn.get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 65536).map(|c| c.reply()) else { return 1.0 };
    let text = String::from_utf8_lossy(&r.value);
    text.lines().find_map(|l| l.strip_prefix("Xft.dpi:")).and_then(|v| v.trim().parse::<f32>().ok()).map_or(1.0, |d| (d / 96.0).clamp(1.0, 4.0))
}

/// Uploads a premultiplied pixmap as BGRX, in bands small enough for the server's request limit.
fn present(conn: &RustConnection, win: Window, gc: Gcontext, pm: &Pixmap, depth: u8) {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let max = conn.maximum_request_bytes().saturating_sub(64);
    let rows_per = (max / (w * 4)).clamp(1, h.min(128));
    let mut buf = vec![0u8; w * rows_per * 4];
    let mut y = 0;
    while y < h {
        let rows = rows_per.min(h - y);
        for (d, s) in buf[..rows * w * 4].chunks_exact_mut(4).zip(pm.data()[y * w * 4..(y + rows) * w * 4].chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], 0xff]);
        }
        let _ = conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w as u16, rows as u16, 0, y as i16, 0, depth, &buf[..rows * w * 4]);
        y += rows;
    }
}

impl Ui {
    pub fn connect() -> Result<Ui, String> {
        let (conn, num) = x11rb::connect(None).map_err(cerr)?;
        let screen = conn.setup().roots[num].clone();
        let root = screen.root;
        let gc = conn.generate_id().map_err(cerr)?;
        conn.create_gc(gc, root, &CreateGCAux::new()).map_err(cerr)?;
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE)).map_err(cerr)?;
        let atom = |name: &str| -> Result<Atom, String> { Ok(conn.intern_atom(false, name.as_bytes()).map_err(cerr)?.reply().map_err(cerr)?.atom) };
        let at = Atoms { wtype: atom("_NET_WM_WINDOW_TYPE")?, wtype_notification: atom("_NET_WM_WINDOW_TYPE_NOTIFICATION")?, wm_name: atom("_NET_WM_NAME")?, utf8: atom("UTF8_STRING")? };
        let mon = panel_monitor(&conn, root, (i32::from(screen.width_in_pixels), i32::from(screen.height_in_pixels)));
        let s = xft_scale(&conn, root);
        let fonts = Fonts::load().ok_or("no usable font found")?;
        let (theme_wake, wake) = UnixStream::pair().map_err(cerr)?;
        theme_wake.set_nonblocking(true).map_err(cerr)?;
        wake.set_nonblocking(true).map_err(cerr)?;
        crate::theme::live::start_watcher(move || {
            let _ = (&wake).write(&[1]);
        });
        Ok(Ui {
            conn,
            root,
            gc,
            depth: screen.root_depth,
            mon,
            s,
            fonts,
            pal: Palette::current(),
            at,
            banners: Vec::new(),
            pending: VecDeque::new(),
            hist: None,
            icon_cache: HashMap::new(),
            default_secs: 5,
            theme_wake,
            theme_generation: crate::theme::live::generation(),
        })
    }

    pub fn fd(&self) -> RawFd {
        self.conn.stream().as_raw_fd()
    }

    pub fn theme_fd(&self) -> RawFd {
        self.theme_wake.as_raw_fd()
    }

    pub fn refresh_theme(&mut self, sh: &Shared) {
        let mut bytes = [0; 64];
        while self.theme_wake.read(&mut bytes).is_ok_and(|n| n > 0) {}
        let generation = crate::theme::live::generation();
        let pal = Palette::current();
        if pal == self.pal && generation == self.theme_generation {
            return;
        }
        self.pal = pal;
        self.theme_generation = generation;
        self.reflow(sh);
    }

    fn reflow(&mut self, sh: &Shared) {
        for i in 0..self.banners.len() {
            let n = self.banners[i].n.clone();
            let layout = self.layout(&n);
            self.apply_banner_layout(i, layout);
        }
        self.restack();
        self.promote();
        for i in 0..self.banners.len() {
            self.redraw(i);
        }
        self.refresh_history(sh);
        let _ = self.conn.flush();
    }

    fn monitor_reflow(&mut self, sh: &Shared) {
        let fallback = self.conn.get_geometry(self.root).ok().and_then(|cookie| cookie.reply().ok()).map(|r| (i32::from(r.width), i32::from(r.height))).unwrap_or((self.mon.w, self.mon.h));
        self.mon = panel_monitor(&self.conn, self.root, fallback);
        self.reflow(sh);
    }

    fn available_height(&self) -> f32 {
        (self.mon.y + self.mon.h - self.top() - self.px(MARGIN).round() as i32).max(1) as f32
    }

    fn surface_width(&self, base: f32) -> f32 {
        let growth = (crate::theme::live::font_scale() - 1.0).max(0.0);
        (base * self.s * (1.0 + growth * 0.5)).min((self.mon.w - self.right_gap() - self.px(MARGIN).round() as i32).max(1) as f32).max(1.0)
    }

    fn x_for(&self, w: u32) -> i32 {
        (self.mon.x + self.mon.w - w as i32 - self.right_gap()).max(self.mon.x)
    }

    fn px(&self, v: f32) -> f32 {
        v * self.s
    }

    fn top(&self) -> i32 {
        // below the classic bar; with the Caelestia pill bar below it when it runs along the top (`top`, `split`), else near the edge
        let th = crate::theme::th();
        let below = if th.pill_bar().is_some() { 12.0 + th.bar_insets().top as f32 / self.s } else { crate::theme::BAR_HEIGHT + 8.0 };
        self.mon.y + (self.px(below).round() as i32).min((self.mon.h - 1).max(0))
    }

    /// Gap between the banners and the right screen edge: the margin, plus the pill bar when it sits on the right edge.
    fn right_gap(&self) -> i32 {
        self.px(MARGIN).round() as i32 + crate::theme::th().bar_insets().right
    }

    fn make_window(&self, x: i32, y: i32, w: u32, h: u32, keys: bool) -> Result<Window, String> {
        let win = self.conn.generate_id().map_err(cerr)?;
        let mut mask = EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::ENTER_WINDOW | EventMask::LEAVE_WINDOW | EventMask::POINTER_MOTION;
        if keys {
            mask |= EventMask::KEY_PRESS;
        }
        let aux = CreateWindowAux::new().override_redirect(1).background_pixel(self.pal.bg_pixel()).event_mask(mask);
        self.conn.create_window(0, win, self.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(cerr)?;
        let _ = self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"decnotify\0Decnotify\0");
        let _ = self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"Notification");
        let _ = self.conn.change_property8(PropMode::REPLACE, win, self.at.wm_name, self.at.utf8, b"Notification");
        let _ = self.conn.change_property32(PropMode::REPLACE, win, self.at.wtype, AtomEnum::ATOM, &[self.at.wtype_notification]);
        Ok(win)
    }

    fn set_shape(&self, win: Window, pm: &Pixmap) {
        let rects = visible_rects(pm);
        let _ = self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, win, 0, 0, &rects);
    }

    fn icon_cached(&mut self, icon: &str, app: &str, entry: &str, size: u32) -> Option<Pixmap> {
        let key = format!("{icon}|{app}|{entry}|{size}");
        if let Some(hit) = self.icon_cache.get(&key) {
            return hit.clone();
        }
        let found = lookup_icon(icon, app, entry, size);
        if self.icon_cache.len() >= 64 {
            self.icon_cache.clear();
        }
        self.icon_cache.insert(key, found.clone());
        found
    }

    // ---- banner lifecycle

    /// Shows `n` (or updates the banner / queued entry with the same id).
    pub fn show(&mut self, n: Notification, secs: u32) {
        self.default_secs = secs;
        if let Some(i) = self.banners.iter().position(|b| b.n.id == n.id) {
            self.update_banner(i, n, secs);
            return;
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.n.id == n.id) {
            if p.started.is_some() {
                p.started = Some(n.deadline(secs));
            }
            p.n = n;
            self.banner_debug();
            return;
        }
        let layout = self.layout(&n);
        if self.banners.len() >= MAX_VISIBLE || !self.fits_banner(layout.h) {
            self.pending.push_back(Pending { n, started: None });
            self.banner_debug();
            return;
        }
        self.create_banner(n, secs);
    }

    fn layout(&self, n: &Notification) -> BannerLayout {
        let s = self.s;
        let w = self.surface_width(BANNER_W);
        let pad = (14.0 * s).min(w * 0.08);
        let icon = (36.0 * s).min(w * 0.16);
        let tx = pad + icon + 10.0 * s;
        let tw = (w - tx - pad).max(1.0);
        let title_line = line_height(&self.fonts, Role::Title, 13.0 * s, Weight::SemiBold, s);
        let body_line = line_height(&self.fonts, Role::Ui, 12.0 * s, Weight::Regular, s);
        let close_size = (18.0 * s * crate::theme::live::font_scale().max(1.0)).min(tw * 0.35);
        let stamp_width = self.fonts.measure("now", 11.0 * s, Weight::Regular).max(close_size) + 8.0 * s;
        let title = ellipsize_role(&self.fonts, Role::Title, &n.summary, 13.0 * s, Weight::SemiBold, (tw - stamp_width).max(1.0));
        let title_y = pad.min(12.0 * s);
        let body_y = title_y + title_line + 4.0 * s;
        let acts: Vec<_> = n.actions.iter().filter(|(k, l)| k != "default" && !l.is_empty()).take(3).collect();
        let button_h = line_height(&self.fonts, Role::Ui, 12.0 * s, Weight::Medium, s) + 10.0 * s;
        let required_w: f32 = acts.iter().map(|(_, label)| self.fonts.measure(label, 12.0 * s, Weight::Medium) + 16.0 * s).sum::<f32>() + acts.len().saturating_sub(1) as f32 * 6.0 * s;
        let vertical_h = acts.len() as f32 * button_h + acts.len().saturating_sub(1) as f32 * 6.0 * s;
        let max_h = self.available_height();
        let stacked = required_w > tw && body_y + vertical_h + 8.0 * s + pad <= max_h;
        let actions_h = if acts.is_empty() {
            0.0
        } else if stacked {
            vertical_h + 8.0 * s
        } else {
            button_h + 8.0 * s
        };
        let max_lines = ((max_h - body_y - actions_h - pad) / body_line).floor().clamp(0.0, 3.0) as usize;
        let body = if max_lines == 0 { Vec::new() } else { wrap(&self.fonts, &n.body, 12.0 * s, Weight::Regular, tw, max_lines) };
        let bottom = (body_y + body_line * body.len() as f32).max(pad + icon);
        let mut btns = Vec::new();
        let mut h = bottom + pad;
        if !acts.is_empty() {
            let by = bottom + 8.0 * s;
            let bw = if stacked { tw } else { ((tw - 6.0 * s * acts.len().saturating_sub(1) as f32) / acts.len() as f32).max(1.0) };
            for (i, (key, label)) in acts.iter().enumerate() {
                let r = R { x: tx + if stacked { 0.0 } else { i as f32 * (bw + 6.0 * s) }, y: by + if stacked { i as f32 * (button_h + 6.0 * s) } else { 0.0 }, w: bw, h: button_h };
                btns.push((r, ellipsize(&self.fonts, label, 12.0 * s, Weight::Medium, (bw - 12.0 * s).max(1.0)), (*key).clone()));
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

    fn fits_banner(&self, height: u32) -> bool {
        self.banners.iter().map(|b| b.h as f32 + self.px(GAP)).sum::<f32>() + height as f32 <= self.available_height()
    }

    fn apply_banner_layout(&mut self, i: usize, layout: BannerLayout) {
        let b = &mut self.banners[i];
        b.w = layout.w;
        b.h = layout.h;
        b.title = layout.title;
        b.body = layout.body;
        b.btns = layout.btns;
        b.close = layout.close;
        b.icon_at = layout.icon_at;
        b.text_x = layout.text_x;
        b.title_y = layout.title_y;
        b.body_y = layout.body_y;
        b.body_line = layout.body_line;
        b.hover_btn = None;
        b.hover_close = false;
        let _ = self.conn.configure_window(b.win, &ConfigureWindowAux::new().width(b.w).height(b.h));
    }

    fn resolve_icon(&mut self, n: &Notification) -> Option<Pixmap> {
        let size = (36.0 * self.s).round() as u32;
        n.image.as_ref().and_then(|i| image_pixmap(i, size)).or_else(|| self.icon_cached(&n.icon, &n.app, &n.entry, size))
    }

    fn create_banner(&mut self, n: Notification, secs: u32) {
        let layout = self.layout(&n);
        let icon = self.resolve_icon(&n);
        let Ok(win) = self.make_window(self.x_for(layout.w), self.top(), layout.w, layout.h, false) else { return };
        let deadline = n.deadline(secs);
        let b = Banner {
            n,
            win,
            w: layout.w,
            h: layout.h,
            y: self.top(),
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
        self.redraw(0);
        let _ = self.conn.map_window(win);
        self.fonts.trim();
        self.restack();
    }

    fn update_banner(&mut self, i: usize, n: Notification, secs: u32) {
        let layout = self.layout(&n);
        let icon = self.resolve_icon(&n);
        let b = &mut self.banners[i];
        b.deadline = n.deadline(secs);
        b.hovered = false;
        b.n = n;
        b.icon = icon;
        self.apply_banner_layout(i, layout);
        self.restack();
        for index in 0..self.banners.len() {
            self.redraw(index);
        }
    }

    fn restack(&mut self) {
        let mut y = self.top();
        let gap = self.px(GAP).round() as i32;
        let bottom = self.top() + self.available_height() as i32;
        let mut keep = 0;
        for b in &self.banners {
            if y + b.h as i32 > bottom {
                break;
            }
            keep += 1;
            y += b.h as i32 + gap;
        }
        for b in self.banners.drain(keep..).rev() {
            let _ = self.conn.destroy_window(b.win);
            let deadline = if b.hovered && b.n.lifetime(self.default_secs).is_some() { Some(Instant::now() + Duration::from_secs(u64::from((self.default_secs / 2).max(2)))) } else { b.deadline };
            self.pending.push_front(Pending { n: b.n, started: Some(deadline) });
        }
        let x = self.mon.x;
        let right = self.mon.x + self.mon.w - self.right_gap();
        y = self.top();
        for b in &mut self.banners {
            b.y = y;
            let _ = self.conn.configure_window(b.win, &ConfigureWindowAux::new().x((right - b.w as i32).max(x)).y(y));
            y += b.h as i32 + gap;
        }
        let _ = self.conn.flush();
        self.banner_debug();
    }

    /// Opt-in geometry/state evidence, without notification text or artwork.
    fn banner_debug(&self) {
        let Some(path) = std::env::var_os("DECNOTIFY_DEBUG_BANNERS") else { return };
        let rect = |r: R| [r.x, r.y, r.w, r.h];
        let active: Vec<_> = self
            .banners
            .iter()
            .map(|b| {
                serde_json::json!({
                    "id": b.n.id, "window": b.win, "rect": [self.x_for(b.w), b.y, b.w as i32, b.h as i32],
                    "title_rect": [b.text_x, b.title_y, (b.close.x - b.text_x).max(0.0), b.body_y - b.title_y],
                    "body_rects": (0..b.body.len()).map(|i| [b.text_x, b.body_y + i as f32 * b.body_line, b.w as f32 - b.text_x, b.body_line]).collect::<Vec<_>>(),
                    "buttons": b.btns.iter().map(|(r,_,key)| serde_json::json!({"rect":rect(*r),"key":key})).collect::<Vec<_>>(),
                    "close_rect":rect(b.close), "deadline":b.deadline.map(|d|format!("{d:?}"))
                })
            })
            .collect();
        let queued: Vec<_> = self.pending.iter().map(|p| serde_json::json!({"id":p.n.id,"started":p.started.is_some(),"deadline":p.started.flatten().map(|d|format!("{d:?}"))})).collect();
        let data = serde_json::json!({"font_scale":crate::theme::live::font_scale(),"generation":self.theme_generation,"active":active,"queued":queued});
        let _ = std::fs::write(path, data.to_string());
    }

    fn redraw(&mut self, i: usize) {
        let pm = paint_banner(&mut self.fonts, &self.pal, self.s, &self.banners[i]);
        let win = self.banners[i].win;
        self.set_shape(win, &pm);
        present(&self.conn, win, self.gc, &pm, self.depth);
        let _ = self.conn.flush();
        self.fonts.trim();
    }

    /// Removes the banner or queued entry with `id`; returns whether it existed.
    pub fn close(&mut self, id: u32, reason: u32) -> Vec<UiEvent> {
        if let Some(i) = self.banners.iter().position(|b| b.n.id == id) {
            let b = self.banners.remove(i);
            let _ = self.conn.destroy_window(b.win);
            self.promote();
            self.restack();
            return vec![UiEvent::Closed(id, reason)];
        }
        if let Some(i) = self.pending.iter().position(|p| p.n.id == id) {
            self.pending.remove(i);
            self.banner_debug();
            return vec![UiEvent::Closed(id, reason)];
        }
        Vec::new()
    }

    fn promote(&mut self) {
        while self.banners.len() < MAX_VISIBLE {
            let Some(next) = self.pending.front() else { break };
            if !self.fits_banner(self.layout(&next.n).h) {
                break;
            }
            let pending = self.pending.pop_front().expect("front checked");
            let before = self.banners.len();
            self.create_banner(pending.n, self.default_secs);
            if self.banners.len() <= before {
                break;
            }
            let mut b = self.banners.remove(0);
            if let Some(deadline) = pending.started {
                b.deadline = deadline;
            }
            self.banners.push(b);
            self.restack();
        }
    }

    /// Drops every banner and queued entry (used when the history panel opens).
    pub fn close_all(&mut self) -> Vec<UiEvent> {
        let ids: Vec<u32> = self.banners.iter().map(|b| b.n.id).chain(self.pending.iter().map(|p| p.n.id)).collect();
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.close(id, 2));
        }
        out
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.banners.iter().filter_map(|b| b.deadline).chain(self.pending.iter().filter_map(|p| p.started.flatten())).min()
    }

    pub fn expire(&mut self, now: Instant) -> Vec<UiEvent> {
        let due: Vec<u32> = self
            .banners
            .iter()
            .filter(|b| b.deadline.is_some_and(|d| d <= now))
            .map(|b| b.n.id)
            .chain(self.pending.iter().filter(|p| p.started.flatten().is_some_and(|d| d <= now)).map(|p| p.n.id))
            .collect();
        due.into_iter().flat_map(|id| self.close(id, 1)).collect()
    }

    // ---- history panel

    pub fn history_open(&self) -> bool {
        self.hist.is_some()
    }

    pub fn toggle_history(&mut self, sh: &Shared) -> Vec<UiEvent> {
        if self.hist.is_some() {
            self.close_history();
            return Vec::new();
        }
        let events = self.close_all();
        let (w, h, rows, layout, content_h) = self.history_dimensions(sh);
        let x = self.x_for(w);
        let Ok(win) = self.make_window(x, self.top(), w, h, true) else { return events };
        self.hist = Some(HistWin { win, w, h, scroll: 0, rows, x, y: self.top(), hover: None, focus: None, ring: false, pan: 0.0, content_h, layout });
        self.redraw_history(sh);
        let _ = self.conn.map_window(win);
        let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, win, x11rb::CURRENT_TIME);
        let _ = self.conn.grab_keyboard(true, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC);
        let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        let _ = self.conn.grab_pointer(true, win, mask, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
        self.fonts.trim();
        self.history_debug(sh);
        events
    }

    fn close_history(&mut self) {
        if let Some(h) = self.hist.take() {
            let _ = self.conn.ungrab_pointer(x11rb::CURRENT_TIME);
            let _ = self.conn.ungrab_keyboard(x11rb::CURRENT_TIME);
            let _ = self.conn.destroy_window(h.win);
            let _ = self.conn.flush();
        }
    }

    pub fn refresh_history(&mut self, sh: &Shared) {
        if self.hist.is_some() {
            self.redraw_history(sh);
        }
    }

    fn history_dimensions(&self, sh: &Shared) -> (u32, u32, usize, HistoryLayout, f32) {
        let w = self.surface_width(HIST_W).round().max(1.0) as u32;
        let layout = HistoryLayout::new(&self.fonts, self.s, w as f32, sh.history.error().is_some());
        let available = self.available_height();
        let rows = ((available - layout.header - 8.0 * self.s) / layout.row).floor().clamp(1.0, HIST_MAX_ROWS as f32) as usize;
        let count = sh.history.items.len().clamp(1, rows);
        let content = layout.header + count as f32 * layout.row + 8.0 * self.s;
        (w, content.ceil().min(available).max(1.0) as u32, rows, layout, content)
    }

    fn redraw_history(&mut self, sh: &Shared) {
        let (w, h, capacity, layout, content_h) = self.history_dimensions(sh);
        let x = self.x_for(w);
        let y = self.top();
        if let Some(hw) = &mut self.hist {
            hw.rows = capacity;
            hw.scroll = hw.scroll.min(sh.history.items.len().saturating_sub(capacity));
            hw.layout = layout;
            hw.content_h = content_h;
            hw.pan = hw.pan.clamp(0.0, (content_h - h as f32).max(0.0));
            if hw.focus.is_some_and(|focus| !history_order(sh).contains(&focus)) {
                hw.focus = Some(HistFocus::Dnd);
            }
            if (hw.w, hw.h, hw.x, hw.y) != (w, h, x, y) {
                (hw.w, hw.h, hw.x, hw.y) = (w, h, x, y);
                let _ = self.conn.configure_window(hw.win, &ConfigureWindowAux::new().x(x).y(y).width(w).height(h));
            }
            if hw.ring {
                reveal_history_focus(hw, sh, self.s);
            }
        }
        let scroll = self.hist.as_ref().map_or(0, |h| h.scroll);
        let size = (32.0 * self.s).round() as u32;
        let items: Vec<HistoryItem> = sh.history.items.iter().skip(scroll).take(capacity).cloned().collect();
        for it in &items {
            self.icon_cached(&it.icon, &it.app, &it.entry, size);
        }
        let Some(hw) = &self.hist else { return };
        let pm = paint_history(&mut self.fonts, &self.pal, self.s, sh, Some(hw), &self.icon_cache);
        self.set_shape(hw.win, &pm);
        present(&self.conn, hw.win, self.gc, &pm, self.depth);
        let _ = self.conn.flush();
        self.fonts.trim();
        self.history_debug(sh);
    }

    fn activate_history(&mut self, target: HistFocus, sh: &mut Shared) {
        match target {
            HistFocus::Retry => {
                let _ = sh.history.retry();
            }
            HistFocus::Clear => {
                let _ = sh.history.clear();
                if let Some(h) = &mut self.hist {
                    h.scroll = 0;
                }
            }
            HistFocus::Dnd => {
                let on = !sh.cfg.dnd;
                if Config::save_dnd(on).is_ok() {
                    sh.cfg.dnd = on;
                }
            }
            HistFocus::Item(id) => {
                let index = sh.history.items.iter().position(|item| item.id == id);
                sh.history.remove(id);
                if let Some(hw) = &mut self.hist {
                    hw.scroll = hw.scroll.min(sh.history.items.len().saturating_sub(hw.rows));
                    hw.focus = index.and_then(|i| sh.history.items.get(i.min(sh.history.items.len().saturating_sub(1)))).map(|item| HistFocus::Item(item.id)).or(Some(HistFocus::Dnd));
                }
            }
        }
        self.redraw_history(sh);
    }

    fn history_key(&mut self, e: KeyPressEvent, sh: &mut Shared) {
        let sym =
            self.conn.get_keyboard_mapping(e.detail, 1).ok().and_then(|cookie| cookie.reply().ok()).and_then(|reply| reply.keysyms.first().copied()).unwrap_or(if e.detail == 9 { 0xff1b } else { 0 });
        if sym == 0xff1b {
            self.close_history();
            return;
        }
        let Some(hw) = &mut self.hist else { return };
        match sym {
            0xff09 | 0xfe20 => {
                let order = history_order(sh);
                let backwards = e.state.contains(KeyButMask::SHIFT) || sym == 0xfe20;
                let index = hw.focus.and_then(|focus| order.iter().position(|target| *target == focus));
                let next = match (index, backwards) {
                    (Some(0) | None, true) => order.len() - 1,
                    (Some(i), true) => i - 1,
                    (Some(i), false) => (i + 1) % order.len(),
                    (None, false) => 0,
                };
                hw.focus = Some(order[next]);
                hw.ring = true;
                if let HistFocus::Item(id) = order[next] {
                    if let Some(index) = sh.history.items.iter().position(|item| item.id == id) {
                        if index < hw.scroll {
                            hw.scroll = index;
                        }
                        if index >= hw.scroll + hw.rows {
                            hw.scroll = index + 1 - hw.rows;
                        }
                    }
                }
                reveal_history_focus(hw, sh, self.s);
                self.redraw_history(sh);
            }
            0xff0d | 0xff8d | 0x20 => {
                if let Some(target) = hw.focus {
                    self.activate_history(target, sh);
                }
            }
            0xffff => {
                if let Some(target @ HistFocus::Item(_)) = hw.focus {
                    self.activate_history(target, sh);
                }
            }
            _ => {}
        }
    }

    fn history_debug(&self, sh: &Shared) {
        let Some(path) = std::env::var_os("DECNOTIFY_DEBUG_HISTORY") else { return };
        let Some(hw) = &self.hist else { return };
        let (clear, dnd, rows) = hist_rects(hw, sh);
        let rect = |r: R| [r.x, r.y, r.w, r.h];
        let items: Vec<_> = sh.history.items.iter().map(|i| serde_json::json!({"id":i.id,"summary":i.summary})).collect();
        let data = serde_json::json!({"window":hw.win,"origin":[hw.x,hw.y],"error":sh.history.error(),"pending":sh.history.pending(),"clear_pending":sh.history.clear_pending(),"items":items,"retry_rect":hw.layout.error.map(|e|rect(shifted(e.retry,hw.pan))),"clear_rect":rect(clear),"dnd_rect":rect(dnd),"row_rects":rows.into_iter().map(rect).collect::<Vec<_>>(),"size":[hw.w,hw.h],"scroll":hw.scroll,"pan":hw.pan,"focus":hw.focus.map(HistFocus::id),"font_scale":crate::theme::live::font_scale()});
        let _ = std::fs::write(path, data.to_string());
    }

    // ---- events

    /// Drains pending X events; returns what to tell the bus.
    pub fn pump(&mut self, sh: &mut Shared) -> Result<Vec<UiEvent>, String> {
        let mut out = Vec::new();
        while let Some(ev) = self.conn.poll_for_event().map_err(cerr)? {
            self.handle(ev, sh, &mut out);
        }
        let _ = self.conn.flush();
        Ok(out)
    }

    fn banner_index(&self, win: Window) -> Option<usize> {
        self.banners.iter().position(|b| b.win == win)
    }

    fn handle(&mut self, ev: Event, sh: &mut Shared, out: &mut Vec<UiEvent>) {
        match ev {
            Event::Expose(e) if e.count == 0 => {
                if let Some(i) = self.banner_index(e.window) {
                    self.redraw(i);
                } else if self.hist.as_ref().is_some_and(|h| h.win == e.window) {
                    self.redraw_history(sh);
                }
            }
            Event::EnterNotify(e) if e.mode == NotifyMode::NORMAL => {
                if let Some(i) = self.banner_index(e.event) {
                    self.banners[i].hovered = true;
                    self.banners[i].deadline = None;
                    self.redraw(i);
                }
            }
            Event::LeaveNotify(e) if e.mode == NotifyMode::NORMAL => {
                if let Some(i) = self.banner_index(e.event) {
                    let secs = sh.cfg.banner_seconds;
                    let b = &mut self.banners[i];
                    b.hovered = false;
                    b.hover_btn = None;
                    b.hover_close = false;
                    // Leaving resumes the countdown with a short grace period.
                    b.deadline = b.n.lifetime(secs).map(|_| Instant::now() + Duration::from_secs(u64::from((secs / 2).max(2))));
                    self.redraw(i);
                }
            }
            Event::MotionNotify(e) => self.motion(e.event, f32::from(e.event_x), f32::from(e.event_y), sh),
            Event::ButtonPress(e) => self.press(e, sh, out),
            Event::KeyPress(e) if self.hist.is_some() => self.history_key(e, sh),
            Event::ConfigureNotify(e) if e.window == self.root => self.monitor_reflow(sh),
            Event::PropertyNotify(e) if e.window == self.root && e.atom == u32::from(AtomEnum::RESOURCE_MANAGER) => {
                self.s = xft_scale(&self.conn, self.root);
                self.monitor_reflow(sh);
            }
            _ => {}
        }
    }

    fn motion(&mut self, win: Window, x: f32, y: f32, sh: &Shared) {
        if let Some(i) = self.banner_index(win) {
            let b = &mut self.banners[i];
            let btn = b.btns.iter().position(|(r, _, _)| r.contains(x, y));
            let close = b.close.contains(x, y);
            if btn != b.hover_btn || close != b.hover_close {
                b.hover_btn = btn;
                b.hover_close = close;
                self.redraw(i);
            }
        } else if self.hist.as_ref().is_some_and(|h| h.win == win) {
            let (_, _, rows) = hist_rects(self.hist.as_ref().expect("history checked"), sh);
            let hover = rows.iter().position(|r| r.contains(x, y));
            if let Some(h) = self.hist.as_mut() {
                if h.hover != hover {
                    h.hover = hover;
                    self.redraw_history(sh);
                }
            }
        }
    }

    fn press(&mut self, e: ButtonPressEvent, sh: &mut Shared, out: &mut Vec<UiEvent>) {
        let (x, y) = (f32::from(e.event_x), f32::from(e.event_y));
        if let Some(i) = self.banner_index(e.event) {
            let b = &self.banners[i];
            let id = b.n.id;
            let mut events = Vec::new();
            match e.detail {
                1 => {
                    if b.close.contains(x, y) {
                        // plain dismissal
                    } else if let Some((_, _, key)) = b.btns.iter().find(|(r, _, _)| r.contains(x, y)) {
                        events.push(UiEvent::Action(id, key.clone()));
                    } else if b.n.actions.iter().any(|(k, _)| k == "default") {
                        events.push(UiEvent::Action(id, "default".into()));
                    }
                }
                2 => {
                    out.extend(self.close_all());
                    return;
                }
                3 => {}
                _ => return,
            }
            let resident = self.banners[i].n.resident && !events.is_empty();
            out.extend(events);
            if !resident {
                out.extend(self.close(id, 2));
            }
            return;
        }
        let Some(hw) = self.hist.as_ref() else { return };
        let (win, hw_w, hw_h) = (hw.win, hw.w as f32, hw.h as f32);
        if e.event == win && (e.detail == 4 || e.detail == 5) {
            if let Some(h) = &mut self.hist {
                let max_pan = (h.content_h - h.h as f32).max(0.0);
                let old_pan = h.pan;
                h.pan = (h.pan + if e.detail == 4 { -30.0 * self.s } else { 30.0 * self.s }).clamp(0.0, max_pan);
                if (h.pan - old_pan).abs() < 0.1 {
                    let max = sh.history.items.len().saturating_sub(h.rows);
                    h.scroll = if e.detail == 4 { h.scroll.saturating_sub(1) } else { (h.scroll + 1).min(max) };
                }
                h.ring = false;
            }
            self.redraw_history(sh);
            return;
        }
        if e.event != win || x < 0.0 || y < 0.0 || x >= hw_w || y >= hw_h {
            self.close_history();
            return;
        }
        let (clear, dnd, rows) = hist_rects(hw, sh);
        let target = if hw.layout.error.is_some_and(|layout| shifted(layout.retry, hw.pan).contains(x, y)) {
            Some(HistFocus::Retry)
        } else if clear.contains(x, y) {
            Some(HistFocus::Clear)
        } else if dnd.contains(x, y) {
            Some(HistFocus::Dnd)
        } else {
            rows.iter().position(|r| r.contains(x, y)).and_then(|i| sh.history.items.get(hw.scroll + i)).map(|item| HistFocus::Item(item.id))
        };
        if let Some(target) = target {
            if let Some(hw) = &mut self.hist {
                hw.focus = Some(target);
                hw.ring = false;
            }
            self.activate_history(target, sh);
        }
    }
}

fn paint_banner(fonts: &mut Fonts, pal: &Palette, s: f32, b: &Banner) -> Pixmap {
    {
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
}

fn paint_history(fonts: &mut Fonts, pal: &Palette, s: f32, sh: &Shared, hw: Option<&HistWin>, icons: &HashMap<String, Option<Pixmap>>) -> Pixmap {
    let (w, h) = hw.map_or((1, 1), |h| (h.w, h.h));
    let mut pm = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"));
    gfx::fill_rounded(&mut pm, 0.0, 0.0, w as f32, h as f32, 16.0 * s, pal.bg_color());
    gfx::stroke_rounded(&mut pm, 0.5, 0.5, w as f32 - 1.0, h as f32 - 1.0, 16.0 * s, pal.border, 1.0);
    let Some(hw) = hw else { return pm };
    let layout = hw.layout;
    let pan = hw.pan;
    let (clear, dnd, rows) = hist_rects(hw, sh);
    let title = ellipsize_role(fonts, Role::Title, "Notifications", 16.0 * s, Weight::Bold, (clear.x - 24.0 * s).max(1.0));
    fonts.draw_as(Role::Title, &mut pm, &title, 16.0 * s, layout.title_y - pan, 16.0 * s, Weight::Bold, pal.title);
    let clear_line = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s);
    fonts.draw(&mut pm, "Clear", clear.x + 8.0 * s, clear.y + (clear.h - clear_line) / 2.0, 12.0 * s, Weight::Medium, pal.link);
    let (ar, ag, ab) = pal.accent;
    gfx::fill_rounded(&mut pm, dnd.x, dnd.y, dnd.w, dnd.h, 8.0 * s, pal.button);
    let dnd_line = line_height(fonts, Role::Ui, 12.5 * s, Weight::Medium, s);
    let label = ellipsize(fonts, "Do Not Disturb", 12.5 * s, Weight::Medium, (dnd.w - 64.0 * s).max(1.0));
    fonts.draw(&mut pm, &label, dnd.x + 10.0 * s, dnd.y + (dnd.h - dnd_line) / 2.0, 12.5 * s, Weight::Medium, pal.title);
    let (sw, sh_) = (34.0 * s, 20.0 * s);
    let (sx, sy) = (dnd.x + dnd.w - sw - 10.0 * s, dnd.y + (dnd.h - sh_) / 2.0);
    let track = if sh.cfg.dnd { color(ar, ag, ab, 1.0) } else { pal.button_hover };
    gfx::fill_rounded(&mut pm, sx, sy, sw, sh_, sh_ / 2.0, track);
    let knob_x = if sh.cfg.dnd { sx + sw - sh_ + 2.0 * s } else { sx + 2.0 * s };
    gfx::fill_circle(&mut pm, knob_x + (sh_ - 4.0 * s) / 2.0, sy + sh_ / 2.0, (sh_ - 4.0 * s) / 2.0, if sh.cfg.dnd { accent_text(pal) } else { pal.title });

    if let (Some(error), Some(el)) = (sh.history.error(), layout.error) {
        let card = shifted(el.card, pan);
        let retry = shifted(el.retry, pan);
        gfx::fill_rounded(&mut pm, card.x, card.y, card.w, card.h, 8.0 * s, pal.button);
        let title = if sh.history.clear_pending() {
            "Clear is not saved"
        } else if sh.history.pending() {
            "History is not saved"
        } else {
            "History is unavailable"
        };
        fonts.draw(&mut pm, &ellipsize(fonts, title, 12.0 * s, Weight::SemiBold, (card.w - 20.0 * s).max(1.0)), 22.0 * s, el.title_y - pan, 12.0 * s, Weight::SemiBold, pal.title);
        let reason = ellipsize(fonts, error, 11.0 * s, Weight::Regular, (retry.x - 30.0 * s).max(1.0));
        fonts.draw(&mut pm, &reason, 22.0 * s, el.reason_y - pan, 11.0 * s, Weight::Regular, pal.body);
        let help = if sh.history.pending() { "Unsaved changes remain here for this session" } else { "History file preserved; repair the problem and Retry" };
        fonts.draw(&mut pm, &ellipsize(fonts, help, 10.0 * s, Weight::Regular, (card.w - 20.0 * s).max(1.0)), 22.0 * s, el.help_y - pan, 10.0 * s, Weight::Regular, pal.body);
        gfx::fill_rounded(&mut pm, retry.x, retry.y, retry.w, retry.h, 6.0 * s, color(ar, ag, ab, 1.0));
        let retry_line = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s);
        let label = ellipsize(fonts, "Retry", 12.0 * s, Weight::Medium, (retry.w - 12.0 * s).max(1.0));
        let width = fonts.measure(&label, 12.0 * s, Weight::Medium);
        fonts.draw(&mut pm, &label, retry.x + (retry.w - width) / 2.0, retry.y + (retry.h - retry_line) / 2.0, 12.0 * s, Weight::Medium, accent_text(pal));
    }

    if sh.history.items.is_empty() {
        let msg = ellipsize(fonts, "No Notifications", 13.0 * s, Weight::Medium, (w as f32 - 24.0 * s).max(1.0));
        let width = fonts.measure(&msg, 13.0 * s, Weight::Medium);
        fonts.draw(&mut pm, &msg, (w as f32 - width) / 2.0, layout.header + 16.0 * s - pan, 13.0 * s, Weight::Medium, pal.dim);
    } else {
        let now = unix_now();
        for (i, r) in rows.iter().enumerate() {
            let item = &sh.history.items[hw.scroll + i];
            if hw.hover == Some(i) {
                gfx::fill_rounded(&mut pm, 8.0 * s, r.y, w as f32 - 16.0 * s, r.h - 4.0 * s, 10.0 * s, pal.button);
            }
            let size = (32.0 * s).round() as u32;
            let icon = icons.get(&format!("{}|{}|{}|{}", item.icon, item.app, item.entry, size)).and_then(Option::as_ref);
            draw_icon(fonts, &mut pm, icon, &item.app, 18.0 * s, r.y + 11.0 * s, 32.0 * s);
            let tx = 62.0 * s;
            let when = ago(item.time, now);
            let tw = fonts.measure(&when, 11.0 * s, Weight::Regular);
            fonts.draw(&mut pm, &when, w as f32 - 18.0 * s - tw, r.y + 8.0 * s, 11.0 * s, Weight::Regular, pal.dim);
            let title = ellipsize(fonts, &item.summary, 12.5 * s, Weight::SemiBold, (w as f32 - tx - tw - 30.0 * s).max(1.0));
            fonts.draw(&mut pm, &title, tx, r.y + 8.0 * s, 12.5 * s, Weight::SemiBold, pal.title);
            let body = ellipsize(fonts, item.body.lines().next().unwrap_or(""), 12.0 * s, Weight::Regular, (w as f32 - tx - 18.0 * s).max(1.0));
            fonts.draw(&mut pm, &body, tx, r.y + 8.0 * s + layout.title_line, 12.0 * s, Weight::Regular, pal.body);
            let app = ellipsize(fonts, &item.app, 10.5 * s, Weight::Regular, (w as f32 - tx - 18.0 * s).max(1.0));
            fonts.draw(&mut pm, &app, tx, r.y + 8.0 * s + layout.title_line + layout.body_line, 10.5 * s, Weight::Regular, pal.dim);
        }
    }
    if hw.ring {
        if let Some(r) = history_focus_rect(hw, sh) {
            let row = matches!(hw.focus, Some(HistFocus::Item(_)));
            let inset = if row { 6.0 * s } else { -3.0 * s };
            let y_inset = if row { 1.0 * s } else { -3.0 * s };
            gfx::stroke_rounded(&mut pm, r.x + inset, r.y + y_inset, (r.w - inset * 2.0).max(1.0), (r.h - y_inset * 2.0).max(1.0), 7.0 * s, pal.link, 2.0 * s);
        }
    }
    if hw.content_h > h as f32 {
        let thumb = ((h as f32 / hw.content_h) * h as f32).max(10.0 * s);
        let y = (h as f32 - thumb) * hw.pan / (hw.content_h - h as f32).max(1.0);
        gfx::fill_rounded(&mut pm, (w as f32 - 4.0 * s).max(0.0), y, 3.0 * s, thumb, 1.5 * s, pal.dim);
    }
    pm
}

fn accent_text(pal: &Palette) -> Color {
    let (r, g, b) = crate::theme::Rgb(pal.accent.0, pal.accent.1, pal.accent.2).readable_on().t();
    color(r, g, b, 1.0)
}

fn history_order(sh: &Shared) -> Vec<HistFocus> {
    let mut order = vec![HistFocus::Clear, HistFocus::Dnd];
    if sh.history.error().is_some() {
        order.push(HistFocus::Retry);
    }
    order.extend(sh.history.items.iter().map(|item| HistFocus::Item(item.id)));
    order
}

fn hist_rects(hw: &HistWin, sh: &Shared) -> (R, R, Vec<R>) {
    let n = sh.history.items.len().saturating_sub(hw.scroll).min(hw.rows);
    (shifted(hw.layout.clear, hw.pan), shifted(hw.layout.dnd, hw.pan), (0..n).map(|i| shifted(hw.layout.row_rect(i, hw.w as f32), hw.pan)).collect())
}

fn reveal_history_focus(hw: &mut HistWin, sh: &Shared, s: f32) {
    if let Some(HistFocus::Item(id)) = hw.focus {
        if let Some(index) = sh.history.items.iter().position(|item| item.id == id) {
            if index < hw.scroll {
                hw.scroll = index;
            }
            if index >= hw.scroll + hw.rows {
                hw.scroll = index + 1 - hw.rows;
            }
        }
    }
    if let Some(r) = history_focus_rect(hw, sh) {
        let margin = (4.0 * s).min(hw.h as f32 / 4.0);
        if r.y < margin {
            hw.pan = (hw.pan + r.y - margin).max(0.0);
        }
        if r.y + r.h > hw.h as f32 - margin {
            hw.pan = (hw.pan + r.y + r.h - hw.h as f32 + margin).min((hw.content_h - hw.h as f32).max(0.0));
        }
    }
}

fn history_focus_rect(hw: &HistWin, sh: &Shared) -> Option<R> {
    let r = match hw.focus? {
        HistFocus::Clear => hw.layout.clear,
        HistFocus::Dnd => hw.layout.dnd,
        HistFocus::Retry => hw.layout.error?.retry,
        HistFocus::Item(id) => {
            let i = sh.history.items.iter().position(|item| item.id == id)?;
            if i < hw.scroll || i >= hw.scroll + hw.rows {
                return None;
            }
            hw.layout.row_rect(i - hw.scroll, hw.w as f32)
        }
    };
    Some(shifted(r, hw.pan))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_history_metrics_keep_lines_and_controls_separate() {
        let fonts = Fonts::load().expect("test fonts");
        for (scale, width) in [(1.0, 296.0), (1.0, 380.0), (2.0, 600.0)] {
            let m = HistoryLayout::new(&fonts, scale, width, true);
            assert!(m.dnd.y >= m.clear.y + m.clear.h);
            assert!(m.clear.x >= 0.0 && m.clear.x + m.clear.w <= width);
            let e = m.error.unwrap();
            assert!(e.card.y >= m.dnd.y + m.dnd.h);
            assert!(e.reason_y >= e.title_y + line_height(&fonts, Role::Ui, 12.0 * scale, Weight::SemiBold, scale));
            assert!(e.help_y >= e.retry.y + e.retry.h);
            assert!(e.retry.x >= 0.0 && e.retry.x + e.retry.w <= width);
            assert!(e.retry.h >= line_height(&fonts, Role::Ui, 12.0 * scale, Weight::Medium, scale));
            assert!(m.header >= e.card.y + e.card.h);
            let row = m.row_rect(0, width);
            assert!(row.h >= 16.0 * scale + m.title_line + m.body_line + line_height(&fonts, Role::Ui, 10.5 * scale, Weight::Regular, scale));
            assert!(m.row_rect(1, width).y >= row.y + row.h);
        }
    }

    #[test]
    fn history_focus_uses_notification_ids_and_matches_visible_hit_rows() {
        let fonts = Fonts::load().expect("test fonts");
        let mut sh = Shared { cfg: Config::default(), history: super::super::model::History::in_memory() };
        for id in 1..=5 {
            sh.history.push(HistoryItem { id, app: "Fixture".into(), icon: String::new(), summary: format!("Item {id}"), body: "Body".into(), entry: String::new(), time: 0 });
        }
        assert_eq!(history_order(&sh), vec![HistFocus::Clear, HistFocus::Dnd, HistFocus::Item(5), HistFocus::Item(4), HistFocus::Item(3), HistFocus::Item(2), HistFocus::Item(1)]);
        let hw = HistWin {
            win: 0,
            w: 296,
            h: 300,
            scroll: 3,
            rows: 2,
            x: 0,
            y: 0,
            hover: None,
            focus: Some(HistFocus::Item(2)),
            ring: true,
            pan: 10.0,
            content_h: 310.0,
            layout: HistoryLayout::new(&fonts, 1.0, 296.0, false),
        };
        let (_, _, rows) = hist_rects(&hw, &sh);
        assert_eq!(history_focus_rect(&hw, &sh), Some(rows[0]));
        assert!(rows[0].contains(rows[0].x + rows[0].w / 2.0, rows[0].y + rows[0].h / 2.0));
        sh.history.remove(2);
        assert!(!history_order(&sh).contains(&HistFocus::Item(2)));
        assert!(history_focus_rect(&hw, &sh).is_none(), "a removed ID cannot target the row that replaces it");
        let mut hw = hw;
        hw.focus = Some(HistFocus::Item(1));
        hw.scroll = 0;
        hw.h = 134;
        hw.content_h = hw.layout.header + hw.rows as f32 * hw.layout.row + 8.0;
        reveal_history_focus(&mut hw, &sh, 1.0);
        let r = history_focus_rect(&hw, &sh).unwrap();
        assert!(r.y >= 4.0 && r.y + r.h <= hw.h as f32 - 4.0, "focused row and ring remain in the viewport: {r:?}");
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

    #[test]
    fn resolved_dynamic_and_accessibility_palettes_drive_notification_paint() {
        use crate::theme::model::{Dynamic, Rgb, Selection, Surfaces};
        let mut t = crate::theme::builtin::def("macos").unwrap().resolve("macos", true, &Selection::default());
        let before = Palette::from_theme(&t);
        let d = Dynamic { enabled: true, surfaces: Surfaces::Full, strength: 1., colors: crate::theme::extract::Extracted::from_color(Rgb(180, 20, 90)), ..Default::default() };
        t = crate::theme::dynamic::apply_to(t, &d, false);
        let after = Palette::from_theme(&t);
        assert_ne!(before, after);
        assert_eq!(after.bg, t.popup.t());
        assert_eq!(after.accent, t.accent.t());
        let accessible = crate::accessibility::Preferences { high_contrast: true, reduce_motion: true, ..Default::default() }.apply(t);
        let p = Palette::from_theme(&accessible);
        assert_eq!(p.bg, (0, 0, 0));
        assert_eq!(p.title, Color::WHITE);
        assert_eq!(p.accent, (255, 255, 0));
        // Native history renderer uses these exact colours, without reading files.
        let mut fonts = Fonts::load().expect("test fonts");
        let mut cfg = Config::default();
        cfg.dnd = true;
        let sh = Shared { cfg, history: super::super::model::History::default() };
        let hist = HistWin {
            win: 0,
            w: 380,
            h: 160,
            rows: 1,
            scroll: 0,
            x: 0,
            y: 0,
            hover: None,
            focus: None,
            ring: false,
            pan: 0.0,
            content_h: 160.0,
            layout: HistoryLayout::new(&fonts, 1.0, 380.0, false),
        };
        let pm = paint_history(&mut fonts, &p, 1., &sh, Some(&hist), &HashMap::new());
        let pixel = pm.pixel(190, 125).unwrap();
        assert_eq!((pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()), (0, 0, 0, 255));
        let thumb = pm.pixel(348, 59).unwrap();
        assert_eq!((thumb.red(), thumb.green(), thumb.blue()), (0, 0, 0), "yellow high-contrast switch needs a dark thumb");
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
    fn rect_contains_is_half_open() {
        let r = R { x: 1.0, y: 1.0, w: 2.0, h: 2.0 };
        assert!(r.contains(1.0, 1.0) && r.contains(2.9, 2.9) && !r.contains(3.0, 1.0));
    }

    #[test]
    fn tile_color_is_stable() {
        assert_eq!(tile_color("Mail"), tile_color("Mail"));
    }
}
