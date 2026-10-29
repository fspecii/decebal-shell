//! Event-driven X11 glass sampling on a separate connection and worker. The GUI
//! never waits for GetImage or CPU blur; requests/results are bounded to one each.
use crate::{
    app::Waker,
    glass::{Material, Region},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use x11rb::{
    connection::Connection,
    protocol::{
        composite::ConnectionExt as _,
        damage::{self, ConnectionExt as _},
        xproto::*,
        Event,
    },
    rust_connection::RustConnection,
};

struct Cached {
    origin: (i32, i32),
    image: Pixmap,
}
#[derive(Clone, PartialEq)]
struct Key {
    position: (i32, i32),
    screen_size: (u16, u16),
    material: (u32, u32),
    regions: Vec<Region>,
}
struct Job {
    key: Key,
    screen: Screen,
    own: Window,
    material: Material,
}
struct Frame {
    key: Key,
    cache: Vec<Cached>,
}
struct Worker {
    jobs: SyncSender<Job>,
    frames: Arc<Mutex<Option<Frame>>>,
}
fn intersects_job(job: &Job, x: i32, y: i32, w: i32, h: i32) -> bool {
    let pad = job.material.padding() as f32;
    job.key.regions.iter().any(|r| {
        let left = job.key.position.0 as f32 + r.x - pad;
        let top = job.key.position.1 as f32 + r.y - pad;
        (x as f32) < left + r.w + 2.0 * pad && ((x + w) as f32) > left && (y as f32) < top + r.h + 2.0 * pad && ((y + h) as f32) > top
    })
}
impl Worker {
    fn start(wake: Option<Waker>) -> Self {
        let (jobs, requests) = mpsc::sync_channel::<Job>(1);
        let frames = Arc::new(Mutex::new(None));
        let results = frames.clone();
        std::thread::spawn(move || {
            let Ok((c, _)) = RustConnection::connect(None) else { return };
            if c.composite_query_version(0, 4).ok().and_then(|r| r.reply().ok()).is_none() || c.damage_query_version(1, 1).ok().and_then(|r| r.reply().ok()).is_none() {
                return;
            }
            let mut job: Option<Job> = None;
            let mut damage = HashMap::new();
            let mut dirty = false;
            let mut sampled = Instant::now() - Duration::from_millis(16);
            loop {
                match requests.recv_timeout(Duration::from_millis(4)) {
                    Ok(next) => {
                        if job.is_none() {
                            let _ = c.change_window_attributes(next.screen.root, &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE));
                        }
                        job = Some(next);
                        dirty = true;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                for _ in 0..256 {
                    match c.poll_for_event() {
                        Ok(Some(Event::DamageNotify(e))) => {
                            let _ = c.damage_subtract(e.damage, 0u32, 0u32);
                            dirty = true;
                        }
                        Ok(Some(Event::ConfigureNotify(e))) => {
                            if job.as_ref().is_some_and(|job| {
                                e.window != job.own && (damage.contains_key(&e.window) || intersects_job(job, i32::from(e.x), i32::from(e.y), i32::from(e.width), i32::from(e.height)))
                            }) {
                                dirty = true;
                            }
                        }
                        Ok(Some(Event::MapNotify(_) | Event::UnmapNotify(_) | Event::DestroyNotify(_) | Event::PropertyNotify(_))) => dirty = true,
                        Ok(Some(_)) => {}
                        Ok(None) => break,
                        Err(_) => return,
                    }
                }
                let _ = c.flush();
                if !dirty || sampled.elapsed() < Duration::from_millis(16) {
                    continue;
                }
                let Some(job) = job.as_ref() else { continue };
                sampled = Instant::now();
                let mut cache = Vec::new();
                let mut watched = HashSet::new();
                for &region in &job.key.regions {
                    if ![region.x, region.y, region.w, region.h, region.radius].iter().all(|v| v.is_finite()) || region.w <= 0.0 || region.h <= 0.0 {
                        continue;
                    }
                    let pos = job.key.position;
                    let pad = job.material.padding();
                    let x = (pos.0 + region.x.floor() as i32 - pad).max(0);
                    let y = (pos.1 + region.y.floor() as i32 - pad).max(0);
                    let right = (pos.0 + (region.x + region.w).ceil() as i32 + pad).min(i32::from(job.screen.width_in_pixels));
                    let bottom = (pos.1 + (region.y + region.h).ceil() as i32 + pad).min(i32::from(job.screen.height_in_pixels));
                    if right <= x || bottom <= y || (right - x) as u64 * (bottom - y) as u64 > 4_000_000 {
                        continue;
                    }
                    if let Some(mut image) = capture(&c, &job.screen, job.own, (x, y, (right - x) as u16, (bottom - y) as u16), &mut damage, &mut watched) {
                        if job.material.blur >= 1.0 {
                            crate::lock::bg::blur(&mut image, job.material.blur.round() as usize, 1);
                        }
                        cache.push(Cached { origin: (x - pos.0, y - pos.1), image });
                    }
                }
                damage.retain(|window, id| {
                    if watched.contains(window) {
                        true
                    } else {
                        let _ = c.damage_destroy(*id);
                        false
                    }
                });
                let _ = c.flush();
                // Replace the pending frame: slow consumers never build a stale frame queue.
                *results.lock().unwrap() = Some(Frame { key: job.key.clone(), cache });
                if let Some(wake) = &wake {
                    wake.wake();
                }
                dirty = false;
            }
            // Closing this connection also releases all Damage objects and event subscriptions.
        });
        Self { jobs, frames }
    }
}
#[derive(Default)]
pub(crate) struct Renderer {
    cache: Vec<Cached>,
    worker: Option<Worker>,
    requested: Option<Key>,
    pending: Option<Frame>,
    wake: Option<Waker>,
}
impl Renderer {
    pub(crate) fn new(wake: Waker) -> Self {
        Self { wake: Some(wake), ..Self::default() }
    }
    pub(crate) fn ready(&mut self) -> bool {
        if let Some(worker) = &self.worker {
            if let Some(frame) = worker.frames.lock().unwrap().take() {
                self.pending = Some(frame);
            }
        }
        self.pending.is_some()
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(&mut self, _c: &RustConnection, screen: &Screen, own: Window, pos: (i32, i32), pm: &mut Pixmap, regions: &[Region], m: Material) -> bool {
        if !m.enabled() || regions.is_empty() {
            self.worker = None;
            self.cache.clear();
            self.pending = None;
            self.requested = None;
            return false;
        }
        let key = Key { position: pos, screen_size: (screen.width_in_pixels, screen.height_in_pixels), material: (m.refraction.to_bits(), m.blur.to_bits()), regions: regions.to_vec() };
        self.ready();
        if let Some(frame) = self.pending.take() {
            if frame.key.position == key.position && frame.key.screen_size == key.screen_size && frame.key.material == key.material && frame.key.regions.len() == key.regions.len() {
                self.cache = frame.cache;
            }
        }
        if self.requested.as_ref() != Some(&key) {
            if self.requested.as_ref().is_some_and(|old| old.position != key.position || old.screen_size != key.screen_size || old.material != key.material || old.regions.len() != key.regions.len()) {
                self.cache.clear();
            }
            let worker = self.worker.get_or_insert_with(|| Worker::start(self.wake.clone()));
            if worker.jobs.try_send(Job { key: key.clone(), screen: screen.clone(), own, material: m }).is_ok() {
                self.requested = Some(key);
            }
        }
        for (cached, region) in self.cache.iter().zip(regions) {
            crate::glass::composite(pm, &cached.image, cached.origin, *region, Material { blur: 0.0, ..m });
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
fn capture(c: &RustConnection, screen: &Screen, own: Window, rect: (i32, i32, u16, u16), damage: &mut HashMap<Window, u32>, watched: &mut HashSet<Window>) -> Option<Pixmap> {
    if c.setup().image_byte_order != ImageOrder::LSB_FIRST || !c.setup().pixmap_formats.iter().any(|f| f.depth == screen.root_depth && f.bits_per_pixel == 32) {
        return None;
    }
    let (x, y, w, h) = rect;
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
    let geometry: Vec<_> = lower[lower.len().saturating_sub(128)..].iter().filter_map(|&win| c.get_geometry(win).ok().map(|cookie| (win, cookie))).collect();
    for (win, cookie) in geometry {
        let Some(g) = cookie.reply().ok() else {
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
        watched.insert(win);
        if let std::collections::hash_map::Entry::Vacant(entry) = damage.entry(win) {
            if let Ok(id) = c.generate_id() {
                if c.damage_create(id, win, damage::ReportLevel::NON_EMPTY).ok().is_some_and(|r| r.check().is_ok()) {
                    entry.insert(id);
                }
            }
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
