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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub temp_c: i32,
    /// wttr.in-style weather code (Open-Meteo codes are mapped onto it).
    pub code: u32,
    /// True after dark (clear/partly-cloudy skies then show a moon).
    #[serde(default)]
    pub night: bool,
    /// Unix seconds when fetched.
    pub fetched: u64,
}

/// Parses `https://wttr.in/?format=j1` output.
pub fn parse_wttr(json: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current_condition")?.get(0)?;
    let temp_c = cur.get("temp_C")?.as_str()?.trim().parse().ok()?;
    let code = cur.get("weatherCode")?.as_str()?.trim().parse().ok()?;
    Some(Weather { temp_c, code, night: false, fetched: now_secs() })
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
    Some(Weather { temp_c: temp.round() as i32, code: open_meteo_to_wttr(code), night, fetched: now_secs() })
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
    Some(base.join("macshell/weather.json"))
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
        let w = Weather { temp_c: 1, code: 113, night: false, fetched: 1000 };
        assert!(is_fresh(&w, 1000 + 14 * 60));
        assert!(!is_fresh(&w, 1000 + 16 * 60));
        assert!(is_fresh(&w, 10), "clock going backwards must not panic");
    }

    #[test]
    fn cache_roundtrips_through_json() {
        let w = Weather { temp_c: 7, code: 116, night: false, fetched: 5 };
        assert_eq!(serde_json::from_str::<Weather>(&serde_json::to_string(&w).unwrap()).unwrap(), w);
    }
}
