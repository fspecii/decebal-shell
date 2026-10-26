//! Current temperature for the bar, from wttr.in (same source as the original plugin).
//!
//! Fetching uses `curl` on a worker thread (no TLS stack in the binary, nothing resident), and the
//! result is cached on disk so restarts do not hit the network.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a cached reading stays fresh.
pub const MAX_AGE: Duration = Duration::from_secs(20 * 60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub temp_c: i32,
    /// wttr.in weather code.
    pub code: u32,
    /// Unix seconds when fetched.
    pub fetched: u64,
}

/// Parses `https://wttr.in/?format=j1` output.
pub fn parse_wttr(json: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cur = v.get("current_condition")?.get(0)?;
    let temp_c = cur.get("temp_C")?.as_str()?.trim().parse().ok()?;
    let code = cur.get("weatherCode")?.as_str()?.trim().parse().ok()?;
    Some(Weather { temp_c, code, fetched: now_secs() })
}

/// Maps a wttr.in code to one of the bar's weather glyphs.
pub fn symbol_for(code: u32) -> &'static str {
    match code {
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
    let w = parse_wttr(&out)?;
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
        assert_eq!(symbol_for(113), "weather-clear");
        assert_eq!(symbol_for(122), "weather-cloud");
        assert_eq!(symbol_for(296), "weather-rain");
        assert_eq!(symbol_for(338), "weather-snow");
        assert_eq!(symbol_for(389), "weather-storm");
        assert_eq!(symbol_for(143), "weather-fog");
        assert_eq!(symbol_for(9999), "weather-cloud");
    }

    #[test]
    fn freshness_window_is_20_minutes() {
        let w = Weather { temp_c: 1, code: 113, fetched: 1000 };
        assert!(is_fresh(&w, 1000 + 19 * 60));
        assert!(!is_fresh(&w, 1000 + 21 * 60));
        assert!(is_fresh(&w, 10), "clock going backwards must not panic");
    }

    #[test]
    fn cache_roundtrips_through_json() {
        let w = Weather { temp_c: 7, code: 116, fetched: 5 };
        assert_eq!(serde_json::from_str::<Weather>(&serde_json::to_string(&w).unwrap()).unwrap(), w);
    }
}
