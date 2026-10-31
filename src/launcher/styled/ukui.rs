//! The UKUI start menu (`ukui-menu`, openKylin `huanghe`): one 766 x 688 window of three columns, from the original QML.
//!
//! Source (gitee.com/openkylin/ukui-menu @ 6330250, GPL-3.0+; measured, no code copied): `qml/AppUI/NormalUI.qml` (a `RowLayout`: the application page 312,
//! a 1 px divider, the widget page = the rest (396), a 1 px divider, the 56 px sidebar), `AppPage.qml` (12 px top margin, 5 px spacing, the 40 px search
//! bar then the list), `SearchInputBar.qml` (pill, 1 px border, search icon, "Search App"), `AppListHeader.qml` (48 px header with a bold title),
//! `AppListView.qml` / `AppControls2/AppItem.qml` (40 px rows, 32 px icon, `kRadiusMin` 4 px hover/press fills, 14 px scroll bar), `AppControls2/LabelItem.qml` (bold letter, a 1 px
//! divider line), `WidgetPage.qml` (12 px top margin, a 32 px strip of plugin tabs), `extensions/FavoriteGridView.qml` (84 + 4 px cells, 100 px high),
//! `Sidebar.qml` (12 px top and bottom, 4 px spacing, 36 px buttons: expand, then user, computer, settings and power at the bottom);
//! `src/windows/menu-main-window.cpp` (margin 8) and `qml/mainXcb.qml` (card, blur region and shadow radius = `kRadiusWindow`, the global theme's window radius) and `src/settings/settings.cpp` (766 x 688, margin 8).
//!
//! Sidebar buttons (`Sidebar.qml`, `sidebar-button-utils.cpp`): user -> "Change account settings" (Decebal Settings, Accounts pane; the original's menu also lists the other
//! accounts and Log Out, which the session menu behind Power offers), computer -> the file manager on `computer:///` (`peony`, else `gio open`, else `xdg-open /`; not drawn when none
//! exists), settings, power. Every drawn control is a target; nothing is painted without an action.
//!
//! Not modelled (and not drawn): the expand button and full-screen mode, extension plugins other than Favorites and Recent, drag and drop, labels pages, tablet mode.
//! A card too narrow for the widget page (a small output at a large scale) drops the widget page, its tabs and favourites.

use super::paint::{Glyph, Painter};
use super::{Model, Target, View, Zone};
use crate::launcher::placement::Rect;
use crate::text::Weight;

const APP_PAGE: f32 = 312.0;
const SIDEBAR: f32 = 56.0;
const DIVIDER: f32 = 1.0;

struct Frame {
    page: Rect,
    widgets: Rect,
    sidebar: Rect,
    search: Rect,
    header: Rect,
    list: Rect,
    tabs: Rect,
    favourites: Rect,
    /// No widget page (see [`frame`]).
    compact: bool,
}

fn frame(r: Rect, s: f32) -> Frame {
    // 312 | 1 | rest | 1 | 56 at the original 766 wide; narrower cards keep the sidebar and give the widget page what is left
    let sidebar_w = (SIDEBAR * s).min(r.w * 0.25);
    let sidebar = Rect { x: r.right() - sidebar_w, w: sidebar_w, ..r };
    let rest = (r.w - sidebar_w - DIVIDER * s).max(1.0);
    let natural_page = (APP_PAGE * s).min(r.w * 0.45);
    // a card too narrow for one favourite cell next to the page (a small output at a large scale) drops the widget page: the list takes the width
    // and the plugin tabs and favourites are not laid out rather than squeezed to nothing
    let compact = rest - natural_page - DIVIDER * s < (88.0 + 24.0) * s;
    let page = Rect { w: if compact { rest } else { natural_page }, ..r };
    let widgets = if compact {
        Rect { x: page.right(), w: 0.0, ..r }
    } else {
        Rect { x: page.right() + DIVIDER * s, w: (sidebar.x - DIVIDER * s - page.right() - DIVIDER * s).max(1.0), ..r }
    };
    // A card shorter than the 160 px the page needs for the search bar, the header and one row (a small output at a large scale) compresses those vertical
    // measures proportionally; the list then shows one squeezed row instead of leaving the card.
    let v = s * (r.h / (160.0 * s)).clamp(0.2, 1.0);
    // AppPage: topMargin 12, spacing 5; search 40 high with 16 px side margins; header 48; the list takes the rest
    let search = Rect { x: page.x + 16.0 * s, y: page.y + 12.0 * v, w: (page.w - 32.0 * s).max(1.0), h: 40.0 * v };
    let header = Rect { x: page.x, y: search.bottom() + 5.0 * v, w: page.w, h: 48.0 * v };
    let list = Rect { x: page.x + 4.0 * s, y: header.bottom() + 5.0 * v, w: (page.w - 4.0 * s).max(1.0), h: (page.bottom() - header.bottom() - 10.0 * v).max(1.0) };
    // WidgetPage: topMargin 12, a 32 px tab strip (12 px left), then the favourites grid (cells 88 wide)
    let tabs = Rect { x: widgets.x + 12.0 * s, y: widgets.y + 12.0 * v, w: (widgets.w - 40.0 * s).max(0.0), h: 32.0 * v };
    let favourites = Rect { x: widgets.x, y: tabs.bottom() + 5.0 * v, w: widgets.w, h: (widgets.bottom() - tabs.bottom() - 10.0 * v).max(1.0) };
    Frame { page, widgets, sidebar, search, header, list, tabs, favourites, compact }
}

/// The full-screen menu's grid, from the formulas in `FullScreenAppList.qml` (logical pixels of the output, `w` x `h`): the proposed app area is `c = (4 + 2560 / w) * w / 8`,
/// `n = clamp(ceil(c / 188), 1, 9)` columns of 160 px cells, horizontal spacing `(c - 160 n) / (n - 1)`, vertical spacing `h / 27`. Returns (columns, cell width, cell height).
fn expanded_grid(w: f32, h: f32) -> (usize, f32, f32) {
    let c = (4.0 + 2560.0 / w.max(1.0)) * w / 8.0;
    let n = ((c / (94.0 * 2.0)).ceil() as usize).clamp(1, 9);
    let gap_h = if n > 1 { ((c - n as f32 * 160.0) / (n as f32 - 1.0)).floor().max(0.0) } else { 0.0 };
    (n, 160.0 + gap_h, 160.0 + (h / 27.0).floor())
}

/// `FullScreenUI.qml`: a 3 x 2 layout. The search bar (372 x 36) is centred at the top, `round(48 * H / 1080)` below the top edge, the restore button (36) at the top right
/// 40 px from the edge, the 112 px sidebar (32 px from the right edge) down the right, the grid in the rest. The label list on the left and the list-mode buttons of the sidebar are not modelled.
struct Expanded {
    search: Rect,
    area: Rect,
    sidebar: Rect,
    restore: Rect,
    cell: (f32, f32),
}

fn expanded_frame(r: Rect, s: f32) -> Expanded {
    let (lw, lh) = (r.w / s, r.h / s);
    let top = (48.0 * lh / 1080.0).round().max(8.0) * s;
    let search = Rect { x: r.x + (r.w - (372.0 * s).min(r.w - 32.0 * s)) / 2.0, y: r.y + top, w: (372.0 * s).min(r.w - 32.0 * s).max(1.0), h: 36.0 * s };
    let restore = Rect { x: r.right() - 40.0 * s - 36.0 * s, y: search.y, w: 36.0 * s, h: 36.0 * s };
    let sidebar_w = (112.0 * s).min(r.w * 0.2);
    let sidebar = Rect { x: r.right() - 32.0 * s - sidebar_w, y: search.bottom() + top, w: sidebar_w, h: (r.bottom() - search.bottom() - top - 4.0 * s).max(1.0) };
    let (cols, cw, ch) = expanded_grid(lw, lh);
    let (cw, ch) = (cw * s, ch * s);
    let want = cols as f32 * cw;
    let free = (sidebar.x - r.x - 48.0 * s - 16.0 * s).max(1.0);
    // a narrow output keeps what fits
    let cols = if want > free { ((free / cw).floor() as usize).max(1) } else { cols };
    let w = (cols as f32 * cw).min(free);
    let area = Rect { x: r.x + 48.0 * s + (free - w) / 2.0, y: sidebar.y, w, h: sidebar.h };
    Expanded { search, area, sidebar, restore, cell: (cw.min(w), ch) }
}

/// The widget page shows Favorites or Recent (Home is Favorites, as in the original's first plugin).
fn shown(m: &Model) -> View {
    if m.view == View::Recent {
        View::Recent
    } else {
        View::Favorites
    }
}

/// The sidebar buttons from the bottom edge up: power, settings, computer (only where something can open it), user (`Sidebar.qml`: 36 px, 12 px margins, 4 px spacing).
/// A short or narrow card shrinks them so the stack never leaves the sidebar. The original's expand button toggles full-screen mode, which is not implemented: it is not drawn.
fn sidebar_buttons(f: &Frame, s: f32, reserved: usize) -> Vec<(Target, Rect)> {
    let mut order = vec![Target::Power, Target::Settings];
    if super::computer_available() {
        order.push(Target::Computer);
    }
    order.push(Target::Account);
    // `reserved` slots (the expand button at the top) take part in the size so a short sidebar never overlaps them
    let slots = (order.len() + reserved) as f32;
    let gaps = 4.0 * s * (slots - 1.0);
    let avail = (f.sidebar.h - 24.0 * s - gaps).max(slots);
    let b = (36.0 * s).min(f.sidebar.w).min(avail / slots).max(1.0);
    let x = f.sidebar.x + (f.sidebar.w - b) / 2.0;
    let mut y = f.sidebar.bottom() - 12.0 * s - b;
    let mut out = Vec::new();
    for t in order {
        out.push((t, Rect { x, y, w: b, h: b }));
        y -= b + 4.0 * s;
    }
    out
}

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    if m.expanded && m.can_expand {
        let e = expanded_frame(r, s);
        let apps = if m.searching() { Model::app_targets(&m.results) } else { Model::app_targets(&m.cat.all()) };
        let grid = Zone::grid(if m.searching() { "results" } else { "apps" }, apps, e.area, e.cell, false, false);
        let mut actions = vec![(Target::Fullscreen, e.restore)];
        let sidebar = Frame { page: e.sidebar, widgets: e.sidebar, sidebar: e.sidebar, search: e.search, header: e.search, list: e.area, tabs: e.search, favourites: e.area, compact: true };
        actions.extend(sidebar_buttons(&sidebar, s, 0).into_iter().rev());
        return vec![grid, Zone::fixed("fav", Vec::new()), Zone::fixed("tabs", Vec::new()), Zone::fixed("actions", actions)];
    }
    let f = frame(r, s);
    let apps = if m.searching() { Model::app_targets(&m.results) } else { m.lettered(&m.cat.all()) };
    let list = Zone::columns(if m.searching() { "results" } else { "apps" }, apps, f.list, 1, (40.0 * s).min(f.list.h), false);
    let cell = (88.0 * s, 100.0 * s);
    let favs = match shown(m) {
        View::Recent => Model::app_targets(&m.apps_of(View::Recent)),
        _ => Model::app_targets(&m.cat.favorites),
    };
    let fav = if f.compact {
        Zone::fixed("fav", Vec::new())
    } else {
        let cols = ((f.favourites.w / cell.0).floor() as usize).max(1);
        let area = Rect { x: f.favourites.x + (f.favourites.w - cols as f32 * cell.0) / 2.0, w: cols as f32 * cell.0, ..f.favourites };
        Zone::grid("fav", favs, area, cell, false, true)
    };
    // the plugin tabs: width = max(56, text + 2 x 12); a narrow card squeezes them to the strip instead of spilling out of the card
    let widths: Vec<f32> = [View::Favorites, View::Recent].map(|v| ((Model::view_title(v).chars().count() as f32 * 8.0 + 24.0) * s).max(56.0 * s)).to_vec();
    let squeeze = (f.tabs.w / widths.iter().sum::<f32>()).min(1.0);
    let mut x = f.tabs.x;
    let mut tabs = Vec::new();
    if !f.compact {
        for (v, w) in [View::Favorites, View::Recent].into_iter().zip(widths) {
            let w = w * squeeze;
            tabs.push((Target::View(v), Rect { x, y: f.tabs.y, w, h: f.tabs.h }));
            x += w;
        }
    }
    // keyboard and pointer order: expand at the top (only where the surface can grow), then the user down to power
    let stack = sidebar_buttons(&f, s, usize::from(m.can_expand));
    let mut actions: Vec<(Target, Rect)> = Vec::new();
    if let (true, Some((_, first))) = (m.can_expand, stack.first()) {
        actions.push((Target::Fullscreen, Rect { y: f.sidebar.y + 12.0 * s, ..*first }));
    }
    actions.extend(stack.into_iter().rev());
    vec![list, fav, Zone::fixed("tabs", tabs), Zone::fixed("actions", actions)]
}

/// The window material (`DtThemeBackground` default: the window colour at the style's transparency). Over a compositor that blurs (the shell asks for it when the theme's `blur` is on)
/// the alpha is the theme's `transparencyBlur` (`panel_alpha`: 65 %, HeYin 75 %); with blur off it is `transparencyNoBlur` (90 % in every UKUI global theme); a surface without an
/// alpha channel is opaque. Text, icons and controls are drawn on top at full opacity, so content stays readable whatever the backdrop.
pub const NO_BLUR_ALPHA: f32 = 0.90;

fn material(p: &Painter, th: &crate::theme::Theme) -> tiny_skia::Color {
    p.base(th.overlay, if th.blur > 0.0 { th.panel_alpha } else { NO_BLUR_ALPHA })
}

pub fn paint(m: &Model, zones: &[Zone], r: Rect, p: &mut Painter) {
    let th = crate::theme::th();
    let s = p.s;
    let f = frame(r, s);
    // mainXcb.qml: the card, its blur region and its shadow all use `GlobalTheme.kRadiusWindow` (the global theme's `windowRadius`: 12, Classic 0)
    let radius = th.radius * s;
    let (ink, sub) = (th.text_c(), th.sub());
    // kRadiusMin 4 (Classic's global theme sets every radius to 0)
    let min_r = if th.radius > 0.0 { 4.0 * s } else { 0.0 };
    let expanded = m.expanded && m.can_expand;
    let e = expanded.then(|| expanded_frame(r, s));
    if expanded {
        // the full-screen menu is flat against the screen edges: no radius, no shadow
        p.fill(r, 0.0, material(p, &th));
    } else {
        p.shadow(r, radius, 1.0);
        p.fill(r, radius, material(p, &th));
        // `backgroundMask`: a 1 px border of `textActive` at alpha 0.15 (none in full-screen mode)
        p.stroke(r, radius, th.ink(0.15), 1.0);
        // the 1 px dividers between the three columns (`kDivider`)
        let div = th.ink(0.10);
        p.fill(Rect { x: f.page.right(), w: DIVIDER * s, ..f.page }, 0.0, div);
        p.fill(Rect { x: f.sidebar.x - DIVIDER * s, w: DIVIDER * s, ..f.sidebar }, 0.0, div);
    }
    let div = th.ink(0.10);
    // `kContainAlphaHover` / `Click`: a faint ink over the popup
    let hover = th.ink(0.06);
    let hl = |p: &mut Painter, rect: Rect, focused: bool, current: bool| {
        if focused {
            p.fill(rect, min_r, hover);
            p.stroke(rect, min_r, th.accent.color(if m.keyboard { 1.0 } else { 0.0 }), if m.keyboard { 2.0 * s } else { 0.0 });
        } else if current {
            p.fill(rect, min_r, th.ink(0.04));
        }
    };

    // the search pill (40 px high in the start menu, 36 in the full-screen one), `kGrayAlpha12` fill, 1 px border (brand colour while typing), the icon half the height
    let search = e.as_ref().map_or(f.search, |e| e.search);
    p.fill(search, search.h / 2.0, th.ink(0.04));
    p.stroke(search, search.h / 2.0, if m.searching() { th.accent.color(1.0) } else { th.ink(0.06) }, 1.0);
    let scy = search.y + search.h / 2.0;
    p.glyph(Glyph::Search, search.x + search.h / 2.0, scy, search.h / 2.0, sub);
    p.query(m.query(), m.field.before_cursor(), "Search App", search.x + search.h, scy, 14.0 * s, ink, th.faint(), search.w - search.h - 12.0 * s);

    // the list header: a bold title (the all-apps list is "All"), 16 px side margin
    if !expanded {
        p.text(if m.searching() { "Search results" } else { "All" }, f.header.x + 16.0 * s, f.header.y + f.header.h / 2.0, 14.0 * s, Weight::Bold, ink, f.header.w - 32.0 * s);
    }

    // the widget page tabs: the selected plugin in the primary ink, the other faint, a `kRadiusMin` fill under the hovered one
    for z in zones.iter().filter(|z| z.id == "tabs") {
        for i in z.visible(0) {
            let Some(rect) = z.rect(i, 0) else { continue };
            let Target::View(v) = z.items[i] else { continue };
            hl(p, rect, m.focused(z.id, i), false);
            let on = shown(m) == v;
            p.text_center(Model::view_title(v), rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, 14.0 * s, Weight::SemiBold, if on { ink } else { th.faint() }, rect.w - 8.0 * s);
        }
    }

    for z in zones {
        let offset = m.offset(z.id);
        for i in z.visible(offset) {
            let Some(rect) = z.rect(i, offset) else { continue };
            let focused = m.focused(z.id, i);
            let cy = rect.y + rect.h / 2.0;
            match (z.id, z.items[i]) {
                ("fav", Target::App(a)) => {
                    // 84 wide inside the 88 cell, the icon on top (8 px margin), the name below; `kRadiusMax` hover
                    let cell = Rect { x: rect.x + 2.0 * s, y: rect.y, w: rect.w - 4.0 * s, h: rect.h };
                    if focused {
                        p.fill(cell, 8.0 * s, hover);
                    }
                    let app = &m.cat.apps[a];
                    p.icon(&app.icon, rect.x + (rect.w - 48.0 * s) / 2.0, rect.y + 8.0 * s, 48.0 * s);
                    p.text_center(&app.name, rect.x + rect.w / 2.0, rect.y + 76.0 * s, 14.0 * s, Weight::Regular, ink, rect.w - 8.0 * s);
                }
                (_, Target::Letter(c)) => {
                    // LabelItem: a bold letter at 12 px, then a 1 px divider line (spacing 14)
                    p.text(&c.to_string(), rect.x + 12.0 * s, cy, 14.0 * s, Weight::Bold, ink, rect.w);
                    p.fill(Rect { x: rect.x + 12.0 * s + 14.0 * s + 14.0 * s, y: cy, w: rect.w - 12.0 * s - 28.0 * s - 14.0 * s - 16.0 * s, h: 1.0 }, 0.0, div);
                }
                ("apps" | "results", Target::App(a)) if expanded => {
                    // FullScreenAppItem: a 160 x 160 `kRadiusMax` plate, the icon 60 % of its width 16 px from the top, the name 8 px below it
                    let plate = Rect { x: rect.x + (rect.w - 160.0 * s) / 2.0, y: rect.y, w: (160.0 * s).min(rect.w), h: (160.0 * s).min(rect.h) };
                    if focused {
                        p.fill(plate, 8.0 * s, hover);
                        p.stroke(plate, 8.0 * s, th.accent.color(if m.keyboard { 1.0 } else { 0.0 }), if m.keyboard { 2.0 * s } else { 0.0 });
                    }
                    let app = &m.cat.apps[a];
                    let icon = plate.w * 0.6;
                    p.icon(&app.icon, plate.x + (plate.w - icon) / 2.0, plate.y + 16.0 * s, icon);
                    p.text_center(&app.name, plate.x + plate.w / 2.0, plate.y + 16.0 * s + icon + 8.0 * s + 10.0 * s, 14.0 * s, Weight::Regular, ink, plate.w - 8.0 * s);
                }
                ("apps" | "results", Target::App(a)) => {
                    // AppItem: the row inset 4, a 32 px icon (2 px from the edge) and the name; `kRadiusMin` fills
                    let row = Rect { x: rect.x, y: rect.y, w: rect.w - 14.0 * s, h: rect.h };
                    hl(p, row, focused, false);
                    let app = &m.cat.apps[a];
                    let icon = 32.0 * s;
                    p.icon(&app.icon, row.x + 8.0 * s, cy - icon / 2.0, icon);
                    p.text(&app.name, row.x + 8.0 * s + icon + 8.0 * s, cy, 14.0 * s, Weight::Regular, ink, row.w - icon - 24.0 * s);
                }
                ("actions", t) => {
                    hl(p, rect, focused, false);
                    let (cx, size) = (rect.x + rect.w / 2.0, rect.w.min(rect.h) * 0.5);
                    match t {
                        Target::Account => p.avatar(&m.user, cx, cy, (rect.w.min(rect.h) / 3.0).max(1.0)),
                        Target::Computer => p.glyph(Glyph::Computer, cx, cy, size, ink),
                        Target::Power => p.glyph(Glyph::Power, cx, cy, size, ink),
                        Target::Fullscreen => p.glyph(if m.expanded { Glyph::Restore } else { Glyph::Expand }, cx, cy, size, ink),
                        _ => p.glyph(Glyph::Gear, cx, cy, size, ink),
                    }
                }
                _ => {}
            }
        }
    }
    let empty = if m.searching() { m.results.is_empty() } else { false };
    if empty {
        let list = e.as_ref().map_or(f.list, |e| e.area);
        p.text_center("No matches", list.x + list.w / 2.0, list.y + 40.0 * s, 14.0 * s, Weight::Regular, sub, f.list.w - 24.0 * s);
    }
    let _ = f.widgets;
}

#[cfg(test)]
mod tests {
    use super::super::sidebar_argv;
    use super::super::tests::styled;
    use super::*;
    use crate::launcher::style::LauncherStyle;

    fn card(s: f32) -> Rect {
        Rect { x: 0.0, y: 0.0, w: 766.0 * s, h: 688.0 * s }
    }

    #[test]
    fn the_three_columns_are_312_396_and_56_at_the_original_size() {
        for s in [1.0, 1.25, 2.0] {
            let f = frame(card(s), s);
            assert_eq!(f.page.w, 312.0 * s);
            assert_eq!(f.sidebar.w, 56.0 * s);
            assert_eq!(f.widgets.w, 396.0 * s, "766 = 312 + 1 + 396 + 1 + 56 at x{s}");
            assert_eq!(f.search.h, 40.0 * s);
            assert_eq!(f.search.x - f.page.x, 16.0 * s);
            assert_eq!(f.header.h, 48.0 * s);
            assert_eq!(f.tabs.h, 32.0 * s);
        }
    }

    #[test]
    fn the_sidebar_buttons_are_36_px_squares_four_apart_with_power_at_the_bottom() {
        let f = frame(card(1.0), 1.0);
        let b = sidebar_buttons(&f, 1.0, 0);
        assert_eq!(b.iter().map(|(t, _)| *t).collect::<Vec<_>>(), [Target::Power, Target::Settings, Target::Computer, Target::Account], "bottom edge up");
        assert!(b.iter().all(|(_, r)| r.w == 36.0 && r.h == 36.0));
        assert_eq!(f.sidebar.bottom() - b[0].1.bottom(), 12.0);
        for pair in b.windows(2) {
            assert_eq!(pair[0].1.y - pair[1].1.bottom(), 4.0);
        }
    }

    #[test]
    fn every_sidebar_button_is_a_real_target_and_there_is_no_inert_one() {
        let mut st = styled(LauncherStyle::Ukui, 20);
        st.full = true;
        st.scale = 1.0;
        st.resize(1920, 1080);
        let actions = st.zones.iter().find(|z| z.id == "actions").unwrap();
        assert_eq!(actions.items, [Target::Fullscreen, Target::Account, Target::Computer, Target::Settings, Target::Power]);
        for i in 0..actions.items.len() {
            let r = actions.rect(i, 0).unwrap();
            let (zone, hit, target) = st.hit(r.x + r.w / 2.0, r.y + r.h / 2.0).expect("clickable");
            assert_eq!((zone, hit, target), ("actions", i, actions.items[i]));
        }
        // activation closes the launcher; the commands behind them
        let have = |t: &str| t == "peony" || t == "xdg-open";
        assert_eq!(sidebar_argv(Target::Computer, have).unwrap(), ["peony", "computer:///"]);
        assert_eq!(sidebar_argv(Target::Computer, |t| t == "gio").unwrap(), ["gio", "open", "computer:///"]);
        assert_eq!(sidebar_argv(Target::Computer, |t| t == "xdg-open").unwrap(), ["xdg-open", "/"]);
        assert!(sidebar_argv(Target::Computer, |_| false).is_none(), "no file manager: no command (and the button is not drawn)");
        let account = sidebar_argv(Target::Account, have).unwrap();
        assert!(account[0].ends_with("decsettings") && account[1] == "users");
        assert!(sidebar_argv(Target::Settings, have).unwrap()[0].ends_with("decsettings"));
        assert!(sidebar_argv(Target::Power, have).unwrap()[0].ends_with("decsession"));
        assert!(sidebar_argv(Target::App(0), have).is_none());
    }

    #[test]
    fn narrow_and_short_cards_keep_every_hit_rectangle_positive_inside_and_clickable() {
        use crate::launcher::placement::{Anchor, Edge};
        for (screen, scale, edge) in [(400, 300), (320, 240), (640, 360), (800, 480), (766, 688)]
            .into_iter()
            .flat_map(|s| [1.0_f32, 1.25, 1.5, 2.0].into_iter().map(move |k| (s, k)))
            .flat_map(|(s, k)| [None, Some(Edge::Bottom), Some(Edge::Top), Some(Edge::Left), Some(Edge::Right)].into_iter().map(move |e| (s, k, e)))
        {
            {
                let mut st = styled(LauncherStyle::Ukui, 30);
                st.full = true;
                st.scale = scale;
                let (w, h, t) = (screen.0 as f32, screen.1 as f32, 32.0 * scale);
                st.anchor = edge.map(|edge| Anchor {
                    edge,
                    point: Some(match edge {
                        Edge::Top => (w * 0.1, t / 2.0),
                        Edge::Bottom => (w / 2.0, h - t / 2.0),
                        Edge::Left => (t / 2.0, h * 0.9),
                        Edge::Right => (w - t / 2.0, h * 0.2),
                    }),
                    thickness: t,
                });
                st.resize(screen.0, screen.1);
                let ctx = format!("{screen:?} @{scale} {edge:?}, card {:?}", st.card);
                let card = st.card;
                let mut seen: Vec<(&str, usize, Rect)> = Vec::new();
                for z in &st.zones {
                    for i in z.visible(st.m.offset(z.id)) {
                        let r = z.rect(i, st.m.offset(z.id)).unwrap();
                        assert!(r.w > 0.0 && r.h > 0.0, "{} item {i} has no area {r:?}: {ctx}", z.id);
                        assert!(r.x >= card.x - 0.5 && r.y >= card.y - 0.5 && r.right() <= card.right() + 0.5 && r.bottom() <= card.bottom() + 0.5, "{} item {i} {r:?} leaves the card: {ctx}", z.id);
                        seen.push((z.id, i, r));
                    }
                }
                for (n, a) in seen.iter().enumerate() {
                    for b in &seen[n + 1..] {
                        let overlap = a.2.x < b.2.right() - 0.5 && b.2.x < a.2.right() - 0.5 && a.2.y < b.2.bottom() - 0.5 && b.2.y < a.2.bottom() - 0.5;
                        // zones of different columns never overlap; letter headings are part of the app list and are not clickable
                        assert!(!overlap, "{} {} and {} {} overlap: {ctx}", a.0, a.1, b.0, b.1);
                    }
                }
                for z in &st.zones {
                    for i in z.visible(st.m.offset(z.id)) {
                        if !z.items[i].focusable() {
                            continue;
                        }
                        let r = z.rect(i, st.m.offset(z.id)).unwrap();
                        let (zone, _, target) = st.hit(r.x + r.w / 2.0, r.y + r.h / 2.0).unwrap_or_else(|| panic!("{} item {i} is not clickable: {ctx}", z.id));
                        assert_eq!((zone, target), (z.id, z.items[i]), "{ctx}");
                    }
                }
                let actions = st.zones.iter().find(|z| z.id == "actions").unwrap();
                assert!(actions.items.contains(&Target::Settings) && actions.items.contains(&Target::Power), "Settings and Power stay reachable: {ctx}");
                // a card without room for a favourite cell drops the widget page and its tabs; otherwise the tabs fit the strip
                for z in st.zones.iter().filter(|z| z.id == "tabs") {
                    for i in 0..z.items.len() {
                        assert!(z.rect(i, 0).unwrap().w > 0.0, "{ctx}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_app_list_has_40_px_rows_and_favourites_88_by_100_cells() {
        let mut st = styled(LauncherStyle::Ukui, 40);
        st.full = true;
        st.scale = 1.0;
        st.resize(1920, 1080);
        assert_eq!((st.card.w, st.card.h), (766.0, 688.0));
        let apps = st.zones.iter().find(|z| z.id == "apps").unwrap();
        let (a, b) = (apps.rect(0, 0).unwrap(), apps.rect(1, 0).unwrap());
        assert_eq!(a.h, 40.0);
        assert_eq!(b.y - a.y, 40.0);
        let fav = st.zones.iter().find(|z| z.id == "fav").unwrap();
        let c = fav.rect(0, 0).unwrap();
        assert_eq!((c.w, c.h), (88.0, 100.0));
        assert_eq!(fav.items.len(), 9);
    }

    #[test]
    fn the_full_screen_grid_follows_the_formulas_of_the_original() {
        // 1920 x 1080: c = 1280, 7 columns of 160, 26 px spacing, 40 px vertical spacing
        assert_eq!(expanded_grid(1920.0, 1080.0), (7, 186.0, 200.0));
        // 1280 x 720: c = 960 -> 6 columns, gap floor((960 - 960) / 5) = 0, vertical 26
        assert_eq!(expanded_grid(1280.0, 720.0), (6, 160.0, 186.0));
        let (n, ..) = expanded_grid(7680.0, 4320.0);
        assert_eq!(n, 9, "never more than nine columns");
        assert_eq!(expanded_grid(300.0, 300.0).0, 3, "the original's formula; a narrower output keeps only the columns that fit");
    }

    #[test]
    fn expand_and_restore_work_keep_the_search_and_fill_the_free_area() {
        use crate::launcher::placement::{Anchor, Edge};
        for (screen, scale, edge) in [((1920, 1080), 1.0, Edge::Bottom), ((1920, 1080), 2.0, Edge::Left), ((800, 480), 1.0, Edge::Top), ((400, 300), 2.0, Edge::Right)] {
            let mut st = styled(LauncherStyle::Ukui, 60);
            st.full = true;
            st.scale = scale;
            let t = 48.0 * scale;
            let (w, h) = (screen.0 as f32, screen.1 as f32);
            st.anchor = Some(Anchor { edge, point: Some((w / 2.0, h / 2.0)), thickness: t });
            st.resize(screen.0, screen.1);
            let ctx = format!("{screen:?} @{scale} {edge:?}");
            let small = st.card;
            let actions = st.zones.iter().find(|z| z.id == "actions").unwrap();
            assert_eq!(actions.items[0], Target::Fullscreen, "the expand button is the first target: {ctx}");
            // a query typed before expanding survives
            for ch in "ab".chars() {
                st.key(&crate::launcher::styled::KeyEvent { key: crate::launcher::styled::Key::Char, text: ch.to_string(), ch: Some(ch), ctrl: false, shift: false, alt: false });
            }
            let query = st.m.query().to_string();
            st.activate(Target::Fullscreen, "");
            assert!(st.m.expanded, "{ctx}");
            assert_eq!(st.m.query(), query, "{ctx}");
            let free = crate::launcher::placement::free_area(st.anchor.as_ref(), (w, h));
            assert_eq!(st.card, free, "the card fills everything the panel leaves: {ctx}");
            assert!(st.card.w * st.card.h > small.w * small.h || small.w * small.h >= free.w * free.h * 0.9, "{ctx}");
            let mut seen: Vec<(&str, usize, Rect)> = Vec::new();
            for z in &st.zones {
                for i in z.visible(st.m.offset(z.id)) {
                    let r = z.rect(i, st.m.offset(z.id)).unwrap();
                    assert!(r.w > 0.0 && r.h > 0.0 && r.x >= free.x - 0.5 && r.y >= free.y - 0.5 && r.right() <= free.right() + 0.5 && r.bottom() <= free.bottom() + 0.5, "{} {i} {r:?} outside {free:?}: {ctx}", z.id);
                    seen.push((z.id, i, r));
                }
            }
            for (n, a) in seen.iter().enumerate() {
                for b in &seen[n + 1..] {
                    assert!(!(a.2.x < b.2.right() - 0.5 && b.2.x < a.2.right() - 0.5 && a.2.y < b.2.bottom() - 0.5 && b.2.y < a.2.bottom() - 0.5), "{} {} / {} {} overlap: {ctx}", a.0, a.1, b.0, b.1);
                }
            }
            let actions = st.zones.iter().find(|z| z.id == "actions").unwrap();
            for i in 0..actions.items.len() {
                let r = actions.rect(i, 0).unwrap();
                assert_eq!(st.hit(r.x + r.w / 2.0, r.y + r.h / 2.0).map(|h| h.2), Some(actions.items[i]), "{ctx}");
            }
            // the same button restores
            st.activate(Target::Fullscreen, "");
            assert!(!st.m.expanded && st.card == small, "{ctx}");
            assert_eq!(st.m.query(), query, "{ctx}");
        }
    }

    #[test]
    fn the_expand_button_is_absent_where_the_surface_cannot_grow() {
        let mut st = styled(LauncherStyle::Ukui, 20);
        st.full = false;
        st.resize(766, 688);
        let actions = st.zones.iter().find(|z| z.id == "actions").unwrap();
        assert!(!actions.items.contains(&Target::Fullscreen), "a plain window cannot grow: no inert button");
    }
}
