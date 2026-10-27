//! macswitch: the Alt+Tab window switcher. A row of window cells (app icon each) on a centred card,
//! the selected window's title below; Alt released (or Return, or a click) commits, Escape cancels.
//!
//! The window list comes from rdwm's IPC `get_state`. The UI is an X11 popup (override-redirect, keyboard
//! and pointer grabbed), see [`crate::backend::x11_window`]. This module holds the logic (list building,
//! ordering, selection, keys, layout, drawing); the `macswitch` binary does the IPC when the user commits.

use crate::app::Button;
use crate::gfx::{blit_scaled, color, fill_rect, fill_rounded, stroke_rounded};
use crate::icons::{self, IconCache};
use crate::launch;
use crate::menubar::rdwm;
use crate::text::{Fonts, Weight};
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tiny_skia::{Color, Pixmap};

/// Second launch while one runs: SIGUSR1 = next, SIGUSR2 = previous.
pub const SIG_NEXT: i32 = libc::SIGUSR1;
pub const SIG_PREV: i32 = libc::SIGUSR2;

/// Window classes of macshell's own panels; they never show up in the switcher.
const PANELS: &[&str] = &["macbar", "macdock", "macspot", "maclaunch", "macswitch", "macnotify"];

const CELL: f32 = 88.0;
const GAP: f32 = 8.0;
const PAD: f32 = 20.0;
const ICON: u32 = 64;
const TITLE_H: f32 = 56.0;
const MIN_W: f32 = 300.0;

// ---------------------------------------------------------------- the window list

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub class: String,
    pub title: String,
    pub minimized: bool,
    /// Tag bitmask of the window.
    pub tags: u32,
    /// The window is on a tag its monitor currently views.
    pub on_view: bool,
    /// Icon id for [`IconCache`] (theme icon name or absolute path); `None` draws a letter tile.
    pub icon: Option<String>,
}

impl Entry {
    /// 1-based number of the lowest tag the window is on (what `view N` takes).
    pub fn tag_number(&self) -> u32 {
        if self.tags == 0 {
            1
        } else {
            self.tags.trailing_zeros() + 1
        }
    }
}

/// True for macshell's own panels and popups.
pub fn is_panel(class: &str) -> bool {
    let c = class.to_lowercase();
    PANELS.contains(&c.as_str())
}

/// The window list from a `get_state` reply, in switching order: the focused window first, then rdwm's own
/// `mru` array when it has one (most recent first), then `saved` (the order macswitch itself last left),
/// then whatever remains in rdwm's list order. Windows of all tags are kept; panels are dropped.
pub fn parse_state(reply: &str, saved: &[u32]) -> Vec<Entry> {
    let Ok(v) = serde_json::from_str::<Value>(reply) else { return Vec::new() };
    let st = v.get("state").unwrap_or(&v);
    let mut views = std::collections::HashMap::new();
    if let Some(mons) = st.get("monitors").and_then(Value::as_array) {
        for m in mons {
            if let (Some(id), Some(view)) = (m.get("id").and_then(Value::as_i64), m.get("view").and_then(Value::as_u64)) {
                views.insert(id, view as u32);
            }
        }
    }
    let have_views = !views.is_empty();
    let mut all: Vec<Entry> = Vec::new();
    for w in st.get("windows").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = w.get("id").and_then(Value::as_u64) else { continue };
        let class = w.get("class").and_then(Value::as_str).unwrap_or("").to_string();
        if is_panel(&class) {
            continue;
        }
        let tags = w.get("tags").and_then(Value::as_u64).unwrap_or(0) as u32;
        let mon = w.get("mon").and_then(Value::as_i64).unwrap_or(0);
        let on_view = !have_views || views.get(&mon).is_none_or(|view| view & tags != 0);
        all.push(Entry {
            id: id as u32,
            title: w.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            class,
            minimized: w.get("minimized").and_then(Value::as_bool).unwrap_or(false),
            tags,
            on_view,
            icon: None,
        });
    }
    let ids = |key: &str| -> Vec<u32> { st.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default() };
    let focused = st.get("focused").and_then(Value::as_u64).map(|n| n as u32);
    let mut order: Vec<u32> = focused.into_iter().collect();
    order.extend(ids("mru"));
    order.extend_from_slice(saved);
    order.extend(all.iter().map(|e| e.id));
    let mut seen = HashSet::new();
    order.retain(|id| seen.insert(*id));
    order.iter().filter_map(|id| all.iter().find(|e| e.id == *id).cloned()).collect()
}

/// The order macswitch leaves behind after committing `chosen` from `list`: `chosen` first, the rest as shown.
pub fn mru_after(list: &[Entry], chosen: u32) -> Vec<u32> {
    std::iter::once(chosen).chain(list.iter().map(|e| e.id).filter(|id| *id != chosen)).collect()
}

fn mru_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(std::env::temp_dir);
    dir.join("macswitch-mru")
}

pub fn load_mru() -> Vec<u32> {
    std::fs::read_to_string(mru_path()).map(|t| t.lines().filter_map(|l| l.trim().parse().ok()).collect()).unwrap_or_default()
}

pub fn save_mru(order: &[u32]) {
    let text: String = order.iter().map(|id| format!("{id}\n")).collect();
    let _ = std::fs::write(mru_path(), text);
}

/// Fetches rdwm's state and builds the list (icons resolved). Empty when rdwm is not running.
pub fn fetch_entries() -> Vec<Entry> {
    let Some(reply) = rdwm::request(r#"{"cmd":"get_state"}"#) else { return Vec::new() };
    let mut entries = parse_state(&reply, &load_mru());
    let classes: Vec<String> = entries.iter().map(|e| e.class.clone()).collect();
    let icons = resolve_icons(&classes);
    for (e, i) in entries.iter_mut().zip(icons) {
        e.icon = i;
    }
    entries
}

/// IPC request lines that bring `e` to the front, in order: its tag first when it is not in view, restore when
/// minimised, then focus (rdwm raises it and shows its workspace).
pub fn commit_requests(e: &Entry) -> Vec<String> {
    let id = e.id.to_string();
    let mut out = Vec::new();
    if !e.on_view {
        out.push(rdwm::action_request("view", &[&e.tag_number().to_string()]));
    }
    if e.minimized {
        out.push(rdwm::action_request("unminimize", &[&id]));
    }
    out.push(rdwm::action_request("focuswindow", &[&id]));
    out
}

// ---------------------------------------------------------------- icons

fn has_icon(id: &str) -> bool {
    if id.starts_with('/') {
        return Path::new(id).is_file();
    }
    !id.is_empty() && (icons::find_svg(id).is_some() || icons::find_png(id).is_some())
}

fn desktop_field(text: &str, key: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(h).join("applications"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("applications")));
    dirs
}

/// The icon a desktop file names, or its own id as the dock does.
fn icon_of_desktop(stem: &str, text: &str) -> Option<String> {
    desktop_field(text, "Icon").filter(|i| has_icon(i)).or_else(|| has_icon(stem).then(|| stem.to_string()))
}

/// Icon ids for window classes, the way the dock finds them: a theme icon named like the class, else the icon
/// of the `.desktop` file for it (by name, `StartupWMClass` or reverse-DNS suffix). One directory scan at most.
pub fn resolve_icons(classes: &[String]) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = classes
        .iter()
        .map(|c| {
            if c.is_empty() {
                return None;
            }
            if has_icon(c) {
                return Some(c.clone());
            }
            let lc = c.to_lowercase();
            if has_icon(&lc) {
                return Some(lc);
            }
            let path = launch::find_desktop_file(c)?;
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            icon_of_desktop(&stem, &std::fs::read_to_string(&path).ok()?)
        })
        .collect();
    if out.iter().zip(classes).all(|(o, c)| o.is_some() || c.is_empty()) {
        return out;
    }
    // One pass over all desktop files for the classes still without an icon.
    let wanted: Vec<(usize, String)> = out.iter().enumerate().filter(|(i, o)| o.is_none() && !classes[*i].is_empty()).map(|(i, _)| (i, classes[i].to_lowercase())).collect();
    for dir in app_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "desktop")).collect();
        files.sort();
        for path in files {
            if wanted.iter().all(|(i, _)| out[*i].is_some()) {
                return out;
            }
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let stem_lc = stem.to_lowercase();
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let wm = desktop_field(&text, "StartupWMClass").map(|c| c.to_lowercase());
            for (i, want) in &wanted {
                if out[*i].is_none() && (stem_lc.ends_with(&format!(".{want}")) || wm.as_deref() == Some(want.as_str())) {
                    out[*i] = icon_of_desktop(&stem, &text);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- the modifier that is held

/// The modifier the switcher hotkey is held with; letting go of it commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldMod {
    Alt,
    Super,
    Ctrl,
}

impl HoldMod {
    pub fn parse(s: &str) -> Option<HoldMod> {
        match s.to_lowercase().as_str() {
            "alt" | "mod1" => Some(HoldMod::Alt),
            "super" | "mod4" | "win" => Some(HoldMod::Super),
            "ctrl" | "control" => Some(HoldMod::Ctrl),
            _ => None,
        }
    }

    /// X11 modifier bit in a key event's state.
    pub fn mask(self) -> u16 {
        match self {
            HoldMod::Alt => 1 << 3,
            HoldMod::Super => 1 << 6,
            HoldMod::Ctrl => 1 << 2,
        }
    }

    /// Whether releasing the key with keysym `sym` lets go of this modifier.
    pub fn released_by(self, sym: u32) -> bool {
        match self {
            HoldMod::Alt => matches!(sym, 0xffe7..=0xffea),
            HoldMod::Super => matches!(sym, 0xffeb | 0xffec),
            HoldMod::Ctrl => matches!(sym, 0xffe3 | 0xffe4),
        }
    }
}

// ---------------------------------------------------------------- layout

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub per_row: usize,
    pub rows: usize,
    pub w: u32,
    pub h: u32,
}

/// Card geometry for `n` cells on a screen `screen_w` wide: one row, wrapping into more when it would not fit.
pub fn layout(n: usize, screen_w: u32) -> Layout {
    let n = n.max(1);
    let avail = (screen_w as f32 - 40.0 - 2.0 * PAD + GAP).max(CELL + GAP);
    let fit = ((avail / (CELL + GAP)).floor() as usize).max(1);
    let per_row = n.min(fit);
    let rows = n.div_ceil(per_row);
    let w = (2.0 * PAD + per_row as f32 * CELL + (per_row as f32 - 1.0) * GAP).max(MIN_W);
    let h = 2.0 * PAD + rows as f32 * CELL + (rows as f32 - 1.0) * GAP + TITLE_H;
    Layout { per_row, rows, w: w.ceil() as u32, h: h.ceil() as u32 }
}

/// Top-left of cell `i` inside a card of width `card_w`.
fn cell_origin(l: &Layout, card_w: f32, i: usize, n: usize) -> (f32, f32) {
    let (row, col) = (i / l.per_row, i % l.per_row);
    // The last row is centred like the others would be when full.
    let in_row = if row + 1 == l.rows { n - row * l.per_row } else { l.per_row };
    let row_w = in_row as f32 * CELL + (in_row as f32 - 1.0) * GAP;
    (((card_w - row_w) / 2.0 + col as f32 * (CELL + GAP)).round(), (PAD + row as f32 * (CELL + GAP)).round())
}

// ---------------------------------------------------------------- the app

/// What the user decided; read by the binary after the window closed.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Outcome {
    #[default]
    Pending,
    Cancel,
    Commit(Entry),
}

/// Counts of "next"/"previous" requests that arrived as signals from a second launch.
#[derive(Default)]
pub struct Pending {
    pub next: AtomicU32,
    pub prev: AtomicU32,
}

pub struct Switcher {
    entries: Vec<Entry>,
    sel: usize,
    hold: HoldMod,
    outcome: Rc<RefCell<Outcome>>,
    closer: Box<dyn FnMut(u32)>,
    screen: (u32, u32),
    size: (u32, u32),
    alpha: bool,
    fonts: Option<Fonts>,
    icons: IconCache,
    /// First pointer position seen (the window appears under a resting mouse; hovering only counts once it moves).
    pointer_home: Option<(f32, f32)>,
    pointer_moved: bool,
    pending: Arc<Pending>,
}

impl Switcher {
    /// `back`: start on the last window (Alt+Shift+Tab) instead of the second one.
    pub fn new(entries: Vec<Entry>, back: bool, hold: HoldMod, outcome: Rc<RefCell<Outcome>>) -> Self {
        let n = entries.len();
        let sel = if back { n.saturating_sub(1) } else { 1.min(n.saturating_sub(1)) };
        Self {
            entries,
            sel,
            hold,
            outcome,
            closer: Box::new(|_| {}),
            screen: (1920, 1080),
            size: (MIN_W as u32, 200),
            alpha: false,
            fonts: Fonts::load(),
            icons: IconCache::default(),
            pointer_home: None,
            pointer_moved: false,
            pending: Arc::new(Pending::default()),
        }
    }

    /// What to call to close the window with that id (`q` / `w`).
    pub fn with_closer(mut self, f: impl FnMut(u32) + 'static) -> Self {
        self.closer = Box::new(f);
        self
    }

    pub fn pending(&self) -> Arc<Pending> {
        self.pending.clone()
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.sel)
    }

    pub fn selected_index(&self) -> usize {
        self.sel
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Moves the selection by `delta`, wrapping around.
    pub fn step(&mut self, delta: isize) -> Response {
        let n = self.entries.len() as isize;
        if n == 0 {
            return Response::NONE;
        }
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        Response::REDRAW
    }

    fn finish(&mut self, commit: bool) -> Response {
        *self.outcome.borrow_mut() = match (commit, self.selected()) {
            (true, Some(e)) => Outcome::Commit(e.clone()),
            _ => Outcome::Cancel,
        };
        Response::act(Action::Close)
    }

    fn close_selected(&mut self) -> Response {
        let Some(e) = self.entries.get(self.sel).cloned() else { return Response::NONE };
        (self.closer)(e.id);
        self.entries.remove(self.sel);
        if self.entries.is_empty() {
            return self.finish(false);
        }
        self.sel = self.sel.min(self.entries.len() - 1);
        Response::REDRAW
    }

    fn layout(&self) -> Layout {
        layout(self.entries.len(), self.screen.0)
    }

    fn cell_at(&self, x: f32, y: f32) -> Option<usize> {
        let l = self.layout();
        let n = self.entries.len();
        (0..n).find(|&i| {
            let (cx, cy) = cell_origin(&l, self.size.0 as f32, i, n);
            x >= cx && x < cx + CELL && y >= cy && y < cy + CELL
        })
    }

    fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, w))
    }

    fn fit(&self, s: &str, px: f32, w: Weight, max_w: f32) -> String {
        if self.measure(s, px, w) <= max_w {
            return s.to_string();
        }
        let mut t = s.to_string();
        while !t.is_empty() && self.measure(&format!("{t}…"), px, w) > max_w {
            t.pop();
        }
        format!("{}…", t.trim_end())
    }

    /// Text centred on `cx`, its vertical middle at `cy`.
    #[allow(clippy::too_many_arguments)] // a drawing helper: target, text, position, size, weight, colour
    fn text_centered(&mut self, pm: &mut Pixmap, s: &str, cx: f32, cy: f32, px: f32, weight: Weight, c: Color) {
        let w = self.measure(s, px, weight);
        if let Some(f) = &mut self.fonts {
            f.draw(pm, s, (cx - w / 2.0).round(), cy - px * 0.62, px, weight, c);
        }
    }

    fn draw_card(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        pm.fill(Color::TRANSPARENT);
        let radius = th.r(1.8);
        let base = if self.alpha { th.overlay_c() } else { th.overlay.color(1.0) };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, base);

        let l = self.layout();
        let n = self.entries.len();
        for i in 0..n {
            let (cx, cy) = cell_origin(&l, w, i, n);
            let e = self.entries[i].clone();
            let selected = i == self.sel;
            if selected {
                fill_rounded(pm, cx, cy, CELL, CELL, th.r(1.0), th.ink(0.16));
                stroke_rounded(pm, cx + 1.0, cy + 1.0, CELL - 2.0, CELL - 2.0, th.r(1.0), th.accent_c(), 2.0);
            }
            let opacity = if e.on_view && !e.minimized { 1.0 } else { 0.38 };
            let ix = cx + (CELL - ICON as f32) / 2.0;
            let iy = cy + (CELL - ICON as f32) / 2.0;
            match &e.icon {
                Some(id) => {
                    let icon = self.icons.get(id, ICON);
                    blit_scaled(pm, icon, ix, iy, ICON as f32, ICON as f32, opacity);
                }
                None => {
                    let (r, g, b) = tile_color(&e.class);
                    let s = ICON as f32;
                    fill_rounded(pm, ix, iy, s, s, s * 0.225, color(r, g, b, opacity));
                    let letter: String = e.class.chars().next().or_else(|| e.title.chars().next()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
                    self.text_centered(pm, &letter, ix + s / 2.0, iy + s / 2.0, 34.0, Weight::Regular, color(255, 255, 255, opacity));
                }
            }
        }

        // The selected window's full title (fitted to the card) and where it is.
        if let Some(e) = self.entries.get(self.sel).cloned() {
            let ty = h - TITLE_H + 6.0;
            fill_rect(pm, PAD, ty - 10.0, w - 2.0 * PAD, 1.0, th.sep());
            let max_w = w - 2.0 * PAD - 8.0;
            let title = if e.title.is_empty() { e.class.clone() } else { e.title.clone() };
            let title = self.fit(&title, 15.0, Weight::Regular, max_w);
            self.text_centered(pm, &title, w / 2.0, ty + 14.0, 15.0, Weight::Regular, th.text_c());
            let sub = match (e.minimized, e.on_view) {
                (true, _) => Some("Minimized".to_string()),
                (false, false) => Some(format!("On desktop {}", e.tag_number())),
                _ => None,
            };
            if let Some(sub) = sub {
                self.text_centered(pm, &sub, w / 2.0, ty + 34.0, 12.0, Weight::Regular, th.sub());
            } else if !e.class.is_empty() && e.class != title {
                let class = self.fit(&e.class, 12.0, Weight::Regular, max_w);
                self.text_centered(pm, &class, w / 2.0, ty + 34.0, 12.0, Weight::Regular, th.sub());
            }
        }
        stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), th.outline_c(), 1.0);
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }
}

/// A stable pleasant colour per app class for the letter tile.
pub fn tile_color(class: &str) -> (u8, u8, u8) {
    let mut h: u32 = 2166136261;
    for b in class.to_lowercase().bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32;
    let (s, l) = (0.55f32, 0.45f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

impl WindowApp for Switcher {
    fn title(&self) -> String {
        "macswitch".into()
    }

    fn app_id(&self) -> &'static str {
        "macswitch"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn popup_centered(&self) -> bool {
        true
    }

    fn popup_translucent(&self) -> bool {
        true
    }

    fn popup_closes_on_focus_loss(&self) -> bool {
        false
    }

    fn set_popup_alpha(&mut self, alpha: bool) {
        self.alpha = alpha;
    }

    fn input_grabbed(&mut self, mods: Option<u16>) -> Response {
        match mods {
            // Without the keyboard we could never see the release or any key: do not hang around.
            None => self.finish(false),
            // The hotkey was already let go of (a quick tap): switch to the preselected window now.
            Some(m) if m & self.hold.mask() == 0 => self.finish(true),
            Some(_) => Response::NONE,
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        let l = self.layout();
        (l.w, l.h)
    }

    fn set_popup(&mut self, _popup: bool) {}

    fn content_size(&self) -> Option<(u32, u32)> {
        let l = self.layout();
        Some((l.w, l.h))
    }

    fn set_waker(&mut self, waker: Waker) {
        // A second `macswitch` launch signals us; a thread turns the signals into counts and wakes the loop.
        let pending = self.pending.clone();
        std::thread::spawn(move || {
            crate::single::block_signals(&[SIG_NEXT, SIG_PREV]);
            while let Some(sig) = crate::single::wait_signal(&[SIG_NEXT, SIG_PREV]) {
                let counter = if sig == SIG_NEXT { &pending.next } else { &pending.prev };
                counter.fetch_add(1, Ordering::SeqCst);
                waker();
            }
        });
    }

    fn set_screen(&mut self, w: u32, h: u32) {
        if w > 0 && h > 0 {
            self.screen = (w, h);
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w, h);
    }

    fn set_rounded(&mut self, _rounded: bool) {}

    fn draw(&mut self, pm: &mut Pixmap) {
        self.draw_card(pm);
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        let home = *self.pointer_home.get_or_insert((x, y));
        if !self.pointer_moved {
            if (x - home.0).abs() + (y - home.1).abs() < 4.0 {
                return Response::NONE;
            }
            self.pointer_moved = true;
        }
        match self.cell_at(x, y) {
            Some(i) if i != self.sel => {
                self.sel = i;
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed || button != Button::Left {
            return Response::NONE;
        }
        match self.cell_at(x, y) {
            Some(i) => {
                self.sel = i;
                self.finish(true)
            }
            None => Response::NONE,
        }
    }

    fn scroll(&mut self, _: f32, _: f32, dy: f32) -> Response {
        self.step(if dy > 0.0 { 1 } else { -1 })
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Tab => self.step(if ev.shift { -1 } else { 1 }),
            Key::Right | Key::Down => self.step(1),
            Key::Left | Key::Up => self.step(-1),
            Key::Escape => self.finish(false),
            Key::Enter => self.finish(true),
            Key::Char if matches!(ev.ch, Some('q' | 'w')) && !ev.ctrl => self.close_selected(),
            _ => Response::NONE,
        }
    }

    fn key_release(&mut self, sym: u32) -> Response {
        if self.hold.released_by(sym) {
            self.finish(true)
        } else {
            Response::NONE
        }
    }

    fn tick(&mut self) -> Response {
        let next = self.pending.next.swap(0, Ordering::SeqCst) as isize;
        let prev = self.pending.prev.swap(0, Ordering::SeqCst) as isize;
        if next == 0 && prev == 0 {
            return Response::NONE;
        }
        self.step(next - prev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(windows: &str, extra: &str) -> String {
        format!(r#"{{"ok":true,"state":{{"monitors":[{{"id":0,"view":1}}],"windows":[{windows}],{extra}"selected_mon":0}}}}"#)
    }

    fn win(id: u32, class: &str, tags: u32) -> String {
        format!(r#"{{"id":{id},"class":"{class}","title":"{class} window {id}","tags":{tags},"mon":0,"minimized":false}}"#)
    }

    fn entry(id: u32) -> Entry {
        Entry { id, class: format!("app{id}"), title: format!("title {id}"), minimized: false, tags: 1, on_view: true, icon: None }
    }

    fn ids(v: &[Entry]) -> Vec<u32> {
        v.iter().map(|e| e.id).collect()
    }

    fn switcher(n: u32, back: bool) -> (Switcher, Rc<RefCell<Outcome>>) {
        let out = Rc::new(RefCell::new(Outcome::Pending));
        ((Switcher::new((1..=n).map(entry).collect(), back, HoldMod::Alt, out.clone())), out)
    }

    fn key(k: Key, shift: bool, ch: Option<char>) -> KeyEvent {
        KeyEvent { key: k, text: String::new(), ch, ctrl: false, shift, alt: true }
    }

    #[test]
    fn focused_window_comes_first_then_list_order() {
        let s = st(&[win(1, "a", 1), win(2, "b", 1), win(3, "c", 1)].join(","), r#""focused":3,"#);
        assert_eq!(ids(&parse_state(&s, &[])), vec![3, 1, 2]);
        let none = st(&[win(1, "a", 1), win(2, "b", 1)].join(","), r#""focused":null,"#);
        assert_eq!(ids(&parse_state(&none, &[])), vec![1, 2]);
    }

    #[test]
    fn rdwm_mru_beats_saved_order_beats_list_order() {
        let w = [win(1, "a", 1), win(2, "b", 1), win(3, "c", 1), win(4, "d", 1)].join(",");
        let with_mru = st(&w, r#""focused":2,"mru":[2,4,9,1],"#);
        assert_eq!(ids(&parse_state(&with_mru, &[3])), vec![2, 4, 1, 3], "unknown ids dropped, saved fills the gap");
        let without = st(&w, r#""focused":2,"#);
        assert_eq!(ids(&parse_state(&without, &[2, 4, 3])), vec![2, 4, 3, 1]);
    }

    #[test]
    fn panels_are_skipped_and_other_tags_dimmed() {
        let w = [win(1, "foot", 1), win(2, "Macdock", 1), win(3, "macspot", 1), win(4, "Maclaunch", 1), win(5, "macbar", 1), win(6, "firefox", 4)].join(",");
        let list = parse_state(&st(&w, r#""focused":1,"#), &[]);
        assert_eq!(ids(&list), vec![1, 6]);
        assert!(list[0].on_view && !list[1].on_view, "tag 3 is not in the view of tag 1");
        assert_eq!(list[1].tag_number(), 3);
    }

    #[test]
    fn multi_monitor_views_are_per_monitor() {
        let s = r#"{"ok":true,"state":{"monitors":[{"id":0,"view":1},{"id":1,"view":2}],"windows":[
            {"id":1,"class":"a","title":"","tags":2,"mon":0},{"id":2,"class":"b","title":"","tags":2,"mon":1}],"focused":null}}"#;
        let l = parse_state(s, &[]);
        assert!(!l[0].on_view && l[1].on_view);
    }

    #[test]
    fn garbage_gives_an_empty_list() {
        assert!(parse_state("", &[]).is_empty());
        assert!(parse_state(r#"{"ok":false}"#, &[]).is_empty());
    }

    #[test]
    fn mru_moves_the_chosen_window_to_the_front() {
        let list: Vec<Entry> = (1..=4).map(entry).collect();
        assert_eq!(mru_after(&list, 3), vec![3, 1, 2, 4]);
    }

    #[test]
    fn initial_selection_is_previous_window_or_last_when_going_back() {
        assert_eq!(switcher(4, false).0.selected_index(), 1);
        assert_eq!(switcher(4, true).0.selected_index(), 3);
        assert_eq!(switcher(1, false).0.selected_index(), 0);
        assert_eq!(switcher(1, true).0.selected_index(), 0);
    }

    #[test]
    fn selection_wraps_both_ways() {
        let (mut s, _) = switcher(3, false);
        s.step(1);
        s.step(1);
        assert_eq!(s.selected_index(), 0, "past the last wraps to the first");
        s.step(-1);
        assert_eq!(s.selected_index(), 2);
        s.step(-7);
        assert_eq!(s.selected_index(), 1);
    }

    #[test]
    fn tab_shift_tab_and_arrows_navigate() {
        let (mut s, _) = switcher(4, false);
        s.key(&key(Key::Tab, false, None));
        assert_eq!(s.selected_index(), 2);
        s.key(&key(Key::Tab, true, None));
        s.key(&key(Key::Tab, true, None));
        assert_eq!(s.selected_index(), 0);
        s.key(&key(Key::Left, false, None));
        assert_eq!(s.selected_index(), 3);
        s.key(&key(Key::Right, false, None));
        s.key(&key(Key::Down, false, None));
        assert_eq!(s.selected_index(), 1);
        s.key(&key(Key::Up, false, None));
        assert_eq!(s.selected_index(), 0);
    }

    #[test]
    fn escape_cancels_and_enter_commits_the_selection() {
        let (mut s, out) = switcher(3, false);
        assert_eq!(s.key(&key(Key::Escape, false, None)).action, Some(Action::Close));
        assert_eq!(*out.borrow(), Outcome::Cancel);
        let (mut s, out) = switcher(3, false);
        s.key(&key(Key::Tab, false, None));
        assert_eq!(s.key(&key(Key::Enter, false, None)).action, Some(Action::Close));
        assert_eq!(*out.borrow(), Outcome::Commit(entry(3)));
    }

    #[test]
    fn releasing_the_held_modifier_commits_but_other_keys_do_not() {
        let (mut s, out) = switcher(3, false);
        assert_eq!(s.key_release(0xff09).action, None, "Tab");
        assert_eq!(s.key_release(0xffe1).action, None, "Shift");
        assert_eq!(*out.borrow(), Outcome::Pending);
        assert_eq!(s.key_release(0xffe9).action, Some(Action::Close), "Alt_L");
        assert_eq!(*out.borrow(), Outcome::Commit(entry(2)));
        let (mut s, out) = switcher(3, false);
        s.key_release(0xffea);
        assert_eq!(*out.borrow(), Outcome::Commit(entry(2)), "Alt_R");
    }

    #[test]
    fn hold_modifier_can_be_super() {
        let out = Rc::new(RefCell::new(Outcome::Pending));
        let mut s = Switcher::new((1..=3).map(entry).collect(), false, HoldMod::Super, out.clone());
        assert_eq!(s.key_release(0xffe9).action, None);
        assert_eq!(s.key_release(0xffeb).action, Some(Action::Close));
        assert_eq!(HoldMod::parse("Super"), Some(HoldMod::Super));
        assert_eq!(HoldMod::parse("nope"), None);
    }

    #[test]
    fn grab_without_the_modifier_held_commits_at_once() {
        let (mut s, out) = switcher(3, false);
        assert_eq!(s.input_grabbed(Some(HoldMod::Alt.mask() | 1)).action, None);
        assert_eq!(*out.borrow(), Outcome::Pending);
        assert_eq!(s.input_grabbed(Some(0)).action, Some(Action::Close));
        assert_eq!(*out.borrow(), Outcome::Commit(entry(2)));
        let (mut s, out) = switcher(3, false);
        assert_eq!(s.input_grabbed(None).action, Some(Action::Close));
        assert_eq!(*out.borrow(), Outcome::Cancel, "no keyboard grab: give up");
    }

    #[test]
    fn q_and_w_close_the_selected_window() {
        let closed = Rc::new(RefCell::new(Vec::new()));
        let c = closed.clone();
        let out = Rc::new(RefCell::new(Outcome::Pending));
        let mut s = Switcher::new((1..=3).map(entry).collect(), false, HoldMod::Alt, out.clone()).with_closer(move |id| c.borrow_mut().push(id));
        s.key(&key(Key::Char, false, Some('q')));
        assert_eq!(*closed.borrow(), vec![2]);
        assert_eq!(s.len(), 2);
        assert_eq!(s.selected().map(|e| e.id), Some(3), "selection moves on to the next window");
        s.key(&key(Key::Char, false, Some('w')));
        assert_eq!(s.selected().map(|e| e.id), Some(1));
        assert_eq!(*closed.borrow(), vec![2, 3]);
        let r = s.key(&key(Key::Char, false, Some('q')));
        assert_eq!(r.action, Some(Action::Close), "nothing left: leave");
        assert_eq!(*out.borrow(), Outcome::Cancel);
    }

    #[test]
    fn signals_from_a_second_launch_move_the_selection() {
        let (mut s, _) = switcher(5, false);
        let p = s.pending();
        p.next.fetch_add(2, Ordering::SeqCst);
        assert!(s.tick().redraw);
        assert_eq!(s.selected_index(), 3);
        p.prev.fetch_add(1, Ordering::SeqCst);
        s.tick();
        assert_eq!(s.selected_index(), 2);
        assert!(!s.tick().redraw);
    }

    #[test]
    fn commit_requests_switch_view_and_restore_before_focusing() {
        let mut e = entry(7);
        assert_eq!(commit_requests(&e), vec![r#"{"action":"focuswindow","args":["7"]}"#]);
        e.on_view = false;
        e.tags = 0b100;
        e.minimized = true;
        assert_eq!(commit_requests(&e), vec![r#"{"action":"view","args":["3"]}"#, r#"{"action":"unminimize","args":["7"]}"#, r#"{"action":"focuswindow","args":["7"]}"#]);
    }

    #[test]
    fn layout_is_one_row_until_the_screen_is_full() {
        let l = layout(3, 1920);
        assert_eq!((l.per_row, l.rows), (3, 1));
        assert!(l.w >= 300 && l.h > 100);
        let many = layout(40, 1000);
        assert!(many.rows > 1 && many.w <= 1000 - 40, "{many:?}");
        assert!(many.per_row * many.rows >= 40);
        assert_eq!(layout(0, 1920).per_row, 1);
    }

    #[test]
    fn clicking_a_cell_commits_it_and_hover_waits_for_the_pointer_to_move() {
        let (mut s, out) = switcher(3, false);
        s.resize(layout(3, 1920).w, layout(3, 1920).h);
        let l = layout(3, 1920);
        let (x0, y0) = cell_origin(&l, l.w as f32, 0, 3);
        // The window appears under a resting pointer: that must not change the selection.
        assert!(!s.pointer_motion(x0 + 10.0, y0 + 10.0).redraw);
        assert_eq!(s.selected_index(), 1);
        assert!(s.pointer_motion(x0 + 20.0, y0 + 20.0).redraw);
        assert_eq!(s.selected_index(), 0);
        let (x2, y2) = cell_origin(&l, l.w as f32, 2, 3);
        let r = s.pointer_button(x2 + 5.0, y2 + 5.0, Button::Left, true);
        assert_eq!(r.action, Some(Action::Close));
        assert_eq!(*out.borrow(), Outcome::Commit(entry(3)));
        assert_eq!(s.pointer_button(1.0, 1.0, Button::Right, true).action, None);
    }

    #[test]
    fn card_draws_cells_and_a_selection_highlight() {
        let (mut s, _) = switcher(3, false);
        s.alpha = false;
        let l = layout(3, 1920);
        s.resize(l.w, l.h);
        let mut pm = Pixmap::new(l.w, l.h).unwrap();
        s.draw(&mut pm);
        assert_eq!(pm.pixel(l.w / 2, 3).map(|p| p.alpha()), Some(255), "opaque card without a compositor");
        assert_eq!(pm.pixel(0, 0).map(|p| p.alpha()), Some(0), "rounded corner");
        let (cx, cy) = cell_origin(&l, l.w as f32, 1, 3);
        let sel = pm.pixel(cx as u32 + 2, cy as u32 + CELL as u32 / 2).unwrap();
        let other = {
            let (ox, oy) = cell_origin(&l, l.w as f32, 0, 3);
            pm.pixel(ox as u32 + 2, oy as u32 + CELL as u32 / 2).unwrap()
        };
        assert_ne!((sel.red(), sel.green(), sel.blue()), (other.red(), other.green(), other.blue()), "selected cell differs");
    }

    #[test]
    fn tile_colours_are_stable_and_class_specific() {
        assert_eq!(tile_color("Foot"), tile_color("foot"));
        assert_ne!(tile_color("foot"), tile_color("firefox"));
    }
}
