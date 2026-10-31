//! Windows 11 Start: a search box on top, a paged Pinned grid (6 x 3) with an "All apps" button, Recommended
//! (recently used apps, 2 columns) and a footer with the user and the power button. All apps is an alphabetical list
//! with letter headings; a search shows the results beside a "best match" detail pane with Open and Pin to Start.
//! Geometry follows the Fluent Start layout (642 x 726 epx, 8 epx corners); colours come from the theme.

use super::paint::{Glyph, Painter};
use super::{Model, Target, View, Zone};
use crate::launcher::placement::Rect;
use crate::text::Weight;

struct Frame {
    search: Rect,
    header: Rect,
    footer: Rect,
    /// Recommended section (home view only), header included.
    recommended: Option<Rect>,
}

fn frame(m: &Model, r: Rect, s: f32) -> Frame {
    let pad = (32.0 * s).min(r.w / 12.0);
    // Short screens (a 600 px netbook at 2x) tighten the spacing; the sections then give way bottom-up.
    let compact = r.h < 560.0 * s;
    let gap = if compact { 8.0 * s } else { 20.0 * s };
    let search = Rect { x: r.x + pad, y: r.y + if compact { 12.0 * s } else { 24.0 * s }, w: r.w - 2.0 * pad, h: 36.0 * s };
    let footer_h = (64.0 * s).min(r.h / 6.0);
    let footer = Rect { x: r.x, y: r.bottom() - footer_h, w: r.w, h: footer_h };
    let header = Rect { x: search.x, y: search.bottom() + gap, w: search.w, h: 28.0 * s };
    let body = footer.y - header.bottom() - 12.0 * s;
    // The recommended block gets up to three rows and never more than 45 % of the body; none when no row fits.
    let row = 56.0 * s;
    let rows = (((body * 0.45) - 36.0 * s) / row).floor().clamp(0.0, 3.0);
    let recommended = (!m.searching() && m.view == View::Home && rows >= 1.0).then(|| {
        let h = 36.0 * s + rows * row;
        Rect { x: search.x, y: footer.y - 12.0 * s - h, w: search.w, h }
    });
    Frame { search, header, footer, recommended }
}

pub fn layout(m: &Model, r: Rect, s: f32) -> Vec<Zone> {
    let f = frame(m, r, s);
    let body_y = f.header.bottom() + 8.0 * s;
    let footer = {
        let b = (40.0 * s).min(f.footer.h - 4.0 * s).max(1.0);
        let cy = f.footer.y + f.footer.h / 2.0;
        let power = Rect { x: f.footer.right() - 32.0 * s - b, y: cy - b / 2.0, w: b, h: b };
        let settings = Rect { x: power.x - b - 4.0 * s, ..power };
        Zone::fixed("actions", vec![(Target::Settings, settings), (Target::Power, power)])
    };
    if m.searching() {
        let list = Rect { x: f.search.x, y: f.header.bottom() + 4.0 * s, w: (f.search.w * 0.48).round(), h: f.footer.y - f.header.bottom() - 16.0 * s };
        let results = Zone::columns("results", Model::app_targets(&m.results), list, 1, 48.0 * s, false);
        let mut zones = vec![results];
        if let Some(&i) = m.results.get(m.sel("results")).or(m.results.first()) {
            let d = detail(&f, s);
            zones.push(Zone::fixed("detail", vec![(Target::App(i), d.open), (Target::Pin(i), d.pin)]));
        }
        zones.push(footer);
        return zones;
    }
    match m.view {
        View::All | View::Category(_) | View::Recent | View::Favorites => {
            let back = Rect { x: f.header.right() - 76.0 * s, y: f.header.y, w: 76.0 * s, h: f.header.h };
            let list = Rect { x: f.search.x, y: body_y, w: f.search.w, h: f.footer.y - body_y - 12.0 * s };
            vec![Zone::fixed("nav", vec![(Target::View(View::Home), back)]), Zone::columns("apps", m.lettered(&m.cat.all()), list, 1, 40.0 * s, false), footer]
        }
        View::Home => {
            let all = Rect { x: f.header.right() - 88.0 * s, y: f.header.y, w: 88.0 * s, h: f.header.h };
            // Pinned: six columns, as many rows (up to three) as fit, paged; the page dots sit to the right.
            let bottom = f.recommended.map_or(f.footer.y, |rec| rec.y);
            let grid = Rect { x: f.search.x, y: body_y, w: f.search.w, h: (bottom - body_y - 8.0 * s).max(1.0) };
            let rows = ((grid.h / (84.0 * s)).floor()).clamp(1.0, 3.0);
            let cell_h = (84.0 * s).min(grid.h);
            let grid = Rect { h: rows * cell_h, ..grid };
            let pinned = Zone::columns("apps", Model::app_targets(&m.cat.favorites), grid, 6, cell_h, true);
            let mut zones = vec![Zone::fixed("nav", vec![(Target::View(View::All), all)]), pinned];
            let pages = zones[1].pages();
            if pages > 1 {
                let x = r.right() - 20.0 * s;
                let dot = 12.0 * s;
                let top = grid.y + grid.h / 2.0 - pages as f32 * dot / 2.0;
                zones.push(Zone::fixed("pages", (0..pages).map(|p| (Target::Page(p), Rect { x: x - dot / 2.0, y: top + p as f32 * dot, w: dot, h: dot })).collect()));
            }
            if let Some(rec) = f.recommended {
                let list = Rect { x: rec.x, y: rec.y + 36.0 * s, w: rec.w, h: rec.h - 36.0 * s };
                let recent: Vec<usize> = m.cat.recent.iter().copied().take(6).collect();
                zones.push(Zone::columns("recommended", Model::app_targets(&recent), list, 2, 56.0 * s, false));
            }
            zones.push(footer);
            zones
        }
    }
}

/// How much of the best-match pane fits: the large icon and name above the actions, a small header row, or
/// only the two actions side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    Full,
    Compact,
    Minimal,
}

/// The best-match pane and its Open / Pin to Start buttons, always inside the pane (and so the card).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Detail {
    pane: Rect,
    tier: Tier,
    open: Rect,
    pin: Rect,
}

fn detail(f: &Frame, s: f32) -> Detail {
    let x = f.search.x + (f.search.w * 0.48).round() + 12.0 * s;
    let pane = Rect { x, y: f.header.bottom() + 4.0 * s, w: (f.search.right() - x).max(1.0), h: (f.footer.y - f.header.bottom() - 16.0 * s).max(1.0) };
    let inset = (16.0 * s).min(pane.w / 8.0);
    let row = 36.0 * s;
    let stacked = |top: f32| {
        let open = Rect { x: pane.x + inset, y: top, w: pane.w - 2.0 * inset, h: row };
        (open, Rect { y: open.bottom() + 4.0 * s, ..open })
    };
    let (tier, (open, pin)) = if pane.h >= 248.0 * s {
        (Tier::Full, stacked(pane.y + 168.0 * s))
    } else if pane.h >= 140.0 * s {
        (Tier::Compact, stacked(pane.y + 60.0 * s))
    } else {
        // side by side, as tall as the pane allows (at most a normal row), with narrow margins
        let inset = (8.0 * s).min(pane.w / 8.0);
        let h = (pane.h - 8.0 * s).clamp(1.0, row);
        let w = ((pane.w - 2.0 * inset - 6.0 * s) / 2.0).max(1.0);
        let open = Rect { x: pane.x + inset, y: pane.y + (pane.h - h) / 2.0, w, h };
        (Tier::Minimal, (open, Rect { x: open.right() + 6.0 * s, ..open }))
    };
    Detail { pane, tier, open, pin }
}

pub fn paint(m: &Model, zones: &[Zone], r: Rect, p: &mut Painter) {
    let th = crate::theme::th();
    let s = p.s;
    let f = frame(m, r, s);
    let radius = 8.0 * s;
    let (ink, sub, faint) = (th.text_c(), th.sub(), th.faint());
    p.shadow(r, radius, 1.0);
    p.fill(r, radius, p.base(th.popup, th.popup_alpha.max(0.9)));
    p.stroke(r, radius, th.ink(0.10), 1.0);
    let hover = th.ink(0.07);

    // Search box: a pill with the magnifier at the start, the query and a caret.
    p.fill(f.search, f.search.h / 2.0, th.surface.color(1.0));
    p.stroke(f.search, f.search.h / 2.0, th.ink(0.10), 1.0);
    let cy = f.search.y + f.search.h / 2.0;
    p.glyph(Glyph::Search, f.search.x + 20.0 * s, cy, 15.0 * s, sub);
    p.fill(Rect { x: f.search.x + f.search.h / 2.0, y: f.search.bottom() - 2.0 * s, w: f.search.w - f.search.h, h: 2.0 * s }, s, th.accent_c());
    p.query(m.query(), m.field.before_cursor(), "Search for apps", f.search.x + 40.0 * s, cy, 13.0 * s, ink, faint, f.search.w - 60.0 * s);

    let title = if m.searching() {
        "Best match"
    } else if m.view == View::Home {
        "Pinned"
    } else {
        "All apps"
    };
    p.text(title, f.header.x + 20.0 * s, f.header.y + f.header.h / 2.0, 14.0 * s, Weight::SemiBold, ink, f.header.w / 2.0);

    for z in zones {
        let offset = m.offset(z.id);
        for i in z.visible(offset) {
            let Some(rect) = z.rect(i, offset) else { continue };
            let focused = m.focused(z.id, i);
            let t = z.items[i];
            match (z.id, t) {
                ("nav", Target::View(v)) => {
                    p.fill(rect, 4.0 * s, if focused { th.ink(0.12) } else { th.ink(0.06) });
                    if focused && m.keyboard {
                        p.stroke(rect, 4.0 * s, ink, 2.0 * s);
                    }
                    let cy = rect.y + rect.h / 2.0;
                    if v == View::Home {
                        p.glyph(Glyph::ChevronLeft, rect.x + 14.0 * s, cy, 10.0 * s, ink);
                        p.text("Back", rect.x + 26.0 * s, cy, 12.0 * s, Weight::Regular, ink, rect.w - 30.0 * s);
                    } else {
                        p.text("All apps", rect.x + 12.0 * s, cy, 12.0 * s, Weight::Regular, ink, rect.w - 30.0 * s);
                        p.glyph(Glyph::ChevronRight, rect.right() - 14.0 * s, cy, 10.0 * s, ink);
                    }
                }
                ("apps", Target::App(a)) if m.view == View::Home => {
                    let cell = Rect { x: rect.x + 2.0 * s, y: rect.y + 2.0 * s, w: rect.w - 4.0 * s, h: rect.h - 4.0 * s };
                    if focused {
                        p.highlight(m.keyboard, cell, 4.0 * s, hover, ink);
                    }
                    let app = &m.cat.apps[a];
                    // a squeezed cell (short screens) keeps the icon and drops the label
                    let icon = (32.0 * s).min(rect.h - 8.0 * s).max(8.0);
                    let top = if rect.h >= 80.0 * s { rect.y + 12.0 * s } else { rect.y + (rect.h - icon) / 2.0 };
                    p.icon(&app.icon, rect.x + (rect.w - icon) / 2.0, top, icon);
                    if rect.h >= 80.0 * s {
                        p.text_center(&app.name, rect.x + rect.w / 2.0, rect.y + 62.0 * s, 12.0 * s, Weight::Regular, ink, rect.w - 10.0 * s);
                    }
                }
                ("apps", Target::Letter(c)) => {
                    p.text(&c.to_string(), rect.x + 12.0 * s, rect.y + rect.h / 2.0, 14.0 * s, Weight::SemiBold, ink, rect.w);
                }
                ("apps" | "results" | "recommended", Target::App(a)) => {
                    if focused {
                        p.highlight(m.keyboard, rect, 4.0 * s, hover, ink);
                    }
                    let app = &m.cat.apps[a];
                    let size = if z.id == "apps" { 24.0 * s } else { 32.0 * s };
                    let cy = rect.y + rect.h / 2.0;
                    p.icon(&app.icon, rect.x + 12.0 * s, cy - size / 2.0, size);
                    let tx = rect.x + 24.0 * s + size;
                    let tw = rect.right() - tx - 8.0 * s;
                    match z.id {
                        "recommended" => {
                            p.text(&app.name, tx, cy - 8.0 * s, 12.0 * s, Weight::Regular, ink, tw);
                            p.text("Recently used", tx, cy + 9.0 * s, 11.0 * s, Weight::Regular, sub, tw);
                        }
                        "results" => {
                            p.text(&app.name, tx, cy - 8.0 * s, 13.0 * s, Weight::Regular, ink, tw);
                            p.text("App", tx, cy + 9.0 * s, 11.0 * s, Weight::Regular, sub, tw);
                        }
                        _ => {
                            p.text(&app.name, tx, cy, 13.0 * s, Weight::Regular, ink, tw);
                        }
                    }
                }
                ("pages", Target::Page(n)) => {
                    let current = n == m.offset("apps");
                    p.circle(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, if current { 3.0 * s } else { 2.0 * s }, if current { ink } else { th.ink(0.35) });
                }
                ("detail", t) => {
                    p.fill(rect, 4.0 * s, th.ink(0.05));
                    if focused {
                        p.highlight(m.keyboard, rect, 4.0 * s, hover, ink);
                    }
                    let cy = rect.y + rect.h / 2.0;
                    let minimal = detail(&f, s).tier == Tier::Minimal;
                    let (glyph, label) = match t {
                        Target::Pin(a) if m.cat.is_favorite(a) => (Glyph::Star, if minimal { "Unpin".to_string() } else { "Unpin from Start".to_string() }),
                        Target::Pin(_) => (Glyph::Star, if minimal { "Pin".to_string() } else { "Pin to Start".to_string() }),
                        // with no header above, Open names the app it opens
                        Target::App(a) if minimal => (Glyph::ChevronRight, format!("Open {}", m.cat.apps[a].name)),
                        _ => (Glyph::ChevronRight, "Open".to_string()),
                    };
                    let g = (12.0 * s).min(rect.h * 0.6);
                    p.glyph(glyph, rect.x + 14.0 * s, cy, g, th.accent_c());
                    let (px, room) = ((13.0 * s).min(rect.h * 0.7), rect.w - 30.0 * s);
                    // a narrow button keeps its verb rather than an unreadable "Open Rec…"
                    let short = label.split(' ').next().unwrap_or("").to_string();
                    let label = if p.measure(&label, px, Weight::Regular) <= room { label } else { short };
                    p.text(&label, rect.x + 26.0 * s, cy, px, Weight::Regular, ink, room);
                }
                _ => {}
            }
        }
    }

    if m.searching() {
        if m.results.is_empty() {
            p.text("No results", f.header.x + 20.0 * s, f.header.bottom() + 28.0 * s, 13.0 * s, Weight::Regular, sub, f.header.w);
        } else if let Some(&a) = m.results.get(m.sel("results")).or(m.results.first()) {
            // The best-match pane: the app (large, small or only named on Open), and its actions.
            let d = detail(&f, s);
            let pane = d.pane;
            p.fill(pane, 8.0 * s, th.ink(0.04));
            let app = &m.cat.apps[a];
            let cx = pane.x + pane.w / 2.0;
            match d.tier {
                Tier::Full => {
                    p.icon(&app.icon, cx - 32.0 * s, pane.y + 24.0 * s, 64.0 * s);
                    p.text_center(&app.name, cx, pane.y + 112.0 * s, 15.0 * s, Weight::SemiBold, ink, pane.w - 24.0 * s);
                    p.text_center("App", cx, pane.y + 134.0 * s, 12.0 * s, Weight::Regular, sub, pane.w - 24.0 * s);
                    p.fill(Rect { x: pane.x + 16.0 * s, y: pane.y + 156.0 * s, w: pane.w - 32.0 * s, h: 1.0 }, 0.0, th.ink(0.10));
                }
                Tier::Compact => {
                    p.icon(&app.icon, pane.x + 16.0 * s, pane.y + 10.0 * s, 32.0 * s);
                    p.text(&app.name, pane.x + 58.0 * s, pane.y + 18.0 * s, 14.0 * s, Weight::SemiBold, ink, pane.w - 70.0 * s);
                    p.text("App", pane.x + 58.0 * s, pane.y + 36.0 * s, 11.0 * s, Weight::Regular, sub, pane.w - 70.0 * s);
                    p.fill(Rect { x: pane.x + 16.0 * s, y: pane.y + 52.0 * s, w: pane.w - 32.0 * s, h: 1.0 }, 0.0, th.ink(0.10));
                }
                Tier::Minimal => {}
            }
        }
    } else if let Some(rec) = f.recommended {
        p.text("Recommended", rec.x + 20.0 * s, rec.y + 14.0 * s, 14.0 * s, Weight::SemiBold, ink, rec.w);
        if m.cat.recent.is_empty() && rec.h > 60.0 * s {
            p.text("The apps you open will show up here.", rec.x + 20.0 * s, rec.y + 52.0 * s, 12.0 * s, Weight::Regular, sub, rec.w - 40.0 * s);
        }
        if m.cat.favorites.is_empty() {
            p.text("Right-click an app in All apps to pin it here.", f.header.x + 20.0 * s, f.header.bottom() + 28.0 * s, 12.0 * s, Weight::Regular, sub, f.header.w - 40.0 * s);
        }
    }

    // Footer: the signed-in user and the power button, on a slightly darker band.
    let band = th.text.over(th.popup, 0.04);
    p.fill(Rect { y: f.footer.y, h: f.footer.h, ..r }, radius, p.base(band, th.popup_alpha.max(0.9)));
    p.fill(Rect { y: f.footer.y, h: radius, ..r }, 0.0, p.base(band, th.popup_alpha.max(0.9)));
    p.fill(Rect { x: r.x, y: f.footer.y, w: r.w, h: 1.0 }, 0.0, th.ink(0.10));
    let cy = f.footer.y + f.footer.h / 2.0;
    p.avatar(&m.user, f.footer.x + 48.0 * s + 16.0 * s, cy, 16.0 * s);
    p.text(&m.user, f.footer.x + 48.0 * s + 40.0 * s, cy, 12.0 * s, Weight::Regular, ink, f.footer.w / 2.0);
    if let Some(z) = zones.iter().find(|z| z.id == "actions") {
        for (i, t) in z.items.iter().enumerate() {
            let Some(rect) = z.rect(i, 0) else { continue };
            if m.focused("actions", i) {
                p.highlight(m.keyboard, rect, 4.0 * s, hover, ink);
            }
            let g = if *t == Target::Power { Glyph::Power } else { Glyph::Gear };
            p.glyph(g, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, 16.0 * s, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::placement::{placement, Anchor, Edge};
    use crate::launcher::style::LauncherStyle;

    fn inside(r: &Rect, outer: &Rect) -> bool {
        r.w >= 1.0 && r.h >= 1.0 && r.x >= outer.x - 0.5 && r.y >= outer.y - 0.5 && r.right() <= outer.right() + 0.5 && r.bottom() <= outer.bottom() + 0.5
    }

    #[test]
    fn best_match_actions_fit_the_card_on_every_screen_scale_and_panel() {
        let mut m = Model::new(crate::launcher::catalog::Catalog::with(
            (0..12).map(|i| crate::apps::AppEntry { name: format!("Recorder {i}"), exec: "true".into(), ..Default::default() }).collect(),
            Some(vec![]),
            Default::default(),
        ));
        m.field.insert("rec");
        m.results = m.cat.search("rec");
        let mut tiers = std::collections::HashSet::new();
        for screen in [(800.0, 480.0), (1024.0, 600.0), (1366.0, 768.0), (1920.0, 1080.0)] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for (edge, t) in [(Edge::Top, 64.0), (Edge::Bottom, 48.0 * scale), (Edge::Left, 40.0), (Edge::Right, 40.0)] {
                    let card = placement(LauncherStyle::Windows, Some(Anchor { edge, point: None, thickness: t }), screen, scale);
                    let zones = layout(&m, card, scale);
                    let d = zones.iter().find(|z| z.id == "detail").expect("the pane's actions");
                    let ctx = format!("{screen:?} @{scale} {edge:?} card {card:?}");
                    for (i, target) in d.items.iter().enumerate() {
                        let r = d.rect(i, 0).unwrap();
                        assert!(inside(&r, &card), "{target:?} {r:?} outside the card: {ctx}");
                    }
                    let det = detail(&frame(&m, card, scale), scale);
                    assert!(inside(&det.open, &det.pane) && inside(&det.pin, &det.pane), "{ctx}");
                    assert!(det.open.x + det.open.w <= det.pin.x + 0.5 || det.open.bottom() <= det.pin.y + 0.5, "Open and Pin never overlap: {ctx}");
                    tiers.insert(format!("{:?}", det.tier));
                    for z in &zones {
                        for i in z.visible(0) {
                            assert!(inside(&z.rect(i, 0).unwrap(), &card), "{} {i}: {ctx}", z.id);
                        }
                    }
                }
            }
        }
        assert_eq!(tiers.len(), 3, "the sweep reaches the full, compact and minimal panes: {tiers:?}");
    }
}
