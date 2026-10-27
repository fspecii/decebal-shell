//! The settings window: sidebar, content pane, popups, keyboard navigation and the glue between
//! pointer/keyboard events and the [`Model`].

use super::icons::pane_icon;
use super::io::{self, Ev, Paths};
use super::model::{CtlVal, Model};
use super::panes::{pane_matches, PANES};
use super::schema::Schema;
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
    search_rect: Rect,
    first_draw: bool,
    /// `MACSETTINGS_DEBUG_LAYOUT=file`: write the control map after every paint (end-to-end tests).
    dump: Option<std::path::PathBuf>,
    dump_meta: (Option<String>, Vec<String>),
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
            m.sel.pane = pane_index(p);
        }
        let s = std::env::var("MACSETTINGS_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.5..=4.0).contains(v)).unwrap_or(1.0);
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
            search_rect: (0.0, 0.0, 0.0, 0.0),
            first_draw: true,
            dump: std::env::var_os("MACSETTINGS_DEBUG_LAYOUT").map(Into::into),
            dump_meta: (None, Vec::new()),
        }
    }

    pub fn model(&self) -> &Model {
        &self.m
    }

    fn sb(&self) -> f32 {
        SIDEBAR_W * self.s
    }

    fn visible_panes(&self) -> Vec<usize> {
        (0..PANES.len()).filter(|&i| pane_matches(&PANES[i], self.search.text())).collect()
    }

    fn select_pane(&mut self, i: usize) {
        if self.m.sel.pane != i {
            self.m.sel.pane = i;
            self.scroll = 0.0;
            self.it.focus = None;
            self.edit = None;
            self.popup = None;
            self.colorpop = None;
            self.m.sel.recording = None;
            if self.m.notice.as_ref().is_some_and(|n| n.error) {
                self.m.notice = None;
            }
        }
    }

    // ------------------------------------------------------------ thumbnails

    fn request_thumb(&mut self, id: &str) {
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
            .name("macsettings-thumbs".into())
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
        let visible = self.visible_panes();
        let mut p = Painter { fonts: self.fonts.as_mut(), s };
        // search field
        let sr = (u(12.0), u(14.0), sbw - u(24.0), u(30.0));
        self.search_rect = sr;
        let focused = self.zone == Zone::Search;
        fill_rounded(pm, sr.0, sr.1, sr.2, sr.3, u(8.0), if focused { color(0, 0, 0, 0.35) } else { pal::field() });
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
        // pane list
        self.sidebar_hits.clear();
        let mut y = u(58.0);
        let ih = u(36.0);
        for &i in &visible {
            let def = &PANES[i];
            let r = (u(8.0), y, sbw - u(16.0), ih - u(2.0));
            let selected = self.m.sel.pane == i;
            let hover = self.it.hover.as_deref() == Some(&format!("pane:{i}"));
            if selected {
                fill_rounded(pm, r.0, r.1, r.2, r.3, u(8.0), if self.zone == Zone::Sidebar { pal::accent() } else { color(255, 255, 255, 0.14) });
            } else if hover {
                fill_rounded(pm, r.0, r.1, r.2, r.3, u(8.0), color(255, 255, 255, 0.06));
            }
            pane_icon(pm, def.glyph, def.color, r.0 + u(8.0), r.1 + (r.3 - u(24.0)) / 2.0, u(24.0));
            p.text(pm, def.title, r.0 + u(42.0), r.1 + r.3 / 2.0, u(13.0), if selected { Weight::Medium } else { Weight::Regular }, pal::text());
            self.sidebar_hits.push((r, i));
            y += ih;
        }
        if visible.is_empty() {
            p.text(pm, "No results", u(24.0), y + u(12.0), u(13.0), Weight::Regular, pal::faint());
        }
    }

    fn draw_content(&mut self, pm: &mut Pixmap) {
        let s = self.s;
        let sbw = self.sb();
        let (w, h) = (pm.width() as f32 - sbw, pm.height() as f32);
        self.m.prepare(self.m.sel.pane);
        let items = self.m.items(self.m.sel.pane);
        let title = PANES[self.m.sel.pane.min(PANES.len() - 1)].title;
        // sync interaction state
        self.it.edit = self.edit.as_ref().map(|(id, f)| (id.clone(), f.text().to_string(), f.cursor()));
        self.it.recording = self.m.sel.recording.as_ref().map(|r| (if r == "new" { "k:new:keys".to_string() } else { r.clone() }, self.m.sel.rec_mods.clone()));
        let max_scroll = (self.frame.height - h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let thumbs = &self.thumbs;
        let lookup = |id: &str| -> Option<Rc<Pixmap>> { thumbs.borrow_mut().get(&id.to_string()).filter(|p| p.width() > 1).cloned() };
        let mut p = Painter { fonts: self.fonts.as_mut(), s };
        // Painted straight into the window buffer (the sidebar is drawn over the left edge afterwards):
        // a separate content layer would cost a second full-window pixmap.
        let frame = draw_items(pm, &mut p, title, &items, self.scroll, sbw, w, &self.it, &lookup);
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
        self.dump_meta = (
            items.iter().find_map(|i| if let Item::Preview { id, .. } = i { Some(id.clone()) } else { None }),
            items.iter().filter_map(|i| if let Item::Note { text, warn: true } = i { Some(text.clone()) } else { None }).collect(),
        );
        for id in wanted {
            self.request_thumb(&id);
        }
        // scrollbar
        let total = self.frame.height;
        if total > h + 1.0 {
            let th = (h * h / total).max(30.0 * s);
            let ty = (h - th) * (self.scroll / (total - h));
            fill_rounded(pm, sbw + w - 7.0 * s, ty + 2.0 * s, 4.0 * s, th - 4.0 * s, 2.0 * s, color(255, 255, 255, 0.22));
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
            "pane": PANES[self.m.sel.pane].id, "scroll": self.scroll, "ctls": ctls,
            "panes": self.sidebar_hits.iter().map(|(r, i)| json!({"id": PANES[*i].id, "rect": [r.0, r.1, r.2, r.3]})).collect::<Vec<_>>(),
            "thumbs": rects(&self.frame.thumbs), "layouts": rects(&self.frame.layouts), "preview": preview, "warnings": warns,
            "notice": self.m.notice.as_ref().map(|n| n.text.clone()), "search": [self.search_rect.0, self.search_rect.1, self.search_rect.2, self.search_rect.3],
            "loaded_thumbs": self.thumbs.borrow().len(), "search_text": self.search.text(), "zone": format!("{:?}", self.zone), "focus": self.it.focus,
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
        fill_rounded(pm, x, y, w, 32.0 * s, 16.0 * s, color(58, 58, 60, 0.98));
        fill_circle(pm, x + 16.0 * s, y + 16.0 * s, 4.0 * s, pal::green());
        p.text(pm, &n.text, x + 28.0 * s, y + 16.0 * s, 12.5 * s, Weight::Medium, pal::text());
    }

    fn draw_popup(&mut self, pm: &mut Pixmap) {
        let s = self.s;
        if let Some(pp) = &self.popup {
            let r = pp.rect;
            soft_shadow(pm, r.0, r.1, r.2, r.3, 8.0 * s, 10.0 * s, 0.45);
            fill_rounded(pm, r.0, r.1, r.2, r.3, 8.0 * s, color(52, 52, 54, 0.99));
            stroke_rounded(pm, r.0, r.1, r.2, r.3, 8.0 * s, color(255, 255, 255, 0.12), 1.0);
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
                p.text(pm, &t, r.0 + 26.0 * s, y + ih / 2.0, 13.0 * s, Weight::Regular, pal::text());
            }
            if pp.opts.is_empty() {
                p.text(pm, "Nothing to choose", r.0 + 12.0 * s, r.1 + 18.0 * s, 12.5 * s, Weight::Regular, pal::faint());
            }
        }
        if let Some(cp) = &self.colorpop {
            let r = cp.rect;
            soft_shadow(pm, r.0, r.1, r.2, r.3, 10.0 * s, 10.0 * s, 0.45);
            fill_rounded(pm, r.0, r.1, r.2, r.3, 10.0 * s, color(52, 52, 54, 0.99));
            stroke_rounded(pm, r.0, r.1, r.2, r.3, 10.0 * s, color(255, 255, 255, 0.12), 1.0);
            for (i, hex) in SWATCHES.iter().enumerate() {
                let (c, rr) = (i % 6, i / 6);
                let (cx, cy) = (r.0 + 22.0 * s + c as f32 * 30.0 * s, r.1 + 22.0 * s + rr as f32 * 30.0 * s);
                let (cr, cg, cb) = ui::parse_hex(hex).unwrap_or((0, 0, 0));
                fill_circle(pm, cx, cy, 11.0 * s, color(cr, cg, cb, 1.0));
                stroke_rounded(pm, cx - 11.0 * s, cy - 11.0 * s, 22.0 * s, 22.0 * s, 11.0 * s, color(255, 255, 255, 0.3), 1.0);
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
            self.edit = Some((p.ctl.id.clone(), TextField::new(val)));
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
                if let Some(&first) = self.visible_panes().first() {
                    if !self.visible_panes().contains(&self.m.sel.pane) || ev.key == Key::Enter {
                        self.select_pane(first);
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

fn stroke_line(pm: &mut Pixmap, x0: f32, y0: f32, x1: f32, y1: f32, c: tiny_skia::Color, w: f32) {
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
        "MacSettings"
    }

    fn initial_size(&self) -> (u32, u32) {
        ((940.0 * self.s) as u32, (680.0 * self.s) as u32)
    }

    fn set_waker(&mut self, waker: Waker) {
        *self.wake_slot.lock().unwrap() = Some(waker.clone());
        self.waker = Some(waker.clone());
        self.start_thumb_worker();
        let files = self.m.watched_files();
        io::spawn_watcher(files, self.m.paths.socket.clone(), self.m.queue.clone(), waker);
    }

    fn set_screen(&mut self, _w: u32, _h: u32) {}

    fn set_scale(&mut self, scale: f32) {
        if std::env::var_os("MACSETTINGS_SCALE").is_none() && (0.5..=4.0).contains(&scale) {
            self.s = scale;
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w, h);
    }

    fn set_rounded(&mut self, _rounded: bool) {}

    fn draw(&mut self, pm: &mut Pixmap) {
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
        let (cx, cy) = self.to_content(x, y);
        if let Some((id, _)) = self.it.drag.clone() {
            if let Some(p) = self.frame.find(&id).cloned() {
                if let Kind::Slider { min, max, step, .. } = p.ctl.kind {
                    let track = p.parts.first().copied().unwrap_or(p.rect);
                    self.it.drag = Some((id, ui::slider_value(cx, track, min, max, step)));
                    return Response::REDRAW;
                }
            }
        }
        self.it.hover = if x < self.sb() {
            self.sidebar_hits.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, i)| format!("pane:{i}"))
        } else {
            self.frame.at(cx, cy).map(|p| p.ctl.id.clone()).or_else(|| self.frame.thumbs.iter().chain(self.frame.layouts.iter()).find(|(r, _)| inside(*r, cx, cy)).map(|(_, id)| id.clone()))
        };
        if before != self.it.hover || self.m.dirty {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn pointer_leave(&mut self) -> Response {
        if self.it.hover.take().is_some() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        self.it.ring = false;
        if !pressed {
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
            if let Some((_, i)) = self.sidebar_hits.iter().find(|(r, _)| inside(*r, x, y)).cloned() {
                self.zone = Zone::Sidebar;
                self.select_pane(i);
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
            return Response::NONE;
        }
        let max = (self.frame.height - self.content_h()).max(0.0);
        let old = self.scroll;
        self.scroll = (self.scroll + dy * self.s * 0.6).clamp(0.0, max);
        let _ = y;
        if (old - self.scroll).abs() > 0.1 {
            // pointer is over something else now
            Response { redraw: true, action: None }
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        self.it.ring = true;
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
        redraw |= self.m.expire_notice();
        redraw |= self.m.dirty;
        Response { redraw, action: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_decode_and_crop() {
        let d = std::env::temp_dir().join(format!("macsettings-thumb-{}", std::process::id()));
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
        let d = std::env::temp_dir().join(format!("macsettings-app-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent".into(), socket: d.join("none.sock") };
        SettingsApp::new(paths, None)
    }

    fn ev(key: Key, ch: Option<char>) -> KeyEvent {
        KeyEvent { key, text: ch.map(String::from).unwrap_or_default(), ch, ctrl: false, shift: false, alt: false }
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
        assert_eq!(a.visible_panes().iter().map(|&i| PANES[i].id).collect::<Vec<_>>(), ["tiling"]);
        a.key(&ev(Key::Escape, None));
        assert_eq!(a.search.text(), "");
    }
}
