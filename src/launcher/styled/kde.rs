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
/// A row of the Computer list (index into `entries["computer"]`).
const COMPUTER_ROW: u8 = 6;
/// A row of the History list that is a document (index into `entries["documents"]`).
const DOCUMENT_ROW: u8 = 7;

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
    search: Rect,
    sidebar: Rect,
    content: Rect,
    footer: Rect,
}

fn frame(r: Rect, s: f32) -> Frame {
    let header = Rect { h: (56.0 * s).min(r.h / 5.0), ..r };
    let footer_h = (FOOTER_H * s).min(r.h / 6.0);
    let footer = Rect { y: r.bottom() - footer_h, h: footer_h, ..r };
    let side_w = (r.w * 0.34).clamp((120.0 * s).min(r.w / 2.0), 220.0 * s);
    let body = Rect { y: header.bottom(), h: (footer.y - header.bottom()).max(1.0), ..r };
    let sidebar = Rect { x: r.x + 6.0 * s, y: body.y + 6.0 * s, w: side_w - 12.0 * s, h: body.h - 12.0 * s };
    let content = Rect { x: r.x + side_w, y: body.y + 6.0 * s, w: r.w - side_w - 6.0 * s, h: body.h - 12.0 * s };
    let gear = 32.0 * s;
    let search = Rect { x: content.x, y: header.y + (header.h - 32.0 * s) / 2.0, w: (content.w - gear - 8.0 * s).max(1.0), h: 32.0 * s };
    Frame { card: r, header, search, sidebar, content, footer }
}

/// The Applications sidebar: Favorites, All Applications, a separator, then the menu categories. Recent applications are not here any more: Kickoff
/// moved them to Places -> History (`showRecentApps: false` in main.qml's root model).
pub fn views(m: &Model) -> Vec<View> {
    let mut v = vec![View::Favorites, View::All];
    v.extend(m.category_views());
    v
}

/// A divider row of the sidebar (a non-focusable `Letter` with this sentinel).
const SEPARATOR: char = '\0';

fn sidebar_targets(m: &Model) -> Vec<Target> {
    let mut out: Vec<Target> = views(m).into_iter().map(Target::View).collect();
    if out.len() > 2 {
        out.insert(2, Target::Letter(SEPARATOR));
    }
    out
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

/// Reads what the Places tab and the footer need from disk, once, when the launcher is built (never while painting).
pub fn prepare(m: &mut Model) {
    let (mut computer, documents) = places::load();
    computer
        .insert(0, Entry { name: "Show KRunner".into(), detail: "Search, calculate, or run a command".into(), icon: "krunner".into(), open: vec![crate::launch::sibling_or_path("decspot")], at: 0 });
    m.entries.insert("computer", computer);
    m.entries.insert("documents", documents);
    m.ui.insert("kde.hibernate", system::probe_hibernate() as usize);
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
    let docs = |key: &'static str| m.entries.get(key).map_or(0, Vec::len);
    match place(m) {
        places::COMPUTER => {
            let mut out: Vec<Target> = (0..docs("computer")).map(|i| Target::Style(COMPUTER_ROW, i as u16)).collect();
            // ComputerModel lists System Settings (when installed) after "Show KRunner"
            if let Some(i) = m.cat.apps.iter().position(|a| matches!(crate::launcher::catalog::app_id(a).as_str(), "systemsettings" | "org.kde.systemsettings")) {
                out.insert(1.min(out.len()), Target::App(i));
            }
            out
        }
        places::HISTORY => {
            // RecentUsageModel: applications and documents, most recent first
            let mut rows: Vec<(u64, Target)> = m.cat.recent.iter().map(|&i| (m.cat.last_launch(i).unwrap_or(0), Target::App(i))).collect();
            rows.extend(m.entries.get("documents").into_iter().flatten().enumerate().map(|(i, e)| (e.at, Target::Style(DOCUMENT_ROW, i as u16))));
            rows.sort_by_key(|r| std::cmp::Reverse(r.0));
            rows.into_iter().take(50).map(|r| r.1).collect()
        }
        _ => Model::app_targets(&m.cat.frequent(30)),
    }
}

// ---------------------------------------------------------------------------------------------------------------- layout

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    // every focus change is followed by a layout: an arming made elsewhere ends here
    if ARM.with(|a| a.get()).is_some() && armed(m).is_none() {
        disarm();
    }
    let f = frame(r, s);
    let places = tab(m) == 1;
    let cats = if places {
        Zone::columns("cats", (0..places::SIDEBAR.len()).map(|i| Target::Style(PLACE, i as u16)).collect(), f.sidebar, 1, 36.0 * s, false)
    } else {
        Zone::columns("cats", sidebar_targets(m), f.sidebar, 1, 36.0 * s, false)
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
    let b = (32.0 * s).min(f.header.h - 4.0 * s).max(1.0);
    let settings = Rect { x: f.header.right() - 8.0 * s - b, y: f.header.y + (f.header.h - b) / 2.0, w: b, h: b };
    let fo = footer(m, &f, s);
    let mut row: Vec<(Target, Rect)> = vec![(Target::Style(TAB, 0), fo.tabs[0]), (Target::Style(TAB, 1), fo.tabs[1])];
    row.extend(fo.buttons.iter().map(|(sys, rect)| (Target::Style(SYS, sys_index(*sys)), *rect)));
    row.extend(fo.session.map(|rect| (Target::Style(SESSION, 0), rect)));
    // an open Session menu is first so that the pointer finds it before the lists it covers
    let mut zones = Vec::new();
    if let Some((_, inner, row)) = fo.popup {
        zones.push(Zone::columns("menu", fo.menu_items.iter().map(|sys| Target::Style(MENU, sys_index(*sys))).collect(), inner, 1, row, false));
    }
    zones.extend([content, cats, Zone::fixed("footer", row), Zone::fixed("actions", vec![(Target::Settings, settings)])]);
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
        PLACE => {
            m.ui.insert("kde.place", (i as usize).min(places::SIDEBAR.len() - 1));
            for id in ["apps", "results"] {
                m.sel.insert(id, 0);
                m.offset.insert(id, 0);
            }
            Outcome::Relayout
        }
        COMPUTER_ROW | DOCUMENT_ROW => {
            let key = if kind == COMPUTER_ROW { "computer" } else { "documents" };
            match m.entries.get(key).and_then(|l| l.get(i as usize)) {
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

    // Header: avatar and name over the sidebar, search over the content.
    let cy = f.header.y + f.header.h / 2.0;
    p.avatar(&m.user, f.header.x + 28.0 * s, cy, 16.0 * s);
    p.text(&m.user, f.header.x + 52.0 * s, cy, 13.0 * s, Weight::SemiBold, ink, f.sidebar.w - 50.0 * s);
    p.fill(f.search, 3.0 * s, th.background.color(1.0));
    p.stroke(f.search, 3.0 * s, if m.searching() { th.accent.color(1.0) } else { th.ink(0.2) }, 1.0);
    let scy = f.search.y + f.search.h / 2.0;
    p.glyph(Glyph::Search, f.search.x + 16.0 * s, scy, 13.0 * s, sub);
    p.query(m.query(), m.field.before_cursor(), "Search…", f.search.x + 32.0 * s, scy, 13.0 * s, ink, th.faint(), f.search.w - 40.0 * s);

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
                    let row = Rect { x: rect.x, y: rect.y + 1.0 * s, w: rect.w, h: rect.h - 2.0 * s };
                    hl(p, row, focused, current);
                    let icon = 22.0 * s;
                    match v {
                        View::Favorites | View::Home => p.glyph(Glyph::Star, rect.x + 8.0 * s + icon / 2.0, cy, icon * 0.8, th.accent_c()),
                        View::All => p.glyph(Glyph::Apps, rect.x + 8.0 * s + icon / 2.0, cy, icon * 0.7, ink),
                        View::Recent => p.glyph(Glyph::Clock, rect.x + 8.0 * s + icon / 2.0, cy, icon * 0.8, ink),
                        View::Category(c) => p.category(c, m.themed.contains(&c), rect.x + 8.0 * s, cy - icon / 2.0, icon),
                    }
                    p.text(Model::view_title(v), rect.x + 16.0 * s + icon, cy, 13.0 * s, Weight::Regular, ink, rect.w - 24.0 * s - icon);
                }
                ("cats", Target::Letter(SEPARATOR)) => {
                    p.fill(Rect { x: rect.x + 8.0 * s, y: cy, w: rect.w - 16.0 * s, h: 1.0 }, 0.0, th.ink(0.12));
                }
                ("cats", Target::Style(PLACE, k)) => {
                    let (title, icon_name) = places::SIDEBAR[(k as usize).min(places::SIDEBAR.len() - 1)];
                    let row = Rect { x: rect.x, y: rect.y + 1.0 * s, w: rect.w, h: rect.h - 2.0 * s };
                    hl(p, row, focused, !m.searching() && k as usize == place(m));
                    let icon = 22.0 * s;
                    tinted(p, icon_name, rect.x + 8.0 * s, cy - icon / 2.0, icon, th.text);
                    p.text(title, rect.x + 16.0 * s + icon, cy, 13.0 * s, Weight::Regular, ink, rect.w - 24.0 * s - icon);
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
                    let list = if kind == COMPUTER_ROW { "computer" } else { "documents" };
                    if let Some(e) = m.entries.get(list).and_then(|l| l.get(k as usize)) {
                        list_row(p, rect, focused, &e.icon, &e.name, &e.detail);
                    }
                }
                ("actions", _) => {
                    hl(p, rect, focused, false);
                    p.glyph(Glyph::Gear, rect.x + rect.w / 2.0, cy, 16.0 * s, ink);
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

#[cfg(test)]
mod tests;
