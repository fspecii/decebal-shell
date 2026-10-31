//! Icons: freedesktop theme lookup with a user-selectable theme, rasterisation and the per-window pixmap cache.
//!
//! * [`index`], [`resolve`]: `index.theme` parsing and the Icon Theme Specification lookup (inherits, hicolor, sizes, scales).
//! * [`names`]: aliases and fallbacks (dock ids, reverse-DNS classes, MIME icons, the generic application icon).
//! * [`config`], [`live`]: the `icons` object of `decebal.json` (theme, size scale, tint, fallback), the automatic
//!   choice from GSettings / GTK and hot reload ([`generation`] changes when icons must be redrawn).
//! * [`raster`], [`xpm`]: SVG / PNG / XPM to a crisp square pixmap. [`cache`]: the bounded LRU both caches use.
//! * [`IconCache`]: what the dock, launchers, bar and switchers hold; it drops its pixmaps when the generation moves.
//!
//! See `docs/icons.md`.

pub mod cache;
pub mod config;
pub mod ctl;
pub mod index;
pub mod live;
pub mod names;
pub mod preview;
pub mod raster;
pub mod resolve;
pub mod xpm;

pub use live::{generation, status};

use resolve::{Found, Kind};
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

/// Pixel size the path-only helpers ([`find_svg`], [`find_png`]) look icons up for.
const LOOKUP_PX: u32 = 128;

/// The SVG for `id` in the current theme chain, if that is what the theme has for it.
pub fn find_svg(id: &str) -> Option<PathBuf> {
    live::lookup(id, LOOKUP_PX, false).filter(|f| f.kind == Kind::Svg).map(|f| f.path)
}

/// The bitmap (PNG or XPM, also from `pixmaps`) for `id` when the theme has no SVG for it.
pub fn find_png(id: &str) -> Option<PathBuf> {
    live::lookup(id, LOOKUP_PX, false).filter(|f| f.kind != Kind::Svg).map(|f| f.path)
}

/// `id` rendered at `size` px (no generic fallback: `None` when the theme chain does not know the name).
pub fn load_named(id: &str, size: u32) -> Option<Pixmap> {
    let (scale, tint) = live::render_settings();
    let content = content_px(size, scale);
    live::lookup(id, content, false).and_then(|f| raster::render(&f, size, raster::Opts { content, tint: tint_colour(tint) }))
}

fn content_px(size: u32, scale: f32) -> u32 {
    ((size as f32 * scale).round() as u32).max(1)
}

fn tint_colour(on: bool) -> Option<(u8, u8, u8)> {
    on.then(|| crate::theme::th().text.t())
}

/// Loads a PNG and fits it into a square pixmap of `size` px, keeping its aspect ratio.
pub fn load_png_scaled(path: &Path, size: u32) -> Option<Pixmap> {
    raster::fit(&Pixmap::load_png(path).ok()?, size, size)
}

/// Rasterises an SVG file to a square pixmap of `size` px. Returns `None` on any error.
pub fn render_svg(path: &Path, size: u32) -> Option<Pixmap> {
    raster::svg(path, size, size)
}

fn render_file(path: &Path, size: u32, opts: raster::Opts) -> Option<Pixmap> {
    let kind = match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("svg") => Kind::Svg,
        Some("xpm") => Kind::Xpm,
        _ => Kind::Png,
    };
    raster::render(&Found { path: path.to_path_buf(), kind }, size, opts)
}

/// The Launchpad tile: light rounded square with a 3x3 grid of coloured dots.
pub fn launchpad(size: u32) -> Option<Pixmap> {
    use crate::gfx::{color, fill_circle, fill_rounded, pixmap};
    let mut pm = pixmap(size, size)?;
    let s = size as f32;
    fill_rounded(&mut pm, 0.0, 0.0, s, s, s * 0.225, color(222, 222, 226, 1.0));
    let cols = [(255, 95, 87), (255, 189, 46), (40, 200, 64), (10, 132, 255), (175, 82, 222), (255, 159, 10), (90, 200, 250), (255, 55, 95), (48, 209, 88)];
    let (pad, step) = (s * 0.22, s * 0.28);
    for (i, (r, g, b)) in cols.iter().enumerate() {
        let (cx, cy) = (pad + (i % 3) as f32 * step + s * 0.04, pad + (i / 3) as f32 * step + s * 0.04);
        fill_circle(&mut pm, cx, cy, s * 0.07, color(*r, *g, *b, 1.0));
    }
    Some(pm)
}

/// Built-in Trash can, used when the icon theme has none: a translucent grey bin with a lid.
pub fn trash(size: u32) -> Option<Pixmap> {
    use crate::gfx::{color, fill_rounded, pixmap};
    use tiny_skia::{FillRule, Paint, PathBuilder, Transform};
    let mut pm = pixmap(size, size)?;
    let s = size as f32;
    let mut body = PathBuilder::new();
    body.move_to(s * 0.22, s * 0.28);
    body.line_to(s * 0.78, s * 0.28);
    body.line_to(s * 0.72, s * 0.92);
    body.line_to(s * 0.28, s * 0.92);
    body.close();
    if let Some(path) = body.finish() {
        let mut paint = Paint::default();
        paint.set_color(color(120, 120, 126, 0.9));
        paint.anti_alias = true;
        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
    fill_rounded(&mut pm, s * 0.16, s * 0.16, s * 0.68, s * 0.1, s * 0.04, color(150, 150, 156, 1.0));
    fill_rounded(&mut pm, s * 0.4, s * 0.08, s * 0.2, s * 0.1, s * 0.03, color(150, 150, 156, 1.0));
    Some(pm)
}

/// Bounds of one [`IconCache`]: a launcher showing hundreds of apps or a magnifying dock must not grow without limit.
pub const CACHE_ENTRIES: usize = 320;
pub const CACHE_BYTES: usize = 10 << 20;

/// Caches rasterised icons by (id, size), least recently used out first. A missing icon yields the theme's
/// generic application icon, then a neutral placeholder. The cache empties itself when the icon settings or the
/// shell theme change (the tint follows the theme's text colour).
type IconKey = (String, u32, Option<(u8, u8, u8)>);

pub struct IconCache {
    lru: cache::Lru<IconKey, Pixmap>,
    gen: (u64, u64),
}

impl Default for IconCache {
    fn default() -> Self {
        IconCache { lru: cache::Lru::new(CACHE_ENTRIES, CACHE_BYTES), gen: (0, 0) }
    }
}

impl IconCache {
    pub fn get(&mut self, id: &str, size: u32) -> &Pixmap {
        self.get_with_tint(id, size, tint_colour(live::render_settings().1))
    }

    /// Surface-specific ink for symbolic icons; coloured artwork and the user's tint switch are preserved.
    pub fn get_for_surface(&mut self, id: &str, size: u32, ink: (u8, u8, u8)) -> &Pixmap {
        self.get_with_tint(id, size, live::render_settings().1.then_some(ink))
    }

    fn get_with_tint(&mut self, id: &str, size: u32, tint: Option<(u8, u8, u8)>) -> &Pixmap {
        let gen = (live::generation(), crate::theme::generation());
        if gen != self.gen {
            self.lru.clear();
            self.gen = gen;
        }
        let key = (id.to_string(), size, tint);
        if !self.lru.contains(&key) {
            let pm = build(id, size, tint);
            let bytes = pm.data().len();
            self.lru.put(key.clone(), pm, bytes);
        }
        self.lru.get(&key).expect("stored above")
    }

    /// Pixmaps held and their bytes (tests, `decthemectl icons status`).
    pub fn usage(&self) -> (usize, usize) {
        (self.lru.len(), self.lru.weight())
    }
}

/// Rasterises one icon at exactly `size` pixels. The pixmap is its own surface, so a panel's render scale (see
/// [`crate::gfx::with_render_scale`]) must not enlarge the built-in artwork that is drawn with the gfx helpers a second time.
fn build(id: &str, size: u32, tint: Option<(u8, u8, u8)>) -> Pixmap {
    crate::gfx::with_render_scale(1.0, || build_at_native_scale(id, size, tint))
}

fn build_at_native_scale(id: &str, size: u32, tint: Option<(u8, u8, u8)>) -> Pixmap {
    let (scale, _) = live::render_settings();
    let content = content_px(size, scale);
    let opts = raster::Opts { content, tint };
    let pm = if id == "launchpad" {
        launchpad(size)
    } else if id.starts_with('/') {
        // An absolute path, as `Icon=` in a .desktop file may give.
        render_file(Path::new(id), size, opts)
    } else {
        let themed = |generic: bool| live::lookup(id, content, generic).and_then(|f| raster::render(&f, size, opts));
        themed(false).or_else(|| matches!(id, "trash" | "trash-full").then(|| trash(size)).flatten()).or_else(|| themed(true))
    };
    pm.or_else(|| placeholder(size)).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"))
}

fn placeholder(size: u32) -> Option<Pixmap> {
    use crate::gfx::{color, fill_rounded, pixmap};
    let mut pm = pixmap(size, size)?;
    let s = size as f32;
    fill_rounded(&mut pm, 0.0, 0.0, s, s, s * 0.225, color(90, 90, 96, 1.0));
    Some(pm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_symbolic_icon_can_cache_distinct_surface_foregrounds() {
        let dir = std::env::temp_dir().join(format!("decebal-symbolic-surface-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("foreground-symbolic.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="black"/></svg>"#).unwrap();
        let mut cache = IconCache::default();
        let id = path.to_str().unwrap();
        for tint in [(255, 255, 255), (0, 0, 0), (255, 255, 255)] {
            let pm = cache.get_with_tint(id, 16, Some(tint));
            let pixel = pm.pixel(8, 8).unwrap();
            assert_eq!((pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()), (tint.0, tint.1, tint.2, 255));
        }
        assert_eq!(cache.usage().0, 2, "surface tint is part of the bounded cache key");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn built_in_icons_are_the_same_picture_at_every_render_scale() {
        // the launcher and the trash are drawn with the gfx helpers: inside a scaled panel they must still fill exactly their own pixmap once
        for id in ["launchpad", "trash", "definitely-not-an-icon-xyz"] {
            let plain = IconCache::default().get(id, 96).clone();
            for k in [1.25, 2.0] {
                let scaled = crate::gfx::with_render_scale(k, || IconCache::default().get(id, 96).clone());
                assert_eq!(plain.data(), scaled.data(), "{id} at render scale {k}");
            }
        }
    }

    #[test]
    fn launchpad_has_pixels() {
        let pm = launchpad(64).unwrap();
        assert_eq!(pm.width(), 64);
        assert!(pm.pixel(32, 32).unwrap().alpha() > 0);
    }

    #[test]
    fn missing_icon_gets_placeholder() {
        let mut c = IconCache::default();
        let pm = c.get("definitely-not-an-icon-xyz", 48);
        assert_eq!(pm.width(), 48);
    }

    #[test]
    fn trash_fallback_draws_something() {
        let pm = trash(64).unwrap();
        assert!(pm.pixel(32, 50).unwrap().alpha() > 0);
        assert_eq!(pm.pixel(2, 2).unwrap().alpha(), 0);
    }

    #[test]
    fn png_icons_are_loaded_and_fitted() {
        let src = launchpad(64).unwrap();
        let p = std::env::temp_dir().join("decebal-test-icon.png");
        src.save_png(&p).unwrap();
        let pm = load_png_scaled(&p, 32).unwrap();
        assert_eq!((pm.width(), pm.height()), (32, 32));
        assert!(pm.pixel(16, 16).unwrap().alpha() > 0);
        assert!(load_png_scaled(Path::new("/definitely/not/here.png"), 32).is_none());
    }

    #[test]
    fn bad_svg_is_none() {
        let p = std::env::temp_dir().join("decebal-bad.svg");
        std::fs::write(&p, b"not svg").unwrap();
        assert!(render_svg(&p, 32).is_none());
    }

    #[test]
    fn the_pixmap_cache_stays_within_its_budget() {
        let mut c = IconCache::default();
        for i in 0..260 {
            assert_eq!(c.get(&format!("no-such-icon-{i}"), 128).width(), 128);
        }
        let (n, bytes) = c.usage();
        assert!(n <= CACHE_ENTRIES && bytes <= CACHE_BYTES, "{n} pixmaps, {bytes} bytes");
        assert!(n >= 60, "but it does keep a useful number: {n}");
    }

    #[test]
    fn absolute_paths_render_without_the_theme() {
        let p = std::env::temp_dir().join(format!("decebal-abs-{}.png", std::process::id()));
        launchpad(64).unwrap().save_png(&p).unwrap();
        let mut c = IconCache::default();
        let pm = c.get(p.to_str().unwrap(), 40);
        assert_eq!((pm.width(), pm.height()), (40, 40));
        assert!(pm.pixel(20, 20).unwrap().alpha() > 0);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn unknown_ids_never_panic_and_the_builtin_trash_is_used_without_a_theme_icon() {
        let mut c = IconCache::default();
        assert_eq!(c.get("", 32).width(), 32);
        assert_eq!(c.get("../../etc/passwd", 32).width(), 32);
        assert_eq!(c.get("launchpad", 32).width(), 32);
    }
}
