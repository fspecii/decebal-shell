//! The settings window: sidebar, content pane, popups, keyboard navigation and the glue between
//! pointer/keyboard events and the [`Model`].

use super::icons::pane_icon;
use super::io::{self, Ev, Paths};
use super::model::{CtlVal, Model};
use super::panes::PANES;
use super::schema::Schema;
use super::system::registry::{self, Hit, SECTIONS};
use super::ui::fill_px;
use super::ui::{self, draw_items, inside, pal, part_at, Frame, Interact, Item, Kind, Opt, Painter, Placed, Rect};
use crate::app::Button;
use crate::gfx::{color, fill_circle, fill_rounded, soft_shadow, stroke_rounded};
use crate::text::{Fonts, Weight};
use crate::wallhaven::cache::Lru;
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use tiny_skia::Pixmap;

const SIDEBAR_W: f32 = 236.0;
const MAX_THUMBS: usize = 32;
const SWATCHES: [&str; 18] = [
    "#ff453a", "#ff9f0a", "#ffd60a", "#30d158", "#63e6e2", "#0a84ff", "#5e5ce6", "#bf5af2", "#ff375f", "#ac8e68", "#ffffff", "#d1d1d6", "#8e8e93", "#636366", "#48484a", "#2c2c2e", "#1c1c1e",
    "#000000",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Search,
    Sidebar,
    Content,
}

struct Popup {
    id: String,
    opts: Vec<Opt>,
    sel: Option<usize>,
    hover: Option<usize>,
    rect: Rect,
    top: usize,
    visible: usize,
    typed: String,
}

struct ColorPop {
    id: String,
    rect: Rect,
    hex: TextField,
}

pub struct SettingsApp {
    m: Model,
    fonts: Option<Fonts>,
    size: (u32, u32),
    s: f32,
    frame: Frame,
    scroll: f32,
    it: Interact,
    zone: Zone,
    search: TextField,
    edit: Option<(String, TextField)>,
    popup: Option<Popup>,
    colorpop: Option<ColorPop>,
    thumbs: RefCell<Lru<String, Rc<Pixmap>>>,
    thumb_req: HashSet<String>,
    thumb_tx: Option<Sender<String>>,
    thumb_rx: Option<Receiver<(String, Option<Pixmap>)>>,
    waker: Option<Waker>,
    wake_slot: Arc<Mutex<Option<Waker>>>,
    sidebar_hits: Vec<(Rect, usize)>,
    /// Per entry of `sidebar_hits`: the setting a search hit points at.
    sidebar_labels: Vec<Option<String>>,
    /// Sidebar scroll offset (px, unscaled by nothing: already in pixels) and whether to keep the selection in view.
    sb_scroll: f32,
    sb_follow: bool,
    sb_total: f32,
    /// Search hits for `hits_for` (panes and individual settings).
    hits: Vec<Hit>,
    hits_for: String,
    /// Row labels of the older panes, built once per search session (building them per keystroke is slow).
    legacy_rows: Option<Vec<(usize, String)>>,
    search_rect: Rect,
    first_draw: bool,
    /// `DECSETTINGS_DEBUG_LAYOUT=file`: write the control map after every paint (end-to-end tests).
    dump: Option<std::path::PathBuf>,
    dump_meta: (Option<String>, Vec<String>),
    /// Paint statistics for the scroll benchmark: (frames, microseconds spent building items, total microseconds painting).
    stats: (u64, u64, u64),
    /// The items of the last paint, reused while the only thing that happened since is scrolling or hovering
    /// (building them reads theme folders, fonts and icon themes: ~27 ms for the Theme pane, per wheel notch).
    items_cache: Option<(usize, Vec<Item>)>,
    reuse_items: bool,
}

enum SbRow {
    Header(&'static str),
    Pane(usize),
    /// One setting inside a pane (search result).
    Hit(usize, String),
}

fn pane_index(id: &str) -> usize {
    PANES.iter().position(|p| p.id == id).unwrap_or(0)
}

impl SettingsApp {
    pub fn new(paths: Paths, start_pane: Option<&str>) -> SettingsApp {
        let schema = Schema::load(&paths.rdwmctl);
        let queue: io::Queue = Arc::new(Mutex::new(Default::default()));
        let wake_slot: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let slot = wake_slot.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(w) = slot.lock().unwrap().as_ref() {
                w();
            }
        });
        let mut m = Model::new(paths, schema, queue, wake);
        if let Some(p) = start_pane {
            // the old names of the panes that became Appearance tabs (theme, icons, wallpaper, desktop) open where they live now
            match super::appearance::resolve(p) {
                Some(v) => {
                    m.sel.pane = pane_index(v.pane().unwrap_or("appearance"));
                    if v.pane().is_none() {
                        super::appearance::show(&mut m, v);
                    }
                    m.sel.reset_scroll = false;
                }
                None => m.sel.pane = pane_index(p),
            }
        }
        let s = std::env::var("DECSETTINGS_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.5..=4.0).contains(v)).unwrap_or(1.0);
        SettingsApp {
            m,
            fonts: Fonts::load(),
            size: (940, 680),
            s,
            frame: Frame::default(),
            scroll: 0.0,
            it: Interact::default(),
            zone: Zone::Sidebar,
            search: TextField::default(),
            edit: None,
            popup: None,
            colorpop: None,
            thumbs: RefCell::new(Lru::new(MAX_THUMBS)),
            thumb_req: HashSet::new(),
            thumb_tx: None,
            thumb_rx: None,
            waker: None,
            wake_slot,
            sidebar_hits: Vec::new(),
            sidebar_labels: Vec::new(),
            sb_scroll: 0.0,
            sb_follow: true,
            sb_total: 0.0,
            hits: Vec::new(),
            hits_for: String::new(),
            legacy_rows: None,
            search_rect: (0.0, 0.0, 0.0, 0.0),
            first_draw: true,
            dump: std::env::var_os("DECSETTINGS_DEBUG_LAYOUT").map(Into::into),
            dump_meta: (None, Vec::new()),
            stats: (0, 0, 0),
            items_cache: None,
            reuse_items: false,
        }
    }

    pub fn model(&self) -> &Model {
        &self.m
    }

    fn sb(&self) -> f32 {
        (self.size.0 as f32 * 0.34).clamp(156.0 * self.s, SIDEBAR_W * self.s)
    }

    /// Search hits for the current query (recomputed only when the query changes).
    fn refresh_hits(&mut self) {
        let q = self.search.text().to_string();
        if q == self.hits_for {
            return;
        }
        self.hits_for = q.clone();
        // rows of the older (rdwm) panes are searched live, the system panes through the static index
        const LEGACY: &[&str] = &["general", "tiling", "windows", "rules", "keyboard", "autostart"];
        if q.trim().is_empty() {
            self.legacy_rows = None;
        } else if self.legacy_rows.is_none() {
            let mut extra: Vec<(usize, String)> = Vec::new();
            for (i, _) in PANES.iter().enumerate().filter(|(_, p)| LEGACY.contains(&p.id)) {
                for it in self.m.items(i) {
                    if let Item::Card(rows) = it {
                        extra.extend(rows.into_iter().filter(|r| !r.label.is_empty()).map(|r| (i, format!("{}\n{}", r.label, r.detail))));
                    }
                }
            }
            self.legacy_rows = Some(extra);
        }
        let extra = self.legacy_rows.clone().unwrap_or_default();
        self.hits = registry::search(&q, &extra);
        self.hits.truncate(60);
    }

    /// What the sidebar shows, top to bottom.
    fn sidebar_rows(&mut self) -> Vec<SbRow> {
        self.refresh_hits();
        let mut rows = Vec::new();
        if self.search.text().trim().is_empty() {
            for s in SECTIONS {
                rows.push(SbRow::Header(s.title));
                rows.extend(s.panes.iter().filter_map(|id| registry::pane_index(id)).map(SbRow::Pane));
            }
        } else {
            for h in &self.hits {
                rows.push(match &h.label {
                    None => SbRow::Pane(h.pane),
                    Some(l) => SbRow::Hit(h.pane, l.clone()),
                });
            }
        }
        rows
    }

    /// Panes in sidebar order (for keyboard navigation); with a search, only panes with a hit.
    fn visible_panes(&mut self) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        for r in self.sidebar_rows() {
            if let SbRow::Pane(i) | SbRow::Hit(i, _) = r {
                if !v.contains(&i) {
                    v.push(i);
                }
            }
        }
        v
    }

    fn select_pane(&mut self, i: usize) {
        if self.m.sel.pane != i {
            crate::log_info!("pane: {}", PANES.get(i).map(|p| p.id).unwrap_or("?"));
            self.m.sel.pane = i;
            self.sb_follow = true;
            self.scroll = 0.0;
            self.it.focus = None;
            self.edit = None;
            self.popup = None;
            self.colorpop = None;
            self.m.sel.recording = None;
            self.m.cfg.leave();
            if self.m.notice.as_ref().is_some_and(|n| n.error) {
                self.m.notice = None;
            }
        }
    }

    // ------------------------------------------------------------ thumbnails

    fn request_thumb(&mut self, id: &str) {
        if id.starts_with("lw:pick:") || id == "lw:motion" {
            if let Some(path) = self.m.lw.thumbnail_path(id).map(|p| p.to_string_lossy().into_owned()) {
                self.request_thumb(&path);
            }
            return;
        }
        if id.is_empty() || self.thumb_req.contains(id) || self.thumbs.borrow().contains(&id.to_string()) {
            return;
        }
        if let Some(tx) = &self.thumb_tx {
            self.thumb_req.insert(id.to_string());
            let _ = tx.send(id.to_string());
        }
    }

    fn start_thumb_worker(&mut self) {
        let (req_tx, req_rx) = channel::<String>();
        let (res_tx, res_rx) = channel::<(String, Option<Pixmap>)>();
        let waker = self.waker.clone();
        std::thread::Builder::new()
            .name("decsettings-thumbs".into())
            .stack_size(1 << 20)
            .spawn(move || {
                while let Ok(id) = req_rx.recv() {
                    let big = id.ends_with("#big");
                    let path = id.trim_end_matches("#big");
                    let px = load_thumb(std::path::Path::new(path), if big { (560, 350) } else { (176, 111) });
                    crate::mem::trim();
                    let _ = res_tx.send((id, px));
                    if let Some(w) = &waker {
                        w();
                    }
                }
            })
            .ok();
        self.thumb_tx = Some(req_tx);
        self.thumb_rx = Some(res_rx);
    }

    fn drain_thumbs(&mut self) -> bool {
        let mut any = false;
        if let Some(rx) = &self.thumb_rx {
            while let Ok((id, px)) = rx.try_recv() {
                self.thumb_req.remove(&id);
                // a failed decode caches a 1x1 transparent placeholder so it is not retried every frame
                let px = px.unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1"));
                self.thumbs.borrow_mut().insert(id, Rc::new(px));
                any = true;
            }
        }
        any
    }

    // ------------------------------------------------------------ drawing

    fn draw_sidebar(&mut self, pm: &mut Pixmap) {
        let s = self.s;
        let u = |v: f32| v * s;
        let sbw = self.sb();
        let h = pm.height() as f32;
        fill_px(pm, 0.0, 0.0, sbw, h, pal::sidebar());
        fill_px(pm, sbw - 1.0, 0.0, 1.0, h, pal::sep());
        let rows = self.sidebar_rows();
        let top = u(100.0);
        // layout pass: y of every row relative to the list top
        let heights: Vec<f32> = rows
            .iter()
            .enumerate()
            .map(|(n, r)| match r {
                SbRow::Header(_) => u(if n == 0 { 22.0 } else { 30.0 }),
                SbRow::Pane(_) => u(34.0),
                SbRow::Hit(..) => u(28.0),
            })
            .collect();
        let total: f32 = heights.iter().sum::<f32>() + u(10.0);
        self.sb_total = total;
        let view = (h - top).max(1.0);
        if self.sb_follow {
            self.sb_follow = false;
            let mut y = 0.0;
            for (r, hh) in rows.iter().zip(&heights) {
                if matches!(r, SbRow::Pane(i) if *i == self.m.sel.pane) {
                    if y < self.sb_scroll + u(30.0) {
                        self.sb_scroll = (y - u(30.0)).max(0.0);
                    } else if y + hh > self.sb_scroll + view {
                        self.sb_scroll = y + hh - view + u(6.0);
                    }
                    break;
                }
                y += hh;
            }
        }
        self.sb_scroll = self.sb_scroll.clamp(0.0, (total - view).max(0.0));
        let mut p = Painter { fonts: self.fonts.as_mut(), s };
        self.sidebar_hits.clear();
        self.sidebar_labels.clear();
        // Navigation groups share one rounded surface, mirroring the reference's inset lists.
        let mut gy = top - self.sb_scroll;
        let mut group: Option<(f32, f32)> = None;
        for (r, hh) in rows.iter().zip(&heights) {
            if matches!(r, SbRow::Header(_)) {
                if let Some((start, height)) = group.take() {
                    fill_rounded(pm, u(8.0), start, sbw - u(16.0), height, u(16.0), pal::card());
                }
            } else {
                let g = group.get_or_insert((gy, 0.0));
                g.1 += hh;
            }
            gy += hh;
        }
        if let Some((start, height)) = group {
            fill_rounded(pm, u(8.0), start, sbw - u(16.0), height, u(16.0), pal::card());
        }
        let mut y = top - self.sb_scroll;
        for (r, hh) in rows.iter().zip(&heights) {
            let ry = y;
            y += hh;
            match r {
                SbRow::Header(t) => {
                    if ry + hh > top && ry < h {
                        let label = p.fit(&t.to_uppercase(), sbw - u(32.0), u(10.5), Weight::SemiBold);
                        p.text(pm, &label, u(20.0), ry + hh - u(10.0), u(10.5), Weight::SemiBold, pal::sub());
                    }
                }
                SbRow::Pane(i) => {
                    let def = &PANES[*i];
                    let rect = (u(12.0), ry + u(2.0), sbw - u(24.0), hh - u(4.0));
                    let selected = self.m.sel.pane == *i;
                    let hover = self.it.hover.as_deref() == Some(&format!("pane:{i}"));
                    if ry + hh > top && ry < h {
                        if selected {
                            fill_rounded(pm, rect.0, rect.1, rect.2, rect.3, u(8.0), crate::theme::th().accent.color(if self.zone == Zone::Sidebar { 0.20 } else { 0.12 }));
                        } else if hover {
                            fill_rounded(pm, rect.0, rect.1, rect.2, rect.3, u(8.0), crate::theme::th().ink(0.06));
                        }
                        pane_icon(pm, def.glyph, def.color, rect.0 + u(8.0), rect.1 + (rect.3 - u(24.0)) / 2.0, u(24.0));
                        let label = p.fit(def.title, rect.2 - u(50.0), u(13.0), Weight::Regular);
                        p.text(pm, &label, rect.0 + u(42.0), rect.1 + rect.3 / 2.0, u(13.0), if selected { Weight::Medium } else { Weight::Regular }, pal::text());
                    }
                    self.sidebar_hits.push((rect, *i));
                    self.sidebar_labels.push(None);
                }
                SbRow::Hit(i, label) => {
                    let rect = (u(12.0), ry + u(2.0), sbw - u(24.0), hh - u(4.0));
                    if ry + hh > top && ry < h {
                        if self.it.hover.as_deref() == Some(&format!("pane:{i}")) {
                            fill_rounded(pm, rect.0, rect.1, rect.2, rect.3, u(8.0), crate::theme::th().ink(0.06));
                        }
                        let t = p.fit(&format!("{}  \u{203a}  {}", PANES[*i].title, label), rect.2 - u(52.0), u(12.0), Weight::Regular);
                        p.text(pm, &t, rect.0 + u(42.0), rect.1 + rect.3 / 2.0, u(12.0), Weight::Regular, pal::sub());
                    }
                    self.sidebar_hits.push((rect, *i));
                    self.sidebar_labels.push(Some(label.clone()));
                }
            }
        }
        if rows.is_empty() {
            p.text(pm, "No results", u(24.0), top + u(14.0), u(13.0), Weight::Regular, pal::faint());
        }
        // search field on top of the scrolled list
        fill_px(pm, 0.0, 0.0, sbw - 1.0, top, pal::sidebar());
        p.text(pm, "Settings", u(20.0), u(26.0), u(22.0), Weight::Bold, pal::text());
        let sr = (u(12.0), u(50.0), sbw - u(24.0), u(32.0));
        self.search_rect = sr;
        let focused = self.zone == Zone::Search;
        fill_rounded(pm, sr.0, sr.1, sr.2, sr.3, u(10.0), if focused { pal::window() } else { pal::field() });
        if focused {
            stroke_rounded(pm, sr.0, sr.1, sr.2, sr.3, u(8.0), pal::accent(), u(1.5));
        }
        // magnifier glyph
        let (mx, my) = (sr.0 + u(14.0), sr.1 + sr.3 / 2.0 - u(1.0));
        stroke_circle(pm, mx, my, u(4.5), pal::sub(), u(1.4));
        stroke_line(pm, mx + u(3.4), my + u(3.4), mx + u(7.0), my + u(7.0), pal::sub(), u(1.5));
        let text = self.search.text().to_string();
        if text.is_empty() {
            p.text(pm, "Search", sr.0 + u(28.0), sr.1 + sr.3 / 2.0, u(13.0), Weight::Regular, pal::faint());
        } else {
            let t = p.fit(&text, sr.2 - u(40.0), u(13.0), Weight::Regular);
            p.text(pm, &t, sr.0 + u(28.0), sr.1 + sr.3 / 2.0, u(13.0), Weight::Regular, pal::text());
        }
        if focused {
            let cx = sr.0 + u(28.0) + p.measure(self.search.before_cursor(), u(13.0), Weight::Regular);
            fill_px(pm, cx, sr.1 + u(7.0), u(1.2), sr.3 - u(14.0), pal::accent());
        }
        // sidebar scrollbar
        if total > h - top + 1.0 {
            let th = ((h - top) * (h - top) / total).max(30.0 * s);
            let ty = top + (h - top - th) * (self.sb_scroll / (total - (h - top)));
            fill_rounded(pm, sbw - 7.0 * s, ty, 3.0 * s, th, 1.5 * s, crate::theme::th().ink(0.2));
        }
    }

    fn draw_content(&mut self, pm: &mut Pixmap) {
        let s = self.s;
        let sbw = self.sb();
        let (w, h) = (pm.width() as f32 - sbw, pm.height() as f32);
        self.follow_requests();
        self.m.prepare(self.m.sel.pane);
        if self.m.wh.take_scroll_top() {
            self.scroll = 0.0;
        }
        let t_items = std::time::Instant::now();
        let pane = self.m.sel.pane;
        let reuse = std::mem::take(&mut self.reuse_items) && !self.m.dirty;
        let items = match self.items_cache.take() {
            Some((p, items)) if reuse && p == pane => items,
            _ => self.m.items(pane),
        };
        self.stats.1 += t_items.elapsed().as_micros() as u64;
        let title = PANES[self.m.sel.pane.min(PANES.len() - 1)].title;
        // sync interaction state
        self.it.edit = self.edit.as_ref().map(|(id, f)| (id.clone(), f.text().to_string(), f.cursor()));
        self.it.recording = self.m.sel.recording.as_ref().map(|r| (if r == "new" { "k:new:keys".to_string() } else { r.clone() }, self.m.sel.rec_mods.clone()));
        let max_scroll = (self.frame.height - h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let thumbs = &self.thumbs;
        let wh = &self.m.wh;
        let lw = &self.m.lw;
        let lookup = |id: &str| -> Option<Rc<Pixmap>> {
            if id == "lw:motion" {
                if let Some(frame) = lw.motion_frame() {
                    return Some(frame);
                }
            }
            let local = lw.thumbnail_path(id).map(|p| p.to_string_lossy().into_owned());
            let id = local.as_deref().unwrap_or(id);
            if id.starts_with("wh:") {
                return wh.thumb(id);
            }
            thumbs.borrow_mut().get(&id.to_string()).filter(|p| p.width() > 1).cloned()
        };
        let mut p = Painter { fonts: self.fonts.as_mut(), s };
        // Painted straight into the window buffer (the sidebar is drawn over the left edge afterwards):
        // a separate content layer would cost a second full-window pixmap.
        let mut frame = draw_items(pm, &mut p, title, &items, self.scroll.round(), sbw, w, &self.it, &lookup);
        // A shorter page than the last one (another tab, a collapsed list): the scroll was clamped with the old height,
        // so paint again at the right offset instead of leaving the page shifted until the next event.
        let max_now = (frame.height - h).max(0.0);
        if self.scroll > max_now + 0.5 {
            self.scroll = max_now;
            pm.fill(pal::window());
            frame = draw_items(pm, &mut p, title, &items, self.scroll.round(), sbw, w, &self.it, &lookup);
        }
        // lazy thumbnails: only what is visible (plus one row of margin); the big preview goes first
        let mut wanted: Vec<String> = Vec::new();
        for it in &items {
            if let Item::Preview { id, .. } = it {
                wanted.push(id.clone());
            }
        }
        for (r, id) in &frame.thumbs {
            if r.1 + r.3 > -r.3 && r.1 < h + r.3 {
                wanted.push(id.clone());
            }
        }
        self.frame = frame;
        super::configpane::sync_geometry(&mut self.m, &self.frame);
        self.dump_meta = (
            items.iter().find_map(|i| if let Item::Preview { id, .. } = i { Some(id.clone()) } else { None }),
            items.iter().filter_map(|i| if let Item::Note { text, warn: true } = i { Some(text.clone()) } else { None }).collect(),
        );
        let (online, local): (Vec<String>, Vec<String>) = wanted.into_iter().partition(|id| id.starts_with("wh:"));
        for id in local {
            self.request_thumb(&id);
        }
        if self.m.wallpaper_open() && self.m.wh.tab == super::wallhaven::Tab::Wallhaven {
            self.m.wh.set_visible(&online);
            super::wallhaven::on_paint(&mut self.m, self.frame.height, self.scroll, h);
        }
        self.items_cache = Some((pane, items));
        // scrollbar
        let total = self.frame.height;
        if total > h + 1.0 {
            let th = (h * h / total).max(30.0 * s);
            let ty = (h - th) * (self.scroll / (total - h));
            fill_rounded(pm, sbw + w - 7.0 * s, ty + 2.0 * s, 4.0 * s, th - 4.0 * s, 2.0 * s, crate::theme::th().ink(0.22));
        }
    }

    fn write_dump(&self) {
        let Some(path) = &self.dump else { return };
        use serde_json::json;
        let ctls: Vec<_> = self
            .frame
            .placed
            .iter()
            .map(|p| {
                let value = match &p.ctl.kind {
                    Kind::Toggle(b) => json!(b),
                    Kind::Slider { v, .. } | Kind::Stepper { v, .. } => json!(v),
                    Kind::Seg(o, s) | Kind::Popup(o, s) => json!(s.and_then(|i| o.get(i)).map(|x| x.1.clone())),
                    Kind::Color(t) | Kind::Text { val: t, .. } | Kind::Key(t) | Kind::Label(t) => json!(t),
                    Kind::Button { label, .. } | Kind::Chip { label, .. } => json!(label),
                };
                json!({"id": p.ctl.id, "label": p.label, "value": value, "enabled": p.ctl.enabled, "rect": [p.rect.0, p.rect.1, p.rect.2, p.rect.3], "parts": p.parts.iter().map(|r| json!([r.0, r.1, r.2, r.3])).collect::<Vec<_>>()})
            })
            .collect();
        let rects = |v: &[(Rect, String)]| v.iter().map(|(r, id)| json!({"id": id, "rect": [r.0, r.1, r.2, r.3]})).collect::<Vec<_>>();
        let (preview, warns) = self.dump_meta.clone();
        let doc = json!({
            "pane": PANES[self.m.sel.pane].id, "tab": self.m.sel.tab.id(), "section": self.m.sel.section.id(), "scroll": self.scroll, "ctls": ctls,
            "panes": self.sidebar_hits.iter().map(|(r, i)| json!({"id": PANES[*i].id, "rect": [r.0, r.1, r.2, r.3], "visible": r.1 >= 100.0 * self.s && r.1 + r.3 <= self.size.1 as f32})).collect::<Vec<_>>(),
            "hit_labels": self.sidebar_labels, "thumbs": rects(&self.frame.thumbs), "layouts": rects(&self.frame.layouts), "preview": preview, "warnings": warns,
            "notice": self.m.notice.as_ref().map(|n| n.text.clone()), "search": [self.search_rect.0, self.search_rect.1, self.search_rect.2, self.search_rect.3],
            "frames": self.stats.0, "items_us": self.stats.1, "paint_us": self.stats.2, "loaded_thumbs": self.thumbs.borrow().len(), "live_preview_frames": self.m.lw.preview_frames, "live_preview_active": self.m.lw.preview_active(), "wh_thumbs": self.m.wh.cached_thumbs(), "search_text": self.search.text(), "zone": format!("{:?}", self.zone), "focus": self.it.focus,
        });
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, doc.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    fn draw_toast(&mut self, pm: &mut Pixmap) {
        let Some(n) = self.m.notice.clone().filter(|n| !n.error) else { return };
        let s = self.s;
        let sbw = self.sb();
        let mut p = Painter { fonts: self.fonts.as_mut(), s };
        let w = p.measure(&n.text, 12.5 * s, Weight::Medium) + 44.0 * s;
        let (x, y) = (sbw + (pm.width() as f32 - sbw - w) / 2.0, pm.height() as f32 - 52.0 * s);
        soft_shadow(pm, x, y, w, 32.0 * s, 16.0 * s, 8.0 * s, 0.35);
        fill_rounded(pm, x, y, w, 32.0 * s, 16.0 * s, crate::theme::th().popup.color(0.98));
        fill_circle(pm, x + 16.0 * s, y + 16.0 * s, 4.0 * s, pal::green());
        p.text(pm, &n.text, x + 28.0 * s, y + 16.0 * s, 12.5 * s, Weight::Medium, pal::text());
    }

    fn draw_popup(&mut self, pm: &mut Pixmap) {
        let s = self.s;
        if let Some(pp) = &self.popup {
            let r = pp.rect;
            soft_shadow(pm, r.0, r.1, r.2, r.3, 8.0 * s, 10.0 * s, 0.45);
            fill_rounded(pm, r.0, r.1, r.2, r.3, 8.0 * s, crate::theme::th().popup.color(0.99));
            stroke_rounded(pm, r.0, r.1, r.2, r.3, 8.0 * s, crate::theme::th().ink(0.12), 1.0);
            let ih = 26.0 * s;
            let mut p = Painter { fonts: self.fonts.as_mut(), s };
            for (row, i) in (pp.top..(pp.top + pp.visible).min(pp.opts.len())).enumerate() {
                let y = r.1 + 4.0 * s + row as f32 * ih;
                if pp.hover == Some(i) {
                    fill_rounded(pm, r.0 + 4.0 * s, y, r.2 - 8.0 * s, ih, 5.0 * s, pal::accent());
                }
                if pp.sel == Some(i) {
                    let (cx, cy) = (r.0 + 14.0 * s, y + ih / 2.0);
                    stroke_line(pm, cx - 3.5 * s, cy, cx - 1.0 * s, cy + 3.0 * s, pal::text(), 1.6 * s);
                    stroke_line(pm, cx - 1.0 * s, cy + 3.0 * s, cx + 4.0 * s, cy - 3.5 * s, pal::text(), 1.6 * s);
                }
                let t = p.fit(&pp.opts[i].0, r.2 - 36.0 * s, 13.0 * s, Weight::Regular);
                p.text(pm, &t, r.0 + 26.0 * s, y + ih / 2.0, 13.0 * s, Weight::Regular, if pp.hover == Some(i) { pal::accent_text() } else { pal::text() });
            }
            if pp.opts.is_empty() {
                p.text(pm, "Nothing to choose", r.0 + 12.0 * s, r.1 + 18.0 * s, 12.5 * s, Weight::Regular, pal::faint());
            }
        }
        if let Some(cp) = &self.colorpop {
            let r = cp.rect;
            soft_shadow(pm, r.0, r.1, r.2, r.3, 10.0 * s, 10.0 * s, 0.45);
            fill_rounded(pm, r.0, r.1, r.2, r.3, 10.0 * s, crate::theme::th().popup.color(0.99));
            stroke_rounded(pm, r.0, r.1, r.2, r.3, 10.0 * s, crate::theme::th().ink(0.12), 1.0);
            for (i, hex) in SWATCHES.iter().enumerate() {
                let (c, rr) = (i % 6, i / 6);
                let (cx, cy) = (r.0 + 22.0 * s + c as f32 * 30.0 * s, r.1 + 22.0 * s + rr as f32 * 30.0 * s);
                let (cr, cg, cb) = ui::parse_hex(hex).unwrap_or((0, 0, 0));
                fill_circle(pm, cx, cy, 11.0 * s, color(cr, cg, cb, 1.0));
                stroke_rounded(pm, cx - 11.0 * s, cy - 11.0 * s, 22.0 * s, 22.0 * s, 11.0 * s, crate::theme::th().ink(0.3), 1.0);
            }
            let fy = r.1 + 22.0 * s + 3.0 * 30.0 * s - 6.0 * s;
            let fr = (r.0 + 10.0 * s, fy, r.2 - 20.0 * s - 70.0 * s, 26.0 * s);
            fill_rounded(pm, fr.0, fr.1, fr.2, fr.3, 6.0 * s, color(0, 0, 0, 0.35));
            stroke_rounded(pm, fr.0, fr.1, fr.2, fr.3, 6.0 * s, pal::accent(), 1.5);
            let mut p = Painter { fonts: self.fonts.as_mut(), s };
            p.text(pm, cp.hex.text(), fr.0 + 8.0 * s, fr.1 + fr.3 / 2.0, 12.5 * s, Weight::Regular, pal::text());
            let cx = fr.0 + 8.0 * s + p.measure(cp.hex.before_cursor(), 12.5 * s, Weight::Regular);
            fill_px(pm, cx, fr.1 + 6.0 * s, 1.2, fr.3 - 12.0 * s, pal::accent());
            let br = (fr.0 + fr.2 + 8.0 * s, fy, 52.0 * s, 26.0 * s);
            fill_rounded(pm, br.0, br.1, br.2, br.3, 6.0 * s, pal::field_hover());
            p.text_center(pm, "Default", br.0 + br.2 / 2.0, br.1 + br.3 / 2.0, 12.0 * s, Weight::Regular, pal::text());
        }
    }

    // ------------------------------------------------------------ helpers for events

    fn sidebar_hit(&self, x: f32, y: f32) -> Option<usize> {
        if y < 100.0 * self.s {
            return None;
        }
        self.sidebar_hits.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, i)| *i)
    }

    /// The search hit's setting under the pointer (None for plain pane rows).
    fn sidebar_label(&self, x: f32, y: f32) -> Option<String> {
        let i = self.sidebar_hits.iter().position(|(r, _)| inside(*r, x, y))?;
        self.sidebar_labels.get(i).cloned().flatten()
    }

    /// Opens pane `i`; a search hit for a setting of the Appearance tabs also opens the tab and section that hold it.
    fn open_pane(&mut self, i: usize, label: Option<&str>) {
        if PANES[i].id == "appearance" {
            if let Some(v) = label.and_then(super::appearance::view_of_label) {
                super::appearance::show(&mut self.m, v);
            }
        }
        self.select_pane(i);
    }

    /// Follows what the model asked for: another pane (a button in a tab) and a scroll back to the top (another tab).
    fn follow_requests(&mut self) {
        if let Some(i) = self.m.sel.goto.take() {
            self.select_pane(i);
        }
        if std::mem::take(&mut self.m.sel.reset_scroll) {
            self.scroll = 0.0;
            self.it.focus = None;
        }
    }

    fn to_content(&self, x: f32, y: f32) -> (f32, f32) {
        (x, y)
    }

    fn content_h(&self) -> f32 {
        self.size.1 as f32
    }

    fn open_popup(&mut self, p: &Placed) {
        let Kind::Popup(opts, sel) = &p.ctl.kind else { return };
        let s = self.s;
        let anchor = (p.rect.0, p.rect.1, p.rect.2, p.rect.3);
        let ih = 26.0 * s;
        let wmax = self.size.0 as f32;
        let hmax = (self.size.1 as f32 - 40.0 * s).min(380.0 * s);
        let visible = (((hmax - 8.0 * s) / ih).floor() as usize).clamp(1, opts.len().max(1));
        let h = visible as f32 * ih + 8.0 * s;
        let mut tw = anchor.2;
        if let Some(f) = &self.fonts {
            for (l, _) in opts.iter().take(200) {
                tw = tw.max(f.measure(l, 13.0 * s, Weight::Regular) + 48.0 * s);
            }
        }
        let w = tw.min(wmax - 20.0 * s);
        let mut x = anchor.0 + anchor.2 - w;
        x = x.clamp(8.0 * s, (wmax - w - 8.0 * s).max(8.0 * s));
        let mut y = anchor.1 + anchor.3 + 4.0 * s;
        if y + h > self.size.1 as f32 - 8.0 * s {
            y = (anchor.1 - h - 4.0 * s).max(8.0 * s);
        }
        let top = sel.map_or(0, |i| i.saturating_sub(visible / 2)).min(opts.len().saturating_sub(visible));
        self.popup = Some(Popup { id: p.ctl.id.clone(), opts: opts.clone(), sel: *sel, hover: *sel, rect: (x, y, w, h), top, visible, typed: String::new() });
    }

    fn open_color(&mut self, p: &Placed) {
        let Kind::Color(hex) = &p.ctl.kind else { return };
        let s = self.s;
        let (w, h) = (210.0 * s, 3.0 * 30.0 * s + 50.0 * s);
        let anchor = (p.rect.0, p.rect.1, p.rect.2, p.rect.3);
        let x = (anchor.0 + anchor.2 - w).clamp(8.0 * s, (self.size.0 as f32 - w - 8.0 * s).max(8.0 * s));
        let mut y = anchor.1 + anchor.3 + 4.0 * s;
        if y + h > self.size.1 as f32 - 8.0 * s {
            y = (anchor.1 - h - 4.0 * s).max(8.0 * s);
        }
        self.colorpop = Some(ColorPop { id: p.ctl.id.clone(), rect: (x, y, w, h), hex: TextField::new(hex) });
    }

    fn popup_choose(&mut self, i: usize) {
        if let Some(pp) = self.popup.take() {
            if let Some((_, v)) = pp.opts.get(i) {
                self.m.apply(&pp.id, CtlVal::Str(v.clone()));
            }
        }
    }

    fn commit_edit(&mut self) {
        if let Some((id, f)) = self.edit.take() {
            let before = self.frame.find(&id).and_then(|p| if let Kind::Text { val, .. } = &p.ctl.kind { Some(val.clone()) } else { None });
            // only write when the text really changed (leaving a field must not touch the file)
            if before.as_deref() != Some(f.text()) {
                self.m.apply(&id, CtlVal::Str(f.text().to_string()));
            }
        }
    }

    fn begin_edit(&mut self, p: &Placed) {
        if let Kind::Text { val, .. } = &p.ctl.kind {
            self.commit_edit();
            // password fields start empty (the stored text is never shown or copied)
            self.edit = Some((p.ctl.id.clone(), TextField::new(if p.ctl.id.contains(":secret") { "" } else { val })));
            self.it.focus = Some(p.ctl.id.clone());
        }
    }

    /// Runs the primary action of a control at pointer position `x` (content coordinates).
    fn click_ctl(&mut self, p: &Placed, x: f32, y: f32) {
        let id = p.ctl.id.clone();
        match &p.ctl.kind {
            Kind::Toggle(v) => self.m.apply(&id, CtlVal::Bool(!v)),
            Kind::Slider { min, max, step, .. } => {
                let track = p.parts.first().copied().unwrap_or(p.rect);
                let v = ui::slider_value(x, track, *min, *max, *step);
                self.it.drag = Some((id, v));
            }
            Kind::Stepper { v, min, max, step, .. } => {
                if let Some(i) = part_at(&p.parts, x, y) {
                    let nv = ui::snap(v + if i == 0 { -step } else { *step }, *min, *max, *step);
                    if (nv - v).abs() > 1e-9 {
                        self.m.apply(&id, CtlVal::Num(nv));
                    }
                }
            }
            Kind::Seg(opts, _) => {
                if let Some(i) = part_at(&p.parts, x, y) {
                    self.m.apply(&id, CtlVal::Str(opts[i].1.clone()));
                }
            }
            Kind::Popup(..) => self.open_popup(p),
            Kind::Color(_) => self.open_color(p),
            Kind::Text { .. } => self.begin_edit(p),
            Kind::Key(_) | Kind::Button { .. } | Kind::Chip { .. } => self.m.apply(&id, CtlVal::Click),
            Kind::Label(_) => {}
        }
    }

    fn ensure_visible(&mut self, id: &str) {
        if let Some(p) = self.frame.find(id) {
            let h = self.content_h();
            let (top, bot) = (p.row_rect.1, p.row_rect.1 + p.row_rect.3);
            let margin = 16.0 * self.s;
            if top < margin {
                self.scroll = (self.scroll + top - margin).max(0.0);
            } else if bot > h - margin {
                self.scroll += bot - (h - margin);
            }
        }
    }

    fn move_focus(&mut self, dir: i32) {
        let order = self.frame.focus_order();
        if order.is_empty() {
            self.zone = Zone::Sidebar;
            return;
        }
        let cur = self.it.focus.as_ref().and_then(|f| order.iter().position(|o| o == f));
        let next = match (cur, dir) {
            (None, d) if d >= 0 => Some(0),
            (None, _) => Some(order.len() - 1),
            (Some(i), d) if d > 0 => (i + 1 < order.len()).then_some(i + 1),
            (Some(i), _) => i.checked_sub(1),
        };
        match next {
            Some(i) => {
                self.zone = Zone::Content;
                self.it.focus = Some(order[i].clone());
                let id = order[i].clone();
                self.ensure_visible(&id);
            }
            None => {
                self.zone = if dir > 0 { Zone::Search } else { Zone::Sidebar };
                self.it.focus = None;
            }
        }
    }

    fn key_content(&mut self, ev: &KeyEvent) -> Response {
        let focus = self.it.focus.clone();
        let placed = focus.as_ref().and_then(|f| self.frame.find(f)).cloned();
        let ret = Response::REDRAW;
        match ev.key {
            Key::Tab => {
                self.move_focus(if ev.shift { -1 } else { 1 });
                return ret;
            }
            Key::Down | Key::Up if placed.as_ref().is_none_or(|p| !matches!(p.ctl.kind, Kind::Slider { .. } | Kind::Stepper { .. } | Kind::Seg(..)) || true) => {
                self.move_focus(if ev.key == Key::Down { 1 } else { -1 });
                return ret;
            }
            Key::PageDown => {
                self.scroll += self.content_h() * 0.8;
                return ret;
            }
            Key::PageUp => {
                self.scroll = (self.scroll - self.content_h() * 0.8).max(0.0);
                return ret;
            }
            Key::Home => {
                self.scroll = 0.0;
                return ret;
            }
            Key::End => {
                self.scroll = f32::MAX / 4.0;
                return ret;
            }
            _ => {}
        }
        let Some(p) = placed else {
            if matches!(ev.key, Key::Enter) || (ev.key == Key::Char && ev.ch == Some(' ')) {
                self.move_focus(1);
            }
            return ret;
        };
        let id = p.ctl.id.clone();
        let space = ev.key == Key::Char && ev.ch == Some(' ');
        let go = matches!(ev.key, Key::Enter) || space;
        let dir = match ev.key {
            Key::Left => -1,
            Key::Right => 1,
            _ => 0,
        };
        match &p.ctl.kind {
            Kind::Toggle(v) if go => self.m.apply(&id, CtlVal::Bool(!v)),
            Kind::Slider { v, min, max, step, .. } | Kind::Stepper { v, min, max, step, .. } if dir != 0 => {
                let big = if ev.shift { 5.0 } else { 1.0 };
                let nv = ui::snap(v + dir as f64 * step * big, *min, *max, *step);
                if (nv - v).abs() > 1e-9 {
                    self.m.apply(&id, CtlVal::Num(nv));
                }
            }
            Kind::Seg(opts, sel) if dir != 0 => {
                let n = opts.len() as i32;
                let cur = sel.map_or(if dir > 0 { -1 } else { n }, |s| s as i32);
                let i = (cur + dir).clamp(0, n - 1) as usize;
                self.m.apply(&id, CtlVal::Str(opts[i].1.clone()));
            }
            Kind::Popup(..) if go => self.open_popup(&p),
            Kind::Color(_) if go => self.open_color(&p),
            Kind::Text { .. } if go => self.begin_edit(&p),
            Kind::Key(_) | Kind::Button { .. } | Kind::Chip { .. } if go => self.m.apply(&id, CtlVal::Click),
            _ => {
                if ev.key == Key::Escape {
                    self.it.focus = None;
                }
            }
        }
        // thumbnails / layout tiles are focusable through the same ids
        ret
    }

    fn key_edit(&mut self, ev: &KeyEvent) -> Response {
        let Some((_, f)) = self.edit.as_mut() else { return Response::NONE };
        match ev.key {
            Key::Enter => self.commit_edit(),
            Key::Escape => self.edit = None,
            Key::Tab => {
                self.commit_edit();
                self.move_focus(if ev.shift { -1 } else { 1 });
            }
            Key::Left => f.left(),
            Key::Right => f.right(),
            Key::Home => f.home(),
            Key::End => f.end(),
            Key::Backspace => f.backspace(),
            Key::Delete => f.delete(),
            Key::Char if ev.ctrl && ev.ch == Some('a') => f.home(),
            Key::Char if ev.ctrl && ev.ch == Some('e') => f.end(),
            Key::Char if ev.ctrl && ev.ch == Some('u') => f.clear(),
            Key::Char if !ev.ctrl && !ev.alt => f.insert(&ev.text),
            _ => {}
        }
        Response::REDRAW
    }

    fn key_record(&mut self, ev: &KeyEvent) -> Response {
        if ev.key == Key::Escape {
            self.m.sel.recording = None;
            return Response::REDRAW;
        }
        let Some(name) = super::binds::key_name(ev) else { return Response::NONE };
        let mut mods: Vec<&'static str> = self.m.sel.rec_mods.clone();
        for (on, n) in [(ev.ctrl, "ctrl"), (ev.alt, "alt"), (ev.shift, "shift")] {
            if on && !mods.contains(&n) {
                mods.push(n);
            }
        }
        let spec = super::binds::build_spec(&mods, &name);
        let id = self.m.sel.recording.clone().unwrap_or_default();
        let target = if id == "new" { "k:new:keys".to_string() } else { id };
        self.m.apply(&target, CtlVal::Str(spec));
        self.m.sel.recording = None;
        Response::REDRAW
    }

    fn key_popup(&mut self, ev: &KeyEvent) -> Response {
        let Some(pp) = self.popup.as_mut() else { return Response::NONE };
        let n = pp.opts.len();
        let step = |pp: &mut Popup, d: i32| {
            if n == 0 {
                return;
            }
            let cur = pp.hover.map_or(if d > 0 { -1 } else { n as i32 }, |h| h as i32);
            let i = (cur + d).clamp(0, n as i32 - 1) as usize;
            pp.hover = Some(i);
            if i < pp.top {
                pp.top = i;
            } else if i >= pp.top + pp.visible {
                pp.top = i + 1 - pp.visible;
            }
        };
        match ev.key {
            Key::Escape => self.popup = None,
            Key::Down => step(pp, 1),
            Key::Up => step(pp, -1),
            Key::PageDown => step(pp, pp.visible as i32),
            Key::PageUp => step(pp, -(pp.visible as i32)),
            Key::Home => step(pp, -(n as i32)),
            Key::End => step(pp, n as i32),
            Key::Enter => {
                if let Some(h) = pp.hover {
                    self.popup_choose(h);
                } else {
                    self.popup = None;
                }
            }
            Key::Char if !ev.ctrl && !ev.alt => {
                // type-ahead
                pp.typed.push_str(&ev.text.to_lowercase());
                let typed = pp.typed.clone();
                if let Some(i) = pp.opts.iter().position(|o| o.0.to_lowercase().starts_with(&typed)).or_else(|| pp.opts.iter().position(|o| o.0.to_lowercase().contains(&typed))) {
                    pp.hover = Some(i);
                    pp.top = i.saturating_sub(pp.visible / 2).min(n.saturating_sub(pp.visible));
                }
            }
            Key::Backspace => {
                pp.typed.pop();
            }
            _ => {}
        }
        Response::REDRAW
    }

    fn key_color(&mut self, ev: &KeyEvent) -> Response {
        let Some(cp) = self.colorpop.as_mut() else { return Response::NONE };
        match ev.key {
            Key::Escape => self.colorpop = None,
            Key::Enter => {
                let (id, hex) = (cp.id.clone(), cp.hex.text().to_string());
                self.colorpop = None;
                self.m.apply(&id, CtlVal::Str(hex));
            }
            Key::Left => cp.hex.left(),
            Key::Right => cp.hex.right(),
            Key::Backspace => cp.hex.backspace(),
            Key::Delete => cp.hex.delete(),
            Key::Char if !ev.ctrl && !ev.alt => cp.hex.insert(&ev.text),
            _ => {}
        }
        Response::REDRAW
    }

    fn key_search(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Escape => {
                if self.search.text().is_empty() {
                    self.zone = Zone::Sidebar;
                } else {
                    self.search.clear();
                }
            }
            Key::Enter | Key::Down => {
                let first_hit = self.sidebar_rows().into_iter().find_map(|r| match r {
                    SbRow::Pane(i) => Some((i, None)),
                    SbRow::Hit(i, l) => Some((i, Some(l))),
                    SbRow::Header(_) => None,
                });
                if let Some((first, label)) = first_hit {
                    if !self.visible_panes().contains(&self.m.sel.pane) || ev.key == Key::Enter {
                        self.open_pane(first, label.as_deref());
                    }
                }
                self.zone = Zone::Sidebar;
            }
            Key::Tab => {
                if ev.shift {
                    self.zone = Zone::Content;
                    self.move_focus(-1);
                } else {
                    self.zone = Zone::Sidebar;
                }
            }
            Key::Left => self.search.left(),
            Key::Right => self.search.right(),
            Key::Home => self.search.home(),
            Key::End => self.search.end(),
            Key::Backspace => self.search.backspace(),
            Key::Delete => self.search.delete(),
            Key::Char if !ev.ctrl && !ev.alt => self.search.insert(&ev.text),
            _ => {}
        }
        // keep the selection on a visible pane
        let vis = self.visible_panes();
        if let Some(&first) = vis.first() {
            if !vis.contains(&self.m.sel.pane) {
                self.select_pane(first);
            }
        }
        Response::REDRAW
    }

    fn key_sidebar(&mut self, ev: &KeyEvent) -> Response {
        let vis = self.visible_panes();
        let pos = vis.iter().position(|&i| i == self.m.sel.pane).unwrap_or(0);
        match ev.key {
            Key::Down if pos + 1 < vis.len() => self.select_pane(vis[pos + 1]),
            Key::Up => {
                if pos > 0 {
                    self.select_pane(vis[pos - 1]);
                } else {
                    self.zone = Zone::Search;
                }
            }
            Key::Home => self.select_pane(vis[0]),
            Key::End => self.select_pane(*vis.last().unwrap_or(&0)),
            Key::Tab | Key::Right | Key::Enter => {
                if ev.shift && ev.key == Key::Tab {
                    self.zone = Zone::Search;
                } else {
                    self.zone = Zone::Content;
                    self.it.focus = None;
                    self.move_focus(1);
                }
            }
            Key::Char if !ev.ctrl && !ev.alt => {
                self.zone = Zone::Search;
                self.search.insert(&ev.text);
            }
            _ => {}
        }
        Response::REDRAW
    }
}

fn stroke_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: tiny_skia::Color, w: f32) {
    stroke_rounded(pm, cx - r, cy - r, 2.0 * r, 2.0 * r, r, c, w);
}

pub(super) fn stroke_line(pm: &mut Pixmap, x0: f32, y0: f32, x1: f32, y1: f32, c: tiny_skia::Color, w: f32) {
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x0, y0);
    pb.line_to(x1, y1);
    if let Some(path) = pb.finish() {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        pm.stroke_path(&path, &paint, &tiny_skia::Stroke { width: w, line_cap: tiny_skia::LineCap::Round, ..Default::default() }, tiny_skia::Transform::identity(), None);
    }
}

/// Decodes `path` and returns a centre-cropped `size` thumbnail as premultiplied RGBA.
pub fn load_thumb(path: &std::path::Path, size: (u32, u32)) -> Option<Pixmap> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 80 * 1024 * 1024 {
        return None;
    }
    let mut reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let small = img.resize_to_fill(size.0, size.1, image::imageops::FilterType::Triangle).to_rgba8();
    drop(img);
    let mut pm = Pixmap::new(small.width(), small.height())?;
    for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(small.pixels()) {
        let a = s.0[3] as u32;
        d[0] = (s.0[0] as u32 * a / 255) as u8;
        d[1] = (s.0[1] as u32 * a / 255) as u8;
        d[2] = (s.0[2] as u32 * a / 255) as u8;
        d[3] = a as u8;
    }
    Some(pm)
}

impl WindowApp for SettingsApp {
    fn title(&self) -> String {
        "System Settings".into()
    }

    fn app_id(&self) -> &'static str {
        "DecSettings"
    }

    fn initial_size(&self) -> (u32, u32) {
        ((940.0 * self.s) as u32, (680.0 * self.s) as u32)
    }

    fn set_waker(&mut self, waker: Waker) {
        *self.wake_slot.lock().unwrap() = Some(waker.clone());
        self.waker = Some(waker.clone());
        self.start_thumb_worker();
        let tw = waker.clone();
        crate::theme::live::start_watcher(move || tw());
        let files = self.m.watched_files();
        io::spawn_watcher(files, self.m.paths.socket.clone(), self.m.queue.clone(), waker);
    }

    fn set_visible(&mut self, visible: bool) {
        self.m.sys_visible(visible);
    }

    fn set_screen(&mut self, w: u32, h: u32) {
        self.m.wh.screen = (w, h);
    }

    fn set_scale(&mut self, scale: f32) {
        if std::env::var_os("DECSETTINGS_SCALE").is_none() && (0.5..=4.0).contains(&scale) {
            self.s = scale;
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w, h);
    }

    fn set_rounded(&mut self, _rounded: bool) {}

    fn draw(&mut self, pm: &mut Pixmap) {
        let t0 = std::time::Instant::now();
        pm.fill(pal::window());
        self.size = (pm.width(), pm.height());
        self.draw_content(pm);
        self.draw_sidebar(pm);
        self.write_dump();
        self.draw_toast(pm);
        self.draw_popup(pm);
        if let Some(f) = &self.fonts {
            f.trim();
        }
        self.m.dirty = false;
        self.first_draw = false;
        self.stats.0 += 1;
        self.stats.2 += t0.elapsed().as_micros() as u64;
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        let before = self.it.hover.clone();
        // popups first
        if let Some(pp) = self.popup.as_mut() {
            let r = pp.rect;
            let old = pp.hover;
            if inside(r, x, y) {
                let ih = 26.0 * self.s;
                let i = pp.top + ((y - r.1 - 4.0 * self.s) / ih).floor().max(0.0) as usize;
                pp.hover = (i < pp.opts.len()).then_some(i);
            }
            return if old != pp.hover { Response::REDRAW } else { Response::NONE };
        }
        if self.colorpop.is_some() {
            return Response::NONE;
        }
        if self.m.cfg.drag {
            let p = Painter { fonts: self.fonts.as_mut(), s: self.s };
            if super::configpane::pointer_drag(&mut self.m, &self.frame, &p, x, y) {
                return Response::REDRAW;
            }
        }
        let (cx, cy) = self.to_content(x, y);
        if let Some((id, _)) = self.it.drag.clone() {
            if let Some(p) = self.frame.find(&id).cloned() {
                if let Kind::Slider { min, max, step, .. } = p.ctl.kind {
                    let track = p.parts.first().copied().unwrap_or(p.rect);
                    self.it.drag = Some((id, ui::slider_value(cx, track, min, max, step)));
                    self.reuse_items = true;
                    return Response::REDRAW;
                }
            }
        }
        self.it.hover = if x < self.sb() {
            self.sidebar_hit(x, y).map(|i| format!("pane:{i}"))
        } else {
            self.frame.at(cx, cy).map(|p| p.ctl.id.clone()).or_else(|| self.frame.thumbs.iter().chain(self.frame.layouts.iter()).find(|(r, _)| inside(*r, cx, cy)).map(|(_, id)| id.clone()))
        };
        if before != self.it.hover || self.m.dirty {
            self.reuse_items = !self.m.dirty;
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn pointer_leave(&mut self) -> Response {
        if self.it.hover.take().is_some() {
            self.reuse_items = true;
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        self.reuse_items = false;
        self.it.ring = false;
        if !pressed {
            super::configpane::pointer_release(&mut self.m);
            if let Some((id, v)) = self.it.drag.take() {
                self.m.apply(&id, CtlVal::Num(v));
                return Response::REDRAW;
            }
            return Response::NONE;
        }
        // overlays swallow the click
        if let Some(pp) = &self.popup {
            if inside(pp.rect, x, y) && button == Button::Left {
                let ih = 26.0 * self.s;
                let i = pp.top + ((y - pp.rect.1 - 4.0 * self.s) / ih).floor().max(0.0) as usize;
                if i < pp.opts.len() {
                    self.popup_choose(i);
                    return Response::REDRAW;
                }
            }
            self.popup = None;
            return Response::REDRAW;
        }
        if let Some(cp) = &self.colorpop {
            let s = self.s;
            let r = cp.rect;
            let id = cp.id.clone();
            if inside(r, x, y) && button == Button::Left {
                for (i, hex) in SWATCHES.iter().enumerate() {
                    let (c, rr) = (i % 6, i / 6);
                    let (px, py) = (r.0 + 22.0 * s + c as f32 * 30.0 * s, r.1 + 22.0 * s + rr as f32 * 30.0 * s);
                    if (x - px).hypot(y - py) <= 13.0 * s {
                        self.colorpop = None;
                        self.m.apply(&id, CtlVal::Str(hex.to_string()));
                        return Response::REDRAW;
                    }
                }
                let fy = r.1 + 22.0 * s + 3.0 * 30.0 * s - 6.0 * s;
                let br = (r.0 + 10.0 * s + r.2 - 20.0 * s - 70.0 * s + 8.0 * s, fy, 52.0 * s, 26.0 * s);
                if inside(br, x, y) {
                    self.colorpop = None;
                    self.m.apply(&id, CtlVal::Reset);
                }
                return Response::REDRAW;
            }
            self.colorpop = None;
            return Response::REDRAW;
        }
        if self.m.sel.recording.is_some() && !self.frame.at(x, y).is_some_and(|p| p.ctl.id.starts_with("mod:")) {
            self.m.sel.recording = None;
        }
        // sidebar
        if x < self.sb() {
            self.commit_edit();
            if inside(self.search_rect, x, y) {
                self.zone = Zone::Search;
                return Response::REDRAW;
            }
            if let Some(i) = self.sidebar_hit(x, y) {
                self.zone = Zone::Sidebar;
                let label = self.sidebar_label(x, y);
                self.open_pane(i, label.as_deref());
            }
            return Response::REDRAW;
        }
        let (cx, cy) = self.to_content(x, y);
        if button == Button::Right {
            if let Some(p) = self.frame.at(cx, cy).cloned() {
                if p.ctl.id.starts_with("r:") || p.ctl.id.starts_with("j:") {
                    self.m.apply(&p.ctl.id, CtlVal::Reset);
                }
            }
            return Response::REDRAW;
        }
        if button != Button::Left {
            return Response::NONE;
        }
        self.zone = Zone::Content;
        if PANES[self.m.sel.pane].id == "config" {
            let p = Painter { fonts: self.fonts.as_mut(), s: self.s };
            if super::configpane::pointer_press(&mut self.m, &self.frame, &p, cx, cy) {
                self.commit_edit();
                self.it.focus = Some(super::configpane::EDITOR_ID.to_string());
                return Response::REDRAW;
            }
        }
        if let Some(p) = self.frame.at(cx, cy).cloned() {
            if !matches!(p.ctl.kind, Kind::Text { .. }) {
                self.commit_edit();
            }
            if self.edit.as_ref().is_some_and(|(id, _)| *id == p.ctl.id) {
                return Response::REDRAW; // click inside the field being edited
            }
            self.it.focus = Some(p.ctl.id.clone());
            self.click_ctl(&p, cx, cy);
            return Response::REDRAW;
        }
        if let Some((_, id)) = self.frame.thumbs.iter().chain(self.frame.layouts.iter()).find(|(r, _)| inside(*r, cx, cy)).cloned() {
            self.commit_edit();
            self.it.focus = Some(id.clone());
            // thumbnails carry the bare file path; layout tiles already are `l:set:<name>`
            let target = if id.starts_with("l:") { id } else { format!("w:{id}") };
            self.m.apply(&target, CtlVal::Click);
            return Response::REDRAW;
        }
        self.commit_edit();
        self.it.focus = None;
        Response::REDRAW
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> Response {
        if let Some(pp) = self.popup.as_mut() {
            let n = pp.opts.len();
            let d = if dy > 0.0 { 3 } else { -3 };
            pp.top = (pp.top as i32 + d).clamp(0, n.saturating_sub(pp.visible) as i32) as usize;
            return Response::REDRAW;
        }
        if x < self.sb() {
            let max = (self.sb_total - (self.size.1 as f32 - 100.0 * self.s)).max(0.0);
            let old = self.sb_scroll;
            self.sb_scroll = (self.sb_scroll + dy * self.s * 0.6).clamp(0.0, max);
            return if (old - self.sb_scroll).abs() > 0.1 { Response::REDRAW } else { Response::NONE };
        }
        if PANES[self.m.sel.pane].id == "config" && super::configpane::wheel(&mut self.m, &self.frame, x, y, dy) {
            return Response::REDRAW;
        }
        let max = (self.frame.height - self.content_h()).max(0.0);
        let old = self.scroll;
        self.scroll = (self.scroll + dy * self.s * 0.6).clamp(0.0, max);
        let _ = y;
        if (old - self.scroll).abs() > 0.1 {
            // pointer is over something else now
            self.reuse_items = true;
            Response { redraw: true, action: None }
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        self.it.ring = true;
        self.reuse_items = false;
        if ev.ctrl && ev.key == Key::Char && ev.ch == Some('w') {
            return Response::act(Action::Close);
        }
        if ev.ctrl && ev.key == Key::Char && ev.ch == Some('f') {
            self.commit_edit();
            self.zone = Zone::Search;
            return Response::REDRAW;
        }
        if self.m.sel.recording.is_some() {
            return self.key_record(ev);
        }
        if self.colorpop.is_some() {
            return self.key_color(ev);
        }
        if self.popup.is_some() {
            return self.key_popup(ev);
        }
        if self.m.cfg.active && PANES[self.m.sel.pane].id == "config" {
            return super::configpane::editor_key(&mut self.m, ev);
        }
        if self.edit.is_some() {
            return self.key_edit(ev);
        }
        if ev.key == Key::Escape {
            return match self.zone {
                Zone::Search if !self.search.text().is_empty() => self.key_search(ev),
                Zone::Content if self.it.focus.is_some() => {
                    self.it.focus = None;
                    self.zone = Zone::Sidebar;
                    Response::REDRAW
                }
                _ => Response::act(Action::Close),
            };
        }
        match self.zone {
            Zone::Search => self.key_search(ev),
            Zone::Sidebar => self.key_sidebar(ev),
            Zone::Content => self.key_content(ev),
        }
    }

    fn unmapped_key(&mut self, sym: u32, shift: bool, ctrl: bool, alt: bool) -> Response {
        self.reuse_items = false;
        // function keys (F1..F35) can be recorded as shortcuts
        if self.m.sel.recording.is_some() && (0xffbe..=0xffe0).contains(&sym) {
            let ev_mods = (ctrl, alt, shift);
            let name = format!("F{}", sym - 0xffbd);
            let mut mods: Vec<&'static str> = self.m.sel.rec_mods.clone();
            for (on, n) in [(ev_mods.0, "ctrl"), (ev_mods.1, "alt"), (ev_mods.2, "shift")] {
                if on && !mods.contains(&n) {
                    mods.push(n);
                }
            }
            let spec = super::binds::build_spec(&mods, &name);
            let id = self.m.sel.recording.take().unwrap_or_default();
            self.m.apply(&if id == "new" { "k:new:keys".to_string() } else { id }, CtlVal::Str(spec));
            return Response::REDRAW;
        }
        Response::NONE
    }

    fn tick(&mut self) -> Response {
        let mut redraw = self.drain_thumbs();
        loop {
            let ev = self.m.queue.lock().unwrap().pop_front();
            match ev {
                Some(ev) => {
                    let ev: Ev = ev;
                    self.m.on_event(ev);
                    redraw = true;
                }
                None => break,
            }
        }
        redraw |= self.m.sys_poll();
        redraw |= super::wallhaven::pump(&mut self.m);
        redraw |= crate::livewall::pane::pump(&mut self.m);
        redraw |= crate::theme::engine::preview::take_changed();
        redraw |= super::configpane::poll(&mut self.m);
        redraw |= self.m.expire_notice();
        if crate::theme::take_changed() {
            self.m.dirty = true; // the theme pane and every colour follow the file
        }
        redraw |= self.m.dirty;
        if self.m.sel.goto.is_some() {
            self.follow_requests();
            redraw = true;
        }
        if redraw {
            self.reuse_items = false; // something other than the pointer or the wheel changed: rebuild the page
        }
        Response { redraw, action: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_decode_and_crop() {
        let d = std::env::temp_dir().join(format!("decsettings-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("a.png");
        let mut img = image::RgbaImage::new(400, 100);
        for px in img.pixels_mut() {
            *px = image::Rgba([200, 10, 10, 255]);
        }
        img.save(&p).unwrap();
        let t = load_thumb(&p, (176, 111)).expect("decodes");
        assert_eq!((t.width(), t.height()), (176, 111));
        assert!(t.pixel(100, 60).unwrap().red() > 150);
        assert!(load_thumb(&d.join("missing.png"), (10, 10)).is_none());
        std::fs::write(d.join("junk.png"), b"not an image").unwrap();
        assert!(load_thumb(&d.join("junk.png"), (10, 10)).is_none());
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    fn app() -> SettingsApp {
        let d = std::env::temp_dir().join(format!("decsettings-app-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent".into(), socket: d.join("none.sock") };
        SettingsApp::new(paths, None)
    }

    #[test]
    fn scrolled_theme_apply_shows_widget_choice_and_replaces_layout() {
        let initial = app();
        let mut a = SettingsApp::new(initial.m.paths.clone(), Some("appearance"));
        super::super::io::json_set(&a.m.paths.shell_json(), "widgets", Some(serde_json::json!({"enabled":true,"items":[{"use":"builtin:clock-card","x":345}]}))).unwrap();
        a.resize(1200, 900);
        let mut pixels = Pixmap::new(1200, 900).unwrap();
        let id = "l:tcard:apply:glyph-rice";
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            a.reuse_items = false;
            a.draw(&mut pixels);
            if a.frame.find(id).is_some() || std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let p = a.frame.find(id).expect("theme apply control after asynchronous discovery").rect;
        a.scroll = (p.1 - 300.0).max(0.0);
        a.draw(&mut pixels);
        let p = a.frame.find(id).unwrap().rect;
        assert!(p.1 >= 0.0 && p.1 + p.3 < 900.0);
        a.pointer_button(p.0 + p.2 / 2.0, p.1 + p.3 / 2.0, Button::Left, true);
        a.draw(&mut pixels);
        assert!(a.m.widget_choice.is_some(), "apply must request widget-layout choice");
        let p = a.frame.find("wc:replace").expect("visible widget choice").rect;
        assert!(p.1 >= 0.0 && p.1 + p.3 < 900.0, "choice must be on screen: {p:?}");
        a.pointer_button(p.0 + p.2 / 2.0, p.1 + p.3 / 2.0, Button::Left, true);
        a.draw(&mut pixels);
        assert!(a.m.widget_choice.is_none());
        let shell = super::super::io::json_load(&a.m.paths.shell_json());
        assert_eq!(shell["theme"]["name"], "glyph-rice");
    }

    /// Drives the Widgets page like a user: open the form, edit the name, create, and check the saved widget and layout.
    #[test]
    fn create_widget_form_works_through_real_controls() {
        let initial = app();
        let sandbox = super::super::widgetbuilder::test_support::sandbox(&initial.m.paths.shell_dir);
        let mut a = SettingsApp::new(initial.m.paths.clone(), Some("widgets"));
        let existing = serde_json::json!({"use":"builtin:clock-card","x":-12,"future":true});
        super::super::io::json_set(&a.m.paths.shell_json(), "widgets", Some(serde_json::json!({"enabled":true,"future":1,"items":[existing.clone()]}))).unwrap();
        a.resize(1100, 1000);
        let mut pixels = Pixmap::new(1100, 1000).unwrap();
        let draw = |a: &mut SettingsApp, pixels: &mut Pixmap| {
            a.reuse_items = false;
            a.draw(pixels);
        };
        let center = |a: &mut SettingsApp, pixels: &mut Pixmap, id: &str| {
            draw(a, pixels);
            let mut r = a.frame.find(id).unwrap_or_else(|| panic!("{id} is on the page")).rect;
            if r.1 < 0.0 || r.1 + r.3 > 1000.0 {
                a.scroll = (a.scroll + r.1 - 300.0).max(0.0);
                draw(a, pixels);
                r = a.frame.find(id).unwrap().rect;
            }
            assert!(r.1 >= 0.0 && r.1 + r.3 <= 1000.0, "{id} must be reachable: {r:?}");
            (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0)
        };
        let (x, y) = center(&mut a, &mut pixels, "wg:new:open");
        a.pointer_button(x, y, Button::Left, true);
        let (x, y) = center(&mut a, &mut pixels, "wg:new:name");
        a.pointer_button(x, y, Button::Left, true);
        assert!(a.edit.is_some(), "the name field starts editing");
        a.key(&ev(Key::Char, Some('!')));
        a.key(&ev(Key::Enter, None));
        assert_eq!(a.m.widget_builder.as_ref().map(|d| d.name.as_str()), Some("My clock!"));
        draw(&mut a, &mut pixels);
        assert!(a.items_cache.as_ref().is_some_and(|(_, items)| items.iter().any(|i| matches!(i, Item::WidgetPreview { .. }))), "the preview is part of the page");
        if let Some(dir) = std::env::var_os("WIDGET_EVIDENCE_DIR") {
            // Set WIDGET_EVIDENCE_DIR to keep a picture of the form for visual review.
            std::fs::create_dir_all(&dir).unwrap();
            a.scroll = 0.0;
            draw(&mut a, &mut pixels);
            pixels.save_png(std::path::Path::new(&dir).join("create-widget-form.png")).unwrap();
        }
        for id in ["wg:new:template", "wg:new:size", "wg:new:anchor", "wg:new:x", "wg:new:y", "wg:new:cancel"] {
            assert!(a.frame.find(id).is_some() || center(&mut a, &mut pixels, id).0 > 0.0, "{id}");
        }
        let (x, y) = center(&mut a, &mut pixels, "wg:new:create");
        a.pointer_button(x, y, Button::Left, true);
        draw(&mut a, &mut pixels);
        assert!(a.m.widget_builder.is_none(), "{:?}", a.m.notice.as_ref().map(|n| &n.text));
        let file = sandbox.dir.join("my-clock.toml");
        assert!(crate::widgets::dsl::parse(&std::fs::read_to_string(&file).unwrap(), crate::widgets::Trust::User).is_ok());
        let widgets = super::super::io::json_load(&a.m.paths.shell_json())["widgets"].clone();
        assert_eq!(widgets["future"], 1);
        assert_eq!(widgets["items"][0], existing);
        assert_eq!((widgets["items"][1]["use"].as_str(), widgets["items"][1]["enabled"].as_bool().unwrap_or(true)), (Some("my-clock"), true));
        assert!(a.frame.find("wg:new:open").is_some(), "the form is closed again");
    }

    fn ev(key: Key, ch: Option<char>) -> KeyEvent {
        KeyEvent { key, text: ch.map(String::from).unwrap_or_default(), ch, ctrl: false, shift: false, alt: false }
    }

    #[test]
    #[ignore = "run headlessly with private config and DECEBAL_DECORATION_EVIDENCE"]
    fn native_decoration_picker_evidence() {
        assert_eq!(std::env::var("DECEBAL_SYSTEM_AUTOMATIC").as_deref(), Ok("0"));
        let initial = app();
        let mut a = SettingsApp::new(initial.m.paths.clone(), Some("appearance/window"));
        a.m.apply("r:decoration:style", super::super::model::CtlVal::Str("adwaita".into()));
        a.resize(1200, 900);
        let mut pixels = Pixmap::new(1200, 900).unwrap();
        a.draw(&mut pixels);
        let control = a.frame.placed.iter().find(|p| p.ctl.id == "r:decoration:style").expect("design picker present").clone();
        a.open_popup(&control);
        assert_eq!(a.popup.as_ref().unwrap().opts.len(), 14);
        a.draw(&mut pixels);
        if let Some(dir) = std::env::var_os("DECEBAL_DECORATION_EVIDENCE") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            pixels.save_png(dir.join(if crate::theme::th().dark { "picker-dark.png" } else { "picker-light.png" })).unwrap();
        }
    }

    #[test]
    fn legacy_pane_names_open_their_pane_at_startup() {
        let initial = app();
        for (name, pane) in [("icons", "icons"), ("wallpaper", "wallpaper"), ("theme", "appearance"), ("appearance/fonts", "appearance")] {
            let a = SettingsApp::new(initial.m.paths.clone(), Some(name));
            assert_eq!(PANES[a.m.sel.pane].id, pane, "{name}");
        }
    }

    #[test]
    fn typing_in_search_filters_panes() {
        let mut a = app();
        let mut f = ev(Key::Char, Some('f'));
        f.ctrl = true;
        f.text.clear();
        a.key(&f);
        assert_eq!(a.zone, Zone::Search);
        for c in "gap".chars() {
            a.key(&ev(Key::Char, Some(c)));
        }
        assert_eq!(a.search.text(), "gap");
        // individual settings are searched too: the pane with the best match comes first
        let ids = a.visible_panes().iter().map(|&i| PANES[i].id).collect::<Vec<_>>();
        assert_eq!(ids[0], "tiling", "{ids:?}");
        a.key(&ev(Key::Escape, None));
        assert_eq!(a.search.text(), "");
    }
}
