//! KDE Plasma's Kickoff: a header with the user and the search field, a sidebar of Favorites, All Applications,
//! Recently Used and the menu categories, the favourites as a grid and every other view as a list, and the footer:
//! the Applications and Places tabs on the left, the system actions (Sleep, Hibernate, Restart, Shut Down) and the
//! Session menu on the right. After plasma-desktop's applets/kickoff (Header.qml, ApplicationsPage.qml,
//! PlacesPage.qml, Footer.qml, LeaveButtons.qml; commit c68d33a9) and plasma-workspace's kicker system model
//! (dec158c0); colours come from the theme. The footer and the Places tab are the first section brought to the
//! original composition: see docs/plasma-theme-ports/kickoff-fidelity.md for the identified original capture,
//! the measurements and what still differs.

mod places;
mod system;

use super::paint::{Glyph, Painter};
use super::{Entry, Model, Outcome, Target, View, Zone};
use crate::launcher::placement::Rect;
use crate::text::Weight;
use system::Sys;

// Style-owned target kinds (`Target::Style(kind, index)`).
const TAB: u8 = 1;
/// A system action; the index is its position in `system::ALL`.
const SYS: u8 = 2;
/// The Session overflow button.
const SESSION: u8 = 3;
/// A row of the Session menu (index in `system::ALL`).
const MENU: u8 = 4;
/// A row of the Places sidebar.
const PLACE: u8 = 5;
/// A header button: 0 = the avatar (the user's account settings), 1 = configure (the launcher's settings).
const HEAD: u8 = 8;
/// A row of the Computer list (index into [`computer_rows`]).
const COMPUTER_ROW: u8 = 6;
/// A row of the History list that is a document (index into [`document_rows`]).
const DOCUMENT_ROW: u8 = 7;

// The header and sidebar, in logical pixels, measured on the same capture (docs/plasma-theme-ports/kickoff-fidelity.md, "Header and sidebar"):
// a 46 px header band (separator on row 45), the avatar button 28 px at x=9, the user's name at x=42, the search field at x=239 (4 px right of the
// sidebar's divider at 235), 338 x 30, the configure button centred 57 px from the right edge (the Keep Open slot to its right is left empty, see
// the document), sidebar rows 35 px apart inside a frame that starts 4 px from the left and is 208 px wide, the icon box at x=11 and the caption at x=44.
// Breeze's Frame_FrameRadius is 5 px (docs/plasma-theme-ports.md, "Geometry").
const HEADER_H: f32 = 46.0;
const SIDE_W: f32 = 235.0;
const ROW_PITCH: f32 = 35.0;
const ROW_RADIUS: f32 = 5.0;

// The footer, in logical pixels, measured on the identified original capture (docs/plasma-theme-ports/kickoff-fidelity.md).
const FOOTER_H: f32 = 45.0;
const TAB_X: f32 = 7.0;
const TAB_W: f32 = 107.0;
const TAB_GAP: f32 = 6.0;
const BTN_PAD: f32 = 8.0;
const ICON_BTN_W: f32 = 30.0;
const OVERFLOW_GAP: f32 = 8.0;
const ROW_H: f32 = 28.0;
const MENU_W: f32 = 168.0;

struct Frame {
    card: Rect,
    header: Rect,
    /// The header's own scale: its parts shrink together when the card is too short for them.
    hs: f32,
    avatar: Rect,
    search: Rect,
    clear: Rect,
    configure: Rect,
    side_w: f32,
    sidebar: Rect,
    content: Rect,
    footer: Rect,
}

fn frame(r: Rect, s: f32) -> Frame {
    let header = Rect { h: (HEADER_H * s).min(r.h / 5.0), ..r };
    let hs = (header.h / HEADER_H).min(s).max(0.01);
    let footer_h = (FOOTER_H * s).min(r.h / 6.0);
    let footer = Rect { y: r.bottom() - footer_h, h: footer_h, ..r };
    let side_w = (SIDE_W * s).min(r.w * 0.4);
    let body = Rect { y: header.bottom(), h: (footer.y - header.bottom()).max(1.0), ..r };
    let sidebar = Rect { x: r.x + 4.0 * s, y: body.y + 6.0 * s, w: (side_w - 27.0 * s).max(side_w * 0.6), h: (body.h - 12.0 * s).max(1.0) };
    let content = Rect { x: r.x + side_w, y: body.y + 6.0 * s, w: r.w - side_w - 6.0 * s, h: (body.h - 12.0 * s).max(1.0) };
    let avatar = Rect { x: r.x + 9.0 * hs, y: header.y + (header.h - 28.0 * hs) / 2.0, w: 28.0 * hs, h: 28.0 * hs };
    // the original has room for two buttons; a card too narrow for that moves the one it has into the right-hand slot
    let b = 32.0 * hs;
    let cx = r.right() - (if r.w >= 480.0 * hs { 57.0 } else { 22.0 }) * hs;
    let configure = Rect { x: cx - b / 2.0, y: header.y + (header.h - b) / 2.0, w: b, h: b };
    let sx = r.x + side_w + 4.0 * hs;
    let search = Rect { x: sx, y: header.y + 8.0 * hs, w: (configure.x - 5.0 * hs - sx).max(1.0), h: 30.0 * hs };
    let c = 24.0 * hs;
    let clear = Rect { x: search.right() - c - 2.0 * hs, y: search.y + (search.h - c) / 2.0, w: c, h: c };
    Frame { card: r, header, hs, avatar, search, clear, configure, side_w, sidebar, content, footer }
}

/// The Applications sidebar: Favorites, All Applications, then the menu categories in alphabetical order, as the original lists them (Development,
/// Education, Games, Graphics, ... Utilities). Recent applications are not here any more: Kickoff moved them to Places -> History (`showRecentApps:
/// false` in main.qml's root model). The divider the original draws after All Applications is painted on the boundary of the second and third row
/// (the shared lists have one row height, so the original's extra 9 px of spacing is not reproduced).
pub fn views(m: &Model) -> Vec<View> {
    let mut categories = m.category_views();
    categories.sort_by_key(|v| Model::view_title(*v));
    let mut v = vec![View::Favorites, View::All];
    v.extend(categories);
    v
}

fn sidebar_targets(m: &Model) -> Vec<Target> {
    views(m).into_iter().map(Target::View).collect()
}

/// The view the content shows (Home is Favorites in Kickoff).
fn shown(m: &Model) -> View {
    if m.view == View::Home {
        View::Favorites
    } else {
        m.view
    }
}

// ---------------------------------------------------------------------------------------------------------------- state

/// Called when the launcher is built: no file system work beyond the tiny `/sys/power/state` probe. The Places lists are read by one worker the first
/// time the tab opens ([`places`]); nothing waits for it.
pub fn prepare(m: &mut Model) {
    m.ui.insert("kde.hibernate", system::probe_hibernate() as usize);
}

fn krunner() -> Entry {
    Entry { name: "Show KRunner".into(), detail: "Search, calculate, or run a command".into(), icon: "krunner".into(), open: vec![crate::launch::sibling_or_path("decspot")], at: 0 }
}

/// The Computer list now: Show KRunner, then the places the worker found, or Home alone (which needs no file system access) while it is still reading.
fn computer_rows() -> Vec<Entry> {
    let mut rows = vec![krunner()];
    match places::data() {
        Some(d) => rows.extend(d.computer.iter().cloned()),
        None => rows.extend(places::fallback().computer),
    }
    rows
}

/// The documents of the history now (none until the worker has delivered them).
fn document_rows() -> Vec<Entry> {
    places::data().map(|d| d.documents.clone()).unwrap_or_default()
}

/// Where the launcher host tells the style how to wake the event loop: called once with the host's waker (see docs/plasma-theme-ports/kickoff-places-hook.md).
pub fn set_waker(w: crate::window_app::Waker) {
    places::set_waker(w);
}

/// Takes a finished Places answer in; true when the host should redraw. The host calls it from its `tick` once the waker fired.
pub fn tick() -> bool {
    places::poll()
}

/// 0 = Applications, 1 = Places.
fn tab(m: &Model) -> usize {
    m.ui("kde.tab").min(1)
}

fn place(m: &Model) -> usize {
    m.ui("kde.place").min(places::SIDEBAR.len() - 1)
}

/// A leaving action waiting for its second press, and the control that has the focus: like `decsession`'s picker, the arming ends the moment the
/// focus is anywhere else (an arrow key, Tab, the pointer moving over another control, typing a search), so coming back to the control never
/// finds it armed. The engine runs `layout` after every such change, which is where this is checked; the unit tests and the dialog never share it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Arm {
    sys: Sys,
    zone: &'static str,
    sel: usize,
}

thread_local! {
    static ARM: std::cell::Cell<Option<Arm>> = const { std::cell::Cell::new(None) };
}

fn arm(m: &Model, sys: Sys) {
    ARM.with(|a| a.set(Some(Arm { sys, zone: m.zone, sel: m.sel(m.zone) })));
}

fn disarm() {
    ARM.with(|a| a.set(None));
}

/// Moving accessibility focus to search cancels a pending leave action.
pub fn cancel_pending() {
    disarm();
}

/// The pending action, only while the focus is still on the control it was armed on.
fn armed(m: &Model) -> Option<Sys> {
    ARM.with(|a| a.get()).filter(|a| a.zone == m.zone && a.sel == m.sel(a.zone)).map(|a| a.sys)
}

fn menu_open(m: &Model) -> bool {
    m.ui("kde.menu") == 1
}

fn sys_index(sys: Sys) -> u16 {
    system::ALL.iter().position(|s| *s == sys).unwrap_or(0) as u16
}

fn sys_valid(m: &Model, sys: Sys) -> bool {
    system::valid(sys, m.ui("kde.hibernate") == 1)
}

/// Width of a footer button with its caption, from the original capture (icon, caption, 8 px each side).
fn button_w(sys: Sys) -> f32 {
    2.0 * BTN_PAD
        + match sys {
            Sys::Suspend => 57.0,
            Sys::Hibernate => 82.0,
            Sys::Reboot => 67.0,
            Sys::Shutdown => 87.0,
            Sys::Lock => 44.0,
            Sys::Logout => 62.0,
            Sys::SaveSession | Sys::SwitchUser => 90.0,
        }
}

struct Footer {
    tabs: [Rect; 2],
    /// The tabs show only their icons (the card is too narrow for the captions).
    tabs_icon_only: bool,
    buttons: Vec<(Sys, Rect)>,
    session: Option<Rect>,
    /// The actions in the Session menu, in order.
    menu_items: Vec<Sys>,
    /// The open Session menu: its frame, the rows' area inside it and the row height. Rows beyond the visible ones scroll.
    popup: Option<(Rect, Rect, f32)>,
}

/// LeaveButtons.qml's layout: the configured actions as captioned buttons, the rest behind the Session button (icon only when the caption does not fit),
/// everything behind it when even the icon-only layout does not fit. Every size is bounded by the card it sits in, so a small physical screen at a high
/// display scale never pushes a control outside it: the tabs lose their captions and then shrink, and the Session menu keeps to the room above its
/// button and scrolls.
fn footer(m: &Model, f: &Frame, s: f32) -> Footer {
    let ft = f.footer;
    let left = (TAB_X * s).min(ft.w / 20.0);
    let right_margin = (4.0 * s).min(ft.w / 40.0);
    let gap = (TAB_GAP * s).min(ft.w / 40.0);
    let session_w = (ICON_BTN_W * s).min(ft.w / 5.0).max(1.0);
    // what is left for the tabs when the Session button is there
    let room = (ft.w - left - right_margin - session_w - gap - gap).max(2.0);
    let full = TAB_W * s;
    let icon_only_w = 34.0 * s;
    let (tab_w, tabs_icon_only) = if 2.0 * full + gap <= room {
        (full, false)
    } else if 2.0 * icon_only_w + gap <= room {
        (icon_only_w, true)
    } else {
        (((room - gap) / 2.0).max(1.0), true)
    };
    let tabs = [0.0, 1.0].map(|i| Rect { x: ft.x + left + i * (tab_w + gap), y: ft.y, w: tab_w, h: ft.h });
    let avail = ft.right() - right_margin - tabs[1].right() - gap;
    let primary: Vec<Sys> = system::BUTTONS.into_iter().filter(|x| sys_valid(m, *x)).collect();
    let rest: Vec<Sys> = system::ALL.into_iter().filter(|x| sys_valid(m, *x) && !system::BUTTONS.contains(x)).collect();
    let captioned: f32 = primary.iter().map(|b| button_w(*b)).sum::<f32>() * s;
    let overflow_gap = (OVERFLOW_GAP * s).min(ft.w / 40.0);
    let fits = if rest.is_empty() { captioned <= avail } else { captioned + session_w + overflow_gap <= avail };
    let (buttons, in_menu) = if fits { (primary, rest) } else { (Vec::new(), system::ALL.into_iter().filter(|x| sys_valid(m, *x)).collect()) };
    let bh = (32.0 * s).min((ft.h - 4.0 * s).max(1.0));
    let by = ft.y + (ft.h - bh) / 2.0;
    let mut right = ft.right() - right_margin;
    let session = (!in_menu.is_empty()).then(|| {
        let r = Rect { x: right - session_w, y: by, w: session_w, h: bh };
        right = r.x - overflow_gap;
        r
    });
    if session.is_none() {
        right = ft.right() - (8.0 * s).min(ft.w / 20.0);
    }
    let mut rects = Vec::new();
    for b in buttons.iter().rev() {
        let w = button_w(*b) * s;
        rects.push((*b, Rect { x: right - w, y: by, w, h: bh }));
        right -= w;
    }
    rects.reverse();
    // PlasmaExtras.Menu, top-posed and right-aligned to the Session button, inside the card
    let mut popup = None;
    if let (Some(sr), true) = (session, menu_open(m)) {
        let pad = (4.0 * s).min(f.card.w / 40.0);
        let edge = (4.0 * s).min(f.card.w / 40.0);
        let w = (MENU_W * s).min((f.card.w - 2.0 * edge).max(1.0));
        let top = f.card.y + edge;
        let room_h = (sr.y - 2.0 * s.min(2.0) - top).max(2.0 * pad + 1.0);
        let want_row = ROW_H * s;
        // as many rows as the room above the button holds (at least one, squeezed when not even one fits)
        let rows = ((room_h - 2.0 * pad) / want_row).floor().clamp(1.0, in_menu.len() as f32);
        let row = want_row.min((room_h - 2.0 * pad) / rows).max(1.0);
        let h = rows * row + 2.0 * pad;
        let x = (sr.right() - w).clamp(f.card.x + edge, (f.card.right() - edge - w).max(f.card.x + edge));
        let pr = Rect { x, y: (sr.y - 2.0 * s.min(2.0) - h).max(top), w, h };
        popup = Some((pr, Rect { x: pr.x + pad, y: pr.y + pad, w: pr.w - 2.0 * pad, h: rows * row }, row));
    }
    Footer { tabs, tabs_icon_only, buttons: rects, session, menu_items: in_menu, popup }
}

/// The apps of the Places page behind a sidebar row, and its documents.
fn places_targets(m: &Model) -> Vec<Target> {
    match place(m) {
        places::COMPUTER => {
            let mut out: Vec<Target> = (0..computer_rows().len()).map(|i| Target::Style(COMPUTER_ROW, i as u16)).collect();
            // ComputerModel lists System Settings (when installed) after "Show KRunner"
            if let Some(i) = m.cat.apps.iter().position(|a| matches!(crate::launcher::catalog::app_id(a).as_str(), "systemsettings" | "org.kde.systemsettings")) {
                out.insert(1.min(out.len()), Target::App(i));
            }
            out
        }
        places::HISTORY => {
            // RecentUsageModel: applications and documents, most recent first
            let mut rows: Vec<(u64, Target)> = m.cat.recent.iter().map(|&i| (m.cat.last_launch(i).unwrap_or(0), Target::App(i))).collect();
            rows.extend(document_rows().iter().enumerate().map(|(i, e)| (e.at, Target::Style(DOCUMENT_ROW, i as u16))));
            rows.sort_by_key(|r| std::cmp::Reverse(r.0));
            rows.into_iter().take(50).map(|r| r.1).collect()
        }
        _ => Model::app_targets(&m.cat.frequent(30)),
    }
}

// ---------------------------------------------------------------------------------------------------------------- layout

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    // a Places answer that arrived since the last event is taken in here (layout runs after every event); it only fills the store, never a page or a query
    places::poll();
    // every focus change is followed by a layout: an arming made elsewhere ends here
    if ARM.with(|a| a.get()).is_some() && armed(m).is_none() {
        disarm();
    }
    let f = frame(r, s);
    let places = tab(m) == 1;
    let cats = if places {
        Zone::columns("cats", (0..places::SIDEBAR.len()).map(|i| Target::Style(PLACE, i as u16)).collect(), f.sidebar, 1, ROW_PITCH * s, false)
    } else {
        Zone::columns("cats", sidebar_targets(m), f.sidebar, 1, ROW_PITCH * s, false)
    };
    let content = if m.searching() {
        Zone::columns("results", Model::app_targets(&m.results), f.content, 1, 44.0 * s, false)
    } else if places {
        Zone::columns("apps", places_targets(m), f.content, 1, 44.0 * s, false)
    } else {
        match shown(m) {
            View::Favorites => Zone::grid("apps", Model::app_targets(&m.cat.favorites), f.content, ((110.0 * s).min(f.content.w.max(1.0)), 104.0 * s), false, true),
            View::All => Zone::columns("apps", m.lettered(&m.cat.all()), f.content, 1, 44.0 * s, false),
            v => Zone::columns("apps", Model::app_targets(&m.apps_of(v)), f.content, 1, 44.0 * s, false),
        }
    };
    // the header's buttons, left to right: the avatar (account settings), the search field's clear button while there is a query, configure; the Keep
    // Open (pin) button is deliberately absent, see docs/plasma-theme-ports/kickoff-fidelity.md
    let mut head: Vec<(Target, Rect)> = vec![(Target::Style(HEAD, 0), f.avatar)];
    if m.searching() && f.search.w >= 60.0 * f.hs {
        head.push((Target::Clear, f.clear));
    }
    head.push((Target::Style(HEAD, 1), f.configure));
    let fo = footer(m, &f, s);
    let mut row: Vec<(Target, Rect)> = vec![(Target::Style(TAB, 0), fo.tabs[0]), (Target::Style(TAB, 1), fo.tabs[1])];
    row.extend(fo.buttons.iter().map(|(sys, rect)| (Target::Style(SYS, sys_index(*sys)), *rect)));
    row.extend(fo.session.map(|rect| (Target::Style(SESSION, 0), rect)));
    // an open Session menu is first so that the pointer finds it before the lists it covers
    let mut zones = Vec::new();
    if let Some((_, inner, row)) = fo.popup {
        zones.push(Zone::columns("menu", fo.menu_items.iter().map(|sys| Target::Style(MENU, sys_index(*sys))).collect(), inner, 1, row, false));
    }
    zones.extend([content, cats, Zone::fixed("footer", row), Zone::fixed("actions", head)]);
    zones
}

// ---------------------------------------------------------------------------------------------------------------- actions

fn spawn(argv: &[String]) {
    #[cfg(test)]
    OPENED.with(|o| o.borrow_mut().push(argv.to_vec()));
    #[cfg(not(test))]
    crate::launch::spawn_argv(argv);
}

#[cfg(test)]
thread_local! {
    /// The commands the unit tests "started".
    static OPENED: std::cell::RefCell<Vec<Vec<String>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn reset_lists(m: &mut Model) {
    for id in ["apps", "results", "cats"] {
        m.sel.insert(id, 0);
        m.offset.insert(id, 0);
    }
    if !m.field.text().is_empty() {
        m.field.clear();
        m.results.clear();
    }
}

fn run_system(m: &mut Model, sys: Sys) -> Outcome {
    if !sys_valid(m, sys) {
        return Outcome::Nothing;
    }
    // Restart, Shut Down and Log Out ask once more: the first press arms the button, the second acts
    if sys.confirms() && armed(m) != Some(sys) {
        arm(m, sys);
        return Outcome::Redraw;
    }
    disarm();
    system::run(sys);
    Outcome::Close
}

/// What a footer tab, button, menu row or Places row does.
pub fn activate(m: &mut Model, kind: u8, i: u16) -> Outcome {
    match kind {
        TAB => {
            m.ui.insert("kde.tab", (i as usize).min(1));
            if i == 1 {
                places::request(places::load);
            } else {
                places::cancel();
            }
            disarm();
            m.ui.insert("kde.menu", 0);
            reset_lists(m);
            Outcome::Relayout
        }
        SYS => match system::ALL.get(i as usize) {
            Some(&sys) => run_system(m, sys),
            None => Outcome::Nothing,
        },
        SESSION => {
            let open = !menu_open(m);
            m.ui.insert("kde.menu", open as usize);
            disarm();
            m.offset.insert("menu", 0);
            if open {
                m.zone = "menu";
                m.sel.insert("menu", 0);
            }
            Outcome::Relayout
        }
        MENU => {
            let Some(&sys) = system::ALL.get(i as usize) else { return Outcome::Nothing };
            let outcome = run_system(m, sys);
            if outcome == Outcome::Close {
                m.ui.insert("kde.menu", 0);
            }
            outcome
        }
        HEAD => {
            // Header.qml: the avatar opens the user's account settings (`KCMLauncher.openSystemSettings("kcm_users")`), the configure button the applet's
            // configuration; here those are Decebal Settings' Users & Login and Launcher panes
            let pane = if i == 0 { "users" } else { "launcher" };
            spawn(&[crate::launch::sibling_or_path("decsettings"), pane.to_string()]);
            Outcome::Close
        }
        PLACE => {
            m.ui.insert("kde.place", (i as usize).min(places::SIDEBAR.len() - 1));
            for id in ["apps", "results"] {
                m.sel.insert(id, 0);
                m.offset.insert(id, 0);
            }
            Outcome::Relayout
        }
        COMPUTER_ROW | DOCUMENT_ROW => {
            let rows = if kind == COMPUTER_ROW { computer_rows() } else { document_rows() };
            match rows.get(i as usize) {
                Some(e) if !e.open.is_empty() => {
                    spawn(&e.open);
                    Outcome::Close
                }
                _ => Outcome::Nothing,
            }
        }
        _ => Outcome::Nothing,
    }
}

/// Escape first closes the open Session menu, or takes back a pending confirmation, before it closes the launcher.
pub fn escape(m: &mut Model) -> bool {
    if armed(m).is_some() {
        disarm();
        return true;
    }
    if menu_open(m) {
        m.ui.insert("kde.menu", 0);
        m.zone = "footer";
        return true;
    }
    false
}

/// A press outside the open menu only closes it; a press elsewhere than on the armed button takes the confirmation back. True when consumed.
pub fn press(m: &mut Model, zone: Option<&'static str>) -> bool {
    if menu_open(m) && zone != Some("menu") && zone != Some("footer") {
        m.ui.insert("kde.menu", 0);
        disarm();
        return true;
    }
    if armed(m).is_some() && zone != Some("footer") && zone != Some("menu") {
        disarm();
    }
    false
}

/// A monochrome icon of the icon theme recoloured to `ink` (the way KIconLoader recolours Breeze's action icons for the colour scheme).
fn tinted(p: &mut Painter, name: &str, x: f32, y: f32, size: f32, ink: crate::theme::model::Rgb) {
    let px = size.round().max(1.0) as u32;
    let icon = p.icons.get_for_surface(name, px, ink.t());
    crate::gfx::blit_scaled(p.pm, icon, x.round(), y.round(), px as f32, px as f32, 1.0);
}

/// A thin scrollbar at `x` for a list that has more rows than show (the original's sidebar has one beside its frame).
fn scrollbar(p: &mut Painter, z: &Zone, offset: usize, x: f32, w: f32, track: tiny_skia::Color, thumb: tiny_skia::Color) {
    let shown = z.visible(offset).len();
    if z.items.len() <= shown {
        return;
    }
    let (a, n) = (z.area(), z.items.len() as f32);
    let bar = Rect { x, y: a.y, w, h: a.h };
    p.fill(bar, w / 2.0, track);
    p.fill(Rect { y: a.y + a.h * (z.first(offset) as f32 / n), h: (a.h * shown as f32 / n).max(4.0), ..bar }, w / 2.0, thumb);
}

pub fn paint(m: &Model, zones: &[Zone], r: Rect, p: &mut Painter) {
    let th = crate::theme::th();
    let s = p.s;
    let f = frame(r, s);
    let radius = (th.radius * 0.5).clamp(2.0, 8.0) * s;
    let (ink, sub) = (th.text_c(), th.sub());
    p.shadow(r, radius, 0.8);
    p.fill(r, radius, p.base(th.popup, th.popup_alpha.max(0.92)));
    // Header and footer use the window header colour: a touch of the text colour over the popup.
    let band = p.base(th.text.over(th.popup, 0.05), th.popup_alpha.max(0.92));
    p.fill(f.header, radius, band);
    p.fill(Rect { y: f.header.bottom() - radius, h: radius, ..f.header }, 0.0, band);
    p.fill(Rect { y: f.header.bottom(), h: 1.0, ..f.header }, 0.0, th.ink(0.12));
    p.fill(f.footer, radius, band);
    p.fill(Rect { h: radius, ..f.footer }, 0.0, band);
    p.fill(Rect { y: f.footer.y, h: 1.0, ..f.footer }, 0.0, th.ink(0.12));
    p.stroke(r, radius, th.ink(0.14), 1.0);
    // Breeze highlight: an accent-tinted fill with an accent outline.
    let hl = |p: &mut Painter, rect: Rect, focused: bool, current: bool| {
        if focused {
            p.fill(rect, 3.0 * s, th.accent.color(0.22));
            p.stroke(rect, 3.0 * s, th.accent.color(if m.keyboard { 1.0 } else { 0.6 }), if m.keyboard { 2.0 * s } else { 1.0 });
        } else if current {
            p.fill(rect, 3.0 * s, th.ink(0.08));
        }
    };

    // Header (Header.qml): the avatar and the name over the sidebar, the search field over the content, a divider between the two columns.
    let hs = f.hs;
    let hcy = f.header.y + f.header.h / 2.0;
    p.avatar(&m.user, f.avatar.x + f.avatar.w / 2.0, f.avatar.y + f.avatar.h / 2.0, f.avatar.w / 2.0);
    p.text(&m.user, r.x + 42.0 * hs, hcy, 14.0 * hs, Weight::Regular, ink, (f.side_w - 46.0 * hs).max(1.0));
    p.fill(Rect { x: r.x + f.side_w - 1.0, y: f.header.bottom() + 1.0, w: 1.0, h: (f.footer.y - f.header.bottom() - 1.0).max(0.0) }, 0.0, th.ink(0.12));
    p.fill(f.search, ROW_RADIUS * hs, th.background.color(1.0));
    p.stroke(f.search, ROW_RADIUS * hs, if m.searching() { th.accent.color(1.0) } else { th.ink(0.2) }, 1.0);
    let scy = f.search.y + f.search.h / 2.0;
    p.glyph(Glyph::Search, f.search.x + 15.0 * hs, scy, 13.0 * hs, sub);
    p.query(m.query(), m.field.before_cursor(), "Search…", f.search.x + 29.0 * hs, scy, 13.0 * hs, ink, th.faint(), (f.search.w - 29.0 * hs - 30.0 * hs).max(1.0));
    // the sidebar's rows: Breeze's list highlight, measured on the original as the accent at about 0.17 over the card with a lighter accent outline;
    // the current row and the hovered one look alike, the keyboard focus gets the accent ring
    let sidebar_frame = |p: &mut Painter, rect: Rect, focused: bool, current: bool| {
        if focused || current {
            p.fill(rect, ROW_RADIUS * s, th.accent.color(0.17));
            if focused && m.keyboard {
                p.stroke(rect, ROW_RADIUS * s, th.accent.color(1.0), 2.0 * s);
            } else {
                p.stroke(rect, ROW_RADIUS * s, th.accent.color(0.55), 1.0);
            }
        }
    };

    let list_row = |p: &mut Painter, rect: Rect, focused: bool, icon: &str, name: &str, detail: &str| {
        let cy = rect.y + rect.h / 2.0;
        let row = Rect { x: rect.x, y: rect.y + 1.0 * s, w: rect.w, h: rect.h - 2.0 * s };
        hl(p, row, focused, false);
        let size = 32.0 * s;
        p.icon(icon, rect.x + 8.0 * s, cy - size / 2.0, size);
        let tx = rect.x + 16.0 * s + size;
        p.text(name, tx, cy - 7.0 * s, 13.0 * s, Weight::Regular, ink, rect.w - size - 24.0 * s);
        p.text(detail, tx, cy + 9.0 * s, 11.0 * s, Weight::Regular, sub, rect.w - size - 24.0 * s);
    };
    let fo = footer(m, &f, s);

    for z in zones.iter().filter(|z| z.id != "menu") {
        let offset = m.offset(z.id);
        for i in z.visible(offset) {
            let Some(rect) = z.rect(i, offset) else { continue };
            let focused = m.focused(z.id, i);
            let cy = rect.y + rect.h / 2.0;
            match (z.id, z.items[i]) {
                ("cats", Target::View(v)) => {
                    let current = !m.searching() && v == shown(m);
                    let row = Rect { x: rect.x, y: rect.y + 0.5 * s, w: rect.w, h: rect.h - 1.0 * s };
                    sidebar_frame(p, row, focused, current);
                    let icon = (22.0 * s).min(rect.h);
                    let (ix, iy) = (rect.x + 7.0 * s, cy - icon / 2.0);
                    // the semantic icons of the source: `bookmarks`, `applications-all-symbolic` (RootModel's group entries) and the menu's own
                    // `applications-*` category icons, recoloured to the text colour like KIconLoader does
                    match v {
                        View::Favorites | View::Home => tinted(p, "bookmarks", ix, iy, icon, th.text),
                        View::All => {
                            tinted(p, "applications-all-symbolic", ix, iy, icon, th.text);
                            // the original's divider after All Applications, on the boundary of the next row
                            if z.items.len() > 2 {
                                p.fill(Rect { x: rect.x + 4.0 * s, y: rect.bottom() - 0.5 * s, w: (rect.w - 8.0 * s).max(0.0), h: 1.0 }, 0.0, th.ink(0.12));
                            }
                        }
                        View::Recent => p.glyph(Glyph::Clock, ix + icon / 2.0, cy, icon * 0.8, ink),
                        View::Category(c) if m.themed.contains(&c) => tinted(p, c.icon(), ix, iy, icon, th.text),
                        View::Category(c) => p.category(c, false, ix, iy, icon),
                    }
                    p.text(Model::view_title(v), rect.x + 40.0 * s, cy, 13.0 * s, Weight::Regular, ink, (rect.w - 44.0 * s).max(1.0));
                }
                ("cats", Target::Style(PLACE, k)) => {
                    let (title, icon_name) = places::SIDEBAR[(k as usize).min(places::SIDEBAR.len() - 1)];
                    let row = Rect { x: rect.x, y: rect.y + 0.5 * s, w: rect.w, h: rect.h - 1.0 * s };
                    sidebar_frame(p, row, focused, !m.searching() && k as usize == place(m));
                    let icon = (22.0 * s).min(rect.h);
                    tinted(p, icon_name, rect.x + 7.0 * s, cy - icon / 2.0, icon, th.text);
                    p.text(title, rect.x + 40.0 * s, cy, 13.0 * s, Weight::Regular, ink, (rect.w - 44.0 * s).max(1.0));
                }
                ("apps", Target::App(a)) if z.cols() > 1 => {
                    let cell = Rect { x: rect.x + 3.0 * s, y: rect.y + 3.0 * s, w: rect.w - 6.0 * s, h: rect.h - 6.0 * s };
                    hl(p, cell, focused, false);
                    let app = &m.cat.apps[a];
                    p.icon(&app.icon, rect.x + (rect.w - 48.0 * s) / 2.0, rect.y + 12.0 * s, 48.0 * s);
                    p.text_center(&app.name, rect.x + rect.w / 2.0, rect.y + 80.0 * s, 12.0 * s, Weight::Regular, ink, rect.w - 12.0 * s);
                }
                ("apps", Target::Letter(c)) => {
                    p.text(&c.to_string(), rect.x + 12.0 * s, cy, 13.0 * s, Weight::SemiBold, sub, rect.w);
                    p.fill(Rect { x: rect.x + 30.0 * s, y: cy, w: rect.w - 40.0 * s, h: 1.0 }, 0.0, th.ink(0.12));
                }
                ("apps" | "results", Target::App(a)) => {
                    let app = &m.cat.apps[a];
                    // the description, as Kickoff shows it, else the menu category
                    let desc = if app.comment.is_empty() { m.cat.category[a].title() } else { app.comment.as_str() };
                    list_row(p, rect, focused, &app.icon, &app.name, desc);
                }
                ("apps", Target::Style(kind @ (COMPUTER_ROW | DOCUMENT_ROW), k)) => {
                    let rows = if kind == COMPUTER_ROW { computer_rows() } else { document_rows() };
                    if let Some(e) = rows.get(k as usize) {
                        list_row(p, rect, focused, &e.icon, &e.name, &e.detail);
                    }
                }
                ("actions", Target::Style(HEAD, k)) => {
                    // the avatar itself was drawn with the header; only its frame belongs to the control
                    hl(p, rect, focused, false);
                    if k == 1 {
                        let icon = (22.0 * hs).min(rect.w).min(rect.h);
                        tinted(p, "configure", rect.x + (rect.w - icon) / 2.0, rect.y + (rect.h - icon) / 2.0, icon, th.text);
                    }
                }
                ("actions", Target::Clear) => {
                    hl(p, rect, focused, false);
                    p.glyph(Glyph::Close, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, 10.0 * hs, sub);
                }
                ("footer", Target::Style(TAB, k)) => {
                    let (title, icon_name) = if k == 0 { ("Applications", "applications-all-symbolic") } else { ("Places", "compass") };
                    hl(p, rect, focused, false);
                    let icon = (22.0 * s).min(rect.w).min(rect.h);
                    if fo.tabs_icon_only {
                        tinted(p, icon_name, rect.x + (rect.w - icon) / 2.0, cy - icon / 2.0, icon, th.text);
                    } else {
                        let content = icon + 4.0 * s + p.measure(title, 13.0 * s, Weight::Regular);
                        let x0 = rect.x + (rect.w - content) / 2.0;
                        tinted(p, icon_name, x0, cy - icon / 2.0, icon, th.text);
                        p.text(title, x0 + icon + 4.0 * s, cy, 13.0 * s, Weight::Regular, ink, rect.w);
                    }
                    // TabBar.Footer: the active tab carries Breeze's accent line along the top of the footer
                    if tab(m) == k as usize {
                        p.fill(Rect { x: rect.x, y: f.footer.y - 1.0 * s, w: rect.w, h: 3.0 * s }, 0.0, th.accent_c());
                    }
                }
                ("footer", Target::Style(SYS, k)) => {
                    let Some(&sys) = system::ALL.get(k as usize) else { continue };
                    let pending = armed(m) == Some(sys);
                    hl(p, rect, focused, false);
                    if pending {
                        p.fill(rect, 3.0 * s, th.accent.color(0.30));
                        p.stroke(rect, 3.0 * s, th.accent.color(1.0), 1.0);
                    }
                    let icon = 22.0 * s;
                    tinted(p, sys.icon(), rect.x + (BTN_PAD - 2.0) * s, cy - icon / 2.0, icon, th.text);
                    let caption = if pending { format!("{}?", sys.label()) } else { sys.label().to_string() };
                    p.text(&caption, rect.x + (BTN_PAD + 24.0) * s, cy, 13.0 * s, Weight::Regular, ink, rect.w - (BTN_PAD + 24.0) * s);
                }
                ("footer", Target::Style(SESSION, _)) => {
                    hl(p, rect, focused, menu_open(m));
                    let icon = 22.0 * s;
                    tinted(p, "system-log-out", rect.x + (rect.w - icon) / 2.0, cy - icon / 2.0, icon, th.text);
                }
                _ => {}
            }
        }
    }
    // the worker has not delivered yet (it may be stuck on a slow mount; the lists above are what needs no disk)
    if tab(m) == 1 && places::state() == places::State::Loading && !m.searching() {
        p.text_center("Reading places...", f.content.x + f.content.w / 2.0, f.content.bottom() - 10.0 * s, 11.0 * s, Weight::Regular, sub, f.content.w - 16.0 * s);
    }
    let empty = if m.searching() { m.results.is_empty() } else { zones.iter().find(|z| z.id == "apps").is_some_and(|z| z.items.is_empty()) };
    if empty {
        let msg = if m.searching() {
            "No matches"
        } else if tab(m) == 1 {
            "Nothing here yet"
        } else {
            "Nothing here yet: right-click an app to add it to Favorites"
        };
        p.text_center(msg, f.content.x + f.content.w / 2.0, f.content.y + 40.0 * s, 13.0 * s, Weight::Regular, sub, f.content.w - 24.0 * s);
    }

    if let Some(z) = zones.iter().find(|z| z.id == "cats") {
        scrollbar(p, z, m.offset("cats"), z.area().right() + 10.0 * s, 4.0 * s, th.ink(0.12), th.ink(0.45));
    }

    // The Session menu, over everything else: the rows the zone shows, a scrollbar when there are more.
    if let (Some((popup, _, _)), Some(z)) = (fo.popup, zones.iter().find(|z| z.id == "menu")) {
        p.shadow(popup, 4.0 * s, 0.6);
        p.fill(popup, 4.0 * s, p.base(th.popup, 1.0));
        p.stroke(popup, 4.0 * s, th.ink(0.2), 1.0);
        let offset = m.offset("menu");
        for i in z.visible(offset) {
            let (Some(rect), Target::Style(MENU, k)) = (z.rect(i, offset), z.items[i]) else { continue };
            let Some(&sys) = system::ALL.get(k as usize) else { continue };
            hl(p, rect, m.focused("menu", i), false);
            let cy = rect.y + rect.h / 2.0;
            let icon = (22.0 * s).min(rect.h);
            tinted(p, sys.icon(), rect.x + 6.0 * s.min(rect.w / 12.0), cy - icon / 2.0, icon, th.text);
            let caption = if armed(m) == Some(sys) { format!("{}?", sys.label()) } else { sys.label().to_string() };
            let tx = rect.x + 10.0 * s.min(rect.w / 12.0) + icon;
            p.text(&caption, tx, cy, 13.0 * s, Weight::Regular, ink, (rect.right() - tx - 4.0 * s).max(1.0));
        }
        if z.items.len() > z.visible(offset).len() {
            let a = z.area();
            let (n, shown) = (z.items.len() as f32, z.per_view() as f32);
            let track = Rect { x: a.right() - 3.0 * s, y: a.y, w: 2.0 * s, h: a.h };
            p.fill(track, s, th.ink(0.12));
            p.fill(Rect { y: a.y + a.h * (z.first(offset) as f32 / n), h: (a.h * shown / n).max(4.0 * s), ..track }, s, th.ink(0.45));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- accessibility

/// A short stable word for a caption ("Shut Down" -> "shut-down").
fn slug(s: &str) -> String {
    let mapped: String = s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    mapped.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// FNV-1a over the words of a command, to tell rows of the same name apart by what they open.
fn fnv(parts: &[String]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for p in parts {
        for b in p.bytes().chain([0u8]) {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// What a screen reader is told about the controls this style owns: the footer tabs and system buttons, the Session
/// button and its menu rows, the Places sidebar and rows, and the header buttons. The captions are the ones painted;
/// a leaving action waiting for its second press says so, as its caption does.
pub fn describe(m: &Model, kind: u8, i: u16) -> Option<super::Semantics> {
    use super::{Control, Semantics};
    let make = |key: String, control: Control, name: &str, description: &str| Semantics {
        key,
        control,
        name: name.to_string(),
        description: description.to_string(),
        enabled: true,
        selected: false,
        popup: None,
    };
    Some(match kind {
        TAB => {
            let (name, what) = if i == 0 { ("Applications", "Shows your applications") } else { ("Places", "Shows the computer, history and frequently used apps") };
            Semantics { selected: tab(m) == i as usize, ..make(format!("tab:{}", slug(name)), Control::Tab, name, what) }
        }
        SYS | MENU => {
            let sys = *system::ALL.get(i as usize)?;
            let pending = armed(m) == Some(sys);
            let name = if pending { format!("{}?", sys.label()) } else { sys.label().to_string() };
            let what = if pending {
                "Press again to confirm"
            } else if sys.confirms() {
                "Asks to confirm"
            } else {
                ""
            };
            let (prefix, control) = if kind == SYS { ("sys", Control::Button) } else { ("menu", Control::MenuItem) };
            Semantics { enabled: sys_valid(m, sys), ..make(format!("{prefix}:{}", slug(sys.label())), control, &name, what) }
        }
        SESSION => Semantics { popup: Some(menu_open(m)), ..make("session".into(), Control::Button, "Session", "Shows more leave actions") },
        PLACE => {
            let (title, _) = *places::SIDEBAR.get(i as usize)?;
            Semantics { selected: !m.searching() && i as usize == place(m), ..make(format!("place:{}", slug(title)), Control::Tab, title, "") }
        }
        HEAD if i == 0 => make("head:account".into(), Control::Button, "Account settings", "Opens the account settings"),
        HEAD => make("head:configure".into(), Control::Button, "Configure launcher", "Opens the launcher settings"),
        COMPUTER_ROW | DOCUMENT_ROW => {
            let rows = if kind == COMPUTER_ROW { computer_rows() } else { document_rows() };
            let e = rows.get(i as usize)?;
            let prefix = if kind == COMPUTER_ROW { "computer" } else { "document" };
            Semantics { enabled: !e.open.is_empty(), ..make(format!("{prefix}:{}-{:08x}", slug(&e.name), fnv(&e.open)), Control::Row, &e.name, &e.detail) }
        }
        _ => return None,
    })
}

/// What the style is waiting for in the background: the worker that reads the Places lists.
pub fn busy(m: &Model) -> Option<&'static str> {
    (tab(m) == 1 && places::state() == places::State::Loading && !m.searching()).then_some("Reading places...")
}

#[cfg(test)]
pub fn install_places(computer: Vec<Entry>, documents: Vec<Entry>) {
    places::install(places::Loaded { computer, documents });
}

/// Starts the Places worker and keeps it busy until `gate` is closed or sent to.
#[cfg(test)]
pub fn hold_places(gate: std::sync::mpsc::Receiver<()>) {
    places::request(move |_| {
        let _ = gate.recv();
        places::Loaded::default()
    });
}

/// The commands the style "opened" and the system actions it "ran" since the last call (nothing really started).
#[cfg(test)]
pub fn effects() -> (Vec<Vec<String>>, Vec<&'static str>) {
    let opened = OPENED.with(|o| std::mem::take(&mut *o.borrow_mut()));
    let ran = system::RAN.with(|r| std::mem::take(&mut *r.borrow_mut()));
    (opened, ran.iter().map(|s| s.id()).collect())
}

#[cfg(test)]
mod tests;
