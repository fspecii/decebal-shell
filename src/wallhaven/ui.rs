//! The Wallhaven browser window: layout, drawing and input handling.
//!
//! Geometry and colours follow the original QML plugin: a 52 px title bar (traffic lights,
//! title, search field), a 40 px toolbar (sort tabs, "fits my screen", status) and an endless
//! grid of 3:2 thumbnails that loads further pages in the background.

use super::api::{self, Item, Search, Sorting};
use super::cache::Lru;
use super::layout::{Grid, BUFFER_ROWS, PREFETCH_ROWS};
use super::model::Model;
use super::net::{Job, Msg, Net};
use super::textfield::TextField;
use crate::app::Button;
use crate::gfx::{color, fill_circle, fill_rounded, rounded_rect, stroke_rounded};
use crate::text::{Fonts, Weight};
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use std::collections::HashSet;
use tiny_skia::{Color, FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, Pattern, Pixmap, PixmapPaint, SpreadMode, Stroke, Transform};

type Rect = (f32, f32, f32, f32);

const TITLEBAR_H: f32 = 52.0;
const TOOLBAR_H: f32 = 40.0;
const GRID_SIDE: f32 = 12.0;
const GRID_TOP_GAP: f32 = 10.0;
const CELL_MARGIN: f32 = 6.0;
const CELL_RADIUS: f32 = 10.0;
const SEARCH_W: f32 = 300.0;
const SEARCH_H: f32 = 30.0;
const WINDOW_RADIUS: f32 = 14.0;
const THUMB_MAX_W: u32 = 480;
const MAX_CACHED_THUMBS: usize = 100;
const MAX_PENDING_THUMBS: usize = 24;

const TEXT: (u8, u8, u8) = (245, 245, 247);
const ACCENT: (u8, u8, u8) = (10, 132, 255);

fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

fn text_color() -> Color {
    color(TEXT.0, TEXT.1, TEXT.2, 1.0)
}
fn subtext_color() -> Color {
    color(255, 255, 255, 0.55)
}
fn field_color() -> Color {
    color(255, 255, 255, 0.08)
}
fn accent(a: f32) -> Color {
    color(ACCENT.0, ACCENT.1, ACCENT.2, a)
}

/// Clickable areas of the title bar and toolbar, in window coordinates.
#[derive(Debug, Clone)]
struct Chrome {
    lights: [Rect; 3],
    search: Rect,
    seg_bg: Rect,
    segs: Vec<(Sorting, Rect)>,
    fit: Rect,
}

pub struct WallhavenApp {
    waker: Option<Waker>,
    net: Option<Net>,
    fonts: Option<Fonts>,
    model: Model,
    search: Search,
    field: TextField,
    screen: (u32, u32),
    size: (u32, u32),
    rounded: bool,
    scroll: f32,
    hover: Option<usize>,
    thumbs: Lru<String, Pixmap>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    applying: Option<String>,
    applied: Option<String>,
    notice: String,
    layer: Option<Pixmap>,
    mask: Option<((u32, u32, u32), Mask)>,
    /// Redraw needed after `tick` (set by message handling).
    dirty: bool,
    last_trim: std::time::Instant,
}

impl WallhavenApp {
    /// `query` seeds the search box; an empty query starts on the Top list. `recent` opens the Recent tab.
    pub fn new(query: &str, recent: bool, screen: Option<(u32, u32)>) -> Self {
        let query = query.trim();
        let sorting = if recent {
            Sorting::Recent
        } else if query.is_empty() {
            Sorting::Toplist
        } else {
            Sorting::Relevance
        };
        let mut search = Search::new(query, sorting);
        search.screen = screen.unwrap_or((0, 0));
        Self {
            waker: None,
            net: None,
            fonts: Fonts::load(),
            model: Model::default(),
            search,
            field: TextField::new(query),
            screen: screen.unwrap_or((0, 0)),
            size: (1280, 820),
            rounded: true,
            scroll: 0.0,
            hover: None,
            thumbs: Lru::new(MAX_CACHED_THUMBS),
            pending: HashSet::new(),
            failed: HashSet::new(),
            applying: None,
            applied: super::wallpaper::current_wallpaper_id(),
            notice: String::new(),
            layer: None,
            mask: None,
            dirty: false,
            last_trim: std::time::Instant::now(),
        }
    }

    // ------------------------------------------------------------ geometry

    fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        match &self.fonts {
            Some(f) => f.measure(s, px, w),
            None => s.chars().count() as f32 * px * 0.55,
        }
    }

    fn chrome(&self) -> Chrome {
        let w = self.size.0 as f32;
        let lights = std::array::from_fn(|i| (16.0 + i as f32 * 20.0, TITLEBAR_H / 2.0 - 6.0, 12.0, 12.0));
        let search = (w - 16.0 - SEARCH_W, (TITLEBAR_H - SEARCH_H) / 2.0, SEARCH_W, SEARCH_H);
        let ty = TITLEBAR_H + (TOOLBAR_H - 28.0) / 2.0;
        let mut x = 18.0;
        let mut segs = Vec::new();
        for s in Sorting::ALL {
            let bw = self.measure(s.label(), 12.0, Weight::Regular) + 26.0;
            segs.push((s, (x, ty + 2.0, bw, 24.0)));
            x += bw;
        }
        let seg_bg = (16.0, ty, x - 16.0 + 2.0, 28.0);
        let label = self.fit_label();
        let fit_w = 16.0 + 6.0 + self.measure(&label, 12.0, Weight::Regular);
        let fit = (seg_bg.0 + seg_bg.2 + 16.0, TITLEBAR_H + (TOOLBAR_H - 16.0) / 2.0, fit_w, 16.0);
        Chrome { lights, search, seg_bg, segs, fit }
    }

    fn fit_label(&self) -> String {
        if self.screen.0 > 0 {
            format!("Fits my screen ({}×{})", self.screen.0, self.screen.1)
        } else {
            "Fits my screen".to_string()
        }
    }

    fn recent_mode(&self) -> bool {
        self.search.sorting == Sorting::Recent
    }

    /// The scrolling grid in window coordinates.
    fn grid(&self) -> Grid {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let y = TITLEBAR_H + TOOLBAR_H + GRID_TOP_GAP;
        Grid::new(GRID_SIDE, y, w - 2.0 * GRID_SIDE, h - y)
    }

    /// The same grid with its origin at (0, 0), for drawing into the clipped layer.
    fn local_grid(&self) -> Grid {
        let g = self.grid();
        Grid::new(0.0, 0.0, g.w, g.h)
    }

    fn clamp_scroll(&mut self) {
        let g = self.grid();
        self.scroll = g.clamp_scroll(self.scroll, self.model.items.len(), self.model.has_more());
    }

    // ------------------------------------------------------------ searching

    fn new_seed() -> String {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        let mut n = nanos as u64 ^ (nanos >> 64) as u64;
        let mut s = String::new();
        for _ in 0..6 {
            s.push(char::from_digit((n % 36) as u32, 36).unwrap_or('a'));
            n /= 36;
        }
        s
    }

    fn start_search(&mut self) {
        self.search.query = self.field.text().trim().to_string();
        self.search.seed = Self::new_seed();
        self.search.screen = self.screen;
        self.notice.clear();
        self.scroll = 0.0;
        self.hover = None;
        let generation = self.model.new_search();
        if self.recent_mode() {
            self.model.loading = true;
            if let Some(net) = &self.net {
                net.submit(Job::Recent { gen: generation });
            }
        } else {
            self.maybe_prefetch();
        }
        self.dirty = true;
    }

    fn set_sorting(&mut self, s: Sorting) {
        self.search.sorting = s;
        self.start_search();
    }

    fn submit_search(&mut self) {
        let q = self.field.text().trim().to_string();
        self.search.sorting = api::sorting_after_submit(self.search.sorting, &q);
        self.start_search();
    }

    /// Keeps fetching while the user is within a few rows of the end.
    fn maybe_prefetch(&mut self) {
        if self.recent_mode() {
            return;
        }
        let Some(page) = self.model.next_page() else {
            return;
        };
        let g = self.grid();
        let n = self.model.items.len();
        if n > 0 && g.rows_left(self.scroll, n, self.model.has_more()) >= PREFETCH_ROWS {
            return;
        }
        if let Some(net) = &self.net {
            self.model.begin_fetch();
            net.submit(Job::Search { gen: self.model.generation, page, params: self.search.params(page) });
        }
    }

    /// Requests thumbnails for the visible rows plus a few rows each side.
    fn request_thumbs(&mut self) {
        let Some(net) = &self.net else { return };
        let g = self.grid();
        for i in g.visible(self.scroll, self.model.items.len(), BUFFER_ROWS) {
            if self.pending.len() >= MAX_PENDING_THUMBS {
                break;
            }
            let item = &self.model.items[i];
            if self.thumbs.contains(&item.id) || self.pending.contains(&item.id) || self.failed.contains(&item.id) {
                continue;
            }
            self.pending.insert(item.id.clone());
            net.submit(Job::Thumb { id: item.id.clone(), src: item.thumb.clone(), max_w: THUMB_MAX_W });
        }
    }

    fn housekeeping(&mut self) {
        self.maybe_prefetch();
        self.request_thumbs();
    }

    /// Applies one finished background job. Returns true when the screen should be redrawn.
    pub fn handle_msg(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Page { gen, page, result } => self.model.apply_page(gen, page, result),
            Msg::Local { gen, items } => self.model.set_local(gen, items),
            Msg::Thumb { id, pixmap } => {
                self.pending.remove(&id);
                match pixmap {
                    Some(pm) => self.thumbs.insert(id, pm),
                    None => {
                        self.failed.insert(id);
                    }
                }
                true
            }
            Msg::Applied { id, result } => {
                self.applying = None;
                match result {
                    Ok(m) => {
                        self.applied = Some(id);
                        self.notice = m;
                    }
                    Err(e) => {
                        self.applied = None;
                        self.notice = e;
                    }
                }
                true
            }
            Msg::Saved { result, .. } => {
                self.notice = result.unwrap_or_else(|e| e);
                true
            }
        }
    }

    fn apply_item(&mut self, i: usize) {
        if self.applying.is_some() {
            return;
        }
        let Some(item) = self.model.items.get(i).cloned() else {
            return;
        };
        self.notice.clear();
        self.applying = Some(item.id.clone());
        if let Some(net) = &self.net {
            net.apply(item);
        }
    }

    fn save_item(&mut self, i: usize) {
        let Some(item) = self.model.items.get(i).cloned() else {
            return;
        };
        self.notice = format!("Saving {}…", item.id);
        if let Some(net) = &self.net {
            net.save(item);
        }
    }

    fn scroll_rows(&mut self, rows: f32) {
        let ch = self.grid().cell_h;
        self.scroll += rows * ch;
        self.clamp_scroll();
        self.housekeeping();
    }

    // ------------------------------------------------------------ drawing

    #[allow(clippy::too_many_arguments)] // a drawing helper: target, text, position, font size, weight, colour
    fn text_at(&mut self, pm: &mut Pixmap, s: &str, x: f32, cy: f32, px: f32, weight: Weight, c: Color) {
        if let Some(f) = &mut self.fonts {
            f.draw(pm, s, x, cy - px * 0.62, px, weight, c);
        }
    }

    fn corner_mask(&mut self, w: u32, h: u32, r: u32) -> Option<&Mask> {
        let key = (w, h, r);
        if self.mask.as_ref().map(|(k, _)| *k) != Some(key) {
            let mut m = Mask::new(w, h)?;
            let path = rounded_rect(0.0, 0.0, w as f32, h as f32, r as f32)?;
            m.fill_path(&path, FillRule::Winding, true, Transform::identity());
            self.mask = Some((key, m));
        }
        self.mask.as_ref().map(|(_, m)| m)
    }

    fn draw_titlebar(&mut self, pm: &mut Pixmap, ch: &Chrome) {
        for (i, c) in [(255u8, 95u8, 87u8), (254, 188, 46), (40, 200, 64)].iter().enumerate() {
            let (x, y, w, h) = ch.lights[i];
            fill_circle(pm, x + w / 2.0, y + h / 2.0, 6.0, color(c.0, c.1, c.2, 1.0));
        }
        self.text_at(pm, "Wallpapers", 80.0, TITLEBAR_H / 2.0, 15.0, Weight::Bold, text_color());

        // Search field.
        let (sx, sy, sw, sh) = ch.search;
        fill_rounded(pm, sx, sy, sw, sh, 8.0, field_color());
        stroke_rounded(pm, sx + 1.0, sy + 1.0, sw - 2.0, sh - 2.0, 7.0, accent(0.8), 2.0);
        draw_magnifier(pm, sx + 8.0, sy + (sh - 15.0) / 2.0, 15.0, subtext_color());
        let (ix, iw) = (sx + 30.0, sw - 38.0);
        if self.field.text().is_empty() {
            self.text_at(pm, "Search wallhaven.cc", ix, sy + sh / 2.0, 13.0, Weight::Regular, subtext_color());
        } else {
            self.draw_field_text(pm, ix, sy, iw, sh);
        }
    }

    fn draw_field_text(&mut self, pm: &mut Pixmap, ix: f32, sy: f32, iw: f32, sh: f32) {
        let before = self.measure(self.field.before_cursor(), 13.0, Weight::Regular);
        let offset = (before - (iw - 4.0)).max(0.0);
        let Some(mut tmp) = Pixmap::new(iw.max(1.0) as u32, sh.max(1.0) as u32) else {
            return;
        };
        let text = self.field.text().to_string();
        if let Some(f) = &mut self.fonts {
            f.draw(&mut tmp, &text, -offset, sh / 2.0 - 13.0 * 0.62, 13.0, Weight::Regular, text_color());
        }
        let mut cp = Paint::default();
        cp.set_color(text_color());
        // Snapped to whole pixels: a fractional 1 px rect trips a debug assertion in tiny-skia.
        if let Some(r) = tiny_skia::Rect::from_xywh((before - offset).round(), (sh / 2.0 - 8.0).round(), 1.0, 16.0) {
            tmp.fill_rect(r, &cp, Transform::identity(), None);
        }
        pm.draw_pixmap(ix as i32, sy as i32, tmp.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }

    fn status_text(&self) -> String {
        if !self.notice.is_empty() {
            return self.notice.clone();
        }
        if !self.model.error.is_empty() {
            return self.model.error.clone();
        }
        let n = self.model.items.len();
        if self.recent_mode() {
            return format!("{n} wallpapers you've used · click one to set it again");
        }
        let tail = if self.model.has_more() {
            if self.model.loading {
                " · loading more…"
            } else {
                " · more below"
            }
        } else {
            ""
        };
        format!("{n} wallpapers{tail}")
    }

    fn draw_toolbar(&mut self, pm: &mut Pixmap, ch: &Chrome) {
        let (bx, by, bw, bh) = ch.seg_bg;
        fill_rounded(pm, bx, by, bw, bh, 7.0, field_color());
        for (s, (x, y, w, h)) in &ch.segs {
            let selected = *s == self.search.sorting;
            if selected {
                fill_rounded(pm, *x, *y, *w, *h, 6.0, color(255, 255, 255, 0.18));
            }
            let weight = if selected { Weight::Bold } else { Weight::Regular };
            let tw = self.measure(s.label(), 12.0, weight);
            self.text_at(pm, s.label(), x + (w - tw) / 2.0, y + h / 2.0, 12.0, weight, text_color());
        }
        if !self.recent_mode() {
            let (fx, fy, _, _) = ch.fit;
            let on = self.search.fit_screen;
            fill_rounded(pm, fx, fy, 16.0, 16.0, 4.0, if on { accent(1.0) } else { field_color() });
            if on {
                draw_check(pm, fx, fy, 16.0);
            }
            let label = self.fit_label();
            self.text_at(pm, &label, fx + 22.0, fy + 8.0, 12.0, Weight::Regular, text_color());
        }
        let status = self.status_text();
        let sw = self.measure(&status, 12.0, Weight::Regular);
        let x = (self.size.0 as f32 - 16.0 - sw).max(ch.fit.0 + ch.fit.2 + 16.0);
        self.text_at(pm, &status, x, TITLEBAR_H + TOOLBAR_H / 2.0, 12.0, Weight::Regular, subtext_color());
    }

    fn draw_grid(&mut self, pm: &mut Pixmap) {
        let g = self.grid();
        let lg = self.local_grid();
        let (lw, lh) = (g.w.max(1.0) as u32, g.h.max(1.0) as u32);
        let mut layer = match self.layer.take() {
            Some(l) if l.width() == lw && l.height() == lh => l,
            _ => match Pixmap::new(lw, lh) {
                Some(l) => l,
                None => return,
            },
        };
        layer.fill(Color::TRANSPARENT);

        let n = self.model.items.len();
        for i in lg.visible(self.scroll, n, 0) {
            let (cx, cy, cw, chh) = lg.cell_rect(i, self.scroll);
            if cy + chh < 0.0 || cy > lg.h {
                continue;
            }
            let item = self.model.items[i].clone();
            self.draw_cell(&mut layer, &item, (cx + CELL_MARGIN, cy + CELL_MARGIN, cw - 2.0 * CELL_MARGIN, chh - 2.0 * CELL_MARGIN), self.hover == Some(i));
        }
        if self.model.loading && n > 0 {
            let y = lg.rows(n) as f32 * lg.cell_h - self.scroll + 24.0;
            if y < lg.h + 12.0 {
                let tw = self.measure("Loading more…", 12.0, Weight::Regular);
                self.text_at(&mut layer, "Loading more…", (lg.w - tw) / 2.0, y, 12.0, Weight::Regular, subtext_color());
            }
        }
        pm.draw_pixmap(g.x as i32, g.y as i32, layer.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        self.layer = Some(layer);
    }

    fn draw_cell(&mut self, layer: &mut Pixmap, item: &Item, frame: Rect, hovered: bool) {
        let (x, y, w, h) = frame;
        fill_rounded(layer, x, y, w, h, CELL_RADIUS, field_color());
        // Local files are named `wallhaven-<id>`, remote items just `<id>`: compare the bare ids.
        let bare = |s: &str| s.trim_start_matches("wallhaven-").to_string();
        let applied = self.applied.as_deref().map(bare) == Some(bare(&item.id));
        let busy = self.applying.as_deref() == Some(item.id.as_str());
        let bw = if applied { 3.0 } else { 0.0 };
        if let Some(thumb) = self.thumbs.get(&item.id) {
            let zoom = if hovered { 1.04 } else { 1.0 };
            draw_cover(layer, thumb, (x + bw, y + bw, w - 2.0 * bw, h - 2.0 * bw), (CELL_RADIUS - bw).max(0.0), zoom);
        }
        if applied {
            stroke_rounded(layer, x + 1.5, y + 1.5, w - 3.0, h - 3.0, CELL_RADIUS - 1.5, accent(1.0), 3.0);
        }
        if busy {
            fill_rounded(layer, x, y, w, h, CELL_RADIUS, color(0, 0, 0, 0.25));
        }
        if hovered || busy {
            if let Some(p) = bottom_rounded(x, y + h - 26.0, w, 26.0, CELL_RADIUS) {
                let mut paint = Paint::default();
                paint.set_color(color(0, 0, 0, 0.55));
                paint.anti_alias = true;
                layer.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
            }
            let cy = y + h - 13.0;
            let left = if busy { "Setting wallpaper…".to_string() } else { item.resolution.clone() };
            self.text_at(layer, &left, x + 8.0, cy, 11.0, Weight::Regular, color(255, 255, 255, 1.0));
            if let (false, Some(fav)) = (busy, item.favorites) {
                let label = fav.to_string();
                let tw = self.measure(&label, 11.0, Weight::Regular);
                self.text_at(layer, &label, x + w - 8.0 - tw, cy, 11.0, Weight::Regular, color(255, 255, 255, 1.0));
                draw_heart(layer, x + w - 8.0 - tw - 14.0, cy - 4.5, 11.0, color(255, 255, 255, 1.0));
            }
        }
    }

    // ------------------------------------------------------------ input

    fn click(&mut self, x: f32, y: f32, button: Button) -> Response {
        let ch = self.chrome();
        if button == Button::Left {
            // Each light is 12 px; the clickable area is a little larger.
            let on_light = |r: Rect| inside((r.0 - 2.0, r.1 - 2.0, 16.0, 16.0), x, y);
            if on_light(ch.lights[0]) {
                return Response::act(Action::Close);
            }
            if on_light(ch.lights[1]) {
                return Response::act(Action::Minimize);
            }
            if on_light(ch.lights[2]) {
                return Response::act(Action::ToggleMaximize);
            }
            if inside(ch.search, x, y) {
                self.field.end();
                return Response::REDRAW;
            }
            for (s, r) in &ch.segs {
                if inside(*r, x, y) {
                    self.set_sorting(*s);
                    return Response::REDRAW;
                }
            }
            if !self.recent_mode() && inside((ch.fit.0, ch.fit.1 - 6.0, ch.fit.2, ch.fit.3 + 12.0), x, y) {
                self.search.fit_screen = !self.search.fit_screen;
                self.start_search();
                return Response::REDRAW;
            }
            if y < TITLEBAR_H {
                return Response::act(Action::Move);
            }
        }
        let g = self.grid();
        if let Some(i) = g.hit(x, y, self.scroll, self.model.items.len()) {
            match button {
                Button::Left => self.apply_item(i),
                Button::Right => self.save_item(i),
                Button::Middle => {}
            }
            return Response::REDRAW;
        }
        Response::NONE
    }
}

impl WindowApp for WallhavenApp {
    fn title(&self) -> String {
        "Wallpapers".into()
    }

    fn initial_size(&self) -> (u32, u32) {
        self.size
    }

    fn set_waker(&mut self, waker: Waker) {
        self.net = Some(Net::start(waker.clone()));
        self.waker = Some(waker);
        self.start_search();
        self.housekeeping();
    }

    fn set_screen(&mut self, w: u32, h: u32) {
        if self.screen == (0, 0) && w > 0 && h > 0 {
            self.screen = (w, h);
            self.search.screen = (w, h);
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w.max(320), h.max(240));
        self.clamp_scroll();
        self.housekeeping();
    }

    fn set_rounded(&mut self, rounded: bool) {
        self.rounded = rounded;
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        let (w, h) = (pm.width(), pm.height());
        if (w, h) != self.size {
            self.size = (w, h);
        }
        pm.fill(Color::TRANSPARENT);
        let radius = if self.rounded { WINDOW_RADIUS } else { 0.0 };
        fill_rounded(pm, 0.0, 0.0, w as f32, h as f32, radius, color(33, 33, 38, 0.97));

        let ch = self.chrome();
        self.draw_titlebar(pm, &ch);
        self.draw_toolbar(pm, &ch);
        let mut line = Paint::default();
        line.set_color(color(255, 255, 255, 0.08));
        if let Some(r) = tiny_skia::Rect::from_xywh(0.0, TITLEBAR_H + TOOLBAR_H, w as f32, 1.0) {
            pm.fill_rect(r, &line, Transform::identity(), None);
        }
        self.draw_grid(pm);

        if self.rounded {
            if let Some(m) = self.corner_mask(w, h, WINDOW_RADIUS as u32) {
                let m = m.clone();
                pm.apply_mask(&m);
            }
        }
        stroke_rounded(pm, 0.5, 0.5, w as f32 - 1.0, h as f32 - 1.0, (radius - 0.5).max(0.0), color(255, 255, 255, 0.12), 1.0);
        self.dirty = false;
        self.request_thumbs();
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        let g = self.grid();
        let hover = g.hit(x, y, self.scroll, self.model.items.len());
        if hover != self.hover {
            self.hover = hover;
            return Response::REDRAW;
        }
        Response::NONE
    }

    fn pointer_leave(&mut self) -> Response {
        if self.hover.take().is_some() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed {
            return Response::NONE;
        }
        self.click(x, y, button)
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> Response {
        let before = self.scroll;
        self.scroll += dy;
        self.clamp_scroll();
        self.housekeeping();
        let moved = (self.scroll - before).abs() > f32::EPSILON;
        if moved {
            self.hover = self.grid().hit(x, y, self.scroll, self.model.items.len());
        }
        if moved {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Escape => return Response::act(Action::Close),
            Key::Enter => self.submit_search(),
            Key::Backspace => self.field.backspace(),
            Key::Delete => self.field.delete(),
            Key::Left => self.field.left(),
            Key::Right => self.field.right(),
            Key::Home if ev.ctrl => self.scroll_rows(-10_000.0),
            Key::End if ev.ctrl => self.scroll_rows(10_000.0),
            Key::Home => self.field.home(),
            Key::End => self.field.end(),
            Key::Up => self.scroll_rows(-1.0),
            Key::Down => self.scroll_rows(1.0),
            Key::PageUp => self.scroll_rows(-3.0),
            Key::PageDown => self.scroll_rows(3.0),
            Key::Tab => return Response::NONE,
            Key::Char => {
                if ev.ctrl {
                    match ev.ch {
                        Some('a') => self.field.home(),
                        Some('e') => self.field.end(),
                        Some('u') => self.field.clear(),
                        _ => return Response::NONE,
                    }
                } else if !ev.alt {
                    self.field.insert(&ev.text);
                }
            }
        }
        Response::REDRAW
    }

    fn tick(&mut self) -> Response {
        let msgs = match &self.net {
            Some(n) => n.poll(),
            None => Vec::new(),
        };
        let mut redraw = false;
        let got_thumbs = msgs.iter().any(|m| matches!(m, Msg::Thumb { .. }));
        for m in msgs {
            redraw |= self.handle_msg(m);
        }
        // Give decode scratch memory back to the OS, at most every couple of seconds.
        if got_thumbs && self.pending.is_empty() && self.last_trim.elapsed() > std::time::Duration::from_secs(2) {
            crate::mem::trim();
            self.last_trim = std::time::Instant::now();
        }
        self.clamp_scroll();
        self.housekeeping();
        if redraw || self.dirty {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
}

// ---------------------------------------------------------------- drawing helpers

/// Draws `thumb` to cover `rect` (cropping the overflow) with rounded corners; `zoom` > 1 magnifies.
fn draw_cover(dst: &mut Pixmap, thumb: &Pixmap, rect: Rect, radius: f32, zoom: f32) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 || thumb.width() == 0 || thumb.height() == 0 {
        return;
    }
    let scale = (w / thumb.width() as f32).max(h / thumb.height() as f32) * zoom;
    let tx = x + (w - thumb.width() as f32 * scale) / 2.0;
    let ty = y + (h - thumb.height() as f32 * scale) / 2.0;
    let pattern = Pattern::new(thumb.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, Transform::from_scale(scale, scale).post_translate(tx, ty));
    let paint = Paint { shader: pattern, anti_alias: true, ..Paint::default() };
    if let Some(path) = rounded_rect(x, y, w, h, radius) {
        dst.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// A rectangle whose bottom corners are rounded (the hover caption bar).
fn bottom_rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.clamp(0.0, h.min(w) / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.close();
    pb.finish()
}

fn stroke_paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn draw_magnifier(pm: &mut Pixmap, x: f32, y: f32, size: f32, c: Color) {
    let s = size / 15.0;
    let stroke = Stroke { width: 1.6 * s, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
    if let Some(circle) = PathBuilder::from_circle(x + 6.4 * s, y + 6.4 * s, 4.7 * s) {
        pm.stroke_path(&circle, &stroke_paint(c), &stroke, Transform::identity(), None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x + 10.0 * s, y + 10.0 * s);
    pb.line_to(x + 13.4 * s, y + 13.4 * s);
    if let Some(p) = pb.finish() {
        pm.stroke_path(&p, &stroke_paint(c), &stroke, Transform::identity(), None);
    }
}

fn draw_check(pm: &mut Pixmap, x: f32, y: f32, size: f32) {
    let s = size / 16.0;
    let mut pb = PathBuilder::new();
    pb.move_to(x + 3.8 * s, y + 8.4 * s);
    pb.line_to(x + 6.8 * s, y + 11.4 * s);
    pb.line_to(x + 12.2 * s, y + 4.8 * s);
    let stroke = Stroke { width: 2.0 * s, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
    if let Some(p) = pb.finish() {
        pm.stroke_path(&p, &stroke_paint(color(255, 255, 255, 1.0)), &stroke, Transform::identity(), None);
    }
}

fn draw_heart(pm: &mut Pixmap, x: f32, y: f32, size: f32, c: Color) {
    let (w, h) = (size, size * 0.92);
    let p = |px: f32, py: f32| (x + px * w, y + py * h);
    let mut pb = PathBuilder::new();
    let (a, b) = p(0.5, 0.95);
    pb.move_to(a, b);
    for (c1, c2, to) in [
        ((0.10, 0.65), (0.0, 0.45), (0.0, 0.28)),
        ((0.0, 0.10), (0.14, 0.0), (0.28, 0.0)),
        ((0.40, 0.0), (0.48, 0.08), (0.5, 0.16)),
        ((0.52, 0.08), (0.60, 0.0), (0.72, 0.0)),
        ((0.86, 0.0), (1.0, 0.10), (1.0, 0.28)),
        ((1.0, 0.45), (0.90, 0.65), (0.5, 0.95)),
    ] {
        let (x1, y1) = p(c1.0, c1.1);
        let (x2, y2) = p(c2.0, c2.1);
        let (x3, y3) = p(to.0, to.1);
        pb.cubic_to(x1, y1, x2, y2, x3, y3);
    }
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &stroke_paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallhaven::api::Page;

    fn item(id: &str) -> Item {
        Item { id: id.into(), thumb: format!("https://th.wallhaven.cc/lg/{id}.jpg"), full: format!("https://w.wallhaven.cc/full/{id}.jpg"), resolution: "3840x2160".into(), favorites: Some(12) }
    }

    fn app_with(n: usize) -> WallhavenApp {
        let mut a = WallhavenApp::new("", false, Some((3440, 1440)));
        a.resize(1280, 820);
        let g = a.model.new_search();
        a.model.apply_page(g, 1, Ok(Page { items: (0..n).map(|i| item(&format!("id{i}"))).collect(), last_page: 1 }));
        a
    }

    fn key(k: Key, text: &str) -> KeyEvent {
        KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift: false, alt: false }
    }

    #[test]
    fn typing_enter_and_sorting_switch() {
        let mut a = app_with(3);
        for c in "cats".chars() {
            a.key(&key(Key::Char, &c.to_string()));
        }
        assert_eq!(a.field.text(), "cats");
        assert_eq!(a.search.sorting, Sorting::Toplist);
        a.key(&key(Key::Enter, ""));
        assert_eq!(a.search.sorting, Sorting::Relevance, "words leave the Top tab");
        assert_eq!(a.search.query, "cats");
        assert!(a.model.items.is_empty(), "a new search clears results");
    }

    #[test]
    fn escape_closes_and_ctrl_shortcuts_edit() {
        let mut a = app_with(1);
        assert_eq!(a.key(&key(Key::Escape, "")).action, Some(Action::Close));
        a.field.set("hello");
        let mut e = key(Key::Char, "u");
        e.ctrl = true;
        a.key(&e);
        assert_eq!(a.field.text(), "");
    }

    #[test]
    fn clicking_a_segment_changes_sorting() {
        let mut a = app_with(1);
        let ch = a.chrome();
        let (s, r) = ch.segs.iter().find(|(s, _)| *s == Sorting::Random).cloned().unwrap();
        a.pointer_button(r.0 + 5.0, r.1 + 5.0, Button::Left, true);
        assert_eq!(a.search.sorting, s);
        assert!(!a.search.seed.is_empty(), "random search gets a seed");
    }

    #[test]
    fn fit_toggle_flips_and_researches() {
        let mut a = app_with(1);
        let ch = a.chrome();
        assert!(a.search.fit_screen);
        a.pointer_button(ch.fit.0 + 3.0, ch.fit.1 + 3.0, Button::Left, true);
        assert!(!a.search.fit_screen);
    }

    #[test]
    fn traffic_lights_and_titlebar_drag() {
        let mut a = app_with(1);
        let ch = a.chrome();
        let c = |r: Rect| (r.0 + 6.0, r.1 + 6.0);
        let (x, y) = c(ch.lights[0]);
        assert_eq!(a.pointer_button(x, y, Button::Left, true).action, Some(Action::Close));
        let (x, y) = c(ch.lights[1]);
        assert_eq!(a.pointer_button(x, y, Button::Left, true).action, Some(Action::Minimize));
        let (x, y) = c(ch.lights[2]);
        assert_eq!(a.pointer_button(x, y, Button::Left, true).action, Some(Action::ToggleMaximize));
        assert_eq!(a.pointer_button(500.0, 20.0, Button::Left, true).action, Some(Action::Move));
    }

    #[test]
    fn wheel_scroll_is_clamped() {
        let mut a = app_with(40);
        assert_eq!(a.scroll(100.0, 300.0, -50.0), Response::NONE, "already at top");
        assert!(a.scroll(100.0, 300.0, 300.0).redraw);
        assert!(a.scroll > 0.0);
        a.scroll(100.0, 300.0, 1e7);
        let max = a.grid().max_scroll(40, false);
        assert_eq!(a.scroll, max);
    }

    #[test]
    fn click_on_cell_marks_it_busy() {
        let mut a = app_with(6);
        let g = a.grid();
        let (x, y, _, _) = g.cell_rect(1, 0.0);
        a.pointer_button(x + 20.0, y + 20.0, Button::Left, true);
        assert_eq!(a.applying.as_deref(), Some("id1"));
        // A second click while busy is ignored.
        let (x, y, _, _) = g.cell_rect(2, 0.0);
        a.pointer_button(x + 20.0, y + 20.0, Button::Left, true);
        assert_eq!(a.applying.as_deref(), Some("id1"));
    }

    #[test]
    fn apply_results_update_status() {
        let mut a = app_with(2);
        a.applying = Some("id0".into());
        assert!(a.handle_msg(Msg::Applied { id: "id0".into(), result: Ok("Wallpaper set".into()) }));
        assert_eq!(a.applied.as_deref(), Some("id0"));
        assert_eq!(a.status_text(), "Wallpaper set");
        a.handle_msg(Msg::Applied { id: "id1".into(), result: Err("Download failed".into()) });
        assert_eq!(a.applied, None);
        assert_eq!(a.status_text(), "Download failed");
    }

    #[test]
    fn status_text_matches_original_wording() {
        let mut a = app_with(5);
        assert_eq!(a.status_text(), "5 wallpapers");
        a.model.last_page = 3;
        assert_eq!(a.status_text(), "5 wallpapers · more below");
        a.model.loading = true;
        assert_eq!(a.status_text(), "5 wallpapers · loading more…");
        a.model.error = "Couldn't reach wallhaven.cc".into();
        assert_eq!(a.status_text(), "Couldn't reach wallhaven.cc");
    }

    #[test]
    fn failed_thumbs_are_not_requested_again() {
        let mut a = app_with(2);
        a.pending.insert("id0".into());
        a.handle_msg(Msg::Thumb { id: "id0".into(), pixmap: None });
        assert!(a.failed.contains("id0"));
        assert!(!a.pending.contains("id0"));
    }

    #[test]
    fn renders_chrome_and_a_thumbnail() {
        let mut a = app_with(8);
        let mut thumb = Pixmap::new(300, 200).unwrap();
        thumb.fill(Color::from_rgba8(30, 160, 60, 255));
        a.handle_msg(Msg::Thumb { id: "id0".into(), pixmap: Some(thumb) });
        let mut pm = Pixmap::new(1280, 820).unwrap();
        a.draw(&mut pm);
        let px = |x: u32, y: u32| pm.pixel(x, y).unwrap();
        assert_eq!(px(0, 0).alpha(), 0, "rounded corner is transparent");
        assert!(px(640, 400).alpha() > 200, "window body is opaque");
        let red = px(22, 26);
        assert!(red.red() > 200 && red.green() < 120, "red traffic light");
        let cell = a.grid().cell_rect(0, 0.0);
        let (cx, cy) = ((cell.0 + cell.2 / 2.0) as u32, (cell.1 + cell.3 / 2.0) as u32);
        let t = px(cx, cy);
        assert!(t.green() > t.red() && t.green() > 100, "thumbnail is drawn inside its cell");
    }

    #[test]
    fn draws_without_fonts_or_items() {
        let mut a = WallhavenApp::new("x", true, None);
        a.fonts = None;
        let mut pm = Pixmap::new(640, 480).unwrap();
        a.draw(&mut pm);
        assert_eq!(a.screen, (0, 0));
    }

    #[test]
    fn tiny_windows_do_not_panic() {
        let mut a = app_with(10);
        a.resize(10, 10);
        let mut pm = Pixmap::new(320, 240).unwrap();
        a.draw(&mut pm);
        a.scroll(10.0, 10.0, 100.0);
    }
}
