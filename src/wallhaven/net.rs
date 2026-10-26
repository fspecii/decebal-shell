//! Background workers: search requests, thumbnail download + decode, and apply/download jobs.
//!
//! The UI thread never blocks; workers push [`Msg`]s into a channel and call the waker so the
//! backend's event loop runs `tick()`.

use super::api::{self, Item, Page};
use super::wallpaper;
use crate::window_app::Waker;
use std::io::Read;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_skia::{IntSize, Pixmap};

const WORKERS: usize = 4;
const MAX_REMOTE_BYTES: u64 = 12 * 1024 * 1024;
const MAX_LOCAL_BYTES: u64 = 64 * 1024 * 1024;
/// Reject images that would decode to more than this many pixels.
const MAX_PIXELS: u64 = 120_000_000;

pub enum Job {
    Search { gen: u64, page: u32, params: Vec<(String, String)> },
    Thumb { id: String, src: String, max_w: u32 },
    Recent { gen: u64 },
}

pub enum Msg {
    Page { gen: u64, page: u32, result: Result<Page, String> },
    Local { gen: u64, items: Vec<Item> },
    Thumb { id: String, pixmap: Option<Pixmap> },
    Applied { id: String, result: Result<String, String> },
    Saved { id: String, result: Result<String, String> },
}

pub struct Net {
    jobs: Sender<Job>,
    msgs: Receiver<Msg>,
    out: Sender<Msg>,
    waker: Waker,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(20)).user_agent("macshell-machaven/0.1").build()
}

impl Net {
    /// Starts the worker threads.
    pub fn start(waker: Waker) -> Net {
        let (jobs, job_rx) = channel::<Job>();
        let (out, msgs) = channel::<Msg>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let agent = agent();
        for _ in 0..WORKERS {
            let (rx, tx, wake, agent) = (job_rx.clone(), out.clone(), waker.clone(), agent.clone());
            std::thread::spawn(move || loop {
                let job = match rx.lock() {
                    Ok(guard) => guard.recv(),
                    Err(_) => return,
                };
                let Ok(job) = job else { return };
                let msg = run_job(&agent, job);
                if tx.send(msg).is_err() {
                    return;
                }
                wake();
            });
        }
        Net { jobs, msgs, out, waker }
    }

    pub fn submit(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    /// Drains finished work without blocking.
    pub fn poll(&self) -> Vec<Msg> {
        self.msgs.try_iter().collect()
    }

    /// Sets a wallpaper on its own thread (can take a while: full-size download).
    pub fn apply(&self, item: Item) {
        let (tx, wake) = (self.out.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let result = wallpaper::apply(&item);
            let _ = tx.send(Msg::Applied { id: item.id, result });
            wake();
        });
    }

    /// Downloads a wallpaper without applying it.
    pub fn save(&self, item: Item) {
        let (tx, wake) = (self.out.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let result = wallpaper::save_only(&item);
            let _ = tx.send(Msg::Saved { id: item.id, result });
            wake();
        });
    }
}

fn run_job(agent: &ureq::Agent, job: Job) -> Msg {
    match job {
        Job::Search { gen, page, params } => Msg::Page { gen, page, result: search(agent, page, &params) },
        Job::Thumb { id, src, max_w } => {
            let pixmap = load_thumb(agent, &src, max_w).ok();
            Msg::Thumb { id, pixmap }
        }
        Job::Recent { gen } => {
            let items = wallpaper::recent_files().iter().map(|p| wallpaper::local_item(p)).collect();
            Msg::Local { gen, items }
        }
    }
}

fn search(agent: &ureq::Agent, page: u32, params: &[(String, String)]) -> Result<Page, String> {
    let mut req = agent.get(api::API_URL);
    for (k, v) in params {
        req = req.query(k, v);
    }
    let resp = req.call().map_err(|e| e.to_string())?;
    let mut body = String::new();
    resp.into_reader().take(8 * 1024 * 1024).read_to_string(&mut body).map_err(|e| e.to_string())?;
    api::parse(&body, page)
}

fn read_limited(mut r: impl Read, max: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    r.by_ref().take(max + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err("image too large".into());
    }
    Ok(buf)
}

fn load_thumb(agent: &ureq::Agent, src: &str, max_w: u32) -> Result<Pixmap, String> {
    let bytes = if let Some(path) = src.strip_prefix("file:") {
        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        read_limited(f, MAX_LOCAL_BYTES)?
    } else if src.starts_with("https://") {
        let resp = agent.get(src).call().map_err(|e| e.to_string())?;
        read_limited(resp.into_reader(), MAX_REMOTE_BYTES)?
    } else {
        return Err("unsupported thumbnail source".into());
    };
    decode_thumb(&bytes, max_w)
}

/// Decodes `bytes`, downsizes to at most `max_w` wide and returns a premultiplied pixmap.
pub fn decode_thumb(bytes: &[u8], max_w: u32) -> Result<Pixmap, String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("unreasonable image size".into());
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let mut rgba = img.to_rgba8();
    drop(img);
    if rgba.width() > max_w {
        let nh = ((u64::from(rgba.height()) * u64::from(max_w)) / u64::from(rgba.width())).max(1) as u32;
        rgba = image::imageops::resize(&rgba, max_w, nh, image::imageops::FilterType::Triangle);
    }
    let (w, h) = (rgba.width(), rgba.height());
    let mut data = rgba.into_raw();
    for px in data.chunks_exact_mut(4) {
        let a = u32::from(px[3]);
        if a != 255 {
            px[0] = ((u32::from(px[0]) * a + 127) / 255) as u8;
            px[1] = ((u32::from(px[1]) * a + 127) / 255) as u8;
            px[2] = ((u32::from(px[2]) * a + 127) / 255) as u8;
        }
    }
    let size = IntSize::from_wh(w, h).ok_or("empty image")?;
    Pixmap::from_vec(data, size).ok_or_else(|| "bad pixmap".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn decodes_and_downsizes() {
        let pm = decode_thumb(&png(1200, 600, [200, 10, 10, 255]), 300).unwrap();
        assert_eq!((pm.width(), pm.height()), (300, 150));
        assert_eq!(pm.pixel(5, 5).unwrap().red(), 200);
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let pm = decode_thumb(&png(100, 50, [1, 2, 3, 255]), 300).unwrap();
        assert_eq!(pm.width(), 100);
    }

    #[test]
    fn premultiplies_alpha() {
        let pm = decode_thumb(&png(4, 4, [255, 255, 255, 128]), 300).unwrap();
        let p = pm.pixel(1, 1).unwrap();
        assert_eq!(p.alpha(), 128);
        assert_eq!(p.red(), 128);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_thumb(b"not an image at all", 300).is_err());
        assert!(decode_thumb(&[], 300).is_err());
        assert!(decode_thumb(&png(8, 8, [0, 0, 0, 255])[..20], 300).is_err(), "truncated png");
    }

    #[test]
    fn read_limit_is_enforced() {
        assert!(read_limited(&[0u8; 100][..], 50).is_err());
        assert_eq!(read_limited(&[7u8; 10][..], 50).unwrap().len(), 10);
    }

    #[test]
    fn unsupported_source_is_rejected() {
        let a = agent();
        assert!(load_thumb(&a, "ftp://x/y.jpg", 300).is_err());
        assert!(load_thumb(&a, "file:/definitely/missing.png", 300).is_err());
    }
}
