//! Xfce's Whisker Menu with its default settings: the profile (avatar, name) and the command buttons on top, the
//! search entry, then the apps of the chosen category on the left (32 px icons with their descriptions) and the
//! category list (Favorites, Recently Used, All Applications, then the menu categories, 24 px icons) on the right.
//! After xfce4-whiskermenu-plugin's settings.cpp (menu 450 x 500, launcher-show-description, category-icon-size
//! Smaller = 24, launcher-icon-size Small = 32) and window.cpp (title, search, contents; the panel's position only
//! moves the window). Colours come from the theme.

use super::paint::{Glyph, Painter};
use super::{Model, Target, View, Zone};
use crate::launcher::placement::Rect;
use crate::text::Weight;

struct Frame {
    header: Rect,
    search: Rect,
    list: Rect,
    cats: Rect,
}

fn frame(r: Rect, s: f32) -> Frame {
    let pad = 8.0 * s;
    let header_h = (44.0 * s).min(r.h / 6.0);
    let search_h = 32.0 * s;
    let body_h = (r.h - header_h - search_h - 4.0 * pad).max(1.0);
    let cats_w = (r.w * 0.4).clamp((110.0 * s).min(r.w / 2.0), 190.0 * s);
    let header = Rect { x: r.x + pad, y: r.y + pad, w: r.w - 2.0 * pad, h: header_h };
    let search = Rect { x: r.x + pad, y: header.bottom() + pad, w: r.w - 2.0 * pad, h: search_h };
    let list = Rect { x: r.x + pad, y: search.bottom() + pad, w: r.w - 3.0 * pad - cats_w, h: body_h };
    let cats = Rect { x: list.right() + pad, y: list.y, w: cats_w, h: body_h };
    Frame { header, search, list, cats }
}

fn shown(m: &Model) -> View {
    if m.view == View::Home {
        View::Favorites
    } else {
        m.view
    }
}

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    let f = frame(r, s);
    let mut views = vec![View::Favorites, View::Recent, View::All];
    views.extend(m.category_views());
    let cats = Zone::columns("cats", views.into_iter().map(Target::View).collect(), f.cats, 1, 34.0 * s, false);
    let list = if m.searching() {
        Zone::columns("results", Model::app_targets(&m.results), f.list, 1, 46.0 * s, false)
    } else {
        Zone::columns("apps", Model::app_targets(&m.apps_of(shown(m))), f.list, 1, 46.0 * s, false)
    };
    let b = 32.0 * s;
    let cy = f.header.y + f.header.h / 2.0;
    let power = Rect { x: f.header.right() - b, y: cy - b / 2.0, w: b, h: b };
    let settings = Rect { x: power.x - b - 4.0 * s, ..power };
    vec![list, cats, Zone::fixed("actions", vec![(Target::Settings, settings), (Target::Power, power)])]
}

pub fn paint(m: &Model, zones: &[Zone], r: Rect, p: &mut Painter) {
    let th = crate::theme::th();
    let s = p.s;
    let f = frame(r, s);
    let radius = (th.radius * 0.3).clamp(0.0, 6.0) * s;
    let (ink, sub) = (th.text_c(), th.sub());
    p.shadow(r, radius, 0.7);
    p.fill(r, radius, p.base(th.popup, th.popup_alpha.max(0.94)));
    p.stroke(r, radius, th.ink(0.16), 1.0);
    // GTK selection: the accent colour with the accent's text colour on it.
    let (sel_bg, sel_ink) = (th.accent.color(1.0), th.accent_text.color(1.0));

    let cy = f.header.y + f.header.h / 2.0;
    p.avatar(&m.user, f.header.x + 18.0 * s, cy, 16.0 * s);
    p.text(&m.user, f.header.x + 42.0 * s, cy, 14.0 * s, Weight::SemiBold, ink, f.header.w - 130.0 * s);

    p.fill(f.search, 3.0 * s, th.background.color(1.0));
    p.stroke(f.search, 3.0 * s, if m.searching() { th.accent.color(1.0) } else { th.ink(0.22) }, 1.0);
    let scy = f.search.y + f.search.h / 2.0;
    p.glyph(Glyph::Search, f.search.x + 16.0 * s, scy, 13.0 * s, sub);
    p.query(m.query(), m.field.before_cursor(), "Search Action", f.search.x + 32.0 * s, scy, 13.0 * s, ink, th.faint(), f.search.w - 40.0 * s);

    for z in zones {
        let offset = m.offset(z.id);
        for i in z.visible(offset) {
            let Some(rect) = z.rect(i, offset) else { continue };
            let focused = m.focused(z.id, i);
            let cy = rect.y + rect.h / 2.0;
            match (z.id, z.items[i]) {
                ("cats", Target::View(v)) => {
                    let current = !m.searching() && v == shown(m);
                    let fg = if focused { sel_ink } else { ink };
                    if focused {
                        p.fill(rect, 3.0 * s, sel_bg);
                    } else if current {
                        p.fill(rect, 3.0 * s, th.ink(0.10));
                    }
                    if focused && m.keyboard {
                        p.stroke(rect, 3.0 * s, th.text.color(0.8), 1.0);
                    }
                    let icon = 24.0 * s;
                    let ix = rect.x + 5.0 * s;
                    match v {
                        View::Favorites | View::Home => p.glyph(Glyph::Star, ix + icon / 2.0, cy, icon * 0.8, fg),
                        View::Recent => p.glyph(Glyph::Clock, ix + icon / 2.0, cy, icon * 0.8, fg),
                        View::All => p.glyph(Glyph::Apps, ix + icon / 2.0, cy, icon * 0.7, fg),
                        View::Category(c) => p.category(c, m.themed.contains(&c), ix, cy - icon / 2.0, icon),
                    }
                    p.text(Model::view_title(v), ix + icon + 8.0 * s, cy, 13.0 * s, Weight::Regular, fg, rect.w - icon - 20.0 * s);
                }
                ("apps" | "results", Target::App(a)) => {
                    let fg = if focused { sel_ink } else { ink };
                    if focused {
                        p.fill(rect, 3.0 * s, sel_bg);
                        if m.keyboard {
                            p.stroke(rect, 3.0 * s, th.text.color(0.8), 1.0);
                        }
                    }
                    let app = &m.cat.apps[a];
                    let icon = 32.0 * s;
                    p.icon(&app.icon, rect.x + 6.0 * s, cy - icon / 2.0, icon);
                    let (tx, tw) = (rect.x + 14.0 * s + icon, rect.w - icon - 34.0 * s);
                    if app.comment.is_empty() {
                        p.text(&app.name, tx, cy, 13.0 * s, Weight::Regular, fg, tw);
                    } else {
                        let dim = if focused { th.accent_text.color(0.8) } else { sub };
                        p.text(&app.name, tx, cy - 8.0 * s, 13.0 * s, Weight::Regular, fg, tw);
                        p.text(&app.comment, tx, cy + 9.0 * s, 11.0 * s, Weight::Regular, dim, tw);
                    }
                    if m.cat.is_favorite(a) && shown(m) != View::Favorites {
                        p.glyph(Glyph::Star, rect.right() - 12.0 * s, cy, 9.0 * s, if focused { sel_ink } else { th.accent_c() });
                    }
                }
                ("actions", t) => {
                    if focused {
                        p.highlight(m.keyboard, rect, 3.0 * s, th.ink(0.12), ink);
                    }
                    let g = if t == Target::Power { Glyph::Power } else { Glyph::Gear };
                    p.glyph(g, rect.x + rect.w / 2.0, cy, 16.0 * s, ink);
                }
                _ => {}
            }
        }
    }
    let empty = if m.searching() { m.results.is_empty() } else { zones.first().is_some_and(|z| z.items.is_empty()) };
    if empty {
        let msg = if m.searching() { "No matches" } else { "Empty: right-click an app to add it to Favorites" };
        p.text_center(msg, f.list.x + f.list.w / 2.0, f.list.y + 24.0 * s, 12.0 * s, Weight::Regular, sub, f.list.w - 12.0 * s);
    }
    // the divider between apps and categories
    p.fill(Rect { x: f.cats.x - 4.0 * s, y: f.cats.y, w: 1.0, h: f.cats.h }, 0.0, th.ink(0.12));
}
