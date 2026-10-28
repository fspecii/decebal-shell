//! Colours of a wallpaper: the seed, a few more distinct colours, and how bright the picture is.
//!
//! The image is shrunk to a 96 px thumbnail before anything is looked at (the only expensive step is decoding it).
//! From the thumbnail:
//!
//! * the **seed** is Material's pick ([`material::seed_scored`]: hue popularity smoothed over +-15 degrees, then
//!   chroma), the colour every scheme is built around;
//! * **others** come from a small deterministic k-means in OKLab (k = 6, farthest-point start, 8 rounds):
//!   the most prominent clusters whose hue is at least 28 degrees away from the seed and from each other
//!   (wallust/pywal pick their accent colours the same way). Only the `palette` scheme uses them;
//! * **tone** is the mean CIE L* (is this a light or a dark picture?);
//! * **grey** says the picture has no colour worth matching (a grey, black or white wallpaper): callers then keep
//!   the theme's own colours instead of inventing an accent.
//!
//! Results are cached in `$XDG_CACHE_HOME/decebal/wallpaper-colors.json` keyed by path, modification time and size,
//! so changing back to a known wallpaper (or any palette setting) never decodes an image again.

use super::material::{self, hue_turn, oklch, tone_of, CHROMA_SCALE, DEFAULT_SEED};
use super::model::Rgb;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Side of the thumbnail the colours are read from.
const THUMB: u32 = 96;
const CLUSTERS: usize = 6;
/// Companion colours must sit at least this many degrees of hue away from each other and the seed.
const MIN_HUE_GAP: f64 = 28.0;
const CACHE_VERSION: u64 = 1;
const CACHE_ENTRIES: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Extracted {
    pub seed: Rgb,
    /// Up to two more distinct colours, most prominent first.
    pub others: Vec<Rgb>,
    /// Mean CIE L* of the picture, 0..100.
    pub tone: f32,
    /// No colour worth matching.
    pub grey: bool,
}

impl Default for Extracted {
    /// Nothing looked at yet: Caelestia's rose, flagged grey so no theme is recoloured by it.
    fn default() -> Self {
        Extracted { seed: DEFAULT_SEED, others: Vec::new(), tone: 25.0, grey: true }
    }
}

impl Extracted {
    /// A colour chosen by hand (never "grey", its tone is the colour's own).
    pub fn from_color(c: Rgb) -> Extracted {
        Extracted { seed: c, others: Vec::new(), tone: ((tone_of(c) * 10.0).round() / 10.0) as f32, grey: false }
    }

    /// Colours of `px` (sRGB triples, any order: the result does not depend on it).
    pub fn from_pixels(px: &[[u8; 3]]) -> Extracted {
        let (seed, grey) = material::seed_scored(px);
        if px.is_empty() {
            return Extracted { seed, others: Vec::new(), tone: 25.0, grey };
        }
        // one decimal: that is what is stored, and a stored value must compare equal to a freshly read one
        let tone = ((px.iter().map(|p| tone_of(Rgb(p[0], p[1], p[2]))).sum::<f64>() / px.len() as f64 * 10.0).round() / 10.0) as f32;
        let others = if grey { Vec::new() } else { companions(px, seed) };
        Extracted { seed, others, tone, grey }
    }

    /// Decodes `path` (PNG, JPEG, WebP; the format is sniffed, not taken from the extension) and reads its colours.
    pub fn from_image(path: &Path) -> Result<Extracted, String> {
        let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let img = image::ImageReader::open(path).map_err(|e| err(&e))?.with_guessed_format().map_err(|e| err(&e))?.decode().map_err(|e| err(&e))?;
        let small = img.thumbnail(THUMB, THUMB).to_rgb8();
        let px: Vec<[u8; 3]> = small.pixels().map(|p| p.0).collect();
        Ok(Extracted::from_pixels(&px))
    }

    /// The accent colours to show as a strip: the seed first, then the companions.
    pub fn swatches(&self) -> Vec<Rgb> {
        std::iter::once(self.seed).chain(self.others.iter().copied()).collect()
    }

    /// Writes `seed`, `accents`, `tone` and `grey` into `o`.
    pub fn to_json_fields(&self, o: &mut Map<String, Value>) {
        o.insert("seed".into(), self.seed.hex().into());
        if !self.others.is_empty() {
            o.insert("accents".into(), self.others.iter().map(|c| Value::from(c.hex())).collect::<Vec<_>>().into());
        }
        o.insert("tone".into(), ((f64::from(self.tone) * 10.0).round() / 10.0).into());
        if self.grey {
            o.insert("grey".into(), true.into());
        }
    }

    pub fn from_json_fields(o: &Map<String, Value>) -> Option<Extracted> {
        let seed = o.get("seed").and_then(Value::as_str).and_then(Rgb::parse)?;
        let others = o.get("accents").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().and_then(Rgb::parse)).take(2).collect()).unwrap_or_default();
        Some(Extracted { seed, others, tone: o.get("tone").and_then(Value::as_f64).map(|t| t.clamp(0.0, 100.0) as f32).unwrap_or(25.0), grey: o.get("grey").and_then(Value::as_bool).unwrap_or(false) })
    }
}

// ---------------------------------------------------------------- k-means companions

type Lab = [f32; 3];

fn lab(c: Rgb) -> Lab {
    let (l, ch, h) = oklch(c);
    [l as f32, (ch * h.to_radians().cos()) as f32, (ch * h.to_radians().sin()) as f32]
}

fn dist2(a: &Lab, b: &Lab) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

struct Cluster {
    rgb: Rgb,
    count: usize,
}

/// Deterministic k-means in OKLab: farthest-point initialisation (ties go to the first pixel), 8 Lloyd rounds.
fn kmeans(px: &[[u8; 3]], k: usize) -> Vec<Cluster> {
    let labs: Vec<Lab> = px.iter().map(|p| lab(Rgb(p[0], p[1], p[2]))).collect();
    let n = labs.len();
    if n == 0 {
        return Vec::new();
    }
    let mean: Lab = {
        let s = labs.iter().fold([0f32; 3], |a, l| [a[0] + l[0], a[1] + l[1], a[2] + l[2]]);
        [s[0] / n as f32, s[1] / n as f32, s[2] / n as f32]
    };
    let first = (0..n).min_by(|&a, &b| dist2(&labs[a], &mean).total_cmp(&dist2(&labs[b], &mean))).unwrap_or(0);
    let mut centres = vec![labs[first]];
    let mut nearest: Vec<f32> = labs.iter().map(|l| dist2(l, &centres[0])).collect();
    while centres.len() < k.min(n) {
        let (far, d) = nearest.iter().enumerate().fold((0, -1.0f32), |best, (i, &d)| if d > best.1 { (i, d) } else { best });
        if d <= 1e-9 {
            break; // every pixel is on a centre already
        }
        centres.push(labs[far]);
        for (i, l) in labs.iter().enumerate() {
            nearest[i] = nearest[i].min(dist2(l, &labs[far]));
        }
    }
    let mut assign = vec![0usize; n];
    for _ in 0..8 {
        for (i, l) in labs.iter().enumerate() {
            assign[i] = (0..centres.len()).min_by(|&a, &b| dist2(l, &centres[a]).total_cmp(&dist2(l, &centres[b]))).unwrap_or(0);
        }
        let mut sums = vec![([0f32; 3], 0usize); centres.len()];
        for (i, l) in labs.iter().enumerate() {
            let s = &mut sums[assign[i]];
            s.0 = [s.0[0] + l[0], s.0[1] + l[1], s.0[2] + l[2]];
            s.1 += 1;
        }
        for (c, (s, cnt)) in centres.iter_mut().zip(&sums) {
            if *cnt > 0 {
                *c = [s[0] / *cnt as f32, s[1] / *cnt as f32, s[2] / *cnt as f32];
            }
        }
    }
    // the colour of a cluster is the average of its pixels, so it is a colour the picture really has
    let mut sums = vec![([0u64; 3], 0usize); centres.len()];
    for (i, p) in px.iter().enumerate() {
        let s = &mut sums[assign[i]];
        for (acc, v) in s.0.iter_mut().zip(p) {
            *acc += u64::from(*v);
        }
        s.1 += 1;
    }
    sums.into_iter().filter(|s| s.1 > 0).map(|(s, cnt)| Cluster { rgb: Rgb((s[0] / cnt as u64) as u8, (s[1] / cnt as u64) as u8, (s[2] / cnt as u64) as u8), count: cnt }).collect()
}

/// Up to two prominent, colourful clusters whose hues differ from the seed's and from each other.
fn companions(px: &[[u8; 3]], seed: Rgb) -> Vec<Rgb> {
    let n = px.len() as f64;
    let mut scored: Vec<(f64, Rgb, f64)> = kmeans(px, CLUSTERS)
        .into_iter()
        .filter_map(|c| {
            let (l, ch, h) = oklch(c.rgb);
            let (chroma, frac) = (ch / CHROMA_SCALE, c.count as f64 / n);
            // small specks and near-greys are not accent colours
            (chroma >= 12.0 && frac >= 0.02 && (0.12..=0.97).contains(&l)).then(|| (frac.sqrt() * (chroma / 50.0).min(1.5), c.rgb, h))
        })
        .collect();
    // best first; equal scores resolve by colour so the order never depends on the cluster numbering
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then((a.1 .0, a.1 .1, a.1 .2).cmp(&(b.1 .0, b.1 .1, b.1 .2))));
    let mut hues = vec![oklch(seed).2];
    let mut out = Vec::new();
    for (_, rgb, h) in scored {
        if out.len() == 2 {
            break;
        }
        if hues.iter().all(|&o| hue_turn(o, h).abs() >= MIN_HUE_GAP) {
            hues.push(h);
            out.push(rgb);
        }
    }
    out
}

// ---------------------------------------------------------------- cache

/// Where the colours of already-seen wallpapers are kept. `None` in unit tests (they must not write into the user's cache).
pub fn default_cache() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let base =
        std::env::var_os("XDG_CACHE_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").filter(|s| !s.is_empty()).map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("decebal/wallpaper-colors.json"))
}

/// Modification time (ns since the epoch) and size: when either changes the cached colours are stale.
fn fingerprint(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let ns = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos() as u64;
    Some((ns, m.len()))
}

fn read_cache(file: &Path) -> Vec<Value> {
    let doc: Value = std::fs::read_to_string(file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if doc["version"].as_u64() != Some(CACHE_VERSION) {
        return Vec::new();
    }
    doc["entries"].as_array().cloned().unwrap_or_default()
}

/// The colours of the image at `path`: from `cache` when its modification time and size still match, else decoded
/// (and remembered). A cache that cannot be read or written is ignored.
pub fn load(path: &Path, cache: Option<&Path>) -> Result<Extracted, String> {
    let fp = fingerprint(path).ok_or_else(|| format!("{}: cannot read the file", path.display()))?;
    let key = path.to_string_lossy().into_owned();
    let mut entries = cache.map(read_cache).unwrap_or_default();
    let hit = entries
        .iter()
        .find(|e| e["path"].as_str() == Some(key.as_str()) && e["mtime"].as_u64() == Some(fp.0) && e["len"].as_u64() == Some(fp.1))
        .and_then(|e| e.as_object().and_then(Extracted::from_json_fields));
    if let Some(x) = hit {
        return Ok(x);
    }
    let x = Extracted::from_image(path)?;
    if let Some(file) = cache {
        entries.retain(|e| e["path"].as_str() != Some(key.as_str()));
        let mut e = Map::new();
        e.insert("path".into(), key.into());
        e.insert("mtime".into(), fp.0.into());
        e.insert("len".into(), fp.1.into());
        x.to_json_fields(&mut e);
        entries.push(Value::Object(e));
        if entries.len() > CACHE_ENTRIES {
            entries.drain(..entries.len() - CACHE_ENTRIES);
        }
        if let Ok(text) = serde_json::to_string(&json!({"version": CACHE_VERSION, "entries": entries})) {
            let _ = crate::settings::io::write_atomic(file, &text);
        }
    }
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(rgb: [u8; 3], n: usize) -> Vec<[u8; 3]> {
        vec![rgb; n]
    }

    fn hue(c: Rgb) -> f64 {
        oklch(c).2
    }

    #[test]
    fn a_known_colour_in_gives_its_hue_out() {
        for (rgb, lo, hi) in [([220u8, 40, 50], 10.0, 40.0), ([30, 160, 80], 140.0, 170.0), ([40, 80, 220], 255.0, 275.0), ([230, 190, 30], 85.0, 110.0)] {
            let x = Extracted::from_pixels(&flat(rgb, 400));
            assert!(!x.grey, "{rgb:?}");
            assert!((lo..=hi).contains(&hue(x.seed)), "{rgb:?}: seed {} hue {}", x.seed.hex(), hue(x.seed));
        }
    }

    #[test]
    fn extraction_is_deterministic_and_ignores_pixel_order() {
        let mut px = flat([40, 90, 200], 900);
        px.extend(flat([210, 120, 20], 500));
        px.extend(flat([30, 170, 90], 300));
        px.extend(flat([20, 20, 24], 800));
        let a = Extracted::from_pixels(&px);
        assert_eq!(a, Extracted::from_pixels(&px));
        let mut rev = px.clone();
        rev.reverse();
        assert_eq!(a.seed, Extracted::from_pixels(&rev).seed);
        assert_eq!(a.others, Extracted::from_pixels(&rev).others, "companions do not depend on the pixel order");
    }

    #[test]
    fn companions_are_distinct_hues_of_the_picture() {
        let mut px = flat([40, 90, 200], 900); // blue: the seed
        px.extend(flat([210, 120, 20], 500)); // orange
        px.extend(flat([30, 170, 90], 300)); // green
        px.extend(flat([20, 20, 24], 800)); // near black: never an accent
        let x = Extracted::from_pixels(&px);
        assert!((250.0..280.0).contains(&hue(x.seed)), "blue seed, got {}", x.seed.hex());
        assert_eq!(x.others.len(), 2, "{:?}", x.others);
        for o in &x.others {
            assert!(hue_turn(hue(x.seed), hue(*o)).abs() >= MIN_HUE_GAP);
        }
        let hues: Vec<f64> = x.others.iter().map(|c| hue(*c)).collect();
        assert!(hues.iter().any(|h| (40.0..80.0).contains(h)), "orange among {hues:?}");
        assert!(hues.iter().any(|h| (130.0..170.0).contains(h)), "green among {hues:?}");
        // a one-colour picture has no companions
        assert!(Extracted::from_pixels(&flat([40, 90, 200], 400)).others.is_empty());
    }

    #[test]
    fn grey_black_and_white_pictures_fall_back_gracefully() {
        for rgb in [[128u8, 128, 128], [0, 0, 0], [255, 255, 255], [250, 248, 246], [12, 12, 12]] {
            let x = Extracted::from_pixels(&flat(rgb, 300));
            assert!(x.grey, "{rgb:?} has no colour to match");
            assert!(x.others.is_empty());
        }
        assert!(Extracted::from_pixels(&[]).grey);
        // a nearly black picture with a bit of real colour is not grey
        let x = Extracted::from_pixels(&flat([10, 14, 40], 300));
        assert!(!x.grey, "{}", x.seed.hex());
        assert!((240.0..280.0).contains(&hue(x.seed)));
        // a very bright pastel keeps its hue (and a high tone, which makes light/dark follow pick the light variant)
        let x = Extracted::from_pixels(&flat([255, 215, 225], 300));
        assert!(!x.grey && (hue(x.seed) < 12.0 || hue(x.seed) > 345.0), "{} hue {}", x.seed.hex(), hue(x.seed));
        assert!(x.tone > 85.0);
    }

    #[test]
    fn tone_tells_light_pictures_from_dark_ones() {
        let dark = Extracted::from_pixels(&flat([20, 30, 60], 200));
        let light = Extracted::from_pixels(&flat([225, 235, 250], 200));
        assert!(dark.tone < 20.0, "{}", dark.tone);
        assert!(light.tone > 85.0, "{}", light.tone);
        assert!((Extracted::from_pixels(&flat([128, 128, 128], 50)).tone - 54.0).abs() < 2.0, "mid grey is L* 54");
    }

    #[test]
    fn json_round_trip_keeps_everything() {
        let x = Extracted { seed: Rgb(1, 2, 3), others: vec![Rgb(4, 5, 6), Rgb(7, 8, 9)], tone: 41.3, grey: true };
        let mut o = Map::new();
        x.to_json_fields(&mut o);
        let back = Extracted::from_json_fields(&o).unwrap();
        assert_eq!((back.seed, back.others.clone(), back.grey), (x.seed, x.others.clone(), true));
        assert!((back.tone - 41.3).abs() < 0.06);
        assert!(Extracted::from_json_fields(&Map::new()).is_none());
    }

    #[test]
    fn the_cache_is_keyed_by_mtime_and_size() {
        let d = std::env::temp_dir().join(format!("decebal-extract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (img, cache) = (d.join("w.png"), d.join("cache/colors.json"));
        image::RgbImage::from_pixel(64, 40, image::Rgb([200, 40, 60])).save(&img).unwrap();
        let first = load(&img, Some(&cache)).unwrap();
        assert!(cache.exists());
        // a lie in the cache is believed while the file is unchanged ...
        let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&cache).unwrap()).unwrap();
        doc["entries"][0]["seed"] = "#00ff00".into();
        std::fs::write(&cache, doc.to_string()).unwrap();
        assert_eq!(load(&img, Some(&cache)).unwrap().seed, Rgb(0, 255, 0), "served from the cache, nothing decoded");
        // ... and discarded as soon as the picture changes
        image::RgbImage::from_pixel(64, 41, image::Rgb([40, 60, 200])).save(&img).unwrap();
        let second = load(&img, Some(&cache)).unwrap();
        assert!((240.0..285.0).contains(&hue(second.seed)), "{}", second.seed.hex());
        assert_ne!(first.seed, second.seed);
        // unreadable cache and missing files
        std::fs::write(&cache, "not json").unwrap();
        assert!(load(&img, Some(&cache)).is_ok());
        assert!(load(&d.join("missing.png"), Some(&cache)).is_err());
        std::fs::write(d.join("junk.png"), "not an image").unwrap();
        assert!(load(&d.join("junk.png"), None).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_cache_stays_small() {
        let d = std::env::temp_dir().join(format!("decebal-extract-small-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let cache = d.join("c.json");
        for i in 0..(CACHE_ENTRIES + 5) {
            let p = d.join(format!("{i}.png"));
            image::RgbImage::from_pixel(4, 4, image::Rgb([200, i as u8, 60])).save(&p).unwrap();
            load(&p, Some(&cache)).unwrap();
        }
        assert_eq!(read_cache(&cache).len(), CACHE_ENTRIES);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_wallpaper_is_read_in_milliseconds() {
        // a 4K picture with a gradient and a bright patch
        let img =
            image::RgbImage::from_fn(
                3840,
                2160,
                |x, y| if (1000..1600).contains(&x) && (600..1100).contains(&y) { image::Rgb([240, 160, 30]) } else { image::Rgb([(x / 20) as u8 / 2, 40, (y / 12) as u8]) },
            );
        let d = std::env::temp_dir().join(format!("decebal-extract-4k-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("4k.png");
        img.save(&p).unwrap();
        let t = std::time::Instant::now();
        let x = Extracted::from_image(&p).unwrap();
        let ms = t.elapsed().as_millis();
        assert!(!x.grey);
        // decoding dominates; the colour maths on the thumbnail is far below a millisecond
        let px: Vec<[u8; 3]> = (0..96 * 54).map(|i| [(i % 200) as u8, 40, 90]).collect();
        let t2 = std::time::Instant::now();
        let _ = Extracted::from_pixels(&px);
        assert!(t2.elapsed().as_millis() < 1000, "colour maths took {:?}", t2.elapsed());
        eprintln!("4K PNG decode + extract: {ms} ms");
        let _ = std::fs::remove_dir_all(&d);
    }
}
