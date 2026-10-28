//! Font files and glyph rasterisers behind [`crate::text::Fonts`].
//!
//! A face is read through a read-only memory map (nothing is copied; untouched pages cost nothing) and drawn by
//! one of three rasterisers, chosen when the face is first opened:
//!
//! * [`Kind::Fontdue`]: ordinary fonts. fontdue parses every glyph up front (about 12 MB for a Latin face) but
//!   its output is what decebal always drew with, so Latin text looks exactly as before.
//! * [`Kind::Outline`]: fonts fontdue would swallow whole (Noto CJK: 65 535 glyphs, 320 MB and 0.6 s) and variable
//!   fonts (fontdue ignores their weight axis). ttf-parser reads one glyph's outline on demand, tiny-skia fills it.
//! * [`Kind::Bitmap`]: colour emoji fonts (CBDT / sbix PNG strikes), box-filtered down to the text size.

use crate::theme::fontdb::{css_weight, FaceRef};
use std::path::Path;

/// A font file's bytes: memory-mapped when possible.
pub enum Backing {
    Map { ptr: *const u8, len: usize },
    Heap(Vec<u8>),
}

impl Backing {
    pub fn open(path: &Path) -> Option<Backing> {
        let f = std::fs::File::open(path).ok()?;
        let len = f.metadata().ok()?.len() as usize;
        if len == 0 {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: a read-only private mapping of a regular file; it is only read through `bytes()` and
            // unmapped in `Drop`. (A font file truncated while mapped would fault, as with any mmap.)
            let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_PRIVATE, f.as_raw_fd(), 0) };
            if p != libc::MAP_FAILED {
                return Some(Backing::Map { ptr: p as *const u8, len });
            }
        }
        std::fs::read(path).ok().map(Backing::Heap)
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            // SAFETY: `ptr`/`len` describe a live mapping owned by `self`.
            Backing::Map { ptr, len } => unsafe { std::slice::from_raw_parts(*ptr, *len) },
            Backing::Heap(v) => v,
        }
    }
}

impl Drop for Backing {
    fn drop(&mut self) {
        if let Backing::Map { ptr, len } = self {
            // SAFETY: unmapping the mapping created in `open`; no slice outlives `self`.
            unsafe {
                libc::munmap(*ptr as *mut libc::c_void, *len);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fontdue,
    Outline,
    Bitmap,
}

/// Fonts with more glyphs than this are rasterised lazily instead of being parsed whole by fontdue.
const FONTDUE_MAX_GLYPHS: u16 = 6000;

/// An opened face: where it is and how it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub face: FaceRef,
    pub kind: Kind,
}

/// Rasterised glyph: placement relative to the pen / baseline plus coverage (`color`: premultiplied RGBA).
pub struct Glyph {
    pub xmin: i32,
    /// Distance from the baseline up to the bitmap's bottom edge.
    pub ymin: i32,
    pub width: usize,
    pub height: usize,
    pub advance: f32,
    pub bitmap: Vec<u8>,
    pub color: bool,
}

fn parse(bytes: &[u8], index: u32) -> Option<ttf_parser::Face<'_>> {
    ttf_parser::Face::parse(bytes, index).ok()
}

/// Decides how a face is drawn; `None` if the file is not a usable font.
pub fn classify(bytes: &[u8], face: &FaceRef) -> Option<Kind> {
    let f = parse(bytes, face.index)?;
    let t = f.tables();
    Some(if t.cbdt.is_some() || t.sbix.is_some() {
        Kind::Bitmap
    } else if f.is_variable() || f.number_of_glyphs() > FONTDUE_MAX_GLYPHS {
        Kind::Outline
    } else {
        Kind::Fontdue
    })
}

/// Does the face have a glyph for `ch`? (A cmap lookup, no rasterising.)
pub fn covers(bytes: &[u8], index: u32, ch: char) -> bool {
    parse(bytes, index).is_some_and(|f| f.glyph_index(ch).is_some())
}

/// Distance from the top of the line box to the baseline at `px`.
pub fn ascent(bytes: &[u8], index: u32, px: f32) -> Option<f32> {
    let f = parse(bytes, index)?;
    Some(f.ascender() as f32 * px / f.units_per_em() as f32)
}

/// Parses on a short-lived thread (see [`crate::mem::limit_arenas`]): fontdue turns every glyph of the face into
/// small allocations (13 MB for Inter). On the main heap they interleave with long-lived data and leave about 5 MB
/// resident after the face is dropped; in the worker arena nothing else lives next to them, so the whole region
/// goes back to the OS when [`drop_isolated`] frees them.
pub fn load_fontdue_isolated(bytes: &[u8], index: u32) -> Option<fontdue::Font> {
    std::thread::scope(|sc| sc.spawn(|| load_fontdue(bytes, index)).join().ok().flatten())
}

/// Drops parsed faces on a short-lived thread: the thread's cache of freed blocks is flushed back into the
/// arena that owns them when it exits, so no freed block stays pinned by the main thread.
pub fn drop_isolated(faces: Vec<fontdue::Font>) {
    if faces.is_empty() {
        return;
    }
    let _ = std::thread::spawn(move || drop(faces)).join();
}

pub fn load_fontdue(bytes: &[u8], index: u32) -> Option<fontdue::Font> {
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings { collection_index: index, ..fontdue::FontSettings::default() }).ok()
}

pub fn raster_fontdue(font: &fontdue::Font, ch: char, px: f32) -> Glyph {
    let (m, bitmap) = font.rasterize(ch, px);
    Glyph { xmin: m.xmin, ymin: m.ymin, width: m.width, height: m.height, advance: m.advance_width, bitmap, color: false }
}

/// Collects a glyph outline into a tiny-skia path in pixels, y pointing down from the baseline.
struct Outline {
    pb: tiny_skia::PathBuilder,
    s: f32,
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(x * self.s, -y * self.s);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(x * self.s, -y * self.s);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(x1 * self.s, -y1 * self.s, x * self.s, -y * self.s);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(x1 * self.s, -y1 * self.s, x2 * self.s, -y2 * self.s, x * self.s, -y * self.s);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

/// Largest bitmap (pixels) a glyph may need; anything bigger is a broken font or an absurd size.
const MAX_PIXELS: usize = 1 << 20;

/// Rasterises one glyph from its outline: ttf-parser reads the outline (and applies the weight axis of a variable
/// font), tiny-skia fills it with anti-aliasing. Metrics follow fontdue's: integer bearings, fractional advance.
pub fn raster_outline(bytes: &[u8], face: &FaceRef, ch: char, px: f32) -> Option<Glyph> {
    let mut f = parse(bytes, face.index)?;
    if face.variable {
        let _ = f.set_variation(ttf_parser::Tag::from_bytes(b"wght"), css_weight(face.weight));
    }
    let gid = f.glyph_index(ch)?;
    let s = px / f.units_per_em() as f32;
    let advance = f.glyph_hor_advance(gid).unwrap_or(0) as f32 * s;
    let mut b = Outline { pb: tiny_skia::PathBuilder::new(), s };
    let empty = |advance| Glyph { xmin: 0, ymin: 0, width: 0, height: 0, advance, bitmap: Vec::new(), color: false };
    let Some(bb) = f.outline_glyph(gid, &mut b) else { return Some(empty(advance)) };
    let Some(path) = b.pb.finish() else { return Some(empty(advance)) };
    let x0 = (bb.x_min as f32 * s).floor();
    let top = (-(bb.y_max as f32) * s).floor();
    let w = ((bb.x_max as f32 * s).ceil() - x0).max(1.0) as usize;
    let h = ((-(bb.y_min as f32) * s).ceil() - top).max(1.0) as usize;
    if w * h > MAX_PIXELS {
        return Some(empty(advance));
    }
    let mut pm = tiny_skia::Pixmap::new(w as u32, h as u32)?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(tiny_skia::Color::WHITE);
    paint.anti_alias = true;
    pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::from_translate(-x0, -top), None);
    let bitmap = pm.data().chunks_exact(4).map(|p| p[3]).collect();
    // Icon fonts draw glyphs wider than their advance (Nerd Font symbols overhang a one-cell advance): never let
    // a glyph run into the next one.
    let advance = advance.max(x0 + w as f32);
    Some(Glyph { xmin: x0 as i32, ymin: (-top) as i32 - h as i32, width: w, height: h, advance, bitmap, color: false })
}

/// Colour emoji from a bitmap strike, shrunk to the text size with a box filter.
pub fn raster_bitmap(bytes: &[u8], face: &FaceRef, ch: char, px: f32) -> Option<Glyph> {
    let f = parse(bytes, face.index)?;
    let gid = f.glyph_index(ch)?;
    let s = px / f.units_per_em() as f32;
    let advance = f.glyph_hor_advance(gid).unwrap_or(0) as f32 * s;
    let img = f.glyph_raster_image(gid, px.round().clamp(1.0, 65535.0) as u16)?;
    if img.format != ttf_parser::RasterImageFormat::PNG {
        return None;
    }
    let rgba = image::load_from_memory_with_format(img.data, image::ImageFormat::Png).ok()?.to_rgba8();
    let (sw, sh) = (rgba.width() as usize, rgba.height() as usize);
    let k = px / img.pixels_per_em.max(1) as f32;
    let (dw, dh) = (((sw as f32 * k).round() as usize).max(1), ((sh as f32 * k).round() as usize).max(1));
    if dw * dh > MAX_PIXELS {
        return None;
    }
    let mut pre = rgba.into_raw();
    for p in pre.chunks_exact_mut(4) {
        let a = p[3] as u32;
        for c in &mut p[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    let bitmap = box_scale(&pre, sw, sh, dw, dh);
    Some(Glyph { xmin: (img.x as f32 * k).round() as i32, ymin: (img.y as f32 * k).round() as i32, width: dw, height: dh, advance, bitmap, color: true })
}

/// Area-average resize of premultiplied RGBA.
pub fn box_scale(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    if (sw, sh) == (dw, dh) {
        return src.to_vec();
    }
    let mut out = vec![0u8; dw * dh * 4];
    for dy in 0..dh {
        let (y0, y1) = (dy * sh / dh, (((dy + 1) * sh).div_ceil(dh)).min(sh).max(dy * sh / dh + 1));
        for dx in 0..dw {
            let (x0, x1) = (dx * sw / dw, (((dx + 1) * sw).div_ceil(dw)).min(sw).max(dx * sw / dw + 1));
            let mut acc = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * sw + x) * 4;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (dy * dw + dx) * 4;
            for c in 0..4 {
                out[o + c] = ((acc[c] + n / 2) / n) as u8;
            }
        }
    }
    out
}

/// Index of the first family of a chain that has a glyph for a character; `None` when none has.
pub fn first_covering(chain_len: usize, covers: impl Fn(usize) -> bool) -> Option<usize> {
    (0..chain_len).find(|i| covers(*i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_family_with_the_glyph_draws_it() {
        // chain: Latin face (a-z), CJK face (kanji), emoji face
        let has = |i: usize, c: char| match i {
            0 => c.is_ascii_lowercase(),
            1 => ('\u{4e00}'..='\u{9fff}').contains(&c),
            _ => c == '\u{1f600}',
        };
        assert_eq!(first_covering(3, |i| has(i, 'a')), Some(0));
        assert_eq!(first_covering(3, |i| has(i, '日')), Some(1));
        assert_eq!(first_covering(3, |i| has(i, '😀')), Some(2));
        assert_eq!(first_covering(3, |i| has(i, '\u{e0b0}')), None);
        assert_eq!(first_covering(0, |_| true), None);
    }

    #[test]
    fn box_scale_averages_and_keeps_size() {
        let src: Vec<u8> = [255u8, 0, 0, 255, 0, 0, 0, 0].repeat(2).into_iter().chain([255u8, 0, 0, 255, 0, 0, 0, 0].repeat(2)).collect();
        // a 4x2 image: red, transparent, red, transparent | same row again
        let out = box_scale(&src, 4, 2, 2, 1);
        assert_eq!(out.len(), 8);
        assert_eq!(out[3], 128, "half of the pixels were opaque");
        assert_eq!(out[0], 128);
        assert_eq!(box_scale(&src, 4, 2, 4, 2), src);
    }

    #[test]
    fn a_missing_file_is_not_a_face() {
        assert!(Backing::open(Path::new("/nonexistent/font.ttf")).is_none());
        assert!(classify(b"not a font", &FaceRef { file: "x".into(), index: 0, variable: false, weight: 80 }).is_none());
    }
}
