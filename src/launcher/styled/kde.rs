//! KDE Plasma's Kickoff: a header with the user and the search field, a sidebar of Favorites, All Applications,
//! Recently Used and the menu categories, the favourites as a grid and every other view as a list, and a footer
//! with the Applications tab and the Leave button. After plasma-desktop's applets/kickoff (Header.qml,
//! ApplicationsPage.qml, Footer.qml) and Breeze highlight metrics; colours come from the theme.

use super::paint::{Glyph, Painter};
use super::{Model, Target, View, Zone};
use crate::launcher::placement::Rect;
use crate::text::Weight;

struct Frame {
    header: Rect,
    search: Rect,
    sidebar: Rect,
    content: Rect,
    footer: Rect,
}

fn frame(r: Rect, s: f32) -> Frame {
    let header = Rect { h: (56.0 * s).min(r.h / 5.0), ..r };
    let footer_h = (48.0 * s).min(r.h / 6.0);
    let footer = Rect { y: r.bottom() - footer_h, h: footer_h, ..r };
    let side_w = (r.w * 0.34).clamp((120.0 * s).min(r.w / 2.0), 220.0 * s);
    let body = Rect { y: header.bottom(), h: (footer.y - header.bottom()).max(1.0), ..r };
    let sidebar = Rect { x: r.x + 6.0 * s, y: body.y + 6.0 * s, w: side_w - 12.0 * s, h: body.h - 12.0 * s };
    let content = Rect { x: r.x + side_w, y: body.y + 6.0 * s, w: r.w - side_w - 6.0 * s, h: body.h - 12.0 * s };
    let gear = 32.0 * s;
    let search = Rect { x: content.x, y: header.y + (header.h - 32.0 * s) / 2.0, w: (content.w - gear - 8.0 * s).max(1.0), h: 32.0 * s };
    Frame { header, search, sidebar, content, footer }
}

pub fn views(m: &Model) -> Vec<View> {
    let mut v = vec![View::Favorites, View::All, View::Recent];
    v.extend(m.category_views());
    v
}

/// The view the content shows (Home is Favorites in Kickoff).
fn shown(m: &Model) -> View {
    if m.view == View::Home {
        View::Favorites
    } else {
        m.view
    }
}

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    let f = frame(r, s);
    let cats = Zone::columns("cats", views(m).into_iter().map(Target::View).collect(), f.sidebar, 1, 36.0 * s, false);
    let content = if m.searching() {
        Zone::columns("results", Model::app_targets(&m.results), f.content, 1, 44.0 * s, false)
    } else {
        match shown(m) {
            View::Favorites => Zone::grid("apps", Model::app_targets(&m.cat.favorites), f.content, (110.0 * s, 104.0 * s), false, true),
            View::All => Zone::columns("apps", m.lettered(&m.cat.all()), f.content, 1, 44.0 * s, false),
            v => Zone::columns("apps", Model::app_targets(&m.apps_of(v)), f.content, 1, 44.0 * s, false),
        }
    };
    let b = (32.0 * s).min(f.header.h - 4.0 * s).max(1.0);
    let settings = Rect { x: f.header.right() - 8.0 * s - b, y: f.header.y + (f.header.h - b) / 2.0, w: b, h: b };
    // the footer shrinks on short screens; the Leave button with it
    let lh = (32.0 * s).min(f.footer.h - 4.0 * s).max(1.0);
    let leave_w = 96.0 * s;
    let leave = Rect { x: f.footer.right() - 8.0 * s - leave_w, y: f.footer.y + (f.footer.h - lh) / 2.0, w: leave_w, h: lh };
    vec![content, cats, Zone::fixed("actions", vec![(Target::Settings, settings), (Target::Power, leave)])]
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

    for z in zones {
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
                    let row = Rect { x: rect.x, y: rect.y + 1.0 * s, w: rect.w, h: rect.h - 2.0 * s };
                    hl(p, row, focused, false);
                    let app = &m.cat.apps[a];
                    let icon = 32.0 * s;
                    p.icon(&app.icon, rect.x + 8.0 * s, cy - icon / 2.0, icon);
                    let tx = rect.x + 16.0 * s + icon;
                    p.text(&app.name, tx, cy - 7.0 * s, 13.0 * s, Weight::Regular, ink, rect.w - icon - 24.0 * s);
                    // the description, as Kickoff shows it, else the menu category
                    let desc = if app.comment.is_empty() { m.cat.category[a].title() } else { app.comment.as_str() };
                    p.text(desc, tx, cy + 9.0 * s, 11.0 * s, Weight::Regular, sub, rect.w - icon - 24.0 * s);
                }
                ("actions", t) => {
                    hl(p, rect, focused, false);
                    if t == Target::Power {
                        p.glyph(Glyph::Power, rect.x + 16.0 * s, cy, 14.0 * s, ink);
                        p.text("Leave…", rect.x + 30.0 * s, cy, 13.0 * s, Weight::Regular, ink, rect.w - 34.0 * s);
                    } else {
                        p.glyph(Glyph::Gear, rect.x + rect.w / 2.0, cy, 16.0 * s, ink);
                    }
                }
                _ => {}
            }
        }
    }
    let empty = if m.searching() { m.results.is_empty() } else { zones.first().is_some_and(|z| z.items.is_empty()) };
    if empty {
        let msg = if m.searching() { "No matches" } else { "Nothing here yet: right-click an app to add it to Favorites" };
        p.text_center(msg, f.content.x + f.content.w / 2.0, f.content.y + 40.0 * s, 13.0 * s, Weight::Regular, sub, f.content.w - 24.0 * s);
    }

    // Footer: the Applications tab (Places is not offered) underlined in the accent colour.
    let tab_w = p.measure("Applications", 13.0 * s, Weight::Regular) + 24.0 * s;
    let tab = Rect { x: f.footer.x + 8.0 * s, y: f.footer.y + 8.0 * s, w: tab_w, h: f.footer.h - 16.0 * s };
    p.text("Applications", tab.x + 12.0 * s, tab.y + tab.h / 2.0, 13.0 * s, Weight::Regular, ink, tab.w);
    p.fill(Rect { y: tab.bottom() - 2.0 * s, h: 2.0 * s, ..tab }, 1.0, th.accent_c());
}
