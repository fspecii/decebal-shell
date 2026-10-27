//! Text rendering with fontdue: glyph cache, measuring and drawing into a tiny-skia pixmap.

use fontdue::{Font, FontSettings};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tiny_skia::{Color, Pixmap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

/// Rasterised glyph: metrics plus an 8-bit coverage bitmap.
struct Glyph {
    metrics: fontdue::Metrics,
    bitmap: Vec<u8>,
}

type FaceSpec = (PathBuf, u32);

/// Cap on cached glyphs (distinct char/size/weight combinations) so odd window titles cannot grow memory.
const MAX_GLYPHS: usize = 4096;

/// UI text: Inter when installed (the original look), otherwise a common sans font.
///
/// fontdue parses every glyph of a face up front (about 12 MB for one Inter face), so faces are
/// loaded lazily the first time a glyph is missing from the cache and dropped again by [`Fonts::trim`].
/// Steady-state drawing then only touches the small glyph cache.
pub struct Fonts {
    specs: RefCell<[FaceSpec; 4]>,
    /// Theme generation the specs/glyphs were built for (a theme change can switch family and scale).
    theme_gen: Cell<u64>,
    /// Font family the specs were picked for (colours and text scale changes keep the glyph cache).
    family: RefCell<String>,
    loaded: RefCell<[Option<Font>; 4]>,
    glyphs: RefCell<HashMap<(char, u32, Weight), Rc<Glyph>>>,
    ascents: RefCell<HashMap<(u32, Weight), f32>>,
}

/// Resolves a fontconfig pattern to (file, face index inside a .ttc collection).
fn find_font(patterns: &[&str]) -> Option<FaceSpec> {
    for pat in patterns {
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}\n%{index}", pat]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let mut lines = text.lines();
        let path = lines.next().unwrap_or("").trim().to_string();
        let index = lines.next().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
        if !path.is_empty() && Path::new(&path).is_file() {
            return Some((PathBuf::from(path), index));
        }
    }
    None
}

/// Like [`find_font`] but only accepts a font whose family really is `family` (fc-match always answers with something).
fn find_family(family: &str, style: &str) -> Option<FaceSpec> {
    let pat = format!("{family}{style}");
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}\n%{index}\n%{family}", &pat]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut lines = text.lines();
    let path = lines.next()?.trim().to_string();
    let index = lines.next().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
    let fam = lines.next().unwrap_or("").to_lowercase();
    (fam.split(',').any(|f| f.trim() == family.to_lowercase()) && Path::new(&path).is_file()).then(|| (PathBuf::from(path), index))
}

/// The four faces for the theme's font family, falling back to Inter and then any sans font.
fn pick_specs() -> Option<[FaceSpec; 4]> {
    let family = crate::theme::current().font_family.clone();
    let try_family = |style: &str, fallback: &[&str]| -> Option<FaceSpec> {
        if !family.is_empty() {
            if let Some(f) = find_family(&family, style) {
                return Some(f);
            }
        }
        find_font(fallback)
    };
    let regular = try_family("", &["Inter", "Inter Variable", "sans-serif"])?;
    load(&regular)?; // fail early (and free it again) if the file is unreadable
    let pick = |style: &str, fb: &[&str]| try_family(style, fb).unwrap_or_else(|| regular.clone());
    let medium = pick(":weight=medium", &["Inter:weight=medium", "sans-serif:weight=medium"]);
    let semibold = pick(":weight=demibold", &["Inter:weight=demibold", "sans-serif:weight=demibold"]);
    let bold = pick(":weight=bold", &["Inter:weight=bold", "sans-serif:weight=bold"]);
    Some([regular, medium, semibold, bold])
}

fn load(face: &FaceSpec) -> Option<Font> {
    let data = std::fs::read(&face.0).ok()?;
    Font::from_bytes(data, FontSettings { collection_index: face.1, ..FontSettings::default() }).ok()
}

/// Hands freed heap pages back to the OS (glibc keeps them otherwise, which would defeat `trim`).
fn release_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim only releases free memory; it has no preconditions.
    unsafe {
        libc::malloc_trim(0);
    }
}

fn slot(w: Weight) -> usize {
    match w {
        Weight::Regular => 0,
        Weight::Medium => 1,
        Weight::SemiBold => 2,
        Weight::Bold => 3,
    }
}

impl Fonts {
    /// `None` only if no usable font file can be found at all.
    pub fn load() -> Option<Self> {
        let specs = pick_specs()?;
        Some(Self {
            specs: RefCell::new(specs),
            theme_gen: Cell::new(crate::theme::generation()),
            family: RefCell::new(crate::theme::current().font_family.clone()),
            loaded: RefCell::new([None, None, None, None]),
            glyphs: RefCell::new(HashMap::new()),
            ascents: RefCell::new(HashMap::new()),
        })
    }

    /// Rebuilds faces and drops cached glyphs when the theme (family, text scale) changed since the last call.
    fn sync_theme(&self) {
        let g = crate::theme::generation();
        if self.theme_gen.get() == g {
            return;
        }
        self.theme_gen.set(g);
        let family = crate::theme::current().font_family.clone();
        if *self.family.borrow() == family {
            return; // only colours / sizes changed: glyph keys already include the scaled size
        }
        *self.family.borrow_mut() = family;
        if let Some(specs) = pick_specs() {
            *self.specs.borrow_mut() = specs;
        }
        *self.loaded.borrow_mut() = [None, None, None, None];
        self.glyphs.borrow_mut().clear();
        self.ascents.borrow_mut().clear();
        release_heap();
    }

    /// Runs `f` with the face for `weight`, loading it if needed. `None` if the file became unreadable.
    fn with_font<R>(&self, weight: Weight, f: impl FnOnce(&Font) -> R) -> Option<R> {
        let i = slot(weight);
        let mut loaded = self.loaded.borrow_mut();
        if loaded[i].is_none() {
            // One parsed face is ~12 MB: never keep two at once.
            *loaded = [None, None, None, None];
            loaded[i] = load(&self.specs.borrow()[i]);
        }
        loaded[i].as_ref().map(f)
    }

    /// Drops the parsed faces (tens of MB); cached glyphs stay. Call after a frame is drawn.
    pub fn trim(&self) {
        let mut loaded = self.loaded.borrow_mut();
        if loaded.iter().all(Option::is_none) {
            return;
        }
        *loaded = [None, None, None, None];
        release_heap();
    }

    /// True while a parsed face is held in memory.
    pub fn has_loaded_face(&self) -> bool {
        self.loaded.borrow().iter().any(Option::is_some)
    }

    fn glyph(&self, ch: char, px: f32, weight: Weight) -> Option<Rc<Glyph>> {
        self.sync_theme();
        let px = px * crate::theme::live::font_scale();
        let key = (ch, (px * 4.0).round() as u32, weight);
        if let Some(g) = self.glyphs.borrow().get(&key) {
            return Some(Rc::clone(g));
        }
        let (metrics, bitmap) = self.with_font(weight, |f| f.rasterize(ch, px))?;
        let g = Rc::new(Glyph { metrics, bitmap });
        let mut cache = self.glyphs.borrow_mut();
        if cache.len() >= MAX_GLYPHS {
            cache.clear();
        }
        cache.insert(key, Rc::clone(&g));
        Some(g)
    }

    /// Distance from the top of the line box to the baseline at pixel size `px`.
    pub fn ascent(&self, px: f32, weight: Weight) -> f32 {
        self.sync_theme();
        let px = px * crate::theme::live::font_scale();
        let key = ((px * 4.0).round() as u32, weight);
        if let Some(a) = self.ascents.borrow().get(&key) {
            return *a;
        }
        let a = self.with_font(weight, |f| f.horizontal_line_metrics(px).map(|m| m.ascent)).flatten().unwrap_or(px * 0.9);
        self.ascents.borrow_mut().insert(key, a);
        a
    }

    /// Advance width of `text` at pixel size `px`.
    pub fn measure(&self, text: &str, px: f32, weight: Weight) -> f32 {
        text.chars().filter_map(|c| self.glyph(c, px, weight)).map(|g| g.metrics.advance_width).sum()
    }

    /// Draws `text` with its baseline-box top-left at (`x`, `y`) and returns the width drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, pm: &mut Pixmap, text: &str, x: f32, y: f32, px: f32, weight: Weight, color: Color) -> f32 {
        let baseline = y + self.ascent(px, weight);
        let (cr, cg, cb, ca) = (color.red(), color.green(), color.blue(), color.alpha());
        let mut pen = x;
        for ch in text.chars() {
            let Some(g) = self.glyph(ch, px, weight) else { continue };
            let m = &g.metrics;
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            blend_glyph(pm, &g.bitmap, m.width, m.height, gx, gy, (cr, cg, cb, ca));
            pen += m.advance_width;
        }
        pen - x
    }
}

/// Source-over blend of an 8-bit coverage bitmap tinted with a straight (non-premultiplied) colour.
fn blend_glyph(pm: &mut Pixmap, bitmap: &[u8], gw: usize, gh: usize, gx: i32, gy: i32, c: (f32, f32, f32, f32)) {
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let stride = pm.width() as usize * 4;
    let data = pm.data_mut();
    for row in 0..gh as i32 {
        let py = gy + row;
        if py < 0 || py >= ph {
            continue;
        }
        for col in 0..gw as i32 {
            let px = gx + col;
            if px < 0 || px >= pw {
                continue;
            }
            let cov = bitmap[row as usize * gw + col as usize] as f32 / 255.0 * c.3;
            if cov <= 0.0 {
                continue;
            }
            let i = py as usize * stride + px as usize * 4;
            let inv = 1.0 - cov;
            data[i] = (c.0 * 255.0 * cov + data[i] as f32 * inv).round() as u8;
            data[i + 1] = (c.1 * 255.0 * cov + data[i + 1] as f32 * inv).round() as u8;
            data[i + 2] = (c.2 * 255.0 * cov + data[i + 2] as f32 * inv).round() as u8;
            data[i + 3] = (255.0 * cov + data[i + 3] as f32 * inv).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measuring_unseen_text_loads_a_face_that_trim_drops() {
        let Some(f) = Fonts::load() else { return };
        assert!(!f.has_loaded_face());
        f.measure("zq unseen \u{2603} text", 12.0, Weight::Regular);
        assert!(f.has_loaded_face());
        f.trim();
        assert!(!f.has_loaded_face());
    }

    #[test]
    fn measures_and_draws() {
        let Some(mut f) = Fonts::load() else { return }; // CI machines may have no fonts
        assert!(f.measure("Finder", 13.0, Weight::Regular) > f.measure("Fi", 13.0, Weight::Regular));
        let mut pm = Pixmap::new(80, 20).unwrap();
        let w = f.draw(&mut pm, "File", 2.0, 2.0, 13.0, Weight::Regular, Color::WHITE);
        assert!(w > 10.0);
        assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0));
    }

    #[test]
    fn bold_is_a_different_face_than_regular() {
        let Some(f) = Fonts::load() else { return };
        assert!(f.measure("Finder File Edit", 12.0, Weight::Bold) > f.measure("Finder File Edit", 12.0, Weight::Regular));
    }

    #[test]
    fn ascent_is_positive_and_scales() {
        let Some(f) = Fonts::load() else { return };
        assert!(f.ascent(24.0, Weight::Regular) > f.ascent(12.0, Weight::Regular) && f.ascent(12.0, Weight::Regular) > 0.0);
    }

    #[test]
    fn trimming_drops_faces_but_text_still_works() {
        let Some(mut f) = Fonts::load() else { return };
        let before = f.measure("Control Center", 12.0, Weight::Regular);
        f.trim();
        assert!(f.loaded.borrow().iter().all(|x| x.is_none()), "faces are released");
        assert_eq!(f.measure("Control Center", 12.0, Weight::Regular), before, "answered from the glyph cache");
        // A glyph never seen before reloads the face transparently.
        assert!(f.measure("Zq", 12.0, Weight::Regular) > 0.0);
        f.trim();
        let mut pm = Pixmap::new(60, 20).unwrap();
        assert!(f.draw(&mut pm, "Edit", 1.0, 1.0, 12.0, Weight::Bold, Color::WHITE) > 0.0);
    }

    #[test]
    fn glyph_cache_is_bounded() {
        let Some(f) = Fonts::load() else { return };
        for c in 0..(MAX_GLYPHS as u32 + 200) {
            if let Some(ch) = char::from_u32(0x4e00 + c) {
                f.measure(&ch.to_string(), 12.0, Weight::Regular);
            }
        }
        assert!(f.glyphs.borrow().len() <= MAX_GLYPHS);
    }

    #[test]
    fn empty_text_is_zero_width() {
        let Some(f) = Fonts::load() else { return };
        assert_eq!(f.measure("", 13.0, Weight::Bold), 0.0);
    }
}
