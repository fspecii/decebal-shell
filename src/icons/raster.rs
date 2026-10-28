//! Turning an icon file into pixels: SVG (resvg), PNG and XPM, fitted into a square canvas.
//!
//! Sharpness rules: SVG is rendered straight at the target size (never scaled afterwards) and placed on whole
//! pixels; bitmaps that shrink go through an exact area average (a plain bilinear sample of a 256 px icon at 48 px
//! aliases and looks rough), bitmaps that grow through a bicubic filter.

use super::resolve::{Found, Kind};
use std::path::Path;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// How the icon sits in its square: `content` px is the side of the box the artwork is fitted into
/// (`icons.size_scale` makes it smaller or larger than the canvas), `tint` repaints it in one colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opts {
    pub content: u32,
    pub tint: Option<(u8, u8, u8)>,
}

/// The `size` x `size` pixmap for `found`, or `None` when the file cannot be read or decoded.
pub fn render(found: &Found, size: u32, opts: Opts) -> Option<Pixmap> {
    let mut pm = match found.kind {
        Kind::Svg => svg(&found.path, size, opts.content)?,
        Kind::Png => fit(&Pixmap::load_png(&found.path).ok()?, size, opts.content)?,
        Kind::Xpm => fit(&super::xpm::parse(&std::fs::read_to_string(&found.path).ok()?)?, size, opts.content)?,
    };
    if let Some(c) = opts.tint.filter(|_| is_symbolic(&found.path)) {
        tint(&mut pm, c);
    }
    Some(pm)
}

/// `foo-symbolic.svg`, `foo-symbolic.symbolic.png` or anything in a `symbolic/` directory.
pub fn is_symbolic(path: &Path) -> bool {
    path.file_stem().is_some_and(|s| s.to_string_lossy().contains("symbolic")) || path.components().any(|c| c.as_os_str() == "symbolic")
}

/// Repaints every pixel in `c`, keeping only the alpha of the artwork (what GTK does for symbolic icons).
pub fn tint(pm: &mut Pixmap, c: (u8, u8, u8)) {
    for px in pm.data_mut().chunks_exact_mut(4) {
        let a = u32::from(px[3]);
        px[0] = (u32::from(c.0) * a / 255) as u8;
        px[1] = (u32::from(c.1) * a / 255) as u8;
        px[2] = (u32::from(c.2) * a / 255) as u8;
    }
}

/// Renders an SVG so that it fits `content` px, centred on whole pixels in a `size` square.
pub fn svg(path: &Path, size: u32, content: u32) -> Option<Pixmap> {
    let data = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
    let mut pm = Pixmap::new(size.max(1), size.max(1))?;
    let s = tree.size();
    if s.width() <= 0.0 || s.height() <= 0.0 {
        return None;
    }
    let content = content.max(1) as f32;
    let scale = (content / s.width()).min(content / s.height());
    let (w, h) = (s.width() * scale, s.height() * scale);
    let dx = ((size as f32 - w) / 2.0).floor();
    let dy = ((size as f32 - h) / 2.0).floor();
    resvg::render(&tree, Transform::from_scale(scale, scale).post_translate(dx, dy), &mut pm.as_mut());
    Some(pm)
}

/// Fits a bitmap into `content` px (aspect kept), centred on whole pixels in a `size` square.
pub fn fit(src: &Pixmap, size: u32, content: u32) -> Option<Pixmap> {
    let mut out = Pixmap::new(size.max(1), size.max(1))?;
    let content = content.max(1) as f32;
    let scale = (content / src.width() as f32).min(content / src.height() as f32);
    let tw = ((src.width() as f32 * scale).round() as u32).max(1);
    let th = ((src.height() as f32 * scale).round() as u32).max(1);
    let x = (size as i64 - tw as i64).div_euclid(2) as i32;
    let y = (size as i64 - th as i64).div_euclid(2) as i32;
    if (tw, th) == (src.width(), src.height()) {
        out.draw_pixmap(x, y, src.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    } else if scale < 1.0 {
        let small = area(src, tw, th)?;
        out.draw_pixmap(x, y, small.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    } else {
        let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..PixmapPaint::default() };
        let t = Transform::from_scale(tw as f32 / src.width() as f32, th as f32 / src.height() as f32).post_translate(x as f32, y as f32);
        out.draw_pixmap(0, 0, src.as_ref(), &paint, t, None);
    }
    Some(out)
}

/// For every destination index the source indices it covers and how much of each (weights sum to 1).
fn weights(src: usize, dst: usize) -> Vec<Vec<(usize, f32)>> {
    let ratio = src as f32 / dst as f32;
    (0..dst)
        .map(|d| {
            let (start, end) = (d as f32 * ratio, (d as f32 + 1.0) * ratio);
            let mut v = Vec::new();
            for s in start.floor() as usize..(end.ceil() as usize).min(src) {
                let overlap = (s as f32 + 1.0).min(end) - (s as f32).max(start);
                if overlap > 0.0 {
                    v.push((s, overlap / ratio));
                }
            }
            v
        })
        .collect()
}

/// Exact area-average downscale of a premultiplied pixmap (averaging premultiplied values is the right blend).
pub fn area(src: &Pixmap, dw: u32, dh: u32) -> Option<Pixmap> {
    let (sw, sh, dw, dh) = (src.width() as usize, src.height() as usize, dw as usize, dh as usize);
    let (wx, wy) = (weights(sw, dw), weights(sh, dh));
    let data = src.data();
    // horizontal pass: sh rows of dw pixels
    let mut tmp = vec![0f32; dw * sh * 4];
    for y in 0..sh {
        for (x, ws) in wx.iter().enumerate() {
            let mut acc = [0f32; 4];
            for (sx, w) in ws {
                let i = (y * sw + sx) * 4;
                for c in 0..4 {
                    acc[c] += f32::from(data[i + c]) * w;
                }
            }
            tmp[(y * dw + x) * 4..(y * dw + x) * 4 + 4].copy_from_slice(&acc);
        }
    }
    let mut out = Pixmap::new(dw as u32, dh as u32)?;
    let o = out.data_mut();
    for (y, ws) in wy.iter().enumerate() {
        for x in 0..dw {
            let mut acc = [0f32; 4];
            for (sy, w) in ws {
                let i = (sy * dw + x) * 4;
                for c in 0..4 {
                    acc[c] += tmp[i + c] * w;
                }
            }
            let a = acc[3].round().clamp(0.0, 255.0);
            let j = (y * dw + x) * 4;
            for c in 0..3 {
                o[j + c] = acc[c].round().clamp(0.0, a) as u8;
            }
            o[j + 3] = a as u8;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        for p in pm.data_mut().chunks_exact_mut(4) {
            p.copy_from_slice(&rgba);
        }
        pm
    }

    #[test]
    fn area_average_keeps_flat_colours_and_alpha_exactly() {
        let src = solid(256, 256, [10, 20, 30, 255]);
        let out = area(&src, 47, 47).unwrap();
        assert!(out.data().chunks_exact(4).all(|p| p == [10, 20, 30, 255]), "no drift on a flat colour");
        let half = solid(100, 100, [50, 50, 50, 128]);
        let o = area(&half, 33, 33).unwrap();
        assert!(o.data().chunks_exact(4).all(|p| p == [50, 50, 50, 128]));
    }

    #[test]
    fn area_average_is_a_box_filter_on_a_checker() {
        let mut src = Pixmap::new(4, 4).unwrap();
        for y in 0..4usize {
            for x in 0..4usize {
                if (x + y) % 2 == 0 {
                    src.data_mut()[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        let out = area(&src, 2, 2).unwrap();
        // every 2x2 block holds two white and two transparent pixels
        assert!(out.data().chunks_exact(4).all(|p| p == [128, 128, 128, 128]), "{:?}", out.data());
    }

    #[test]
    fn fit_centres_on_whole_pixels_and_keeps_the_aspect() {
        let src = solid(100, 50, [255, 0, 0, 255]);
        let out = fit(&src, 40, 40).unwrap();
        assert_eq!((out.width(), out.height()), (40, 40));
        assert_eq!(out.pixel(20, 20).unwrap().alpha(), 255);
        assert_eq!(out.pixel(20, 5).unwrap().alpha(), 0, "letterboxed top");
        assert_eq!(out.pixel(0, 10).unwrap().alpha(), 255, "full width: the bar starts at the left edge");
        // the red band is exactly 20 rows high, no soft edge rows
        let rows = (0..40).filter(|y| out.pixel(20, *y).unwrap().alpha() == 255).count();
        let soft = (0..40).filter(|y| matches!(out.pixel(20, *y).unwrap().alpha(), 1..=254)).count();
        assert_eq!((rows, soft), (20, 0));
    }

    #[test]
    fn a_smaller_content_box_shrinks_the_artwork_inside_the_same_canvas() {
        let src = solid(64, 64, [0, 255, 0, 255]);
        let out = fit(&src, 40, 30).unwrap();
        assert_eq!((out.width(), out.height()), (40, 40));
        assert_eq!(out.pixel(2, 2).unwrap().alpha(), 0);
        assert_eq!(out.pixel(20, 20).unwrap().alpha(), 255);
        assert_eq!(out.pixel(4, 20).unwrap().alpha(), 0);
        assert_eq!(out.pixel(5, 20).unwrap().alpha(), 255, "30 px box centred in 40 starts at x=5");
        let inside = (0..40).filter(|x| out.pixel(*x, 20).unwrap().alpha() == 255).count();
        assert_eq!(inside, 30);
        // larger than the canvas: cropped, still full
        let big = fit(&src, 40, 60).unwrap();
        assert!(big.data().chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn upscaling_a_small_bitmap_works() {
        let out = fit(&solid(16, 16, [1, 2, 3, 255]), 48, 48).unwrap();
        assert_eq!(out.pixel(24, 24).unwrap().alpha(), 255);
        assert_eq!(out.pixel(0, 0).unwrap().alpha(), 255);
    }

    #[test]
    fn tint_keeps_alpha_and_replaces_colour() {
        let mut pm = solid(2, 2, [20, 20, 20, 128]);
        tint(&mut pm, (255, 255, 255));
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 128);
        assert_eq!(pm.pixel(0, 0).unwrap().red(), 128, "premultiplied white at half alpha");
    }

    #[test]
    fn symbolic_files_are_recognised() {
        assert!(is_symbolic(Path::new("/x/Adwaita/symbolic/apps/foo.svg")));
        assert!(is_symbolic(Path::new("/x/a/16x16/status/audio-volume-high-symbolic.svg")));
        assert!(is_symbolic(Path::new("/x/a/foo-symbolic.symbolic.png")));
        assert!(!is_symbolic(Path::new("/x/a/scalable/apps/firefox.svg")));
    }

    #[test]
    fn svg_renders_crisply_on_whole_pixels() {
        let p = std::env::temp_dir().join(format!("macshell-raster-{}.svg", std::process::id()));
        std::fs::write(&p, r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><rect width="64" height="64" fill="#ff0000"/></svg>"##).unwrap();
        let pm = svg(&p, 47, 47).unwrap();
        assert!(pm.data().chunks_exact(4).all(|q| q == [255, 0, 0, 255]), "fills the canvas exactly");
        let pm = svg(&p, 47, 30).unwrap();
        let cols = (0..47).filter(|x| pm.pixel(*x, 23).unwrap().alpha() == 255).count();
        assert!((29..=31).contains(&cols), "{cols}");
        let soft = (0..47).filter(|x| matches!(pm.pixel(*x, 23).unwrap().alpha(), 1..=254)).count();
        assert!(soft <= 2, "edges land on whole pixels: {soft}");
        let _ = std::fs::remove_file(p);
    }
}
