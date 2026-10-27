//! Installed applications, read from freedesktop `.desktop` files (for the launcher).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppEntry {
    pub name: String,
    pub exec: String,
    pub icon: String,
    /// `Path=`: working directory of the program.
    pub path: String,
    /// `Terminal=true`: needs a terminal window.
    pub terminal: bool,
    /// `TryExec=`: the app is not installed when this program is missing.
    pub try_exec: String,
    /// The `.desktop` file (for `%k`); empty when unknown.
    pub file: String,
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
    let (e, hidden) = parse_any(text)?;
    (!hidden && !e.exec.is_empty()).then_some(e)
}

/// Parses an application entry including hidden ones and ones without `Exec=` (D-Bus activatable);
/// the flag says `NoDisplay`/`Hidden`. `None` for non-applications or entries without a name.
pub fn parse_any(text: &str) -> Option<(AppEntry, bool)> {
    let mut e = AppEntry::default();
    let (mut name, mut have_exec) = (false, false);
    let (mut in_entry, mut is_app, mut hidden) = (false, true, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("Name=") {
                if !name {
                    e.name = v.to_string();
                    name = true;
                }
            } else if let Some(v) = line.strip_prefix("Exec=") {
                if !have_exec {
                    e.exec = v.to_string();
                    have_exec = true;
                }
            } else if let Some(v) = line.strip_prefix("Icon=") {
                e.icon = v.to_string();
            } else if let Some(v) = line.strip_prefix("Path=") {
                e.path = v.to_string();
            } else if let Some(v) = line.strip_prefix("TryExec=") {
                e.try_exec = v.to_string();
            } else if let Some(v) = line.strip_prefix("Terminal=") {
                e.terminal = v == "true";
            } else if let Some(v) = line.strip_prefix("Type=") {
                is_app = v == "Application";
            } else if line == "NoDisplay=true" || line == "Hidden=true" {
                hidden = true;
            }
        }
    }
    (is_app && name).then_some((e, hidden))
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
            if let Some(mut entry) = read_entry(&path) {
                entry.file = path.to_string_lossy().into_owned();
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
            let mut p = l.splitn(7, '\t');
            let (name, exec, icon) = (p.next()?.to_string(), p.next()?.to_string(), p.next()?.to_string());
            // Older caches have three columns only.
            let mut rest = || p.next().unwrap_or("").to_string();
            let (path, terminal, try_exec, file) = (rest(), rest() == "1", rest(), rest());
            Some(AppEntry { name, exec, icon, path, terminal, try_exec, file })
        })
        .collect()
}

pub fn save_cache(apps: &[AppEntry]) {
    let Some(path) = cache_file() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String =
        apps.iter().map(|a| format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\n", esc(&a.name), esc(&a.exec), esc(&a.icon), esc(&a.path), u8::from(a.terminal), esc(&a.try_exec), esc(&a.file))).collect();
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
        AppEntry { name: name.into(), exec: "x".into(), ..Default::default() }
    }

    #[test]
    fn parses_a_normal_entry() {
        let t = "[Desktop Entry]\nType=Application\nName=Foot\nExec=foot %U\nIcon=foot\n[Desktop Action x]\nName=Other\n";
        assert_eq!(parse_entry(t), Some(AppEntry { name: "Foot".into(), exec: "foot %U".into(), icon: "foot".into(), ..Default::default() }));
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
