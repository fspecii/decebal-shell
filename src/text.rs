//! Text rendering with fontdue: glyph cache, measuring and drawing into a tiny-skia pixmap.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::{Color, Pixmap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

/// Loads the UI font. Looks for Inter (the original look) then falls back to common sans fonts.
pub struct Fonts {
    regular: Font,
    medium: Font,
    semibold: Font,
    bold: Font,
    cache: HashMap<(char, u32, Weight), (fontdue::Metrics, Vec<u8>)>,
}

/// Resolves a fontconfig pattern to (file, face index inside a .ttc collection).
fn find_font(patterns: &[&str]) -> Option<(PathBuf, u32)> {
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

fn load(face: &(PathBuf, u32)) -> Option<Font> {
    let data = std::fs::read(&face.0).ok()?;
    Font::from_bytes(data, FontSettings { collection_index: face.1, ..FontSettings::default() }).ok()
}

impl Fonts {
    /// `None` only if no usable font file can be found at all.
    pub fn load() -> Option<Self> {
        let regular = find_font(&["Inter", "Inter Variable", "sans-serif"]).and_then(|p| load(&p))?;
        let pick = |patterns: &[&str]| find_font(patterns).and_then(|p| load(&p)).unwrap_or_else(|| regular.clone());
        let medium = pick(&["Inter:weight=medium", "sans-serif:weight=medium"]);
        let semibold = pick(&["Inter:weight=demibold", "sans-serif:weight=demibold"]);
        let bold = pick(&["Inter:weight=bold", "sans-serif:weight=bold"]);
        Some(Self { regular, medium, semibold, bold, cache: HashMap::new() })
    }

    fn font(&self, w: Weight) -> &Font {
        match w {
            Weight::Regular => &self.regular,
            Weight::Medium => &self.medium,
            Weight::SemiBold => &self.semibold,
            Weight::Bold => &self.bold,
        }
    }

    /// Distance from the top of the line box to the baseline at pixel size `px`.
    pub fn ascent(&self, px: f32, weight: Weight) -> f32 {
        self.font(weight).horizontal_line_metrics(px).map(|m| m.ascent).unwrap_or(px * 0.9)
    }

    /// Advance width of `text` at pixel size `px`.
    pub fn measure(&self, text: &str, px: f32, weight: Weight) -> f32 {
        text.chars().map(|c| self.font(weight).metrics(c, px).advance_width).sum()
    }

    /// Draws `text` with its baseline-box top-left at (`x`, `y`) and returns the width drawn.
    pub fn draw(&mut self, pm: &mut Pixmap, text: &str, x: f32, y: f32, px: f32, weight: Weight, color: Color) -> f32 {
        let ascent = self.font(weight).horizontal_line_metrics(px).map(|m| m.ascent).unwrap_or(px * 0.9);
        let baseline = y + ascent;
        let (cr, cg, cb, ca) = (color.red(), color.green(), color.blue(), color.alpha());
        let mut pen = x;
        for ch in text.chars() {
            let key = (ch, px.round() as u32 * 4, weight);
            if !self.cache.contains_key(&key) {
                let rendered = self.font(weight).rasterize(ch, px);
                self.cache.insert(key, rendered);
            }
            let (m, bitmap) = &self.cache[&key];
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            blend_glyph(pm, bitmap, m.width, m.height, gx, gy, (cr, cg, cb, ca));
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
    fn empty_text_is_zero_width() {
        let Some(f) = Fonts::load() else { return };
        assert_eq!(f.measure("", 13.0, Weight::Bold), 0.0);
    }
}
