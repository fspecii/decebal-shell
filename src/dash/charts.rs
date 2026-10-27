//! Rings, sparklines and bars for the dashboard, drawn with tiny-skia paths.

use super::layout::Rect;
use crate::gfx::fill_rounded;
use tiny_skia::{Color, FillRule, FilterQuality, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pattern, Pixmap, Point, SpreadMode, Stroke, Transform};

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

/// Points of an arc starting at the top (12 o'clock) going clockwise for `frac` of a turn.
pub fn arc_points(cx: f32, cy: f32, r: f32, frac: f32) -> Vec<(f32, f32)> {
    let frac = frac.clamp(0.0, 1.0);
    let steps = ((frac * 90.0).ceil() as usize).max(1);
    (0..=steps)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * i as f32 / steps as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn polyline(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.finish()
}

/// A gauge: a faint full circle and `frac` of it in `fill`, starting at the top.
#[allow(clippy::too_many_arguments)] // a drawing helper: target, centre, radius, thickness, value, two colours
pub fn ring(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, thick: f32, frac: f32, track: Color, fill: Color) {
    let stroke = Stroke { width: thick, line_cap: LineCap::Round, ..Stroke::default() };
    if let Some(circle) = PathBuilder::from_circle(cx, cy, r) {
        pm.stroke_path(&circle, &paint(track), &Stroke { width: thick, ..Stroke::default() }, Transform::identity(), None);
    }
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.004 {
        return;
    }
    if frac >= 0.996 {
        if let Some(circle) = PathBuilder::from_circle(cx, cy, r) {
            pm.stroke_path(&circle, &paint(fill), &Stroke { width: thick, ..Stroke::default() }, Transform::identity(), None);
        }
        return;
    }
    if let Some(path) = polyline(&arc_points(cx, cy, r, frac)) {
        pm.stroke_path(&path, &paint(fill), &stroke, Transform::identity(), None);
    }
}

/// A horizontal bar: rounded track and the filled part.
pub fn bar(pm: &mut Pixmap, r: Rect, frac: f32, track: Color, fill: Color) {
    fill_rounded(pm, r.x, r.y, r.w, r.h, r.h / 2.0, track);
    let w = (r.w * frac.clamp(0.0, 1.0)).max(0.0);
    if w >= 1.0 {
        fill_rounded(pm, r.x, r.y, w.max(r.h), r.h, r.h / 2.0, fill);
    }
}

/// Points of a series scaled into `r`, newest at the right edge; the series is `slots` samples wide.
pub fn series_points(r: Rect, values: &[f32], max: f32, slots: usize) -> Vec<(f32, f32)> {
    let max = if max > 0.0 { max } else { 1.0 };
    let step = r.w / (slots.max(2) - 1) as f32;
    let n = values.len();
    values.iter().enumerate().map(|(i, v)| (r.right() - (n - 1 - i) as f32 * step, r.bottom() - (v / max).clamp(0.0, 1.0) * r.h)).collect()
}

/// A line chart with a soft gradient under it.
pub fn sparkline(pm: &mut Pixmap, r: Rect, values: &[f32], max: f32, slots: usize, line: Color, area: bool) {
    let pts = series_points(r, values, max, slots);
    if pts.len() < 2 {
        return;
    }
    if area {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, r.bottom());
        for p in &pts {
            pb.line_to(p.0, p.1);
        }
        pb.line_to(pts[pts.len() - 1].0, r.bottom());
        pb.close();
        if let Some(path) = pb.finish() {
            let mut top = line;
            top.set_alpha(line.alpha() * 0.30);
            let mut bottom = line;
            bottom.set_alpha(0.0);
            let mut p = paint(top);
            if let Some(g) = LinearGradient::new(
                Point::from_xy(0.0, r.y),
                Point::from_xy(0.0, r.bottom()),
                vec![GradientStop::new(0.0, top), GradientStop::new(1.0, bottom)],
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                p.shader = g;
            }
            pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        }
    }
    if let Some(path) = polyline(&pts) {
        let stroke = Stroke { width: 1.6, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        pm.stroke_path(&path, &paint(line), &stroke, Transform::identity(), None);
    }
}

/// Small vertical meters, one per value (0..=1), filling from the bottom of `r`.
pub fn meters(pm: &mut Pixmap, r: Rect, values: &[f32], track: Color, fill: Color) {
    let n = values.len();
    if n == 0 {
        return;
    }
    let gap = if n > 32 { 1.0 } else { 3.0 };
    let w = ((r.w - gap * (n as f32 - 1.0)) / n as f32).max(1.0);
    for (i, v) in values.iter().enumerate() {
        let x = r.x + i as f32 * (w + gap);
        fill_rounded(pm, x, r.y, w, r.h, (w / 2.0).min(3.0), track);
        let h = (r.h * v.clamp(0.0, 1.0)).max(if *v > 0.0 { 2.0 } else { 0.0 });
        if h > 0.0 {
            fill_rounded(pm, x, r.bottom() - h, w, h, (w / 2.0).min(3.0), fill);
        }
    }
}

/// A small filled triangle pointing up or down (network direction).
pub fn arrow(pm: &mut Pixmap, cx: f32, cy: f32, size: f32, up: bool, c: Color) {
    let (h, w) = (size, size * 0.9);
    let mut pb = PathBuilder::new();
    if up {
        pb.move_to(cx, cy - h / 2.0);
        pb.line_to(cx + w / 2.0, cy + h / 2.0);
        pb.line_to(cx - w / 2.0, cy + h / 2.0);
    } else {
        pb.move_to(cx, cy + h / 2.0);
        pb.line_to(cx + w / 2.0, cy - h / 2.0);
        pb.line_to(cx - w / 2.0, cy - h / 2.0);
    }
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

/// Draws `src` scaled into `r`, clipped to a rounded rectangle (album art).
pub fn image_rounded(pm: &mut Pixmap, src: &Pixmap, r: Rect, radius: f32) {
    if let Some(path) = crate::gfx::rounded_rect(r.x, r.y, r.w, r.h, radius) {
        fill_with_image(pm, &path, src, r);
    }
}

/// Draws `src` scaled into `r`, clipped to the circle inside it (the avatar).
pub fn image_circle(pm: &mut Pixmap, src: &Pixmap, r: Rect) {
    if let Some(path) = PathBuilder::from_circle(r.cx(), r.cy(), r.w.min(r.h) / 2.0) {
        fill_with_image(pm, &path, src, r);
    }
}

fn fill_with_image(pm: &mut Pixmap, path: &Path, src: &Pixmap, r: Rect) {
    if src.width() == 0 || src.height() == 0 {
        return;
    }
    let t = Transform::from_scale(r.w / src.width() as f32, r.h / src.height() as f32).post_translate(r.x, r.y);
    let p = Paint { anti_alias: true, shader: Pattern::new(src.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, t), ..Paint::default() };
    pm.fill_path(path, &p, FillRule::Winding, Transform::identity(), None);
}

/// Colour of a load gauge: the accent normally, warning past 80 %, danger past 92 %.
pub fn load_color(th: &crate::theme::Theme, frac: f32) -> Color {
    if frac >= 0.92 {
        th.danger.color(1.0)
    } else if frac >= 0.80 {
        th.warn.color(1.0)
    } else {
        th.accent.color(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::color;

    #[test]
    fn arc_starts_at_the_top_and_goes_clockwise() {
        let p = arc_points(50.0, 50.0, 20.0, 0.25);
        let (first, last) = (p[0], p[p.len() - 1]);
        assert!((first.0 - 50.0).abs() < 1e-3 && (first.1 - 30.0).abs() < 1e-3, "starts at 12 o'clock: {first:?}");
        assert!((last.0 - 70.0).abs() < 1e-3 && (last.1 - 50.0).abs() < 1e-3, "a quarter turn ends at 3 o'clock: {last:?}");
        let full = arc_points(0.0, 0.0, 10.0, 5.0);
        assert!((full[full.len() - 1].1 + 10.0).abs() < 1e-3, "clamped to one turn");
        assert!(arc_points(0.0, 0.0, 10.0, 0.0).len() >= 2);
    }

    #[test]
    fn series_is_right_aligned_and_scaled() {
        let r = Rect::new(10.0, 20.0, 60.0, 30.0);
        let p = series_points(r, &[0.0, 1.0, 0.5], 1.0, 4);
        assert_eq!(p.len(), 3);
        assert!((p[2].0 - 70.0).abs() < 1e-4, "newest at the right edge");
        assert!((p[0].0 - 30.0).abs() < 1e-4, "two steps of 20 px to the left");
        assert!((p[0].1 - 50.0).abs() < 1e-4 && (p[1].1 - 20.0).abs() < 1e-4 && (p[2].1 - 35.0).abs() < 1e-4);
        let over = series_points(r, &[5.0], 0.0, 4);
        assert!((over[0].1 - 20.0).abs() < 1e-4, "values clamp to the top, a zero max does not divide by zero");
    }

    fn painted(pm: &Pixmap) -> usize {
        pm.data().chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn drawing_primitives_put_pixels_down() {
        let (track, fill) = (color(255, 255, 255, 0.2), color(255, 0, 0, 1.0));
        let mut empty = Pixmap::new(80, 80).unwrap();
        ring(&mut empty, 40.0, 40.0, 30.0, 8.0, 0.0, track, fill);
        let mut half = Pixmap::new(80, 80).unwrap();
        ring(&mut half, 40.0, 40.0, 30.0, 8.0, 0.5, track, fill);
        let mut full = Pixmap::new(80, 80).unwrap();
        ring(&mut full, 40.0, 40.0, 30.0, 8.0, 1.0, track, fill);
        let red = |pm: &Pixmap| pm.data().chunks_exact(4).filter(|p| p[0] > 200 && p[1] < 60 && p[3] > 200).count();
        assert_eq!(red(&empty), 0);
        assert!(red(&half) > 200 && red(&full) > red(&half) * 3 / 2, "half {} full {}", red(&half), red(&full));
        // The right half is lit for 50 %, the left is not.
        let px = |pm: &Pixmap, x, y| pm.pixel(x, y).unwrap();
        assert!(px(&half, 70, 40).red() > 200, "3 o'clock lit");
        assert!(px(&half, 10, 40).red() < 100, "9 o'clock not lit");

        let mut pm = Pixmap::new(100, 40).unwrap();
        sparkline(&mut pm, Rect::new(0.0, 0.0, 100.0, 40.0), &[0.1, 0.9, 0.4, 0.8], 1.0, 4, fill, true);
        assert!(painted(&pm) > 300);
        let mut none = Pixmap::new(100, 40).unwrap();
        sparkline(&mut none, Rect::new(0.0, 0.0, 100.0, 40.0), &[0.5], 1.0, 4, fill, true);
        assert_eq!(painted(&none), 0, "one sample draws nothing");

        let mut m = Pixmap::new(100, 40).unwrap();
        meters(&mut m, Rect::new(0.0, 0.0, 100.0, 40.0), &[0.0, 0.5, 1.0], track, fill);
        assert!(m.pixel(50, 35).unwrap().red() > 150, "bottom of the half meter is filled");
        assert!(m.pixel(50, 5).unwrap().red() < 150, "its top is only track");
        assert!(m.pixel(95, 3).unwrap().red() > 150, "a full meter reaches the top");
        meters(&mut m, Rect::new(0.0, 0.0, 100.0, 40.0), &[], track, fill);

        let mut b = Pixmap::new(100, 10).unwrap();
        bar(&mut b, Rect::new(0.0, 3.0, 100.0, 4.0), 0.3, track, fill);
        assert!(b.pixel(20, 5).unwrap().red() > 200 && b.pixel(60, 5).unwrap().red() < 150);
        let mut a = Pixmap::new(20, 20).unwrap();
        arrow(&mut a, 10.0, 10.0, 10.0, true, fill);
        assert!(a.pixel(10, 12).unwrap().red() > 200 && a.pixel(2, 4).unwrap().alpha() == 0);
    }

    #[test]
    fn images_are_clipped_to_their_shape() {
        let mut src = Pixmap::new(4, 4).unwrap();
        src.fill(Color::from_rgba8(0, 255, 0, 255));
        let mut pm = Pixmap::new(60, 60).unwrap();
        image_rounded(&mut pm, &src, Rect::new(10.0, 10.0, 40.0, 40.0), 12.0);
        assert!(pm.pixel(30, 30).unwrap().green() > 240);
        assert_eq!(pm.pixel(10, 10).unwrap().alpha(), 0, "the corner is rounded away");
        let mut c = Pixmap::new(60, 60).unwrap();
        image_circle(&mut c, &src, Rect::new(10.0, 10.0, 40.0, 40.0));
        assert!(c.pixel(30, 30).unwrap().green() > 240 && c.pixel(11, 11).unwrap().alpha() == 0);
        image_circle(&mut c, &Pixmap::new(1, 1).unwrap(), Rect::default());
    }

    #[test]
    fn load_colour_steps() {
        let th = crate::theme::th();
        assert_eq!(load_color(&th, 0.1), th.accent.color(1.0));
        assert_eq!(load_color(&th, 0.85), th.warn.color(1.0));
        assert_eq!(load_color(&th, 0.99), th.danger.color(1.0));
    }
}
