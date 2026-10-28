//! The clipboard history: text entries, newest first, deduplicated, capped and kept in a small JSON file.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Entries kept; the oldest fall off.
pub const MAX_ENTRIES: usize = 100;
/// Longest text recorded (100 KB).
pub const MAX_BYTES: usize = 100 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub text: String,
    /// Unix seconds of the last time this text was copied.
    pub time: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct History {
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    entries: Vec<Entry>,
}

impl History {
    /// Newest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn latest(&self) -> Option<&Entry> {
        self.entries.first()
    }

    /// True for text worth keeping: not blank and not bigger than [`MAX_BYTES`].
    pub fn acceptable(text: &str) -> bool {
        !text.trim().is_empty() && text.len() <= MAX_BYTES
    }

    /// Records `text` copied at `now`: a copy of an existing entry moves it to the top, the list stays within
    /// [`MAX_ENTRIES`]. Returns false (and changes nothing) for unacceptable text.
    pub fn push(&mut self, text: &str, now: u64) -> bool {
        if !Self::acceptable(text) {
            return false;
        }
        if let Some(i) = self.entries.iter().position(|e| e.text == text) {
            self.entries.remove(i);
        }
        self.entries.insert(0, Entry { text: text.to_string(), time: now });
        self.entries.truncate(MAX_ENTRIES);
        true
    }

    pub fn remove(&mut self, index: usize) -> Option<Entry> {
        (index < self.entries.len()).then(|| self.entries.remove(index))
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Parses the file text; a damaged file gives an empty history, and entries that break the rules are dropped.
    pub fn parse(json: &str) -> History {
        let Ok(f) = serde_json::from_str::<File>(json) else { return History::default() };
        let mut h = History::default();
        for e in f.entries {
            if Self::acceptable(&e.text) && !h.entries.iter().any(|x| x.text == e.text) {
                h.entries.push(e);
            }
        }
        h.entries.truncate(MAX_ENTRIES);
        h
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&File { version: 1, entries: self.entries.clone() }).unwrap_or_else(|_| String::from("{\"version\":1,\"entries\":[]}"))
    }

    pub fn load(path: &Path) -> History {
        std::fs::read_to_string(path).map(|t| History::parse(&t)).unwrap_or_default()
    }

    /// Writes the file atomically (temp file in the same directory, mode 0600, rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        let result = (|| {
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            // An older temp file of ours may have had other permissions.
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            f.write_all(self.to_json().as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}

/// `$XDG_STATE_HOME/decebal/clipboard-history` (`~/.local/state/...` when unset).
pub fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("decebal").join("clipboard-history"))
}

// ---------------------------------------------------------------- what the picker shows

/// Indices of the entries whose text contains `query` (case-insensitive substring; an empty query matches all).
pub fn filter(entries: &[Entry], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    entries.iter().enumerate().filter(|(_, e)| q.is_empty() || e.text.to_lowercase().contains(&q)).map(|(i, _)| i).collect()
}

/// The first non-blank line (surrounding whitespace removed, tabs as spaces) and the number of lines.
pub fn summarize(text: &str) -> (String, usize) {
    let lines = text.trim_end_matches(['\n', '\r']).split('\n').count();
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().replace('\t', " ");
    (first, lines)
}

/// A short relative age: `now`, `5m`, `3h`, `2d`, `4w`.
pub fn age(now: u64, then: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..=44 => "now".into(),
        45..=3599 => format!("{}m", (s / 60).max(1)),
        3600..=86399 => format!("{}h", s / 3600),
        86400..=604_799 => format!("{}d", s / 86400),
        _ => format!("{}w", s / 604_800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(h: &History) -> Vec<&str> {
        h.entries().iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn newest_first_and_recopy_moves_to_top() {
        let mut h = History::default();
        for (i, t) in ["a", "b", "c"].iter().enumerate() {
            assert!(h.push(t, 10 + i as u64));
        }
        assert_eq!(texts(&h), ["c", "b", "a"]);
        assert!(h.push("a", 99));
        assert_eq!(texts(&h), ["a", "c", "b"]);
        assert_eq!(h.latest().unwrap().time, 99, "the time follows the re-copy");
        assert_eq!(h.len(), 3, "no duplicate");
    }

    #[test]
    fn blank_and_oversized_text_is_not_recorded() {
        let mut h = History::default();
        assert!(!h.push("", 1));
        assert!(!h.push(" \n\t ", 1));
        assert!(!h.push(&"x".repeat(MAX_BYTES + 1), 1));
        assert!(h.push(&"x".repeat(MAX_BYTES), 1));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn capped_at_100_dropping_the_oldest() {
        let mut h = History::default();
        for i in 0..150 {
            h.push(&format!("entry {i}"), i);
        }
        assert_eq!(h.len(), MAX_ENTRIES);
        assert_eq!(h.entries()[0].text, "entry 149");
        assert_eq!(h.entries()[MAX_ENTRIES - 1].text, "entry 50");
    }

    #[test]
    fn remove_and_clear() {
        let mut h = History::default();
        h.push("a", 1);
        h.push("b", 2);
        assert_eq!(h.remove(0).unwrap().text, "b");
        assert!(h.remove(5).is_none());
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn json_round_trip_keeps_unicode_newlines_and_order() {
        let mut h = History::default();
        h.push("line1\nline2 \"quoted\" é€ \u{1f600}", 5);
        h.push("tab\there", 6);
        assert_eq!(History::parse(&h.to_json()), h);
    }

    #[test]
    fn a_damaged_or_hand_edited_file_is_tolerated() {
        assert!(History::parse("not json").is_empty());
        assert!(History::parse("").is_empty());
        let h = History::parse(r#"{"version":1,"entries":[{"text":"a","time":1},{"text":"a","time":2},{"text":"  ","time":3},{"text":"b","time":4}]}"#);
        assert_eq!(texts(&h), ["a", "b"], "duplicates and blanks dropped");
    }

    #[test]
    fn save_is_private_atomic_and_loads_back() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("decclip-test-{}-{}", std::process::id(), line!()));
        let path = dir.join("decebal/clipboard-history");
        let mut h = History::default();
        h.push("secret-ish", 1);
        h.save(&path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(History::load(&path), h);
        h.push("second", 2);
        h.save(&path).unwrap();
        assert_eq!(History::load(&path).len(), 2);
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().filter(|e| e.as_ref().unwrap().file_name() != "clipboard-history").count();
        assert_eq!(leftovers, 0, "no temp file left behind");
        assert!(History::load(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filter_is_a_case_insensitive_substring() {
        let mut h = History::default();
        for t in ["Hello World", "foo bar", "HELLO again", "xyz"] {
            h.push(t, 1);
        }
        let e = h.entries();
        assert_eq!(filter(e, "").len(), 4);
        assert_eq!(filter(e, "hello").iter().map(|&i| e[i].text.as_str()).collect::<Vec<_>>(), ["HELLO again", "Hello World"]);
        assert_eq!(filter(e, "O W").len(), 1);
        assert!(filter(e, "nomatch").is_empty());
        assert_eq!(filter(e, "  foo ").len(), 1, "surrounding spaces in the query are ignored");
    }

    #[test]
    fn summaries_show_the_first_line_and_the_line_count() {
        assert_eq!(summarize("single"), ("single".into(), 1));
        assert_eq!(summarize("one\ntwo\nthree"), ("one".into(), 3));
        assert_eq!(summarize("one\n"), ("one".into(), 1), "a trailing newline is not a line");
        assert_eq!(summarize("\n\n  indented\tx\nnext"), ("indented x".into(), 4));
    }

    #[test]
    fn relative_ages() {
        assert_eq!(age(100, 100), "now");
        assert_eq!(age(100, 70), "now");
        assert_eq!(age(1000, 1000 - 120), "2m");
        assert_eq!(age(100_000, 100_000 - 3 * 3600), "3h");
        assert_eq!(age(1_000_000, 1_000_000 - 2 * 86400), "2d");
        assert_eq!(age(10_000_000, 10_000_000 - 3 * 604_800), "3w");
        assert_eq!(age(5, 100), "now", "a clock that went backwards");
    }
}
