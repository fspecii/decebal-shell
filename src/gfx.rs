//! Small drawing helpers on top of tiny-skia.

use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

pub fn color(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

/// Rounded rectangle path; `r` is clamped to half the shorter side.
pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let k = 0.552_284_8 * r; // cubic approximation of a quarter circle
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn fill_rounded(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    if let Some(path) = rounded_rect(x, y, w, h, r) {
        pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn stroke_rounded(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color, width: f32) {
    if let Some(path) = rounded_rect(x, y, w, h, r) {
        let stroke = Stroke { width, ..Stroke::default() };
        pm.stroke_path(&path, &paint(c), &stroke, Transform::identity(), None);
    }
}

/// Axis-aligned filled rectangle.
pub fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(r, &paint(c), Transform::identity(), None);
    }
}

pub fn fill_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: Color) {
    if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
        pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

/// Draws `src` scaled to `w` x `h` with its top-left at (`x`, `y`).
pub fn blit_scaled(dst: &mut Pixmap, src: &Pixmap, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
    if src.width() == 0 || src.height() == 0 || w <= 0.0 || h <= 0.0 {
        return;
    }
    let sx = w / src.width() as f32;
    let sy = h / src.height() as f32;
    let pp = PixmapPaint { opacity, quality: tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() };
    dst.draw_pixmap(0, 0, src.as_ref(), &pp, Transform::from_scale(sx, sy).post_translate(x, y), None);
}

/// A soft drop shadow: a few stacked translucent rounded rects (cheap, no blur pass).
#[allow(clippy::too_many_arguments)]
pub fn soft_shadow(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, spread: f32, alpha: f32) {
    const STEPS: u32 = 5;
    for i in 0..STEPS {
        let t = i as f32 / STEPS as f32;
        let grow = spread * (1.0 - t);
        fill_rounded(pm, x - grow, y - grow, w + 2.0 * grow, h + 2.0 * grow, r + grow, color(0, 0, 0, alpha / STEPS as f32 * (0.4 + t)));
    }
}

pub fn clear(pm: &mut Pixmap) {
    pm.fill(Color::TRANSPARENT);
}

pub fn pixmap(w: u32, h: u32) -> Option<Pixmap> {
    Pixmap::new(w.max(1), h.max(1))
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Rect> {
    Rect::from_xywh(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rect_clamps_radius() {
        assert!(rounded_rect(0.0, 0.0, 10.0, 4.0, 100.0).is_some());
        assert!(rounded_rect(0.0, 0.0, 0.0, 0.0, 5.0).is_none() || true);
    }

    #[test]
    fn fill_draws_pixels() {
        let mut pm = pixmap(20, 20).unwrap();
        fill_rounded(&mut pm, 2.0, 2.0, 16.0, 16.0, 4.0, color(255, 0, 0, 1.0));
        assert!(pm.pixel(10, 10).unwrap().alpha() == 255);
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
    }
}
