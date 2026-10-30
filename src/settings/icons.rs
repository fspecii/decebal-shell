//! Pane icons (coloured rounded squares with a white glyph) and layout pictograms.
//!
//! [`layout_rects`] is the single description of what each rdwm layout looks like; decbar's layout
//! switcher can draw the same pictograms through [`layout_icon`].

use super::ui::fill_px;
use crate::gfx::{color, fill_circle, fill_rounded, stroke_rounded};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub type R = (f32, f32, f32, f32);

fn cols(r: R, n: usize) -> Vec<R> {
    (0..n).map(|i| (r.0 + r.2 * i as f32 / n as f32, r.1, r.2 / n as f32, r.3)).collect()
}
fn rows(r: R, n: usize) -> Vec<R> {
    (0..n).map(|i| (r.0, r.1 + r.3 * i as f32 / n as f32, r.2, r.3 / n as f32)).collect()
}
fn master_stack(master_w: f32, stack: usize, mirrored: bool) -> Vec<R> {
    let mut v = vec![(0.0, 0.0, master_w, 1.0)];
    v.extend(rows((master_w, 0.0, 1.0 - master_w, 1.0), stack));
    if mirrored {
        v.iter_mut().for_each(|r| r.0 = 1.0 - r.0 - r.2);
    }
    v
}
/// Fibonacci-style halving: `spiral` winds around the centre, otherwise alternates left/top.
fn fib(n: usize, spiral: bool) -> Vec<R> {
    let mut rest: R = (0.0, 0.0, 1.0, 1.0);
    let mut out = Vec::new();
    for i in 0..n {
        if i == n - 1 {
            out.push(rest);
            break;
        }
        let (x, y, w, h) = rest;
        let dir = if spiral { i % 4 } else { (i % 2) * 2 };
        let (win, next) = match (spiral, dir) {
            (false, d) if d % 2 == 0 => ((x, y, w / 2.0, h), (x + w / 2.0, y, w / 2.0, h)),
            (false, _) => ((x, y, w, h / 2.0), (x, y + h / 2.0, w, h / 2.0)),
            (true, 0) => ((x, y, w / 2.0, h), (x + w / 2.0, y, w / 2.0, h)),
            (true, 1) => ((x, y, w, h / 2.0), (x, y + h / 2.0, w, h / 2.0)),
            (true, 2) => ((x + w / 2.0, y, w / 2.0, h), (x, y, w / 2.0, h)),
            _ => ((x, y + h / 2.0, w, h / 2.0), (x, y, w, h / 2.0)),
        };
        out.push(win);
        rest = next;
    }
    out
}

/// Windows of `name` in the unit square (five windows where the layout has a count).
pub fn layout_rects(name: &str) -> Vec<R> {
    let unit = (0.0, 0.0, 1.0, 1.0);
    match name {
        "tile" => master_stack(0.56, 3, false),
        "rtile" => master_stack(0.56, 3, true),
        "monocle" => vec![unit],
        "floating" => vec![(0.04, 0.08, 0.52, 0.56), (0.34, 0.3, 0.6, 0.56), (0.14, 0.52, 0.4, 0.42)],
        "bottomstack" => {
            let mut v = vec![(0.0, 0.0, 1.0, 0.55)];
            v.extend(cols((0.0, 0.55, 1.0, 0.45), 3));
            v
        }
        "bstackhoriz" => {
            let mut v = vec![(0.0, 0.0, 1.0, 0.46)];
            v.extend(rows((0.0, 0.46, 1.0, 0.54), 3));
            v
        }
        "grid" => {
            let mut v = cols((0.0, 0.0, 1.0, 0.5), 3);
            v.extend(cols((0.0, 0.5, 1.0, 0.5), 2));
            v
        }
        "gaplessgrid" => {
            let mut v = rows((0.0, 0.0, 0.5, 1.0), 3);
            v.extend(rows((0.5, 0.0, 0.5, 1.0), 2));
            v
        }
        "horizgrid" => {
            let mut v = cols((0.0, 0.0, 1.0, 0.5), 2);
            v.extend(cols((0.0, 0.5, 1.0, 0.5), 3));
            v
        }
        "nrowgrid" => {
            let mut v = rows((0.0, 0.0, 1.0 / 3.0, 1.0), 2);
            v.extend(rows((1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0), 2));
            v.push((2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0));
            v
        }
        "deck" => vec![(0.0, 0.0, 0.55, 1.0), (0.55, 0.12, 0.45, 0.88), (0.6, 0.06, 0.4, 0.06), (0.65, 0.0, 0.35, 0.05)],
        "centeredmaster" => {
            let mut v = vec![(0.27, 0.0, 0.46, 1.0)];
            v.extend(rows((0.0, 0.0, 0.27, 1.0), 2));
            v.extend(rows((0.73, 0.0, 0.27, 1.0), 2));
            v
        }
        "centeredfloatingmaster" => {
            let mut v = cols(unit, 4);
            v.push((0.2, 0.18, 0.6, 0.64));
            v
        }
        "dwindle" => fib(5, false),
        "spiral" => fib(5, true),
        "tatami" => vec![(0.0, 0.0, 0.6, 0.5), (0.6, 0.0, 0.4, 0.62), (0.0, 0.5, 0.32, 0.5), (0.32, 0.5, 0.3, 0.5), (0.62, 0.62, 0.38, 0.38)],
        "columns" => cols(unit, 4),
        "rows" => rows(unit, 4),
        "threecol" => {
            let mut v = vec![(0.0, 0.0, 0.4, 1.0)];
            v.extend(rows((0.4, 0.0, 0.3, 1.0), 2));
            v.extend(rows((0.7, 0.0, 0.3, 1.0), 2));
            v
        }
        "bsp" => vec![(0.0, 0.0, 0.5, 1.0), (0.5, 0.0, 0.5, 0.5), (0.5, 0.5, 0.25, 0.5), (0.75, 0.5, 0.25, 0.5)],
        "scroll" => vec![(-0.1, 0.0, 0.3, 1.0), (0.24, 0.0, 0.42, 1.0), (0.7, 0.0, 0.3, 1.0)],
        "tabbed" => {
            let mut v = cols((0.0, 0.0, 1.0, 0.16), 4);
            v.push((0.0, 0.2, 1.0, 0.8));
            v
        }
        "stacked" => vec![(0.0, 0.0, 1.0, 0.13), (0.0, 0.17, 1.0, 0.13), (0.0, 0.34, 1.0, 0.66)],
        _ => vec![unit],
    }
}

/// Draws the pictogram of layout `name` filling `area` (x, y, w, h in px).
pub fn layout_icon(pm: &mut Pixmap, name: &str, area: R, s: f32, c: Color) {
    let gap = (1.6 * s).max(1.0);
    for r in layout_rects(name) {
        let (x0, x1) = ((area.0 + r.0 * area.2).max(area.0), (area.0 + (r.0 + r.2) * area.2).min(area.0 + area.2));
        let (y0, y1) = (area.1 + r.1 * area.3, area.1 + (r.1 + r.3) * area.3);
        let (w, h) = (x1 - x0 - gap, y1 - y0 - gap);
        if w > 0.5 && h > 0.5 {
            fill_rounded(pm, x0 + gap / 2.0, y0 + gap / 2.0, w, h, (2.2 * s).min(w / 2.0).min(h / 2.0), c);
        }
    }
}

// ---------------------------------------------------------------- pane icons

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Gear,
    Brush,
    Dock,
    Photo,
    Tiles,
    Windows,
    Rules,
    Keyboard,
    Rocket,
    Monitor,
    /// Icon-theme pane: a small grid of app icons.
    Icons,
    /// Glyph of a system pane (`system::glyphs`).
    Sys(u8),
}

fn line(pm: &mut Pixmap, pts: &[(f32, f32)], w: f32, c: Color) {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pb.line_to(p.0, p.1);
    }
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        pm.stroke_path(&path, &paint, &Stroke { width: w, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() }, Transform::identity(), None);
    }
}

/// A coloured rounded square with a white glyph, `size` px wide.
pub fn pane_icon(pm: &mut Pixmap, g: Glyph, bg: (u8, u8, u8), x: f32, y: f32, size: f32) {
    pane_icon_tonal(pm, g, color(bg.0, bg.1, bg.2, 1.0), color(255, 255, 255, 0.96), x, y, size);
}

/// Native vector glyph with theme-derived badge and ink; no per-frame raster allocation.
#[allow(clippy::too_many_arguments)]
pub fn pane_icon_tonal(pm: &mut Pixmap, g: Glyph, bg: tiny_skia::Color, fg: tiny_skia::Color, x: f32, y: f32, size: f32) {
    fill_rounded(pm, x, y, size, size, size * 0.24, bg);
    pane_glyph(pm, g, bg, fg, x, y, size);
}

/// The glyph alone, with no badge behind it. `bg` must be the opaque colour it is drawn on: cut-outs (gear hub, brush
/// bite, ...) are painted in it.
#[allow(clippy::too_many_arguments)]
pub fn pane_glyph(pm: &mut Pixmap, g: Glyph, bg: tiny_skia::Color, fg: tiny_skia::Color, x: f32, y: f32, size: f32) {
    let (cx, cy, u) = (x + size / 2.0, y + size / 2.0, size / 24.0);
    let mut muted = fg;
    muted.set_alpha(fg.alpha() * 0.75 / 0.96);
    let mut faint = fg;
    faint.set_alpha(fg.alpha() * 0.70 / 0.96);
    match g {
        Glyph::Gear => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (dx, dy) = (a.cos(), a.sin());
                line(pm, &[(cx + dx * 6.0 * u, cy + dy * 6.0 * u), (cx + dx * 9.0 * u, cy + dy * 9.0 * u)], 3.2 * u, fg);
            }
            fill_circle(pm, cx, cy, 7.0 * u, fg);
            fill_circle(pm, cx, cy, 3.0 * u, bg);
        }
        Glyph::Brush => {
            fill_circle(pm, cx, cy, 8.5 * u, fg);
            fill_circle(pm, cx + 3.5 * u, cy - 2.5 * u, 5.0 * u, bg);
            fill_circle(pm, cx - 2.0 * u, cy + 3.0 * u, 2.2 * u, bg);
        }
        Glyph::Dock => {
            fill_rounded(pm, cx - 9.5 * u, cy + 2.0 * u, 19.0 * u, 7.0 * u, 3.0 * u, fg);
            for i in 0..3 {
                fill_rounded(pm, cx - 7.0 * u + i as f32 * 5.6 * u, cy - 7.0 * u, 4.2 * u, 4.2 * u, 1.2 * u, muted);
            }
        }
        Glyph::Photo => {
            stroke_rounded(pm, cx - 9.0 * u, cy - 7.5 * u, 18.0 * u, 15.0 * u, 3.0 * u, fg, 2.0 * u);
            fill_circle(pm, cx + 3.5 * u, cy - 2.5 * u, 2.0 * u, fg);
            line(pm, &[(cx - 8.0 * u, cy + 6.0 * u), (cx - 2.0 * u, cy), (cx + 2.0 * u, cy + 4.0 * u), (cx + 5.0 * u, cy + 1.5 * u), (cx + 8.0 * u, cy + 5.0 * u)], 2.0 * u, fg);
        }
        Glyph::Tiles => {
            for (dx, dy, w, h) in [(-9.0, -8.0, 10.0, 16.0), (2.0, -8.0, 7.0, 7.0), (2.0, 1.0, 7.0, 7.0)] {
                fill_rounded(pm, cx + dx * u, cy + dy * u, w * u, h * u, 2.0 * u, fg);
            }
        }
        Glyph::Windows => {
            stroke_rounded(pm, cx - 9.0 * u, cy - 7.0 * u, 12.0 * u, 11.0 * u, 2.5 * u, faint, 1.8 * u);
            fill_rounded(pm, cx - 3.0 * u, cy - 3.0 * u, 12.0 * u, 11.0 * u, 2.5 * u, fg);
            fill_px(pm, cx - 3.0 * u, cy - 0.5 * u, 12.0 * u, 1.6 * u, bg);
        }
        Glyph::Rules => {
            for i in 0..3 {
                let yy = cy - 6.0 * u + i as f32 * 6.0 * u;
                fill_circle(pm, cx - 7.0 * u, yy, 1.5 * u, fg);
                line(pm, &[(cx - 3.0 * u, yy), (cx + 8.5 * u, yy)], 2.0 * u, fg);
            }
        }
        Glyph::Keyboard => {
            stroke_rounded(pm, cx - 10.0 * u, cy - 6.5 * u, 20.0 * u, 13.0 * u, 3.0 * u, fg, 1.8 * u);
            for row in 0..2 {
                for i in 0..4 {
                    fill_px(pm, cx - 6.5 * u + i as f32 * 3.6 * u, cy - 3.0 * u + row as f32 * 3.6 * u, 2.2 * u, 2.0 * u, fg);
                }
            }
            fill_px(pm, cx - 4.0 * u, cy + 3.6 * u, 8.0 * u, 1.8 * u, fg);
        }
        Glyph::Rocket => {
            let mut pb = PathBuilder::new();
            pb.move_to(cx, cy - 10.0 * u);
            pb.cubic_to(cx + 6.0 * u, cy - 6.0 * u, cx + 6.0 * u, cy + 3.0 * u, cx + 4.0 * u, cy + 7.0 * u);
            pb.line_to(cx - 4.0 * u, cy + 7.0 * u);
            pb.cubic_to(cx - 6.0 * u, cy + 3.0 * u, cx - 6.0 * u, cy - 6.0 * u, cx, cy - 10.0 * u);
            pb.close();
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(fg);
                paint.anti_alias = true;
                pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
            }
            fill_circle(pm, cx, cy - 2.5 * u, 2.2 * u, bg);
            line(pm, &[(cx - 3.0 * u, cy + 10.0 * u), (cx + 3.0 * u, cy + 10.0 * u)], 2.0 * u, faint);
        }
        Glyph::Icons => {
            for (dx, dy, s) in [(-8.5, -8.5, 7.5), (1.0, -8.5, 7.5), (-8.5, 1.0, 7.5), (1.0, 1.0, 7.5)] {
                fill_rounded(pm, cx + dx * u, cy + dy * u, s * u, s * u, 2.0 * u, fg);
            }
            fill_rounded(pm, cx - 3.2 * u, cy - 3.2 * u, 6.4 * u, 6.4 * u, 1.6 * u, bg);
            fill_rounded(pm, cx - 1.6 * u, cy - 1.6 * u, 3.2 * u, 3.2 * u, 0.8 * u, fg);
        }
        Glyph::Sys(k) => super::system::glyphs::draw_tonal(pm, k, bg, fg, cx, cy, u),
        Glyph::Monitor => {
            fill_rounded(pm, cx - 10.0 * u, cy - 8.0 * u, 20.0 * u, 13.5 * u, 2.5 * u, fg);
            line(pm, &[(cx, cy + 5.5 * u), (cx, cy + 8.5 * u)], 2.0 * u, fg);
            line(pm, &[(cx - 4.5 * u, cy + 9.0 * u), (cx + 4.5 * u, cy + 9.0 * u)], 2.2 * u, fg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_has_windows_inside_the_unit_square() {
        for (name, _) in LAYOUT_NAMES {
            let rs = layout_rects(name);
            assert!(!rs.is_empty(), "{name}");
            for r in rs {
                assert!(r.2 > 0.0 && r.3 > 0.0, "{name}: {r:?}");
                if *name != "scroll" {
                    assert!(r.0 >= -1e-6 && r.1 >= -1e-6 && r.0 + r.2 <= 1.0 + 1e-6 && r.1 + r.3 <= 1.0 + 1e-6, "{name}: {r:?}");
                }
            }
        }
    }

    const LAYOUT_NAMES: &[(&str, &str)] = &[
        ("tile", ""),
        ("monocle", ""),
        ("floating", ""),
        ("bottomstack", ""),
        ("bstackhoriz", ""),
        ("rtile", ""),
        ("grid", ""),
        ("gaplessgrid", ""),
        ("horizgrid", ""),
        ("nrowgrid", ""),
        ("deck", ""),
        ("centeredmaster", ""),
        ("centeredfloatingmaster", ""),
        ("dwindle", ""),
        ("spiral", ""),
        ("tatami", ""),
        ("columns", ""),
        ("rows", ""),
        ("threecol", ""),
        ("bsp", ""),
        ("scroll", ""),
        ("tabbed", ""),
        ("stacked", ""),
    ];

    #[test]
    fn tonal_navigation_glyphs_use_supplied_ink() {
        let bg = color(8, 12, 16, 1.0);
        let fg = color(80, 100, 90, 1.0);
        let mut glyphs = vec![Glyph::Gear, Glyph::Brush, Glyph::Dock, Glyph::Photo, Glyph::Tiles, Glyph::Windows, Glyph::Rules, Glyph::Keyboard, Glyph::Rocket, Glyph::Icons, Glyph::Monitor];
        glyphs.extend((0..super::super::system::glyphs::COUNT).map(Glyph::Sys));
        for glyph in glyphs {
            let mut pm = Pixmap::new(56, 56).unwrap();
            pane_icon_tonal(&mut pm, glyph, bg, fg, 4.0, 4.0, 48.0);
            assert!(pm.data().chunks_exact(4).any(|p| p[1] > 50), "missing foreground: {glyph:?}");
            assert!(pm.data().chunks_exact(4).all(|p| p[0] <= 80 && p[1] <= 100 && p[2] <= 90), "hardcoded ink: {glyph:?}");
        }
    }

    #[test]
    fn flat_glyphs_draw_ink_without_a_badge() {
        let bg = color(24, 24, 28, 1.0);
        let fg = color(120, 190, 255, 1.0);
        let mut glyphs = vec![Glyph::Gear, Glyph::Brush, Glyph::Dock, Glyph::Photo, Glyph::Tiles, Glyph::Windows, Glyph::Rules, Glyph::Keyboard, Glyph::Rocket, Glyph::Monitor];
        glyphs.extend((0..super::super::system::glyphs::COUNT).map(Glyph::Sys));
        for g in glyphs {
            let mut pm = Pixmap::new(48, 48).unwrap();
            pane_glyph(&mut pm, g, bg, fg, 4.0, 4.0, 40.0);
            let px = pm.data().chunks_exact(4);
            assert!(px.clone().any(|p| p[3] > 0), "nothing drawn: {g:?}");
            // the badge corner is untouched: nothing but the glyph is painted
            assert_eq!(&pm.data()[4 * (4 * 48 + 4)..4 * (4 * 48 + 4) + 4], &[0, 0, 0, 0], "badge fill leaked: {g:?}");
        }
    }

    #[test]
    fn icons_draw() {
        let mut pm = Pixmap::new(64, 64).unwrap();
        layout_icon(&mut pm, "spiral", (4.0, 4.0, 56.0, 56.0), 1.0, color(255, 255, 255, 1.0));
        assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0));
        for g in [Glyph::Gear, Glyph::Brush, Glyph::Dock, Glyph::Photo, Glyph::Tiles, Glyph::Windows, Glyph::Rules, Glyph::Keyboard, Glyph::Rocket, Glyph::Monitor] {
            pane_icon(&mut pm, g, (10, 132, 255), 8.0, 8.0, 48.0);
        }
    }
}
