//! Files the Wallhaven pane keeps, all under XDG directories and none of them `macshell.json`
//! (which people export and share):
//!
//! * `$XDG_CONFIG_HOME/macshell/wallhaven.json` - filters and the "allow sketchy / NSFW" switch;
//! * `$XDG_CONFIG_HOME/macshell/wallhaven.key` - the API key, mode 0600, never shown, never logged;
//! * `$XDG_STATE_HOME/macshell/wallhaven-favourites.json` - the local favourites list;
//! * `$XDG_CACHE_HOME/macshell/wallhaven/` - the last search (offline view) and small thumbnails (bounded).

use super::parse::{valid_id, Wall};
use super::query::Prefs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// A string that must not leak: its `Debug` and `Display` never show the content.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Accepts the characters a wallhaven key has (letters and digits, 8-64 of them); anything else is refused so
    /// a pasted line with spaces, quotes or a newline can never end up in a header.
    pub fn new(s: &str) -> Option<Secret> {
        let s = s.trim();
        ((8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| Secret(s.to_string()))
    }

    /// For the one place that sends it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[derive(Debug, Clone)]
pub struct Dirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("macshell")
}

impl Dirs {
    pub fn discover() -> Dirs {
        Dirs { config: xdg("XDG_CONFIG_HOME", ".config"), state: xdg("XDG_STATE_HOME", ".local/state"), cache: xdg("XDG_CACHE_HOME", ".cache").join("wallhaven") }
    }

    pub fn under(root: &Path) -> Dirs {
        Dirs { config: root.join("config"), state: root.join("state"), cache: root.join("cache") }
    }

    pub fn prefs_file(&self) -> PathBuf {
        self.config.join("wallhaven.json")
    }
    pub fn key_file(&self) -> PathBuf {
        self.config.join("wallhaven.key")
    }
    pub fn favourites_file(&self) -> PathBuf {
        self.state.join("wallhaven-favourites.json")
    }
    pub fn last_search_file(&self) -> PathBuf {
        self.cache.join("last-search.json")
    }
    pub fn thumb_dir(&self) -> PathBuf {
        self.cache.join("thumbs")
    }
}

/// Writes `bytes` next to `path`, fsyncs and renames over it. The temp file is created with `mode` from the start
/// (so a secret is never readable by others, not even briefly) and the directory is created 0700.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let dir = path.parent().ok_or("no directory")?;
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let tmp = dir.join(format!(".{}.{}.tmp", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let res = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

// ---------------------------------------------------------------- preferences

pub fn load_prefs(d: &Dirs) -> Prefs {
    std::fs::read_to_string(d.prefs_file()).ok().and_then(|s| serde_json::from_str::<Prefs>(&s).ok()).unwrap_or_default().sanitised()
}

pub fn save_prefs(d: &Dirs, p: &Prefs) -> Result<(), String> {
    let text = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    write_atomic(&d.prefs_file(), format!("{text}\n").as_bytes(), 0o600)
}

// ---------------------------------------------------------------- API key

/// The saved key, if the file is present, private enough and well formed.
pub fn load_key(d: &Dirs) -> Option<Secret> {
    let path = d.key_file();
    let meta = std::fs::metadata(&path).ok()?;
    // refuse a key file other users can read: that is a leak waiting to happen
    if meta.permissions().mode() & 0o077 != 0 {
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Secret::new(&std::fs::read_to_string(&path).ok()?)
}

pub fn save_key(d: &Dirs, key: &str) -> Result<(), String> {
    let Some(k) = Secret::new(key) else {
        return Err("That does not look like a wallhaven API key (letters and digits only)".into());
    };
    write_atomic(&d.key_file(), format!("{}\n", k.expose()).as_bytes(), 0o600)
}

pub fn remove_key(d: &Dirs) {
    let _ = std::fs::remove_file(d.key_file());
}

// ---------------------------------------------------------------- favourites

pub const MAX_FAVOURITES: usize = 500;

pub fn load_favourites(d: &Dirs) -> Vec<Wall> {
    let mut v: Vec<Wall> = std::fs::read_to_string(d.favourites_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.retain(|w| valid_id(&w.id));
    v.truncate(MAX_FAVOURITES);
    v
}

pub fn save_favourites(d: &Dirs, list: &[Wall]) -> Result<(), String> {
    let text = serde_json::to_string(&list.iter().take(MAX_FAVOURITES).collect::<Vec<_>>()).map_err(|e| e.to_string())?;
    write_atomic(&d.favourites_file(), text.as_bytes(), 0o600)
}

/// Adds (newest first) or removes `wall`; returns whether it is a favourite afterwards.
pub fn toggle_favourite(list: &mut Vec<Wall>, wall: &Wall) -> bool {
    if let Some(i) = list.iter().position(|w| w.id == wall.id) {
        list.remove(i);
        false
    } else {
        list.insert(0, wall.clone());
        list.truncate(MAX_FAVOURITES);
        true
    }
}

// ---------------------------------------------------------------- offline cache

/// Search results kept for the offline view.
pub const MAX_CACHED_RESULTS: usize = 96;

pub fn save_last_search(d: &Dirs, walls: &[Wall]) {
    let list: Vec<&Wall> = walls.iter().take(MAX_CACHED_RESULTS).collect();
    if let Ok(text) = serde_json::to_string(&list) {
        let _ = write_atomic(&d.last_search_file(), text.as_bytes(), 0o600);
    }
}

pub fn load_last_search(d: &Dirs) -> Vec<Wall> {
    let mut v: Vec<Wall> = std::fs::read_to_string(d.last_search_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.retain(|w| valid_id(&w.id));
    v.truncate(MAX_CACHED_RESULTS);
    v
}

/// Thumbnails kept on disk and the most bytes they may take.
pub const MAX_THUMB_FILES: usize = 400;
pub const MAX_THUMB_BYTES: u64 = 24 * 1024 * 1024;

fn thumb_path(d: &Dirs, id: &str, large: bool) -> Option<PathBuf> {
    valid_id(id).then(|| d.thumb_dir().join(format!("{id}{}", if large { ".l" } else { ".s" })))
}

pub fn read_thumb(d: &Dirs, id: &str, large: bool) -> Option<Vec<u8>> {
    std::fs::read(thumb_path(d, id, large)?).ok()
}

pub fn write_thumb(d: &Dirs, id: &str, large: bool, bytes: &[u8]) {
    if let Some(p) = thumb_path(d, id, large) {
        let _ = write_atomic(&p, bytes, 0o600);
    }
}

/// Deletes the oldest thumbnails until at most `max_files` files and `max_bytes` bytes are left. Returns how many went.
pub fn prune_thumbs(dir: &Path, max_files: usize, max_bytes: u64) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    let (mut count, mut bytes, mut removed) = (0usize, 0u64, 0usize);
    for (_, len, path) in files {
        count += 1;
        bytes += len;
        if count > max_files || bytes > max_bytes {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
            count -= 1;
            bytes -= len;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::super::parse::{parse_search, tests::SEARCH};
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wh-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_key_file_is_private_and_the_key_never_prints() {
        let root = tmp("key");
        let d = Dirs::under(&root);
        assert!(load_key(&d).is_none());
        save_key(&d, "  AbCdEfGh12345678AbCdEfGh12345678\n").unwrap();
        let mode = std::fs::metadata(d.key_file()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dmode = std::fs::metadata(&d.config).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700, "the config folder it created is private too");
        let k = load_key(&d).unwrap();
        assert_eq!(k.expose(), "AbCdEfGh12345678AbCdEfGh12345678");
        assert!(!format!("{k:?}").contains("AbCd"), "Debug does not leak");
        // never in the prefs file
        save_prefs(&d, &Prefs { allow_mature: true, ..Prefs::default() }).unwrap();
        assert!(!std::fs::read_to_string(d.prefs_file()).unwrap().contains("AbCd"));
        // a file somebody made world-readable is tightened
        std::fs::set_permissions(d.key_file(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_key(&d).is_some());
        assert_eq!(std::fs::metadata(d.key_file()).unwrap().permissions().mode() & 0o777, 0o600);
        remove_key(&d);
        assert!(load_key(&d).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn odd_keys_are_refused() {
        let d = Dirs::under(&tmp("badkey"));
        for bad in ["", "short", "has space in it 12345678", "quote\"12345678", "new\nline12345678", &"a".repeat(65), "ключ1234567890"] {
            assert!(save_key(&d, bad).is_err(), "{bad:?}");
        }
        assert!(!d.key_file().exists());
        assert!(Secret::new("abcDEF1234").is_some());
    }

    #[test]
    fn prefs_roundtrip_and_a_broken_file_gives_defaults() {
        let d = Dirs::under(&tmp("prefs"));
        assert_eq!(load_prefs(&d), Prefs::default());
        let p = Prefs { query: "city".into(), allow_mature: true, ..Prefs::default() };
        save_prefs(&d, &p).unwrap();
        assert_eq!(load_prefs(&d), p);
        std::fs::write(d.prefs_file(), "{not json").unwrap();
        assert_eq!(load_prefs(&d), Prefs::default());
    }

    #[test]
    fn favourites_toggle_persist_and_are_capped() {
        let d = Dirs::under(&tmp("fav"));
        let walls = parse_search(SEARCH, 1).unwrap().walls;
        let mut list = load_favourites(&d);
        assert!(list.is_empty());
        assert!(toggle_favourite(&mut list, &walls[0]));
        assert!(toggle_favourite(&mut list, &walls[1]));
        assert_eq!(list[0].id, walls[1].id, "newest first");
        save_favourites(&d, &list).unwrap();
        assert_eq!(load_favourites(&d), list);
        assert!(!toggle_favourite(&mut list, &walls[1]));
        assert_eq!(list.len(), 1);
        // cap
        let mut big = Vec::new();
        for i in 0..(MAX_FAVOURITES + 30) {
            let mut w = walls[0].clone();
            w.id = format!("a{i}");
            toggle_favourite(&mut big, &w);
        }
        assert_eq!(big.len(), MAX_FAVOURITES);
        // junk ids in a hand-edited file are dropped
        let mut bad = walls[0].clone();
        bad.id = "../../etc".into();
        std::fs::write(d.favourites_file(), serde_json::to_string(&vec![bad, walls[2].clone()]).unwrap()).unwrap();
        assert_eq!(load_favourites(&d).len(), 1);
    }

    #[test]
    fn last_search_cache_is_bounded() {
        let d = Dirs::under(&tmp("last"));
        let w = parse_search(SEARCH, 1).unwrap().walls[0].clone();
        let many: Vec<Wall> = (0..300).map(|i| Wall { id: format!("x{i}"), ..w.clone() }).collect();
        save_last_search(&d, &many);
        assert_eq!(load_last_search(&d).len(), MAX_CACHED_RESULTS);
        std::fs::write(d.last_search_file(), "garbage").unwrap();
        assert!(load_last_search(&d).is_empty());
    }

    #[test]
    fn thumbnail_cache_prunes_oldest_first_by_count_and_by_size() {
        let root = tmp("thumbs");
        let d = Dirs::under(&root);
        for i in 0..10u64 {
            write_thumb(&d, &format!("t{i}"), false, &vec![0u8; 1000]);
            let f = thumb_path(&d, &format!("t{i}"), false).unwrap();
            let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i);
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(when).unwrap();
        }
        assert_eq!(prune_thumbs(&d.thumb_dir(), 6, u64::MAX), 4);
        assert!(read_thumb(&d, "t3", false).is_none() && read_thumb(&d, "t4", false).is_some(), "the oldest four went");
        assert_eq!(prune_thumbs(&d.thumb_dir(), 100, 2500), 4);
        assert!(read_thumb(&d, "t9", false).is_some() && read_thumb(&d, "t7", false).is_none());
        assert_eq!(prune_thumbs(&d.thumb_dir(), 100, 2500), 0);
        assert!(read_thumb(&d, "../x", false).is_none(), "ids are validated before they become paths");
        write_thumb(&d, "../evil", false, b"x");
        assert!(!root.join("evil.s").exists() && !root.join("cache/evil.s").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn discover_uses_xdg_and_falls_back_to_home() {
        // pure function of the environment; just check the shape without mutating it
        let d = Dirs::discover();
        assert!(d.config.ends_with("macshell") && d.state.ends_with("macshell") && d.cache.ends_with("macshell/wallhaven"));
        assert_ne!(d.config, d.state);
    }
}
