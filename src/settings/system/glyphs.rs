//! White glyphs for the system panes' sidebar icons (drawn on the coloured rounded square by `icons::pane_icon`).

use crate::gfx::{color, fill_circle, fill_rounded, stroke_rounded};
use tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub const SPEAKER: u8 = 0;
pub const WIFI: u8 = 1;
pub const BLUETOOTH: u8 = 2;
pub const BELL: u8 = 3;
pub const MOUSE: u8 = 4;
pub const BATTERY: u8 = 5;
pub const APPS: u8 = 6;
pub const CLOCK: u8 = 7;
pub const USER: u8 = 8;
pub const LOCK: u8 = 9;
pub const INFO: u8 = 10;
pub const ACCESS: u8 = 11;
pub const GLOBE: u8 = 12;
pub const COUNT: u8 = 13;

fn stroke(pm: &mut Pixmap, pts: &[(f32, f32)], w: f32, c: Color) {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pb.line_to(p.0, p.1);
    }
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        pm.stroke_path(&path, &paint, &Stroke { width: w, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() }, Transform::identity(), None);
    }
}

fn poly(pm: &mut Pixmap, pts: &[(f32, f32)], c: Color) {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// Arc around (cx, cy) from angle a0 to a1 (radians, 0 = right, clockwise on screen).
fn arc(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, w: f32, c: Color) {
    let n = 14;
    let pts: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    stroke(pm, &pts, w, c);
}

pub fn draw(pm: &mut Pixmap, k: u8, bg: (u8, u8, u8), cx: f32, cy: f32, u: f32) {
    draw_tonal(pm, k, color(bg.0, bg.1, bg.2, 1.0), color(255, 255, 255, 0.96), cx, cy, u);
}

/// Theme-derived foreground and badge, shared with the Settings navigation painter.
pub fn draw_tonal(pm: &mut Pixmap, k: u8, bgc: Color, white: Color, cx: f32, cy: f32, u: f32) {
    let pi = std::f32::consts::PI;
    match k {
        SPEAKER => {
            poly(pm, &[(cx - 9.0 * u, cy - 3.0 * u), (cx - 5.0 * u, cy - 3.0 * u), (cx, cy - 8.0 * u), (cx, cy + 8.0 * u), (cx - 5.0 * u, cy + 3.0 * u), (cx - 9.0 * u, cy + 3.0 * u)], white);
            arc(pm, cx, cy, 4.5 * u, -pi / 3.0, pi / 3.0, 1.8 * u, white);
            arc(pm, cx, cy, 8.0 * u, -pi / 3.0, pi / 3.0, 1.8 * u, white);
        }
        WIFI => {
            for (i, r) in [4.0, 8.0, 12.0].iter().enumerate() {
                arc(pm, cx, cy + 7.0 * u, r * u, -pi * 0.82, -pi * 0.18, (2.0 - i as f32 * 0.1) * u, white);
            }
            fill_circle(pm, cx, cy + 6.5 * u, 1.8 * u, white);
        }
        BLUETOOTH => {
            stroke(
                pm,
                &[(cx - 5.0 * u, cy - 5.0 * u), (cx + 5.0 * u, cy + 4.0 * u), (cx, cy + 9.0 * u), (cx, cy - 9.0 * u), (cx + 5.0 * u, cy - 4.0 * u), (cx - 5.0 * u, cy + 5.0 * u)],
                1.9 * u,
                white,
            );
        }
        BELL => {
            let mut pb = PathBuilder::new();
            pb.move_to(cx - 7.0 * u, cy + 4.0 * u);
            pb.cubic_to(cx - 5.0 * u, cy + 1.0 * u, cx - 6.0 * u, cy - 8.0 * u, cx, cy - 8.0 * u);
            pb.cubic_to(cx + 6.0 * u, cy - 8.0 * u, cx + 5.0 * u, cy + 1.0 * u, cx + 7.0 * u, cy + 4.0 * u);
            pb.close();
            if let Some(p) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(white);
                paint.anti_alias = true;
                pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
            }
            fill_circle(pm, cx, cy + 7.0 * u, 2.0 * u, white);
        }
        MOUSE => {
            stroke_rounded(pm, cx - 5.5 * u, cy - 9.0 * u, 11.0 * u, 18.0 * u, 5.5 * u, white, 1.9 * u);
            stroke(pm, &[(cx, cy - 8.0 * u), (cx, cy - 2.0 * u)], 1.8 * u, white);
        }
        BATTERY => {
            stroke_rounded(pm, cx - 10.0 * u, cy - 5.0 * u, 18.0 * u, 10.0 * u, 2.5 * u, white, 1.8 * u);
            fill_rounded(pm, cx - 8.0 * u, cy - 3.0 * u, 11.0 * u, 6.0 * u, 1.2 * u, white);
            fill_rounded(pm, cx + 9.0 * u, cy - 2.0 * u, 2.0 * u, 4.0 * u, 1.0 * u, white);
        }
        APPS => {
            for r in 0..3 {
                for c in 0..3 {
                    fill_rounded(pm, cx - 8.5 * u + c as f32 * 6.0 * u, cy - 8.5 * u + r as f32 * 6.0 * u, 4.4 * u, 4.4 * u, 1.2 * u, white);
                }
            }
        }
        CLOCK => {
            stroke_circle(pm, cx, cy, 9.0 * u, 1.9 * u, white);
            stroke(pm, &[(cx, cy - 5.0 * u), (cx, cy), (cx + 4.0 * u, cy + 2.5 * u)], 1.8 * u, white);
        }
        USER => {
            fill_circle(pm, cx, cy - 4.0 * u, 4.2 * u, white);
            arc(pm, cx, cy + 10.0 * u, 8.5 * u, -pi * 0.95, -pi * 0.05, 4.2 * u, white);
        }
        LOCK => {
            fill_rounded(pm, cx - 7.0 * u, cy - 1.0 * u, 14.0 * u, 10.0 * u, 2.5 * u, white);
            arc(pm, cx, cy - 1.0 * u, 4.5 * u, pi, 2.0 * pi, 1.9 * u, white);
            fill_circle(pm, cx, cy + 4.0 * u, 1.5 * u, bgc);
        }
        INFO => {
            stroke_circle(pm, cx, cy, 9.0 * u, 1.9 * u, white);
            fill_circle(pm, cx, cy - 4.2 * u, 1.3 * u, white);
            stroke(pm, &[(cx, cy - 0.5 * u), (cx, cy + 5.0 * u)], 2.0 * u, white);
        }
        ACCESS => {
            stroke_circle(pm, cx, cy, 9.5 * u, 1.7 * u, white);
            fill_circle(pm, cx, cy - 5.0 * u, 1.5 * u, white);
            stroke(pm, &[(cx - 5.5 * u, cy - 2.0 * u), (cx + 5.5 * u, cy - 2.0 * u)], 1.7 * u, white);
            stroke(pm, &[(cx, cy - 2.0 * u), (cx, cy + 2.0 * u), (cx - 3.0 * u, cy + 7.0 * u)], 1.7 * u, white);
            stroke(pm, &[(cx, cy + 2.0 * u), (cx + 3.0 * u, cy + 7.0 * u)], 1.7 * u, white);
        }
        _ => {
            stroke_circle(pm, cx, cy, 9.0 * u, 1.8 * u, white);
            stroke(pm, &[(cx - 9.0 * u, cy), (cx + 9.0 * u, cy)], 1.5 * u, white);
            arc(pm, cx - 9.0 * u * 0.0, cy, 9.0 * u * 0.45, -pi / 2.0, pi / 2.0, 1.5 * u, white);
            arc(pm, cx, cy, 9.0 * u * 0.45, pi / 2.0, 1.5 * pi, 1.5 * u, white);
        }
    }
}

fn stroke_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, w: f32, c: Color) {
    arc(pm, cx, cy, r, 0.0, std::f32::consts::TAU, w, c);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_draws_something() {
        for k in 0..COUNT {
            let mut pm = Pixmap::new(48, 48).unwrap();
            pm.fill(Color::from_rgba8(0, 0, 0, 255));
            draw(&mut pm, k, (10, 132, 255), 24.0, 24.0, 2.0);
            assert!(pm.data().chunks_exact(4).any(|p| p[0] > 100), "glyph {k} drew nothing");
        }
    }
}
