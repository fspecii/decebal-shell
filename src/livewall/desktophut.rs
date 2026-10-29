//! DesktopHut's public gallery and VideoObject metadata. Never executes provider scripts.
use super::catalog::{Media, Page};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const ROOT: &str = "https://www.desktophut.com";
const LIMIT: u64 = 512 * 1024 * 1024;
fn agent() -> ureq::Agent {
    agent_for(Duration::from_secs(45))
}
fn agent_for(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .timeout(timeout)
        .redirects(0)
        .user_agent(concat!("Decebal/", env!("CARGO_PKG_VERSION"), " live wallpaper picker"))
        .build()
}
fn safe_path(url: &str) -> Option<&str> {
    let path = url.strip_prefix(ROOT)?;
    (path.starts_with('/') && !path.contains(['\\', '%', '?', '#']) && !path.contains("..") && !path.chars().any(char::is_control)).then_some(path)
}
pub fn trusted_media(url: &str) -> bool {
    safe_path(url).is_some_and(|p| p.starts_with("/files/") && p.ends_with(".mp4"))
}
fn trusted_preview(url: &str) -> bool {
    safe_path(url).is_some_and(|p| p.starts_with("/previews/") && p.ends_with(".mp4"))
}
fn trusted_image(url: &str) -> bool {
    safe_path(url).is_some_and(|p| p.starts_with("/images/") && [".jpg", ".jpeg", ".png", ".webp"].iter().any(|ext| p.ends_with(ext)))
}
fn trusted_page(url: &str) -> bool {
    safe_path(url).is_some_and(|p| p.len() > 1 && p[1..].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}
fn plain(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#039;", "'").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").chars().filter(|c| !c.is_control()).take(300).collect()
}
// Small, bounded attribute reader for the site's server-rendered HTML, including either quote style.
fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && !b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'_') {
            i += 1;
        }
        let name = &tag[start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let quote = b[i];
        if quote != b'\'' && quote != b'"' {
            while i < b.len() && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            continue;
        }
        i += 1;
        let start = i;
        while i < b.len() && b[i] != quote {
            i += 1;
        }
        let value = &tag[start..i];
        i += usize::from(i < b.len());
        if name == key {
            return Some(value);
        }
    }
    None
}
fn elements<'a>(html: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    html.match_indices(&open)
        .filter_map(|(i, _)| {
            let rest = &html[i + open.len()..];
            if !rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '>') {
                return None;
            }
            let end = rest.find('>')?;
            let body = &rest[end + 1..];
            Some((&rest[..end], &body[..body.find(&close).unwrap_or(0)]))
        })
        .collect()
}
fn id(page: &str) -> u64 {
    page.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}
pub fn parse_gallery(html: &str, page: u64) -> Page {
    let mut media = Vec::new();
    for (tag, body) in elements(html, "a") {
        if !attr(tag, "class").is_some_and(|s| s.split_whitespace().any(|c| c == "wallpaper-card")) {
            continue;
        }
        let Some(href) = attr(tag, "href") else { continue };
        let url = if href.starts_with('/') { format!("{ROOT}{href}") } else { href.to_string() };
        if !trusted_page(&url) || media.iter().any(|m: &Media| m.page == url) {
            continue;
        }
        let thumb = elements(body, "img").first().and_then(|(t, _)| attr(t, "src")).filter(|u| trusted_image(u)).unwrap_or("");
        media.push(Media {
            id: id(&url),
            title: plain(attr(tag, "aria-label").or_else(|| attr(tag, "title")).unwrap_or("DesktopHut wallpaper")),
            url: String::new(),
            page: url,
            thumb: thumb.into(),
            bytes: 0,
            width: 0,
            height: 0,
            license: "See source terms".into(),
            artist: "DesktopHut contributor".into(),
            preview: None,
            motion: elements(body, "video").first().and_then(|(t, _)| attr(t, "data-src")).filter(|u| trusted_preview(u)).unwrap_or("").into(),
        });
        if media.len() == 48 {
            break;
        }
    }
    let has_next = ["link", "a"].iter().any(|name| elements(html, name).iter().any(|(t, _)| attr(t, "rel").is_some_and(|s| s.split_whitespace().any(|r| r == "next"))));
    Page { media, next: has_next.then_some(page + 1) }
}
fn video(v: &Value) -> Option<&Value> {
    if v["@type"] == "VideoObject" {
        return Some(v);
    }
    v.get("@graph").and_then(Value::as_array).and_then(|items| items.iter().find_map(video))
}
pub fn parse_detail(html: &str, original: &Media) -> Result<Media, String> {
    for (tag, body) in elements(html, "script") {
        if attr(tag, "type") != Some("application/ld+json") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(body) else { continue };
        let Some(v) = video(&v) else { continue };
        let Some(url) = v["contentUrl"].as_str().filter(|u| trusted_media(u)) else { continue };
        let mut m = original.clone();
        m.url = url.into();
        m.title = plain(v["name"].as_str().unwrap_or(&m.title));
        m.width = v["width"].as_u64().unwrap_or(0);
        m.height = v["height"].as_u64().unwrap_or(0);
        return Ok(m);
    }
    Err("DesktopHut: no supported direct MP4 found on this wallpaper page".into())
}
fn read_html(url: &str) -> Result<String, String> {
    let response = agent().get(url).call().map_err(|e| format!("DesktopHut: {e}"))?;
    let mut bytes = Vec::new();
    response.into_reader().take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("DesktopHut page exceeds 4 MB".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
fn encoded(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}
pub fn search(query: &str, offset: u64) -> Result<Page, String> {
    if query.len() > 200 {
        return Err("Search is limited to 200 characters".into());
    }
    let page = offset.max(1);
    let url = if query.trim().is_empty() { format!("{ROOT}/?page={page}") } else { format!("{ROOT}/search/{}?page={page}", encoded(query.trim())) };
    let mut result = parse_gallery(&read_html(&url)?, page);
    if result.media.is_empty() {
        return Err("DesktopHut returned no wallpapers. Try another search; the site may also be temporarily unavailable.".into());
    }
    std::fs::create_dir_all(super::cache()).map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    for m in result.media.iter_mut() {
        if std::time::Instant::now() >= deadline {
            break;
        }
        if m.thumb.is_empty() {
            continue;
        }
        let extension = m.thumb.rsplit('.').next().unwrap_or("jpg");
        let dest = super::cache().join(format!("desktophut-{}.{extension}", m.id));
        if dest.is_file() || fetch(&m.thumb, &dest, 2 * 1024 * 1024, false).is_ok() {
            m.preview = Some(dest);
        }
    }
    Ok(result)
}
fn fetch(url: &str, dest: &Path, limit: u64, mp4: bool) -> Result<(), String> {
    if !(if mp4 { trusted_media(url) || trusted_preview(url) } else { trusted_image(url) }) {
        return Err("Unsupported DesktopHut media URL".into());
    }
    let response = agent_for(Duration::from_secs(if mp4 { 45 } else { 5 })).get(url).call().map_err(|e| format!("DesktopHut: {e}"))?;
    if response.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).is_some_and(|n| n > limit) {
        return Err("DesktopHut file exceeds download limit".into());
    }
    let part = dest.with_extension(format!("{}.part", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let n = std::io::copy(&mut response.into_reader().take(limit + 1), &mut file).map_err(|e| e.to_string())?;
        if n == 0 || n > limit {
            return Err("Empty or oversized DesktopHut download".into());
        }
        file.flush().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&part, dest).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
}
pub fn preview(m: &Media) -> Result<PathBuf, String> {
    if !trusted_preview(&m.motion) {
        return Err("This wallpaper has no supported video preview".into());
    }
    std::fs::create_dir_all(super::cache()).map_err(|e| e.to_string())?;
    let path = super::cache().join(format!("desktophut-preview-{}.mp4", m.id));
    if !path.is_file() {
        fetch(&m.motion, &path, 16 * 1024 * 1024, true)?;
    }
    Ok(path)
}
pub fn download_page(page: &str) -> Result<PathBuf, String> {
    let m = Media {
        id: id(page),
        title: "DesktopHut wallpaper".into(),
        page: page.into(),
        url: String::new(),
        thumb: String::new(),
        bytes: 0,
        width: 0,
        height: 0,
        license: "See source terms".into(),
        artist: "DesktopHut contributor".into(),
        preview: None,
        motion: String::new(),
    };
    download(&m)
}
pub fn download(m: &Media) -> Result<PathBuf, String> {
    if !trusted_page(&m.page) {
        return Err("Invalid DesktopHut source page".into());
    }
    let mut media = parse_detail(&read_html(&m.page)?, m)?;
    std::fs::create_dir_all(super::library()).map_err(|e| e.to_string())?;
    let path = super::library().join(format!("desktophut-{}.mp4", id(&media.page)));
    fetch(&media.url, &path, LIMIT, true)?;
    media.bytes = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    crate::settings::io::write_atomic(&path.with_extension("credit.json"), &serde_json::to_string_pretty(&media).map_err(|e| e.to_string())?)?;
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    const GALLERY: &str = r#"<link rel='next' href='/?page=2'><a href='/clouds' class='wallpaper-card' aria-label='Clouds &amp; sky'><img src='https://www.desktophut.com/images/clouds.webp'><video data-src='https://www.desktophut.com/previews/clouds.mp4'></video></a><a class='wallpaper-card' href='https://evil.com/clouds'></a><a class='wallpaper-card' href='/clouds'></a>"#;
    #[test]
    fn gallery_has_safe_deduplicated_results_and_next_page() {
        let p = parse_gallery(GALLERY, 1);
        assert_eq!(p.media.len(), 1);
        assert_eq!(p.media[0].title, "Clouds & sky");
        assert_eq!(p.next, Some(2));
        assert_eq!(parse_gallery("<a rel='next' href='?page=2'>Next</a>", 1).next, Some(2));
        let large_page = (0..26).map(|n| format!("<a class=\"wallpaper-card\" href=\"/wallpaper-{n}\"></a>")).collect::<String>();
        assert_eq!(parse_gallery(&large_page, 1).media.len(), 26, "preview limit must not discard the rest of a page");
        assert!(!p.media[0].thumb.is_empty());
        assert_eq!(p.media[0].motion, "https://www.desktophut.com/previews/clouds.mp4");
        assert!(!trusted_preview("https://evil.com/previews/clouds.mp4"));
        assert_eq!(parse_gallery("<a href='/x'>not a wallpaper</a>", 1).media.len(), 0);
    }
    #[test]
    fn detail_uses_original_not_preview_video_and_rejects_external_media() {
        let m = &parse_gallery(GALLERY, 1).media[0];
        let s = r#"<script type='application/ld+json'>{"@graph":[{"@type":"VideoObject","name":"Clouds","contentUrl":"https://www.desktophut.com/files/clouds.mp4"}]}</script><source src='/previews/clouds.mp4'>"#;
        assert_eq!(parse_detail(s, m).unwrap().url, "https://www.desktophut.com/files/clouds.mp4");
        assert!(parse_detail(&s.replace("www.desktophut.com/files", "evil.com/files"), m).is_err());
    }
    #[test]
    fn url_boundaries_and_query_encoding() {
        for url in [
            "https://www.desktophut.com.evil/files/a.mp4",
            "https://www.desktophut.com@evil.com/files/a.mp4",
            "https://www.desktophut.com/files/../a.mp4",
            "https://www.desktophut.com/files/%2e/a.mp4",
            "file:///tmp/a.mp4",
        ] {
            assert!(!trusted_media(url), "{url}");
        }
        assert_eq!(encoded("cloud & ocean/夜"), "cloud%20%26%20ocean%2F%E5%A4%9C");
    }
}
