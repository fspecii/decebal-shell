//! The blurred, dimmed backdrop. Everything is done at a fraction of the screen size (a box downscale, three box-blur
//! passes, a dim) and kept as one small pixmap that is stretched bilinearly when painted, so a 4K screen costs a few
//! hundred kilobytes instead of 33 MB.

use std::path::Path;
use tiny_skia::{Color, Pixmap};

/// Screen pixels per backdrop pixel.
pub const FACTOR: u32 = 8;

/// Size of the backdrop for a `w` x `h` screen.
pub fn small_size(w: u32, h: u32) -> (u32, u32) {
    (w.div_ceil(FACTOR).max(1), h.div_ceil(FACTOR).max(1))
}

/// Accumulates rows of 32-bit BGRX screen data (as `GetImage` returns them, a band at a time) into the small size.
pub struct Downscaler {
    w: usize,
    sw: usize,
    sh: usize,
    sum: Vec<u32>,
    cnt: Vec<u32>,
}

impl Downscaler {
    pub fn new(w: u32, h: u32) -> Downscaler {
        let (sw, sh) = small_size(w, h);
        let n = sw as usize * sh as usize;
        Downscaler { w: w as usize, sw: sw as usize, sh: sh as usize, sum: vec![0; n * 3], cnt: vec![0; n] }
    }

    /// Adds `data`, whole rows starting at screen row `y0` (`w * 4` bytes each).
    pub fn add_rows(&mut self, y0: usize, data: &[u8]) {
        let f = FACTOR as usize;
        for (r, row) in data.chunks_exact(self.w * 4).enumerate() {
            let sy = ((y0 + r) / f).min(self.sh - 1);
            for (x, px) in row.chunks_exact(4).enumerate() {
                let i = sy * self.sw + (x / f).min(self.sw - 1);
                self.sum[i * 3] += u32::from(px[2]);
                self.sum[i * 3 + 1] += u32::from(px[1]);
                self.sum[i * 3 + 2] += u32::from(px[0]);
                self.cnt[i] += 1;
            }
        }
    }

    pub fn finish(self) -> Option<Pixmap> {
        let mut pm = Pixmap::new(self.sw as u32, self.sh as u32)?;
        for (i, px) in pm.data_mut().chunks_exact_mut(4).enumerate() {
            let c = self.cnt[i].max(1);
            px[0] = (self.sum[i * 3] / c) as u8;
            px[1] = (self.sum[i * 3 + 1] / c) as u8;
            px[2] = (self.sum[i * 3 + 2] / c) as u8;
            px[3] = 255;
        }
        Some(pm)
    }
}

/// Gaussian-like blur: `passes` box blurs of `radius` pixels in each direction (edges clamp).
pub fn blur(pm: &mut Pixmap, radius: usize, passes: usize) {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    if radius == 0 || w == 0 || h == 0 {
        return;
    }
    let data = pm.data_mut();
    let mut tmp = vec![0u8; w.max(h) * 4];
    let mut col = vec![0u8; h * 4];
    for _ in 0..passes {
        for row in data.chunks_exact_mut(w * 4) {
            box_line(row, 4, w, radius, &mut tmp);
        }
        for x in 0..w {
            for y in 0..h {
                col[y * 4..y * 4 + 4].copy_from_slice(&data[(y * w + x) * 4..(y * w + x) * 4 + 4]);
            }
            box_line(&mut col, 4, h, radius, &mut tmp);
            for y in 0..h {
                data[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&col[y * 4..y * 4 + 4]);
            }
        }
    }
}

/// One box blur over `n` pixels of `stride` bytes (running sum, edge pixels repeated).
fn box_line(buf: &mut [u8], stride: usize, n: usize, r: usize, tmp: &mut [u8]) {
    let win = (2 * r + 1) as u32;
    for c in 0..3 {
        let at = |i: isize| -> u32 { u32::from(buf[i.clamp(0, n as isize - 1) as usize * stride + c]) };
        let mut sum: u32 = (-(r as isize)..=r as isize).map(at).sum();
        for i in 0..n {
            tmp[i * stride + c] = (sum / win) as u8;
            sum += at(i as isize + r as isize + 1);
            sum -= at(i as isize - r as isize);
        }
    }
    for i in 0..n {
        for c in 0..3 {
            buf[i * stride + c] = tmp[i * stride + c];
        }
    }
}

/// Mixes `tint` over every pixel with opacity `alpha` (0 = unchanged, 1 = flat tint).
pub fn dim(pm: &mut Pixmap, tint: (u8, u8, u8), alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    for px in pm.data_mut().chunks_exact_mut(4) {
        px[0] = (f32::from(px[0]) * (1.0 - a) + f32::from(tint.0) * a) as u8;
        px[1] = (f32::from(px[1]) * (1.0 - a) + f32::from(tint.1) * a) as u8;
        px[2] = (f32::from(px[2]) * (1.0 - a) + f32::from(tint.2) * a) as u8;
    }
}

/// A flat backdrop.
pub fn solid(w: u32, h: u32, c: (u8, u8, u8)) -> Option<Pixmap> {
    let (sw, sh) = small_size(w, h);
    let mut pm = Pixmap::new(sw, sh)?;
    pm.fill(Color::from_rgba8(c.0, c.1, c.2, 255));
    Some(pm)
}

/// Decodes `path` by its content (`~/.face` and Omarchy's `current/background` have no extension).
pub fn open_image(path: &Path) -> Option<image::DynamicImage> {
    image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()
}

/// The picture at `path`, scaled to cover a `w` x `h` screen (centre-cropped), at backdrop size.
pub fn from_image(path: &Path, w: u32, h: u32) -> Option<Pixmap> {
    let img = open_image(path)?.to_rgb8();
    let (sw, sh) = small_size(w, h);
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 {
        return None;
    }
    let scale = (sw as f32 / iw as f32).max(sh as f32 / ih as f32);
    let (nw, nh) = (((iw as f32 * scale).ceil() as u32).max(sw), ((ih as f32 * scale).ceil() as u32).max(sh));
    let small = image::imageops::thumbnail(&img, nw, nh);
    let (cx, cy) = ((nw - sw) / 2, (nh - sh) / 2);
    let crop = image::imageops::crop_imm(&small, cx, cy, sw, sh).to_image();
    let mut pm = Pixmap::new(sw, sh)?;
    for (dst, src) in pm.data_mut().chunks_exact_mut(4).zip(crop.pixels()) {
        dst[..3].copy_from_slice(&src.0);
        dst[3] = 255;
    }
    Some(pm)
}

/// The finished backdrop: blurred and dimmed. `blur_px` is in backdrop pixels.
pub fn finish(mut pm: Pixmap, blur_px: usize, dim_alpha: f32) -> Pixmap {
    blur(&mut pm, blur_px, 3);
    dim(&mut pm, (6, 7, 12), dim_alpha);
    pm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_size_rounds_up_and_is_never_zero() {
        assert_eq!(small_size(1920, 1080), (240, 135));
        assert_eq!(small_size(1921, 1081), (241, 136));
        assert_eq!(small_size(1, 1), (1, 1));
    }

    #[test]
    fn downscale_averages_blocks_and_converts_bgrx() {
        let (w, h) = (16u32, 16u32);
        let mut d = Downscaler::new(w, h);
        // left half pure blue (BGRX: 255,0,0), right half pure red; fed in two bands
        let row: Vec<u8> = (0..w).flat_map(|x| if x < 8 { [255, 0, 0, 0] } else { [0, 0, 255, 0] }).collect();
        let band: Vec<u8> = (0..8).flat_map(|_| row.clone()).collect();
        d.add_rows(0, &band);
        d.add_rows(8, &band);
        let pm = d.finish().unwrap();
        assert_eq!((pm.width(), pm.height()), (2, 2));
        let p = pm.pixel(0, 0).unwrap();
        assert_eq!((p.red(), p.green(), p.blue(), p.alpha()), (0, 0, 255, 255));
        let p = pm.pixel(1, 1).unwrap();
        assert_eq!((p.red(), p.green(), p.blue()), (255, 0, 0));
    }

    #[test]
    fn blur_smooths_an_edge_and_keeps_flat_areas() {
        let mut pm = Pixmap::new(20, 4).unwrap();
        for (i, px) in pm.data_mut().chunks_exact_mut(4).enumerate() {
            let v = if i % 20 < 10 { 0 } else { 200 };
            px.copy_from_slice(&[v, v, v, 255]);
        }
        blur(&mut pm, 2, 3);
        let mid = pm.pixel(9, 1).unwrap().red();
        let left = pm.pixel(0, 1).unwrap().red();
        let right = pm.pixel(19, 1).unwrap().red();
        assert!(mid > 10 && mid < 190, "the edge is soft ({mid})");
        assert!(left < 5 && right > 195, "far from the edge nothing changed ({left}, {right})");
        let mut flat = Pixmap::new(8, 8).unwrap();
        flat.fill(Color::from_rgba8(40, 90, 160, 255));
        blur(&mut flat, 3, 3);
        let p = flat.pixel(4, 4).unwrap();
        assert_eq!((p.red(), p.green(), p.blue()), (40, 90, 160));
    }

    #[test]
    fn dim_darkens_towards_the_tint() {
        let mut pm = solid(80, 80, (200, 200, 200)).unwrap();
        dim(&mut pm, (0, 0, 0), 0.5);
        assert_eq!(pm.pixel(0, 0).unwrap().red(), 100);
    }

    #[test]
    fn an_image_is_cover_cropped_to_the_backdrop() {
        let dir = std::env::temp_dir().join(format!("declock-bg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("w.png");
        // 200 x 100 image: left half red, right half blue; cover into a 160 x 160 screen (20 x 20 backdrop) keeps the middle
        let mut img = image::RgbImage::new(200, 100);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 100 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) };
        }
        img.save(&f).unwrap();
        let pm = from_image(&f, 160, 160).unwrap();
        assert_eq!((pm.width(), pm.height()), (20, 20));
        assert!(pm.pixel(2, 10).unwrap().red() > 200 && pm.pixel(17, 10).unwrap().blue() > 200);
        // a file without an extension is decoded by content
        let bare = dir.join("background");
        img.save_with_format(&bare, image::ImageFormat::Png).unwrap();
        assert!(from_image(&bare, 160, 160).is_some());
        assert!(from_image(&dir.join("missing.png"), 160, 160).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
