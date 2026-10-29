//! Wikimedia Commons video discovery: explicit searches/downloads, no account or scraping.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

const API: &str = "https://commons.wikimedia.org/w/api.php";
const LIMIT: u64 = 512 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub page: String,
    pub thumb: String,
    pub bytes: u64,
    pub width: u64,
    pub height: u64,
    pub license: String,
    pub artist: String,
    pub preview: Option<PathBuf>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub media: Vec<Media>,
    pub next: Option<u64>,
}
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .timeout(Duration::from_secs(45))
        .redirects(0)
        .user_agent(concat!("Decebal/", env!("CARGO_PKG_VERSION"), " (live wallpaper picker; https://github.com/fspecii/macshell)"))
        .build()
}
fn api(params: &[(&str, String)]) -> Result<Value, String> {
    let mut req = agent().get(API).query("action", "query").query("format", "json").query("formatversion", "2");
    for (k, v) in params {
        req = req.query(k, v);
    }
    let response = req.call().map_err(|e| format!("Commons: {e}"))?;
    let mut bytes = Vec::new();
    response.into_reader().take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Commons response exceeds 2 MB".into());
    }
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if v.get("error").is_some() {
        return Err(format!("Commons: {}", v["error"]["info"].as_str().unwrap_or("API error")));
    }
    Ok(v)
}
/// Strip markup from attribution metadata; keep it as text, never execute provider HTML.
fn plain(s: &str) -> String {
    let mut tag = false;
    s.chars()
        .filter(|c| match c {
            '<' => {
                tag = true;
                false
            }
            '>' => {
                tag = false;
                false
            }
            _ => !tag,
        })
        .take(400)
        .collect::<String>()
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
pub fn trusted(url: &str) -> bool {
    (url.starts_with("https://upload.wikimedia.org/wikipedia/commons/") || url.starts_with("https://thumb.wikimedia.org/wikipedia/commons/")) && !url.chars().any(char::is_control)
}
pub fn parse(v: &Value) -> Page {
    let media = v["query"]["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let i = &p["videoinfo"][0];
            let url = i["url"].as_str()?;
            let mime = i["mime"].as_str().unwrap_or("");
            let bytes = i["size"].as_u64()?;
            let width = i["width"].as_u64()?;
            let height = i["height"].as_u64()?;
            if !url.starts_with("https://upload.wikimedia.org/wikipedia/commons/")
                || !trusted(url)
                || !matches!(mime, "video/webm" | "video/ogg")
                || bytes == 0
                || bytes > LIMIT
                || width == 0
                || height == 0
                || width > 7680
                || height > 4320
            {
                return None;
            }
            let meta = &i["extmetadata"];
            let license = plain(meta["LicenseShortName"]["value"].as_str().unwrap_or(""));
            if license.is_empty() {
                return None;
            }
            let page = i["descriptionurl"].as_str().filter(|s| s.starts_with("https://commons.wikimedia.org/wiki/File:"))?;
            Some(Media {
                id: p["pageid"].as_u64()?,
                title: plain(p["title"].as_str()?).trim_start_matches("File:").into(),
                url: url.into(),
                page: page.into(),
                thumb: i["thumburl"].as_str().filter(|u| trusted(u)).unwrap_or("").into(),
                bytes,
                width,
                height,
                license,
                artist: plain(meta["Artist"]["value"].as_str().unwrap_or("See source page")),
                preview: None,
            })
        })
        .collect();
    Page { media, next: v["continue"]["gsroffset"].as_u64() }
}
fn info_params() -> Vec<(&'static str, String)> {
    vec![("prop", "videoinfo".into()), ("viprop", "url|size|mime|extmetadata".into()), ("viurlwidth", "320".into()), ("viextmetadatafilter", "LicenseShortName|Artist".into())]
}
pub fn search(words: &str, offset: u64) -> Result<Page, String> {
    if words.len() > 200 {
        return Err("Search is limited to 200 characters".into());
    }
    let mut p = info_params();
    p.extend([
        ("generator", "search".into()),
        ("gsrsearch", format!("{} filetype:video", if words.trim().is_empty() { "landscape nature" } else { words.trim() })),
        ("gsrnamespace", "6".into()),
        ("gsrlimit", "12".into()),
        ("gsroffset", offset.to_string()),
    ]);
    let mut page = parse(&api(&p)?);
    // At most twelve small JPEG/PNG previews; they are loaded by the Settings image worker.
    std::fs::create_dir_all(super::cache()).map_err(|e| e.to_string())?;
    for m in &mut page.media {
        if m.thumb.is_empty() {
            continue;
        }
        let dest = super::cache().join(format!("commons-{}.jpg", m.id));
        if dest.is_file() || fetch(&m.thumb, &dest, 2 * 1024 * 1024).is_ok() {
            m.preview = Some(dest);
        }
    }
    Ok(page)
}
pub fn lookup(id: u64) -> Result<Media, String> {
    let mut p = info_params();
    p.push(("pageids", id.to_string()));
    parse(&api(&p)?).media.into_iter().next().ok_or("No supported, licensed Commons video found".into())
}
fn fetch(url: &str, dest: &std::path::Path, limit: u64) -> Result<(), String> {
    if !trusted(url) {
        return Err("Only Wikimedia Commons media URLs are accepted".into());
    }
    let response = agent().get(url).call().map_err(|e| e.to_string())?;
    if response.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).is_some_and(|n| n > limit) {
        return Err("Media exceeds download limit".into());
    }
    let part = dest.with_extension(format!("{}.part", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let n = std::io::copy(&mut response.into_reader().take(limit + 1), &mut file).map_err(|e| e.to_string())?;
        if n > limit || n == 0 {
            return Err("Empty or oversized media download".into());
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
pub fn download(m: &Media) -> Result<PathBuf, String> {
    std::fs::create_dir_all(super::library()).map_err(|e| e.to_string())?;
    let extension = if m.url.split('?').next().unwrap_or("").to_ascii_lowercase().ends_with(".ogv") { "ogv" } else { "webm" };
    let path = super::library().join(format!("commons-{}.{extension}", m.id));
    fetch(&m.url, &path, LIMIT)?;
    // Attribution travels with the video and stays available offline.
    crate::settings::io::write_atomic(&path.with_extension("credit.json"), &serde_json::to_string_pretty(m).map_err(|e| e.to_string())?)?;
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_untrusted_large_and_unlicensed_media() {
        let mut p = json!({"query":{"pages":[{"pageid":7,"title":"File:Cloud.webm","videoinfo":[{"url":"https://upload.wikimedia.org/wikipedia/commons/a/ab/Cloud.webm","descriptionurl":"https://commons.wikimedia.org/wiki/File:Cloud.webm","size":4000,"width":1920,"height":1080,"mime":"video/webm","extmetadata":{"LicenseShortName":{"value":"CC BY-SA 4.0"},"Artist":{"value":"<a>Artist</a>"}}}]}]}});
        assert_eq!(parse(&p).media[0].artist, "Artist");
        p["query"]["pages"][0]["videoinfo"][0]["size"] = json!(LIMIT + 1);
        assert!(parse(&p).media.is_empty());
        p["query"]["pages"][0]["videoinfo"][0]["size"] = json!(4000);
        p["query"]["pages"][0]["videoinfo"][0]["url"] = json!("https://example.com/evil.webm");
        assert!(parse(&p).media.is_empty());
        p["query"]["pages"][0]["videoinfo"][0]["url"] = json!("https://upload.wikimedia.org/wikipedia/commons/a/ab/Cloud.webm");
        p["query"]["pages"][0]["videoinfo"][0]["extmetadata"]["LicenseShortName"]["value"] = json!("");
        assert!(parse(&p).media.is_empty());
        assert!(!trusted("https://upload.wikimedia.org.evil/wikipedia/commons/a"));
        assert!(!trusted("file:///tmp/a"));
    }
}
