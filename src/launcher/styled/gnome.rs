//! GNOME Shell's Activities overview, app grid view: a centred "Type to search" entry, a paged grid of every app
//! (one of gnome-shell's grid modes 8 x 3, 6 x 4, 4 x 6, 3 x 8, icon size 96 down to 16), page dots, and the dash with the favourites and the
//! Show Apps button at the bottom. A search turns the grid into the matching apps. Wheel and arrows turn pages.
//! Layout after gnome-shell's appDisplay.js / dash.js and the GNOME HIG; colours come from the theme.

use super::paint::{Glyph, Painter};
use super::{Model, Shape, Target, Zone};
use crate::launcher::placement::Rect;
use crate::text::Weight;

/// gnome-shell's iconGrid.js `defaultGridModes` as (columns, rows): 3 x 8, 4 x 6, 6 x 4 and 8 x 3.
const GRID_MODES: [(usize, usize); 4] = [(3, 8), (4, 6), (6, 4), (8, 3)];
/// iconGrid.js `IconSize`, largest first, as `_findBestIconSize` tries them.
const ICON_SIZES: [f32; 6] = [96.0, 64.0, 48.0, 32.0, 24.0, 16.0];

/// The cell around an icon: room for the label below and the spacing around.
fn cell_for(icon: f32, s: f32) -> (f32, f32) {
    (icon * s + 60.0 * s, icon * s + 56.0 * s)
}

/// Like gnome-shell: the grid mode whose shape best matches the space, then the largest icon size that fits it.
/// A space too small for even a 3 x 3 of 16 px icons gets as many cells as fit.
fn grid_mode(avail: Rect, s: f32) -> (f32, (f32, f32), usize, usize) {
    let aspect = avail.w / avail.h.max(1.0);
    let mut modes = GRID_MODES;
    let shape = |(c, r): (usize, usize)| {
        let (w, h) = cell_for(96.0, s);
        ((c as f32 * w) / (r as f32 * h) / aspect).ln().abs()
    };
    modes.sort_by(|a, b| shape(*a).total_cmp(&shape(*b)));
    for (cols, rows) in modes {
        if let Some(icon) = ICON_SIZES.into_iter().find(|&i| {
            let (w, h) = cell_for(i, s);
            cols as f32 * w <= avail.w && rows as f32 * h <= avail.h
        }) {
            return (icon * s, cell_for(icon, s), cols, rows);
        }
    }
    let cell = cell_for(16.0, s);
    (16.0 * s, cell, ((avail.w / cell.0).floor() as usize).max(1), ((avail.h / cell.1).floor() as usize).max(1))
}

struct Frame {
    search: Rect,
    grid: Rect,
    dots_y: f32,
    dash: Rect,
    icon: f32,
    cell: (f32, f32),
    dash_icon: f32,
}

fn frame(m: &Model, r: Rect, s: f32) -> Frame {
    let search_w = (360.0 * s).min(r.w - 48.0 * s).max(1.0);
    let search = Rect { x: r.x + (r.w - search_w) / 2.0, y: r.y + 24.0 * s, w: search_w, h: 36.0 * s };
    // The dash: one slot per favourite plus Show Apps, as many as fit across the screen.
    let dash_icon = if r.h < 420.0 * s { 32.0 * s } else { 48.0 * s };
    let slot = dash_icon + 16.0 * s;
    let fit = (((r.w - 64.0 * s) / slot).floor() as usize).max(2);
    let slots = (m.cat.favorites.len() + 1).min(fit);
    let dash_w = slots as f32 * slot + 16.0 * s + 12.0 * s;
    let dash_h = slot + 16.0 * s;
    let dash = Rect { x: r.x + (r.w - dash_w) / 2.0, y: r.bottom() - dash_h - 16.0 * s, w: dash_w, h: dash_h };
    let dots_y = dash.y - 22.0 * s;
    let top = search.bottom() + 24.0 * s;
    let avail = Rect { x: r.x + 32.0 * s, y: top, w: r.w - 64.0 * s, h: (dots_y - 14.0 * s - top).max(1.0) };
    let (icon, cell, cols, rows) = grid_mode(avail, s);
    let (gw, gh) = (cols as f32 * cell.0, rows as f32 * cell.1);
    let grid = Rect { x: (avail.x + (avail.w - gw) / 2.0).round(), y: (avail.y + (avail.h - gh) / 2.0).round(), w: gw, h: gh };
    Frame { search, grid, dots_y, dash, icon, cell, dash_icon }
}

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    let f = frame(m, r, s);
    let cols = (f.grid.w / f.cell.0).round() as usize;
    let rows = (f.grid.h / f.cell.1).round() as usize;
    let (id, items) = if m.searching() { ("results", Model::app_targets(&m.results)) } else { ("apps", Model::app_targets(&m.cat.all())) };
    let grid = Zone { id, items, shape: Shape::Grid { area: f.grid, cell: f.cell, cols, rows, paged: true } };
    let pages = grid.pages();
    let mut zones = vec![grid];
    if pages > 1 && !m.searching() {
        let gap = (20.0 * s).min((r.w - 64.0 * s).max(pages as f32) / pages as f32);
        let x0 = r.x + r.w / 2.0 - (pages as f32 - 1.0) * gap / 2.0;
        zones.push(Zone::fixed(
            "pages",
            (0..pages).map(|p| (Target::Page(p), Rect { x: x0 + p as f32 * gap - gap.min(16.0 * s) / 2.0, y: f.dots_y - 8.0 * s, w: gap.min(16.0 * s), h: 16.0 * s })).collect(),
        ));
    }
    let slot = f.dash_icon + 16.0 * s;
    let slots = ((f.dash.w - 28.0 * s) / slot).round() as usize;
    let mut dash: Vec<(Target, Rect)> = m
        .cat
        .favorites
        .iter()
        .take(slots.saturating_sub(1))
        .enumerate()
        .map(|(k, &a)| (Target::App(a), Rect { x: f.dash.x + 8.0 * s + k as f32 * slot, y: f.dash.y + 8.0 * s, w: slot, h: slot }))
        .collect();
    // Show Apps sits after the separator; in the app grid it returns to the desktop, which here means closing.
    dash.push((Target::Close, Rect { x: f.dash.right() - 8.0 * s - slot, y: f.dash.y + 8.0 * s, w: slot, h: slot }));
    zones.push(Zone::fixed("dash", dash));
    zones
}

pub fn paint(m: &Model, zones: &[Zone], r: Rect, p: &mut Painter) {
    let th = crate::theme::th();
    let s = p.s;
    let f = frame(m, r, s);
    // The overview backdrop: the theme's background, dimmed (deeper in dark mode).
    let backdrop = th.background.over(crate::theme::model::Rgb(0, 0, 0), if th.dark { 0.55 } else { 0.86 });
    p.fill(r, 0.0, p.base(backdrop, 0.94));
    let (ink, sub) = (th.text_c(), th.sub());

    p.fill(f.search, f.search.h / 2.0, th.ink(0.10));
    if m.zone == "results" || m.searching() {
        p.stroke(f.search, f.search.h / 2.0, th.accent.color(0.9), 2.0 * s);
    }
    let cy = f.search.y + f.search.h / 2.0;
    p.glyph(Glyph::Search, f.search.x + 18.0 * s, cy, 14.0 * s, sub);
    if m.searching() {
        p.query(m.query(), m.field.before_cursor(), "", f.search.x + 36.0 * s, cy, 14.0 * s, ink, sub, f.search.w - 52.0 * s);
    } else {
        p.text_center("Type to search", f.search.x + f.search.w / 2.0, cy, 14.0 * s, Weight::Regular, sub, f.search.w - 72.0 * s);
    }

    for z in zones {
        let offset = m.offset(z.id);
        for i in z.visible(offset) {
            let Some(rect) = z.rect(i, offset) else { continue };
            let focused = m.focused(z.id, i);
            match (z.id, z.items[i]) {
                ("apps" | "results", Target::App(a)) => {
                    let cell = Rect { x: rect.x + 4.0 * s, y: rect.y + 4.0 * s, w: rect.w - 8.0 * s, h: rect.h - 8.0 * s };
                    if focused {
                        p.highlight(m.keyboard, cell, 18.0 * s, th.ink(0.14), th.ink(0.8));
                    }
                    let app = &m.cat.apps[a];
                    let top = rect.y + (rect.h - f.icon - 30.0 * s) / 2.0;
                    p.icon(&app.icon, rect.x + (rect.w - f.icon) / 2.0, top, f.icon);
                    p.text_center(&app.name, rect.x + rect.w / 2.0, top + f.icon + 16.0 * s, 13.0 * s, Weight::Regular, ink, rect.w - 16.0 * s);
                }
                ("pages", Target::Page(n)) => {
                    let current = n == m.offset("apps");
                    p.circle(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, 4.0 * s, if current { ink } else { th.ink(0.3) });
                }
                _ => {}
            }
        }
    }
    if m.searching() && m.results.is_empty() {
        p.text_center("No Results", r.x + r.w / 2.0, f.grid.y + f.grid.h / 3.0, 18.0 * s, Weight::SemiBold, sub, r.w);
    }

    // The dash.
    p.fill(f.dash, 22.0 * s, th.ink(0.12));
    if let Some(z) = zones.iter().find(|z| z.id == "dash") {
        for (i, t) in z.items.iter().enumerate() {
            let Some(rect) = z.rect(i, 0) else { continue };
            let inner = Rect { x: rect.x + 4.0 * s, y: rect.y + 4.0 * s, w: rect.w - 8.0 * s, h: rect.h - 8.0 * s };
            if m.focused("dash", i) {
                p.highlight(m.keyboard, inner, 12.0 * s, th.ink(0.16), th.ink(0.8));
            }
            match t {
                Target::App(a) => p.icon(&m.cat.apps[*a].icon, rect.x + 8.0 * s, rect.y + 8.0 * s, f.dash_icon),
                _ => {
                    // the separator, then Show Apps, lit because the app grid is showing
                    p.fill(Rect { x: rect.x - 6.0 * s, y: rect.y + 12.0 * s, w: 1.0 * s, h: rect.h - 24.0 * s }, 0.0, th.ink(0.3));
                    p.circle(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, f.dash_icon / 2.0, th.ink(0.18));
                    p.glyph(Glyph::Grid, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, f.dash_icon * 0.45, ink);
                }
            }
        }
    }
}
