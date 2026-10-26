//! Wallhaven search API: query parameters and response parsing. No I/O here.

use serde::Deserialize;

pub const API_URL: &str = "https://wallhaven.cc/api/v1/search";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sorting {
    Relevance,
    Toplist,
    DateAdded,
    Random,
    /// Local history of wallpapers already used; not a network query.
    Recent,
}

impl Sorting {
    pub const ALL: [Sorting; 5] = [
        Sorting::Relevance,
        Sorting::Toplist,
        Sorting::DateAdded,
        Sorting::Random,
        Sorting::Recent,
    ];

    pub fn api_value(self) -> &'static str {
        match self {
            Sorting::Relevance => "relevance",
            Sorting::Toplist => "toplist",
            Sorting::DateAdded => "date_added",
            Sorting::Random => "random",
            Sorting::Recent => "recent",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sorting::Relevance => "Relevant",
            Sorting::Toplist => "Top",
            Sorting::DateAdded => "Latest",
            Sorting::Random => "Random",
            Sorting::Recent => "Recent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub sorting: Sorting,
    /// Only wallpapers at least as large as the screen, landscape.
    pub fit_screen: bool,
    pub screen: (u32, u32),
    /// Keeps `random` results stable across pages.
    pub seed: String,
}

impl Search {
    pub fn new(query: &str, sorting: Sorting) -> Self {
        Self {
            query: query.trim().to_string(),
            sorting,
            fit_screen: true,
            screen: (0, 0),
            seed: String::new(),
        }
    }

    /// Query-string pairs for `page`, matching the original QML plugin
    /// (general category only, safe-for-work only).
    pub fn params(&self, page: u32) -> Vec<(String, String)> {
        let mut p: Vec<(String, String)> = vec![
            ("q".into(), self.query.clone()),
            ("categories".into(), "100".into()),
            ("purity".into(), "100".into()),
            ("sorting".into(), self.sorting.api_value().into()),
            ("order".into(), "desc".into()),
            ("page".into(), page.to_string()),
        ];
        if self.sorting == Sorting::Toplist {
            p.push(("topRange".into(), "1M".into()));
        }
        if self.sorting == Sorting::Random && !self.seed.is_empty() {
            p.push(("seed".into(), self.seed.clone()));
        }
        if self.fit_screen && self.screen.0 > 0 && self.screen.1 > 0 {
            p.push((
                "atleast".into(),
                format!("{}x{}", self.screen.0, self.screen.1),
            ));
            p.push(("ratios".into(), "landscape".into()));
        }
        p
    }
}

/// Pressing Enter with words typed leaves the Top/Recent tabs for Relevant, like the original.
pub fn sorting_after_submit(current: Sorting, query: &str) -> Sorting {
    if !query.trim().is_empty() && matches!(current, Sorting::Toplist | Sorting::Recent) {
        Sorting::Relevance
    } else {
        current
    }
}

/// One wallpaper in a result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    /// Thumbnail: an https URL, or `file:/path` for a local wallpaper.
    pub thumb: String,
    /// Full image: an https URL, or an absolute path for a local wallpaper.
    pub full: String,
    pub resolution: String,
    /// `None` for local files.
    pub favorites: Option<u32>,
}

impl Item {
    pub fn is_local(&self) -> bool {
        self.full.starts_with('/')
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<Item>,
    pub last_page: u32,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    data: Vec<Wall>,
    meta: Option<Meta>,
}

#[derive(Deserialize)]
struct Meta {
    last_page: Option<u32>,
}

#[derive(Deserialize)]
struct Wall {
    id: String,
    path: String,
    #[serde(default)]
    resolution: String,
    #[serde(default)]
    favorites: u32,
    thumbs: Thumbs,
}

#[derive(Deserialize)]
struct Thumbs {
    large: String,
}

/// Parses a search response. `requested` is the page asked for (used when `meta` is missing).
pub fn parse(json: &str, requested: u32) -> Result<Page, String> {
    let r: Response = serde_json::from_str(json).map_err(|e| format!("bad response: {e}"))?;
    let items = r
        .data
        .into_iter()
        .map(|w| Item {
            id: w.id,
            thumb: w.thumbs.large,
            full: w.path,
            resolution: w.resolution,
            favorites: Some(w.favorites),
        })
        .collect();
    let last_page = r.meta.and_then(|m| m.last_page).unwrap_or(requested).max(1);
    Ok(Page { items, last_page })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed capture of a real `https://wallhaven.cc/api/v1/search` response.
    const SAMPLE: &str = r##"{"data":[
      {"id":"x8m2yp","url":"https://wallhaven.cc/w/x8m2yp","short_url":"https://whvn.cc/x8m2yp","views":1203,"favorites":57,
       "source":"","purity":"sfw","category":"general","dimension_x":3840,"dimension_y":2160,"resolution":"3840x2160","ratio":"1.78",
       "file_size":4519311,"file_type":"image/jpeg","created_at":"2026-10-01 10:11:12","colors":["#000000","#424153"],
       "path":"https://w.wallhaven.cc/full/x8/wallhaven-x8m2yp.jpg",
       "thumbs":{"large":"https://th.wallhaven.cc/lg/x8/x8m2yp.jpg","original":"https://th.wallhaven.cc/orig/x8/x8m2yp.jpg","small":"https://th.wallhaven.cc/small/x8/x8m2yp.jpg"}},
      {"id":"9d7kqw","views":5,"favorites":0,"resolution":"5120x2160","path":"https://w.wallhaven.cc/full/9d/wallhaven-9d7kqw.png",
       "thumbs":{"large":"https://th.wallhaven.cc/lg/9d/9d7kqw.jpg","original":"o","small":"s"}}
    ],"meta":{"current_page":1,"last_page":37,"per_page":24,"total":873,"query":null,"seed":null}}"##;

    #[test]
    fn parses_sample_response() {
        let p = parse(SAMPLE, 1).unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.last_page, 37);
        assert_eq!(p.items[0].id, "x8m2yp");
        assert_eq!(p.items[0].favorites, Some(57));
        assert_eq!(p.items[0].thumb, "https://th.wallhaven.cc/lg/x8/x8m2yp.jpg");
        assert_eq!(p.items[1].resolution, "5120x2160");
        assert!(!p.items[0].is_local());
    }

    #[test]
    fn missing_meta_uses_requested_page() {
        let p = parse(r#"{"data":[]}"#, 4).unwrap();
        assert!(p.items.is_empty());
        assert_eq!(p.last_page, 4);
    }

    #[test]
    fn meta_query_may_be_an_object() {
        let json = r#"{"data":[],"meta":{"last_page":2,"query":{"id":1,"tag":"nature"}}}"#;
        assert_eq!(parse(json, 1).unwrap().last_page, 2);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse("<html>rate limited</html>", 1).is_err());
        assert!(parse("", 1).is_err());
    }

    fn get<'a>(p: &'a [(String, String)], k: &str) -> Option<&'a str> {
        p.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn toplist_params_match_original() {
        let mut s = Search::new("  forest ", Sorting::Toplist);
        s.screen = (3440, 1440);
        let p = s.params(3);
        assert_eq!(get(&p, "q"), Some("forest"));
        assert_eq!(get(&p, "categories"), Some("100"));
        assert_eq!(get(&p, "purity"), Some("100"));
        assert_eq!(get(&p, "sorting"), Some("toplist"));
        assert_eq!(get(&p, "topRange"), Some("1M"));
        assert_eq!(get(&p, "page"), Some("3"));
        assert_eq!(get(&p, "atleast"), Some("3440x1440"));
        assert_eq!(get(&p, "ratios"), Some("landscape"));
    }

    #[test]
    fn random_uses_seed_and_fit_can_be_off() {
        let mut s = Search::new("", Sorting::Random);
        s.seed = "abc123".into();
        s.fit_screen = false;
        s.screen = (1920, 1080);
        let p = s.params(1);
        assert_eq!(get(&p, "seed"), Some("abc123"));
        assert_eq!(get(&p, "atleast"), None);
        assert_eq!(get(&p, "topRange"), None);
    }

    #[test]
    fn unknown_screen_skips_size_filter() {
        let s = Search::new("x", Sorting::Relevance);
        assert_eq!(get(&s.params(1), "atleast"), None);
    }

    #[test]
    fn submit_leaves_top_and_recent_only_with_words() {
        assert_eq!(
            sorting_after_submit(Sorting::Toplist, "cats"),
            Sorting::Relevance
        );
        assert_eq!(
            sorting_after_submit(Sorting::Recent, "cats"),
            Sorting::Relevance
        );
        assert_eq!(
            sorting_after_submit(Sorting::Toplist, "  "),
            Sorting::Toplist
        );
        assert_eq!(
            sorting_after_submit(Sorting::Random, "cats"),
            Sorting::Random
        );
    }
}
