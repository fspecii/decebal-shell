//! Bounded X11 backdrop sampling for shell glass. Never samples the composited
//! root (which would contain our own pixels); reads the wallpaper pixmap and
//! redirected windows strictly below this surface instead.
use crate::glass::{Material, Region};
use std::time::{Duration, Instant};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use x11rb::connection::Connection;
use x11rb::protocol::composite::ConnectionExt as _;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

struct Cached {
    region: Region,
    origin: (i32, i32),
    image: Pixmap,
}
#[derive(Default)]
pub(crate) struct Renderer {
    cache: Vec<Cached>,
    captured: Option<Instant>,
    position: (i32, i32),
    material: Option<(u32, u32)>,
    regions: Vec<Region>,
}
impl Renderer {
    #[allow(clippy::too_many_arguments)] // One X11 surface context plus its material and foreground.
    pub(crate) fn paint(&mut self, c: &RustConnection, screen: &Screen, own: Window, pos: (i32, i32), pm: &mut Pixmap, regions: &[Region], m: Material) -> bool {
        if !m.enabled() || regions.is_empty() {
            self.cache.clear();
            return false;
        }
        let key = (m.refraction.to_bits(), m.blur.to_bits());
        let refresh = self.captured.is_none_or(|t| t.elapsed() >= Duration::from_millis(120)) || self.position != pos || self.material != Some(key) || self.regions != regions;
        if refresh {
            self.cache.clear();
            for &region in regions {
                if ![region.x, region.y, region.w, region.h, region.radius].iter().all(|v| v.is_finite()) || region.w <= 0.0 || region.h <= 0.0 {
                    continue;
                }
                let pad = m.padding();
                let x = (pos.0 + region.x.floor() as i32 - pad).max(0);
                let y = (pos.1 + region.y.floor() as i32 - pad).max(0);
                let right = (pos.0 + (region.x + region.w).ceil() as i32 + pad).min(i32::from(screen.width_in_pixels));
                let bottom = (pos.1 + (region.y + region.h).ceil() as i32 + pad).min(i32::from(screen.height_in_pixels));
                if right <= x || bottom <= y || (right - x) as u64 * (bottom - y) as u64 > 4_000_000 {
                    continue;
                }
                if let Some(mut image) = capture(c, screen, own, (x, y, (right - x) as u16, (bottom - y) as u16)) {
                    if m.blur >= 1.0 {
                        crate::lock::bg::blur(&mut image, m.blur.round() as usize, 1);
                    }
                    self.cache.push(Cached { region, origin: (x - pos.0, y - pos.1), image });
                }
            }
            self.position = pos;
            self.material = Some(key);
            self.captured = Some(Instant::now());
            self.regions = regions.to_vec();
        }
        for region in &self.cache {
            crate::glass::composite(pm, &region.image, region.origin, region.region, Material { blur: 0.0, ..m });
        }
        !self.cache.is_empty()
    }
}
fn read(c: &RustConnection, drawable: Drawable, x: i16, y: i16, w: u16, h: u16) -> Option<Pixmap> {
    let image = c.get_image(ImageFormat::Z_PIXMAP, drawable, x, y, w, h, !0).ok()?.reply().ok()?;
    if image.depth < 24 || image.data.len() != usize::from(w) * usize::from(h) * 4 {
        return None;
    }
    let mut out = Pixmap::new(u32::from(w), u32::from(h))?;
    for (d, s) in out.data_mut().chunks_exact_mut(4).zip(image.data.chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], if image.depth == 32 { s[3] } else { 255 }]);
    }
    Some(out)
}
fn capture(c: &RustConnection, screen: &Screen, own: Window, rect: (i32, i32, u16, u16)) -> Option<Pixmap> {
    if c.setup().image_byte_order != ImageOrder::LSB_FIRST || !c.setup().pixmap_formats.iter().any(|f| f.depth == screen.root_depth && f.bits_per_pixel == 32) {
        return None;
    }
    let (x, y, w, h) = rect;
    let _ = c.composite_query_version(0, 4).ok().and_then(|r| r.reply().ok());
    let tree = c.query_tree(screen.root).ok()?.reply().ok()?;
    let own_index = tree.children.iter().position(|&win| win == own)?;
    let mut base = None;
    for name in [b"_XROOTPMAP_ID".as_slice(), b"ESETROOT_PMAP_ID".as_slice()] {
        let a = c.intern_atom(true, name).ok()?.reply().ok()?.atom;
        if a == 0 {
            continue;
        }
        let p = c.get_property(false, screen.root, a, AtomEnum::PIXMAP, 0, 1).ok()?.reply().ok()?.value32().and_then(|mut v| v.next());
        if let Some(p) = p {
            base = read(c, p, x as i16, y as i16, w, h);
            if base.is_some() {
                break;
            }
        }
    }
    let mut base = base?;
    // Bound round trips in sessions with many retired/hidden root children.
    let lower = &tree.children[..own_index];
    for &win in &lower[lower.len().saturating_sub(128)..] {
        let Some(g) = c.get_geometry(win).ok().and_then(|v| v.reply().ok()) else {
            continue;
        };
        let (wx, wy) = (i32::from(g.x) + i32::from(g.border_width), i32::from(g.y) + i32::from(g.border_width));
        let (left, top) = (x.max(wx), y.max(wy));
        let (right, bottom) = ((x + i32::from(w)).min(wx + i32::from(g.width)), (y + i32::from(h)).min(wy + i32::from(g.height)));
        if right <= left || bottom <= top {
            continue;
        }
        let Some(a) = c.get_window_attributes(win).ok().and_then(|v| v.reply().ok()) else {
            continue;
        };
        if a.map_state != MapState::VIEWABLE || a.class == WindowClass::INPUT_ONLY {
            continue;
        }
        let Some(pix) = c.generate_id().ok() else {
            continue;
        };
        if !c.composite_name_window_pixmap(win, pix).ok().is_some_and(|r| r.check().is_ok()) {
            continue;
        }
        let image = read(c, pix, (left - wx + i32::from(g.border_width)) as i16, (top - wy + i32::from(g.border_width)) as i16, (right - left) as u16, (bottom - top) as u16);
        let _ = c.free_pixmap(pix);
        if let Some(image) = image {
            base.draw_pixmap(left - x, top - y, image.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    Some(base)
}
