//! decexpose: a Mission Control / Exposé style window overview. A dimmed full-screen overlay with the windows of the
//! current workspace (rdwm tag view) as a grid of thumbnails, or with `--all` every window grouped by workspace
//! under a strip of workspace chips.
//!
//! The window list comes from rdwm's IPC `get_state` (see [`crate::switcher`], whose entries and commit requests are
//! reused). The thumbnails are captured once, before the overlay exists, with X11 (Composite `NameWindowPixmap` when
//! available, else `GetImage` on the window); windows that are not mapped (other tags, minimised) get an icon card.
//! The UI is an X11 popup like decsession's (override-redirect, full screen, keyboard and pointer grabbed). This
//! module holds the logic (layout maths, filtering, keys, drawing); the `decexpose` binary talks to rdwm once the
//! popup, and with it the grabs, is gone.

use crate::app::Button;
use crate::gfx::{blit_scaled, color, fill_rect, fill_rounded, rounded_rect, soft_shadow, stroke_rounded};
use crate::icons::IconCache;
use crate::menubar::rdwm;
use crate::switcher::{self, Entry};
use crate::text::{Fonts, Weight};
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tiny_skia::{Color, FillRule, FilterQuality, Paint, Pattern, Pixmap, SpreadMode, Transform};

/// Number of workspace chips (rdwm's tags 1-9).
pub const TAGS: u32 = 9;

// ---------------------------------------------------------------- the window list

/// One window of the overview: the switcher's entry plus its size and (when it could be captured) its thumbnail.
#[derive(Debug, Clone)]
pub struct Item {
    pub e: Entry,
    /// Client size in pixels (what the thumbnail's aspect ratio comes from); a 16:10 guess when unknown.
    pub size: (u32, u32),
    pub thumb: Option<Pixmap>,
}

impl Item {
    pub fn new(e: Entry) -> Self {
        Item { e, size: (1280, 800), thumb: None }
    }
}

/// What rdwm's `get_state` says, shaped for the overview.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// All windows in rdwm's list (creation) order, panels dropped.
    pub items: Vec<Item>,
    pub focused: Option<u32>,
    /// Bitmask of the tags the selected monitor shows.
    pub view: u32,
    /// Bitmask of tags that hold a window.
    pub occupied: u32,
}

impl State {
    /// Everything but the windows.
    pub fn clone_meta(&self) -> State {
        State { items: Vec::new(), focused: self.focused, view: self.view, occupied: self.occupied }
    }
}

/// Parses a `get_state` reply. Unlike the Alt+Tab list the order is rdwm's stable list order, not recency.
pub fn parse_state(reply: &str) -> State {
    let Ok(v) = serde_json::from_str::<Value>(reply) else { return State::default() };
    let st = v.get("state").unwrap_or(&v);
    let mut entries = switcher::parse_state(reply, &[]);
    let order: Vec<u32> = st.get("windows").and_then(Value::as_array).map(|a| a.iter().filter_map(|w| w.get("id").and_then(Value::as_u64).map(|n| n as u32)).collect()).unwrap_or_default();
    entries.sort_by_key(|e| order.iter().position(|id| *id == e.id).unwrap_or(usize::MAX));
    let mons = st.get("monitors").and_then(Value::as_array).cloned().unwrap_or_default();
    let selected = st.get("selected_mon").and_then(Value::as_i64);
    let mon_mask =
        |key: &str| -> Option<u32> { mons.iter().find(|m| selected.is_none_or(|s| m.get("id").and_then(Value::as_i64) == Some(s))).and_then(|m| m.get(key)).and_then(Value::as_u64).map(|n| n as u32) };
    let view = mon_mask("view").or_else(|| mons.first().and_then(|m| m.get("view")).and_then(Value::as_u64).map(|n| n as u32)).unwrap_or(1);
    let occupied = mons.iter().filter_map(|m| m.get("occupied").and_then(Value::as_u64)).fold(0, |a, b| a | b as u32) | entries.iter().fold(0, |a, e| a | e.tags);
    State { items: entries.into_iter().map(Item::new).collect(), focused: st.get("focused").and_then(Value::as_u64).map(|n| n as u32), view, occupied }
}

/// Asks rdwm for its state; `None` when it does not answer.
pub fn fetch_state() -> Option<State> {
    rdwm::request(r#"{"cmd":"get_state"}"#).map(|r| parse_state(&r))
}

/// Case-insensitive filter: every word of `query` must appear in the title or the class.
pub fn matches(e: &Entry, query: &str) -> bool {
    let hay = format!("{} {}", e.title, e.class).to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| hay.contains(w))
}

/// Indices of the items the overview shows, in cell order: the windows of the current view (or all of them, sorted
/// by workspace, with `all`) that match `query`.
pub fn visible_items(items: &[Item], all: bool, query: &str) -> Vec<usize> {
    let mut v: Vec<usize> = (0..items.len()).filter(|&i| (all || items[i].e.on_view) && matches(&items[i].e, query)).collect();
    if all {
        v.sort_by_key(|&i| items[i].e.tag_number());
    }
    v
}

// ---------------------------------------------------------------- layout

/// Spacing constants in pixels (already multiplied by the UI scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub gap: f32,
    pub label_h: f32,
    pub row_gap: f32,
    pub header_h: f32,
}

impl Metrics {
    pub fn new(ui: f32) -> Self {
        Metrics { gap: 28.0 * ui, label_h: 30.0 * ui, row_gap: 16.0 * ui, header_h: 34.0 * ui }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A thumbnail slot. The label (icon and title) sits right below it, `label_h` tall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Index in the flattened list of cells (= the visible list).
    pub idx: usize,
    pub group: usize,
    /// Row number counted over all groups.
    pub row: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Cell {
    pub fn contains(&self, px: f32, py: f32, label_h: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h + label_h
    }

    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.0
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLayout {
    /// Scale applied to the (virtual) window sizes, at most 1.
    pub scale: f32,
    pub cells: Vec<Cell>,
    /// Group heading positions: (group, y). Only when headings were asked for.
    pub headers: Vec<(usize, f32)>,
    pub rows: usize,
}

/// Window sizes smaller than this (on the longer side) are enlarged for layout, so a tiny dialog stays clickable.
const MIN_SIDE: f32 = 260.0;

/// The size a window takes in the layout: its real size, never tiny and never degenerate.
pub fn virtual_size(w: u32, h: u32) -> (f32, f32) {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let k = (MIN_SIDE / w.max(h)).max(1.0);
    (w * k, h * k)
}

/// Splits `widths` into at most `rows` consecutive runs, minimising the widest run (binary search on the limit).
fn partition(widths: &[f32], rows: usize) -> Vec<(usize, usize)> {
    let n = widths.len();
    if n == 0 {
        return Vec::new();
    }
    let rows = rows.clamp(1, n);
    let pack = |limit: f32| -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let (mut start, mut sum) = (0, 0.0f32);
        for (i, w) in widths.iter().enumerate() {
            if i > start && sum + w > limit {
                out.push((start, i));
                start = i;
                sum = 0.0;
            }
            sum += w;
        }
        out.push((start, n));
        out
    };
    let (mut lo, mut hi) = (widths.iter().cloned().fold(0.0, f32::max), widths.iter().sum::<f32>() + 1.0);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if pack(mid).len() <= rows {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    pack(hi)
}

struct Plan {
    scale: f32,
    /// Per group the row ranges (start, end) into that group's windows.
    rows: Vec<Vec<(usize, usize)>>,
}

fn plan(groups: &[Vec<(f32, f32)>], rows_per_group: &[usize], area: Area, m: &Metrics, headers: bool) -> Plan {
    let mut rows_all = Vec::new();
    let (mut s_w, mut sum_h, mut nrows) = (f32::MAX, 0.0f32, 0usize);
    for (g, win) in groups.iter().enumerate() {
        let widths: Vec<f32> = win.iter().map(|s| s.0).collect();
        let ranges = partition(&widths, rows_per_group[g]);
        for &(a, b) in &ranges {
            let sum_w: f32 = widths[a..b].iter().sum();
            s_w = s_w.min((area.w - (b - a - 1) as f32 * m.gap).max(1.0) / sum_w);
            sum_h += win[a..b].iter().map(|s| s.1).fold(0.0, f32::max);
            nrows += 1;
        }
        rows_all.push(ranges);
    }
    let fixed = nrows as f32 * (m.label_h + m.row_gap) + if headers { groups.iter().filter(|g| !g.is_empty()).count() as f32 * m.header_h } else { 0.0 };
    let s_h = if sum_h > 0.0 { (area.h - fixed).max(1.0) / sum_h } else { 1.0 };
    Plan { scale: s_w.min(s_h).clamp(0.02, 1.0), rows: rows_all }
}

/// The grid: every group of windows (given as their virtual sizes, see [`virtual_size`]) in rows, one uniform scale
/// for all windows, the largest at which everything fits `area`. The number of rows per group is chosen to maximise
/// that scale (the widest row is minimised for each row count), preferring fewer rows on a near tie, which balances
/// the rows by aspect ratio. Rows are centred, the whole block is centred vertically, thumbnails sit on the bottom
/// of their row so the labels line up. With `headers` each non-empty group gets a heading line above its rows.
pub fn grid(groups: &[Vec<(f32, f32)>], area: Area, m: Metrics, headers: bool) -> GridLayout {
    let mut rows_per: Vec<usize> = vec![1; groups.len()];
    let mut best = plan(groups, &rows_per, area, &m, headers);
    for _ in 0..3 {
        let mut changed = false;
        for g in 0..groups.len() {
            let before = rows_per[g];
            let mut pick = before;
            for r in 1..=groups[g].len().max(1) {
                if r == before {
                    continue;
                }
                let mut trial = rows_per.clone();
                trial[g] = r;
                let p = plan(groups, &trial, area, &m, headers);
                // Prefer fewer rows unless the other count is clearly bigger.
                let better = if r < pick { p.scale >= best.scale * 0.995 } else { p.scale > best.scale * 1.005 };
                if better {
                    best = p;
                    pick = r;
                }
            }
            if pick != before {
                rows_per[g] = pick;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let s = best.scale;
    let nrows: usize = best.rows.iter().map(Vec::len).sum();
    let mut block_h = nrows as f32 * (m.label_h + m.row_gap) - if nrows > 0 { m.row_gap } else { 0.0 };
    if headers {
        block_h += groups.iter().filter(|g| !g.is_empty()).count() as f32 * m.header_h;
    }
    for (g, ranges) in best.rows.iter().enumerate() {
        for &(a, b) in ranges {
            block_h += s * groups[g][a..b].iter().map(|v| v.1).fold(0.0, f32::max);
        }
    }
    let mut y = area.y + ((area.h - block_h) / 2.0).max(0.0);
    let mut out = GridLayout { scale: s, rows: nrows, ..GridLayout::default() };
    let (mut idx, mut row) = (0, 0);
    for (g, ranges) in best.rows.iter().enumerate() {
        if headers && !groups[g].is_empty() {
            out.headers.push((g, y));
            y += m.header_h;
        }
        for &(a, b) in ranges {
            let sizes = &groups[g][a..b];
            let row_h = s * sizes.iter().map(|v| v.1).fold(0.0, f32::max);
            let row_w = s * sizes.iter().map(|v| v.0).sum::<f32>() + (b - a - 1) as f32 * m.gap;
            let mut x = area.x + (area.w - row_w) / 2.0;
            for v in sizes {
                let (w, h) = (s * v.0, s * v.1);
                out.cells.push(Cell { idx, group: g, row, x: x.round(), y: (y + row_h - h).round(), w: w.round().max(1.0), h: h.round().max(1.0) });
                idx += 1;
                x += w + m.gap;
            }
            row += 1;
            y += row_h + m.label_h + m.row_gap;
        }
    }
    out
}

/// The cell under a point (the label counts as part of the cell).
pub fn cell_at(l: &GridLayout, label_h: f32, x: f32, y: f32) -> Option<usize> {
    l.cells.iter().find(|c| c.contains(x, y, label_h)).map(|c| c.idx)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The cell to move to from `from`: left / right step through reading order (wrapping), up / down jump to the nearest
/// cell (by horizontal centre) of the row above / below and stay put at the edge.
pub fn neighbor(cells: &[Cell], from: usize, dir: Dir) -> usize {
    let n = cells.len();
    if n == 0 {
        return 0;
    }
    let from = from.min(n - 1);
    match dir {
        Dir::Right => (from + 1) % n,
        Dir::Left => (from + n - 1) % n,
        Dir::Up | Dir::Down => {
            let row = cells[from].row;
            let target = if dir == Dir::Up { row.checked_sub(1) } else { Some(row + 1) };
            let cx = cells[from].center_x();
            target.and_then(|r| cells.iter().filter(|c| c.row == r).min_by(|a, b| (a.center_x() - cx).abs().total_cmp(&(b.center_x() - cx).abs()))).map_or(from, |c| c.idx)
        }
    }
}

/// Groups the visible items for the layout: one group (the current view), or one per workspace with `all`.
/// Returns (workspace number or 0, virtual sizes).
fn groups_of(items: &[Item], visible: &[usize], all: bool) -> Vec<(u32, Vec<(f32, f32)>)> {
    let mut out: Vec<(u32, Vec<(f32, f32)>)> = Vec::new();
    for &i in visible {
        let tag = if all { items[i].e.tag_number() } else { 0 };
        let size = virtual_size(items[i].size.0, items[i].size.1);
        match out.last_mut() {
            Some((t, v)) if *t == tag => v.push(size),
            _ => out.push((tag, vec![size])),
        }
    }
    out
}

const MARGIN: f32 = 56.0;

/// Where the grid may go on a screen `w` x `h`: below the chips / search strip.
pub fn grid_area(w: f32, h: f32, all: bool, ui: f32) -> Area {
    let top = if all { 108.0 } else { 70.0 } * ui;
    let bottom = 36.0 * ui;
    let side = MARGIN * ui;
    Area { x: side, y: top, w: (w - 2.0 * side).max(1.0), h: (h - top - bottom).max(1.0) }
}

/// The layout for `items`' visible windows on a `w` x `h` screen, plus the visible list it is for.
pub fn plan_cells(items: &[Item], all: bool, query: &str, w: f32, h: f32, ui: f32) -> (Vec<usize>, Vec<u32>, GridLayout) {
    let visible = visible_items(items, all, query);
    let groups = groups_of(items, &visible, all);
    let area = grid_area(w, h, all, ui);
    let l = grid(&groups.iter().map(|g| g.1.clone()).collect::<Vec<_>>(), area, Metrics::new(ui), all);
    (visible, groups.iter().map(|g| g.0).collect(), l)
}

/// The size each item's thumbnail needs to have: twice its first cell (headroom for filtering growing the cells),
/// at most its real size. Items that are not shown get their real size capped to a small preview.
pub fn thumb_caps(items: &[Item], all: bool, w: f32, h: f32, ui: f32) -> Vec<(u32, u32)> {
    let (visible, _, l) = plan_cells(items, all, "", w, h, ui);
    let mut caps: Vec<(u32, u32)> = items.iter().map(|it| (it.size.0.clamp(1, 256), it.size.1.clamp(1, 256))).collect();
    for (c, &i) in l.cells.iter().zip(&visible) {
        caps[i] = (((c.w * 2.0) as u32).min(items[i].size.0).max(1), ((c.h * 2.0) as u32).min(items[i].size.1).max(1));
    }
    caps
}

// ---------------------------------------------------------------- what the user decided

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Outcome {
    #[default]
    Pending,
    Cancel,
    /// A window cell was chosen.
    Commit(Entry),
    /// A workspace chip was clicked: the tag number for `view`.
    View(u32),
}

/// Shared with the binary, which reads it after the window closed.
#[derive(Debug, Default)]
pub struct Shared {
    pub outcome: Outcome,
    /// Windows the user closed from the overview.
    pub closed: Vec<u32>,
}

/// IPC request lines that carry out `o`.
pub fn requests_for(o: &Outcome) -> Vec<String> {
    match o {
        Outcome::Commit(e) => switcher::commit_requests(e),
        Outcome::View(n) => vec![rdwm::action_request("view", &[&n.to_string()])],
        _ => Vec::new(),
    }
}

/// The request that puts the focus back where it was when the overview is cancelled (closing the overlay can move
/// the pointer focus onto whatever is under it): none when that window is gone, closed, hidden or minimised.
pub fn restore_request(state: &State, closed: &[u32]) -> Option<String> {
    let id = state.focused?;
    let e = state.items.iter().map(|i| &i.e).find(|e| e.id == id)?;
    (e.on_view && !e.minimized && !closed.contains(&id)).then(|| rdwm::action_request("focuswindow", &[&id.to_string()]))
}

// ---------------------------------------------------------------- the app

/// Rectangles of the workspace chips: (tag number, x, y, w, h), centred at the top of a screen `w` wide.
pub fn chip_rects(w: f32, ui: f32) -> Vec<(u32, f32, f32, f32, f32)> {
    let (cw, ch, gap) = (44.0 * ui, 30.0 * ui, 10.0 * ui);
    let total = TAGS as f32 * cw + (TAGS as f32 - 1.0) * gap;
    let x0 = (w - total) / 2.0;
    (0..TAGS).map(|i| (i + 1, (x0 + i as f32 * (cw + gap)).round(), (22.0 * ui).round(), cw, ch)).collect()
}

pub struct Expose {
    items: Vec<Item>,
    all: bool,
    focused: Option<u32>,
    view: u32,
    occupied: u32,
    query: String,
    /// The search pill is showing (a filter character, or `/`, was typed).
    typing: bool,
    visible: Vec<usize>,
    group_tags: Vec<u32>,
    layout: GridLayout,
    sel: usize,
    shared: Rc<RefCell<Shared>>,
    closer: Box<dyn FnMut(u32)>,
    size: (u32, u32),
    ui: f32,
    alpha: bool,
    /// The first frame is drawn slightly smaller; the second (with labels) settles it. Costs one more frame.
    intro: bool,
    fonts: Option<Fonts>,
    icons: IconCache,
    pointer_home: Option<(f32, f32)>,
    pointer_moved: bool,
    hover_chip: Option<u32>,
    term: Arc<AtomicBool>,
    started: Option<Instant>,
}

impl Expose {
    pub fn new(state: State, all: bool, shared: Rc<RefCell<Shared>>) -> Self {
        let mut s = Self {
            items: state.items,
            all,
            focused: state.focused,
            view: state.view,
            occupied: state.occupied,
            query: String::new(),
            typing: false,
            visible: Vec::new(),
            group_tags: Vec::new(),
            layout: GridLayout::default(),
            sel: 0,
            shared,
            closer: Box::new(|_| {}),
            size: (1920, 1080),
            ui: 1.0,
            alpha: false,
            intro: true,
            fonts: None,
            icons: IconCache::default(),
            pointer_home: None,
            pointer_moved: false,
            hover_chip: None,
            term: Arc::new(AtomicBool::new(false)),
            started: None,
        };
        s.relayout(s.focused);
        s
    }

    /// What to call to close the window with that id (`w`, Delete, middle click).
    pub fn with_closer(mut self, f: impl FnMut(u32) + 'static) -> Self {
        self.closer = Box::new(f);
        self
    }

    pub fn started_at(mut self, t: Instant) -> Self {
        self.started = Some(t);
        self
    }

    /// The flag a SIGTERM thread sets to ask the overview to close (a second launch toggles it off).
    pub fn term_flag(&self) -> Arc<AtomicBool> {
        self.term.clone()
    }

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.visible.get(self.sel).map(|&i| &self.items[i].e)
    }

    pub fn selected_index(&self) -> usize {
        self.sel
    }

    pub fn layout(&self) -> &GridLayout {
        &self.layout
    }

    pub fn typing(&self) -> bool {
        self.typing
    }

    fn metrics(&self) -> Metrics {
        Metrics::new(self.ui)
    }

    /// Recomputes visible list and grid; keeps the selection on window `keep` when it is still shown (else the first).
    fn relayout(&mut self, keep: Option<u32>) {
        let (mut w, mut h) = (self.size.0 as f32, self.size.1 as f32);
        if self.intro {
            // The first frame: a little smaller, centred.
            w *= 0.94;
            h *= 0.94;
        }
        let (visible, tags, mut l) = plan_cells(&self.items, self.all, &self.query, w, h, self.ui);
        if self.intro {
            let (dx, dy) = (self.size.0 as f32 * 0.03, self.size.1 as f32 * 0.03);
            for c in &mut l.cells {
                c.x += dx;
                c.y += dy;
            }
            for hd in &mut l.headers {
                hd.1 += dy;
            }
        }
        self.visible = visible;
        self.group_tags = tags;
        self.layout = l;
        self.sel = keep.and_then(|id| self.visible.iter().position(|&i| self.items[i].e.id == id)).unwrap_or(0);
    }

    fn selected_id(&self) -> Option<u32> {
        self.selected().map(|e| e.id)
    }

    fn finish(&mut self, o: Outcome) -> Response {
        self.shared.borrow_mut().outcome = o;
        Response::act(Action::Close)
    }

    fn commit(&mut self) -> Response {
        match self.selected().cloned() {
            Some(e) => self.finish(Outcome::Commit(e)),
            None => Response::NONE,
        }
    }

    fn select(&mut self, i: usize) -> Response {
        if i < self.visible.len() && i != self.sel {
            self.sel = i;
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn step(&mut self, dir: Dir) -> Response {
        if self.visible.is_empty() {
            return Response::NONE;
        }
        let to = neighbor(&self.layout.cells, self.sel, dir);
        self.select(to)
    }

    /// Closes the window in cell `i` (its WM_DELETE_WINDOW) and drops it from the overview.
    fn close_cell(&mut self, i: usize) -> Response {
        let Some(&item) = self.visible.get(i) else { return Response::NONE };
        let id = self.items[item].e.id;
        (self.closer)(id);
        self.shared.borrow_mut().closed.push(id);
        let sel_id = self.selected_id();
        let next_id = self.visible.get(i + 1).or_else(|| i.checked_sub(1).and_then(|p| self.visible.get(p))).map(|&j| self.items[j].e.id);
        self.items.remove(item);
        self.relayout(if sel_id == Some(id) { next_id } else { sel_id });
        if self.visible.is_empty() && self.query.is_empty() {
            return self.finish(Outcome::Cancel);
        }
        Response::REDRAW
    }

    fn refilter(&mut self) -> Response {
        let keep = self.selected_id();
        self.relayout(keep);
        Response::REDRAW
    }

    fn chip_at(&self, x: f32, y: f32) -> Option<u32> {
        if !self.all {
            return None;
        }
        chip_rects(self.size.0 as f32, self.ui).into_iter().find(|&(_, cx, cy, cw, ch)| x >= cx && x < cx + cw && y >= cy && y < cy + ch).map(|c| c.0)
    }

    // ------------------------------------------------------------ drawing

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

    /// Text with its horizontal middle at `cx`; `top` is the top of its line box.
    #[allow(clippy::too_many_arguments)]
    fn text_centered(&mut self, pm: &mut Pixmap, s: &str, cx: f32, top: f32, px: f32, weight: Weight, c: Color) {
        let w = self.measure(s, px, weight);
        if let Some(f) = &mut self.fonts {
            f.draw(pm, s, (cx - w / 2.0).round(), top, px, weight, c);
        }
    }

    fn draw_icon(&mut self, pm: &mut Pixmap, e: &Entry, x: f32, y: f32, size: f32, opacity: f32) {
        match &e.icon {
            Some(id) => {
                let icon = self.icons.get(id, size.ceil() as u32);
                blit_scaled(pm, icon, x, y, size, size, opacity);
            }
            None => {
                let (r, g, b) = switcher::tile_color(&e.class);
                fill_rounded(pm, x, y, size, size, size * 0.225, color(r, g, b, opacity));
                let letter: String = e.class.chars().next().or_else(|| e.title.chars().next()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
                self.text_centered(pm, &letter, x + size / 2.0, y + size * 0.12, size * 0.62, Weight::Regular, color(255, 255, 255, opacity));
            }
        }
    }

    fn draw_all(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        let ui = self.ui;
        let m = self.metrics();
        // The dimmer: translucent black over the desktop, or (no compositor) an opaque darkened theme background.
        let scrim = if self.alpha { color(0, 0, 0, 0.62) } else { th.background.over(crate::theme::Rgb(0, 0, 0), 0.7).color(1.0) };
        pm.fill(Color::TRANSPARENT);
        fill_rect(pm, 0.0, 0.0, w, h, scrim);
        let white = |a: f32| color(255, 255, 255, a);
        let radius = 10.0 * ui;

        if self.all {
            for (n, x, y, cw, ch) in chip_rects(w, ui) {
                let occ = self.occupied & (1 << (n - 1)) != 0;
                let cur = self.view & (1 << (n - 1)) != 0;
                let hov = self.hover_chip == Some(n);
                let fill = if occ { 0.22 } else { 0.07 } + if hov { 0.12 } else { 0.0 };
                fill_rounded(pm, x, y, cw, ch, ch / 2.0, white(fill));
                if cur {
                    stroke_rounded(pm, x + 1.0, y + 1.0, cw - 2.0, ch - 2.0, ch / 2.0 - 1.0, th.accent_c(), 2.0);
                }
                self.text_centered(pm, &n.to_string(), x + cw / 2.0, y + (ch - 15.0 * ui * 1.2) / 2.0, 15.0 * ui, Weight::SemiBold, white(if occ { 0.95 } else { 0.45 }));
            }
        }

        let cells = self.layout.cells.clone();
        for c in &cells {
            let i = self.visible[c.idx];
            let selected = c.idx == self.sel;
            let focused = Some(self.items[i].e.id) == self.focused;
            let (minimized, e) = (self.items[i].e.minimized, self.items[i].e.clone());
            let opacity = if minimized { 0.55 } else { 1.0 };
            if selected {
                fill_rounded(pm, c.x - 10.0 * ui, c.y - 10.0 * ui, c.w + 20.0 * ui, c.h + m.label_h + 16.0 * ui, radius + 6.0 * ui, white(0.14));
            }
            soft_shadow(pm, c.x, c.y + 4.0 * ui, c.w, c.h, radius, 10.0 * ui, 0.45);
            match &self.items[i].thumb {
                Some(t) => {
                    let shader =
                        Pattern::new(t.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, opacity, Transform::from_scale(c.w / t.width() as f32, c.h / t.height() as f32).post_translate(c.x, c.y));
                    let paint = Paint { shader, anti_alias: true, ..Paint::default() };
                    if let Some(path) = rounded_rect(c.x, c.y, c.w, c.h, radius) {
                        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                    }
                }
                None => {
                    // Not mapped (another workspace, or minimised): a card with the app icon.
                    fill_rounded(pm, c.x, c.y, c.w, c.h, radius, white(0.12 * opacity.max(0.7)));
                    let size = (c.w.min(c.h) * 0.46).clamp(24.0, 128.0 * ui);
                    self.draw_icon(pm, &e, c.x + (c.w - size) / 2.0, c.y + (c.h - size) / 2.0 - if minimized { 8.0 * ui } else { 0.0 }, size, opacity);
                    if minimized {
                        self.text_centered(pm, "Minimized", c.x + c.w / 2.0, c.y + c.h / 2.0 + size / 2.0 - 2.0 * ui, 12.0 * ui, Weight::Regular, white(0.6));
                    }
                }
            }
            stroke_rounded(pm, c.x + 0.5, c.y + 0.5, c.w - 1.0, c.h - 1.0, radius - 0.5, white(0.14), 1.0);
            if focused {
                stroke_rounded(pm, c.x - 1.5 * ui, c.y - 1.5 * ui, c.w + 3.0 * ui, c.h + 3.0 * ui, radius + 1.5 * ui, white(0.75), 2.0 * ui);
            }
            if selected {
                stroke_rounded(pm, c.x - 5.0 * ui, c.y - 5.0 * ui, c.w + 10.0 * ui, c.h + 10.0 * ui, radius + 5.0 * ui, th.accent_c(), 3.0 * ui);
            }
            // The label: small icon and title, centred under the thumbnail.
            if self.fonts.is_some() {
                let icon = 18.0 * ui;
                let title = if e.title.is_empty() { e.class.clone() } else { e.title.clone() };
                let px = 13.0 * ui;
                let title = self.fit(&title, px, Weight::Regular, (c.w - icon - 8.0 * ui).max(20.0 * ui));
                let tw = self.measure(&title, px, Weight::Regular);
                let total = icon + 6.0 * ui + tw;
                let x0 = c.x + (c.w - total) / 2.0;
                let ly = c.y + c.h + 8.0 * ui;
                self.draw_icon(pm, &e, x0.round(), ly.round(), icon, opacity);
                if let Some(f) = &mut self.fonts {
                    f.draw(pm, &title, (x0 + icon + 6.0 * ui).round(), ly + (icon - px * 1.2) / 2.0, px, Weight::Regular, white(0.92 * opacity));
                }
            }
        }
        if self.fonts.is_some() {
            let headers = self.layout.headers.clone();
            for (g, y) in headers {
                let tag = self.group_tags.get(g).copied().unwrap_or(0);
                let name = format!("Desktop {tag}");
                let cur = self.view & (1 << (tag.max(1) - 1)) != 0;
                let px = 15.0 * ui;
                let tx = MARGIN * ui;
                if let Some(f) = &mut self.fonts {
                    let wd = f.draw(pm, &name, tx, y + 4.0 * ui, px, Weight::SemiBold, white(0.9));
                    if cur {
                        f.draw(pm, "current", tx + wd + 10.0 * ui, y + 6.0 * ui, 12.0 * ui, Weight::Regular, th.accent_c());
                    }
                }
                let gx = tx + 190.0 * ui;
                fill_rect(pm, gx, (y + 14.0 * ui).round(), (w - gx - MARGIN * ui).max(0.0), 1.0, white(0.12));
            }
        }
        if self.visible.is_empty() && self.fonts.is_some() {
            let msg = if self.query.is_empty() { "No windows" } else { "No matching windows" };
            self.text_centered(pm, msg, w / 2.0, h / 2.0 - 10.0 * ui, 18.0 * ui, Weight::Regular, white(0.6));
        }
        if self.typing {
            self.draw_pill(pm, w);
        }
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }

    /// The search pill at the top.
    fn draw_pill(&mut self, pm: &mut Pixmap, w: f32) {
        let ui = self.ui;
        let (pw, ph) = (360.0 * ui, 34.0 * ui);
        let y = if self.all { 62.0 * ui } else { 18.0 * ui };
        let x = ((w - pw) / 2.0).round();
        fill_rounded(pm, x, y, pw, ph, ph / 2.0, color(255, 255, 255, 0.20));
        stroke_rounded(pm, x + 0.5, y + 0.5, pw - 1.0, ph - 1.0, ph / 2.0, color(255, 255, 255, 0.25), 1.0);
        let px = 15.0 * ui;
        let (text, c) = if self.query.is_empty() {
            ("Type to filter windows".to_string(), color(255, 255, 255, 0.5))
        } else {
            (self.fit(&self.query.clone(), px, Weight::Regular, pw - 40.0 * ui), color(255, 255, 255, 0.95))
        };
        let tw = self.measure(&text, px, Weight::Regular);
        let tx = (x + 18.0 * ui).round();
        let ty = y + (ph - px * 1.2) / 2.0;
        if let Some(f) = &mut self.fonts {
            f.draw(pm, &text, tx, ty, px, Weight::Regular, c);
        }
        if !self.query.is_empty() {
            fill_rect(pm, tx + tw + 2.0, ty, 1.5 * ui, px * 1.2, color(255, 255, 255, 0.9));
        }
    }
}

impl WindowApp for Expose {
    fn title(&self) -> String {
        "decexpose".into()
    }

    fn app_id(&self) -> &'static str {
        "decexpose"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn popup_fullscreen(&self) -> bool {
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
        if let (Some(t), Some(_)) = (self.started, std::env::var_os("DECEXPOSE_TIMING")) {
            eprintln!("decexpose: first frame painted {} ms after start", t.elapsed().as_millis());
        }
        // Without the keyboard the overview could only be left with the mouse: do not hang around.
        if mods.is_none() {
            return self.finish(Outcome::Cancel);
        }
        // The thumbnails are up; now settle the layout and add the labels (finding a font costs more than the rest).
        self.fonts = Fonts::load();
        let classes: Vec<String> = self.items.iter().map(|it| it.e.class.clone()).collect();
        for (it, icon) in self.items.iter_mut().zip(switcher::resolve_icons(&classes)) {
            it.e.icon = icon;
        }
        self.intro = false;
        let keep = self.selected_id();
        self.relayout(keep);
        Response::REDRAW
    }

    fn initial_size(&self) -> (u32, u32) {
        self.size
    }

    fn set_waker(&mut self, waker: Waker) {
        // A second launch sends SIGTERM (see `single`): turn it into a normal cancel, so the focus is restored.
        let flag = self.term.clone();
        std::thread::spawn(move || {
            crate::single::block_signals(&[libc::SIGTERM]);
            while let Some(_sig) = crate::single::wait_signal(&[libc::SIGTERM]) {
                flag.store(true, Ordering::SeqCst);
                waker();
            }
        });
    }

    fn set_screen(&mut self, w: u32, h: u32) {
        if w > 0 && h > 0 {
            self.size = (w, h);
            let keep = self.selected_id();
            self.relayout(keep);
        }
    }

    fn set_scale(&mut self, scale: f32) {
        self.ui = scale.clamp(1.0, 4.0);
        let keep = self.selected_id();
        self.relayout(keep);
    }

    fn resize(&mut self, w: u32, h: u32) {
        if (w, h) != self.size {
            self.size = (w, h);
            let keep = self.selected_id();
            self.relayout(keep);
        }
    }

    fn set_rounded(&mut self, _rounded: bool) {}

    fn draw(&mut self, pm: &mut Pixmap) {
        self.draw_all(pm);
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        let home = *self.pointer_home.get_or_insert((x, y));
        if !self.pointer_moved {
            // The overview appears under a resting mouse: hovering only counts once it moves.
            if (x - home.0).abs() + (y - home.1).abs() < 4.0 {
                return Response::NONE;
            }
            self.pointer_moved = true;
        }
        let chip = self.chip_at(x, y);
        let mut redraw = chip != self.hover_chip;
        self.hover_chip = chip;
        if let Some(i) = cell_at(&self.layout, self.metrics().label_h, x, y) {
            if i != self.sel {
                self.sel = i;
                redraw = true;
            }
        }
        Response { redraw, action: None }
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed {
            return Response::NONE;
        }
        let hit = cell_at(&self.layout, self.metrics().label_h, x, y);
        match button {
            Button::Left => {
                if let Some(n) = self.chip_at(x, y) {
                    self.finish(Outcome::View(n))
                } else if let Some(i) = hit {
                    self.sel = i;
                    self.commit()
                } else {
                    // A click on the dimmed background dismisses the overview.
                    self.finish(Outcome::Cancel)
                }
            }
            Button::Middle => match hit {
                Some(i) => self.close_cell(i),
                None => Response::NONE,
            },
            _ => Response::NONE,
        }
    }

    fn scroll(&mut self, _x: f32, _y: f32, _dy: f32) -> Response {
        Response::NONE
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Escape => self.finish(Outcome::Cancel),
            Key::Enter => self.commit(),
            Key::Tab => self.step(if ev.shift { Dir::Left } else { Dir::Right }),
            Key::Right => self.step(Dir::Right),
            Key::Left => self.step(Dir::Left),
            Key::Up => self.step(Dir::Up),
            Key::Down => self.step(Dir::Down),
            Key::Home => self.select(0),
            Key::End => self.select(self.visible.len().saturating_sub(1)),
            Key::Delete => self.close_cell(self.sel),
            Key::Backspace => {
                if self.query.pop().is_some() {
                    self.refilter()
                } else if self.typing {
                    self.typing = false;
                    Response::REDRAW
                } else {
                    Response::NONE
                }
            }
            Key::Char if ev.ctrl && ev.ch == Some('w') => self.close_cell(self.sel),
            Key::Char if ev.ctrl || ev.alt => Response::NONE,
            // `w` closes the selected window until a filter is being typed (`/` starts one, so "w..." can be searched).
            Key::Char if ev.ch == Some('w') && !ev.shift && !self.typing => self.close_cell(self.sel),
            Key::Char if ev.ch == Some('/') && !self.typing => {
                self.typing = true;
                Response::REDRAW
            }
            Key::Char => {
                let text = if ev.text.is_empty() { ev.ch.map(String::from).unwrap_or_default() } else { ev.text.clone() };
                if text.is_empty() || text.chars().any(char::is_control) {
                    return Response::NONE;
                }
                self.typing = true;
                self.query.push_str(&text);
                self.refilter()
            }
            _ => Response::NONE,
        }
    }

    fn tick(&mut self) -> Response {
        if self.term.swap(false, Ordering::SeqCst) {
            return self.finish(Outcome::Cancel);
        }
        Response::NONE
    }
}

// ---------------------------------------------------------------- capturing thumbnails

#[cfg(feature = "x11")]
pub mod capture {
    //! Thumbnails and sizes of the windows, from the X server, on one short-lived connection.

    use super::Item;
    use tiny_skia::Pixmap;
    use x11rb::connection::{Connection, RequestConnection};
    use x11rb::protocol::composite::{self, ConnectionExt as _};
    use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, MapState};
    use x11rb::rust_connection::RustConnection;

    /// A connection and the primary monitor's size and the UI scale: what the overview will cover.
    pub struct Probe {
        pub conn: RustConnection,
        pub screen: (u32, u32),
        pub ui: f32,
    }

    /// Connects, learns the monitor size and fills in every item's client size.
    pub fn probe(items: &mut [Item]) -> Option<Probe> {
        let (conn, num) = crate::x11_connection::connect(None).ok()?;
        let root = conn.setup().roots[num].root;
        let (sw, sh) = (u32::from(conn.setup().roots[num].width_in_pixels), u32::from(conn.setup().roots[num].height_in_pixels));
        let mon = crate::backend::x11::panel_monitor(&conn, root, (sw as i32, sh as i32));
        let ui = crate::backend::x11_window::xft_scale(&conn, root);
        let cookies: Vec<_> = items.iter().map(|it| conn.get_geometry(it.e.id).ok()).collect();
        for (it, c) in items.iter_mut().zip(cookies) {
            if let Some(g) = c.and_then(|c| c.reply().ok()) {
                if g.width > 0 && g.height > 0 {
                    it.size = (u32::from(g.width), u32::from(g.height));
                }
            }
        }
        Some(Probe { conn, screen: (mon.w.max(1) as u32, mon.h.max(1) as u32), ui })
    }

    /// Captures every window that is in view and not minimised, scaled down to at most `caps` (per item).
    /// Windows that cannot be captured (unmapped, partly off screen, odd pixel formats) keep no thumbnail.
    pub fn capture(p: &Probe, items: &mut [Item], caps: &[(u32, u32)]) {
        let conn = &p.conn;
        let composite_ok = conn.extension_information(composite::X11_EXTENSION_NAME).ok().flatten().is_some() && conn.composite_query_version(0, 4).ok().and_then(|c| c.reply().ok()).is_some();
        let want: Vec<usize> = (0..items.len()).filter(|&i| items[i].e.on_view && !items[i].e.minimized).collect();
        let attrs: Vec<_> = want.iter().map(|&i| (i, conn.get_window_attributes(items[i].e.id).ok(), conn.get_geometry(items[i].e.id).ok())).collect();
        for (i, a, g) in attrs {
            let (Some(a), Some(g)) = (a.and_then(|c| c.reply().ok()), g.and_then(|c| c.reply().ok())) else { continue };
            if a.map_state != MapState::VIEWABLE || g.width == 0 || g.height == 0 {
                continue;
            }
            let (w, h, bw) = (u32::from(g.width), u32::from(g.height), g.border_width as i16);
            let id = items[i].e.id;
            let mut data = None;
            if composite_ok {
                // The window's own off-screen pixmap: the true contents even when something overlaps it.
                if let Ok(pix) = conn.generate_id() {
                    if conn.composite_name_window_pixmap(id, pix).ok().is_some_and(|c| c.check().is_ok()) {
                        data = conn.get_image(ImageFormat::Z_PIXMAP, pix, bw, bw, g.width, g.height, !0).ok().and_then(|c| c.reply().ok());
                        let _ = conn.free_pixmap(pix);
                    }
                }
            }
            let data = data.or_else(|| conn.get_image(ImageFormat::Z_PIXMAP, id, 0, 0, g.width, g.height, !0).ok().and_then(|c| c.reply().ok()));
            let Some(img) = data else { continue };
            if img.depth < 24 || img.data.len() != (w * h * 4) as usize {
                continue;
            }
            let cap = caps.get(i).copied().unwrap_or((w, h));
            items[i].size = (w, h);
            items[i].thumb = downscale_bgrx(&img.data, w, h, cap.0.min(w).max(1), cap.1.min(h).max(1));
        }
    }

    /// After a window was chosen: moves the pointer into it unless it is there already. With focus-follows-mouse the
    /// window under the pointer would otherwise take the focus back as soon as the overlay is gone. Waits a moment for
    /// the window to be mapped (its workspace may just have been switched to).
    pub fn warp_into(id: u32) {
        let Ok((conn, num)) = crate::x11_connection::connect(None) else { return };
        let root = conn.setup().roots[num].root;
        for _ in 0..40 {
            let viewable = conn.get_window_attributes(id).ok().and_then(|c| c.reply().ok()).is_some_and(|a| a.map_state == MapState::VIEWABLE);
            if viewable {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (Some(g), Some(t)) = (conn.get_geometry(id).ok().and_then(|c| c.reply().ok()), conn.translate_coordinates(id, root, 0, 0).ok().and_then(|c| c.reply().ok())) else { return };
        let Some(p) = conn.query_pointer(root).ok().and_then(|c| c.reply().ok()) else { return };
        let (x0, y0) = (i32::from(t.dst_x), i32::from(t.dst_y));
        let inside = i32::from(p.root_x) >= x0 && i32::from(p.root_x) < x0 + i32::from(g.width) && i32::from(p.root_y) >= y0 && i32::from(p.root_y) < y0 + i32::from(g.height);
        if !inside {
            let _ = conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, (x0 + i32::from(g.width) / 2) as i16, (y0 + i32::from(g.height) / 2) as i16);
        }
        let _ = conn.get_input_focus().map(|c| c.reply());
    }

    /// Box-filter scale of 32-bit BGRX pixels (`w` x `h`) down to `tw` x `th`, as an opaque premultiplied RGBA pixmap.
    pub fn downscale_bgrx(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Option<Pixmap> {
        if src.len() < (w * h * 4) as usize || tw == 0 || th == 0 || tw > w || th > h {
            return None;
        }
        let mut pm = Pixmap::new(tw, th)?;
        let out = pm.data_mut();
        let (w, h) = (w as usize, h as usize);
        for ty in 0..th as usize {
            let (y0, y1) = (ty * h / th as usize, ((ty + 1) * h / th as usize).max(ty * h / th as usize + 1));
            for tx in 0..tw as usize {
                let (x0, x1) = (tx * w / tw as usize, ((tx + 1) * w / tw as usize).max(tx * w / tw as usize + 1));
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for y in y0..y1 {
                    let row = &src[(y * w + x0) * 4..(y * w + x1) * 4];
                    for px in row.chunks_exact(4) {
                        b += u32::from(px[0]);
                        g += u32::from(px[1]);
                        r += u32::from(px[2]);
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as u32;
                let o = (ty * tw as usize + tx) * 4;
                out[o] = (r / n) as u8;
                out[o + 1] = (g / n) as u8;
                out[o + 2] = (b / n) as u8;
                out[o + 3] = 255;
            }
        }
        Some(pm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, class: &str, title: &str, tags: u32, on_view: bool) -> Entry {
        Entry { id, class: class.into(), title: title.into(), minimized: false, tags, on_view, icon: None }
    }

    fn item(id: u32, class: &str, title: &str, tags: u32, on_view: bool, size: (u32, u32)) -> Item {
        Item { e: entry(id, class, title, tags, on_view), size, thumb: None }
    }

    fn state(n: u32) -> State {
        State { items: (1..=n).map(|i| item(i, &format!("app{i}"), &format!("title {i}"), 1, true, (800, 500))).collect(), focused: Some(1), view: 1, occupied: 1 }
    }

    fn expose(n: u32) -> (Expose, Rc<RefCell<Shared>>) {
        let sh = Rc::new(RefCell::new(Shared::default()));
        let mut e = Expose::new(state(n), false, sh.clone());
        e.intro = false;
        e.set_screen(1600, 1000);
        e.relayout(Some(1));
        (e, sh)
    }

    fn key(k: Key, ch: Option<char>) -> KeyEvent {
        KeyEvent { key: k, text: ch.map(String::from).unwrap_or_default(), ch, ctrl: false, shift: false, alt: false }
    }

    fn area() -> Area {
        Area { x: 50.0, y: 70.0, w: 1500.0, h: 880.0 }
    }

    fn sizes(n: usize, w: f32, h: f32) -> Vec<(f32, f32)> {
        vec![(w, h); n]
    }

    fn inside(c: &Cell, a: Area, m: &Metrics) -> bool {
        c.x >= a.x - 1.0 && c.x + c.w <= a.x + a.w + 1.0 && c.y >= a.y - 1.0 && c.y + c.h + m.label_h <= a.y + a.h + 1.0
    }

    fn overlap(a: &Cell, b: &Cell, label_h: f32) -> bool {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h + label_h && b.y < a.y + a.h + label_h
    }

    #[test]
    fn grid_fits_the_area_without_overlap_for_any_count() {
        let m = Metrics::new(1.0);
        for n in 1..=24 {
            let l = grid(&[sizes(n, 1280.0, 800.0)], area(), m, false);
            assert_eq!(l.cells.len(), n);
            assert!(l.scale > 0.0 && l.scale <= 1.0);
            for (i, c) in l.cells.iter().enumerate() {
                assert!(inside(c, area(), &m), "n={n} cell {i} {c:?} outside");
                for d in &l.cells[i + 1..] {
                    assert!(!overlap(c, d, m.label_h), "n={n}: {c:?} overlaps {d:?}");
                }
            }
        }
    }

    #[test]
    fn grid_picks_rows_by_aspect_ratio() {
        let m = Metrics::new(1.0);
        // Wide screen, 16:10 windows: four windows sit in two rows of two or one row of four, whichever scales bigger.
        let four = grid(&[sizes(4, 1600.0, 1000.0)], area(), m, false);
        assert_eq!(four.rows, 2, "{four:?}");
        // Very wide windows stack in rows; very tall ones stand side by side.
        let wide = grid(&[sizes(3, 2400.0, 400.0)], area(), m, false);
        assert_eq!(wide.rows, 3);
        let tall = grid(&[sizes(3, 400.0, 1200.0)], area(), m, false);
        assert_eq!(tall.rows, 1);
        // One window: its natural size when it fits, never enlarged.
        let one = grid(&[sizes(1, 400.0, 300.0)], area(), m, false);
        assert!((one.scale - 1.0).abs() < 1e-6);
        assert_eq!((one.cells[0].w, one.cells[0].h), (400.0, 300.0));
    }

    #[test]
    fn grid_scale_is_the_largest_that_fits() {
        let m = Metrics::new(1.0);
        let l = grid(&[sizes(6, 1920.0, 1080.0)], area(), m, false);
        let used_w = l.cells.iter().map(|c| c.x + c.w).fold(0.0, f32::max) - l.cells.iter().map(|c| c.x).fold(f32::MAX, f32::min);
        let used_h = l.cells.iter().map(|c| c.y + c.h + m.label_h).fold(0.0, f32::max) - l.cells.iter().map(|c| c.y).fold(f32::MAX, f32::min);
        // Either the width or the height of the area is (nearly) used up: a bigger scale would not fit.
        assert!(used_w > area().w - 2.0 * m.gap || used_h > area().h - m.row_gap - m.label_h - 24.0, "w {used_w} h {used_h} scale {}", l.scale);
        // More windows never make the scale bigger.
        let mut last = 2.0;
        for n in 1..=16 {
            let s = grid(&[sizes(n, 1920.0, 1080.0)], area(), m, false).scale;
            assert!(s <= last + 1e-4, "n={n} {s} > {last}");
            last = s;
        }
    }

    #[test]
    fn rows_are_centred_and_thumbnails_share_a_bottom_line() {
        let m = Metrics::new(1.0);
        let l = grid(&[vec![(800.0, 500.0), (500.0, 800.0), (800.0, 500.0)]], area(), m, false);
        let bottoms: Vec<f32> = l.cells.iter().filter(|c| c.row == 0).map(|c| c.y + c.h).collect();
        assert!(bottoms.windows(2).all(|w| (w[0] - w[1]).abs() <= 1.0), "{bottoms:?}");
        let first = l.cells.iter().filter(|c| c.row == 0);
        let (lo, hi) = (first.clone().map(|c| c.x).fold(f32::MAX, f32::min), first.map(|c| c.x + c.w).fold(0.0, f32::max));
        assert!(((lo - area().x) - (area().x + area().w - hi)).abs() <= 2.0, "row not centred: {lo} {hi}");
    }

    #[test]
    fn groups_get_headings_and_their_own_rows() {
        let m = Metrics::new(1.0);
        let l = grid(&[sizes(2, 1280.0, 800.0), sizes(3, 1280.0, 800.0), vec![]], area(), m, true);
        assert_eq!(l.cells.len(), 5);
        assert_eq!(l.headers.len(), 2, "the empty group has no heading");
        assert!(l.cells.iter().filter(|c| c.group == 0).all(|c| c.row == l.cells[0].row));
        assert!(l.cells.iter().filter(|c| c.group == 1).all(|c| c.y > l.cells[0].y));
        assert!(l.headers[1].1 > l.headers[0].1);
        for c in &l.cells {
            assert!(inside(c, area(), &m), "{c:?}");
        }
        // A crowded group wraps into more rows instead of shrinking everything.
        let crowd = grid(&[sizes(12, 1280.0, 800.0), sizes(1, 1280.0, 800.0)], area(), m, true);
        assert!(crowd.cells.iter().filter(|c| c.group == 0).map(|c| c.row).max().unwrap() >= 1);
    }

    #[test]
    fn hit_testing_includes_the_label_but_not_the_gaps() {
        let m = Metrics::new(1.0);
        let l = grid(&[sizes(4, 1280.0, 800.0)], area(), m, false);
        let c = l.cells[2];
        assert_eq!(cell_at(&l, m.label_h, c.x + 3.0, c.y + 3.0), Some(2));
        assert_eq!(cell_at(&l, m.label_h, c.x + c.w - 1.0, c.y + c.h + m.label_h - 1.0), Some(2), "label area");
        assert_eq!(cell_at(&l, m.label_h, c.x - 2.0, c.y + 3.0), None, "gap");
        assert_eq!(cell_at(&l, m.label_h, 1.0, 1.0), None, "empty space");
    }

    #[test]
    fn arrow_navigation_moves_between_rows_by_position() {
        let m = Metrics::new(1.0);
        let l = grid(&[sizes(5, 1600.0, 1000.0)], area(), m, false);
        assert!(l.rows >= 2);
        let c = &l.cells;
        assert_eq!(neighbor(c, 0, Dir::Right), 1);
        assert_eq!(neighbor(c, 4, Dir::Right), 0, "wraps");
        assert_eq!(neighbor(c, 0, Dir::Left), 4);
        assert_eq!(neighbor(c, 0, Dir::Up), 0, "stays at the top edge");
        let down = neighbor(c, 0, Dir::Down);
        assert!(c[down].row == 1 && (c[down].center_x() - c[0].center_x()).abs() <= c[down].w, "{down}");
        assert_eq!(c[neighbor(c, down, Dir::Up)].row, 0);
        let last = c.len() - 1;
        assert_eq!(neighbor(c, last, Dir::Down), last);
        assert_eq!(neighbor(&[], 0, Dir::Down), 0);
    }

    #[test]
    fn tiny_windows_are_enlarged_for_layout() {
        let (w, h) = virtual_size(100, 50);
        assert!(w.max(h) >= MIN_SIDE - 0.01 && (w / h - 2.0).abs() < 1e-4);
        assert_eq!(virtual_size(1000, 500), (1000.0, 500.0));
        let (w, h) = virtual_size(0, 0);
        assert!(w > 0.0 && h > 0.0);
    }

    #[test]
    fn filter_matches_words_in_title_and_class_ignoring_case() {
        let e = entry(1, "Firefox", "Rust docs - Mozilla", 1, true);
        assert!(matches(&e, ""));
        assert!(matches(&e, "fire"));
        assert!(matches(&e, "RUST doc"));
        assert!(matches(&e, "docs fox"), "words may come from title and class");
        assert!(!matches(&e, "chrome"));
        assert!(!matches(&e, "rust nope"));
    }

    #[test]
    fn current_workspace_shows_only_windows_in_view_and_all_sorts_by_tag() {
        let items = vec![item(1, "a", "A", 1, true, (1, 1)), item(2, "b", "B", 4, false, (1, 1)), item(3, "c", "C", 2, false, (1, 1)), item(4, "d", "D", 1, true, (1, 1))];
        assert_eq!(visible_items(&items, false, ""), vec![0, 3]);
        assert_eq!(visible_items(&items, true, ""), vec![0, 3, 2, 1], "by workspace, creation order within");
        assert_eq!(visible_items(&items, true, "c"), vec![2]);
        assert_eq!(visible_items(&items, false, "c"), Vec::<usize>::new());
    }

    #[test]
    fn typing_filters_and_w_closes_until_a_filter_is_started() {
        let (mut e, sh) = expose(4);
        let closed = Rc::new(RefCell::new(Vec::new()));
        let c = closed.clone();
        e.closer = Box::new(move |id| c.borrow_mut().push(id));
        // "3" filters by title.
        assert!(e.key(&key(Key::Char, Some('3'))).redraw);
        assert!(e.typing() && e.query() == "3");
        assert_eq!(e.len(), 1);
        assert_eq!(e.selected().map(|x| x.id), Some(3));
        // While typing, w is a letter: filter becomes "3w" with nothing matching.
        e.key(&key(Key::Char, Some('w')));
        assert_eq!((e.query(), e.len()), ("3w", 0));
        assert!(closed.borrow().is_empty());
        assert_eq!(e.key(&key(Key::Enter, None)).action, None, "nothing selected, nothing to commit");
        e.key(&key(Key::Backspace, None));
        e.key(&key(Key::Backspace, None));
        assert_eq!((e.query(), e.len()), ("", 4));
        e.key(&key(Key::Backspace, None));
        assert!(!e.typing(), "backspace on an empty filter leaves search mode");
        // Without a filter, w closes the selected window (Delete too).
        e.key(&key(Key::Char, Some('w')));
        assert_eq!(*closed.borrow(), vec![3], "the selection stayed on window 3 through the filtering");
        assert_eq!(e.len(), 3);
        e.key(&key(Key::Delete, None));
        assert_eq!(closed.borrow().len(), 2);
        assert_eq!(sh.borrow().closed, *closed.borrow());
        // `/` starts a filter so that it can begin with w.
        e.key(&key(Key::Char, Some('/')));
        assert!(e.typing() && e.query().is_empty());
        e.key(&key(Key::Char, Some('w')));
        assert_eq!(e.query(), "w");
        // Ctrl+W always closes.
        let n = e.len();
        let mut k = key(Key::Char, Some('w'));
        k.ctrl = true;
        e.key(&k);
        assert!(closed.borrow().len() == 2 || n == 0);
    }

    #[test]
    fn closing_the_last_window_leaves() {
        let (mut e, sh) = expose(1);
        let r = e.key(&key(Key::Char, Some('w')));
        assert_eq!(r.action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::Cancel);
        assert_eq!(sh.borrow().closed, vec![1]);
    }

    #[test]
    fn selection_follows_the_window_when_the_filter_changes_and_after_a_close() {
        let (mut e, _) = expose(4);
        assert_eq!(e.selected().map(|x| x.id), Some(1), "starts on the focused window");
        e.key(&key(Key::Right, None));
        e.key(&key(Key::Right, None));
        assert_eq!(e.selected().map(|x| x.id), Some(3));
        e.key(&key(Key::Char, Some('t'))); // all titles match
        assert_eq!(e.selected().map(|x| x.id), Some(3), "kept");
        e.key(&key(Key::Backspace, None));
        e.key(&key(Key::Char, Some('2')));
        assert_eq!(e.selected().map(|x| x.id), Some(2), "gone from the list: first match");
    }

    #[test]
    fn keys_navigate_and_commit_or_cancel() {
        let (mut e, sh) = expose(4);
        let mut shift_tab = key(Key::Tab, None);
        e.key(&key(Key::Tab, None));
        assert_eq!(e.selected_index(), 1);
        shift_tab.shift = true;
        e.key(&shift_tab);
        e.key(&shift_tab);
        assert_eq!(e.selected_index(), 3, "wraps backwards");
        e.key(&key(Key::Home, None));
        assert_eq!(e.selected_index(), 0);
        e.key(&key(Key::End, None));
        assert_eq!(e.selected_index(), 3);
        assert_eq!(e.key(&key(Key::Enter, None)).action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::Commit(entry(4, "app4", "title 4", 1, true)));
        let (mut e, sh) = expose(2);
        assert_eq!(e.key(&key(Key::Escape, None)).action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::Cancel);
    }

    #[test]
    fn clicks_commit_close_or_dismiss() {
        let (mut e, sh) = expose(3);
        let c = e.layout().cells[2];
        assert_eq!(e.pointer_button(c.x + 5.0, c.y + 5.0, Button::Left, false).action, None, "release does nothing");
        assert_eq!(e.pointer_button(c.x + 5.0, c.y + 5.0, Button::Right, true).action, None);
        let r = e.pointer_button(c.x + 5.0, c.y + 5.0, Button::Left, true);
        assert_eq!(r.action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::Commit(entry(3, "app3", "title 3", 1, true)));
        let (mut e, sh) = expose(3);
        assert_eq!(e.pointer_button(2.0, 2.0, Button::Left, true).action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::Cancel, "empty space cancels");
        let (mut e, sh) = expose(3);
        let c = e.layout().cells[1];
        let r = e.pointer_button(c.x + 5.0, c.y + 5.0, Button::Middle, true);
        assert!(r.redraw && r.action.is_none());
        assert_eq!(sh.borrow().closed, vec![2], "middle click closes the cell");
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn hover_selects_once_the_pointer_has_moved() {
        let (mut e, _) = expose(3);
        let c = e.layout().cells[2];
        assert!(!e.pointer_motion(c.x + 10.0, c.y + 10.0).redraw, "a resting pointer under the overview");
        assert_eq!(e.selected_index(), 0);
        assert!(e.pointer_motion(c.x + 30.0, c.y + 30.0).redraw);
        assert_eq!(e.selected_index(), 2);
    }

    #[test]
    fn chips_switch_workspace_only_with_all() {
        let sh = Rc::new(RefCell::new(Shared::default()));
        let mut e = Expose::new(State { view: 1, occupied: 0b11, ..state(2) }, true, sh.clone());
        e.set_screen(1600, 1000);
        let chips = chip_rects(1600.0, 1.0);
        assert_eq!(chips.len(), 9);
        let (n, x, y, w, h) = chips[2];
        assert_eq!(n, 3);
        assert_eq!(e.pointer_button(x + w / 2.0, y + h / 2.0, Button::Left, true).action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::View(3));
        // Without --all there are no chips: the same spot is empty space.
        let (mut e, sh) = expose(2);
        e.pointer_button(x + w / 2.0, y + h / 2.0, Button::Left, true);
        assert_eq!(sh.borrow().outcome, Outcome::Cancel);
    }

    #[test]
    fn commit_requests_reuse_the_switchers() {
        let mut e = entry(7, "a", "t", 0b100, false);
        assert_eq!(requests_for(&Outcome::Commit(e.clone())), vec![r#"{"action":"view","args":["3"]}"#, r#"{"action":"focuswindow","args":["7"]}"#]);
        e.minimized = true;
        e.on_view = true;
        assert_eq!(requests_for(&Outcome::Commit(e)), vec![r#"{"action":"unminimize","args":["7"]}"#, r#"{"action":"focuswindow","args":["7"]}"#]);
        assert_eq!(requests_for(&Outcome::View(5)), vec![r#"{"action":"view","args":["5"]}"#]);
        assert!(requests_for(&Outcome::Cancel).is_empty());
        assert!(requests_for(&Outcome::Pending).is_empty());
    }

    #[test]
    fn cancel_restores_the_focus_unless_it_cannot() {
        let mut s = state(3);
        assert_eq!(restore_request(&s, &[]), Some(r#"{"action":"focuswindow","args":["1"]}"#.to_string()));
        assert_eq!(restore_request(&s, &[1]), None, "the focused window was closed from the overview");
        s.items[0].e.minimized = true;
        assert_eq!(restore_request(&s, &[]), None);
        s.items[0].e.minimized = false;
        s.items[0].e.on_view = false;
        assert_eq!(restore_request(&s, &[]), None, "on another tag");
        s.focused = None;
        assert_eq!(restore_request(&s, &[]), None);
    }

    #[test]
    fn state_is_parsed_in_list_order_with_view_and_occupancy() {
        let reply = r#"{"ok":true,"state":{"monitors":[{"id":0,"view":2,"occupied":5}],"selected_mon":0,"focused":3,"mru":[3,1,2],
            "windows":[{"id":1,"class":"a","title":"A","tags":1,"mon":0},{"id":2,"class":"decbar","title":"","tags":1,"mon":0},
            {"id":3,"class":"c","title":"C","tags":2,"mon":0,"minimized":true}]}}"#;
        let s = parse_state(reply);
        assert_eq!(s.items.iter().map(|i| i.e.id).collect::<Vec<_>>(), vec![1, 3], "creation order, panels dropped, not MRU");
        assert_eq!((s.focused, s.view, s.occupied), (Some(3), 2, 5 | 1 | 2));
        assert!(s.items[1].e.on_view && s.items[1].e.minimized && !s.items[0].e.on_view);
        let none = parse_state("garbage");
        assert!(none.items.is_empty());
    }

    #[test]
    fn thumb_caps_are_twice_the_cell_but_never_more_than_the_window() {
        let items: Vec<Item> = (1..=4).map(|i| item(i, "a", "t", 1, true, (1920, 1080))).collect();
        let (_, _, l) = plan_cells(&items, false, "", 1600.0, 1000.0, 1.0);
        let caps = thumb_caps(&items, false, 1600.0, 1000.0, 1.0);
        assert_eq!(caps.len(), 4);
        assert_eq!(caps[0].0, ((l.cells[0].w * 2.0) as u32).min(1920));
        let small = vec![item(1, "a", "t", 1, true, (300, 200))];
        assert_eq!(thumb_caps(&small, false, 1600.0, 1000.0, 1.0)[0], (300, 200));
    }

    fn draw_test(e: &mut Expose) -> Pixmap {
        e.alpha = false;
        let mut pm = Pixmap::new(e.size.0, e.size.1).unwrap();
        e.draw(&mut pm);
        pm
    }

    fn rgb(pm: &Pixmap, x: f32, y: f32) -> (u8, u8, u8) {
        let p = pm.pixel(x as u32, y as u32).unwrap();
        (p.red(), p.green(), p.blue())
    }

    #[test]
    fn draws_thumbnails_with_rounded_corners_and_a_selection_ring() {
        let sh = Rc::new(RefCell::new(Shared::default()));
        let mut st = state(3);
        for (it, c) in st.items.iter_mut().zip([(204u8, 51u8, 51u8), (51, 204, 102), (51, 102, 204)]) {
            let mut t = Pixmap::new(40, 25).unwrap();
            t.fill(Color::from_rgba8(c.0, c.1, c.2, 255));
            it.thumb = Some(t);
        }
        let mut e = Expose::new(st, false, sh);
        e.intro = false;
        e.set_screen(1600, 1000);
        e.relayout(Some(1));
        let pm = draw_test(&mut e);
        let cells = e.layout().cells.clone();
        let want = [(204, 51, 51), (51, 204, 102), (51, 102, 204)];
        for (c, w) in cells.iter().zip(want) {
            let got = rgb(&pm, c.x + c.w / 2.0, c.y + c.h / 2.0);
            assert!((i32::from(got.0) - i32::from(w.0)).abs() <= 2 && (i32::from(got.1) - i32::from(w.1)).abs() <= 2 && (i32::from(got.2) - i32::from(w.2)).abs() <= 2, "{got:?} vs {w:?}");
        }
        let scrim = rgb(&pm, 3.0, 3.0);
        let c0 = cells[0];
        assert_ne!(rgb(&pm, c0.x + 1.0, c0.y + 1.0), want[0], "the corner is rounded off");
        // The selected cell (the focused window, 0) has the accent ring just outside; the others do not.
        assert_ne!(rgb(&pm, c0.x + c0.w / 2.0, c0.y - 4.0), scrim);
        let c1 = cells[1];
        assert_eq!(rgb(&pm, c1.x + c1.w / 2.0, c1.y - 7.0), scrim);
    }

    #[test]
    fn unmapped_windows_draw_a_card_and_all_mode_draws_chips() {
        let sh = Rc::new(RefCell::new(Shared::default()));
        let mut st = state(2);
        st.items[1].e.tags = 4;
        st.items[1].e.on_view = false;
        st.occupied = 0b101;
        let mut e = Expose::new(st, true, sh);
        e.intro = false;
        e.set_screen(1600, 1000);
        e.relayout(Some(1));
        let pm = draw_test(&mut e);
        let scrim = rgb(&pm, 3.0, 3.0);
        let card = e.layout().cells[1];
        assert_ne!(rgb(&pm, card.x + 6.0, card.y + card.h / 2.0), scrim, "card background");
        let chips = chip_rects(1600.0, 1.0);
        let occupied = chips[2];
        let empty = chips[4];
        assert_ne!(rgb(&pm, occupied.1 + 6.0, occupied.2 + occupied.4 / 2.0), rgb(&pm, empty.1 + 6.0, empty.2 + empty.4 / 2.0), "occupied chip is brighter");
        assert_eq!(e.layout().headers.len(), 2);
    }

    #[test]
    fn term_signal_cancels() {
        let (mut e, sh) = expose(2);
        assert_eq!(e.tick().action, None);
        e.term_flag().store(true, Ordering::SeqCst);
        assert_eq!(e.tick().action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::Cancel);
    }

    #[test]
    fn grab_failure_cancels() {
        let (mut e, sh) = expose(2);
        assert_eq!(e.input_grabbed(None).action, Some(Action::Close));
        assert_eq!(sh.borrow().outcome, Outcome::Cancel);
    }

    #[cfg(feature = "x11")]
    #[test]
    fn downscale_averages_blocks() {
        // 4x2 BGRX: left half blue, right half red.
        let mut src = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                src.extend_from_slice(if x < 2 { &[255, 0, 0, 0] } else { &[0, 0, 255, 0] });
            }
        }
        let pm = capture::downscale_bgrx(&src, 4, 2, 2, 1).unwrap();
        assert_eq!(rgb(&pm, 0.0, 0.0), (0, 0, 255));
        assert_eq!(rgb(&pm, 1.0, 0.0), (255, 0, 0));
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 255);
        let mixed = capture::downscale_bgrx(&src, 4, 2, 1, 1).unwrap();
        assert_eq!(rgb(&mixed, 0.0, 0.0), (127, 0, 127));
        assert!(capture::downscale_bgrx(&src, 4, 2, 8, 1).is_none(), "never upscales");
        assert!(capture::downscale_bgrx(&src[..8], 4, 2, 2, 1).is_none(), "short buffer");
    }
}
