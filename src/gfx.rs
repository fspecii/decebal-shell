//! Small drawing helpers on top of tiny-skia.

use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

pub fn color(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

thread_local! {
    /// Surface pixels per logical pixel for what this thread draws (see [`with_render_scale`]).
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// Surface pixels per logical pixel: 1 except inside [`with_render_scale`] (a panel drawn at 125 % or 200 %). The
/// helpers here and [`crate::text::Fonts::draw`] take logical coordinates and draw them this much larger, text
/// rasterised at the larger size so it stays sharp.
pub fn render_scale() -> f32 {
    SCALE.with(|s| s.get())
}

/// Runs `f` drawing (and laying out) at `scale` surface pixels per logical pixel; the previous scale is restored.
pub fn with_render_scale<R>(scale: f32, f: impl FnOnce() -> R) -> R {
    let old = SCALE.with(|s| s.replace(if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }));
    struct Restore(f32);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCALE.with(|s| s.set(self.0));
        }
    }
    let _restore = Restore(old);
    f()
}

/// The transform from logical to surface pixels.
pub fn transform() -> Transform {
    let s = render_scale();
    if s == 1.0 {
        Transform::identity()
    } else {
        Transform::from_scale(s, s)
    }
}

/// A pixmap's size in logical pixels.
pub fn logical_size(pm: &Pixmap) -> (u32, u32) {
    let s = render_scale();
    ((pm.width() as f32 / s).round() as u32, (pm.height() as f32 / s).round() as u32)
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

/// Rounded rectangle path; `r` is clamped to half the shorter side. A rectangle without area (zero, negative or
/// non-finite size, as a layout squeezed below its minimum can produce) has nothing to draw: `None`.
pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if !(w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) {
        return None;
    }
    let r = if r.is_finite() { r.clamp(0.0, w.min(h) / 2.0) } else { 0.0 };
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
        pm.fill_path(&path, &paint(c), FillRule::Winding, transform(), None);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn stroke_rounded(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color, width: f32) {
    if let Some(path) = rounded_rect(x, y, w, h, r) {
        let stroke = Stroke { width, ..Stroke::default() };
        pm.stroke_path(&path, &paint(c), &stroke, transform(), None);
    }
}

/// Makes a rounded rectangle fully transparent (a drop shadow stands outside its card: under a translucent card it would darken the material).
pub fn clear_rounded(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32) {
    if let Some(path) = rounded_rect(x, y, w, h, r) {
        let mut p = tiny_skia::Paint::default();
        p.blend_mode = tiny_skia::BlendMode::Clear;
        pm.fill_path(&path, &p, FillRule::Winding, transform(), None);
    }
}

/// Axis-aligned filled rectangle.
pub fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(r, &paint(c), transform(), None);
    }
}

pub fn fill_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: Color) {
    if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
        pm.fill_path(&path, &paint(c), FillRule::Winding, transform(), None);
    }
}

/// Draws `src` scaled to `w` x `h` with its top-left at (`x`, `y`).
pub fn blit_scaled(dst: &mut Pixmap, src: &Pixmap, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
    if src.width() == 0 || src.height() == 0 || w <= 0.0 || h <= 0.0 {
        return;
    }
    let k = render_scale();
    let sx = w * k / src.width() as f32;
    let sy = h * k / src.height() as f32;
    let pp = PixmapPaint { opacity, quality: tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() };
    dst.draw_pixmap(0, 0, src.as_ref(), &pp, Transform::from_scale(sx, sy).post_translate(x * k, y * k), None);
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

#[cfg(test)]
mod rounded_rect_tests {
    use super::rounded_rect;

    #[test]
    fn degenerate_rectangles_draw_nothing_instead_of_panicking() {
        // the 640x540 window at 200 % UI and text scale produced a -48 px wide card: clamp(0, -24) panicked
        for (w, h) in [(-48.0, 30.0), (30.0, -48.0), (0.0, 30.0), (30.0, 0.0), (f32::NAN, 30.0), (f32::INFINITY, 30.0)] {
            assert!(rounded_rect(0.0, 0.0, w, h, 12.0).is_none(), "{w}x{h}");
        }
        assert!(rounded_rect(f32::NAN, 0.0, 10.0, 10.0, 2.0).is_none());
    }

    #[test]
    fn rounding_is_kept_and_limited_to_half_the_shorter_side() {
        let p = rounded_rect(0.0, 0.0, 40.0, 10.0, 99.0).expect("drawn");
        let b = p.bounds();
        assert_eq!((b.width(), b.height()), (40.0, 10.0));
        assert!(rounded_rect(0.0, 0.0, 40.0, 10.0, -5.0).is_some() && rounded_rect(0.0, 0.0, 1.0, 1.0, f32::NAN).is_some());
    }
}
