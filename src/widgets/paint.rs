//! Draws a laid-out widget with the current theme's colours.
//!
//! Every leaf takes logical coordinates and draws through [`gfx::transform`] (or a gfx helper / [`Fonts::draw`] that does),
//! so inside [`gfx::with_render_scale`] a card drawn at 125 % or 200 % has the same geometry as at 1x, only sharper. Strokes,
//! clip paths and image patterns are transformed together; pixel snapping happens in surface pixels.

use super::dsl::{Align, ColorRef};
use super::layout::{Item, Leaf, Metrics, Rect};
use crate::dash::charts;
use crate::gfx;
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use crate::theme::model::{Rgb, Theme};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::{Color, FillRule, FilterQuality, Paint, Pattern, Pixmap, SpreadMode, Transform};

/// Real font metrics for layout.
pub struct FontMetrics<'a>(pub &'a Fonts);

impl Metrics for FontMetrics<'_> {
    fn text_scale(&self) -> f32 {
        crate::theme::live::font_scale()
    }
    fn text_width(&self, s: &str, px: f32, bold: bool) -> f32 {
        self.0.measure(s, px, if bold { Weight::Bold } else { Weight::Regular })
    }
    fn mono_cell_width(&self, px: f32) -> f32 {
        self.0.measure_as(crate::text::Role::Mono, "M", px, Weight::Regular)
    }
    fn line_height(&self, px: f32) -> f32 {
        (px * crate::theme::live::font_scale() * 1.25).ceil()
    }
}

/// Decoded images by path, so redraws do not re-read files. Relative paths resolve under `base` and may not leave it.
#[derive(Default)]
pub struct ImageCache {
    pub base: PathBuf,
    map: HashMap<String, Option<Pixmap>>,
}

impl ImageCache {
    pub fn new(base: &Path) -> ImageCache {
        ImageCache { base: base.to_path_buf(), map: HashMap::new() }
    }

    pub fn get(&mut self, path: &str) -> Option<&Pixmap> {
        let base = self.base.clone();
        self.map.entry(path.to_string()).or_insert_with(|| load(&base, path)).as_ref()
    }
}

fn load(base: &Path, path: &str) -> Option<Pixmap> {
    let p = Path::new(path);
    // A `file` of a package widget is relative to the package; a data source (cover art) is an absolute path from the player.
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        super::super::theme::engine::archive::safe_rel_path(path).ok()?;
        base.join(p)
    };
    let meta = std::fs::symlink_metadata(&full).ok()?;
    if !meta.is_file() && !(p.is_absolute() && meta.file_type().is_symlink()) || meta.len() > 32 << 20 {
        return None;
    }
    let img = image::ImageReader::open(&full).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if img.width() > 1024 || img.height() > 1024 { img.thumbnail(1024, 1024) } else { img };
    let rgba = img.to_rgba8();
    let mut pm = Pixmap::new(rgba.width(), rgba.height())?;
    for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(rgba.pixels()) {
        let a = u32::from(s[3]);
        d[0] = (u32::from(s[0]) * a / 255) as u8;
        d[1] = (u32::from(s[1]) * a / 255) as u8;
        d[2] = (u32::from(s[2]) * a / 255) as u8;
        d[3] = s[3];
    }
    Some(pm)
}

pub struct Painter<'a> {
    pub fonts: &'a mut Fonts,
    pub symbols: &'a mut SymbolCache,
    pub images: &'a mut ImageCache,
    pub theme: &'a Theme,
    /// Per-instance material opacity; foreground text and controls remain readable.
    pub background_opacity: f32,
}

pub fn token(th: &Theme, name: &str) -> Rgb {
    match name {
        "background" => th.background,
        "sidebar" => th.sidebar,
        "surface" => th.surface,
        "bar" => th.bar,
        "bar_text" => th.bar_text,
        "popup" => th.popup,
        "dock" => th.dock,
        "overlay" => th.overlay,
        "titlebar" => th.titlebar,
        "titlebar_inactive" => th.titlebar_inactive,
        "border" => th.border,
        "text_secondary" => th.text_secondary,
        "text_faint" => th.text_faint,
        "accent" => th.accent,
        "accent_text" => th.accent_text,
        "danger" => th.danger,
        "ok" => th.ok,
        "warn" => th.warn,
        _ => th.text,
    }
}

pub fn rgb_of(th: &Theme, c: &ColorRef) -> (Rgb, f32) {
    match c {
        ColorRef::Token(n, a) => (token(th, n), *a),
        ColorRef::Fixed(c, a) => (*c, *a),
    }
}

fn col(th: &Theme, c: &ColorRef, op: f32) -> Color {
    let (rgb, a) = rgb_of(th, c);
    rgb.color(a * op)
}

/// Theme surface alphas apply to material backgrounds, never to foreground text.
fn background_color(p: &Painter, color: &ColorRef, opacity: f32) -> Color {
    let material_alpha = match color {
        ColorRef::Token(token, _) => match token.as_str() {
            "dock" => p.theme.dock_alpha,
            "bar" => p.theme.bar_alpha,
            "overlay" => p.theme.panel_alpha,
            "popup" | "background" | "surface" | "sidebar" | "titlebar" | "titlebar_inactive" => p.theme.popup_alpha,
            _ => p.theme.popup_alpha,
        },
        ColorRef::Fixed(..) => 1.0,
    };
    col(p.theme, color, opacity * material_alpha * p.background_opacity)
}

/// Saved instance option, also supported by off-screen rendering fixtures.
pub fn background_opacity(data: &super::data::Data) -> f32 {
    data.num("opt.background_opacity").filter(|v| v.is_finite()).unwrap_or(1.0).clamp(0.0, 1.0) as f32
}

/// Paints `it` (and its children) into `pm`; (0, 0) of the item tree is the pixmap's top-left.
pub fn paint(pm: &mut Pixmap, it: &Item, p: &mut Painter, parent_opacity: f32) {
    let op = parent_opacity * it.opacity;
    let r = it.rect;
    if let Some(bg) = &it.background {
        let color = if it.kind == "button" { col(p.theme, bg, op) } else { background_color(p, bg, op) };
        gfx::fill_rounded(pm, r.x, r.y, r.w, r.h, it.radius, color);
    }
    if let Some(b) = &it.border {
        let w = it.border_width;
        gfx::stroke_rounded(pm, r.x + w / 2.0, r.y + w / 2.0, r.w - w, r.h - w, (it.radius - w / 2.0).max(0.0), col(p.theme, b, op), w);
    }
    match &it.leaf {
        Leaf::None => {}
        Leaf::Text { s, size, bold, color, align } => {
            if s.is_empty() {
                return;
            }
            let weight = if *bold { Weight::Bold } else { Weight::Regular };
            let w = p.fonts.measure(s, *size, weight);
            let lh = FontMetrics(p.fonts).line_height(*size);
            let x = match align {
                Align::Center => r.x + (r.w - w) / 2.0,
                Align::End => r.x + r.w - w,
                _ => r.x,
            };
            p.fonts.draw(pm, s, x, r.y + (r.h - lh) / 2.0, *size, weight, col(p.theme, color, op));
        }
        Leaf::DotText { s, size, color } => {
            super::dots::paint(pm, s, *size, r.x, r.y + (r.h - size) / 2.0, col(p.theme, color, op));
        }
        Leaf::Ascii { lines, size, cell, color } => {
            let line_height = size * 1.25;
            let width = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0) as f32 * cell;
            let x = r.x + (r.w - width).max(0.0) / 2.0;
            let y = r.y + (r.h - lines.len() as f32 * line_height).max(0.0) / 2.0;
            // block edges on whole surface pixels, so neighbouring cells meet without an antialiased seam at any scale
            let k = gfx::render_scale();
            let snap = |v: f32| (v * k).round() / k;
            for (row, line) in lines.iter().enumerate() {
                for (column, ch) in line.chars().enumerate() {
                    let (x, y) = (x + column as f32 * cell, y + row as f32 * line_height);
                    if ch == '█' {
                        // Terminal full blocks fill their entire cell, with no font-dependent seams.
                        gfx::fill_rounded(pm, snap(x), snap(y), snap(x + cell) - snap(x), snap(y + line_height) - snap(y), 0.0, col(p.theme, color, op));
                    } else if ch != ' ' && *size > 0.0 {
                        p.fonts.draw_as(crate::text::Role::Mono, pm, &ch.to_string(), x, y, *size, Weight::Regular, col(p.theme, color, op));
                    }
                }
            }
        }
        Leaf::Icon { name, size, color } => {
            let (rgb, a) = rgb_of(p.theme, color);
            // rasterised at the surface size (the same key as before at 1x), then drawn into the logical box
            if let Some(img) = p.symbols.get(name, rgb.t(), (size * gfx::render_scale()).round().max(1.0) as u32) {
                gfx::blit_scaled(pm, img, r.x + (r.w - size) / 2.0, r.y + (r.h - size) / 2.0, *size, *size, a * op);
            }
        }
        Leaf::Bar { frac, thickness, color, track } => {
            let y = r.y + (r.h - thickness) / 2.0;
            // Narrow waveform bars must not expand their fill to the bar's height.
            gfx::fill_rounded(pm, r.x, y, r.w, *thickness, r.w.min(*thickness) / 2.0, col(p.theme, track, op));
            let width = r.w * frac.clamp(0.0, 1.0);
            if width >= 1.0 {
                gfx::fill_rounded(pm, r.x, y, width, *thickness, width.min(*thickness) / 2.0, col(p.theme, color, op));
            }
        }
        Leaf::AnalogClock { hour, minute, second, size, color, track, second_color, numerals, seconds } => {
            let d = size.min(r.w).min(r.h);
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let radius = d * 0.48;
            let ink = col(p.theme, color, op);
            let tick = col(p.theme, track, op);
            charts::ring(pm, cx, cy, radius, (d * 0.006).max(0.7), 1.0, tick, tick);
            for n in 1..=12 {
                let turn = n as f32 / 12.0;
                let (x, y) = clock_point(cx, cy, radius * 0.90, turn);
                if *numerals {
                    let text = n.to_string();
                    let px = d * 0.085;
                    let width = p.fonts.measure(&text, px, Weight::Bold);
                    let lh = FontMetrics(p.fonts).line_height(px);
                    p.fonts.draw(pm, &text, x - width / 2.0, y - lh / 2.0, px, Weight::Bold, ink);
                } else {
                    let (ex, ey) = clock_point(cx, cy, radius * 0.80, turn);
                    clock_line(pm, x, y, ex, ey, (d * 0.012).max(1.0), tick);
                }
            }
            for (turn, length, width, c) in [(*hour, 0.48, 0.028, ink), (*minute, 0.73, 0.018, ink)] {
                let (x, y) = clock_point(cx, cy, radius * length, turn);
                clock_line(pm, cx, cy, x, y, (d * width).max(1.0), c);
            }
            if *seconds {
                let (x, y) = clock_point(cx, cy, radius * 0.76, *second);
                clock_line(pm, cx, cy, x, y, (d * 0.007).max(0.8), col(p.theme, second_color, op));
            }
            gfx::fill_circle(pm, cx, cy, (d * 0.023).max(1.0), ink);
        }
        Leaf::Ring { frac, size, thickness, color, track } => {
            let d = size.min(r.w).min(r.h);
            charts::ring(pm, r.x + r.w / 2.0, r.y + r.h / 2.0, (d - thickness) / 2.0, *thickness, *frac, col(p.theme, track, op), col(p.theme, color, op));
        }
        Leaf::Graph { values, max, color, fill } => {
            charts::sparkline(pm, crate::dash::layout::Rect::new(r.x, r.y + 1.0, r.w, (r.h - 2.0).max(1.0)), values, *max, 60, col(p.theme, color, op), *fill);
        }
        Leaf::Image { path, radius, cover } => {
            if let Some(img) = path.as_deref().and_then(|f| p.images.get(f)) {
                draw_image(pm, img, r, *radius, *cover, op);
            }
        }
        Leaf::Dots { tags, size, gap, color, active } => {
            let total = tags.len() as f32 * (size + gap) - gap;
            let mut x = r.x + ((r.w - total) / 2.0).max(0.0);
            for t in tags {
                let c = if t.urgent {
                    p.theme.danger.color(op)
                } else if t.active {
                    col(p.theme, active, op)
                } else {
                    let (rgb, a) = rgb_of(p.theme, color);
                    rgb.color((if t.occupied { a.max(0.6) } else { a * 0.5 }) * op)
                };
                let w = if t.active { size * 1.0 } else { *size };
                gfx::fill_circle(pm, x + w / 2.0, r.y + r.h / 2.0, w / 2.0, c);
                x += size + gap;
            }
        }
    }
    for c in &it.children {
        paint(pm, c, p, op);
    }
}

/// Fractions of a clockwise turn, with zero at twelve o'clock.
fn clock_point(cx: f32, cy: f32, radius: f32, turn: f32) -> (f32, f32) {
    let a = turn * std::f32::consts::TAU;
    (cx + a.sin() * radius, cy - a.cos() * radius)
}

fn clock_line(pm: &mut Pixmap, x: f32, y: f32, ex: f32, ey: f32, width: f32, color: Color) {
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(x, y);
    path.line_to(ex, ey);
    if let Some(path) = path.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke { width, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
        pm.stroke_path(&path, &paint, &stroke, gfx::transform(), None);
    }
}

fn draw_image(pm: &mut Pixmap, img: &Pixmap, r: Rect, radius: f32, cover: bool, op: f32) {
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    if iw == 0.0 || ih == 0.0 || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let s = if cover { (r.w / iw).max(r.h / ih) } else { (r.w / iw).min(r.h / ih) };
    let (w, h) = (iw * s, ih * s);
    let (x, y) = (r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0);
    // pattern and clip are logical; the paint transform takes both to the surface, so the cached source picture is
    // resampled once at the surface resolution and the rounded clip stays on the box
    let paint = Paint { shader: Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, op, Transform::from_scale(s, s).post_translate(x, y)), anti_alias: true, ..Paint::default() };
    // cover crops to the box; contain keeps the whole picture
    let (bx, by, bw, bh) = if cover { (r.x, r.y, r.w, r.h) } else { (x, y, w, h) };
    if let Some(path) = gfx::rounded_rect(bx, by, bw, bh, radius) {
        pm.fill_path(&path, &paint, FillRule::Winding, gfx::transform(), None);
    }
}

/// Renders `def` with `data` on a transparent pixmap `margin` px larger than the card on every side (room for a drop shadow).
pub fn render(def: &super::dsl::WidgetDef, data: &super::data::Data, th: &Theme, fonts: &mut Fonts, base: &Path, margin: u32) -> Option<Pixmap> {
    let it = super::layout::layout(def, &super::layout::Ctx { data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, None);
    let (w, h) = (it.rect.w.ceil() as u32 + 2 * margin, it.rect.h.ceil() as u32 + 2 * margin);
    let mut pm = Pixmap::new(w, h)?;
    let mut sym = SymbolCache::default();
    let mut imgs = ImageCache::new(base);
    let mut shifted = it;
    shift(&mut shifted, margin as f32);
    paint(&mut pm, &shifted, &mut Painter { fonts, symbols: &mut sym, images: &mut imgs, theme: th, background_opacity: background_opacity(data) }, 1.0);
    fonts.trim();
    Some(pm)
}

fn shift(it: &mut Item, d: f32) {
    it.rect.x += d;
    it.rect.y += d;
    for c in &mut it.children {
        shift(c, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::model::{Def, Selection};
    use crate::widgets::data;
    use crate::widgets::data::sample;
    use crate::widgets::layout::{layout, Ctx, Fixed};
    use crate::widgets::{dsl, Trust};

    #[test]
    fn analog_clock_parser_layout_and_fixed_time_hands() {
        let src = "format=1\n[root]\ntype=\"analog-clock\"\nsize=160\nseconds=false\nnumerals=false\n";
        let def = dsl::parse(src, Trust::Theme).unwrap();
        assert!(def.paths.contains("clock"));
        assert!(dsl::parse(&src.replace("size=160", "size=0"), Trust::Theme).is_err());
        assert!(dsl::parse(&src.replace("seconds=false", "seconds=7"), Trust::Theme).is_err());
        let mut data = data::sample();
        data.time.hour = 3;
        data.time.min = 0;
        data.time.sec = 0;
        let it = super::super::layout::layout(&def, &super::super::layout::Ctx { data: &data, metrics: &super::super::layout::Fixed, theme_radius: 18.0 }, None);
        assert_eq!((it.rect.w, it.rect.h), (160.0, 160.0));
        let Leaf::AnalogClock { hour, minute, second, .. } = it.leaf else { panic!("clock leaf") };
        assert_eq!((hour, minute, second), (0.25, 0.0, 0.0));
        let right = clock_point(80.0, 80.0, 40.0, hour);
        let up = clock_point(80.0, 80.0, 40.0, minute);
        assert!((right.0 - 120.0).abs() < 0.001 && (right.1 - 80.0).abs() < 0.001);
        assert!((up.0 - 80.0).abs() < 0.001 && (up.1 - 40.0).abs() < 0.001);
        data.time.hour = 12;
        data.time.min = 30;
        data.time.sec = 30;
        let it = super::super::layout::layout(&def, &super::super::layout::Ctx { data: &data, metrics: &super::super::layout::Fixed, theme_radius: 18.0 }, None);
        let Leaf::AnalogClock { hour, minute, second, .. } = it.leaf else { panic!("clock leaf") };
        assert!((hour - 0.04236111).abs() < 0.00001);
        assert!((minute - 0.5083333).abs() < 0.00001);
        assert_eq!(second, 0.5);
    }

    #[test]
    fn analog_clock_renders_hands_and_transparent_corners() {
        let Some(mut fonts) = fonts() else { return };
        let def = dsl::parse("format=1\n[root]\ntype=\"analog-clock\"\nsize=160\nseconds=false\nnumerals=false\ncolor=\"#ff0000\"\ntrack=\"#00ff00\"\n", Trust::Theme).unwrap();
        let mut data = data::sample();
        data.time.hour = 3;
        data.time.min = 0;
        data.time.sec = 0;
        let pm = render(&def, &data, &theme(), &mut fonts, Path::new("."), 0).unwrap();
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
        assert!(px(&pm, 105, 80)[0] > 240, "hour hand points right");
        assert!(px(&pm, 80, 40)[0] > 240, "minute hand points up");
        assert_eq!(pm.pixel(60, 80).unwrap().alpha(), 0, "no opposite hour hand");
        assert_eq!(pm.pixel(80, 115).unwrap().alpha(), 0, "no opposite minute hand");
    }

    #[test]
    fn narrow_bars_never_paint_outside_their_allocated_width() {
        let mut fonts = fonts().expect("render verification requires fonts");
        let def = dsl::parse("format=1\nsize=[80,80]\n[root]\ntype=\"row\"\npadding=10\n[[root.children]]\ntype=\"bar\"\nwidth=8\nheight=40\nvalue=1\n", Trust::Theme).unwrap();
        let pm = render(&def, &sample(), &theme(), &mut fonts, Path::new("."), 0).unwrap();
        assert!(pm.pixel(14, 30).unwrap().alpha() > 0);
        assert_eq!(pm.pixel(30, 30).unwrap().alpha(), 0);
        for y in 0..80 {
            for x in 19..80 {
                assert_eq!(pm.pixel(x, y).unwrap().alpha(), 0, "overflow at {x},{y}");
            }
        }
    }

    #[test]
    fn theme_surface_opacity_and_instance_opacity_preserve_foreground_and_controls() {
        let def = dsl::parse("format=1\nsize=[120,60]\n[root]\ntype=\"row\"\nbackground=\"popup@0.8\"\n[[root.children]]\ntype=\"button\"\nwidth=20\nheight=20\nbackground=\"accent\"\n", Trust::Theme)
            .unwrap();
        let mut fonts = fonts().expect("render verification requires fonts");
        let mut th = theme();
        th.popup_alpha = 0.5;
        let mut data = sample();
        data.set("opt.background_opacity", data::Val::Num(0.5));
        let pm = render(&def, &data, &th, &mut fonts, Path::new("."), 0).unwrap();
        assert!(pm.pixel(100, 50).unwrap().alpha().abs_diff(51) <= 1, "theme .5 × token .8 × instance .5");
        assert_eq!(pm.pixel(10, 10).unwrap().alpha(), 255, "button stays opaque");
        data.set("opt.background_opacity", data::Val::Num(0.0));
        let pm = render(&def, &data, &th, &mut fonts, Path::new("."), 0).unwrap();
        assert_eq!(pm.pixel(100, 50).unwrap().alpha(), 0);
        assert_eq!(pm.pixel(10, 10).unwrap().alpha(), 255);
        for (token, expected) in [("popup", 0.5), ("surface", 0.5), ("dock", 0.3), ("bar", 0.4), ("overlay", 0.6)] {
            th.dock_alpha = 0.3;
            th.bar_alpha = 0.4;
            th.panel_alpha = 0.6;
            let (mut symbols, mut images) = (SymbolCache::default(), ImageCache::default());
            let p = Painter { fonts: &mut fonts, symbols: &mut symbols, images: &mut images, theme: &th, background_opacity: 1.0 };
            assert!((background_color(&p, &ColorRef::Token(token.into(), 1.0), 1.0).alpha() - expected).abs() < 0.003);
        }
    }

    #[test]
    fn ascii_full_blocks_form_a_seamless_fixed_cell_grid() {
        let mut fonts = fonts().expect("render verification requires fonts");
        let def = dsl::parse("format=1\nsize=[40,30]\n[root]\ntype=\"ascii\"\ntext=\"{fetch.logo}\"\nsize=12\n", Trust::Theme).unwrap();
        let mut data = sample();
        data.set("fetch.logo", data::Val::Text("██\n██".into()));
        let pm = render(&def, &data, &theme(), &mut fonts, Path::new("."), 0).unwrap();
        for y in 0..30 {
            for x in 15..25 {
                assert_eq!(pm.pixel(x, y).unwrap().alpha(), 255, "seam at {x},{y}");
            }
        }
        assert_eq!(pm.pixel(0, 15).unwrap().alpha(), 0, "logo is centered in its slot");
    }

    fn theme() -> Theme {
        crate::theme::builtin::def("nord").unwrap().resolve("nord", true, &Selection::default())
    }

    fn fonts() -> Option<Fonts> {
        Fonts::load()
    }

    const SHAPES: &str = r##"
format = 1
size = [200, 80]
[root]
type = "row"
padding = 10
gap = 10
align = "center"
background = "popup"
radius = 16
[[root.children]]
type = "ring"
value = "cpu.usage"
size = 56
thickness = 8
color = "accent"
[[root.children]]
type = "col"
gap = 8
align = "stretch"
grow = 1
[[root.children.children]]
type = "bar"
value = "mem.usage"
height = 8
color = "ok"
[[root.children.children]]
type = "graph"
series = "cpu.history"
height = 24
color = "warn"
"##;

    fn render_shapes() -> Pixmap {
        let def = dsl::parse(SHAPES, Trust::Theme).unwrap();
        let d = sample();
        let th = theme();
        let it = layout(&def, &Ctx { data: &d, metrics: &Fixed, theme_radius: th.radius }, None);
        let mut pm = Pixmap::new(200, 80).unwrap();
        // shapes only: text is not involved, so no fonts are needed and the pixels are the same on every machine
        let mut fonts = match fonts() {
            Some(f) => f,
            None => return pm,
        };
        let (mut sym, mut imgs) = (SymbolCache::default(), ImageCache::default());
        paint(&mut pm, &it, &mut Painter { fonts: &mut fonts, symbols: &mut sym, images: &mut imgs, theme: &th, background_opacity: 1.0 }, 1.0);
        pm
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * pm.width() + x) * 4) as usize;
        pm.data()[i..i + 4].try_into().unwrap()
    }

    fn near(p: [u8; 4], c: Rgb) -> bool {
        [(p[0], c.0), (p[1], c.1), (p[2], c.2)].iter().all(|(a, b)| a.abs_diff(*b) <= 3)
    }

    #[test]
    fn rings_bars_and_graphs_land_where_the_layout_says() {
        let pm = render_shapes();
        if pm.data().iter().all(|b| *b == 0) {
            eprintln!("(no fonts here: skipped)");
            return;
        }
        let th = theme();
        // the card: rounded, so its corner is clear; its middle is the popup colour
        assert_eq!(px(&pm, 0, 0)[3], 0, "rounded corner is transparent");
        assert!(near(px(&pm, 100, 3), th.popup), "{:?}", px(&pm, 100, 3));
        // the ring is 56 px at (10, 12), 8 thick: its arc starts at 12 o'clock, the centre of the stroke at (38, 16)
        assert!(near(px(&pm, 38, 16), th.accent), "ring start {:?}", px(&pm, 38, 16));
        // 37 % of a turn clockwise ends before 9 o'clock: the left of the ring is only the track
        assert!(!near(px(&pm, 14, 40), th.accent), "ring is not full");
        // the memory bar sits at (76, 20), 114 x 8: filled for 62 %, then the track
        assert!(near(px(&pm, 82, 24), th.ok), "bar fill {:?}", px(&pm, 82, 24));
        assert!(!near(px(&pm, 180, 24), th.ok), "bar track");
    }

    /// A pixel hash of a text-free widget: tiny-skia is deterministic, so a change here is a change in drawing.
    #[test]
    fn shapes_render_is_deterministic() {
        let (a, b) = (render_shapes(), render_shapes());
        let (ha, hb) = (crate::theme::engine::sha256::digest_hex(a.data()), crate::theme::engine::sha256::digest_hex(b.data()));
        assert_eq!(ha, hb);
        if cfg!(target_arch = "x86_64") && a.data().iter().any(|b| *b != 0) {
            assert_eq!(ha, GOLDEN_SHAPES, "the shapes widget changed (update GOLDEN_SHAPES if intended)");
        }
        let _ = Def::default();
    }

    const GOLDEN_SHAPES: &str = "eb9033d15cb6fda3476678bd60a24109de9f61a3e46d568975b8bc13913427ae";

    // The same cards at 1x, 125 % and 200 %, painted as the Wayland host paints them: logical layout, a surface-sized pixmap
    // and `gfx::with_render_scale`. Bounds are measured on the pixels, not recomputed with the drawing helpers.

    const SCALES: [f32; 3] = [1.0, 1.25, 2.0];

    fn paint_at(k: f32, w: f32, h: f32, it: &Item, fonts: &mut Fonts, images: &mut ImageCache) -> Pixmap {
        let mut pm = Pixmap::new((w * k).ceil() as u32, (h * k).ceil() as u32).unwrap();
        let th = theme();
        let mut symbols = SymbolCache::default();
        gfx::with_render_scale(k, || paint(&mut pm, it, &mut Painter { fonts, symbols: &mut symbols, images, theme: &th, background_opacity: 1.0 }, 1.0));
        pm
    }

    /// Inclusive box, in surface pixels, of the pixels `ink(x, y, rgba)` accepts.
    fn ink_box(pm: &Pixmap, ink: impl Fn(u32, u32, [u8; 4]) -> bool) -> Option<(u32, u32, u32, u32)> {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in 0..pm.height() {
            for x in 0..pm.width() {
                if ink(x, y, px(pm, x, y)) {
                    b = Some(b.map_or((x, y, x, y), |(x0, y0, x1, y1)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y))));
                }
            }
        }
        b
    }

    /// What one leaf added to a card: the box of the pixels that differ between the two renders.
    fn diff_box(a: &Pixmap, b: &Pixmap) -> Option<(u32, u32, u32, u32)> {
        ink_box(a, |x, y, p| p != px(b, x, y))
    }

    /// The surface span `got` (inclusive pixel indices) covers logical `lo..hi` at scale `k`, within an antialiased pixel.
    fn spans(got: (u32, u32), lo: f32, hi: f32, k: f32, what: &str) {
        let (a, b) = (got.0 as f32, got.1 as f32 + 1.0);
        assert!((a - lo * k).abs() <= 1.5 && (b - hi * k).abs() <= 1.5, "{what} at {k}: surface {a}..{b}, expected {}..{}", lo * k, hi * k);
    }

    fn find<'a>(it: &'a Item, kind: &str) -> Option<&'a Item> {
        if it.kind == kind {
            return Some(it);
        }
        it.children.iter().find_map(|c| find(c, kind))
    }

    fn without(it: &Item, kind: &str) -> Item {
        let mut c = it.clone();
        fn strip(it: &mut Item, kind: &str) {
            if it.kind == kind {
                it.leaf = Leaf::None;
            }
            it.children.iter_mut().for_each(|c| strip(c, kind));
        }
        strip(&mut c, kind);
        c
    }

    fn item(kind: &'static str, x: f32, y: f32, w: f32, h: f32, leaf: Leaf) -> Item {
        Item {
            kind,
            rect: Rect { x, y, w, h },
            leaf,
            background: None,
            radius: 0.0,
            border: None,
            border_width: 0.0,
            opacity: 1.0,
            on_click: None,
            on_scroll_up: None,
            on_scroll_down: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn analog_clock_dial_hands_and_hub_keep_their_logical_geometry_at_every_scale() {
        let mut fonts = fonts().expect("render verification requires fonts");
        let def = dsl::parse("format=1\n[root]\ntype=\"analog-clock\"\nsize=160\nseconds=false\nnumerals=false\ncolor=\"#ff0000\"\ntrack=\"#00ff00\"\n", Trust::Theme).unwrap();
        let mut data = data::sample();
        data.time.hour = 3;
        data.time.min = 0;
        data.time.sec = 0;
        let it = layout(&def, &Ctx { data: &data, metrics: &Fixed, theme_radius: 18.0 }, None);
        let red = |p: [u8; 4]| p[3] > 200 && p[0] > 200 && p[1] < 80;
        for k in SCALES {
            let pm = paint_at(k, 160.0, 160.0, &it, &mut fonts, &mut ImageCache::default());
            assert_eq!((pm.width(), pm.height()), ((160.0 * k).ceil() as u32, (160.0 * k).ceil() as u32));
            let at = |x: f32, y: f32| px(&pm, (x * k) as u32, (y * k) as u32);
            assert!(red(at(105.0, 80.0)), "hour hand points right at {k}");
            assert!(red(at(80.0, 40.0)), "minute hand points up at {k}");
            assert_eq!(at(60.0, 80.0)[3], 0, "no opposite hour hand at {k}");
            assert_eq!(at(80.0, 115.0)[3], 0, "no opposite minute hand at {k}");
            assert_eq!(at(2.0, 2.0)[3], 0, "corners stay clear at {k}");
            // the dial is a 0.96 px circle of radius 76.8 around the card centre
            let edge = 80.0 - 76.8 - 0.48;
            let (x0, y0, x1, y1) = ink_box(&pm, |_, _, p| p[3] > 0 && p[1] > p[0]).expect("dial");
            spans((x0, x1), edge, 160.0 - edge, k, "dial x");
            spans((y0, y1), edge, 160.0 - edge, k, "dial y");
            // the hour hand: 4.48 px thick, round-capped, from the hub to 36.86 px right of the centre
            let across = (0..pm.height()).filter(|y| red(px(&pm, (100.0 * k) as u32, *y))).count() as f32;
            assert!((across - 4.48 * k).abs() <= 2.0, "hour hand {across} px thick at {k}, expected {}", 4.48 * k);
            let (_, _, tip, _) = ink_box(&pm, |x, y, p| red(p) && y == (80.0 * k) as u32 && x as f32 > 90.0 * k).expect("hour hand");
            assert!((tip as f32 + 1.0 - (80.0 + 36.864 + 2.24) * k).abs() <= 1.5, "hour hand ends at {tip} at {k}");
        }
    }

    #[test]
    fn rings_bars_and_graphs_keep_their_layout_boxes_at_every_scale() {
        let mut fonts = fonts().expect("render verification requires fonts");
        let th = theme();
        let def = dsl::parse(SHAPES, Trust::Theme).unwrap();
        let d = sample();
        let it = layout(&def, &Ctx { data: &d, metrics: &Fixed, theme_radius: th.radius }, None);
        let (ring, graph) = (find(&it, "ring").unwrap(), find(&it, "graph").unwrap());
        let Leaf::Ring { size, .. } = ring.leaf else { panic!("ring leaf") };
        let (rd, rr) = (size.min(ring.rect.w).min(ring.rect.h), ring.rect);
        let (cx, cy) = (rr.x + rr.w / 2.0, rr.y + rr.h / 2.0);
        let g = graph.rect;
        for k in SCALES {
            let full = paint_at(k, 200.0, 80.0, &it, &mut fonts, &mut ImageCache::default());
            if k == 1.0 {
                assert_eq!(full.data(), render_shapes().data(), "1x through the scaled pipeline is the golden 1x picture");
            }
            let at = |x: f32, y: f32| px(&full, (x * k) as u32, (y * k) as u32);
            assert_eq!(at(0.0, 0.0)[3], 0, "rounded card corner at {k}");
            assert!(near(at(38.0, 16.0), th.accent), "ring starts at 12 o'clock at {k}: {:?}", at(38.0, 16.0));
            assert!(!near(at(14.0, 40.0), th.accent), "37 % ring leaves 9 o'clock unlit at {k}");
            assert!(near(at(82.0, 24.0), th.ok), "bar fill at {k}");
            assert!(!near(at(180.0, 24.0), th.ok), "bar track at {k}");
            // the ring's ink is its layout circle: diameter `rd` around the centre of its box
            let (x0, y0, x1, y1) = diff_box(&full, &paint_at(k, 200.0, 80.0, &without(&it, "ring"), &mut fonts, &mut ImageCache::default())).expect("ring ink");
            spans((x0, x1), cx - rd / 2.0, cx + rd / 2.0, k, "ring x");
            spans((y0, y1), cy - rd / 2.0, cy + rd / 2.0, k, "ring y");
            // 60 samples in 60 slots span the graph's box edge to edge (+0.8 px round caps), inset 1 px vertically
            let (x0, y0, x1, y1) = diff_box(&full, &paint_at(k, 200.0, 80.0, &without(&it, "graph"), &mut fonts, &mut ImageCache::default())).expect("graph ink");
            spans((x0, x1), g.x - 0.8, g.x + g.w + 0.8, k, "graph x");
            assert!(y0 as f32 >= (g.y + 0.2) * k - 1.5, "graph starts inside its box at {k}: {y0}");
            assert!(y1 as f32 + 1.0 <= (g.y + g.h - 1.0) * k + 1.5 && y1 > y0, "graph ends inside its box at {k}: {y1}");
        }
    }

    fn quadrants() -> Pixmap {
        let mut pm = Pixmap::new(8, 8).unwrap();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let c = match (x < 4, y < 4) {
                    (true, true) => [255, 0, 0],
                    (false, true) => [0, 255, 0],
                    (true, false) => [0, 0, 255],
                    (false, false) => [255, 255, 255],
                };
                let i = ((y * 8 + x) * 4) as usize;
                pm.data_mut()[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        pm
    }

    #[test]
    fn images_keep_their_box_rounded_clip_and_orientation_at_every_scale() {
        let mut fonts = fonts().expect("render verification requires fonts");
        let mut images = ImageCache::default();
        images.map.insert("q.png".into(), Some(quadrants()));
        let mut root = item("stack", 0.0, 0.0, 100.0, 60.0, Leaf::None);
        root.children.push(item("image", 10.0, 10.0, 40.0, 40.0, Leaf::Image { path: Some("q.png".into()), radius: 12.0, cover: true }));
        // contain: an 8 x 8 picture in a 40 x 50 box is 40 x 40 at (55, 10)
        root.children.push(item("image", 55.0, 5.0, 40.0, 50.0, Leaf::Image { path: Some("q.png".into()), radius: 0.0, cover: false }));
        let is = |p: [u8; 4], c: [u8; 3]| p[3] > 240 && (0..3).all(|i| p[i].abs_diff(c[i]) <= 8);
        for k in SCALES {
            let pm = paint_at(k, 100.0, 60.0, &root, &mut fonts, &mut images);
            let at = |x: f32, y: f32| px(&pm, (x * k) as u32, (y * k) as u32);
            for (x, y, c, what) in [
                (20.0, 20.0, [255, 0, 0], "cover top-left"),
                (40.0, 20.0, [0, 255, 0], "cover top-right"),
                (20.0, 40.0, [0, 0, 255], "cover bottom-left"),
                (40.0, 40.0, [255, 255, 255], "cover bottom-right"),
                (65.0, 20.0, [255, 0, 0], "contain top-left"),
                (85.0, 40.0, [255, 255, 255], "contain bottom-right"),
            ] {
                assert!(is(at(x, y), c), "{what} at {k}: {:?}", at(x, y));
            }
            assert_eq!(at(10.5, 10.5)[3], 0, "rounded corner is clipped away at {k}");
            assert_eq!(at(75.0, 7.0)[3], 0, "contain leaves the top of its box empty at {k}");
            let (x0, y0, x1, y1) = ink_box(&pm, |x, _, p| p[3] > 0 && (x as f32) < 52.5 * k).expect("cover");
            spans((x0, x1), 10.0, 50.0, k, "cover x");
            spans((y0, y1), 10.0, 50.0, k, "cover y");
            let (x0, y0, x1, y1) = ink_box(&pm, |x, _, p| p[3] > 0 && (x as f32) >= 52.5 * k).expect("contain");
            spans((x0, x1), 55.0, 95.0, k, "contain x");
            spans((y0, y1), 10.0, 50.0, k, "contain y");
        }
        assert_eq!(images.map.len(), 1, "one decoded source per path, whatever the scale");
        let cached = images.get("q.png").unwrap();
        assert_eq!((cached.width(), cached.height()), (8, 8), "the cache keeps the source picture, not a scaled copy");
    }

    #[test]
    fn icons_and_borders_stay_in_their_boxes_at_every_scale() {
        let mut fonts = fonts().expect("render verification requires fonts");
        let white = ColorRef::Fixed(Rgb(255, 255, 255), 1.0);
        let mut root = item("stack", 0.0, 0.0, 64.0, 32.0, Leaf::None);
        root.children.push(item("icon", 0.0, 0.0, 32.0, 32.0, Leaf::Icon { name: "wifi-3".into(), size: 16.0, color: white.clone() }));
        let mut framed = item("row", 36.0, 4.0, 24.0, 24.0, Leaf::None);
        framed.border = Some(white);
        framed.border_width = 2.0;
        framed.radius = 6.0;
        root.children.push(framed);
        for k in SCALES {
            let pm = paint_at(k, 64.0, 32.0, &root, &mut fonts, &mut ImageCache::default());
            let (x0, y0, x1, y1) = ink_box(&pm, |x, _, p| p[3] > 0 && (x as f32) < 34.0 * k).expect("icon");
            assert!(x0 as f32 >= 8.0 * k - 1.5 && y0 as f32 >= 8.0 * k - 1.5 && x1 as f32 + 1.0 <= 24.0 * k + 1.5 && y1 as f32 + 1.0 <= 24.0 * k + 1.5, "icon inside its 16 px box at {k}");
            assert!((x1 - x0 + 1) as f32 >= 8.0 * k, "icon is drawn at its size at {k}");
            let (x0, y0, x1, y1) = ink_box(&pm, |x, _, p| p[3] > 0 && (x as f32) >= 34.0 * k).expect("border");
            spans((x0, x1), 36.0, 60.0, k, "border x");
            spans((y0, y1), 4.0, 28.0, k, "border y");
            assert_eq!(px(&pm, (48.0 * k) as u32, (16.0 * k) as u32)[3], 0, "the frame is a stroke, not a fill, at {k}");
        }
    }

    #[test]
    fn ascii_full_blocks_stay_seamless_at_fractional_scales() {
        let mut fonts = fonts().expect("render verification requires fonts");
        let th = theme();
        let def = dsl::parse("format=1\nsize=[40,30]\n[root]\ntype=\"ascii\"\ntext=\"{fetch.logo}\"\nsize=12\n", Trust::Theme).unwrap();
        let mut data = sample();
        data.set("fetch.logo", data::Val::Text("██\n██".into()));
        let it = layout(&def, &Ctx { data: &data, metrics: &FontMetrics(&fonts), theme_radius: th.radius }, None);
        for k in SCALES {
            let pm = paint_at(k, 40.0, 30.0, &it, &mut fonts, &mut ImageCache::default());
            // rows meet at logical y 15, which is 18.75 surface px at 125 %: snapped, there is no half-covered row there
            for y in 0..(30.0 * k) as u32 {
                for x in (15.0 * k).ceil() as u32..(25.0 * k) as u32 {
                    assert_eq!(px(&pm, x, y)[3], 255, "seam at {x},{y} at {k}");
                }
            }
            assert_eq!(px(&pm, 0, (15.0 * k) as u32)[3], 0, "logo stays centred at {k}");
        }
    }
}
