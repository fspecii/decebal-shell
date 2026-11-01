//! A BlackBerry 10-inspired home: a Hub column of accounts (here: All apps, Favorites, Recent and the categories,
//! with their counts and the accent stripe on the open one) beside a paged grid of large app tiles, and the bottom
//! bar with universal search, the page indicator (the Hub glyph, then a dot per page) and close. The Hub column
//! folds away on narrow screens. Inspired by the BlackBerry 10 UI guidelines; colours come from the theme
//! (dark themes get BB10's near-black home screen).

use super::paint::{Glyph, Painter};
use super::{Model, Shape, Target, View, Zone};
use crate::launcher::placement::Rect;
use crate::text::Weight;

struct Frame {
    hub: Option<Rect>,
    grid: Rect,
    bar: Rect,
    search: Rect,
    icon: f32,
    cell: (f32, f32),
}

fn frame(r: Rect, s: f32) -> Frame {
    let bar_h = 64.0 * s;
    let bar = Rect { y: r.bottom() - bar_h, h: bar_h, ..r };
    let hub = (r.w >= 640.0 * s).then(|| Rect { x: r.x, y: r.y, w: (r.w * 0.28).clamp(220.0 * s, 320.0 * s), h: bar.y - r.y });
    let gx = hub.map_or(r.x, |h| h.right());
    let avail = Rect { x: gx + 24.0 * s, y: r.y + 32.0 * s, w: r.right() - gx - 48.0 * s, h: bar.y - r.y - 48.0 * s };
    let (icon, cell) = [(88.0, (148.0, 150.0)), (64.0, (112.0, 116.0)), (44.0, (84.0, 86.0))]
        .into_iter()
        .map(|(i, (w, h))| (i * s, (w * s, h * s)))
        .find(|(_, (w, h))| avail.w / w >= 3.0 && avail.h / h >= 2.0)
        .unwrap_or((44.0 * s, (84.0 * s, 86.0 * s)));
    let cols = ((avail.w / cell.0).floor() as usize).clamp(1, 6);
    let rows = ((avail.h / cell.1).floor() as usize).clamp(1, 4);
    let (gw, gh) = (cols as f32 * cell.0, rows as f32 * cell.1);
    let grid = Rect { x: (avail.x + (avail.w - gw) / 2.0).round(), y: avail.y.round(), w: gw, h: gh };
    let search = Rect { x: bar.x + 16.0 * s, y: bar.y + 14.0 * s, w: (bar.w * 0.3).clamp(1.0, 280.0 * s), h: 36.0 * s };
    Frame { hub, grid, bar, search, icon, cell }
}

fn hub_views(m: &Model) -> Vec<View> {
    let mut v = vec![View::All, View::Favorites, View::Recent];
    v.extend(m.category_views());
    v
}

fn shown(m: &Model) -> View {
    if m.view == View::Home {
        View::All
    } else {
        m.view
    }
}

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    let f = frame(r, s);
    let cols = (f.grid.w / f.cell.0).round() as usize;
    let rows = (f.grid.h / f.cell.1).round() as usize;
    let (id, items) = if m.searching() { ("results", Model::app_targets(&m.results)) } else { ("apps", Model::app_targets(&m.apps_of(shown(m)))) };
    let grid = Zone { id, items, shape: Shape::Grid { area: f.grid, cell: f.cell, cols, rows, paged: true } };
    let pages = grid.pages();
    let mut zones = vec![grid];
    if let Some(hub) = f.hub {
        let area = Rect { x: hub.x, y: hub.y + 56.0 * s, w: hub.w, h: hub.h - 56.0 * s };
        zones.push(Zone::columns("cats", hub_views(m).into_iter().map(Target::View).collect(), area, 1, 52.0 * s, false));
    }
    if pages > 1 && !m.searching() {
        // centred between the search field and the close button, closer together when there are many pages
        let room = (f.bar.w - 2.0 * (f.search.w + 40.0 * s)).max(pages as f32);
        let gap = (22.0 * s).min(room / pages as f32);
        let x0 = f.bar.x + f.bar.w / 2.0 - (pages as f32 - 1.0) * gap / 2.0;
        let (cy, d) = (f.bar.y + f.bar.h / 2.0, gap.min(18.0 * s));
        zones.push(Zone::fixed("pages", (0..pages).map(|p| (Target::Page(p), Rect { x: x0 + p as f32 * gap - d / 2.0, y: cy - d / 2.0, w: d, h: d })).collect()));
    }
    let b = 40.0 * s;
    zones.push(Zone::fixed("actions", vec![(Target::Close, Rect { x: f.bar.right() - 16.0 * s - b, y: f.bar.y + (f.bar.h - b) / 2.0, w: b, h: b })]));
    zones
}

/// The words painted over a grid with nothing in it; the screen reader is told the same ones (`Styled::empty_note`).
pub fn empty_text(m: &Model, zones: &[Zone]) -> Option<&'static str> {
    if m.searching() {
        m.results.is_empty().then_some("No results")
    } else {
        zones.first().is_some_and(|z| z.items.is_empty()).then_some("Nothing here yet")
    }
}

pub fn paint(m: &Model, zones: &[Zone], r: Rect, p: &mut Painter) {
    let th = crate::theme::th();
    let s = p.s;
    let f = frame(r, s);
    let black = crate::theme::model::Rgb(0, 0, 0);
    let bg = if th.dark { th.background.over(black, 0.18) } else { th.background };
    p.fill(r, 0.0, p.base(bg, 0.96));
    let (ink, sub) = (th.text_c(), th.sub());
    let accent = th.accent_c();

    if let Some(hub) = f.hub {
        p.fill(hub, 0.0, th.ink(0.05));
        p.fill(Rect { x: hub.right() - 1.0, w: 1.0, ..hub }, 0.0, th.ink(0.12));
        p.text("Hub", hub.x + 20.0 * s, hub.y + 30.0 * s, 20.0 * s, Weight::Regular, ink, hub.w - 40.0 * s);
    }
    for z in zones {
        let offset = m.offset(z.id);
        for i in z.visible(offset) {
            let Some(rect) = z.rect(i, offset) else { continue };
            let focused = m.focused(z.id, i);
            let cy = rect.y + rect.h / 2.0;
            match (z.id, z.items[i]) {
                ("cats", Target::View(v)) => {
                    let current = !m.searching() && v == shown(m);
                    if focused {
                        p.fill(rect, 0.0, th.ink(0.10));
                    }
                    if current {
                        p.fill(Rect { w: 4.0 * s, ..rect }, 0.0, accent);
                    }
                    if focused && m.keyboard {
                        p.stroke(rect, 0.0, accent, 2.0 * s);
                    }
                    let icon = 28.0 * s;
                    let ix = rect.x + 18.0 * s;
                    match v {
                        View::Favorites | View::Home => p.glyph(Glyph::Star, ix + icon / 2.0, cy, icon * 0.7, ink),
                        View::Recent => p.glyph(Glyph::Clock, ix + icon / 2.0, cy, icon * 0.7, ink),
                        View::All => p.glyph(Glyph::Apps, ix + icon / 2.0, cy, icon * 0.6, ink),
                        View::Category(c) => p.category(c, m.themed.contains(&c), ix, cy - icon / 2.0, icon),
                    }
                    let count = m.apps_of(v).len().to_string();
                    let label = if v == View::All { "All apps" } else { Model::view_title(v) };
                    p.text(label, ix + icon + 14.0 * s, cy, 15.0 * s, Weight::Regular, ink, rect.w - icon - 90.0 * s);
                    p.text_right(&count, rect.right() - 16.0 * s, cy, 13.0 * s, Weight::Regular, sub, 60.0 * s);
                    p.fill(Rect { x: ix, y: rect.bottom() - 1.0, w: rect.right() - ix, h: 1.0 }, 0.0, th.ink(0.08));
                }
                ("apps" | "results", Target::App(a)) => {
                    let tile = Rect { x: rect.x + 6.0 * s, y: rect.y + 4.0 * s, w: rect.w - 12.0 * s, h: rect.h - 8.0 * s };
                    if focused {
                        p.fill(tile, 8.0 * s, th.accent.color(0.16));
                        p.stroke(tile, 8.0 * s, accent, if m.keyboard { 2.5 * s } else { 1.5 * s });
                    }
                    let app = &m.cat.apps[a];
                    let top = rect.y + (rect.h - f.icon - 28.0 * s) / 2.0;
                    p.icon(&app.icon, rect.x + (rect.w - f.icon) / 2.0, top, f.icon);
                    p.text_center(&app.name, rect.x + rect.w / 2.0, top + f.icon + 16.0 * s, 13.0 * s, Weight::Regular, ink, rect.w - 14.0 * s);
                }
                ("pages", Target::Page(n)) => {
                    let current = n == m.offset("apps");
                    let side = if current { 9.0 * s } else { 6.0 * s };
                    let c = if current { accent } else { th.ink(0.45) };
                    p.fill(Rect { x: rect.x + (rect.w - side) / 2.0, y: cy - side / 2.0, w: side, h: side }, 1.5 * s, c);
                }
                ("actions", _) => {
                    if focused {
                        p.highlight(m.keyboard, rect, rect.w / 2.0, th.ink(0.12), accent);
                    }
                    p.glyph(Glyph::Close, rect.x + rect.w / 2.0, cy, 14.0 * s, ink);
                }
                _ => {}
            }
        }
    }
    if let Some(msg) = empty_text(m, zones) {
        p.text_center(msg, f.grid.x + f.grid.w / 2.0, f.grid.y + 60.0 * s, 16.0 * s, Weight::Regular, sub, f.grid.w);
    }

    // The bottom bar: universal search on the left, the page indicator (Hub glyph first) in the middle.
    p.fill(f.bar, 0.0, th.ink(0.06));
    p.fill(Rect { h: 1.0, ..f.bar }, 0.0, th.ink(0.12));
    p.fill(f.search, f.search.h / 2.0, th.ink(0.10));
    if m.searching() {
        p.stroke(f.search, f.search.h / 2.0, accent, 1.5 * s);
    }
    let scy = f.search.y + f.search.h / 2.0;
    p.glyph(Glyph::Search, f.search.x + 18.0 * s, scy, 14.0 * s, sub);
    p.query(m.query(), m.field.before_cursor(), "Search", f.search.x + 36.0 * s, scy, 14.0 * s, ink, sub, f.search.w - 48.0 * s);
    if let Some(z) = zones.iter().find(|z| z.id == "pages") {
        if let Some(first) = z.rect(0, 0) {
            p.glyph(Glyph::Lines, first.x - 16.0 * s, first.y + first.h / 2.0, 14.0 * s, sub);
        }
    }
}
