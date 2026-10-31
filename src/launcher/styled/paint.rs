//! Drawing helpers the styles share: shapes, text fitted to a width, app icons from the icon theme, and a few
//! glyphs drawn as vector paths in code (search, power, chevrons, grid, star, gear), so no artwork is shipped.

use super::super::placement::Rect;
use crate::gfx::{blit_scaled, fill_circle, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::text::{Fonts, Weight};
use crate::theme::model::Rgb;
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Search,
    Power,
    ChevronLeft,
    ChevronRight,
    Grid,
    Star,
    Gear,
    Clock,
    Apps,
    Close,
    Lines,
    /// A monitor on a stand (UKUI's `computer-symbolic`).
    Computer,
    /// Four corner brackets pointing out (view-fullscreen) / in (view-restore).
    Expand,
    Restore,
}

pub struct Painter<'a> {
    pub pm: &'a mut Pixmap,
    pub fonts: Option<&'a mut Fonts>,
    pub icons: &'a mut IconCache,
    /// UI scale: logical pixels to surface pixels.
    pub s: f32,
    /// The surface has an alpha channel; without it the base layers are painted opaque.
    pub alpha: bool,
}

impl Painter<'_> {
    /// Logical to surface pixels.
    pub fn u(&self, v: f32) -> f32 {
        v * self.s
    }

    /// A base-layer colour: `alpha` only when the surface can show it.
    pub fn base(&self, c: Rgb, alpha: f32) -> Color {
        c.color(if self.alpha { alpha } else { 1.0 })
    }

    pub fn fill(&mut self, r: Rect, radius: f32, c: Color) {
        if r.w > 0.0 && r.h > 0.0 {
            fill_rounded(self.pm, r.x, r.y, r.w, r.h, radius.min(r.w / 2.0).min(r.h / 2.0), c);
        }
    }

    pub fn stroke(&mut self, r: Rect, radius: f32, c: Color, width: f32) {
        if r.w > width && r.h > width {
            let h = width / 2.0;
            stroke_rounded(self.pm, r.x + h, r.y + h, r.w - width, r.h - width, (radius - h).max(0.0).min(r.w / 2.0).min(r.h / 2.0), c, width);
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        fill_circle(self.pm, cx, cy, r, c);
    }

    /// A soft drop shadow around a card (only where the surface has alpha).
    pub fn shadow(&mut self, r: Rect, radius: f32, strength: f32) {
        if self.alpha && strength > 0.0 {
            crate::gfx::soft_shadow(self.pm, r.x, r.y, r.w, r.h, radius, self.u(18.0), 0.28 * strength);
        }
    }

    pub fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, w))
    }

    /// `s` shortened with an ellipsis to fit `max` pixels.
    pub fn fit(&self, s: &str, px: f32, w: Weight, max: f32) -> String {
        let s: String = s.chars().filter(|c| !c.is_control()).take(200).collect();
        if self.measure(&s, px, w) <= max {
            return s;
        }
        let mut t = s;
        while !t.is_empty() && self.measure(&format!("{t}…"), px, w) > max {
            t.pop();
        }
        if t.is_empty() {
            String::new()
        } else {
            format!("{}…", t.trim_end())
        }
    }

    /// Text with its left edge at `x`, vertically centred on `cy`, cut to `max`; returns the width drawn.
    #[allow(clippy::too_many_arguments)] // text, position, size, weight, colour, width limit
    pub fn text(&mut self, s: &str, x: f32, cy: f32, px: f32, w: Weight, c: Color, max: f32) -> f32 {
        let s = self.fit(s, px, w, max.max(0.0));
        let width = self.measure(&s, px, w);
        if let Some(f) = self.fonts.as_mut() {
            let top = match f.ink_vertical("Ag", px, w) {
                Some((t, b)) => cy - (b - t) / 2.0 - t - f.ascent(px, w),
                None => cy - px * 0.62,
            };
            f.draw(self.pm, &s, x, top, px, w, c);
        }
        width
    }

    /// Text centred on `cx`.
    #[allow(clippy::too_many_arguments)]
    pub fn text_center(&mut self, s: &str, cx: f32, cy: f32, px: f32, w: Weight, c: Color, max: f32) {
        let fitted = self.fit(s, px, w, max);
        let width = self.measure(&fitted, px, w);
        self.text(&fitted, cx - width / 2.0, cy, px, w, c, max + 1.0);
    }

    /// Text whose right edge is at `right`.
    #[allow(clippy::too_many_arguments)]
    pub fn text_right(&mut self, s: &str, right: f32, cy: f32, px: f32, w: Weight, c: Color, max: f32) {
        let fitted = self.fit(s, px, w, max);
        let width = self.measure(&fitted, px, w);
        self.text(&fitted, right - width, cy, px, w, c, max + 1.0);
    }

    /// An app or category icon from the user's icon theme, `size` surface pixels square at (`x`, `y`).
    pub fn icon(&mut self, name: &str, x: f32, y: f32, size: f32) {
        let px = size.round().max(1.0) as u32;
        let icon = self.icons.get(name, px);
        blit_scaled(self.pm, icon, x.round(), y.round(), px as f32, px as f32, 1.0);
    }

    /// The hover / keyboard-focus look of an item: `fill` behind it, and a `ring` when the keyboard moved there.
    pub fn highlight(&mut self, keyboard: bool, r: Rect, radius: f32, fill: Color, ring: Color) {
        self.fill(r, radius, fill);
        if keyboard {
            self.stroke(r, radius, ring, self.u(2.0).max(1.5));
        }
    }

    /// The query (or a placeholder) in a search field with a text caret.
    #[allow(clippy::too_many_arguments)] // field text, placeholder, position, size, colours, width
    pub fn query(&mut self, text: &str, before_cursor: &str, placeholder: &str, x: f32, cy: f32, px: f32, ink: Color, faint: Color, max: f32) {
        if text.is_empty() {
            self.text(placeholder, x, cy, px, Weight::Regular, faint, max);
            self.fill(Rect { x: x.round(), y: (cy - px * 0.7).round(), w: self.u(1.0).max(1.0), h: (px * 1.4).round() }, 0.0, ink);
        } else {
            // keep the end of a long query (where the caret is) visible
            let shown = if self.measure(text, px, Weight::Regular) > max { text.chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect() } else { text.to_string() };
            self.text(&shown, x, cy, px, Weight::Regular, ink, max);
            let caret = if shown.len() == text.len() { self.measure(before_cursor, px, Weight::Regular) } else { self.measure(&shown, px, Weight::Regular) };
            self.fill(Rect { x: (x + caret.min(max)).round(), y: (cy - px * 0.7).round(), w: self.u(1.0).max(1.0), h: (px * 1.4).round() }, 0.0, ink);
        }
    }

    /// A category's icon from the icon theme when `themed`, else a tinted tile with its initial (icon themes such
    /// as Adwaita dropped the legacy `applications-*` names).
    pub fn category(&mut self, c: crate::launcher::catalog::Category, themed: bool, x: f32, y: f32, size: f32) {
        if themed {
            return self.icon(c.icon(), x, y, size);
        }
        let th = crate::theme::th();
        let r = Rect { x, y, w: size, h: size };
        self.fill(r, size * 0.25, th.accent.color(0.22));
        let initial: String = c.title().chars().take(1).collect();
        self.text_center(&initial, x + size / 2.0, y + size / 2.0, size * 0.55, Weight::SemiBold, th.accent.over(th.text, 0.6).color(1.0), size);
    }

    /// A round avatar with the user's initial.
    pub fn avatar(&mut self, name: &str, cx: f32, cy: f32, r: f32) {
        let th = crate::theme::th();
        self.circle(cx, cy, r, th.accent.color(1.0));
        let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        self.text_center(&initial, cx, cy, r * 1.05, Weight::SemiBold, th.accent_text.color(1.0), r * 2.0);
    }

    fn stroke_path(&mut self, pb: PathBuilder, c: Color, width: f32) {
        let Some(path) = pb.finish() else { return };
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        let stroke = Stroke { width, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
        self.pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    fn fill_path(&mut self, pb: PathBuilder, c: Color) {
        let Some(path) = pb.finish() else { return };
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        self.pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
    }

    /// A line glyph `size` pixels square centred on (`cx`, `cy`).
    pub fn glyph(&mut self, g: Glyph, cx: f32, cy: f32, size: f32, c: Color) {
        let h = size / 2.0;
        let lw = (size / 11.0).max(1.2);
        let mut pb = PathBuilder::new();
        match g {
            Glyph::Search => {
                pb.push_circle(cx - h * 0.18, cy - h * 0.18, h * 0.55);
                pb.move_to(cx + h * 0.22, cy + h * 0.22);
                pb.line_to(cx + h * 0.8, cy + h * 0.8);
            }
            Glyph::Power => {
                // a ring open at the top (35 degrees either side), and a bar down into the gap
                let r = h * 0.72;
                let steps = 28;
                for k in 0..=steps {
                    let a = (35.0 + 290.0 * k as f32 / steps as f32).to_radians();
                    let (x, y) = (cx + r * a.sin(), cy - r * a.cos());
                    if k == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                pb.move_to(cx, cy - h * 0.95);
                pb.line_to(cx, cy - h * 0.15);
            }
            Glyph::ChevronLeft | Glyph::ChevronRight => {
                let d = if g == Glyph::ChevronLeft { -1.0 } else { 1.0 };
                pb.move_to(cx - d * h * 0.25, cy - h * 0.5);
                pb.line_to(cx + d * h * 0.25, cy);
                pb.line_to(cx - d * h * 0.25, cy + h * 0.5);
            }
            Glyph::Close => {
                pb.move_to(cx - h * 0.5, cy - h * 0.5);
                pb.line_to(cx + h * 0.5, cy + h * 0.5);
                pb.move_to(cx + h * 0.5, cy - h * 0.5);
                pb.line_to(cx - h * 0.5, cy + h * 0.5);
            }
            Glyph::Lines => {
                for k in [-0.5f32, 0.0, 0.5] {
                    pb.move_to(cx - h * 0.7, cy + k * h);
                    pb.line_to(cx + h * 0.7, cy + k * h);
                }
            }
            Glyph::Computer => {
                if let Some(r) = tiny_skia::Rect::from_xywh(cx - h * 0.8, cy - h * 0.7, h * 1.6, h * 1.1) {
                    pb.push_rect(r);
                }
                pb.move_to(cx, cy + h * 0.4);
                pb.line_to(cx, cy + h * 0.75);
                pb.move_to(cx - h * 0.45, cy + h * 0.78);
                pb.line_to(cx + h * 0.45, cy + h * 0.78);
            }
            Glyph::Expand | Glyph::Restore => {
                // an L at each corner: out at the corners of the full square (expand), in at the corners of a smaller one, opening outward (restore)
                let (corner, arm, sign) = if g == Glyph::Expand { (h * 0.75, h * 0.4, 1.0) } else { (h * 0.28, h * 0.4, -1.0) };
                for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let (px, py) = (cx + sx * corner, cy + sy * corner);
                    pb.move_to(px - sx * sign * arm, py);
                    pb.line_to(px, py);
                    pb.line_to(px, py - sy * sign * arm);
                }
            }
            Glyph::Clock => {
                pb.push_circle(cx, cy, h * 0.8);
                pb.move_to(cx, cy - h * 0.5);
                pb.line_to(cx, cy);
                pb.line_to(cx + h * 0.35, cy + h * 0.25);
            }
            Glyph::Grid | Glyph::Apps => {
                // a 3x3 (grid) or 2x2 (apps) arrangement of dots / squares, filled
                let n = if g == Glyph::Grid { 3 } else { 2 };
                let step = size / n as f32;
                let mut fill = PathBuilder::new();
                for row in 0..n {
                    for col in 0..n {
                        let (x, y) = (cx - h + step * (col as f32 + 0.5), cy - h + step * (row as f32 + 0.5));
                        if g == Glyph::Grid {
                            fill.push_circle(x, y, step * 0.2);
                        } else if let Some(r) = tiny_skia::Rect::from_xywh(x - step * 0.36, y - step * 0.36, step * 0.72, step * 0.72) {
                            fill.push_rect(r);
                        }
                    }
                }
                self.fill_path(fill, c);
                return;
            }
            Glyph::Star => {
                let mut fill = PathBuilder::new();
                for k in 0..10 {
                    let r = if k % 2 == 0 { h * 0.95 } else { h * 0.42 };
                    let a = std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
                    let (x, y) = (cx + r * a.cos(), cy - r * a.sin());
                    if k == 0 {
                        fill.move_to(x, y);
                    } else {
                        fill.line_to(x, y);
                    }
                }
                fill.close();
                self.fill_path(fill, c);
                return;
            }
            Glyph::Gear => {
                pb.push_circle(cx, cy, h * 0.3);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_4;
                    pb.move_to(cx + h * 0.5 * a.cos(), cy + h * 0.5 * a.sin());
                    pb.line_to(cx + h * 0.85 * a.cos(), cy + h * 0.85 * a.sin());
                }
                pb.push_circle(cx, cy, h * 0.6);
            }
        }
        self.stroke_path(pb, c, lw);
    }
}
