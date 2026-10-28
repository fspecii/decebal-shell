//! Stay Awake (the coffee-cup icon): turning it off from the bar, how long it has been on, and timed sessions.
//!
//! The switch itself is Omarchy's (`omarchy toggle idle`, state = the `stay-awake` indicator file), the same one the
//! Control Center tile flips, so every view stays consistent. A timed session additionally leaves an expiry
//! (unix seconds) in `$XDG_STATE_HOME/decebal/stay-awake-until`; the bar's once-a-minute refresh turns Stay Awake off when
//! it has passed (so a timer is accurate to about a minute and only runs while the bar does).

use crate::controlcenter::sys;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The durations the popup offers, in seconds (`None` = until turned off).
pub const DURATIONS: [(Option<u64>, &str); 4] = [(Some(30 * 60), "30 minutes"), (Some(3600), "1 hour"), (Some(2 * 3600), "2 hours"), (None, "Indefinitely")];

/// Tests point the module at a private directory (and emulate the Omarchy toggle there): never the real state.
#[cfg(test)]
pub(crate) static TEST_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

fn state_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(d) = TEST_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(d);
    }
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
}

fn indicator(dir: &Path) -> PathBuf {
    dir.join("omarchy/indicators/stay-awake")
}

fn until_file(dir: &Path) -> PathBuf {
    dir.join("decebal/stay-awake-until")
}

/// Whether Stay Awake is on (the indicator file exists).
pub fn is_on() -> bool {
    state_dir().is_some_and(|d| indicator(&d).exists())
}

/// The Omarchy toggle (a flip); in tests it flips the indicator in the private directory instead.
fn toggle() {
    #[cfg(test)]
    if let Some(d) = TEST_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        let f = indicator(&d);
        if f.exists() {
            let _ = std::fs::remove_file(f);
        } else {
            let _ = std::fs::write(f, "");
        }
        return;
    }
    sys::toggle_stay_awake();
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether a timed session ending at `until` (unix seconds) is over at `now`.
pub fn expired(until: Option<u64>, now: u64) -> bool {
    until.is_some_and(|u| now >= u)
}

/// Unix seconds since Stay Awake was switched on (the indicator file's modification time), if it is on.
pub fn since_in(dir: &Path) -> Option<u64> {
    let m = std::fs::metadata(indicator(dir)).ok()?.modified().ok()?;
    m.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

pub fn since() -> Option<u64> {
    since_in(&state_dir()?)
}

pub fn until_in(dir: &Path) -> Option<u64> {
    std::fs::read_to_string(until_file(dir)).ok()?.trim().parse().ok()
}

/// The end of the running timed session, if there is one.
pub fn until() -> Option<u64> {
    until_in(&state_dir()?)
}

/// `1 h 05 min`, `42 min`, `under a minute`.
pub fn span_text(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    match (h, m) {
        (0, 0) => "under a minute".into(),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m:02} min"),
    }
}

/// The two lines of the hover popup: what it is and how long it has been on / when it ends.
pub fn status_lines(since: Option<u64>, until: Option<u64>, now: u64) -> (String, String) {
    let on = match since {
        Some(s) => format!("On for {}", span_text(now.saturating_sub(s))),
        None => "On".to_string(),
    };
    let end = match until {
        Some(u) if u > now => format!("Turns off in {}", span_text(u - now)),
        Some(_) => "Turning off".to_string(),
        None => "Stays on until you turn it off".to_string(),
    };
    (on, end)
}

/// Switches Stay Awake to `on` through the Omarchy toggle, only when that changes something (the toggle is a flip).
/// Returns whether it is now (assumed) `on`.
pub fn set(on: bool) -> bool {
    if is_on() != on {
        toggle();
    }
    if !on {
        clear_until();
    }
    on
}

pub fn clear_until() {
    if let Some(d) = state_dir() {
        let _ = std::fs::remove_file(until_file(&d));
    }
}

/// Starts (or restarts) Stay Awake for `secs` seconds, or until turned off with `None`.
pub fn start(secs: Option<u64>) {
    set(true);
    let Some(d) = state_dir() else { return };
    let f = until_file(&d);
    match secs {
        Some(s) => {
            let _ = std::fs::create_dir_all(f.parent().unwrap_or(&d));
            let _ = std::fs::write(&f, (now_secs() + s).to_string());
        }
        None => {
            let _ = std::fs::remove_file(&f);
        }
    }
}

/// The bar's refresh: is Stay Awake on, after ending an expired timed session and dropping a stale expiry.
pub fn refresh() -> bool {
    let on = is_on();
    let Some(dir) = state_dir() else { return on };
    let f = until_file(&dir);
    let until = until_in(&dir);
    if on && expired(until, now_secs()) {
        toggle();
        let _ = std::fs::remove_file(&f);
        return false;
    }
    // Switched off by someone else: the expiry is stale (not while an `omarchy` toggle may still be landing).
    if !on && until.is_some() && std::fs::metadata(&f).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() > 10) {
        let _ = std::fs::remove_file(&f);
    }
    on
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decebal-awake-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("omarchy/indicators")).unwrap();
        std::fs::create_dir_all(d.join("decebal")).unwrap();
        d
    }

    #[test]
    fn expiry_is_inclusive_and_absent_means_never() {
        assert!(!expired(None, 10));
        assert!(!expired(Some(11), 10));
        assert!(expired(Some(10), 10));
        assert!(expired(Some(5), 10));
    }

    #[test]
    fn since_and_until_read_their_files() {
        let d = tmp("read");
        assert_eq!(since_in(&d), None);
        assert_eq!(until_in(&d), None);
        std::fs::write(indicator(&d), "").unwrap();
        std::fs::write(until_file(&d), " 1234\n").unwrap();
        let s = since_in(&d).unwrap();
        assert!(s <= now_secs() && now_secs() - s < 60);
        assert_eq!(until_in(&d), Some(1234));
        std::fs::write(until_file(&d), "soon").unwrap();
        assert_eq!(until_in(&d), None, "garbage is no expiry");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Serialises the tests that use the private state directory.
    pub(crate) fn private_state(name: &str) -> (std::sync::MutexGuard<'static, ()>, PathBuf) {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = tmp(name);
        *TEST_DIR.lock().unwrap() = Some(d.clone());
        (g, d)
    }

    #[test]
    fn set_only_flips_when_the_state_differs_and_timed_sessions_expire() {
        let (_g, d) = private_state("flow");
        assert!(!is_on());
        set(false);
        assert!(!is_on(), "turning off what is off flips nothing");
        start(Some(600));
        assert!(is_on());
        let until = until_in(&d).unwrap();
        assert!(until >= now_secs() + 599 && until <= now_secs() + 601);
        assert!(refresh(), "still running");
        // the end passes: the bar's refresh switches it off and drops the expiry
        std::fs::write(until_file(&d), (now_secs() - 1).to_string()).unwrap();
        assert!(!refresh());
        assert!(!is_on());
        assert_eq!(until_in(&d), None);
        // indefinitely: no expiry file, and an old one is removed
        std::fs::write(until_file(&d), "99999999999").unwrap();
        start(None);
        assert!(is_on());
        assert_eq!(until_in(&d), None);
        assert!(refresh());
        set(false);
        assert!(!is_on());
        *TEST_DIR.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn texts() {
        assert_eq!(span_text(20), "under a minute");
        assert_eq!(span_text(42 * 60 + 5), "42 min");
        assert_eq!(span_text(3600 + 5 * 60), "1 h 05 min");
        let (a, b) = status_lines(Some(1000), None, 1000 + 90 * 60);
        assert_eq!(a, "On for 1 h 30 min");
        assert_eq!(b, "Stays on until you turn it off");
        let (a, b) = status_lines(None, Some(2000), 1000);
        assert_eq!(a, "On");
        assert_eq!(b, "Turns off in 16 min");
        assert_eq!(status_lines(Some(0), Some(5), 10).1, "Turning off");
    }
}
