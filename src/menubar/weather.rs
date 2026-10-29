//! Current temperature for the bar, fetched the way the original plugin does it: wttr.in finds the
//! coordinates, then Open-Meteo supplies the temperature and a day/night-aware condition.
//!
//! Fetching uses `curl` on a worker thread (no TLS stack in the binary, nothing resident), and the
//! result is cached on disk so restarts do not hit the network.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a cached reading stays fresh.
pub const MAX_AGE: Duration = Duration::from_secs(15 * 60);

/// One forecast day (wttr.in `weather[]`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub high_c: i32,
    pub low_c: i32,
    /// wttr.in code around midday.
    pub code: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub temp_c: i32,
    /// wttr.in-style weather code (Open-Meteo codes are mapped onto it).
    pub code: u32,
    /// True after dark (clear/partly-cloudy skies then show a moon).
    #[serde(default)]
    pub night: bool,
    /// Unix seconds when fetched.
    pub fetched: u64,
    /// Everything below came with the same wttr.in answer; older caches lack it (`serde(default)`).
    #[serde(default)]
    pub feels_c: Option<i32>,
    #[serde(default)]
    pub humidity: Option<u32>,
    #[serde(default)]
    pub wind_kmh: Option<u32>,
    /// 16-point compass direction the wind blows from (`NW`).
    #[serde(default)]
    pub wind_dir: String,
    /// "Partly cloudy" and the like, as wttr.in words it.
    #[serde(default)]
    pub desc: String,
    /// "Berlin, Germany".
    #[serde(default)]
    pub location: String,
    /// Today first.
    #[serde(default)]
    pub days: Vec<Day>,
}

fn first_value(v: &serde_json::Value, key: &str) -> Option<String> {
    let s = v.get(key)?.get(0)?.get("value")?.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn num<T: std::str::FromStr>(v: &serde_json::Value, key: &str) -> Option<T> {
    v.get(key)?.as_str()?.trim().parse().ok()
}

/// Parses `https://wttr.in/?format=j1` output.
pub fn parse_wttr(json: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current_condition")?.get(0)?;
    let temp_c = cur.get("temp_C")?.as_str()?.trim().parse().ok()?;
    let code = cur.get("weatherCode")?.as_str()?.trim().parse().ok()?;
    let mut w = Weather { temp_c, code, night: false, fetched: now_secs(), ..Weather::default() };
    w.feels_c = num(cur, "FeelsLikeC");
    w.humidity = num(cur, "humidity");
    w.wind_kmh = num(cur, "windspeedKmph");
    w.wind_dir = cur.get("winddir16Point").and_then(|d| d.as_str()).unwrap_or("").trim().to_string();
    w.desc = first_value(cur, "weatherDesc").unwrap_or_default();
    if let Some(area) = v.get("nearest_area").and_then(|a| a.get(0)) {
        let parts: Vec<String> = ["areaName", "country"].iter().filter_map(|k| first_value(area, k)).collect();
        w.location = parts.join(", ");
    }
    for d in v.get("weather").and_then(|d| d.as_array()).into_iter().flatten().take(4) {
        let (Some(high_c), Some(low_c)) = (num(d, "maxtempC"), num(d, "mintempC")) else { continue };
        // the midday slot of the 3-hourly list stands for the day
        let hourly = d.get("hourly").and_then(|h| h.as_array());
        let code = hourly.and_then(|h| h.get(4).or_else(|| h.first())).and_then(|h| num(h, "weatherCode")).unwrap_or(119);
        w.days.push(Day { date: d.get("date").and_then(|x| x.as_str()).unwrap_or("").to_string(), high_c, low_c, code });
    }
    Some(w)
}

/// Short weekday name of a `YYYY-MM-DD` date (`Tue`), `None` when it is not one.
pub fn weekday_short(date: &str) -> Option<&'static str> {
    let mut it = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Sakamoto's method; 0 = Sunday
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let w = (y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d).rem_euclid(7);
    Some(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][w as usize])
}

/// Words for a wttr.in code when the answer carried none (cached readings of older versions).
pub fn condition_words(code: u32, night: bool) -> &'static str {
    match symbol_for(code, night) {
        "weather-clear" => "Clear",
        "weather-moon" => "Clear night",
        "weather-partly" => "Partly cloudy",
        "weather-fog" => "Fog",
        "weather-rain" => "Rain",
        "weather-snow" => "Snow",
        "weather-storm" => "Thunderstorm",
        _ => "Cloudy",
    }
}

/// "just now", "5 min ago", "2 h ago".
pub fn age_words(fetched: u64, now: u64) -> String {
    let s = now.saturating_sub(fetched);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

/// Seconds since the epoch.
pub fn now() -> u64 {
    now_secs()
}

/// Coordinates wttr.in detected for this connection (`nearest_area`).
pub fn parse_wttr_area(json: &str) -> Option<(f64, f64)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let area = v.get("nearest_area")?.get(0)?;
    let lat = area.get("latitude")?.as_str()?.trim().parse().ok()?;
    let lon = area.get("longitude")?.as_str()?.trim().parse().ok()?;
    Some((lat, lon))
}

/// Maps an Open-Meteo (WMO) code onto the wttr.in code the glyph table uses (same mapping as the original plugin).
pub fn open_meteo_to_wttr(code: u32) -> u32 {
    match code {
        0 => 113,
        1 | 2 => 116,
        3 => 119,
        45 | 48 => 143,
        51 | 53 | 55 | 56 | 57 | 61 => 266,
        63 | 65 | 66 | 67 | 80 | 81 | 82 => 308,
        71 | 73 | 75 | 77 | 85 | 86 => 338,
        95 | 96 | 99 => 389,
        _ => 119,
    }
}

/// Parses Open-Meteo's `current` block.
pub fn parse_open_meteo(json: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current")?;
    let temp = cur.get("temperature_2m")?.as_f64()?;
    let code = cur.get("weather_code")?.as_u64()? as u32;
    let night = cur.get("is_day").and_then(|d| d.as_u64()) == Some(0);
    Some(Weather { temp_c: temp.round() as i32, code: open_meteo_to_wttr(code), night, fetched: now_secs(), ..Weather::default() })
}

/// The original plugin's Nerd Font weather glyphs: (wttr codes, day char, night char).
const NERD: &[(&[u32], u32, u32)] = &[
    (&[113], 0xe30d, 0xe32b),
    (&[116], 0xe302, 0xe32e),
    (&[119, 122], 0xe33d, 0xe33d),
    (&[143, 248, 260], 0xe313, 0xe346),
    (&[176, 263, 353], 0xe308, 0xe333),
    (&[179, 227, 230, 323, 326, 368], 0xe30a, 0xe327),
    (&[182, 185, 281, 284, 311, 314, 317, 320, 350, 362, 365, 374, 377], 0xe3ad, 0xe3ad),
    (&[200, 386, 389, 392, 395], 0xe31d, 0xe31d),
    (&[266, 293, 296, 299, 302, 305, 308, 356, 359], 0xe318, 0xe318),
    (&[329, 332, 335, 338, 371], 0xe31a, 0xe31a),
];
/// Glyph for anything not listed above.
const NERD_DEFAULT: u32 = 0xe33d;

/// Code point of the original plugin's Nerd Font glyph for a condition.
pub fn nerd_codepoint(code: u32, night: bool) -> u32 {
    NERD.iter().find(|(codes, _, _)| codes.contains(&code)).map(|&(_, day, nt)| if night { nt } else { day }).unwrap_or(NERD_DEFAULT)
}

/// Maps a wttr.in code to one of the bar's weather glyphs.
pub fn symbol_for(code: u32, night: bool) -> &'static str {
    match code {
        113 | 116 if night => "weather-moon",
        113 => "weather-clear",
        116 => "weather-partly",
        119 | 122 => "weather-cloud",
        143 | 248 | 260 => "weather-fog",
        200 | 386 | 389 | 392 | 395 => "weather-storm",
        179 | 182 | 185 | 227 | 230 | 281 | 284 | 317 | 320 | 323 | 326 | 329 | 332 | 335 | 338 | 350 | 362 | 365 | 368 | 371 | 374 | 377 => "weather-snow",
        176 | 263 | 266 | 293 | 296 | 299 | 302 | 305 | 308 | 311 | 314 | 353 | 356 | 359 => "weather-rain",
        _ => "weather-cloud",
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("decebal/weather.json"))
}

pub fn is_fresh(w: &Weather, now: u64) -> bool {
    now.saturating_sub(w.fetched) < MAX_AGE.as_secs()
}

pub fn load_cached() -> Option<Weather> {
    serde_json::from_str(&std::fs::read_to_string(cache_path()?).ok()?).ok()
}

fn store(w: &Weather) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string(w) {
        let _ = std::fs::write(path, s);
    }
}

/// Network fetch; blocking, call from a worker thread. `None` on any failure (stale data stays visible).
pub fn fetch() -> Option<Weather> {
    let out = crate::controlcenter::sys::run(&["curl", "-fsS", "--max-time", "10", "https://wttr.in/?format=j1"], Duration::from_secs(12))?;
    let mut w = parse_wttr(&out)?;
    // Like the original plugin without configured coordinates, temperature and condition come from wttr.in;
    // Open-Meteo at the coordinates wttr.in detected only supplies day/night.
    if let Some((lat, lon)) = parse_wttr_area(&out) {
        let url = format!("https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code,is_day&timezone=auto");
        if let Some(body) = crate::controlcenter::sys::run(&["curl", "-fsS", "--max-time", "8", &url], Duration::from_secs(10)) {
            if let Some(om) = parse_open_meteo(&body) {
                w.night = om.night;
            }
        }
    }
    store(&w);
    Some(w)
}

/// A fresh cached reading, or `None` if a fetch is needed.
pub fn fresh_cached() -> Option<Weather> {
    load_cached().filter(|w| is_fresh(w, now_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"current_condition":[{"FeelsLikeC":"6","temp_C":"9","weatherCode":"122","humidity":"80"}],"weather":[]}"#;

    #[test]
    fn parses_wttr_sample() {
        let w = parse_wttr(SAMPLE).unwrap();
        assert_eq!((w.temp_c, w.code), (9, 122));
    }

    #[test]
    fn parses_open_meteo_current_and_night() {
        let w = parse_open_meteo(r#"{"current":{"temperature_2m":9.4,"weather_code":3,"is_day":0}}"#).unwrap();
        assert_eq!((w.temp_c, w.code, w.night), (9, 119, true));
        let d = parse_open_meteo(r#"{"current":{"temperature_2m":-2.6,"weather_code":0,"is_day":1}}"#).unwrap();
        assert_eq!((d.temp_c, d.code, d.night), (-3, 113, false));
        assert!(parse_open_meteo(r#"{"current":{}}"#).is_none());
    }

    #[test]
    fn wttr_area_gives_coordinates() {
        let j = r#"{"nearest_area":[{"latitude":"51.5","longitude":"-0.12"}]}"#;
        assert_eq!(parse_wttr_area(j), Some((51.5, -0.12)));
        assert_eq!(parse_wttr_area("{}"), None);
    }

    #[test]
    fn open_meteo_codes_map_like_the_plugin() {
        assert_eq!(open_meteo_to_wttr(0), 113);
        assert_eq!(open_meteo_to_wttr(2), 116);
        assert_eq!(open_meteo_to_wttr(65), 308);
        assert_eq!(open_meteo_to_wttr(99), 389);
        assert_eq!(open_meteo_to_wttr(1234), 119);
    }

    #[test]
    fn negative_temperatures_parse() {
        let w = parse_wttr(r#"{"current_condition":[{"temp_C":"-3","weatherCode":"326"}]}"#).unwrap();
        assert_eq!(w.temp_c, -3);
    }

    #[test]
    fn bad_json_is_none() {
        assert!(parse_wttr("<html>rate limited</html>").is_none());
        assert!(parse_wttr(r#"{"current_condition":[]}"#).is_none());
        assert!(parse_wttr(r#"{"current_condition":[{"temp_C":"x","weatherCode":"1"}]}"#).is_none());
    }

    #[test]
    fn symbols_cover_the_main_conditions() {
        assert_eq!(symbol_for(113, false), "weather-clear");
        assert_eq!(symbol_for(113, true), "weather-moon");
        assert_eq!(symbol_for(119, true), "weather-cloud");
        assert_eq!(symbol_for(122, false), "weather-cloud");
        assert_eq!(symbol_for(296, false), "weather-rain");
        assert_eq!(symbol_for(338, false), "weather-snow");
        assert_eq!(symbol_for(389, false), "weather-storm");
        assert_eq!(symbol_for(143, false), "weather-fog");
        assert_eq!(symbol_for(9999, false), "weather-cloud");
    }

    #[test]
    fn nerd_glyphs_match_the_plugin_table() {
        assert_eq!(nerd_codepoint(113, false), 0xe30d);
        assert_eq!(nerd_codepoint(113, true), 0xe32b);
        assert_eq!(nerd_codepoint(143, true), 0xe346);
        assert_eq!(nerd_codepoint(182, false), 0xe3ad); // listed on a continuation line in the JS
        assert_eq!(nerd_codepoint(9999, false), 0xe33d);
    }

    #[test]
    fn freshness_window_is_15_minutes() {
        let w = Weather { temp_c: 1, code: 113, night: false, fetched: 1000, ..Weather::default() };
        assert!(is_fresh(&w, 1000 + 14 * 60));
        assert!(!is_fresh(&w, 1000 + 16 * 60));
        assert!(is_fresh(&w, 10), "clock going backwards must not panic");
    }

    #[test]
    fn cache_roundtrips_through_json() {
        let w = Weather { temp_c: 7, code: 116, night: false, fetched: 5, humidity: Some(40), days: vec![Day { date: "2026-10-29".into(), high_c: 9, low_c: 2, code: 113 }], ..Weather::default() };
        assert_eq!(serde_json::from_str::<Weather>(&serde_json::to_string(&w).unwrap()).unwrap(), w);
    }

    const FULL: &str = r#"{"current_condition":[{"FeelsLikeC":"6","temp_C":"9","weatherCode":"122","humidity":"80","windspeedKmph":"14","winddir16Point":"NW","weatherDesc":[{"value":"Overcast"}]}],
        "nearest_area":[{"latitude":"52.5","longitude":"13.4","areaName":[{"value":"Berlin"}],"country":[{"value":"Germany"}]}],
        "weather":[{"date":"2026-10-29","maxtempC":"11","mintempC":"4","hourly":[{"weatherCode":"119"},{},{},{},{"weatherCode":"296"}]},
                   {"date":"2026-10-30","maxtempC":"-1","mintempC":"-6","hourly":[]},{"date":"bad"}]}"#;

    #[test]
    fn parses_the_details_the_popup_shows() {
        let w = parse_wttr(FULL).unwrap();
        assert_eq!((w.feels_c, w.humidity, w.wind_kmh), (Some(6), Some(80), Some(14)));
        assert_eq!((w.wind_dir.as_str(), w.desc.as_str(), w.location.as_str()), ("NW", "Overcast", "Berlin, Germany"));
        assert_eq!(w.days.len(), 2, "a day without temperatures is skipped");
        assert_eq!((w.days[0].high_c, w.days[0].low_c, w.days[0].code), (11, 4, 296), "midday slot");
        assert_eq!((w.days[1].high_c, w.days[1].low_c, w.days[1].code), (-1, -6, 119), "no hourly data: a neutral cloud");
    }

    #[test]
    fn the_minimal_sample_still_parses_and_old_caches_load() {
        let w = parse_wttr(SAMPLE).unwrap();
        assert!(w.days.is_empty() && w.location.is_empty() && w.humidity == Some(80));
        let old: Weather = serde_json::from_str(r#"{"temp_c":3,"code":113,"night":false,"fetched":7}"#).unwrap();
        assert_eq!((old.temp_c, old.humidity, old.days.len()), (3, None, 0));
    }

    #[test]
    fn weekdays_and_ages_read_naturally() {
        assert_eq!(weekday_short("2026-10-29"), Some("Thu"));
        assert_eq!(weekday_short("2000-02-29"), Some("Tue"));
        assert_eq!(weekday_short("nonsense"), None);
        assert_eq!(weekday_short("2026-13-01"), None);
        assert_eq!(age_words(100, 130), "just now");
        assert_eq!(age_words(100, 100 + 5 * 60), "5 min ago");
        assert_eq!(age_words(100, 100 + 7300), "2 h ago");
        assert_eq!(age_words(500, 100), "just now");
        assert_eq!(condition_words(113, true), "Clear night");
        assert_eq!(condition_words(9999, false), "Cloudy");
    }
}
