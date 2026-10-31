//! Kickoff's Places tab, after plasma-desktop `applets/kickoff/PlacesPage.qml` (commit c68d33a9): a sidebar of Computer, History and Frequently Used
//! and a list for the one chosen. The lists are Plasma's models without Plasma:
//!
//! * Computer (`Kicker::ComputerModel`): "Show KRunner", System Settings when installed, then the places of `KFilePlacesModel` that are neither hidden
//!   nor fixed devices: Home, the user directories that exist, Trash, removable media. Network (`remote:/`) and Recent Files / Recent Locations
//!   (`recentlyused:/`) are KIO locations Decebal cannot open, so they are not listed (History covers recent files).
//! * History (`RecentUsageModel`, most recent first): the applications launched from Decebal and the documents of `recently-used.xbel`, by time.
//! * Frequently Used (`RecentUsageModel`, popular ordering): the applications launched most often.
//!
//! All files are read once when the launcher is built (`prepare`), never while painting.

use super::super::Entry;
use std::path::{Path, PathBuf};

pub const COMPUTER: usize = 0;
pub const HISTORY: usize = 1;
/// (title, icon) of the sidebar rows, `placesCategoryModel`.
pub const SIDEBAR: [(&str, &str); 3] = [("Computer", "computer"), ("History", "view-history"), ("Frequently Used", "clock")];

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

fn open_path(p: &Path) -> Vec<String> {
    crate::launch::open_argv(&p.to_string_lossy())
}

/// `XDG_*_DIR` values of `user-dirs.dirs`, `$HOME` expanded.
fn user_dirs(text: &str, home: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        let Some(name) = key.trim().strip_prefix("XDG_").and_then(|k| k.strip_suffix("_DIR")) else { continue };
        let value = value.trim().trim_matches('"');
        let path = match value.strip_prefix("$HOME") {
            Some(rest) => home.join(rest.trim_start_matches('/')),
            None => PathBuf::from(value),
        };
        out.push((name.to_string(), path));
    }
    out
}

/// The Computer list: the places that exist on this machine.
pub fn computer(home: &Path, config: &Path, run_media: &Path, trash: &Path) -> Vec<Entry> {
    let place = |name: &str, icon: &str, path: &Path| Entry { name: name.into(), detail: path.display().to_string(), icon: icon.into(), open: open_path(path), at: 0 };
    let mut out = vec![place("Home", "user-home", home)];
    let dirs = std::fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
    let dirs = user_dirs(&dirs, home);
    // KFilePlacesModel's default order after Home
    for (key, title, icon) in [
        ("DESKTOP", "Desktop", "user-desktop"),
        ("DOCUMENTS", "Documents", "folder-documents"),
        ("DOWNLOAD", "Downloads", "folder-download"),
        ("MUSIC", "Music", "folder-music"),
        ("PICTURES", "Pictures", "folder-pictures"),
        ("VIDEOS", "Videos", "folder-videos"),
    ] {
        let path = dirs.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone()).unwrap_or_else(|| home.join(title));
        if path.is_dir() && path != home {
            out.push(place(title, icon, &path));
        }
    }
    if trash.is_dir() {
        let full = std::fs::read_dir(trash).is_ok_and(|mut d| d.next().is_some());
        out.push(place("Trash", if full { "user-trash-full" } else { "user-trash" }, trash));
    }
    // removable media mounted for this user (a fixed internal disk is filtered out in Plasma too)
    let mut media: Vec<PathBuf> = std::fs::read_dir(run_media).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    media.sort();
    for m in media {
        let name = m.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(place(&name, "drive-removable-media", &m));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')?;
    Some(&tag[start..start + end])
}

/// Seconds since the epoch of an `xbel` timestamp (`2026-10-31T11:37:00Z`, fractions and offsets ignored).
pub fn parse_time(t: &str) -> Option<u64> {
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-').map(|v| v.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut c = time.trim_end_matches('Z').split(['+', '-']).next()?.split(':');
    let (hh, mm) = (c.next()?.parse::<i64>().ok()?, c.next()?.parse::<i64>().ok()?);
    let ss = c.next().map(|s| s.split('.').next().unwrap_or("0")).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    // days from civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    u64::try_from(days * 86400 + hh * 3600 + mm * 60 + ss).ok()
}

/// The local files of `recently-used.xbel`, newest first, those that still exist.
pub fn recent_documents(xbel: &str, home: &Path, limit: usize) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for chunk in xbel.split("<bookmark ").skip(1) {
        let head = chunk.split('>').next().unwrap_or("");
        let Some(href) = attr(head, "href").and_then(|h| h.strip_prefix("file://")) else { continue };
        let path = PathBuf::from(percent_decode(href));
        if !path.exists() || out.iter().any(|e| e.open.last().is_some_and(|p| Path::new(p) == path)) {
            continue;
        }
        let at = attr(head, "modified").or_else(|| attr(head, "visited")).and_then(parse_time).unwrap_or(0);
        let mime = chunk.split("mime-type type=\"").nth(1).and_then(|m| m.split('"').next()).unwrap_or("text/plain");
        let parent = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let detail = home.to_str().and_then(|h| parent.strip_prefix(h)).map(|r| format!("~{r}")).unwrap_or(parent);
        out.push(Entry { name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), detail, icon: mime.replace('/', "-"), open: open_path(&path), at });
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.at));
    out.truncate(limit);
    out
}

/// Everything the Places tab reads from disk, in one go.
pub fn load() -> (Vec<Entry>, Vec<Entry>) {
    let Some(home) = home() else { return (Vec::new(), Vec::new()) };
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let data = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let user = std::env::var("USER").unwrap_or_default();
    let media = PathBuf::from("/run/media").join(user);
    let docs = std::fs::read_to_string(data.join("recently-used.xbel")).unwrap_or_default();
    (computer(&home, &config, &media, &data.join("Trash/files")), recent_documents(&docs, &home, 40))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kde-places-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_computer_list_has_home_the_existing_user_dirs_trash_and_media_in_places_order() {
        let root = scratch("computer");
        let (home, cfg, media, trash) = (root.join("h"), root.join("c"), root.join("m"), root.join("t"));
        for d in ["h/Desktop", "h/Docs", "h/Downloads", "h/Music", "c", "m/STICK", "t"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(cfg.join("user-dirs.dirs"), "XDG_DOCUMENTS_DIR=\"$HOME/Docs\"\nXDG_PICTURES_DIR=\"$HOME/Pictures\"\n# c\n").unwrap();
        let list = computer(&home, &cfg, &media, &trash);
        let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Home", "Desktop", "Documents", "Downloads", "Music", "Trash", "STICK"], "Pictures and Videos do not exist, Documents follows user-dirs.dirs");
        assert!(list[2].detail.ends_with("h/Docs") && list[5].icon == "user-trash" && list[6].icon == "drive-removable-media");
        std::fs::write(trash.join("old.txt"), "x").unwrap();
        assert_eq!(computer(&home, &cfg, &media, &trash)[5].icon, "user-trash-full");
        assert!(list.iter().all(|e| !e.open.is_empty()), "every row opens something");
    }

    #[test]
    fn recent_documents_come_newest_first_decoded_and_only_if_they_exist() {
        let root = scratch("recent");
        let home = root.join("h");
        std::fs::create_dir_all(home.join("my docs")).unwrap();
        std::fs::write(home.join("my docs/a b.txt"), "x").unwrap();
        std::fs::write(home.join("new.png"), "x").unwrap();
        let h = home.display();
        let xbel = format!(
            r#"<?xml version="1.0"?><xbel>
<bookmark href="file://{h}/my%20docs/a%20b.txt" added="2026-10-01T10:00:00Z" modified="2026-10-02T10:00:00Z" visited="2026-10-02T10:00:00Z"><info><metadata><mime:mime-type type="text/plain"/></metadata></info></bookmark>
<bookmark href="file://{h}/new.png" modified="2026-10-30T12:30:15.123456Z"><info><metadata><mime:mime-type type="image/png"/></metadata></info></bookmark>
<bookmark href="file://{h}/gone.txt" modified="2026-10-31T00:00:00Z"/>
<bookmark href="https://example.org/" modified="2026-10-31T00:00:00Z"/></xbel>"#
        );
        let docs = recent_documents(&xbel, &home, 10);
        assert_eq!(docs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["new.png", "a b.txt"]);
        assert_eq!((docs[0].icon.as_str(), docs[1].detail.as_str()), ("image-png", "~/my docs"));
        assert!(docs[0].at > docs[1].at && docs[1].open.last().unwrap().ends_with("a b.txt"));
        assert_eq!(recent_documents(&xbel, &home, 1).len(), 1);
    }

    #[test]
    fn xbel_times_are_unix_seconds() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-10-31T11:37:00Z"), Some(1793446620));
        assert_eq!(parse_time("2024-02-29T23:59:59.5Z"), Some(1709251199));
        assert_eq!(parse_time("garbage"), None);
    }
}
