//! Search preferences and the query-string builder for wallhaven's `/api/v1/search`. No I/O.
//!
//! Purity is gated here and nowhere else decides it: sketchy / NSFW results are only requested when
//! the user switched "Allow sketchy and NSFW" on **and** has saved an API key (wallhaven itself refuses
//! them otherwise). [`purity_bits`] is the single source of truth, and [`purity_allowed`] re-checks
//! every item that comes back.

use serde::{Deserialize, Serialize};

pub const MAX_QUERY_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    Toplist,
    DateAdded,
    Views,
    Favorites,
    Random,
    Relevance,
}

impl Sort {
    pub const ALL: [Sort; 6] = [Sort::Toplist, Sort::DateAdded, Sort::Views, Sort::Favorites, Sort::Random, Sort::Relevance];

    /// Value of the `sorting` parameter.
    pub fn api(self) -> &'static str {
        match self {
            Sort::Toplist => "toplist",
            Sort::DateAdded => "date_added",
            Sort::Views => "views",
            Sort::Favorites => "favorites",
            Sort::Random => "random",
            Sort::Relevance => "relevance",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Toplist => "Top",
            Sort::DateAdded => "Latest",
            Sort::Views => "Views",
            Sort::Favorites => "Faves",
            Sort::Random => "Random",
            Sort::Relevance => "Relevant",
        }
    }

    pub fn parse(s: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|x| x.api() == s)
    }
}

/// Values of `topRange`, with the labels shown in the pane.
pub const TOP_RANGES: [(&str, &str); 7] = [("1d", "1d"), ("3d", "3d"), ("1w", "1w"), ("1M", "1m"), ("3M", "3m"), ("6M", "6m"), ("1y", "1y")];

pub const CATEGORIES: [(&str, &str); 3] = [("general", "General"), ("anime", "Anime"), ("people", "People")];

/// Fixed colour palette wallhaven's `colors` filter accepts (hex without `#`).
pub const COLORS: [&str; 29] = [
    "660000", "990000", "cc0000", "cc3333", "ea4c88", "993399", "663399", "333399", "0066cc", "0099cc", "66cccc", "77cc33", "669900", "336600", "666600", "999900", "cccc33", "ffff00", "ffcc33",
    "ff9900", "ff6600", "cc6633", "996633", "663300", "000000", "999999", "cccccc", "ffffff", "424153",
];

pub fn valid_color(hex: &str) -> bool {
    COLORS.contains(&hex)
}

/// `(label, value)` choices of the resolution filter. Values: `any`, `screen` (at least this screen),
/// `exact` (exactly this screen) or `WxH` (at least that size).
pub const RESOLUTIONS: [(&str, &str); 7] = [
    ("Any size", "any"),
    ("At least this screen", "screen"),
    ("Exactly this screen", "exact"),
    ("At least 1920×1080", "1920x1080"),
    ("At least 2560×1440", "2560x1440"),
    ("At least 3840×2160", "3840x2160"),
    ("At least 5120×2880", "5120x2880"),
];

/// `(label, value)` choices of the aspect-ratio filter. `screen` resolves to this screen's ratio.
pub const RATIOS: [(&str, &str); 10] = [
    ("Any shape", "any"),
    ("Match this screen", "screen"),
    ("Landscape", "landscape"),
    ("Portrait", "portrait"),
    ("16:9", "16x9"),
    ("16:10", "16x10"),
    ("21:9 ultrawide", "21x9"),
    ("32:9 super ultrawide", "32x9"),
    ("4:3", "4x3"),
    ("Square", "1x1"),
];

/// What the user chose; stored in `wallhaven.json` (never the API key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub query: String,
    pub sort: Sort,
    pub top_range: String,
    /// general, anime, people
    pub categories: [bool; 3],
    pub sketchy: bool,
    pub nsfw: bool,
    /// Master switch for sketchy / NSFW; ignored without an API key.
    pub allow_mature: bool,
    pub resolution: String,
    pub ratio: String,
    pub color: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            query: String::new(),
            sort: Sort::Toplist,
            top_range: "1M".into(),
            categories: [true, true, true],
            sketchy: false,
            nsfw: false,
            allow_mature: false,
            resolution: "screen".into(),
            ratio: "any".into(),
            color: None,
        }
    }
}

impl Prefs {
    /// Fixes up values read from a hand-edited file so the builder never emits anything odd.
    pub fn sanitised(mut self) -> Prefs {
        self.query = clean_query(&self.query);
        if !TOP_RANGES.iter().any(|(v, _)| *v == self.top_range) {
            self.top_range = "1M".into();
        }
        if !self.categories.iter().any(|c| *c) {
            self.categories = [true, true, true];
        }
        if !RESOLUTIONS.iter().any(|(_, v)| *v == self.resolution) {
            self.resolution = "any".into();
        }
        if !RATIOS.iter().any(|(_, v)| *v == self.ratio) {
            self.ratio = "any".into();
        }
        if self.color.as_deref().is_some_and(|c| !valid_color(c)) {
            self.color = None;
        }
        self
    }
}

/// Trims, drops control characters, collapses whitespace and caps the length.
pub fn clean_query(q: &str) -> String {
    let joined: String = q.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().take(MAX_QUERY_CHARS).collect()
}

/// The `purity` parameter: sfw, sketchy, nsfw. SFW is always on.
pub fn purity_bits(p: &Prefs, has_key: bool) -> [bool; 3] {
    let mature = p.allow_mature && has_key;
    [true, mature && p.sketchy, mature && p.nsfw]
}

/// Whether a result with wallhaven purity `purity` may be shown / downloaded under `bits`.
pub fn purity_allowed(purity: &str, bits: [bool; 3]) -> bool {
    match purity {
        "sfw" => bits[0],
        "sketchy" => bits[1],
        "nsfw" => bits[2],
        _ => false,
    }
}

fn bits_str(b: [bool; 3]) -> String {
    b.iter().map(|x| if *x { '1' } else { '0' }).collect()
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Nearest well-known ratio of a screen, else the gcd-reduced pair.
pub fn screen_ratio(w: u32, h: u32) -> Option<String> {
    if w == 0 || h == 0 {
        return None;
    }
    let r = f64::from(w) / f64::from(h);
    let known = [(16u32, 9u32), (16, 10), (21, 9), (32, 9), (4, 3), (5, 4), (3, 2), (1, 1), (9, 16)];
    let ratio = |k: &(u32, u32)| f64::from(k.0) / f64::from(k.1);
    let best = known.iter().min_by(|a, b| (r - ratio(a)).abs().total_cmp(&(r - ratio(b)).abs())).copied()?;
    if (r - ratio(&best)).abs() / r < 0.04 {
        return Some(format!("{}x{}", best.0, best.1));
    }
    let g = gcd(w, h);
    Some(format!("{}x{}", w / g, h / g))
}

fn valid_wxh(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    ((1..=20000).contains(&w) && (1..=20000).contains(&h)).then_some((w, h))
}

/// A search, ready to be turned into a query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub sort: Sort,
    pub top_range: String,
    pub categories: [bool; 3],
    pub purity: [bool; 3],
    /// `atleast=` size.
    pub atleast: Option<(u32, u32)>,
    /// `resolutions=` exact size.
    pub exact: Option<(u32, u32)>,
    pub ratios: Vec<String>,
    pub color: Option<String>,
    /// Keeps `random` stable across pages.
    pub seed: Option<String>,
}

impl Query {
    pub fn from_prefs(p: &Prefs, has_key: bool, screen: (u32, u32)) -> Query {
        let p = p.clone().sanitised();
        let screen_ok = screen.0 > 0 && screen.1 > 0;
        let (mut atleast, mut exact) = (None, None);
        match p.resolution.as_str() {
            "screen" if screen_ok => atleast = Some(screen),
            "exact" if screen_ok => exact = Some(screen),
            other => atleast = valid_wxh(other),
        }
        let ratios = match p.ratio.as_str() {
            "any" => vec![],
            "screen" => screen_ratio(screen.0, screen.1).into_iter().collect(),
            other => vec![other.to_string()],
        };
        Query {
            text: p.query.clone(),
            sort: p.sort,
            top_range: p.top_range.clone(),
            categories: p.categories,
            purity: purity_bits(&p, has_key),
            atleast,
            exact,
            ratios,
            color: p.color.clone(),
            seed: None,
        }
    }

    /// Query-string pairs for `page` (unencoded).
    pub fn pairs(&self, page: u32) -> Vec<(&'static str, String)> {
        let mut v: Vec<(&'static str, String)> = Vec::new();
        if !self.text.is_empty() {
            v.push(("q", self.text.clone()));
        }
        v.push(("categories", bits_str(self.categories)));
        v.push(("purity", bits_str(self.purity)));
        v.push(("sorting", self.sort.api().into()));
        v.push(("order", "desc".into()));
        if self.sort == Sort::Toplist {
            v.push(("topRange", self.top_range.clone()));
        }
        if let Some((w, h)) = self.atleast {
            v.push(("atleast", format!("{w}x{h}")));
        }
        if let Some((w, h)) = self.exact {
            v.push(("resolutions", format!("{w}x{h}")));
        }
        if !self.ratios.is_empty() {
            v.push(("ratios", self.ratios.join(",")));
        }
        if let Some(c) = &self.color {
            v.push(("colors", c.clone()));
        }
        if self.sort == Sort::Random {
            if let Some(s) = self.seed.as_ref().filter(|s| !s.is_empty() && s.len() <= 12 && s.chars().all(|c| c.is_ascii_alphanumeric())) {
                v.push(("seed", s.clone()));
            }
        }
        v.push(("page", page.max(1).to_string()));
        v
    }

    /// `base` + `/search?...`, percent-encoded. The API key never goes into the URL (it is a header).
    pub fn url(&self, base: &str, page: u32) -> String {
        let qs: Vec<String> = self.pairs(page).iter().map(|(k, v)| format!("{k}={}", encode(v))).collect();
        format!("{}/search?{}", base.trim_end_matches('/'), qs.join("&"))
    }
}

/// Percent-encodes everything but the RFC 3986 unreserved characters (and `,` which wallhaven lists use).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b',') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(v: &'a [(&'static str, String)], k: &str) -> Option<&'a str> {
        v.iter().find(|(key, _)| *key == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn defaults_are_sfw_toplist_at_least_the_screen() {
        let q = Query::from_prefs(&Prefs::default(), false, (3440, 1440));
        let p = q.pairs(1);
        assert_eq!(get(&p, "purity"), Some("100"));
        assert_eq!(get(&p, "categories"), Some("111"));
        assert_eq!(get(&p, "sorting"), Some("toplist"));
        assert_eq!(get(&p, "topRange"), Some("1M"));
        assert_eq!(get(&p, "atleast"), Some("3440x1440"));
        assert_eq!(get(&p, "page"), Some("1"));
        assert_eq!(get(&p, "q"), None);
        assert_eq!(get(&p, "apikey"), None, "the key is a header, never a parameter");
    }

    #[test]
    fn query_string_is_encoded() {
        let p = Prefs { query: "  dark   forest & mist=1 #x ".into(), sort: Sort::Relevance, ..Prefs::default() };
        let url = Query::from_prefs(&p, false, (0, 0)).url("http://127.0.0.1:9/api/v1/", 3);
        assert!(url.starts_with("http://127.0.0.1:9/api/v1/search?q=dark%20forest%20%26%20mist%3D1%20%23x&categories=111&purity=100&sorting=relevance&order=desc&page=3"), "{url}");
        assert!(!url.contains("topRange"));
        assert!(!url.contains("atleast"), "unknown screen: no size filter");
    }

    #[test]
    fn query_text_is_cleaned_and_capped() {
        assert_eq!(clean_query("a\tb\nc\u{7}d  "), "a b c d");
        assert_eq!(clean_query(&"x".repeat(500)).chars().count(), MAX_QUERY_CHARS);
    }

    #[test]
    fn categories_are_general_anime_people() {
        let mut p = Prefs { categories: [false, true, false], ..Prefs::default() };
        assert_eq!(get(&Query::from_prefs(&p, false, (0, 0)).pairs(1), "categories"), Some("010"));
        p.categories = [true, false, true];
        assert_eq!(get(&Query::from_prefs(&p, false, (0, 0)).pairs(1), "categories"), Some("101"));
        p.categories = [false; 3];
        assert_eq!(get(&Query::from_prefs(&p, false, (0, 0)).pairs(1), "categories"), Some("111"), "never an empty selection");
    }

    #[test]
    fn mature_content_needs_the_switch_and_a_key() {
        let mut p = Prefs { sketchy: true, nsfw: true, ..Prefs::default() };
        // boxes ticked but the master switch is off
        assert_eq!(purity_bits(&p, true), [true, false, false]);
        p.allow_mature = true;
        // switch on but no key
        assert_eq!(purity_bits(&p, false), [true, false, false]);
        assert_eq!(purity_bits(&p, true), [true, true, true]);
        p.nsfw = false;
        assert_eq!(get(&Query::from_prefs(&p, true, (0, 0)).pairs(1), "purity"), Some("110"));
        assert_eq!(get(&Query::from_prefs(&p, false, (0, 0)).pairs(1), "purity"), Some("100"));
    }

    #[test]
    fn returned_items_are_filtered_by_purity_again() {
        assert!(purity_allowed("sfw", [true, false, false]));
        assert!(!purity_allowed("sketchy", [true, false, false]));
        assert!(!purity_allowed("nsfw", [true, true, false]));
        assert!(purity_allowed("nsfw", [true, true, true]));
        assert!(!purity_allowed("", [true, true, true]), "unknown purity is never shown");
        assert!(!purity_allowed("SFW", [true, true, true]));
    }

    #[test]
    fn sorting_params() {
        let mut p = Prefs { sort: Sort::Random, ..Prefs::default() };
        let mut q = Query::from_prefs(&p, false, (0, 0));
        assert_eq!(get(&q.pairs(1), "seed"), None);
        q.seed = Some("aB3xyZ".into());
        let pairs = q.pairs(2);
        assert_eq!(get(&pairs, "seed"), Some("aB3xyZ"));
        assert_eq!(get(&pairs, "topRange"), None);
        q.seed = Some("bad seed&".into());
        assert_eq!(get(&q.pairs(2), "seed"), None, "odd seeds are not forwarded");
        p.sort = Sort::Views;
        assert_eq!(get(&Query::from_prefs(&p, false, (0, 0)).pairs(1), "sorting"), Some("views"));
        p.sort = Sort::DateAdded;
        assert_eq!(get(&Query::from_prefs(&p, false, (0, 0)).pairs(1), "sorting"), Some("date_added"));
        for s in Sort::ALL {
            assert_eq!(Sort::parse(s.api()), Some(s));
        }
    }

    #[test]
    fn resolution_and_ratio_filters() {
        let mut p = Prefs { resolution: "exact".into(), ratio: "screen".into(), ..Prefs::default() };
        let pairs = Query::from_prefs(&p, false, (3440, 1440)).pairs(1);
        assert_eq!(get(&pairs, "resolutions"), Some("3440x1440"));
        assert_eq!(get(&pairs, "atleast"), None);
        assert_eq!(get(&pairs, "ratios"), Some("21x9"));
        p.resolution = "2560x1440".into();
        p.ratio = "portrait".into();
        let pairs = Query::from_prefs(&p, false, (0, 0)).pairs(1);
        assert_eq!(get(&pairs, "atleast"), Some("2560x1440"));
        assert_eq!(get(&pairs, "ratios"), Some("portrait"));
        // "this screen" with an unknown screen asks for nothing
        p.resolution = "screen".into();
        p.ratio = "screen".into();
        let pairs = Query::from_prefs(&p, false, (0, 0)).pairs(1);
        assert_eq!((get(&pairs, "atleast"), get(&pairs, "ratios")), (None, None));
    }

    #[test]
    fn hand_edited_prefs_are_repaired() {
        let p = Prefs { top_range: "forever".into(), resolution: "0x0; drop".into(), ratio: "evil".into(), color: Some("zzzzzz".into()), categories: [false; 3], ..Prefs::default() }.sanitised();
        assert_eq!((p.top_range.as_str(), p.resolution.as_str(), p.ratio.as_str()), ("1M", "any", "any"));
        assert_eq!((p.color, p.categories), (None, [true; 3]));
    }

    #[test]
    fn screen_ratios() {
        assert_eq!(screen_ratio(3440, 1440).as_deref(), Some("21x9"));
        assert_eq!(screen_ratio(1920, 1080).as_deref(), Some("16x9"));
        assert_eq!(screen_ratio(1920, 1200).as_deref(), Some("16x10"));
        assert_eq!(screen_ratio(5120, 1440).as_deref(), Some("32x9"));
        assert_eq!(screen_ratio(0, 10), None);
        assert_eq!(screen_ratio(1500, 500).as_deref(), Some("3x1"));
    }

    #[test]
    fn prefs_json_roundtrip_and_partial_files() {
        let p = Prefs { query: "city".into(), sort: Sort::Favorites, color: Some("0066cc".into()), ..Prefs::default() };
        let s = serde_json::to_string(&p).unwrap();
        assert!(!s.to_lowercase().contains("key"));
        assert_eq!(serde_json::from_str::<Prefs>(&s).unwrap(), p);
        assert_eq!(serde_json::from_str::<Prefs>(r#"{"sort":"random"}"#).unwrap().sort, Sort::Random);
        assert!(serde_json::from_str::<Prefs>(r#"{"sort":"nonsense"}"#).is_err());
    }
}
