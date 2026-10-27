//! X11 front end of macnotify: banner windows stacked below the menu bar, and the history panel.
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
use std::os::fd::{AsRawFd, RawFd};
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
const HIST_HEADER: f32 = 84.0;
const HIST_ROW: f32 = 60.0;
const HIST_MAX_ROWS: usize = 8;
const KEYCODE_ESCAPE: u8 = 9;

/// What the front end reports back to the D-Bus side.
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// (id, reason) with the spec's reasons: 1 expired, 2 dismissed by the user, 3 closed by a call.
    Closed(u32, u32),
    Action(u32, String),
}

/// Colours. The adaptation point for the shared theme module: build a `Palette` from whatever it exposes.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: (u8, u8, u8),
    pub border: Color,
    pub title: Color,
    pub body: Color,
    pub dim: Color,
    pub button: Color,
    pub button_hover: Color,
    pub accent: (u8, u8, u8),
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
        }
    }

    /// Follows the same dark/light switch the control center uses.
    pub fn current() -> Palette {
        if crate::controlcenter::sys::is_light() {
            Palette::light()
        } else {
            Palette::dark()
        }
    }

    fn bg_color(&self) -> Color {
        color(self.bg.0, self.bg.1, self.bg.2, 1.0)
    }

    fn bg_pixel(&self) -> u32 {
        (u32::from(self.bg.0) << 16) | (u32::from(self.bg.1) << 8) | u32::from(self.bg.2)
    }
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
    if f.measure(s, px, wt) <= max_w {
        return s.to_string();
    }
    let mut out: Vec<char> = s.chars().collect();
    while !out.is_empty() {
        out.pop();
        let cand: String = out.iter().collect::<String>() + "\u{2026}";
        if f.measure(&cand, px, wt) <= max_w {
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
        if let Some(p) = icons::find_svg(name).and_then(|p| icons::render_svg(&p, size)) {
            return Some(p);
        }
        if let Some(p) = icons::find_png(name).and_then(|p| icons::load_png_scaled(&p, size)) {
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
    let px = size * 0.5;
    let w = fonts.measure(&letter, px, Weight::SemiBold);
    fonts.draw(pm, &letter, x + (size - w) / 2.0, y + size * 0.22, px, Weight::SemiBold, Color::WHITE);
}

// ----------------------------------------------------------------- banners

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
}

struct HistWin {
    win: Window,
    w: u32,
    h: u32,
    scroll: usize,
    hover: Option<usize>,
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
    pending: VecDeque<Notification>,
    hist: Option<HistWin>,
    icon_cache: HashMap<String, Option<Pixmap>>,
    default_secs: u32,
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
        let atom = |name: &str| -> Result<Atom, String> { Ok(conn.intern_atom(false, name.as_bytes()).map_err(cerr)?.reply().map_err(cerr)?.atom) };
        let at = Atoms { wtype: atom("_NET_WM_WINDOW_TYPE")?, wtype_notification: atom("_NET_WM_WINDOW_TYPE_NOTIFICATION")?, wm_name: atom("_NET_WM_NAME")?, utf8: atom("UTF8_STRING")? };
        let mon = panel_monitor(&conn, root, (i32::from(screen.width_in_pixels), i32::from(screen.height_in_pixels)));
        let s = xft_scale(&conn, root);
        let fonts = Fonts::load().ok_or("no usable font found")?;
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
        })
    }

    pub fn fd(&self) -> RawFd {
        self.conn.stream().as_raw_fd()
    }

    fn px(&self, v: f32) -> f32 {
        v * self.s
    }

    fn top(&self) -> i32 {
        // below the horizontal bar; with a vertical one (Caelestia) nothing is at the top edge
        let below = if crate::theme::th().bar_vertical() { 12.0 } else { crate::theme::BAR_HEIGHT + 8.0 };
        self.mon.y + self.px(below).round() as i32
    }

    fn make_window(&self, x: i32, y: i32, w: u32, h: u32, keys: bool) -> Result<Window, String> {
        let win = self.conn.generate_id().map_err(cerr)?;
        let mut mask = EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::ENTER_WINDOW | EventMask::LEAVE_WINDOW | EventMask::POINTER_MOTION;
        if keys {
            mask |= EventMask::KEY_PRESS;
        }
        let aux = CreateWindowAux::new().override_redirect(1).background_pixel(self.pal.bg_pixel()).event_mask(mask);
        self.conn.create_window(0, win, self.root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).map_err(cerr)?;
        let _ = self.conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"macnotify\0Macnotify\0");
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
        self.pal = Palette::current();
        self.default_secs = secs;
        if let Some(i) = self.banners.iter().position(|b| b.n.id == n.id) {
            self.update_banner(i, n, secs);
            return;
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.id == n.id) {
            *p = n;
            return;
        }
        if self.banners.len() >= MAX_VISIBLE {
            self.pending.push_back(n);
            return;
        }
        self.create_banner(n, secs);
    }

    #[allow(clippy::type_complexity)]
    fn layout(&mut self, n: &Notification) -> (u32, u32, String, Vec<String>, Vec<(R, String, String)>, R, R) {
        let s = self.s;
        let w = BANNER_W * s;
        let (pad, icon, gap) = (14.0 * s, 36.0 * s, 10.0 * s);
        let tx = pad + icon + gap;
        let tw = w - tx - pad;
        let title_w = tw - 44.0 * s;
        let title = ellipsize(&self.fonts, &n.summary, 13.0 * s, Weight::SemiBold, title_w);
        let body = wrap(&self.fonts, &n.body, 12.0 * s, Weight::Regular, tw, 3);
        let bottom = 12.0 * s + 17.0 * s + 15.0 * s * body.len() as f32;
        let mut h = (bottom + 12.0 * s).max(pad * 2.0 + icon);
        let mut btns = Vec::new();
        let acts: Vec<&(String, String)> = n.actions.iter().filter(|(k, l)| k != "default" && !l.is_empty()).take(3).collect();
        if !acts.is_empty() {
            let by = (bottom + 8.0 * s).max(pad + icon + 4.0 * s);
            let bw = (tw - 6.0 * s * (acts.len() as f32 - 1.0)) / acts.len() as f32;
            for (i, (k, l)) in acts.iter().enumerate() {
                let r = R { x: tx + i as f32 * (bw + 6.0 * s), y: by, w: bw, h: 26.0 * s };
                btns.push((r, ellipsize(&self.fonts, l, 12.0 * s, Weight::Medium, bw - 12.0 * s), k.clone()));
            }
            h = by + 26.0 * s + 12.0 * s;
        }
        let close = R { x: w - pad - 18.0 * s, y: 11.0 * s, w: 18.0 * s, h: 18.0 * s };
        let icon_at = R { x: pad, y: pad, w: icon, h: icon };
        (w.round() as u32, h.round() as u32, title, body, btns, close, icon_at)
    }

    fn resolve_icon(&mut self, n: &Notification) -> Option<Pixmap> {
        let size = (36.0 * self.s).round() as u32;
        n.image.as_ref().and_then(|i| image_pixmap(i, size)).or_else(|| self.icon_cached(&n.icon, &n.app, &n.entry, size))
    }

    fn create_banner(&mut self, n: Notification, secs: u32) {
        let (w, h, title, body, btns, close, icon_at) = self.layout(&n);
        let icon = self.resolve_icon(&n);
        let x = self.mon.x + self.mon.w - w as i32 - self.px(MARGIN).round() as i32;
        let Ok(win) = self.make_window(x, self.top(), w, h, false) else { return };
        let deadline = n.deadline(secs);
        let b = Banner { n, win, w, h, y: self.top(), deadline, hovered: false, hover_btn: None, hover_close: false, icon, title, body, btns, close, icon_at };
        let pm = paint_banner(&mut self.fonts, &self.pal, self.s, &b);
        self.set_shape(win, &pm);
        let _ = self.conn.map_window(win);
        present(&self.conn, win, self.gc, &pm, self.depth);
        self.banners.insert(0, b);
        self.fonts.trim();
        self.restack();
    }

    fn update_banner(&mut self, i: usize, n: Notification, secs: u32) {
        let (w, h, title, body, btns, close, icon_at) = self.layout(&n);
        let icon = self.resolve_icon(&n);
        let b = &mut self.banners[i];
        b.deadline = n.deadline(secs);
        b.hovered = false;
        (b.n, b.w, b.h, b.title, b.body, b.btns, b.close, b.icon_at, b.icon) = (n, w, h, title, body, btns, close, icon_at, icon);
        let win = b.win;
        let _ = self.conn.configure_window(win, &ConfigureWindowAux::new().width(w).height(h));
        self.restack();
        self.redraw(i);
    }

    fn restack(&mut self) {
        let mut y = self.top();
        let gap = self.px(GAP).round() as i32;
        let x_of = |w: u32, mon: &MonRect, m: i32| mon.x + mon.w - w as i32 - m;
        let m = self.px(MARGIN).round() as i32;
        for b in &mut self.banners {
            b.y = y;
            let _ = self.conn.configure_window(b.win, &ConfigureWindowAux::new().x(x_of(b.w, &self.mon, m)).y(y));
            y += b.h as i32 + gap;
        }
        let _ = self.conn.flush();
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
        if let Some(i) = self.pending.iter().position(|p| p.id == id) {
            self.pending.remove(i);
            return vec![UiEvent::Closed(id, reason)];
        }
        Vec::new()
    }

    fn promote(&mut self) {
        while self.banners.len() < MAX_VISIBLE {
            let Some(n) = self.pending.pop_front() else { break };
            // Queued entries start their clock when they become visible.
            self.create_banner_at_end(n, self.default_secs);
        }
    }

    fn create_banner_at_end(&mut self, n: Notification, secs: u32) {
        let before = self.banners.len();
        self.create_banner(n, secs);
        // create_banner inserts at the top; a promoted entry belongs below the ones already shown.
        if self.banners.len() > before && before > 0 {
            let b = self.banners.remove(0);
            self.banners.push(b);
            self.restack();
        }
    }

    /// Drops every banner and queued entry (used when the history panel opens).
    pub fn close_all(&mut self) -> Vec<UiEvent> {
        let ids: Vec<u32> = self.banners.iter().map(|b| b.n.id).chain(self.pending.iter().map(|p| p.id)).collect();
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.close(id, 2));
        }
        out
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.banners.iter().filter_map(|b| b.deadline).min()
    }

    pub fn expire(&mut self, now: Instant) -> Vec<UiEvent> {
        let due: Vec<u32> = self.banners.iter().filter(|b| b.deadline.is_some_and(|d| d <= now)).map(|b| b.n.id).collect();
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
        self.pal = Palette::current();
        let rows = sh.history.items.len().clamp(1, HIST_MAX_ROWS);
        let (w, h) = ((HIST_W * self.s).round() as u32, ((HIST_HEADER + rows as f32 * HIST_ROW + 10.0) * self.s).round() as u32);
        let x = self.mon.x + self.mon.w - w as i32 - self.px(MARGIN).round() as i32;
        let Ok(win) = self.make_window(x, self.top(), w, h, true) else { return events };
        self.hist = Some(HistWin { win, w, h, scroll: 0, hover: None });
        let pm = paint_history(&mut self.fonts, &self.pal, self.s, sh, self.hist.as_ref(), &self.icon_cache);
        self.set_shape(win, &pm);
        let _ = self.conn.map_window(win);
        present(&self.conn, win, self.gc, &pm, self.depth);
        let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, win, x11rb::CURRENT_TIME);
        let _ = self.conn.grab_keyboard(true, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC);
        let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        let _ = self.conn.grab_pointer(true, win, mask, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
        self.fonts.trim();
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

    fn redraw_history(&mut self, sh: &Shared) {
        // Make sure every visible row has its icon cached before painting (painting only reads the cache).
        let scroll = self.hist.as_ref().map_or(0, |h| h.scroll);
        let size = (32.0 * self.s).round() as u32;
        let items: Vec<HistoryItem> = sh.history.items.iter().skip(scroll).take(HIST_MAX_ROWS).cloned().collect();
        for it in &items {
            self.icon_cached(&it.icon, &it.app, &it.entry, size);
        }
        let Some(win) = self.hist.as_ref().map(|h| h.win) else { return };
        let rows = sh.history.items.len().clamp(1, HIST_MAX_ROWS);
        let nh = ((HIST_HEADER + rows as f32 * HIST_ROW + 10.0) * self.s).round() as u32;
        if let Some(h) = self.hist.as_mut() {
            if h.h != nh {
                h.h = nh;
                let _ = self.conn.configure_window(win, &ConfigureWindowAux::new().height(nh));
            }
        }
        let pm = paint_history(&mut self.fonts, &self.pal, self.s, sh, self.hist.as_ref(), &self.icon_cache);
        self.set_shape(win, &pm);
        present(&self.conn, win, self.gc, &pm, self.depth);
        let _ = self.conn.flush();
        self.fonts.trim();
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
            Event::KeyPress(e) if e.detail == KEYCODE_ESCAPE && self.hist.is_some() => self.close_history(),
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
            let (_, _, rows) = hist_rects(self.s, self.hist.as_ref().map_or(0, |h| h.scroll), sh);
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
            let max = sh.history.items.len().saturating_sub(HIST_MAX_ROWS);
            if let Some(h) = self.hist.as_mut() {
                h.scroll = if e.detail == 4 { h.scroll.saturating_sub(1) } else { (h.scroll + 1).min(max) };
            }
            self.redraw_history(sh);
            return;
        }
        if e.event != win || x < 0.0 || y < 0.0 || x >= hw_w || y >= hw_h {
            self.close_history();
            return;
        }
        let (clear, dnd, rows) = hist_rects(self.s, self.hist.as_ref().map_or(0, |h| h.scroll), sh);
        if clear.contains(x, y) {
            sh.history.clear();
            if let Some(h) = self.hist.as_mut() {
                h.scroll = 0;
            }
        } else if dnd.contains(x, y) {
            let on = !sh.cfg.dnd;
            if Config::save_dnd(on).is_ok() {
                sh.cfg.dnd = on;
            }
        } else if let Some(i) = rows.iter().position(|r| r.contains(x, y)) {
            let scroll = self.hist.as_ref().map_or(0, |h| h.scroll);
            if let Some(id) = sh.history.items.get(scroll + i).map(|it| it.id) {
                sh.history.remove(id);
            }
            let max = sh.history.items.len().saturating_sub(HIST_MAX_ROWS);
            if let Some(h) = self.hist.as_mut() {
                h.scroll = h.scroll.min(max);
            }
        }
        self.redraw_history(sh);
    }
}

fn paint_banner(fonts: &mut Fonts, pal: &Palette, s: f32, b: &Banner) -> Pixmap {
    {
        let mut pm = Pixmap::new(b.w, b.h).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"));
        let (w, h) = (b.w as f32, b.h as f32);
        gfx::fill_rounded(&mut pm, 0.0, 0.0, w, h, 16.0 * s, pal.bg_color());
        gfx::stroke_rounded(&mut pm, 0.5, 0.5, w - 1.0, h - 1.0, 16.0 * s, pal.border, 1.0);
        draw_icon(fonts, &mut pm, b.icon.as_ref(), &b.n.app, b.icon_at.x, b.icon_at.y, b.icon_at.w);
        let tx = 14.0 * s + 36.0 * s + 10.0 * s;
        fonts.draw_as(Role::Title, &mut pm, &b.title, tx, 12.0 * s, 13.0 * s, Weight::SemiBold, pal.title);
        for (i, line) in b.body.iter().enumerate() {
            fonts.draw(&mut pm, line, tx, 12.0 * s + 17.0 * s + i as f32 * 15.0 * s, 12.0 * s, Weight::Regular, pal.body);
        }
        if b.hovered {
            let c = b.close;
            let bg = if b.hover_close { pal.button_hover } else { pal.button };
            gfx::fill_circle(&mut pm, c.x + c.w / 2.0, c.y + c.h / 2.0, c.w / 2.0, bg);
            let t = fonts.measure("\u{d7}", 14.0 * s, Weight::Medium);
            fonts.draw(&mut pm, "\u{d7}", c.x + (c.w - t) / 2.0, c.y + 1.0 * s, 14.0 * s, Weight::Medium, pal.title);
        } else {
            let t = fonts.measure("now", 11.0 * s, Weight::Regular);
            fonts.draw(&mut pm, "now", w - 14.0 * s - t, 13.0 * s, 11.0 * s, Weight::Regular, pal.dim);
        }
        for (i, (r, label, _)) in b.btns.iter().enumerate() {
            let bg = if b.hover_btn == Some(i) { pal.button_hover } else { pal.button };
            gfx::fill_rounded(&mut pm, r.x, r.y, r.w, r.h, 7.0 * s, bg);
            let t = fonts.measure(label, 12.0 * s, Weight::Medium);
            fonts.draw(&mut pm, label, r.x + (r.w - t) / 2.0, r.y + 5.0 * s, 12.0 * s, Weight::Medium, pal.title);
        }
        pm
    }
}

fn paint_history(fonts: &mut Fonts, pal: &Palette, s: f32, sh: &Shared, hw: Option<&HistWin>, icons: &HashMap<String, Option<Pixmap>>) -> Pixmap {
    let (w, h) = hw.map_or((1, 1), |h| (h.w, h.h));
    let mut pm = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"));
    gfx::fill_rounded(&mut pm, 0.0, 0.0, w as f32, h as f32, 16.0 * s, pal.bg_color());
    gfx::stroke_rounded(&mut pm, 0.5, 0.5, w as f32 - 1.0, h as f32 - 1.0, 16.0 * s, pal.border, 1.0);
    let (clear, dnd, rows) = hist_rects(s, hw.map_or(0, |h| h.scroll), sh);
    fonts.draw_as(Role::Title, &mut pm, "Notifications", 16.0 * s, 13.0 * s, 16.0 * s, Weight::Bold, pal.title);
    let (ar, ag, ab) = pal.accent;
    fonts.draw(&mut pm, "Clear", clear.x + 8.0 * s, clear.y + 3.0 * s, 12.0 * s, Weight::Medium, color(ar, ag, ab, 1.0));
    // Do Not Disturb row with a switch.
    gfx::fill_rounded(&mut pm, dnd.x, dnd.y, dnd.w, dnd.h, 8.0 * s, pal.button);
    fonts.draw(&mut pm, "Do Not Disturb", dnd.x + 10.0 * s, dnd.y + 7.0 * s, 12.5 * s, Weight::Medium, pal.title);
    let (sw, sh_) = (34.0 * s, 20.0 * s);
    let (sx, sy) = (dnd.x + dnd.w - sw - 10.0 * s, dnd.y + (dnd.h - sh_) / 2.0);
    let track = if sh.cfg.dnd { color(ar, ag, ab, 1.0) } else { color(120, 120, 128, 0.5) };
    gfx::fill_rounded(&mut pm, sx, sy, sw, sh_, sh_ / 2.0, track);
    let knob_x = if sh.cfg.dnd { sx + sw - sh_ + 2.0 * s } else { sx + 2.0 * s };
    gfx::fill_circle(&mut pm, knob_x + (sh_ - 4.0 * s) / 2.0, sy + sh_ / 2.0, (sh_ - 4.0 * s) / 2.0, Color::WHITE);

    if sh.history.items.is_empty() {
        let msg = "No Notifications";
        let t = fonts.measure(msg, 13.0 * s, Weight::Medium);
        fonts.draw(&mut pm, msg, (w as f32 - t) / 2.0, (HIST_HEADER + 20.0) * s, 13.0 * s, Weight::Medium, pal.dim);
        return pm;
    }
    let scroll = hw.map_or(0, |h| h.scroll);
    let now = unix_now();
    for (i, r) in rows.iter().enumerate() {
        let item = &sh.history.items[scroll + i];
        if hw.is_some_and(|h| h.hover == Some(i)) {
            gfx::fill_rounded(&mut pm, 8.0 * s, r.y, w as f32 - 16.0 * s, r.h - 4.0 * s, 10.0 * s, pal.button);
        }
        let size = (32.0 * s).round() as u32;
        let icon = icons.get(&format!("{}|{}|{}|{}", item.icon, item.app, item.entry, size)).and_then(Option::as_ref);
        draw_icon(fonts, &mut pm, icon, &item.app, 18.0 * s, r.y + 11.0 * s, 32.0 * s);
        let tx = 62.0 * s;
        let when = ago(item.time, now);
        let tw = fonts.measure(&when, 11.0 * s, Weight::Regular);
        fonts.draw(&mut pm, &when, w as f32 - 18.0 * s - tw, r.y + 10.0 * s, 11.0 * s, Weight::Regular, pal.dim);
        let title = ellipsize(fonts, &item.summary, 12.5 * s, Weight::SemiBold, w as f32 - tx - tw - 30.0 * s);
        fonts.draw(&mut pm, &title, tx, r.y + 9.0 * s, 12.5 * s, Weight::SemiBold, pal.title);
        let first = item.body.lines().next().unwrap_or("");
        let body = ellipsize(fonts, first, 12.0 * s, Weight::Regular, w as f32 - tx - 18.0 * s);
        fonts.draw(&mut pm, &body, tx, r.y + 27.0 * s, 12.0 * s, Weight::Regular, pal.body);
        fonts.draw(&mut pm, &ellipsize(fonts, &item.app, 10.5 * s, Weight::Regular, w as f32 - tx - 18.0 * s), tx, r.y + 42.0 * s, 10.5 * s, Weight::Regular, pal.dim);
    }
    pm
}

fn hist_rects(s: f32, scroll: usize, sh: &Shared) -> (R, R, Vec<R>) {
    let w = HIST_W * s;
    let clear = R { x: w - 70.0 * s, y: 14.0 * s, w: 56.0 * s, h: 22.0 * s };
    let dnd = R { x: 12.0 * s, y: 44.0 * s, w: w - 24.0 * s, h: 30.0 * s };
    let n = sh.history.items.len().saturating_sub(scroll).min(HIST_MAX_ROWS);
    let rows = (0..n).map(|i| R { x: 0.0, y: (HIST_HEADER + i as f32 * HIST_ROW) * s, w, h: HIST_ROW * s }).collect();
    (clear, dnd, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

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
