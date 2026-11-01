//! The launcher's accessibility adapter (AT-SPI2): the semantic tree of what the launcher shows in the style it runs
//! in, and the narrow dispatcher that runs assistive-technology requests through the same state changes the keyboard
//! and pointer cause. Nothing here synthesises pointer or key events.
//!
//! One adapter serves every style. The Launchpad and Dux are described from their own grid and list layout; the
//! Windows, GNOME, KDE, Xfce, BlackBerry and UKUI styles from the engine's [`Zone`]s, i.e. the geometry the style
//! really laid out and painted, so a style that adds or moves a control is described without a change here.
//!
//! Keys (stable node ids): `frame`, `search`, `search:clear` (Dux), `status`, `announce:empty`, `announce:loading`,
//! `zone:<zone>` (a list, tab strip or group; `zone:<zone>/list` for the apps inside a mixed group) and, for the
//! things inside a zone (`results` counts as `apps`: search results replace the list), `<zone>/app:<desktop id>`, `<zone>/letter:<c>`, `<zone>/view:<view>`, `<zone>/page:<n>`,
//! `detail/open:<desktop id>`, `detail/pin:<desktop id>`, `<zone>/settings|power|account|computer|fullscreen|clear|close`, and the style's own `<zone>/<semantic key>` (`footer/tab:places`,
//! `footer/sys:shut-down`, `menu/menu:log-out`, `cats/place:history`, ...).
//! Apps are always keyed by their desktop-file id (never by list position), so a key survives searching, reordering,
//! pinning and scrolling; a second app with the same id gets a `#n` suffix in catalog order.
//!
//! Only what is on screen is described (a scrolled-off row or a page that is not showing has no node, hence no
//! action); every request is re-resolved against the live model before it acts, so one that raced a keystroke is
//! refused instead of acting on whatever moved into its place.
//!
//! The controls a style owns itself (`Target::Style`: Kickoff's footer tabs, system buttons, Session button and menu,
//! header buttons, Places sidebar and rows) are described by the style (`Styled::semantics`: stable key, role, name,
//! state) and run by the engine's own `Styled::activate`, like every other control; an open Session menu is a modal
//! `menu` whose rows are operable while everything else is refused. A style that adds such controls must describe
//! them in `Styled::semantics`: the tests fail for any `Target::Style` it leaves undescribed.

use super::catalog::app_id;
use super::placement::Rect as PRect;
use super::style::LauncherStyle;
use super::styled::{Control, Model, Shape, Styled, Target as Tgt, View, Zone};
use super::{Launcher, CELL_H, CELL_W, GRID_Y, PAD, SEARCH_H, SEARCH_Y};
use crate::a11y::{ActionDef, ActionError, NodeId, Op, Role, Spec, State, Target, TextInfo, TreeBuilder};
use crate::apps::AppEntry;
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response, WindowApp};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

/// Where the person's keyboard acts as far as a screen reader is concerned: typing goes to the search field, the
/// arrow keys move the selection in the lists. The launcher itself has one selection and always-live text input;
/// this says which of the two the last key was for, so the screen reader reads the field while typing and the
/// highlighted app while navigating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Area {
    #[default]
    Search,
    List,
}

/// The children of a list or tab strip in the last tree (`Selection.SelectChild` counts in children).
#[derive(Debug, Clone, Default)]
struct Group {
    /// Choosing a child shows it (tabs) rather than only moving the selection onto it (lists).
    tabs: bool,
    children: Vec<String>,
}

type Lists = HashMap<String, Group>;

/// What the adapter remembers between trees; lives in [`Launcher`].
#[derive(Debug, Default)]
pub(super) struct Memory {
    area: Area,
    lists: Lists,
}

/// The order zones are read in; zones a style adds that are not listed here follow in the style's own order.
const ORDER: [&str; 13] = ["menu", "nav", "tabs", "cats", "detail", "results", "apps", "recommended", "fav", "pages", "dash", "footer", "actions"];

/// Longest description taken from a desktop entry's `Comment=`.
const MAX_COMMENT: usize = 200;

// ------------------------------------------------------------------------------------------------ shared pieces

fn ints(x: f32, y: f32, w: f32, h: f32) -> crate::a11y::Rect {
    (x.round() as i32, y.round() as i32, w.round().max(0.0) as i32, h.round().max(0.0) as i32)
}

fn prect(r: &PRect) -> crate::a11y::Rect {
    ints(r.x, r.y, r.w, r.h)
}

/// The desktop ids of `apps` as node keys: the id, or `id#n` for the n-th app (in list order) that shares it.
fn app_keys(apps: &[AppEntry]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    apps.iter()
        .map(|a| {
            let id = app_id(a);
            let n = seen.entry(id.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                id
            } else {
                format!("{id}#{n}")
            }
        })
        .collect()
}

/// Whether starting the app can work at all (a missing `TryExec` program or an empty command cannot); the same
/// check the launch itself makes first.
fn launchable(app: &AppEntry) -> bool {
    crate::launch::plan_app(app).error.is_none()
}

fn describe(app: &AppEntry, pinned: bool) -> String {
    let comment: String = app.comment.chars().filter(|c| !c.is_control()).take(MAX_COMMENT).collect();
    match (pinned, comment.is_empty()) {
        (true, true) => "Pinned".into(),
        (true, false) => format!("Pinned. {comment}"),
        (false, _) => comment,
    }
}

fn placeholder(style: LauncherStyle) -> &'static str {
    match style {
        LauncherStyle::Dux | LauncherStyle::Kde => "Search…",
        LauncherStyle::Windows => "Search for apps",
        LauncherStyle::Gnome => "Type to search",
        LauncherStyle::Xfce => "Search Action",
        LauncherStyle::BlackBerry => "Search",
        LauncherStyle::Ukui => "Search App",
        _ => "Search apps",
    }
}

fn view_key(v: View) -> String {
    match v {
        View::Home => "home".into(),
        View::All => "all".into(),
        View::Favorites => "favorites".into(),
        View::Recent => "recent".into(),
        View::Category(c) => format!("category:{}", c.title()),
    }
}

/// The view a style marks as current (its `shown`): the start view stands for a different list in each style.
fn current_view(style: LauncherStyle, m: &Model) -> View {
    match (style, m.view) {
        (LauncherStyle::Kde | LauncherStyle::Xfce, View::Home) => View::Favorites,
        (LauncherStyle::BlackBerry, View::Home) => View::All,
        (LauncherStyle::Ukui, View::Recent) => View::Recent,
        (LauncherStyle::Ukui, _) => View::Favorites,
        (_, v) => v,
    }
}

/// The list that holds the user's pins, in the styles where the apps zone shows them.
fn is_pin_list(st: &Styled, zone: &str) -> bool {
    match zone {
        "fav" | "dash" => true,
        "apps" => matches!(st.style, LauncherStyle::Windows | LauncherStyle::Kde | LauncherStyle::Xfce) && !st.m.searching() && matches!(st.m.view, View::Home | View::Favorites),
        _ => false,
    }
}

impl Launcher {
    fn app_list(&self) -> &[AppEntry] {
        match &self.styled {
            Some(st) => &st.m.cat.apps,
            None => &self.apps,
        }
    }

    fn field_ref(&self) -> &TextField {
        match &self.styled {
            Some(st) => &st.m.field,
            None => &self.field,
        }
    }

    /// Keeps the screen reader's focus (search field or list) in step with where the last key acted.
    pub(super) fn a11y_note_key(&mut self, ev: &KeyEvent) {
        let searching = !self.field_ref().text().trim().is_empty();
        let to = match ev.key {
            Key::Char if ev.ctrl && ev.ch == Some('u') => Some(Area::Search),
            Key::Char if ev.ctrl || ev.alt || ev.text.is_empty() => None,
            Key::Char | Key::Backspace | Key::Delete => Some(Area::Search),
            Key::Home | Key::End => Some(if self.styled.is_some() && !searching { Area::List } else { Area::Search }),
            Key::Left | Key::Right if self.is_dux() => Some(Area::Search),
            Key::Tab if self.is_dux() => None,
            Key::Up | Key::Down | Key::Left | Key::Right | Key::Tab | Key::PageUp | Key::PageDown => Some(Area::List),
            Key::Enter | Key::Escape => None,
        };
        if let Some(a) = to {
            self.a11y.area = a;
        }
    }
}

// ------------------------------------------------------------------------------------------------ what is shown

/// One actionable or list-like thing on screen, as both the tree and the dispatcher see it.
#[derive(Debug, Clone)]
struct Entry {
    key: String,
    zone: &'static str,
    /// Index in the zone's items (styled) or in the shown list (Launchpad and Dux).
    pos: usize,
    target: Tgt,
    rect: crate::a11y::Rect,
}

fn target_key(zone: &str, t: Tgt, ids: &[String], style: Option<&Styled>) -> Option<String> {
    // an app is the same object in the list and in the results that replace it
    let zone = if zone == "results" { "apps" } else { zone };
    Some(match t {
        Tgt::App(i) => format!("{zone}/{}:{}", if zone == "detail" { "open" } else { "app" }, ids.get(i)?),
        Tgt::Pin(i) => format!("{zone}/pin:{}", ids.get(i)?),
        Tgt::Letter(c) => format!("{zone}/letter:{c}"),
        Tgt::View(v) => format!("{zone}/view:{}", view_key(v)),
        Tgt::Page(p) => format!("{zone}/page:{p}"),
        Tgt::Settings => format!("{zone}/settings"),
        Tgt::Power => format!("{zone}/power"),
        Tgt::Account => format!("{zone}/account"),
        Tgt::Computer => format!("{zone}/computer"),
        Tgt::Fullscreen => format!("{zone}/fullscreen"),
        Tgt::Clear => format!("{zone}/clear"),
        Tgt::Close => format!("{zone}/close"),
        // owned by one style, which says what it is called
        Tgt::Style(..) => format!("{zone}/{}", style?.semantics(t)?.key),
    })
}

fn styled_entries(st: &Styled, ids: &[String]) -> Vec<Entry> {
    let mut out = Vec::new();
    for z in &st.zones {
        let offset = st.m.offset(z.id);
        let range = match &z.shape {
            Shape::Fixed(_) => 0..z.items.len(),
            Shape::Grid { .. } => z.visible(offset),
        };
        for pos in range {
            let (Some(r), Some(&t)) = (z.rect(pos, offset), z.items.get(pos)) else { continue };
            let rect = prect(&r);
            let Some(key) = target_key(z.id, t, ids, Some(st)) else { continue };
            if rect.2 == 0 || rect.3 == 0 {
                continue;
            }
            out.push(Entry { key, zone: z.id, pos, target: t, rect });
        }
    }
    // two rows that read the same (equal Places entries) are told apart by order, as the tree builder would
    let mut seen: HashMap<String, usize> = HashMap::new();
    for e in &mut out {
        let n = seen.entry(e.key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            e.key = format!("{}#{n}", e.key);
        }
    }
    out
}

fn classic_entries(l: &Launcher, ids: &[String]) -> Vec<Entry> {
    let cols = l.cols();
    let last = ((l.scroll + l.visible_rows()) * cols).min(l.shown.len());
    let first = (l.scroll * cols).min(last);
    let mut out = Vec::new();
    for pos in first..last {
        let Some(&i) = l.shown.get(pos) else { continue };
        let Some(id) = ids.get(i) else { continue };
        let (x, y, w, h) = l.cell_rect(pos);
        let rect = ints(x + l.origin.0, y + l.origin.1, w, h);
        if rect.2 == 0 || rect.3 == 0 {
            continue;
        }
        out.push(Entry { key: format!("apps/app:{id}"), zone: "apps", pos, target: Tgt::App(i), rect });
    }
    if l.is_dux() && !l.field.text().is_empty() {
        let w = l.dux.as_ref().map_or(l.size.0 as f32, |s| s.layout.width);
        out.push(Entry { key: "search:clear".into(), zone: "search", pos: 0, target: Tgt::Clear, rect: ints(l.origin.0 + w - 52.0, l.origin.1 + 12.0, 40.0, 48.0) });
    }
    out
}

fn entries(l: &Launcher, ids: &[String]) -> Vec<Entry> {
    match &l.styled {
        Some(st) => styled_entries(st, ids),
        None => classic_entries(l, ids),
    }
}

fn has_menu(st: &Styled) -> bool {
    st.zones.iter().any(|z| z.id == "menu")
}

fn surface(l: &Launcher) -> (u32, u32) {
    match &l.styled {
        Some(st) => st.surface,
        None if l.overlay && l.screen.0 > 0 && l.screen.1 > 0 => l.screen,
        None => l.size,
    }
}

// ------------------------------------------------------------------------------------------------ the tree

/// Describes the launcher as it was last drawn.
pub(super) fn tree(l: &mut Launcher, b: &mut TreeBuilder) {
    let ids = app_keys(l.app_list());
    let es = entries(l, &ids);
    let mut lists = Lists::new();
    build(l, &es, b, &mut lists);
    l.a11y.lists = lists;
}

fn build(l: &Launcher, es: &[Entry], b: &mut TreeBuilder, lists: &mut Lists) {
    let (sw, sh) = surface(l);
    let Some(frame) = b.add(NodeId::ROOT, Spec::new("frame", Role::Frame, l.title()).bounds(Some(ints(0.0, 0.0, sw as f32, sh as f32))).state(State::Showing, true)) else { return };
    match &l.styled {
        Some(st) => styled_tree(st, es, b, frame, lists),
        None => classic_tree(l, es, b, frame, lists),
    }
    b.focus(&focus_key(l, es));
}

fn search_spec(f: &TextField, bounds: Option<crate::a11y::Rect>, placeholder: &str) -> Spec {
    Spec::new("search", Role::Entry, "Search apps")
        .bounds(bounds)
        .state(State::Showing, true)
        .state(State::Focusable, true)
        .state(State::Editable, true)
        .state(State::SingleLine, true)
        .state(State::SelectableText, true)
        .text(TextInfo::plain(f.text(), f.cursor(), f.selection(), true))
        .attr("placeholder-text", placeholder)
        .action(ActionDef::new("activate", "Open the selected app").key("Return"))
}

/// The result count (a status line, read on demand) and, while a search finds nothing, a one-off announcement.
/// Typing itself is not announced and neither is a count that merely changed: only reaching "no matches" is.
fn status(b: &mut TreeBuilder, frame: NodeId, count: usize, searching: bool) {
    if !searching {
        return;
    }
    let text = match count {
        0 => "No matching apps".to_string(),
        1 => "1 result".to_string(),
        n => format!("{n} results"),
    };
    b.add(frame, Spec::new("status", Role::StatusBar, text).state(State::Showing, true));
    if count == 0 {
        b.add(frame, Spec::new("announce:empty", Role::Notification, "No matching apps"));
    }
}

fn container(key: &str, role: Role, name: &str, bounds: Option<crate::a11y::Rect>) -> Spec {
    let spec = Spec::new(key, role, name).bounds(bounds).state(State::Showing, true);
    if matches!(role, Role::List | Role::PageTabList | Role::Menu) {
        spec.selection()
    } else {
        spec
    }
}

fn app_item(key: &str, app: &AppEntry, rect: crate::a11y::Rect, selected: bool, (rank, total): (usize, usize), pin: Option<bool>, say_pinned: bool) -> Spec {
    let ok = launchable(app);
    let mut s = Spec::new(key, Role::ListItem, app.name.as_str())
        .description(describe(app, say_pinned && pin == Some(true)))
        .bounds(Some(rect))
        .enabled(ok)
        .state(State::Showing, true)
        .state(State::Focusable, true)
        .state(State::Selectable, true)
        .state(State::Selected, selected)
        .attr("posinset", rank.to_string())
        .attr("setsize", total.to_string());
    if ok {
        let open = ActionDef::new("activate", format!("Open {}", app.name));
        s = s.action(if selected { open.key("Return") } else { open });
        match pin {
            Some(true) => s = s.action(ActionDef::new("unpin", "Remove from favorites")),
            Some(false) => s = s.action(ActionDef::new("pin", "Add to favorites")),
            None => {}
        }
    }
    s
}

fn classic_tree(l: &Launcher, es: &[Entry], b: &mut TreeBuilder, frame: NodeId, lists: &mut Lists) {
    let (ox, oy) = l.origin;
    let dux = l.is_dux();
    let width = if dux { l.dux.as_ref().map_or(l.size.0 as f32, |s| s.layout.width) } else { l.size.0 as f32 };
    let search = if dux { ints(ox + 12.0, oy + 12.0, width - 24.0, 48.0) } else { ints(ox + PAD, oy + SEARCH_Y, width - 2.0 * PAD, SEARCH_H) };
    b.add(frame, search_spec(&l.field, Some(search), placeholder(l.style)));
    if let Some(e) = es.iter().find(|e| e.key == "search:clear") {
        b.add(
            frame,
            Spec::new(e.key.as_str(), Role::PushButton, "Clear search")
                .bounds(Some(e.rect))
                .state(State::Showing, true)
                .state(State::Focusable, true)
                .action(ActionDef::new("click", "Clear the search text")),
        );
    }
    let searching = !l.field.text().is_empty();
    status(b, frame, l.shown.len(), searching || l.shown.is_empty());
    let bounds = if dux {
        l.dux.as_ref().map(|s| {
            let (x, y, w, _) = s.layout.cell(0, 0);
            ints(ox + x, oy + y, w, s.layout.rows as f32 * (s.layout.row + 2.0))
        })
    } else {
        let h = (l.visible_rows() as f32 * CELL_H).min(l.size.1 as f32 - GRID_Y);
        Some(ints(ox + l.grid_x(), oy + GRID_Y, l.cols() as f32 * CELL_W, h))
    };
    let name = if searching { "Search results" } else { "Applications" };
    let Some(list) = b.add(frame, container("zone:apps", Role::List, name, bounds)) else { return };
    let mut kids = Vec::new();
    for e in es.iter().filter(|e| e.zone == "apps") {
        let Tgt::App(i) = e.target else { continue };
        let Some(app) = l.apps.get(i) else { continue };
        if b.add(list, app_item(&e.key, app, e.rect, e.pos == l.sel, (e.pos + 1, l.shown.len()), None, false)).is_some() {
            kids.push(e.key.clone());
        }
    }
    lists.insert("zone:apps".into(), Group { tabs: false, children: kids });
}

fn zone_name(st: &Styled, id: &str) -> String {
    let m = &st.m;
    match id {
        "results" => "Search results".into(),
        "apps" => match st.style {
            LauncherStyle::Windows if m.view == View::Home => "Pinned apps".into(),
            LauncherStyle::Windows => "All apps".into(),
            LauncherStyle::Gnome => "Applications".into(),
            LauncherStyle::Ukui => "All applications".into(),
            LauncherStyle::Kde if m.ui("kde.tab") == 1 => "Places".into(),
            style => Model::view_title(current_view(style, m)).into(),
        },
        "recommended" => "Recommended".into(),
        "fav" => Model::view_title(current_view(st.style, m)).into(),
        "cats" => "Categories".into(),
        "tabs" => "Favorites and recent apps".into(),
        "nav" => "Navigation".into(),
        "pages" => "Pages".into(),
        "dash" => "Favorites".into(),
        "detail" => "Best match".into(),
        "footer" | "actions" => "Settings and session".into(),
        other => other.into(),
    }
}

/// What an entry is, for the role and the container it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    App,
    Heading,
    /// A row of a list the style fills itself.
    Row,
    Tab,
    Button,
    MenuItem,
}

fn kind_of(st: &Styled, e: &Entry) -> Kind {
    match e.target {
        Tgt::App(_) if e.zone != "detail" => Kind::App,
        Tgt::Letter(_) => Kind::Heading,
        Tgt::View(_) if e.zone != "nav" => Kind::Tab,
        Tgt::Page(_) => Kind::Tab,
        Tgt::Style(..) => match st.semantics(e.target).map(|s| s.control) {
            Some(Control::Tab) => Kind::Tab,
            Some(Control::MenuItem) => Kind::MenuItem,
            Some(Control::Row) => Kind::Row,
            _ => Kind::Button,
        },
        _ => Kind::Button,
    }
}

/// The name and description of the controls that are not apps.
fn target_label(st: &Styled, e: &Entry) -> (String, String) {
    let two = |a: &str, b: &str| (a.to_string(), b.to_string());
    match e.target {
        Tgt::View(v) if e.zone == "nav" => two(if v == View::Home { "Back" } else { "All apps" }, ""),
        Tgt::View(v) => two(Model::view_title(v), ""),
        Tgt::Page(p) => (format!("Page {}", p + 1), String::new()),
        Tgt::Settings => two("Settings", "Opens Decebal Settings"),
        Tgt::Power => two("Power", "Opens the session menu"),
        Tgt::Account => two("Account settings", "Opens the account settings"),
        Tgt::Computer => two("Computer", "Opens the file manager on the computer view"),
        Tgt::Fullscreen if st.m.expanded => two("Restore", "Leaves the full-screen menu"),
        Tgt::Fullscreen => two("Expand", "Fills the screen with the menu"),
        Tgt::Pin(i) if st.m.cat.is_favorite(i) => two("Unpin from favorites", ""),
        Tgt::Pin(_) => two("Pin to favorites", ""),
        Tgt::App(i) => (format!("Open {}", st.m.cat.apps.get(i).map_or("", |a| a.name.as_str())), String::new()),
        Tgt::Clear => two("Clear search", "Clears the search text"),
        Tgt::Close if st.style == LauncherStyle::Gnome => two("Show Apps", "Closes the application grid"),
        Tgt::Close => two("Close", ""),
        Tgt::Letter(_) => (String::new(), String::new()),
        Tgt::Style(..) => st.semantics(e.target).map(|s| (s.name, s.description)).unwrap_or_default(),
    }
}

fn tab_selected(st: &Styled, e: &Entry) -> bool {
    match (e.zone, e.target) {
        ("cats", Tgt::View(v)) => !st.m.searching() && v == current_view(st.style, &st.m),
        ("tabs", Tgt::View(v)) => v == current_view(st.style, &st.m),
        (_, Tgt::Page(p)) => p == st.m.offset("apps"),
        (_, Tgt::Style(..)) => st.semantics(e.target).is_some_and(|s| s.selected),
        _ => false,
    }
}

/// Whether the engine's keyboard focus is on this entry (the highlighted app, the focused tab or button).
fn focused_here(st: &Styled, e: &Entry) -> bool {
    st.m.zone == e.zone && st.m.sel(e.zone) == e.pos
}

fn tab_spec(st: &Styled, e: &Entry) -> Spec {
    let (name, desc) = target_label(st, e);
    let ok = st.semantics(e.target).is_none_or(|s| s.enabled);
    let spec = Spec::new(e.key.as_str(), Role::PageTab, name)
        .description(desc)
        .bounds(Some(e.rect))
        .enabled(ok)
        .state(State::Showing, true)
        .state(State::Focusable, true)
        .state(State::Selectable, true)
        .state(State::Selected, tab_selected(st, e));
    if ok {
        spec.action(ActionDef::new("click", "Show"))
    } else {
        spec
    }
}

fn button_spec(st: &Styled, e: &Entry) -> Spec {
    let (name, desc) = target_label(st, e);
    let sem = st.semantics(e.target);
    let ok = sem.as_ref().is_none_or(|s| s.enabled);
    let mut spec = Spec::new(e.key.as_str(), Role::PushButton, name).description(desc).bounds(Some(e.rect)).enabled(ok).state(State::Showing, true).state(State::Focusable, true);
    if let Some(open) = sem.and_then(|s| s.popup) {
        spec = spec.state(State::HasPopup, true).state(State::Expandable, true).state(State::Expanded, open).state(State::Collapsed, !open);
    }
    if ok {
        spec = spec.action(ActionDef::new("click", "Activate"));
    }
    spec
}

/// A row of a list or menu a style fills itself: opens something when activated.
fn row_spec(st: &Styled, e: &Entry, role: Role) -> Spec {
    let sem = st.semantics(e.target);
    let ok = sem.as_ref().is_none_or(|s| s.enabled);
    let (name, desc) = target_label(st, e);
    let selected = focused_here(st, e);
    let spec = Spec::new(e.key.as_str(), role, name)
        .description(desc)
        .bounds(Some(e.rect))
        .enabled(ok)
        .state(State::Showing, true)
        .state(State::Focusable, true)
        .state(State::Selectable, true)
        .state(State::Selected, selected);
    if ok {
        spec.action(if selected { ActionDef::new("click", "Open").key("Return") } else { ActionDef::new("click", "Open") })
    } else {
        spec
    }
}

fn styled_tree(st: &Styled, es: &[Entry], b: &mut TreeBuilder, frame: NodeId, lists: &mut Lists) {
    // the engine does not publish where a style puts its search box; the field is described without a rectangle
    b.add(frame, search_spec(&st.m.field, None, placeholder(st.style)));
    match st.busy() {
        // a style's background work (Kickoff reading its places) is a busy status and one polite announcement
        Some(what) => {
            b.add(frame, Spec::new("status", Role::StatusBar, what).state(State::Showing, true).state(State::Busy, true));
            b.add(frame, Spec::new("announce:loading", Role::Notification, what));
        }
        None => status(b, frame, st.m.results.len(), st.m.searching()),
    }
    let mut zones: Vec<&Zone> = st.zones.iter().collect();
    zones.sort_by_key(|z| ORDER.iter().position(|o| *o == z.id).unwrap_or(ORDER.len()));
    for z in zones {
        let mine: Vec<&Entry> = es.iter().filter(|e| e.zone == z.id).collect();
        zone_nodes(st, z, &mine, b, frame, lists);
    }
}

fn zone_nodes(st: &Styled, z: &Zone, mine: &[&Entry], b: &mut TreeBuilder, frame: NodeId, lists: &mut Lists) {
    let kinds: Vec<Kind> = mine.iter().map(|e| kind_of(st, e)).collect();
    let count = |want: &[Kind]| kinds.iter().filter(|k| want.contains(k)).count();
    let (n_list, n_tab, n_menu, n_button) = (count(&[Kind::App, Kind::Heading, Kind::Row]), count(&[Kind::Tab]), count(&[Kind::MenuItem]), count(&[Kind::Button]));
    let list_zone = matches!(z.id, "apps" | "results");
    if mine.is_empty() && !list_zone {
        return;
    }
    let area = z.area();
    let bounds = (area.w >= 1.0 && area.h >= 1.0).then(|| prect(&area));
    let name = zone_name(st, z.id);
    let key = format!("zone:{}", z.id);
    // the position of each app among the zone's apps, offscreen ones included ("3 of 40")
    let mut rank = vec![0usize; z.items.len()];
    let mut total = 0;
    for (i, t) in z.items.iter().enumerate() {
        if z.id != "detail" && matches!(t, Tgt::App(_)) {
            total += 1;
            rank[i] = total;
        }
    }
    let entries_of = |want: &[Kind]| mine.iter().zip(&kinds).filter(|(_, k)| want.contains(k)).map(|(e, _)| *e).collect::<Vec<&Entry>>();
    let items = |b: &mut TreeBuilder, parent: NodeId| -> Vec<String> {
        let mut kids = Vec::new();
        for e in entries_of(&[Kind::App, Kind::Heading, Kind::Row]) {
            let spec = match e.target {
                Tgt::Letter(c) => Spec::new(e.key.as_str(), Role::Heading, c.to_string()).bounds(Some(e.rect)).state(State::Showing, true).attr("level", "2"),
                Tgt::App(i) => {
                    let Some(app) = st.m.cat.apps.get(i) else { continue };
                    app_item(&e.key, app, e.rect, focused_here(st, e), (rank.get(e.pos).copied().unwrap_or(0), total), Some(st.m.cat.is_favorite(i)), !is_pin_list(st, z.id))
                }
                _ => row_spec(st, e, Role::ListItem),
            };
            if b.add(parent, spec).is_some() {
                kids.push(e.key.clone());
            }
        }
        kids
    };
    let tabs = |b: &mut TreeBuilder, parent: NodeId| -> Vec<String> { entries_of(&[Kind::Tab]).into_iter().filter_map(|e| b.add(parent, tab_spec(st, e)).map(|_| e.key.clone())).collect() };
    let menu = |b: &mut TreeBuilder, parent: NodeId| -> Vec<String> {
        entries_of(&[Kind::MenuItem]).into_iter().filter_map(|e| b.add(parent, row_spec(st, e, Role::MenuItem)).map(|_| e.key.clone())).collect()
    };
    let only = |n: usize| n == mine.len();
    if only(n_tab) && n_tab > 0 {
        let Some(c) = b.add(frame, container(&key, Role::PageTabList, &name, bounds)) else { return };
        let children = tabs(b, c);
        lists.insert(key, Group { tabs: true, children });
    } else if only(n_menu) && n_menu > 0 {
        // an open menu is modal: its rows are what can be operated
        let Some(c) = b.add(frame, container(&key, Role::Menu, &name, bounds).description("Press Escape to close the menu").state(State::Modal, true)) else { return };
        let children = menu(b, c);
        lists.insert(key, Group { tabs: true, children });
    } else if n_button + n_tab + n_menu == 0 {
        let Some(c) = b.add(frame, container(&key, Role::List, &name, bounds)) else { return };
        let children = items(b, c);
        lists.insert(key, Group { tabs: false, children });
    } else {
        let Some(c) = b.add(frame, container(&key, Role::Panel, &name, bounds)) else { return };
        if n_list > 0 {
            let inner = format!("{key}/list");
            if let Some(list) = b.add(c, container(&inner, Role::List, &name, bounds)) {
                let children = items(b, list);
                lists.insert(inner, Group { tabs: false, children });
            }
        }
        if n_tab > 0 {
            let inner = format!("{key}/tabs");
            if let Some(strip) = b.add(c, container(&inner, Role::PageTabList, &name, bounds)) {
                let children = tabs(b, strip);
                lists.insert(inner, Group { tabs: true, children });
            }
        }
        for e in entries_of(&[Kind::Button]) {
            b.add(c, button_spec(st, e));
        }
    }
}

/// The key of the node the keyboard acts on: the search field while typing, otherwise the selected item.
fn focus_key(l: &Launcher, es: &[Entry]) -> String {
    // an open menu holds the keyboard
    if l.a11y.area == Area::Search && !l.styled.as_ref().is_some_and(has_menu) {
        return "search".into();
    }
    match &l.styled {
        Some(st) => {
            let (zone, sel) = (st.m.zone, st.m.sel(st.m.zone));
            if let Some(e) = es.iter().find(|e| e.zone == zone && e.pos == sel) {
                e.key.clone()
            } else if es.iter().any(|e| e.zone == zone) {
                format!("zone:{zone}")
            } else {
                "search".into()
            }
        }
        None => es.iter().find(|e| e.zone == "apps" && e.pos == l.sel).map_or_else(|| "search".into(), |e| e.key.clone()),
    }
}

// ------------------------------------------------------------------------------------------------ actions

fn refused(why: impl Into<String>) -> ActionError {
    ActionError::Refused(why.into())
}

/// Text operations on the search field, exactly like the keys would edit it.
fn text_op(f: &mut TextField, op: &Op) -> Result<(), ActionError> {
    let n = f.text().chars().count();
    match op {
        Op::SetCaret(o) => f.set_cursor(*o),
        Op::SetSelection(Some((a, z))) => f.set_selection(*a, *z),
        Op::SetSelection(None) => {
            let c = f.cursor();
            f.set_cursor(c);
        }
        Op::SetText(t) => f.set(&t.chars().filter(|c| !c.is_control()).collect::<String>()),
        Op::InsertText { offset, text } => {
            f.set_cursor((*offset).min(n));
            f.insert(text);
        }
        Op::DeleteText { start, end, cut } => {
            f.set_selection((*start).min(n), (*end).min(n));
            if *cut {
                if let Some(t) = f.selected_text() {
                    crate::clipboard::copy(&t);
                }
            }
            f.delete_selection();
        }
        Op::CopyText { start, end } => {
            let t: String = f.text().chars().skip(*start).take(end.saturating_sub(*start)).collect();
            crate::clipboard::copy(&t);
        }
        Op::PasteText { offset } => {
            let t = crate::settings::configpane::system_clipboard().unwrap_or_default();
            f.set_cursor((*offset).min(n));
            f.insert(t.lines().next().unwrap_or(""));
        }
        _ => return Err(ActionError::Unsupported("not a text operation".into())),
    }
    Ok(())
}

fn is_text_op(op: &Op) -> bool {
    matches!(op, Op::SetCaret(_) | Op::SetSelection(_) | Op::SetText(_) | Op::InsertText { .. } | Op::DeleteText { .. } | Op::CopyText { .. } | Op::PasteText { .. })
}

/// Edits the search field; the lists follow when the text changed, like typing.
fn edit(l: &mut Launcher, op: &Op) -> Result<(), ActionError> {
    l.a11y.area = Area::Search;
    if let Some(st) = l.styled.as_mut() {
        st.cancel_pending();
        st.m.keyboard = true;
        let before = st.m.field.text().to_string();
        text_op(&mut st.m.field, op)?;
        if st.m.field.text() != before {
            st.refilter();
            st.relayout();
        }
        return Ok(());
    }
    let before = l.field.text().to_string();
    text_op(&mut l.field, op)?;
    if l.field.text() != before {
        l.refilter();
    }
    Ok(())
}

/// Enter: opens the selected app (the first result while the focus is not on a list).
fn enter(l: &mut Launcher) -> Result<Response, ActionError> {
    if let Some(st) = l.styled.as_mut() {
        let (from, t) = match st.current() {
            Some(t @ Tgt::App(_)) => (st.m.zone, t),
            _ => match st.m.results.first() {
                Some(&i) if st.m.searching() => ("results", Tgt::App(i)),
                _ => return Err(refused("there is no app to open")),
            },
        };
        return run(st, from, t);
    }
    launch_current(l)
}

/// The Launchpad / Dux Enter: starts the selected app through the launcher's own `launch_selected`.
fn launch_current(l: &mut Launcher) -> Result<Response, ActionError> {
    let Some(app) = l.shown.get(l.sel).and_then(|&i| l.apps.get(i)) else { return Err(refused("there is no app to open")) };
    let name = app.name.clone();
    if !launchable(app) {
        return Err(ActionError::Disabled);
    }
    let r = l.launch_selected();
    if r.action.is_some() {
        Ok(r)
    } else {
        Err(refused(format!("{name} could not be started")))
    }
}

/// Moves a styled launcher's keyboard focus onto item `i` of `zone` (the engine's own focus).
fn move_focus(st: &mut Styled, zone: &'static str, i: usize) {
    st.focus(zone, i);
    // layouts may depend on the selection (Windows' best-match pane)
    st.relayout();
}

fn focus_entry(l: &mut Launcher, e: &Entry) {
    l.a11y.area = Area::List;
    if let Some(st) = l.styled.as_mut() {
        st.m.keyboard = true;
        if e.target.focusable() {
            move_focus(st, e.zone, e.pos);
        }
        return;
    }
    if matches!(e.target, Tgt::App(_)) {
        l.sel = e.pos;
        l.reveal_selection();
    }
}

/// A styled launcher's activation of `t` found in zone `from`: the engine's own `activate` (so a view, a page, a pin,
/// a footer button, a style's own tab or menu row all do exactly what a click on them does). Only starting an app is
/// wrapped, to tell the person why it did not start.
fn run(st: &mut Styled, from: &'static str, t: Tgt) -> Result<Response, ActionError> {
    // the best-match pane acts on the result selected now, never on a stale layout
    let t = match (from, t, st.m.results.get(st.m.sel("results")).copied()) {
        ("detail", Tgt::App(_), Some(cur)) => Tgt::App(cur),
        ("detail", Tgt::Pin(_), Some(cur)) => Tgt::Pin(cur),
        _ => t,
    };
    match t {
        Tgt::App(i) => {
            let Some(app) = st.m.cat.apps.get(i) else { return Err(ActionError::UnknownNode) };
            if !launchable(app) {
                return Err(ActionError::Disabled);
            }
            let name = app.name.clone();
            if st.m.cat.launch(i) {
                Ok(Response::act(Action::Close))
            } else {
                Err(refused(format!("{name} could not be started")))
            }
        }
        Tgt::Letter(_) => Err(ActionError::Unsupported("a letter heading has no action".into())),
        Tgt::Style(..) => match st.semantics(t) {
            Some(s) if !s.enabled => Err(ActionError::Disabled),
            Some(_) => Ok(st.activate(t, from)),
            None => Err(ActionError::Unsupported("this launcher style does not describe the control".into())),
        },
        _ => Ok(st.activate(t, from)),
    }
}

/// What a left click on the item does: focus it, then activate it.
fn activate_entry(l: &mut Launcher, e: &Entry) -> Result<Response, ActionError> {
    l.a11y.area = if matches!(e.target, Tgt::Clear) { Area::Search } else { Area::List };
    if let Some(st) = l.styled.as_mut() {
        st.m.keyboard = false;
        if e.target.focusable() {
            st.m.zone = e.zone;
            st.m.sel.insert(e.zone, e.pos);
        }
        return run(st, e.zone, e.target);
    }
    match e.target {
        Tgt::App(i) => {
            if l.shown.get(e.pos) != Some(&i) {
                return Err(ActionError::UnknownNode);
            }
            l.sel = e.pos;
            launch_current(l)
        }
        Tgt::Clear => {
            l.field.clear();
            l.refilter();
            Ok(Response::REDRAW)
        }
        _ => Err(ActionError::Unsupported("nothing to do here".into())),
    }
}

/// Pins or unpins like a right click on the item; refuses when the app already is (or is not) pinned.
fn pin_entry(l: &mut Launcher, e: &Entry, want: bool) -> Result<Response, ActionError> {
    let Some(st) = l.styled.as_mut() else { return Err(ActionError::Unsupported("this launcher style has no favorites".into())) };
    let Tgt::App(i) = e.target else { return Err(ActionError::Unsupported("only an app can be pinned".into())) };
    if e.zone == "detail" {
        return Err(ActionError::Unsupported("use the pin button".into()));
    }
    if st.m.cat.is_favorite(i) == want {
        return Err(refused(if want { "the app is already pinned" } else { "the app is not pinned" }));
    }
    st.m.keyboard = false;
    st.m.zone = e.zone;
    st.m.sel.insert(e.zone, e.pos);
    st.m.cat.toggle_favorite(i);
    st.relayout();
    Ok(Response::REDRAW)
}

/// Runs one validated request. The tree the request was checked against may be a keystroke old, so every item is
/// found again in what the launcher shows now.
pub(super) fn act(l: &mut Launcher, t: &Target) -> Result<Response, ActionError> {
    let key = t.key().to_string();
    let op = t.op.clone();
    // A modal menu is like a click outside it: only its rows and the button that opened it may act while it is open.
    let modal = op != Op::ScrollTo && l.styled.as_ref().is_some_and(has_menu);
    let modal_refusal = || refused("the session menu is open: choose one of its items or close it first");
    if key == "search" {
        if modal {
            return Err(modal_refusal());
        }
        if let Some(st) = l.styled.as_mut() {
            st.cancel_pending();
        }
        return match &op {
            Op::Focus => {
                l.a11y.area = Area::Search;
                Ok(Response::REDRAW)
            }
            Op::Action("activate") => {
                l.a11y.area = Area::Search;
                enter(l)
            }
            Op::ScrollTo => Ok(Response::NONE),
            o if is_text_op(o) => edit(l, o).map(|_| Response::REDRAW),
            o => Err(ActionError::Unsupported(format!("{o:?} on the search field"))),
        };
    }
    let ids = app_keys(l.app_list());
    let es = entries(l, &ids);
    let find = |k: &str| es.iter().find(|e| e.key == k).cloned();
    if key.starts_with("zone:") {
        if modal && key != "zone:menu" {
            return Err(modal_refusal());
        }
        return match &op {
            Op::ScrollTo => Ok(Response::NONE),
            Op::SelectChild(i) => {
                let group = l.a11y.lists.get(&key).cloned().ok_or(ActionError::UnknownNode)?;
                let child = group.children.get(*i).and_then(|k| find(k)).ok_or(ActionError::UnknownNode)?;
                if group.tabs {
                    activate_entry(l, &child)
                } else {
                    focus_entry(l, &child);
                    Ok(Response::REDRAW)
                }
            }
            o => Err(ActionError::Unsupported(format!("{o:?} on a list"))),
        };
    }
    if matches!(key.as_str(), "frame" | "status" | "announce:empty" | "announce:loading") {
        return match &op {
            Op::ScrollTo => Ok(Response::NONE),
            o => Err(ActionError::Unsupported(format!("{o:?} on {key}"))),
        };
    }
    let e = find(&key).ok_or(ActionError::UnknownNode)?;
    if modal {
        let opens_menu = l.styled.as_ref().and_then(|st| st.semantics(e.target)).is_some_and(|s| s.popup.is_some());
        if e.zone != "menu" && !opens_menu {
            return Err(modal_refusal());
        }
    }
    match &op {
        Op::ScrollTo => Ok(Response::NONE),
        Op::Focus => {
            focus_entry(l, &e);
            Ok(Response::REDRAW)
        }
        Op::Action("activate" | "click") => activate_entry(l, &e),
        Op::Action("pin") => pin_entry(l, &e, true),
        Op::Action("unpin") => pin_entry(l, &e, false),
        o => Err(ActionError::Unsupported(format!("{o:?} on {key}"))),
    }
}
