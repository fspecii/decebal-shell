//! The engine behind the Windows, GNOME, KDE, Xfce and BlackBerry styles.
//!
//! A style is two functions: `layout` turns the [`Model`] and the card rectangle into [`Zone`]s (lists of real
//! catalog targets with their geometry) and `paint` draws them. The engine owns everything else, so every style gets
//! the same behaviour: type-to-search, arrow keys across zones, Tab / Shift+Tab between zones, Enter, hover, click,
//! right click / Ctrl+P to pin, wheel scrolling or paging, Escape and outside clicks to close.

use super::catalog::{Catalog, Category};
use super::placement::{placement, Anchor, Rect};
use super::style::LauncherStyle;
use crate::app::Button;
use crate::icons::IconCache;
use crate::text::Fonts;
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response};
use std::collections::HashMap;
use tiny_skia::Pixmap;

mod blackberry;
mod gnome;
mod kde;
pub(super) mod paint;
mod ukui;
mod windows;
mod xfce;

/// What a hit area does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    App(usize),
    /// A letter heading in an alphabetical list (not focusable).
    Letter(char),
    View(View),
    Page(usize),
    /// The session menu (decsession: lock, log out, restart, shut down; it asks before acting).
    Power,
    /// Decebal Settings.
    Settings,
    /// The account settings (UKUI's user button: "Change account settings"; the session menu behind [`Target::Power`] logs out).
    Account,
    /// The file manager on the computer view (UKUI's computer button: `peony computer:///`).
    Computer,
    /// UKUI's expand / restore button: the card fills the output (full-screen menu) and goes back.
    Fullscreen,
    /// Pins or unpins the app (Windows' "Pin to Start").
    Pin(usize),
    /// Clears the search.
    Clear,
    Close,
    /// A tab, button or menu row that belongs to one style (kind, index): only that style knows what it is and what it does.
    /// `Styled::activate` hands it to the style's own `activate` (see docs/plasma-theme-ports/kickoff-contract.md).
    Style(u8, u16),
}

/// What a style's own action did, for the shared engine to turn into a `Response`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    /// Only the picture changed.
    Redraw,
    /// The zones change (a tab, a page, a menu opened or closed).
    Relayout,
    Close,
}

/// One row of a list a style fills itself (Kickoff's Places: computer, history), opened with a command.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    pub name: String,
    pub detail: String,
    pub icon: String,
    /// The command that opens it (`xdg-open PATH`, `decspot`).
    pub open: Vec<String>,
    /// Unix time it was last used (0: unknown), to order a history.
    pub at: u64,
}

impl Target {
    pub fn focusable(self) -> bool {
        !matches!(self, Target::Letter(_))
    }
}

/// The program behind a sidebar target, `have` telling which tools exist: the session menu, Settings, Settings on the account pane, the file manager on the computer view
/// (`peony computer:///` as in UKUI, else `gio open computer:///`, else `xdg-open /`; none: no command, and the Computer button is not drawn).
pub fn sidebar_argv(t: Target, have: impl Fn(&str) -> bool) -> Option<Vec<String>> {
    let own = |name: &str| crate::launch::sibling_or_path(name);
    match t {
        Target::Power => Some(vec![own("decsession")]),
        Target::Settings => Some(vec![own("decsettings")]),
        Target::Account => Some(vec![own("decsettings"), "users".into()]),
        Target::Computer => {
            let argv = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
            if have("peony") {
                argv(&["peony", "computer:///"])
            } else if have("gio") {
                argv(&["gio", "open", "computer:///"])
            } else if have("xdg-open") {
                argv(&["xdg-open", "/"])
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Whether this machine has something to open the computer view with (decided once; unit tests assume it does).
pub fn computer_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| cfg!(test) || sidebar_argv(Target::Computer, crate::controlcenter::sys::have).is_some())
}

/// Which list the content area shows while not searching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// The style's start view (Windows: Pinned + Recommended; Kickoff / Whisker: Favourites; GNOME / BB10: the grid).
    #[default]
    Home,
    All,
    Favorites,
    Recent,
    Category(Category),
}

/// The geometry of a zone's items.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// Row-major cells: `cols` per row, `rows` visible, scrolled by rows or turned by pages.
    Grid { area: Rect, cell: (f32, f32), cols: usize, rows: usize, paged: bool },
    /// One rectangle per item.
    Fixed(Vec<Rect>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub id: &'static str,
    pub items: Vec<Target>,
    pub shape: Shape,
}

impl Zone {
    /// A grid filling `area` with as many `cell`s as fit (centred horizontally when `center`).
    pub fn grid(id: &'static str, items: Vec<Target>, area: Rect, cell: (f32, f32), paged: bool, center: bool) -> Zone {
        // an area shorter than one cell gets one squeezed row rather than an item outside it
        let area = Rect { h: area.h.max(1.0), ..area };
        let cell = (cell.0, cell.1.min(area.h));
        let cols = ((area.w / cell.0).floor() as usize).max(1);
        let rows = ((area.h / cell.1).floor() as usize).max(1);
        let used = cols as f32 * cell.0;
        let area = if center { Rect { x: (area.x + (area.w - used) / 2.0).round(), w: used, ..area } } else { area };
        Zone { id, items, shape: Shape::Grid { area, cell, cols, rows, paged } }
    }

    /// A grid with an exact column count.
    pub fn columns(id: &'static str, items: Vec<Target>, area: Rect, cols: usize, row_h: f32, paged: bool) -> Zone {
        let cols = cols.max(1);
        let area = Rect { h: area.h.max(1.0), ..area };
        let row_h = row_h.min(area.h);
        let rows = ((area.h / row_h).floor() as usize).max(1);
        Zone { id, items, shape: Shape::Grid { area, cell: (area.w / cols as f32, row_h), cols, rows, paged } }
    }

    pub fn fixed(id: &'static str, items: Vec<(Target, Rect)>) -> Zone {
        let (items, rects) = items.into_iter().unzip();
        Zone { id, items, shape: Shape::Fixed(rects) }
    }

    pub fn area(&self) -> Rect {
        match &self.shape {
            Shape::Grid { area, .. } => *area,
            Shape::Fixed(rects) => {
                let Some(first) = rects.first() else { return Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 } };
                let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
                for r in rects {
                    (x0, y0, x1, y1) = (x0.min(r.x), y0.min(r.y), x1.max(r.right()), y1.max(r.bottom()));
                }
                Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
            }
        }
    }

    pub fn cols(&self) -> usize {
        match &self.shape {
            Shape::Grid { cols, .. } => *cols,
            Shape::Fixed(_) => 1,
        }
    }

    /// Items per screenful (grids), all items (fixed).
    pub fn per_view(&self) -> usize {
        match &self.shape {
            Shape::Grid { cols, rows, .. } => cols * rows,
            Shape::Fixed(r) => r.len().max(1),
        }
    }

    pub fn paged(&self) -> bool {
        matches!(self.shape, Shape::Grid { paged: true, .. })
    }

    pub fn pages(&self) -> usize {
        self.items.len().div_ceil(self.per_view()).max(1)
    }

    /// The first visible item for `offset` (a row for scrolled grids, a page for paged ones).
    pub fn first(&self, offset: usize) -> usize {
        match &self.shape {
            Shape::Grid { cols, rows, paged: true, .. } => offset * cols * rows,
            Shape::Grid { cols, .. } => offset * cols,
            Shape::Fixed(_) => 0,
        }
    }

    pub fn visible(&self, offset: usize) -> std::ops::Range<usize> {
        let first = self.first(offset).min(self.items.len());
        first..(first + self.per_view()).min(self.items.len())
    }

    pub fn rect(&self, i: usize, offset: usize) -> Option<Rect> {
        match &self.shape {
            Shape::Fixed(rects) => rects.get(i).copied(),
            Shape::Grid { area, cell, cols, .. } => {
                if !self.visible(offset).contains(&i) {
                    return None;
                }
                let pos = i - self.first(offset);
                Some(Rect { x: area.x + (pos % cols) as f32 * cell.0, y: area.y + (pos / cols) as f32 * cell.1, w: cell.0, h: cell.1 })
            }
        }
    }

    /// The largest useful offset.
    pub fn max_offset(&self) -> usize {
        match &self.shape {
            Shape::Grid { paged: true, .. } => self.pages() - 1,
            Shape::Grid { cols, rows, .. } => self.items.len().div_ceil(*cols).saturating_sub(*rows),
            Shape::Fixed(_) => 0,
        }
    }

    /// The offset that shows item `i`, starting from `offset`.
    pub fn reveal(&self, i: usize, offset: usize) -> usize {
        match &self.shape {
            Shape::Grid { cols, rows, paged: true, .. } => i / (cols * rows),
            Shape::Grid { cols, rows, .. } => {
                let row = i / cols;
                if row < offset {
                    row
                } else if row >= offset + rows {
                    row + 1 - rows
                } else {
                    offset
                }
            }
            Shape::Fixed(_) => 0,
        }
    }

    /// Lays a grid out horizontally (a dash or a row of buttons) instead of vertically for arrow keys.
    fn horizontal(&self) -> bool {
        match &self.shape {
            // a row: the items share a vertical centre (Kickoff's full-height tabs sit beside shorter buttons)
            Shape::Fixed(r) => r.len() > 1 && r.iter().all(|x| ((x.y + x.h / 2.0) - (r[0].y + r[0].h / 2.0)).abs() < 1.0),
            Shape::Grid { .. } => false,
        }
    }
}

/// Everything the styles read: the catalog, the query, the current view and the keyboard focus.
pub struct Model {
    pub cat: Catalog,
    pub field: TextField,
    pub results: Vec<usize>,
    pub view: View,
    /// The zone with the keyboard focus.
    pub zone: &'static str,
    pub sel: HashMap<&'static str, usize>,
    pub offset: HashMap<&'static str, usize>,
    /// The focus moved by keyboard (draw a focus ring), not by hovering.
    pub keyboard: bool,
    pub user: String,
    /// UKUI's full-screen menu is showing (the card fills the output).
    pub expanded: bool,
    /// The surface is a full-output one, so the card can grow to fill it (set by the layout pass; never in a plain window).
    pub can_expand: bool,
    /// The categories whose freedesktop menu icon the icon theme has.
    pub themed: std::collections::HashSet<Category>,
    /// State a style keeps for itself (its current tab, an open menu): zone-like names, small integers.
    pub ui: HashMap<&'static str, usize>,
}

impl Model {
    pub fn new(cat: Catalog) -> Model {
        Model {
            cat,
            field: TextField::default(),
            results: Vec::new(),
            view: View::Home,
            zone: "",
            sel: HashMap::new(),
            offset: HashMap::new(),
            keyboard: false,
            user: String::new(),
            expanded: false,
            can_expand: false,
            themed: Default::default(),
            ui: HashMap::new(),
        }
    }

    pub fn ui(&self, key: &str) -> usize {
        self.ui.get(key).copied().unwrap_or(0)
    }

    pub fn query(&self) -> &str {
        self.field.text()
    }

    pub fn searching(&self) -> bool {
        !self.query().trim().is_empty()
    }

    pub fn sel(&self, zone: &str) -> usize {
        self.sel.get(zone).copied().unwrap_or(0)
    }

    pub fn offset(&self, zone: &str) -> usize {
        self.offset.get(zone).copied().unwrap_or(0)
    }

    /// Item `i` of `zone` has the focus.
    pub fn focused(&self, zone: &str, i: usize) -> bool {
        self.zone == zone && self.sel(zone) == i
    }

    /// The apps of a view (Home is the style's business; it gets the favourites here).
    pub fn apps_of(&self, view: View) -> Vec<usize> {
        match view {
            View::Home | View::Favorites => self.cat.favorites.clone(),
            View::All => self.cat.all(),
            View::Recent => self.cat.recent.clone(),
            View::Category(c) => self.cat.in_category(c),
        }
    }

    pub fn app_targets(list: &[usize]) -> Vec<Target> {
        list.iter().map(|&i| Target::App(i)).collect()
    }

    /// An alphabetical list with a letter heading before each initial (Windows All apps, Kickoff All Applications).
    pub fn lettered(&self, list: &[usize]) -> Vec<Target> {
        let mut out = Vec::new();
        let mut last = None;
        for &i in list {
            let c = self.cat.apps[i].name.chars().next().map(|c| c.to_ascii_uppercase()).filter(char::is_ascii_alphabetic).unwrap_or('#');
            if last != Some(c) {
                out.push(Target::Letter(c));
                last = Some(c);
            }
            out.push(Target::App(i));
        }
        out
    }

    /// The categories as sidebar views, with Favourites / Recent / All in front when `extra`.
    pub fn category_views(&self) -> Vec<View> {
        self.cat.present.iter().map(|&c| View::Category(c)).collect()
    }

    pub fn view_title(view: View) -> &'static str {
        match view {
            View::Home | View::Favorites => "Favorites",
            View::All => "All Applications",
            View::Recent => "Recently Used",
            View::Category(c) => c.title(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The five native styles behind one interface (see the module docs).
pub struct Styled {
    pub style: LauncherStyle,
    pub m: Model,
    /// The card in surface pixels.
    pub card: Rect,
    pub surface: (u32, u32),
    pub scale: f32,
    /// The surface has an alpha channel (Wayland, or X11 with a compositor).
    pub alpha: bool,
    /// The surface covers the whole output, so a click outside the card dismisses.
    pub full: bool,
    pub anchor: Option<Anchor>,
    pub fonts: Option<Fonts>,
    pub icons: IconCache,
    pub zones: Vec<Zone>,
    wheel: f32,
}

impl Styled {
    pub fn new(style: LauncherStyle, cat: Catalog) -> Styled {
        let mut m = Model::new(cat);
        m.user = Catalog::user_name();
        m.themed = Category::ALL.into_iter().filter(|c| crate::icons::find_svg(c.icon()).or_else(|| crate::icons::find_png(c.icon())).is_some()).collect();
        if style == LauncherStyle::Kde {
            kde::prepare(&mut m);
        }
        Styled {
            style,
            m,
            card: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            surface: (1, 1),
            scale: 1.0,
            alpha: true,
            full: false,
            anchor: None,
            fonts: Fonts::load(),
            icons: IconCache::default(),
            zones: Vec::new(),
            wheel: 0.0,
        }
    }

    /// Forward the host's wake-up callback to styles that load data in the background.
    pub fn set_waker(&mut self, waker: crate::window_app::Waker) {
        if self.style == LauncherStyle::Kde {
            kde::set_waker(waker);
        }
    }

    pub fn tick(&mut self) -> Response {
        if self.style == LauncherStyle::Kde && kde::tick() {
            self.relayout();
            return Response::REDRAW;
        }
        Response::NONE
    }

    /// The window size wanted when the launcher is a plain window (no layer shell): the card alone.
    pub fn natural_size(&self, screen: (u32, u32)) -> (u32, u32) {
        let screen = if screen.0 == 0 || screen.1 == 0 { (1280.0, 800.0) } else { (screen.0 as f32, screen.1 as f32) };
        let r = placement(self.style, None, screen, self.scale);
        (r.w as u32, r.h as u32)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.surface = (w.max(1), h.max(1));
        let size = (self.surface.0 as f32, self.surface.1 as f32);
        self.card = if !self.full {
            Rect { x: 0.0, y: 0.0, w: size.0, h: size.1 }
        } else if self.m.expanded && self.style == LauncherStyle::Ukui {
            super::placement::free_area(self.anchor.as_ref(), size)
        } else {
            placement(self.style, self.anchor, size, self.scale)
        };
        self.relayout();
    }

    pub fn relayout(&mut self) {
        let s = self.scale;
        self.m.can_expand = self.full;
        self.zones = match self.style {
            LauncherStyle::Windows => windows::layout(&self.m, self.card, s),
            LauncherStyle::Gnome => gnome::layout(&self.m, self.card, s),
            LauncherStyle::Kde => kde::layout(&self.m, self.card, s),
            LauncherStyle::Ukui => ukui::layout(&self.m, self.card, s),
            LauncherStyle::Xfce => xfce::layout(&self.m, self.card, s),
            LauncherStyle::BlackBerry => blackberry::layout(&self.m, self.card, s),
            LauncherStyle::Default | LauncherStyle::Dux => Vec::new(),
        };
        // Keep the focus on an existing, focusable item and offsets within range.
        for z in &self.zones {
            let max = z.max_offset();
            let o = self.m.offset.entry(z.id).or_insert(0);
            *o = (*o).min(max);
            let len = z.items.len();
            let sel = self.m.sel.entry(z.id).or_insert(0);
            if len > 0 && *sel >= len {
                *sel = len - 1;
            }
        }
        let has = |id: &str, zones: &[Zone]| zones.iter().any(|z| z.id == id && z.items.iter().any(|t| t.focusable()));
        // An empty result list keeps the focus (Enter then does nothing rather than hitting a footer button).
        let empty_results = self.m.searching() && self.m.zone == "results" && self.zone("results").is_some();
        if !has(self.m.zone, &self.zones) && !empty_results {
            self.m.zone = self.zones.iter().find(|z| z.items.iter().any(|t| t.focusable())).map_or("", |z| z.id);
        }
        self.skip_unfocusable(true);
    }

    fn zone(&self, id: &str) -> Option<&Zone> {
        self.zones.iter().find(|z| z.id == id)
    }

    /// The focused target.
    pub fn current(&self) -> Option<Target> {
        self.zone(self.m.zone)?.items.get(self.m.sel(self.m.zone)).copied()
    }

    fn skip_unfocusable(&mut self, forward: bool) {
        let Some(z) = self.zone(self.m.zone) else { return };
        let mut i = self.m.sel(z.id);
        while z.items.get(i).is_some_and(|t| !t.focusable()) {
            if forward && i + 1 < z.items.len() {
                i += 1;
            } else if !forward && i > 0 {
                i -= 1;
            } else {
                break;
            }
        }
        let id = z.id;
        let o = z.reveal(i, self.m.offset(id));
        self.m.sel.insert(id, i);
        self.m.offset.insert(id, o);
    }

    pub(super) fn focus(&mut self, zone: &'static str, i: usize) {
        self.m.zone = zone;
        self.m.sel.insert(zone, i);
        if let Some(z) = self.zone(zone) {
            let o = z.reveal(i, self.m.offset(zone));
            self.m.offset.insert(zone, o);
        }
    }

    fn step(&mut self, dir: Dir) -> bool {
        let Some(z) = self.zone(self.m.zone) else { return false };
        let (n, cols, i) = (z.items.len(), z.cols(), self.m.sel(z.id));
        let horizontal = z.horizontal();
        let linear = cols > 1 || horizontal;
        let next = |i: usize| -> Option<usize> {
            match dir {
                Dir::Left if linear && cols > 1 => i.checked_sub(1),
                Dir::Right if linear && cols > 1 => (i + 1 < n).then_some(i + 1),
                Dir::Left if horizontal => i.checked_sub(1),
                Dir::Right if horizontal => (i + 1 < n).then_some(i + 1),
                Dir::Up if !horizontal => i.checked_sub(cols),
                Dir::Down if !horizontal => {
                    if i + cols < n {
                        Some(i + cols)
                    } else if i / cols < (n.saturating_sub(1)) / cols {
                        Some(n - 1)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };
        let mut j = next(i);
        while let Some(k) = j {
            if z.items[k].focusable() {
                break;
            }
            j = next(k);
        }
        if let Some(k) = j {
            let id = z.id;
            self.focus(id, k);
            return true;
        }
        self.leave(dir)
    }

    /// Moves the focus to the nearest zone in `dir`, onto its item nearest the current one.
    fn leave(&mut self, dir: Dir) -> bool {
        let Some(z) = self.zone(self.m.zone) else { return false };
        let from = z.rect(self.m.sel(z.id), self.m.offset(z.id)).unwrap_or_else(|| z.area());
        let (fx, fy) = (from.x + from.w / 2.0, from.y + from.h / 2.0);
        let mut best: Option<(f32, &'static str, usize)> = None;
        for other in self.zones.iter().filter(|o| o.id != z.id) {
            let a = other.area();
            let ahead = match dir {
                Dir::Left => a.right() <= from.x + 1.0,
                Dir::Right => a.x >= from.right() - 1.0,
                Dir::Up => a.bottom() <= from.y + 1.0,
                Dir::Down => a.y >= from.bottom() - 1.0,
            };
            if !ahead {
                continue;
            }
            let offset = self.m.offset(other.id);
            for i in other.visible(offset) {
                if !other.items[i].focusable() {
                    continue;
                }
                let Some(r) = other.rect(i, offset) else { continue };
                let (dx, dy) = (r.x + r.w / 2.0 - fx, r.y + r.h / 2.0 - fy);
                // distance along the direction counts once, across it three times
                let d = match dir {
                    Dir::Left | Dir::Right => dx.abs() + 3.0 * dy.abs(),
                    Dir::Up | Dir::Down => dy.abs() + 3.0 * dx.abs(),
                };
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, other.id, i));
                }
            }
        }
        match best {
            Some((_, id, i)) => {
                self.focus(id, i);
                true
            }
            None => false,
        }
    }

    fn cycle(&mut self, back: bool) -> bool {
        let ids: Vec<&'static str> = self.zones.iter().filter(|z| z.items.iter().any(|t| t.focusable())).map(|z| z.id).collect();
        if ids.len() < 2 {
            return false;
        }
        let at = ids.iter().position(|&id| id == self.m.zone).unwrap_or(0);
        let next = if back { (at + ids.len() - 1) % ids.len() } else { (at + 1) % ids.len() };
        let id = ids[next];
        // land on the first visible item of the zone (or where its focus was, when that is visible)
        let z = self.zone(id).expect("listed above");
        let offset = self.m.offset(id);
        let keep = self.m.sel(id);
        let i = if z.visible(offset).contains(&keep) { keep } else { z.visible(offset).start };
        self.focus(id, i);
        self.skip_unfocusable(true);
        true
    }

    pub(super) fn refilter(&mut self) {
        self.m.results = self.m.cat.search(self.m.query());
        self.m.sel.insert("results", 0);
        self.m.offset.insert("results", 0);
        self.relayout();
        if self.m.searching() && self.zone("results").is_some() {
            self.m.zone = "results";
        }
    }

    /// What a click, Enter or an accessibility `activate` on `t` (found in zone `from`) does.
    pub(super) fn activate(&mut self, t: Target, from: &'static str) -> Response {
        // Detail actions (Windows' best-match pane) act on the result selected now, never on a stale layout.
        let t = match (from, t, self.m.results.get(self.m.sel("results")).copied()) {
            ("detail", Target::App(_), Some(cur)) => Target::App(cur),
            ("detail", Target::Pin(_), Some(cur)) => Target::Pin(cur),
            _ => t,
        };
        match t {
            Target::App(i) => {
                if self.m.cat.launch(i) {
                    Response::act(Action::Close)
                } else {
                    Response::NONE
                }
            }
            Target::Letter(_) => Response::NONE,
            Target::View(v) => {
                self.m.view = v;
                for id in ["apps", "results"] {
                    self.m.sel.insert(id, 0);
                    self.m.offset.insert(id, 0);
                }
                if !self.m.field.text().is_empty() {
                    self.m.field.clear();
                    self.m.results.clear();
                }
                self.relayout();
                // a mode switch (Windows All apps / Back) hands the focus to the list it opened
                if from == "nav" && self.zone("apps").is_some() {
                    self.m.zone = "apps";
                    self.skip_unfocusable(true);
                }
                Response::REDRAW
            }
            Target::Page(p) => {
                self.m.offset.insert("apps", p);
                if let Some(z) = self.zone("apps") {
                    let first = z.first(p);
                    self.m.sel.insert("apps", first.min(z.items.len().saturating_sub(1)));
                }
                Response::REDRAW
            }
            Target::Power | Target::Settings | Target::Account | Target::Computer => {
                // (unit tests never start another program)
                if let Some(argv) = sidebar_argv(t, crate::controlcenter::sys::have).filter(|_| !cfg!(test)) {
                    crate::launch::spawn_argv(&argv);
                }
                Response::act(Action::Close)
            }
            Target::Fullscreen => {
                // the search text is kept; the grids start from the top
                self.m.expanded = !self.m.expanded;
                for id in ["apps", "results", "fav"] {
                    self.m.sel.insert(id, 0);
                    self.m.offset.insert(id, 0);
                }
                self.resize(self.surface.0, self.surface.1);
                Response::REDRAW
            }
            Target::Pin(i) => {
                self.m.cat.toggle_favorite(i);
                self.relayout();
                Response::REDRAW
            }
            Target::Clear => {
                self.m.field.clear();
                self.refilter();
                Response::REDRAW
            }
            Target::Close => Response::act(Action::Close),
            Target::Style(kind, i) => {
                let outcome = match self.style {
                    LauncherStyle::Kde => kde::activate(&mut self.m, kind, i),
                    _ => Outcome::Nothing,
                };
                match outcome {
                    Outcome::Nothing => Response::NONE,
                    Outcome::Redraw => Response::REDRAW,
                    Outcome::Relayout => {
                        self.relayout();
                        Response::REDRAW
                    }
                    Outcome::Close => Response::act(Action::Close),
                }
            }
        }
    }

    /// The zone and item under a surface point.
    pub fn hit(&self, x: f32, y: f32) -> Option<(&'static str, usize, Target)> {
        for z in &self.zones {
            let offset = self.m.offset(z.id);
            let range = match &z.shape {
                Shape::Fixed(r) => 0..r.len(),
                Shape::Grid { .. } => z.visible(offset),
            };
            for i in range {
                if z.rect(i, offset).is_some_and(|r| r.contains(x, y)) {
                    return Some((z.id, i, z.items[i]));
                }
            }
        }
        None
    }

    pub fn key(&mut self, ev: &KeyEvent) -> Response {
        if ev.ctrl && ev.key == Key::Char {
            return match ev.ch {
                Some('u') => self.activate(Target::Clear, ""),
                Some('p') => self.toggle_pin(),
                _ => Response::NONE,
            };
        }
        self.m.keyboard = true;
        let moved = match ev.key {
            Key::Escape => {
                // a style's open menu or pending confirmation takes the key first
                if self.style == LauncherStyle::Kde && kde::escape(&mut self.m) {
                    self.relayout();
                    return Response::REDRAW;
                }
                return Response::act(Action::Close);
            }
            Key::Enter => {
                let from = self.m.zone;
                return match self.current() {
                    Some(t) if t.focusable() => self.activate(t, from),
                    _ => match self.m.results.first() {
                        Some(&i) if self.m.searching() => self.activate(Target::App(i), "results"),
                        _ => Response::NONE,
                    },
                };
            }
            Key::Tab => self.cycle(ev.shift),
            Key::Left => self.step(Dir::Left),
            Key::Right => self.step(Dir::Right),
            Key::Up => self.step(Dir::Up),
            Key::Down => self.step(Dir::Down),
            Key::PageUp | Key::PageDown => {
                let Some(z) = self.zone(self.m.zone) else { return Response::NONE };
                let jump = z.per_view();
                let i = self.m.sel(z.id);
                let k = if ev.key == Key::PageDown { (i + jump).min(z.items.len().saturating_sub(1)) } else { i.saturating_sub(jump) };
                let id = z.id;
                self.focus(id, k);
                self.skip_unfocusable(ev.key == Key::PageDown);
                true
            }
            Key::Home | Key::End if !self.m.searching() => {
                let Some(z) = self.zone(self.m.zone) else { return Response::NONE };
                let (id, k) = (z.id, if ev.key == Key::Home { 0 } else { z.items.len().saturating_sub(1) });
                self.focus(id, k);
                self.skip_unfocusable(ev.key == Key::Home);
                true
            }
            Key::Home => {
                self.m.field.home();
                true
            }
            Key::End => {
                self.m.field.end();
                true
            }
            Key::Backspace => {
                self.m.field.backspace();
                self.refilter();
                true
            }
            Key::Delete => {
                self.m.field.delete();
                self.refilter();
                true
            }
            Key::Char if !ev.alt && !ev.text.is_empty() && !ev.text.chars().any(char::is_control) => {
                self.m.field.insert(&ev.text);
                self.refilter();
                true
            }
            Key::Char => false,
        };
        if moved {
            // layouts may depend on the selection (Windows' detail pane)
            self.relayout();
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn toggle_pin(&mut self) -> Response {
        match self.current() {
            Some(Target::App(i)) => {
                self.m.cat.toggle_favorite(i);
                self.relayout();
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    pub fn motion(&mut self, x: f32, y: f32) -> Response {
        match self.hit(x, y) {
            Some((zone, i, t)) if t.focusable() && (!self.m.focused(zone, i) || self.m.keyboard) => {
                self.m.keyboard = false;
                self.m.zone = zone;
                self.m.sel.insert(zone, i);
                // layouts may depend on the selection (Windows' detail pane)
                self.relayout();
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    pub fn button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed {
            return Response::NONE;
        }
        if self.full && !self.card.contains(x, y) {
            return Response::act(Action::Close);
        }
        let hit = self.hit(x, y);
        // a style's open menu or pending confirmation consumes the press that is not on it
        if self.style == LauncherStyle::Kde && kde::press(&mut self.m, hit.map(|h| h.0)) {
            self.relayout();
            return Response::REDRAW;
        }
        let Some((zone, i, t)) = hit else { return Response::NONE };
        match button {
            Button::Left => {
                self.m.keyboard = false;
                if t.focusable() {
                    self.m.zone = zone;
                    self.m.sel.insert(zone, i);
                }
                self.activate(t, zone)
            }
            Button::Right if matches!(t, Target::App(_)) => {
                self.m.zone = zone;
                self.m.sel.insert(zone, i);
                self.toggle_pin()
            }
            _ => Response::NONE,
        }
    }

    /// Wheel: pages a paged grid one page per notch, scrolls a list by rows.
    pub fn scroll(&mut self, x: f32, y: f32, dy: f32) -> Response {
        let under = self.zones.iter().find(|z| z.area().contains(x, y) && matches!(z.shape, Shape::Grid { .. }));
        let Some(z) = under.or_else(|| self.zone("results").or_else(|| self.zone("apps"))) else { return Response::NONE };
        let (id, max, before, paged) = (z.id, z.max_offset(), self.m.offset(z.id), z.paged());
        // never more than a screenful less one row per event, so a short list does not skip items
        let page_rows = match z.shape {
            Shape::Grid { rows, .. } => rows.saturating_sub(1).max(1),
            Shape::Fixed(_) => 1,
        };
        if self.wheel.signum() != dy.signum() {
            self.wheel = 0.0;
        }
        self.wheel += dy;
        let notch = 50.0;
        let steps = if paged { (self.wheel.abs() >= notch) as usize } else { ((self.wheel.abs() / notch) as usize).min(page_rows) };
        if steps == 0 {
            return Response::NONE;
        }
        self.wheel = if paged { 0.0 } else { self.wheel % notch };
        let after = if dy > 0.0 { (before + steps).min(max) } else { before.saturating_sub(steps) };
        if after == before {
            return Response::NONE;
        }
        self.m.offset.insert(id, after);
        // keep the focus on screen
        let z = self.zone(id).expect("found above");
        let visible = z.visible(after);
        let sel = self.m.sel(id);
        if !visible.contains(&sel) && !visible.is_empty() {
            self.m.sel.insert(id, if sel < visible.start { visible.start } else { visible.end - 1 });
        }
        self.relayout();
        Response::REDRAW
    }

    pub fn draw(&mut self, pm: &mut Pixmap) {
        pm.fill(tiny_skia::Color::TRANSPARENT);
        let mut p = paint::Painter { pm, fonts: self.fonts.as_mut(), icons: &mut self.icons, s: self.scale, alpha: self.alpha };
        match self.style {
            LauncherStyle::Windows => windows::paint(&self.m, &self.zones, self.card, &mut p),
            LauncherStyle::Gnome => gnome::paint(&self.m, &self.zones, self.card, &mut p),
            LauncherStyle::Kde => kde::paint(&self.m, &self.zones, self.card, &mut p),
            LauncherStyle::Ukui => ukui::paint(&self.m, &self.zones, self.card, &mut p),
            LauncherStyle::Xfce => xfce::paint(&self.m, &self.zones, self.card, &mut p),
            LauncherStyle::BlackBerry => blackberry::paint(&self.m, &self.zones, self.card, &mut p),
            LauncherStyle::Default | LauncherStyle::Dux => {}
        }
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }
}

/// How assistive technology treats a control a style owns itself ([`Target::Style`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// A tab or a sidebar choice: one of several, the current one is selected.
    Tab,
    Button,
    /// A row of a menu the style opened.
    MenuItem,
    /// A row of a list the style fills itself; activating it opens it.
    Row,
}

/// What a style says about one of its own controls, for assistive technology. Read only: it describes what the
/// style draws and does, and changes neither.
#[derive(Debug, Clone, PartialEq)]
pub struct Semantics {
    /// Stable within a zone, from what the control is and never from its position (`tab:places`, `sys:shut-down`).
    pub key: String,
    pub control: Control,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// The current tab or choice.
    pub selected: bool,
    /// `Some(open)` for a button that opens a menu.
    pub popup: Option<bool>,
}

impl Styled {
    /// The accessible description of a style-owned control; `None` for anything else, and for a control whose style
    /// does not describe it yet (the accessibility tests fail for every such control, so none goes unnoticed).
    pub fn semantics(&self, t: Target) -> Option<Semantics> {
        let Target::Style(kind, i) = t else { return None };
        match self.style {
            LauncherStyle::Kde => kde::describe(&self.m, kind, i),
            _ => None,
        }
    }

    /// Something the style is still loading in the background, in words (Kickoff's Places).
    pub fn busy(&self) -> Option<&'static str> {
        match self.style {
            LauncherStyle::Kde => kde::busy(&self.m),
            _ => None,
        }
    }
}

/// Test support for the accessibility tests: Places data for Kickoff, a Places worker that stays busy, and what the
/// style's controls did since the last call (commands "opened", system actions "run"; nothing really starts).
#[cfg(test)]
pub(super) fn kde_install_places(computer: Vec<Entry>, documents: Vec<Entry>) {
    kde::install_places(computer, documents)
}

#[cfg(test)]
pub(super) fn kde_hold_places(gate: std::sync::mpsc::Receiver<()>) {
    kde::hold_places(gate)
}

#[cfg(test)]
pub(super) fn kde_effects() -> (Vec<Vec<String>>, Vec<&'static str>) {
    kde::effects()
}

#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
mod wheel_tests {
    use super::*;

    #[test]
    fn one_notch_never_skips_rows_a_short_list_cannot_show() {
        let mut st = tests::styled(LauncherStyle::Xfce, 40);
        st.full = true;
        st.resize(800, 480);
        let a = st.zones.iter().find(|z| z.id == "apps").unwrap().area();
        // a list squeezed to one visible row (the engine relayouts after scrolling, keeping the new offset)
        st.zones = vec![Zone::columns("apps", (0..10).map(Target::App).collect(), Rect { h: 30.0, ..a }, 1, 30.0, false)];
        st.scroll(a.x + 2.0, a.y + 2.0, 100.0);
        assert_eq!(st.m.offset("apps"), 1, "one X11 notch (100 px) moves one row when one row shows");
    }
}
