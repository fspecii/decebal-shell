//! Album art for the Media tab: `file://` covers are read from disk, `http(s)` ones are fetched once with `curl`
//! into `~/.cache/macshell/dash-art/`. Decoding happens on a worker thread; the UI picks the result up in `tick`.

use super::info::square_pixmap;
use super::mpris::file_url_path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_skia::Pixmap;

/// Side of the stored cover (the card shows it at 208 px; a little more keeps it crisp when scaled).
pub const SIDE: u32 = 256;

/// The decoded cover for one URL; `image` is `None` when it could not be loaded.
pub struct Loaded {
    pub url: String,
    pub image: Option<image::RgbaImage>,
}

#[derive(Default)]
pub struct ArtState {
    /// URL being loaded or shown.
    pub wanted: Option<String>,
    pub pixmap: Option<Pixmap>,
    /// The loaded URL (so a finished load for a stale URL is dropped).
    pub shown: Option<String>,
    slot: Arc<Mutex<Option<Loaded>>>,
}

impl ArtState {
    /// Points the cover at `url` (or none); starts a background load when it is new. Returns true when the picture changed right now.
    pub fn want(&mut self, url: Option<&str>, waker: &Arc<dyn Fn() + Send + Sync>) -> bool {
        if self.wanted.as_deref() == url {
            return false;
        }
        self.wanted = url.map(str::to_string);
        let had = self.pixmap.take().is_some();
        self.shown = None;
        if let Some(u) = url {
            let (u, slot, waker) = (u.to_string(), self.slot.clone(), waker.clone());
            std::thread::spawn(move || {
                let image = fetch(&u);
                if let Ok(mut s) = slot.lock() {
                    *s = Some(Loaded { url: u, image });
                }
                waker();
            });
        }
        had
    }

    /// Takes a finished load if it is for the URL still wanted. Returns true when the pixmap changed.
    pub fn poll(&mut self) -> bool {
        let Some(done) = self.slot.lock().ok().and_then(|mut s| s.take()) else { return false };
        if self.wanted.as_deref() != Some(done.url.as_str()) {
            return false;
        }
        self.shown = Some(done.url);
        self.pixmap = done.image.map(|img| square_pixmap(&img, SIDE));
        true
    }
}

fn cache_file(url: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    Some(base.join("macshell/dash-art").join(format!("{h:016x}")))
}

fn fetch(url: &str) -> Option<image::RgbaImage> {
    if let Some(path) = file_url_path(url) {
        return image::open(path).ok().map(|i| i.to_rgba8());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let cached = cache_file(url)?;
    if !cached.is_file() {
        std::fs::create_dir_all(cached.parent()?).ok()?;
        let tmp = cached.with_extension("part");
        let tmp_s = tmp.to_str()?;
        let got = crate::controlcenter::sys::run(&["curl", "-fsSL", "--max-time", "10", "--max-filesize", "8000000", "-o", tmp_s, url], Duration::from_secs(12));
        if got.is_none() || std::fs::rename(&tmp, &cached).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
    }
    image::open(cached).ok().map(|i| i.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_a_local_cover_and_ignores_stale_loads() {
        let dir = std::env::temp_dir().join(format!("macdash-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cover art.png");
        image::RgbaImage::from_pixel(30, 20, image::Rgba([10, 200, 30, 255])).save(&path).unwrap();
        let url = format!("file://{}", path.to_str().unwrap().replace(' ', "%20"));
        let woke = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let w = woke.clone();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let mut art = ArtState::default();
        assert!(!art.want(Some(&url), &waker));
        for _ in 0..200 {
            if art.poll() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let pm = art.pixmap.as_ref().expect("cover decoded");
        assert_eq!((pm.width(), pm.height()), (SIDE, SIDE));
        assert!(pm.pixel(10, 10).unwrap().green() > 150);
        assert!(woke.load(std::sync::atomic::Ordering::SeqCst) >= 1, "the UI was woken");
        // Switching to no cover drops the picture; wanting the same thing twice does nothing.
        assert!(art.want(None, &waker), "the old picture goes");
        assert!(art.pixmap.is_none() && !art.want(None, &waker));
        // A load that finishes after the wanted URL changed is thrown away.
        art.wanted = Some("file:///other".into());
        *art.slot.lock().unwrap() = Some(Loaded { url: url.clone(), image: Some(image::RgbaImage::new(2, 2)) });
        assert!(!art.poll() && art.pixmap.is_none());
        // Broken and unknown sources fail quietly.
        assert!(fetch("file:///nonexistent/cover.png").is_none());
        assert!(fetch("ftp://x/y.png").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_names_are_stable_per_url() {
        let a = cache_file("https://x/y.jpg");
        assert_eq!(a, cache_file("https://x/y.jpg"));
        assert_ne!(a, cache_file("https://x/z.jpg"));
    }
}
