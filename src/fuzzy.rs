//! Fuzzy matching for Spotlight, plus the small usage history that makes frequently chosen results rank higher.

use std::collections::HashMap;
use std::path::PathBuf;

/// How well `query` matches `text` (both compared case-insensitively); `None` when it does not match at all.
/// Exact > prefix > word prefix > substring > in-order subsequence (with bonuses for word starts and runs).
pub fn score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.trim().to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    if t.len() < q.len() {
        return None;
    }
    let len_pen = (t.len() - q.len()).min(60) as i32;
    if t == q {
        return Some(1000);
    }
    if t.starts_with(&q) {
        return Some(850 - len_pen);
    }
    let is_start = |i: usize| i == 0 || !t[i - 1].is_alphanumeric();
    if let Some(i) = (0..=t.len() - q.len()).find(|&i| is_start(i) && t[i..].starts_with(&q)) {
        return Some(700 - len_pen - (i as i32).min(40));
    }
    if let Some(i) = (0..=t.len() - q.len()).find(|&i| t[i..].starts_with(&q)) {
        return Some(450 - len_pen - (i as i32).min(40));
    }
    // Subsequence: every query char in order. Prefer runs and word starts; reject sparse matches.
    let mut score = 200;
    let mut ti = 0;
    let mut last: Option<usize> = None;
    for &c in &q {
        let found = (ti..t.len()).find(|&i| t[i] == c)?;
        if last.is_some_and(|l| found == l + 1) {
            score += 12;
        } else if let Some(l) = last {
            score -= ((found - l - 1) as i32).min(10);
        }
        if is_start(found) {
            score += 15;
        }
        last = Some(found);
        ti = found + 1;
    }
    // A query of 1-2 letters matching by subsequence alone is noise.
    (q.len() >= 3).then_some(score - len_pen / 2)
}

/// Best score of `query` against any of `fields`, later fields discounted.
pub fn best(query: &str, fields: &[&str]) -> Option<i32> {
    fields.iter().enumerate().filter_map(|(i, f)| score(query, f).map(|s| s - i as i32 * 120)).max()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Use {
    count: u32,
    last: u64,
}

/// How often and how recently each result was opened. Stored as `count<TAB>unix-time<TAB>key` lines.
#[derive(Debug, Default, Clone)]
pub struct History {
    map: HashMap<String, Use>,
}

fn state_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("decebal/spotlight-history"))
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl History {
    pub fn parse(text: &str) -> History {
        let mut map = HashMap::new();
        for line in text.lines() {
            let mut p = line.splitn(3, '\t');
            let (Some(c), Some(t), Some(k)) = (p.next(), p.next(), p.next()) else { continue };
            if let (Ok(count), Ok(last)) = (c.parse(), t.parse()) {
                map.insert(k.to_string(), Use { count, last });
            }
        }
        History { map }
    }

    pub fn load() -> History {
        state_file().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| History::parse(&t)).unwrap_or_default()
    }

    pub fn serialize(&self) -> String {
        let mut rows: Vec<_> = self.map.iter().collect();
        rows.sort_by(|a, b| b.1.last.cmp(&a.1.last).then(a.0.cmp(b.0)));
        rows.into_iter().take(500).map(|(k, u)| format!("{}\t{}\t{}\n", u.count, u.last, k)).collect()
    }

    pub fn save(&self) {
        let Some(path) = state_file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, self.serialize()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    pub fn bump(&mut self, key: &str, at: u64) {
        let u = self.map.entry(key.to_string()).or_default();
        u.count = u.count.saturating_add(1);
        u.last = at;
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Ranking bonus: grows with use count (capped) and decays with age.
    pub fn boost(&self, key: &str, at: u64) -> i32 {
        let Some(u) = self.map.get(key) else { return 0 };
        let freq = (u.count.min(30) as f32).sqrt() * 18.0;
        let age_days = at.saturating_sub(u.last) as f32 / 86400.0;
        let recency = 60.0 / (1.0 + age_days / 3.0);
        (freq + recency) as i32
    }

    /// Keys by most recent use.
    pub fn recent(&self, prefix: &str, n: usize) -> Vec<String> {
        let mut rows: Vec<_> = self.map.iter().filter(|(k, _)| k.starts_with(prefix)).collect();
        rows.sort_by(|a, b| b.1.last.cmp(&a.1.last).then(b.1.count.cmp(&a.1.count)));
        rows.into_iter().take(n).map(|(k, _)| k.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_tiers() {
        let exact = score("firefox", "Firefox").unwrap();
        let prefix = score("fire", "Firefox").unwrap();
        let word = score("code", "Visual Studio Code").unwrap();
        let inside = score("fox", "Firefox").unwrap();
        let fuzzy = score("ffx", "Firefox").unwrap();
        assert!(exact > prefix && prefix > word && word > inside && inside > fuzzy, "{exact} {prefix} {word} {inside} {fuzzy}");
    }

    #[test]
    fn non_matches_and_short_noise() {
        assert_eq!(score("zzz", "Firefox"), None);
        assert_eq!(score("xf", "Firefox"), None, "out of order");
        assert_eq!(score("fx", "Firefox"), None, "two-letter subsequence is noise");
        assert!(score("vsc", "Visual Studio Code").is_some());
    }

    #[test]
    fn shorter_names_win_ties() {
        assert!(score("term", "Terminal").unwrap() > score("term", "Terminal Emulator Pro").unwrap());
    }

    #[test]
    fn secondary_fields_are_discounted() {
        assert!(best("foot", &["Foot", "x"]).unwrap() > best("foot", &["Terminal", "foot"]).unwrap());
    }

    #[test]
    fn history_round_trip_and_boost() {
        let mut h = History::default();
        h.bump("app:Firefox", 1000);
        h.bump("app:Firefox", 2000);
        h.bump("app:Files", 500);
        let h2 = History::parse(&h.serialize());
        assert_eq!(h2.map.len(), 2);
        assert!(h2.boost("app:Firefox", 2100) > h2.boost("app:Files", 2100));
        assert_eq!(h2.boost("app:Nope", 2100), 0);
        assert_eq!(h2.recent("app:", 1), vec!["app:Firefox".to_string()]);
        assert!(History::parse("garbage\n1\tx\tk\n").is_empty());
    }

    #[test]
    fn old_use_decays() {
        let mut h = History::default();
        h.bump("a", 0);
        assert!(h.boost("a", 10) > h.boost("a", 86400 * 60));
    }
}
