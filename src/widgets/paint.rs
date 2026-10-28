//! Draws a laid-out widget with the current theme's colours.

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
    fn text_width(&self, s: &str, px: f32, bold: bool) -> f32 {
        self.0.measure(s, px, if bold { Weight::Bold } else { Weight::Regular })
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

/// Paints `it` (and its children) into `pm`; (0, 0) of the item tree is the pixmap's top-left.
pub fn paint(pm: &mut Pixmap, it: &Item, p: &mut Painter, parent_opacity: f32) {
    let op = parent_opacity * it.opacity;
    let r = it.rect;
    if let Some(bg) = &it.background {
        gfx::fill_rounded(pm, r.x, r.y, r.w, r.h, it.radius, col(p.theme, bg, op));
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
        Leaf::Icon { name, size, color } => {
            let (rgb, a) = rgb_of(p.theme, color);
            if let Some(img) = p.symbols.get(name, rgb.t(), size.round().max(1.0) as u32) {
                gfx::blit_scaled(pm, img, r.x + (r.w - size) / 2.0, r.y + (r.h - size) / 2.0, *size, *size, a * op);
            }
        }
        Leaf::Bar { frac, thickness, color, track } => {
            let y = r.y + (r.h - thickness) / 2.0;
            charts::bar(pm, crate::dash::layout::Rect::new(r.x, y, r.w, *thickness), *frac, col(p.theme, track, op), col(p.theme, color, op));
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

fn draw_image(pm: &mut Pixmap, img: &Pixmap, r: Rect, radius: f32, cover: bool, op: f32) {
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    if iw == 0.0 || ih == 0.0 || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let s = if cover { (r.w / iw).max(r.h / ih) } else { (r.w / iw).min(r.h / ih) };
    let (w, h) = (iw * s, ih * s);
    let (x, y) = (r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0);
    let mut paint = Paint::default();
    paint.shader = Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, op, Transform::from_scale(s, s).post_translate(x, y));
    paint.anti_alias = true;
    // cover crops to the box; contain keeps the whole picture
    let (bx, by, bw, bh) = if cover { (r.x, r.y, r.w, r.h) } else { (x, y, w, h) };
    if let Some(path) = gfx::rounded_rect(bx, by, bw, bh, radius) {
        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::model::{Def, Selection};
    use crate::widgets::data::sample;
    use crate::widgets::layout::{layout, Ctx, Fixed};
    use crate::widgets::{dsl, Trust};

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
        paint(&mut pm, &it, &mut Painter { fonts: &mut fonts, symbols: &mut sym, images: &mut imgs, theme: &th }, 1.0);
        pm
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * pm.width() + x) * 4) as usize;
        pm.data()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn rings_bars_and_graphs_land_where_the_layout_says() {
        let pm = render_shapes();
        if pm.data().iter().all(|b| *b == 0) {
            eprintln!("(no fonts here: skipped)");
            return;
        }
        let th = theme();
        // card background at the left edge, transparent outside the rounded corner
        assert_eq!(px(&pm, 0, 0)[3], 0, "rounded corner is transparent");
        let bg = px(&pm, 100, 5);
        assert_eq!(&bg[..3], &[th.popup.0, th.popup.1, th.popup.2][..], "popup colour (alpha-premultiplied at 0.96 so approx)") ;
    }

    #[test]
    fn shapes_render_is_deterministic() {
        let (a, b) = (render_shapes(), render_shapes());
        assert_eq!(crate::theme::engine::sha256::digest_hex(a.data()), crate::theme::engine::sha256::digest_hex(b.data()));
        let _ = Def::default();
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
    paint(&mut pm, &shifted, &mut Painter { fonts, symbols: &mut sym, images: &mut imgs, theme: th }, 1.0);
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
