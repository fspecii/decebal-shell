//! Parsing of wallhaven's JSON (`/search` and `/w/<id>`). Strict about what it keeps: ids, colours and
//! sizes are validated, anything odd is dropped, and the uploader (present in `/w/<id>`) is ignored.

use serde::{Deserialize, Serialize};

/// One wallpaper. Also the on-disk form of the favourites list and of the last-search cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wall {
    pub id: String,
    /// Page on wallhaven.cc.
    pub url: String,
    pub thumb_small: String,
    pub thumb_large: String,
    /// Full image.
    pub full: String,
    pub resolution: String,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub file_type: String,
    pub purity: String,
    pub category: String,
    pub views: u64,
    pub favorites: u64,
    /// `#rrggbb`
    pub colors: Vec<String>,
}

/// Extra data from `/w/<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detail {
    pub tags: Vec<String>,
    pub views: Option<u64>,
    pub favorites: Option<u64>,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub walls: Vec<Wall>,
    pub current: u32,
    pub last_page: u32,
    pub total: u64,
    /// Returned for `random` sorting; reused to keep paging stable.
    pub seed: Option<String>,
}

/// Wallhaven ids are short lowercase alphanumerics (the same rule `dechaven` uses).
pub fn valid_id(id: &str) -> bool {
    crate::wallhaven::wallpaper::valid_id(id)
}

pub fn valid_hex(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn hex_rgb(c: &str) -> Option<(u8, u8, u8)> {
    if !valid_hex(c) {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).ok();
    Some((p(1)?, p(3)?, p(5)?))
}

#[derive(Deserialize)]
struct RawThumbs {
    #[serde(default)]
    small: String,
    #[serde(default)]
    large: String,
}

#[derive(Deserialize)]
struct RawWall {
    id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    views: u64,
    #[serde(default)]
    favorites: u64,
    #[serde(default)]
    purity: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    dimension_x: u32,
    #[serde(default)]
    dimension_y: u32,
    #[serde(default)]
    resolution: String,
    #[serde(default)]
    file_size: u64,
    #[serde(default)]
    file_type: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    colors: Vec<String>,
    #[serde(default)]
    path: String,
    thumbs: Option<RawThumbs>,
    #[serde(default)]
    tags: Vec<RawTag>,
}

#[derive(Deserialize)]
struct RawTag {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawMeta {
    #[serde(default)]
    current_page: Option<u32>,
    #[serde(default)]
    last_page: Option<u32>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    seed: Option<String>,
}

#[derive(Deserialize)]
struct RawSearch {
    #[serde(default)]
    data: Vec<RawWall>,
    meta: Option<RawMeta>,
}

#[derive(Deserialize)]
struct RawDetail {
    data: RawWall,
}

fn short(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

fn clean_wall(r: RawWall) -> Option<Wall> {
    if !valid_id(&r.id) || r.path.is_empty() {
        return None;
    }
    let t = r.thumbs?;
    let (w, h) = (r.dimension_x, r.dimension_y);
    let resolution = if w > 0 && h > 0 { format!("{w}x{h}") } else { short(&r.resolution, 16) };
    Some(Wall {
        id: r.id.clone(),
        url: short(&r.url, 200),
        thumb_small: if t.small.is_empty() { t.large.clone() } else { t.small },
        thumb_large: if t.large.is_empty() { String::new() } else { t.large },
        full: r.path,
        resolution,
        width: w,
        height: h,
        file_size: r.file_size,
        file_type: short(&r.file_type, 24),
        purity: short(&r.purity, 8),
        category: short(&r.category, 12),
        views: r.views,
        favorites: r.favorites,
        colors: r.colors.into_iter().filter(|c| valid_hex(c)).take(8).collect(),
    })
    .filter(|w| !w.thumb_small.is_empty())
}

/// Parses a `/search` response. `requested` is the page asked for (used when `meta` is missing).
pub fn parse_search(json: &str, requested: u32) -> Result<Page, String> {
    let r: RawSearch = serde_json::from_str(json).map_err(|e| format!("unexpected answer from wallhaven: {e}"))?;
    let walls: Vec<Wall> = r.data.into_iter().filter_map(clean_wall).collect();
    let m = r.meta;
    Ok(Page {
        walls,
        current: m.as_ref().and_then(|m| m.current_page).unwrap_or(requested).max(1),
        last_page: m.as_ref().and_then(|m| m.last_page).unwrap_or(requested).max(1),
        total: m.as_ref().and_then(|m| m.total).unwrap_or(0),
        seed: m.and_then(|m| m.seed).filter(|s| !s.is_empty() && s.len() <= 12 && s.bytes().all(|c| c.is_ascii_alphanumeric())),
    })
}

/// Parses a `/w/<id>` response into the wallpaper and its tags. The uploader is never read.
pub fn parse_detail(json: &str) -> Result<(Wall, Detail), String> {
    let r: RawDetail = serde_json::from_str(json).map_err(|e| format!("unexpected answer from wallhaven: {e}"))?;
    let tags: Vec<String> = r.data.tags.iter().map(|t| short(&t.name, 40)).filter(|t| !t.is_empty()).take(30).collect();
    let (views, favorites, created) = (r.data.views, r.data.favorites, short(&r.data.created_at, 24));
    let wall = clean_wall(r.data).ok_or("wallhaven sent an incomplete wallpaper")?;
    Ok((wall, Detail { tags, views: Some(views), favorites: Some(favorites), created }))
}

/// Human size: `6.7 MB`.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= KB * KB * KB {
        format!("{:.1} GB", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// `1234567` -> `1.2M`.
pub fn short_count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const SEARCH: &str = include_str!("../../../tests/fixtures/wallhaven/search.json");
    pub const QUERY: &str = include_str!("../../../tests/fixtures/wallhaven/search_query.json");
    pub const DETAIL: &str = include_str!("../../../tests/fixtures/wallhaven/detail.json");

    #[test]
    fn parses_a_recorded_toplist_page() {
        let p = parse_search(SEARCH, 1).unwrap();
        assert_eq!(p.walls.len(), 6);
        assert_eq!((p.current, p.last_page, p.total), (1, 22, 514));
        assert_eq!(p.seed, None);
        let w = &p.walls[0];
        assert_eq!(w.id, "d8vvlo");
        assert_eq!((w.width, w.height, w.resolution.as_str()), (3840, 2160, "3840x2160"));
        assert_eq!((w.file_size, w.file_type.as_str(), w.purity.as_str(), w.category.as_str()), (7063337, "image/png", "sfw", "general"));
        assert_eq!(w.thumb_small, "https://th.wallhaven.cc/small/d8/d8vvlo.jpg");
        assert_eq!(w.thumb_large, "https://th.wallhaven.cc/lg/d8/d8vvlo.jpg");
        assert_eq!(w.full, "https://w.wallhaven.cc/full/d8/wallhaven-d8vvlo.png");
        assert_eq!(w.colors, ["#999999", "#663300", "#424153", "#000000", "#996633"]);
        assert_eq!((w.views > 0, w.favorites > 0), (true, true));
        assert_eq!(p.walls[2].resolution, "4000x5001", "portrait items are kept");
    }

    #[test]
    fn parses_a_text_search_whose_meta_query_is_a_string() {
        let p = parse_search(QUERY, 1).unwrap();
        assert_eq!(p.walls.len(), 4);
        assert_eq!(p.walls[0].id, "4g1k9q");
        assert_eq!(p.last_page, 22);
    }

    #[test]
    fn meta_query_may_also_be_an_object_or_null() {
        let a = parse_search(r#"{"data":[],"meta":{"last_page":2,"query":{"id":1,"tag":"nature"}}}"#, 1).unwrap();
        assert_eq!(a.last_page, 2);
        assert!(parse_search(r#"{"data":[],"meta":{"last_page":2,"query":null,"seed":"aB3xyZ"}}"#, 1).unwrap().seed.is_some());
    }

    #[test]
    fn missing_meta_uses_the_requested_page() {
        let p = parse_search(r#"{"data":[]}"#, 4).unwrap();
        assert!(p.walls.is_empty());
        assert_eq!((p.current, p.last_page), (4, 4));
    }

    #[test]
    fn garbage_and_html_are_errors() {
        assert!(parse_search("<html>Too Many Requests</html>", 1).is_err());
        assert!(parse_search("", 1).is_err());
        assert!(parse_search("[]", 1).is_err());
        assert!(parse_detail("{}").is_err());
    }

    #[test]
    fn entries_with_bad_ids_or_without_thumbs_are_dropped() {
        let json = r##"{"data":[
          {"id":"../etc","path":"p","thumbs":{"small":"s","large":"l"}},
          {"id":"UPPER","path":"p","thumbs":{"small":"s","large":"l"}},
          {"id":"nothumbs","path":"p"},
          {"id":"nopath","thumbs":{"small":"s","large":"l"}},
          {"id":"ok1","path":"https://w.wallhaven.cc/full/ok/wallhaven-ok1.jpg","resolution":"10x10","colors":["#fff","#00ff00","red"],"thumbs":{"small":"s","large":"l"}}
        ]}"##;
        let p = parse_search(json, 1).unwrap();
        assert_eq!(p.walls.len(), 1);
        assert_eq!(p.walls[0].id, "ok1");
        assert_eq!(p.walls[0].colors, ["#00ff00"], "malformed colours are dropped");
        assert_eq!(p.walls[0].resolution, "10x10");
    }

    #[test]
    fn detail_has_tags_and_never_the_uploader() {
        let (w, d) = parse_detail(DETAIL).unwrap();
        assert_eq!(w.id, "d8vvlo");
        assert_eq!(d.tags, ["car", "Ferrari", "Ferrari F40", "supercars", "race cars", "Marlboro"]);
        assert_eq!(d.created, "2026-10-02 17:37:45");
        assert!(d.views.unwrap() > 0);
        assert!(!DETAIL.contains("uploader"), "the recorded fixture has the uploader stripped");
        // a response that does carry an uploader parses and the name goes nowhere
        let with = DETAIL.replacen("\"data\": {", "\"data\": {\"uploader\":{\"username\":\"someone\",\"group\":\"User\"},", 1);
        let (w2, d2) = parse_detail(&with).unwrap();
        assert_eq!((w2, d2), (w, d));
    }

    #[test]
    fn wall_roundtrips_through_json_for_the_favourites_file() {
        let p = parse_search(SEARCH, 1).unwrap();
        let s = serde_json::to_string(&p.walls).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Wall>>(&s).unwrap(), p.walls);
    }

    #[test]
    fn helpers() {
        assert!(valid_id("x8m2yp") && !valid_id("") && !valid_id("a/b") && !valid_id(&"a".repeat(17)));
        assert_eq!(hex_rgb("#0a84ff"), Some((10, 132, 255)));
        assert_eq!(hex_rgb("0a84ff"), None);
        assert_eq!(human_size(7063337), "6.7 MB");
        assert_eq!(human_size(900), "900 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(short_count(58499), "58.5k");
        assert_eq!(short_count(12), "12");
    }
}
