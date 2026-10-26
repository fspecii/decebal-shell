//! Freedesktop icon lookup (WhiteSur first) and SVG rasterisation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::{Pixmap, Transform};

const THEMES: &[&str] = &["WhiteSur-dark", "WhiteSur", "hicolor", "Adwaita"];
const SUBDIRS: &[&str] = &["apps/scalable", "places/scalable", "apps/512", "apps/256", "apps/128", "apps/96", "apps/64", "apps/48", "places/48", "places/64", "devices/scalable"];

fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(h) = std::env::var_os("HOME") {
        roots.push(Path::new(&h).join(".local/share/icons"));
        roots.push(Path::new(&h).join(".icons"));
    }
    roots.push(PathBuf::from("/usr/share/icons"));
    roots
}

/// Maps dock ids to the icon names themes actually use.
fn alias(id: &str) -> &str {
    match id {
        "trash" => "user-trash",
        other => other,
    }
}

/// Finds an SVG for `id`, trying the id as given then lower-cased.
pub fn find_svg(id: &str) -> Option<PathBuf> {
    let name = alias(id);
    let candidates = [name.to_string(), name.to_lowercase()];
    for root in icon_roots() {
        for theme in THEMES {
            for sub in SUBDIRS {
                for cand in &candidates {
                    let p = root.join(theme).join(sub).join(format!("{cand}.svg"));
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

/// Finds a PNG icon (hicolor/Adwaita bitmap sizes, then /usr/share/pixmaps) when no SVG exists.
pub fn find_png(id: &str) -> Option<PathBuf> {
    let name = alias(id);
    let candidates = [name.to_string(), name.to_lowercase()];
    const SIZES: &[&str] = &["256x256", "128x128", "96x96", "64x64", "48x48"];
    for root in icon_roots() {
        for theme in THEMES {
            for size in SIZES {
                for kind in ["apps", "places"] {
                    for cand in &candidates {
                        let p = root.join(theme).join(size).join(kind).join(format!("{cand}.png"));
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }
    candidates.iter().map(|c| PathBuf::from("/usr/share/pixmaps").join(format!("{c}.png"))).find(|p| p.is_file())
}

/// Loads a PNG and resamples it into a square pixmap of `size` px, keeping its aspect ratio.
pub fn load_png_scaled(path: &Path, size: u32) -> Option<Pixmap> {
    let src = Pixmap::load_png(path).ok()?;
    let mut out = Pixmap::new(size, size)?;
    let scale = (size as f32 / src.width() as f32).min(size as f32 / src.height() as f32);
    let (w, h) = (src.width() as f32 * scale, src.height() as f32 * scale);
    crate::gfx::blit_scaled(&mut out, &src, (size as f32 - w) / 2.0, (size as f32 - h) / 2.0, w, h, 1.0);
    Some(out)
}

/// Rasterises an SVG file to a square pixmap of `size` px. Returns `None` on any error.
pub fn render_svg(path: &Path, size: u32) -> Option<Pixmap> {
    let data = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
    let mut pm = Pixmap::new(size, size)?;
    let s = tree.size();
    let scale = (size as f32 / s.width()).min(size as f32 / s.height());
    let dx = (size as f32 - s.width() * scale) / 2.0;
    let dy = (size as f32 - s.height() * scale) / 2.0;
    resvg::render(&tree, Transform::from_scale(scale, scale).post_translate(dx, dy), &mut pm.as_mut());
    Some(pm)
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

/// Caches rasterised icons by (id, size). A missing icon yields a neutral placeholder.
#[derive(Default)]
pub struct IconCache {
    map: HashMap<(String, u32), Pixmap>,
}

impl IconCache {
    pub fn get(&mut self, id: &str, size: u32) -> &Pixmap {
        self.map.entry((id.to_string(), size)).or_insert_with(|| {
            let pm = if id == "launchpad" {
                launchpad(size)
            } else if id.starts_with('/') {
                // An absolute path, as `Icon=` in a .desktop file may give.
                let path = Path::new(id);
                if path.extension().is_some_and(|e| e == "svg") {
                    render_svg(path, size)
                } else {
                    load_png_scaled(path, size)
                }
            } else {
                find_svg(id).and_then(|p| render_svg(&p, size)).or_else(|| find_png(id).and_then(|p| load_png_scaled(&p, size)))
            };
            pm.or_else(|| placeholder(size)).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"))
        })
    }
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
        let p = std::env::temp_dir().join("macshell-test-icon.png");
        src.save_png(&p).unwrap();
        let pm = load_png_scaled(&p, 32).unwrap();
        assert_eq!((pm.width(), pm.height()), (32, 32));
        assert!(pm.pixel(16, 16).unwrap().alpha() > 0);
        assert!(load_png_scaled(Path::new("/definitely/not/here.png"), 32).is_none());
    }

    #[test]
    fn bad_svg_is_none() {
        let p = std::env::temp_dir().join("macshell-bad.svg");
        std::fs::write(&p, b"not svg").unwrap();
        assert!(render_svg(&p, 32).is_none());
    }

    #[test]
    fn trash_alias() {
        assert_eq!(alias("trash"), "user-trash");
        assert_eq!(alias("foot"), "foot");
    }
}
