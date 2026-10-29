//! Native glass material. Backdrop pixels and foreground are kept separate: only
//! the rounded bezel bends the scene, while labels and icons retain their pixels.
use tiny_skia::Pixmap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub refraction: f32,
    pub edge: f32,
    pub highlight: f32,
    pub grain: f32,
    pub blur: f32,
}
impl Material {
    pub fn from_theme(t: &crate::theme::Theme) -> Self {
        Self { refraction: t.glass_refraction, edge: t.glass_edge, highlight: t.glass_highlight, grain: t.glass_grain, blur: t.glass_blur }
    }
    pub fn enabled(self) -> bool {
        self.refraction > 0.0
    }
    pub fn padding(self) -> i32 {
        (self.refraction + self.blur + 2.0).ceil() as i32
    }
}
impl Region {
    /// Inward distance and outward unit normal of the nearest rounded boundary.
    fn boundary(self, x: f32, y: f32) -> (f32, f32, f32) {
        let r = self.radius.max(0.0).min(self.w.min(self.h) * 0.5);
        let (dx, dy) = (x - x.clamp(self.x + r, self.x + self.w - r), y - y.clamp(self.y + r, self.y + self.h - r));
        if dx != 0.0 && dy != 0.0 {
            let len = dx.hypot(dy);
            return (r - len, dx / len, dy / len);
        }
        [(x - self.x, -1.0, 0.0), (self.x + self.w - x, 1.0, 0.0), (y - self.y, 0.0, -1.0), (self.y + self.h - y, 0.0, 1.0)].into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap()
    }
}
fn sample(pm: &Pixmap, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, pm.width().saturating_sub(1) as f32);
    let y = y.clamp(0.0, pm.height().saturating_sub(1) as f32);
    let (ix, iy) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let mut out = [0.0; 3];
    for (xx, wx) in [(ix, 1.0 - fx), ((ix + 1).min(pm.width() - 1), fx)] {
        for (yy, wy) in [(iy, 1.0 - fy), ((iy + 1).min(pm.height() - 1), fy)] {
            let n = ((yy * pm.width() + xx) * 4) as usize;
            for (c, d) in out.iter_mut().enumerate() {
                *d += pm.data()[n + c] as f32 * wx * wy;
            }
        }
    }
    out
}
/// Compose one screen-registered backdrop crop into a visible region. `origin`
/// is that crop's origin in surface coordinates, so moving a surface never drags
/// its texture. `foreground` contains ordinary premultiplied drawing commands.
pub fn composite(foreground: &mut Pixmap, backdrop: &Pixmap, origin: (i32, i32), region: Region, m: Material) {
    if !m.enabled() {
        return;
    }
    let mut softened = backdrop.clone();
    if m.blur >= 1.0 {
        crate::lock::bg::blur(&mut softened, (m.blur.round() as usize).min(24), 1);
    }
    let (x0, y0) = (region.x.floor().max(0.0) as u32, region.y.floor().max(0.0) as u32);
    let (x1, y1) = ((region.x + region.w).ceil().min(foreground.width() as f32) as u32, (region.y + region.h).ceil().min(foreground.height() as f32) as u32);
    let edge = m.edge.min(region.w.min(region.h) * 0.5).max(1.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let (d, nx, ny) = region.boundary(x as f32 + 0.5, y as f32 + 0.5);
            if d < -0.5 {
                continue;
            }
            let t = (1.0 - d / edge).clamp(0.0, 1.0);
            let bend = m.refraction * (std::f32::consts::PI * t).sin().powi(2);
            let bg = sample(&softened, x as f32 - origin.0 as f32 - nx * bend, y as f32 - origin.1 as f32 - ny * bend);
            let i = ((y * foreground.width() + x) * 4) as usize;
            let p = &mut foreground.data_mut()[i..i + 4];
            let a = p[3] as f32 / 255.0;
            // Fully opaque foreground is invariant; highlights belong to the material beneath it.
            let light = (-nx * 0.55 - ny * 0.83).clamp(-1.0, 1.0);
            let rim = (-d.max(0.0) / 1.1).exp() * m.highlight;
            let noise = (((x.wrapping_mul(1973) ^ y.wrapping_mul(9277) ^ 89173).wrapping_mul(26699) & 65535) as f32 / 65535.0 - 0.5) * m.grain * 255.0;
            let coverage = (d + 0.5).clamp(0.0, 1.0);
            for c in 0..3 {
                let material = (bg[c] + rim * light * 100.0 + noise).clamp(0.0, 255.0);
                p[c] = ((p[c] as f32 + material * (1.0 - a)) * coverage).round().clamp(0.0, 255.0) as u8;
            }
            p[3] = (coverage * 255.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn material() -> Material {
        Material { refraction: 12.0, edge: 14.0, highlight: 0.0, grain: 0.0, blur: 0.0 }
    }
    fn scene() -> Pixmap {
        let mut p = Pixmap::new(220, 140).unwrap();
        for y in 0..140 {
            for x in 0..220 {
                let n = ((y * 220 + x) * 4) as usize;
                let c = if x % 12 < 6 { 230 } else { 20 };
                p.data_mut()[n..n + 4].copy_from_slice(&[c, 90, 255 - c, 255]);
            }
        }
        p
    }
    #[test]
    fn edge_lens_warps_background_but_center_and_foreground_stay_crisp() {
        let bg = scene();
        let r = Region { x: 20.0, y: 20.0, w: 160.0, h: 100.0, radius: 18.0 };
        let mut p = Pixmap::new(220, 140).unwrap();
        let glyph = ((25 * 220 + 80) * 4) as usize;
        p.data_mut()[glyph..glyph + 4].copy_from_slice(&[10, 240, 30, 255]);
        composite(&mut p, &bg, (0, 0), r, material());
        assert_eq!(&p.data()[glyph..glyph + 4], &[10, 240, 30, 255], "opaque ink must never refract or blur");
        let center = ((70 * 220 + 100) * 4) as usize;
        assert_eq!(&p.data()[center..center + 4], &bg.data()[center..center + 4]);
        let mut changed = 0;
        for y in 40..100 {
            for x in 22..32 {
                let i = ((y * 220 + x) * 4) as usize;
                if p.data()[i] != bg.data()[i] {
                    changed += 1;
                }
            }
        }
        assert!(changed > 100, "a tinted rectangle is not a refracting lens");
        assert_eq!(&p.data()[0..4], &[0, 0, 0, 0], "unused canvas must stay transparent");
    }
    #[test]
    fn moving_surface_keeps_backdrop_registered_to_screen() {
        let bg = scene();
        let mut a = Pixmap::new(100, 100).unwrap();
        let mut b = a.clone();
        let r = Region { x: 0.0, y: 0.0, w: 100.0, h: 100.0, radius: 12.0 };
        composite(&mut a, &bg, (0, 0), r, material());
        composite(&mut b, &bg, (-40, 0), r, material());
        let at = |p: &Pixmap, x: u32| p.data()[((50 * 100 + x) * 4) as usize];
        assert_eq!(at(&a, 70), at(&b, 30));
        let mut disabled = Pixmap::new(100, 100).unwrap();
        composite(&mut disabled, &bg, (0, 0), r, Material { refraction: 0.0, ..material() });
        assert!(disabled.data().iter().all(|v| *v == 0));
    }
}
