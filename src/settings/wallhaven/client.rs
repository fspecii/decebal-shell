//! Blocking HTTP for the Wallhaven pane. Only ever called from worker threads (the UI never waits on it).
//!
//! * the API (`/search`, `/w/<id>`) goes through the shared [`Throttle`]; a 429 starts a cool-down and nothing is
//!   retried on its own;
//! * the API key is sent as the `X-API-Key` header to the API host only, never in a URL, never to image hosts;
//! * thumbnails and wallpapers are fetched only from wallhaven's image hosts (or, for tests, the host of
//!   `WALLHAVEN_API_BASE`), with content-type, size and magic-byte checks, written atomically.

use super::parse::{self, Detail, Page, Wall};
use super::query::{purity_allowed, Query};
use super::store::Secret;
use super::throttle::{Throttle, Verdict, MAX_WAIT};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_BASE: &str = "https://wallhaven.cc/api/v1";
pub const USER_AGENT: &str = concat!("macshell-macsettings/", env!("CARGO_PKG_VERSION"), " (wallhaven wallpaper picker)");

/// Largest wallpaper accepted.
pub const MAX_IMAGE_BYTES: u64 = 40 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_THUMB_NET_BYTES: u64 = 2 * 1024 * 1024;
/// Images that would decode to more pixels than this are refused.
const MAX_PIXELS: u64 = 120_000_000;
const DOWNLOAD_DEADLINE: Duration = Duration::from_secs(600);

/// `WALLHAVEN_API_BASE` (tests, mirrors) or the real API.
pub fn api_base() -> String {
    std::env::var("WALLHAVEN_API_BASE")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// No connection, DNS failure, timeout.
    Offline(String),
    /// 429 from the server or our own limiter; seconds until it is worth trying again.
    RateLimited(u64),
    /// 401 / 403: missing or wrong API key.
    Unauthorized,
    NotFound,
    Http(u16),
    /// Unexpected or malformed answer.
    Bad(String),
    TooLarge,
    WrongType(String),
    /// Not allowed by the purity settings or not a wallhaven address.
    Refused(String),
    Disk(String),
}

impl NetError {
    /// What the pane shows.
    pub fn friendly(&self) -> String {
        match self {
            NetError::Offline(_) => "Can't reach wallhaven.cc. Check your connection; showing what was loaded before.".into(),
            NetError::RateLimited(s) => format!("Wallhaven is asking for a pause (it allows 45 requests a minute). Try again in {s} s."),
            NetError::Unauthorized => "Wallhaven did not accept the API key. Check it in the account section.".into(),
            NetError::NotFound => "Wallhaven does not have that wallpaper (any more).".into(),
            NetError::Http(c) => format!("Wallhaven answered with error {c}. Try again in a moment."),
            NetError::Bad(m) => format!("Unexpected answer from wallhaven ({m})."),
            NetError::TooLarge => format!("That image is larger than {} MB; not downloaded.", MAX_IMAGE_BYTES / 1024 / 1024),
            NetError::WrongType(t) => format!("Not a JPEG, PNG or WebP image ({t}); not downloaded."),
            NetError::Refused(m) => m.clone(),
            NetError::Disk(m) => format!("Could not save the wallpaper: {m}"),
        }
    }
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.friendly())
    }
}

/// `scheme://host[:port]` of an http(s) URL, without userinfo tricks.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() || host.contains(['@', '\\', ' ']) {
        return None;
    }
    Some(format!("{scheme}://{}", host.to_ascii_lowercase()))
}

/// Image / thumbnail addresses we will fetch: wallhaven's hosts, or the host of an overridden API base.
pub fn allowed_media_url(url: &str, base: &str) -> bool {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let Some(o) = origin(url) else { return false };
    if o == "https://w.wallhaven.cc" || o == "https://th.wallhaven.cc" {
        return true;
    }
    base != DEFAULT_BASE && origin(base).is_some_and(|b| b == o)
}

/// `image/jpeg; charset=x` -> `jpg`. Only the three types wallhaven serves are accepted.
pub fn ext_for_content_type(ct: &str) -> Option<&'static str> {
    match ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// What the first bytes say the file is.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Cheap end-of-file check per format, so a download cut off at a chunk boundary is not kept.
pub fn looks_complete(kind: &str, head: &[u8], tail: &[u8], total: u64) -> bool {
    match kind {
        "png" => tail.ends_with(&[b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]),
        "jpg" => {
            let t: &[u8] = tail;
            let end = t.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
            t[..end].ends_with(&[0xFF, 0xD9])
        }
        "webp" => head.len() >= 8 && u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]])) + 8 <= total,
        _ => false,
    }
}

/// Fixed shape so a hostile id can never become a path: `wallhaven-<id>.<ext>`.
pub fn file_name(id: &str, ext: &str) -> Option<String> {
    (parse::valid_id(id) && matches!(ext, "jpg" | "png" | "webp")).then(|| format!("wallhaven-{id}.{ext}"))
}

/// Checks a declared size before any bytes are fetched.
pub fn check_declared_size(len: Option<u64>) -> Result<(), NetError> {
    match len {
        Some(n) if n > MAX_IMAGE_BYTES => Err(NetError::TooLarge),
        Some(0) => Err(NetError::Bad("empty image".into())),
        _ => Ok(()),
    }
}

fn retry_after(resp: &ureq::Response) -> Option<Duration> {
    resp.header("retry-after").and_then(|v| v.trim().parse::<u64>().ok()).map(Duration::from_secs)
}

fn map_err(e: ureq::Error, throttle: Option<&Mutex<Throttle>>) -> NetError {
    match e {
        ureq::Error::Status(429, resp) => {
            let wait = retry_after(&resp);
            // only the API has a cool-down; the image hosts have no such limit
            if let Some(Ok(mut t)) = throttle.map(|t| t.lock()) {
                t.penalise(Instant::now(), wait);
            }
            NetError::RateLimited(wait.map_or(60, |d| d.as_secs().clamp(1, 600)))
        }
        ureq::Error::Status(401 | 403, _) => NetError::Unauthorized,
        ureq::Error::Status(404, _) => NetError::NotFound,
        ureq::Error::Status(c, _) => NetError::Http(c),
        ureq::Error::Transport(t) => NetError::Offline(t.to_string()),
    }
}

/// Progress of a download, shared with the UI.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl Progress {
    pub fn fraction(&self) -> Option<f32> {
        let t = self.total.load(Ordering::Relaxed);
        (t > 0).then(|| (self.done.load(Ordering::Relaxed) as f32 / t as f32).clamp(0.0, 1.0))
    }
}

pub struct Client {
    base: String,
    key: Option<Secret>,
    api: ureq::Agent,
    media: ureq::Agent,
    throttle: Arc<Mutex<Throttle>>,
}

impl Client {
    pub fn new(base: String, key: Option<Secret>, throttle: Arc<Mutex<Throttle>>) -> Client {
        // no redirects on the API agent: the key header must not follow a redirect to another host
        let api = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout(Duration::from_secs(20)).redirects(0).user_agent(USER_AGENT).build();
        let media = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout_read(Duration::from_secs(20)).redirects(2).user_agent(USER_AGENT).build();
        Client { base, key, api, media, throttle }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// Waits for a slot (briefly) or says why not.
    fn slot(&self) -> Result<(), NetError> {
        loop {
            let v = self.throttle.lock().map_err(|_| NetError::Bad("lock".into()))?.check(Instant::now());
            match v {
                Verdict::Go => return Ok(()),
                Verdict::Wait(d) if d <= MAX_WAIT => std::thread::sleep(d + Duration::from_millis(20)),
                Verdict::Wait(d) | Verdict::Blocked(d) => return Err(NetError::RateLimited(d.as_secs() + 1)),
            }
        }
    }

    fn api_get(&self, url: &str) -> Result<String, NetError> {
        self.slot()?;
        let mut req = self.api.get(url).set("Accept", "application/json");
        if let Some(k) = &self.key {
            req = req.set("X-API-Key", k.expose());
        }
        let resp = req.call().map_err(|e| map_err(e, Some(&self.throttle)))?;
        let mut body = String::new();
        resp.into_reader().take(MAX_JSON_BYTES).read_to_string(&mut body).map_err(|e| NetError::Offline(e.to_string()))?;
        Ok(body)
    }

    /// One page of results. Anything the purity settings do not allow is dropped even if the server sent it.
    pub fn search(&self, q: &Query, page: u32) -> Result<Page, NetError> {
        let body = self.api_get(&q.url(&self.base, page))?;
        let mut p = parse::parse_search(&body, page).map_err(NetError::Bad)?;
        p.walls.retain(|w| purity_allowed(&w.purity, q.purity));
        Ok(p)
    }

    pub fn detail(&self, id: &str) -> Result<(Wall, Detail), NetError> {
        if !parse::valid_id(id) {
            return Err(NetError::Refused("Not a wallhaven id".into()));
        }
        let body = self.api_get(&format!("{}/w/{id}", self.base))?;
        parse::parse_detail(&body).map_err(NetError::Bad)
    }

    /// A thumbnail's bytes (not counted against the API limit: they come from the image host).
    pub fn thumb(&self, url: &str) -> Result<Vec<u8>, NetError> {
        if !allowed_media_url(url, &self.base) {
            return Err(NetError::Refused("Not a wallhaven image address".into()));
        }
        let resp = self.media.get(url).call().map_err(|e| map_err(e, None))?;
        if !resp.content_type().starts_with("image/") {
            return Err(NetError::WrongType(resp.content_type().to_string()));
        }
        let mut buf = Vec::new();
        resp.into_reader().take(MAX_THUMB_NET_BYTES + 1).read_to_end(&mut buf).map_err(|e| NetError::Offline(e.to_string()))?;
        if buf.len() as u64 > MAX_THUMB_NET_BYTES {
            return Err(NetError::TooLarge);
        }
        if sniff(&buf).is_none() {
            return Err(NetError::WrongType("unknown format".into()));
        }
        Ok(buf)
    }

    /// Downloads `wall` into `dir` as `wallhaven-<id>.<ext>` and returns the path. Already downloaded: returned as is.
    pub fn download(&self, wall: &Wall, dir: &Path, bits: [bool; 3], progress: &Progress) -> Result<PathBuf, NetError> {
        if !parse::valid_id(&wall.id) {
            return Err(NetError::Refused("Not a wallhaven id".into()));
        }
        if !purity_allowed(&wall.purity, bits) {
            return Err(NetError::Refused("That wallpaper is not allowed by the content settings (sketchy / NSFW need the switch and an API key).".into()));
        }
        if !allowed_media_url(&wall.full, &self.base) {
            return Err(NetError::Refused("Not a wallhaven image address".into()));
        }
        std::fs::create_dir_all(dir).map_err(|e| NetError::Disk(format!("{}: {e}", dir.display())))?;
        // already here (any of the three extensions)?
        for ext in ["jpg", "png", "webp"] {
            if let Some(name) = file_name(&wall.id, ext) {
                let p = dir.join(name);
                if std::fs::metadata(&p).map(|m| m.is_file() && m.len() > 0).unwrap_or(false) {
                    progress.total.store(1, Ordering::Relaxed);
                    progress.done.store(1, Ordering::Relaxed);
                    return Ok(p);
                }
            }
        }
        let resp = self.media.get(&wall.full).call().map_err(|e| map_err(e, None))?;
        let ext = ext_for_content_type(resp.content_type()).ok_or_else(|| NetError::WrongType(resp.content_type().to_string()))?;
        let declared = resp.header("content-length").and_then(|v| v.trim().parse::<u64>().ok());
        check_declared_size(declared)?;
        progress.total.store(declared.unwrap_or(wall.file_size), Ordering::Relaxed);
        progress.done.store(0, Ordering::Relaxed);
        let name = file_name(&wall.id, ext).ok_or_else(|| NetError::Refused("Not a wallhaven id".into()))?;
        let target = dir.join(&name);
        let part = dir.join(format!(".{name}.part"));
        let res = stream_to(resp.into_reader(), &part, declared, ext, progress);
        match res {
            Ok(()) => {
                if let Err(e) = std::fs::rename(&part, &target) {
                    let _ = std::fs::remove_file(&part);
                    return Err(NetError::Disk(e.to_string()));
                }
                Ok(target)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
        }
    }
}

/// Copies at most [`MAX_IMAGE_BYTES`] into `part`, then checks the file really is the claimed kind of image.
fn stream_to(mut r: impl Read, part: &Path, declared: Option<u64>, ext: &str, progress: &Progress) -> Result<(), NetError> {
    let mut out = std::fs::File::create(part).map_err(|e| NetError::Disk(format!("{}: {e}", part.display())))?;
    let started = Instant::now();
    let (mut buf, mut total, mut head, mut tail) = (vec![0u8; 64 * 1024], 0u64, Vec::new(), Vec::<u8>::new());
    loop {
        let n = r.read(&mut buf).map_err(|e| NetError::Offline(e.to_string()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_IMAGE_BYTES {
            return Err(NetError::TooLarge);
        }
        if started.elapsed() > DOWNLOAD_DEADLINE {
            return Err(NetError::Offline("timed out".into()));
        }
        if head.len() < 16 {
            head.extend_from_slice(&buf[..n.min(16 - head.len())]);
        }
        tail.extend_from_slice(&buf[..n]);
        if tail.len() > 64 {
            tail.drain(..tail.len() - 64);
        }
        out.write_all(&buf[..n]).map_err(|e| NetError::Disk(e.to_string()))?;
        progress.done.store(total, Ordering::Relaxed);
    }
    out.sync_all().map_err(|e| NetError::Disk(e.to_string()))?;
    drop(out);
    if total == 0 {
        return Err(NetError::Bad("empty download".into()));
    }
    if declared.is_some_and(|d| d != total) {
        return Err(NetError::Bad("download was cut short".into()));
    }
    match sniff(&head) {
        Some(kind) if kind == ext => {}
        Some(kind) => return Err(NetError::WrongType(format!("says {ext}, is {kind}"))),
        None => return Err(NetError::WrongType("not an image".into())),
    }
    if !looks_complete(ext, &head, &tail, total) {
        return Err(NetError::Bad("download was cut short".into()));
    }
    let (w, h) = image::ImageReader::open(part)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| NetError::Disk(e.to_string()))?
        .into_dimensions()
        .map_err(|e| NetError::Bad(format!("unreadable image: {e}")))?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(NetError::Bad("unreasonable image size".into()));
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::super::parse::parse_search;
    use super::super::parse::tests::{DETAIL, SEARCH};
    use super::super::query::{Prefs, Query};
    use super::*;
    use std::collections::HashMap;
    use std::net::TcpListener;

    pub type Headers = HashMap<String, String>;
    pub type Reply = (u16, Vec<(&'static str, String)>, Vec<u8>);

    /// A throw-away HTTP server on 127.0.0.1. Returns its base URL and the log of `(path, headers)` seen.
    pub type Log = Arc<Mutex<Vec<(String, Headers)>>>;

    pub fn serve(handler: impl Fn(&str, &Headers) -> Reply + Send + 'static) -> (String, Log) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log: Log = Arc::default();
        let l2 = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut reader = std::io::BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                use std::io::BufRead;
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut headers = Headers::new();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                    }
                }
                l2.lock().unwrap().push((path.clone(), headers.clone()));
                let (code, hdrs, body) = handler(&path, &headers);
                let mut head = format!("HTTP/1.1 {code} X\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
                for (k, v) in hdrs {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        (base, log)
    }

    pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 200, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    fn client(base: &str, key: Option<&str>) -> Client {
        Client::new(base.to_string(), key.and_then(Secret::new), Arc::default())
    }

    fn json(body: &str) -> Reply {
        (200, vec![("Content-Type", "application/json".into())], body.as_bytes().to_vec())
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wh-client-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn wall_at(base: &str, id: &str) -> Wall {
        let mut w = parse_search(SEARCH, 1).unwrap().walls[0].clone();
        w.id = id.into();
        w.full = format!("{base}/full/{id}");
        w.purity = "sfw".into();
        w
    }

    #[test]
    fn search_sends_agent_and_key_header_never_the_key_in_the_url() {
        let (base, log) = serve(|_, _| json(SEARCH));
        let key = "AbCdEfGh12345678AbCdEfGh12345678";
        let c = client(&base, Some(key));
        let q = Query::from_prefs(&Prefs::default(), true, (1600, 900));
        let p = c.search(&q, 2).unwrap();
        assert_eq!(p.walls.len(), 6);
        let log = log.lock().unwrap();
        let (path, h) = &log[0];
        assert!(path.starts_with("/search?categories=111&purity=100&sorting=toplist&order=desc&topRange=1M&atleast=1600x900&page=2"), "{path}");
        assert!(!path.contains(key) && !path.contains("apikey"));
        assert_eq!(h.get("x-api-key").map(String::as_str), Some(key));
        assert!(h.get("user-agent").unwrap().starts_with("macshell-macsettings/"), "{h:?}");
        assert!(!USER_AGENT.contains("ureq"));
    }

    #[test]
    fn no_key_means_no_header() {
        let (base, log) = serve(|_, _| json(SEARCH));
        client(&base, None).search(&Query::from_prefs(&Prefs::default(), false, (0, 0)), 1).unwrap();
        assert!(!log.lock().unwrap()[0].1.contains_key("x-api-key"));
    }

    #[test]
    fn results_the_purity_setting_does_not_allow_are_dropped() {
        let mixed = SEARCH.replacen("\"purity\": \"sfw\"", "\"purity\": \"nsfw\"", 1);
        let (base, _) = serve(move |_, _| json(&mixed));
        let q = Query::from_prefs(&Prefs::default(), false, (0, 0));
        let p = client(&base, None).search(&q, 1).unwrap();
        assert_eq!(p.walls.len(), 5, "the nsfw item the server should not have sent is dropped");
        assert!(p.walls.iter().all(|w| w.purity == "sfw"));
    }

    #[test]
    fn detail_hits_the_w_endpoint() {
        let (base, log) = serve(|_, _| json(DETAIL));
        let (w, d) = client(&base, None).detail("d8vvlo").unwrap();
        assert_eq!(w.id, "d8vvlo");
        assert!(d.tags.len() >= 3);
        assert_eq!(log.lock().unwrap()[0].0, "/w/d8vvlo");
        assert!(client(&base, None).detail("../etc").is_err());
    }

    #[test]
    fn a_429_starts_a_cooldown_and_nothing_else_is_sent() {
        let (base, log) = serve(|_, _| (429, vec![("Retry-After", "30".into())], b"{\"error\":\"slow down\"}".to_vec()));
        let c = client(&base, None);
        let q = Query::from_prefs(&Prefs::default(), false, (0, 0));
        assert_eq!(c.search(&q, 1), Err(NetError::RateLimited(30)));
        // the next calls are refused locally: no retry storm
        for _ in 0..5 {
            assert!(matches!(c.search(&q, 1), Err(NetError::RateLimited(s)) if s <= 31));
        }
        assert_eq!(log.lock().unwrap().len(), 1, "only one request ever reached the server");
        assert!(NetError::RateLimited(30).friendly().contains("30 s"));
    }

    #[test]
    fn our_own_limiter_refuses_instead_of_blocking_for_long() {
        let (base, log) = serve(|_, _| json(SEARCH));
        let c = Client::new(base, None, Arc::new(Mutex::new(Throttle::new(2, Duration::from_secs(60)))));
        let q = Query::from_prefs(&Prefs::default(), false, (0, 0));
        assert!(c.search(&q, 1).is_ok() && c.search(&q, 1).is_ok());
        let t = Instant::now();
        assert!(matches!(c.search(&q, 1), Err(NetError::RateLimited(_))));
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[test]
    fn http_errors_map_to_friendly_ones() {
        for (code, want) in [(401u16, NetError::Unauthorized), (403, NetError::Unauthorized), (404, NetError::NotFound), (500, NetError::Http(500))] {
            let (base, _) = serve(move |_, _| (code, vec![], vec![]));
            assert_eq!(client(&base, None).detail("abc"), Err(want));
        }
        let (base, _) = serve(|_, _| json("<html>oops</html>"));
        assert!(matches!(client(&base, None).detail("abc"), Err(NetError::Bad(_))));
    }

    #[test]
    fn offline_is_an_error_not_a_hang() {
        // a port nothing listens on
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        let t = Instant::now();
        let e = client(&base, None).search(&Query::from_prefs(&Prefs::default(), false, (0, 0)), 1).unwrap_err();
        assert!(matches!(e, NetError::Offline(_)), "{e:?}");
        assert!(t.elapsed() < Duration::from_secs(10));
        assert!(e.friendly().contains("Can't reach"));
    }

    #[test]
    fn media_addresses_are_restricted() {
        let d = DEFAULT_BASE;
        assert!(allowed_media_url("https://w.wallhaven.cc/full/d8/wallhaven-d8vvlo.png", d));
        assert!(allowed_media_url("https://th.wallhaven.cc/small/d8/d8vvlo.jpg", d));
        assert!(!allowed_media_url("http://w.wallhaven.cc/full/x.png", d), "plain http to the real host");
        assert!(!allowed_media_url("https://w.wallhaven.cc.evil.example/x.png", d));
        assert!(!allowed_media_url("https://evil.example/https://w.wallhaven.cc/x.png", d));
        assert!(!allowed_media_url("https://w.wallhaven.cc@evil.example/x.png", d));
        assert!(!allowed_media_url("https://w.wallhaven.cc/a b.png", d));
        assert!(!allowed_media_url("file:///etc/passwd", d));
        assert!(!allowed_media_url("http://127.0.0.1:1/x.png", d), "loopback is only allowed when the API base is overridden");
        assert!(allowed_media_url("http://127.0.0.1:8123/full/x.png", "http://127.0.0.1:8123/api/v1"));
        assert!(!allowed_media_url("http://127.0.0.1:9999/full/x.png", "http://127.0.0.1:8123/api/v1"));
    }

    #[test]
    fn content_types_sniffing_and_names() {
        assert_eq!(ext_for_content_type("image/jpeg"), Some("jpg"));
        assert_eq!(ext_for_content_type("IMAGE/PNG; charset=binary"), Some("png"));
        assert_eq!(ext_for_content_type("image/webp"), Some("webp"));
        for bad in ["image/gif", "image/svg+xml", "text/html", "application/octet-stream", ""] {
            assert_eq!(ext_for_content_type(bad), None, "{bad}");
        }
        assert_eq!(sniff(&png_bytes(2, 2)), Some("png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some("jpg"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"GIF89a......"), None);
        assert_eq!(sniff(b"<svg xmlns"), None);
        assert_eq!(file_name("d8vvlo", "png").as_deref(), Some("wallhaven-d8vvlo.png"));
        assert_eq!(file_name("../x", "png"), None);
        assert_eq!(file_name("abc", "sh"), None);
    }

    #[test]
    fn completeness_check_per_format() {
        let png = png_bytes(8, 8);
        assert!(looks_complete("png", &png[..16], &png, png.len() as u64));
        assert!(!looks_complete("png", &png[..16], &png[..png.len() - 5], 100));
        assert!(looks_complete("jpg", &[0xFF, 0xD8, 0xFF], &[1, 2, 0xFF, 0xD9, 0, 0], 10), "trailing zero padding is fine");
        assert!(!looks_complete("jpg", &[0xFF, 0xD8, 0xFF], &[1, 2, 3, 4], 10));
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&(92u32).to_le_bytes());
        webp.extend_from_slice(b"WEBPVP8 ");
        assert!(looks_complete("webp", &webp, &[], 100));
        assert!(!looks_complete("webp", &webp, &[], 50), "RIFF length says more than arrived");
    }

    #[test]
    fn size_limit_is_forty_megabytes() {
        assert!(check_declared_size(Some(MAX_IMAGE_BYTES)).is_ok());
        assert_eq!(check_declared_size(Some(MAX_IMAGE_BYTES + 1)), Err(NetError::TooLarge));
        assert!(check_declared_size(None).is_ok());
        assert!(check_declared_size(Some(0)).is_err());
        assert_eq!(MAX_IMAGE_BYTES, 40 * 1024 * 1024);
    }

    #[test]
    fn download_writes_atomically_with_a_safe_name_and_reports_progress() {
        let png = png_bytes(64, 36);
        let p2 = png.clone();
        let (base, _) = serve(move |_, _| (200, vec![("Content-Type", "image/png".into())], p2.clone()));
        let dir = tmp("dl");
        let prog = Progress::default();
        let path = client(&base, None).download(&wall_at(&base, "abc123"), &dir, [true, false, false], &prog).unwrap();
        assert_eq!(path, dir.join("wallhaven-abc123.png"), "the extension comes from the content type");
        assert_eq!(std::fs::read(&path).unwrap(), png);
        assert_eq!(prog.fraction(), Some(1.0));
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(leftovers, ["wallhaven-abc123.png"], "no .part file stays behind");
        // second time: no request at all
        let (base2, log2) = serve(|_, _| (500, vec![], vec![]));
        let again = client(&base2, None).download(&wall_at(&base2, "abc123"), &dir, [true, false, false], &prog).unwrap();
        assert_eq!(again, path);
        assert!(log2.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_refuses_wrong_types_fakes_and_oversize() {
        let dir = tmp("dlbad");
        let png = png_bytes(8, 8);
        let cases: Vec<(&str, Reply)> = vec![
            ("html", (200, vec![("Content-Type", "text/html".into())], b"<html>".to_vec())),
            ("gif", (200, vec![("Content-Type", "image/gif".into())], b"GIF89a".to_vec())),
            ("liar", (200, vec![("Content-Type", "image/png".into())], b"#!/bin/sh\necho hi\n".to_vec())),
            ("mismatch", (200, vec![("Content-Type", "image/jpeg".into())], png.clone())),
            ("empty", (200, vec![("Content-Type", "image/png".into())], vec![])),
            ("truncated", (200, vec![("Content-Type", "image/png".into())], png[..png.len() / 2].to_vec())),
            ("huge", (200, vec![("Content-Type", "image/png".into()), ("X-Pad", "1".into())], vec![])),
        ];
        for (id, reply) in cases {
            let r = reply.clone();
            let (base, _) = serve(move |_, _| r.clone());
            let res = client(&base, None).download(&wall_at(&base, id), &dir, [true; 3], &Progress::default());
            assert!(res.is_err(), "{id} should be refused: {res:?}");
        }
        let files: Vec<_> = std::fs::read_dir(&dir).map(|rd| rd.flatten().collect()).unwrap_or_default();
        assert!(files.is_empty(), "nothing was left on disk: {files:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_refuses_a_declared_size_over_the_limit_without_fetching_the_body() {
        // a server that claims 41 MB but would send 10 bytes: the Content-Length alone is enough to refuse
        let dir = tmp("dlbig");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n0123456789", MAX_IMAGE_BYTES + 1).as_bytes());
            }
        });
        let res = client(&base, None).download(&wall_at(&base, "bigone"), &dir, [true; 3], &Progress::default());
        assert_eq!(res, Err(NetError::TooLarge));
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_respects_purity_and_addresses() {
        let dir = tmp("dlpurity");
        let (base, log) = serve(|_, _| (200, vec![("Content-Type", "image/png".into())], png_bytes(4, 4)));
        let mut w = wall_at(&base, "sketch1");
        w.purity = "sketchy".into();
        let c = client(&base, None);
        assert!(matches!(c.download(&w, &dir, [true, false, false], &Progress::default()), Err(NetError::Refused(_))));
        let mut far = wall_at(&base, "far1");
        far.full = "https://evil.example/x.png".into();
        assert!(matches!(c.download(&far, &dir, [true; 3], &Progress::default()), Err(NetError::Refused(_))));
        let mut bad = wall_at(&base, "x");
        bad.id = "../../x".into();
        assert!(matches!(c.download(&bad, &dir, [true; 3], &Progress::default()), Err(NetError::Refused(_))));
        assert!(log.lock().unwrap().is_empty(), "refused before any request");
        // allowed once the sketchy bit is on
        assert!(c.download(&w, &dir, [true, true, false], &Progress::default()).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn thumb_fetch_checks_type_and_size() {
        let png = png_bytes(20, 12);
        let (base, log) = serve(move |path, _| match path {
            "/t/ok" => (200, vec![("Content-Type", "image/png".into())], png.clone()),
            "/t/html" => (200, vec![("Content-Type", "text/html".into())], b"<html>".to_vec()),
            "/t/fake" => (200, vec![("Content-Type", "image/png".into())], b"not really".to_vec()),
            _ => (404, vec![], vec![]),
        });
        let c = client(&base, Some("AbCdEfGh12345678AbCdEfGh12345678"));
        assert!(c.thumb(&format!("{base}/t/ok")).is_ok());
        assert!(c.thumb(&format!("{base}/t/html")).is_err());
        assert!(c.thumb(&format!("{base}/t/fake")).is_err());
        assert_eq!(c.thumb(&format!("{base}/t/missing")), Err(NetError::NotFound));
        assert!(c.thumb("https://evil.example/t.png").is_err());
        assert!(log.lock().unwrap().iter().all(|(_, h)| !h.contains_key("x-api-key")), "the key goes to the API only, not to image requests");
    }

    #[test]
    fn friendly_messages_exist_for_every_error() {
        for e in [
            NetError::Offline("x".into()),
            NetError::RateLimited(5),
            NetError::Unauthorized,
            NetError::NotFound,
            NetError::Http(502),
            NetError::Bad("x".into()),
            NetError::TooLarge,
            NetError::WrongType("text/html".into()),
            NetError::Refused("no".into()),
            NetError::Disk("full".into()),
        ] {
            assert!(!e.friendly().is_empty() && !e.to_string().contains("ureq"));
        }
    }
}
