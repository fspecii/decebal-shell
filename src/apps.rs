//! Installed applications, read from freedesktop `.desktop` files (for the launcher).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    pub name: String,
    pub exec: String,
    pub icon: String,
}

fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(h).join("applications"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("applications")));
    dirs
}

/// Parses one `.desktop` file; `None` for entries that should not be listed.
pub fn parse_entry(text: &str) -> Option<AppEntry> {
    let (mut name, mut exec, mut icon) = (None, None, String::new());
    let (mut in_entry, mut is_app) = (false, true);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("Name=") {
                name.get_or_insert_with(|| v.to_string());
            } else if let Some(v) = line.strip_prefix("Exec=") {
                exec.get_or_insert_with(|| v.to_string());
            } else if let Some(v) = line.strip_prefix("Icon=") {
                icon = v.to_string();
            } else if let Some(v) = line.strip_prefix("Type=") {
                is_app = v == "Application";
            } else if line == "NoDisplay=true" || line == "Hidden=true" {
                return None;
            }
        }
    }
    if !is_app {
        return None;
    }
    Some(AppEntry { name: name?, exec: exec?, icon })
}

/// Every visible application, sorted by name. An earlier directory shadows a later one with the same file name.
pub fn list() -> Vec<AppEntry> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for dir in app_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let id = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !seen.insert(id) {
                continue;
            }
            if let Some(entry) = read_entry(&path) {
                out.push(entry);
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

fn read_entry(path: &Path) -> Option<AppEntry> {
    parse_entry(&std::fs::read_to_string(path).ok()?)
}

fn cache_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("macshell/apps.tsv"))
}

fn esc(s: &str) -> String {
    s.replace(['\t', '\n'], " ")
}

/// The app list saved by [`save_cache`]; empty when there is none yet.
pub fn load_cache() -> Vec<AppEntry> {
    let Some(text) = cache_file().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            Some(AppEntry { name: p.next()?.to_string(), exec: p.next()?.to_string(), icon: p.next()?.to_string() })
        })
        .collect()
}

pub fn save_cache(apps: &[AppEntry]) {
    let Some(path) = cache_file() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = apps.iter().map(|a| format!("{}\t{}\t{}\n", esc(&a.name), esc(&a.exec), esc(&a.icon))).collect();
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Indices of `apps` matching `query` (case-insensitive): name prefixes first, then other substrings.
pub fn filter(apps: &[AppEntry], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..apps.len()).collect();
    }
    let (mut prefix, mut inside) = (Vec::new(), Vec::new());
    for (i, a) in apps.iter().enumerate() {
        let n = a.name.to_lowercase();
        if n.starts_with(&q) || n.split_whitespace().any(|w| w.starts_with(&q)) {
            prefix.push(i);
        } else if n.contains(&q) {
            inside.push(i);
        }
    }
    prefix.extend(inside);
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppEntry {
        AppEntry { name: name.into(), exec: "x".into(), icon: String::new() }
    }

    #[test]
    fn parses_a_normal_entry() {
        let t = "[Desktop Entry]\nType=Application\nName=Foot\nExec=foot %U\nIcon=foot\n[Desktop Action x]\nName=Other\n";
        assert_eq!(parse_entry(t), Some(AppEntry { name: "Foot".into(), exec: "foot %U".into(), icon: "foot".into() }));
    }

    #[test]
    fn hidden_and_non_apps_are_skipped() {
        assert!(parse_entry("[Desktop Entry]\nName=A\nExec=a\nNoDisplay=true\n").is_none());
        assert!(parse_entry("[Desktop Entry]\nType=Link\nName=A\nExec=a\n").is_none());
        assert!(parse_entry("[Desktop Entry]\nName=A\n").is_none());
    }

    #[test]
    fn filter_prefers_prefixes() {
        let apps = vec![app("Visual Studio"), app("Files"), app("Profile"), app("Fire")];
        assert_eq!(filter(&apps, "fi"), vec![1, 3, 2]);
        assert_eq!(filter(&apps, "").len(), 4);
        assert!(filter(&apps, "zzz").is_empty());
    }

    #[test]
    fn word_prefix_counts() {
        assert_eq!(filter(&[app("Visual Studio Code")], "stu"), vec![0]);
    }
}
