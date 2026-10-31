//! The UKUI start menu (`ukui-menu`, openKylin `huanghe`): one 766 x 688 window of three columns, from the original QML.
//!
//! Source (gitee.com/openkylin/ukui-menu @ 6330250, GPL-3.0+; measured, no code copied): `qml/AppUI/NormalUI.qml` (a `RowLayout`: the application page 312,
//! a 1 px divider, the widget page = the rest (396), a 1 px divider, the 56 px sidebar), `AppPage.qml` (12 px top margin, 5 px spacing, the 40 px search
//! bar then the list), `SearchInputBar.qml` (pill, 1 px border, search icon, "Search App"), `AppListHeader.qml` (48 px header with a bold title),
//! `AppListView.qml` / `AppItem.qml` (40 px rows, 32 px icon, `kRadiusMin` 4 px hover/press fills, 14 px scroll bar), `LabelItem.qml` (bold letter, a 1 px
//! divider line), `WidgetPage.qml` (12 px top margin, a 32 px strip of plugin tabs), `FavoriteGridView.qml` (84 + 4 px cells, 100 px high),
//! `Sidebar.qml` (12 px top and bottom, 4 px spacing, 36 px buttons: expand, then user, computer, settings and power at the bottom);
//! `src/windows/menu-main-window.cpp` (margin 8, blur/mask radius 16) and `src/settings/settings.cpp` (766 x 688, margin 8).
//!
//! Not modelled: full-screen mode, the user/computer/expand buttons' actions (drawn, not focusable), extension plugins other than Favorites and Recent,
//! drag and drop, labels pages, tablet mode.

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
}

fn frame(r: Rect, s: f32) -> Frame {
    // 312 | 1 | rest | 1 | 56 at the original 766 wide; narrower cards keep the two fixed columns and give the widget page what is left
    let page = Rect { w: (APP_PAGE * s).min(r.w * 0.45), ..r };
    let sidebar = Rect { x: r.right() - SIDEBAR * s, w: SIDEBAR * s, ..r };
    let widgets = Rect { x: page.right() + DIVIDER * s, w: (sidebar.x - DIVIDER * s - page.right() - DIVIDER * s).max(1.0), ..r };
    // AppPage: topMargin 12, spacing 5; search 40 high with 16 px side margins; header 48; the list takes the rest
    let search = Rect { x: page.x + 16.0 * s, y: page.y + 12.0 * s, w: page.w - 32.0 * s, h: 40.0 * s };
    let header = Rect { x: page.x, y: search.bottom() + 5.0 * s, w: page.w, h: 48.0 * s };
    let list = Rect { x: page.x + 4.0 * s, y: header.bottom() + 5.0 * s, w: page.w - 4.0 * s, h: (page.bottom() - header.bottom() - 10.0 * s).max(1.0) };
    // WidgetPage: topMargin 12, a 32 px tab strip (12 px left), then the favourites grid (cells 88 wide)
    let tabs = Rect { x: widgets.x + 12.0 * s, y: widgets.y + 12.0 * s, w: widgets.w - 40.0 * s, h: 32.0 * s };
    let favourites = Rect { x: widgets.x, y: tabs.bottom() + 5.0 * s, w: widgets.w, h: (widgets.bottom() - tabs.bottom() - 10.0 * s).max(1.0) };
    Frame { page, widgets, sidebar, search, header, list, tabs, favourites }
}

/// The widget page shows Favorites or Recent (Home is Favorites, as in the original's first plugin).
fn shown(m: &Model) -> View {
    if m.view == View::Recent {
        View::Recent
    } else {
        View::Favorites
    }
}

/// The 36 px sidebar buttons: expand at the top, then user, computer, settings, power from the bottom edge up (`Sidebar.qml`: 12 px margins, 4 px spacing).
fn sidebar_buttons(f: &Frame, s: f32) -> [Rect; 5] {
    let b = 36.0 * s;
    let x = f.sidebar.x + (f.sidebar.w - b) / 2.0;
    let top = f.sidebar.y + 12.0 * s;
    let mut y = f.sidebar.bottom() - 12.0 * s - b;
    let mut bottom = [Rect { x, y, w: b, h: b }; 4];
    for slot in bottom.iter_mut().rev() {
        *slot = Rect { x, y, w: b, h: b };
        y -= b + 4.0 * s;
    }
    [Rect { x, y: top, w: b, h: b }, bottom[0], bottom[1], bottom[2], bottom[3]]
}

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    let f = frame(r, s);
    let apps = if m.searching() { Model::app_targets(&m.results) } else { m.lettered(&m.cat.all()) };
    let list = Zone::columns(if m.searching() { "results" } else { "apps" }, apps, f.list, 1, 40.0 * s, false);
    let cell = (88.0 * s, 100.0 * s);
    let favs = match shown(m) {
        View::Recent => Model::app_targets(&m.apps_of(View::Recent)),
        _ => Model::app_targets(&m.cat.favorites),
    };
    let cols = ((f.favourites.w / cell.0).floor() as usize).max(1);
    let area = Rect { x: f.favourites.x + (f.favourites.w - cols as f32 * cell.0) / 2.0, w: cols as f32 * cell.0, ..f.favourites };
    let fav = Zone::grid("fav", favs, area, cell, false, true);
    // the plugin tabs: width = max(56, text + 2 x 12); a narrow card squeezes them to the strip instead of spilling out of the card
    let widths: Vec<f32> = [View::Favorites, View::Recent].map(|v| ((Model::view_title(v).chars().count() as f32 * 8.0 + 24.0) * s).max(56.0 * s)).to_vec();
    let squeeze = (f.tabs.w / widths.iter().sum::<f32>()).min(1.0);
    let mut x = f.tabs.x;
    let mut tabs = Vec::new();
    for (v, w) in [View::Favorites, View::Recent].into_iter().zip(widths) {
        let w = w * squeeze;
        tabs.push((Target::View(v), Rect { x, y: f.tabs.y, w, h: f.tabs.h }));
        x += w;
    }
    let sb = sidebar_buttons(&f, s);
    vec![list, fav, Zone::fixed("tabs", tabs), Zone::fixed("actions", vec![(Target::Settings, sb[3]), (Target::Power, sb[4])])]
}

pub fn paint(m: &Model, zones: &[Zone], r: Rect, p: &mut Painter) {
    let th = crate::theme::th();
    let s = p.s;
    let f = frame(r, s);
    let radius = 16.0 * s;
    let (ink, sub) = (th.text_c(), th.sub());
    let min_r = (4.0 * s).min(th.radius.max(4.0) * s);
    p.shadow(r, radius, 1.0);
    p.fill(r, radius, p.base(th.popup, th.popup_alpha.max(0.78)));
    p.stroke(r, radius, th.ink(0.10), 1.0);
    // the 1 px dividers between the three columns (`kDivider`)
    let div = th.ink(0.10);
    p.fill(Rect { x: f.page.right(), w: DIVIDER * s, ..f.page }, 0.0, div);
    p.fill(Rect { x: f.sidebar.x - DIVIDER * s, w: DIVIDER * s, ..f.sidebar }, 0.0, div);
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

    // the search pill: 40 px high, `kGrayAlpha12` fill, 1 px border (brand colour while typing), the icon half the height
    p.fill(f.search, f.search.h / 2.0, th.ink(0.04));
    p.stroke(f.search, f.search.h / 2.0, if m.searching() { th.accent.color(1.0) } else { th.ink(0.06) }, 1.0);
    let scy = f.search.y + f.search.h / 2.0;
    p.glyph(Glyph::Search, f.search.x + f.search.h / 2.0, scy, f.search.h / 2.0, sub);
    p.query(m.query(), m.field.before_cursor(), "Search App", f.search.x + f.search.h, scy, 14.0 * s, ink, th.faint(), f.search.w - f.search.h - 12.0 * s);

    // the list header: a bold title (the all-apps list is "All"), 16 px side margin
    p.text(if m.searching() { "Search results" } else { "All" }, f.header.x + 16.0 * s, f.header.y + f.header.h / 2.0, 14.0 * s, Weight::Bold, ink, f.header.w - 32.0 * s);

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
                    p.glyph(if t == Target::Power { Glyph::Power } else { Glyph::Gear }, rect.x + rect.w / 2.0, cy, 18.0 * s, ink);
                }
                _ => {}
            }
        }
    }
    // the sidebar's expand, user and computer buttons are drawn as in the original (not focusable here)
    let sb = sidebar_buttons(&f, s);
    p.glyph(Glyph::Apps, sb[0].x + sb[0].w / 2.0, sb[0].y + sb[0].h / 2.0, 16.0 * s, sub);
    p.avatar(&m.user, sb[1].x + sb[1].w / 2.0, sb[1].y + sb[1].h / 2.0, 12.0 * s);
    p.glyph(Glyph::Apps, sb[2].x + sb[2].w / 2.0, sb[2].y + sb[2].h / 2.0, 16.0 * s, sub);
    let empty = if m.searching() { m.results.is_empty() } else { false };
    if empty {
        p.text_center("No matches", f.list.x + f.list.w / 2.0, f.list.y + 40.0 * s, 14.0 * s, Weight::Regular, sub, f.list.w - 24.0 * s);
    }
    let _ = f.widgets;
}

#[cfg(test)]
mod tests {
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
        let b = sidebar_buttons(&f, 1.0);
        assert!(b.iter().all(|r| r.w == 36.0 && r.h == 36.0));
        assert_eq!(b[0].y, 12.0);
        assert_eq!(f.sidebar.bottom() - b[4].bottom(), 12.0);
        for pair in b[1..].windows(2) {
            assert_eq!(pair[1].y - pair[0].bottom(), 4.0);
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
}
