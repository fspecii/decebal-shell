//! Kickoff's Places tab, after plasma-desktop `applets/kickoff/PlacesPage.qml` (commit c68d33a9): a sidebar of Computer, History and Frequently Used
//! and a list for the one chosen. The lists are Plasma's models without Plasma:
//!
//! * Computer (`Kicker::ComputerModel`): "Show KRunner", System Settings when installed, then the places of `KFilePlacesModel` that are neither hidden
//!   nor fixed devices: Home, the user directories that exist, Trash, removable media. Network (`remote:/`) and Recent Files / Recent Locations
//!   (`recentlyused:/`) are KIO locations Decebal cannot open, so they are not listed (History covers recent files).
//! * History (`RecentUsageModel`, most recent first): the applications launched from Decebal and the documents of `recently-used.xbel`, by time.
//! * Frequently Used (`RecentUsageModel`, popular ordering): the applications launched most often.
//!
//! Nothing here runs when the launcher starts, while it paints, or on the interface's thread. The first time the Places tab opens, ONE worker thread reads
//! the lists; the tab shows what needs no file system (Show KRunner, Home, the applications of the history) and a "Reading places..." note at once, and the
//! interface never waits. Single flight: while a worker has not finished (even if it is stuck in a stalled mount or a stuck `stat`) no second one is ever
//! started, however often the tab is opened and closed, so threads cannot pile up. Leaving the tab asks the worker to stop (best effort: it checks between
//! steps; a blocked system call cannot be interrupted) and discards what it brings back. The answer lands only in this module's store, never in a page,
//! a query or a selection: the lists shown are always derived from the current state, so a late answer cannot be applied to a stale one.
//!
//! Reads are bounded before anything is allocated: regular files only (a FIFO or device is skipped, never opened), `user-dirs.dirs` at most
//! [`USER_DIRS_CAP`] bytes, `recently-used.xbel` at most [`XBEL_WINDOW`] bytes from each end, [`MAX_BOOKMARKS`] bookmarks looked at, [`MAX_STATS`]
//! existence checks, [`MAX_MEDIA`] devices.
//!
//! The completion is announced through [`set_waker`], the hook the launcher host (`Launcher::set_waker` / `tick`) calls the style with; until the host
//! forwards its waker (docs/plasma-theme-ports/kickoff-places-hook.md) the tab picks the answer up at the next input event instead of immediately.

use super::super::Entry;
use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const COMPUTER: usize = 0;
pub const HISTORY: usize = 1;
/// (title, icon) of the sidebar rows, `placesCategoryModel`.
pub const SIDEBAR: [(&str, &str); 3] = [("Computer", "computer"), ("History", "view-history"), ("Frequently Used", "clock")];

/// The most bytes read of `user-dirs.dirs`.
pub const USER_DIRS_CAP: u64 = 64 * 1024;
/// Bytes read from the start and from the end of `recently-used.xbel` (GTK keeps a few hundred items, well inside this).
pub const XBEL_WINDOW: u64 = 512 * 1024;
/// Bookmarks examined per window and the longest one considered (bytes).
pub const MAX_BOOKMARKS: usize = 400;
pub const MAX_BOOKMARK_BYTES: usize = 8 * 1024;
/// Existence checks (`stat`) made for history documents, and documents listed.
pub const MAX_STATS: usize = 120;
pub const MAX_DOCUMENTS: usize = 40;
/// Removable media listed and directory entries looked at.
pub const MAX_MEDIA: usize = 16;
const MAX_MEDIA_SCAN: usize = 64;

/// What the worker brings back.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Loaded {
    pub computer: Vec<Entry>,
    pub documents: Vec<Entry>,
}

// ------------------------------------------------------------------------------------------------------------------------ single-flight loading

/// One worker's mailbox. Written by the worker, read by the interface.
struct Slot {
    result: Mutex<Option<Loaded>>,
    done: AtomicBool,
    /// The user left the tab: stop early if you can, and nobody wants the answer.
    cancel: AtomicBool,
}

#[derive(Default)]
struct Store {
    /// The one worker that has not been harvested yet. While it is here no other is started.
    flight: Option<Arc<Slot>>,
    loaded: Option<Rc<Loaded>>,
    waker: Option<crate::window_app::Waker>,
    spawned: usize,
}

thread_local! {
    // the interface runs on one thread; nothing here is shared with the worker except its `Slot`
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Never asked for (or the worker thread could not be started).
    Idle,
    Loading,
    Ready,
}

/// Where the worker announces that it finished, so the host redraws without waiting for input.
pub fn set_waker(w: crate::window_app::Waker) {
    STORE.with(|s| s.borrow_mut().waker = Some(w));
}

/// Takes a finished worker's answer into the store (an abandoned one is dropped). True when the data changed.
pub fn poll() -> bool {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(slot) = s.flight.clone() else { return false };
        if !slot.done.load(Ordering::Acquire) {
            return false;
        }
        s.flight = None;
        if slot.cancel.load(Ordering::Acquire) {
            return false;
        }
        let answer = slot.result.lock().unwrap_or_else(|p| p.into_inner()).take();
        match answer {
            Some(l) => {
                s.loaded = Some(Rc::new(l));
                true
            }
            None => false,
        }
    })
}

/// Asks for the lists: starts the one worker when none is running and nothing is loaded, otherwise only says where things stand. Never waits.
pub fn request(worker: impl FnOnce(&AtomicBool) -> Loaded + Send + 'static) -> State {
    poll();
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        if s.loaded.is_some() {
            return State::Ready;
        }
        if let Some(slot) = &s.flight {
            // still running (perhaps stuck): the user is back, so its answer is wanted again; no second worker
            slot.cancel.store(false, Ordering::Release);
            return State::Loading;
        }
        let slot = Arc::new(Slot { result: Mutex::new(None), done: AtomicBool::new(false), cancel: AtomicBool::new(false) });
        let (mine, waker) = (slot.clone(), s.waker.clone());
        let started = std::thread::Builder::new().name("kickoff-places".into()).spawn(move || {
            // a panic must not leave the flight open for ever: it would block every later request
            let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker(&mine.cancel))).ok();
            *mine.result.lock().unwrap_or_else(|p| p.into_inner()) = loaded;
            mine.done.store(true, Ordering::Release);
            if let Some(w) = waker {
                w();
            }
        });
        match started {
            Ok(_) => {
                s.spawned += 1;
                s.flight = Some(slot);
                State::Loading
            }
            Err(_) => State::Idle,
        }
    })
}

/// The user left the tab: tell the running worker (if any) to stop and drop its answer.
pub fn cancel() {
    STORE.with(|s| {
        if let Some(slot) = &s.borrow().flight {
            slot.cancel.store(true, Ordering::Release);
        }
    });
}

pub fn state() -> State {
    STORE.with(|s| {
        let s = s.borrow();
        if s.loaded.is_some() {
            State::Ready
        } else if s.flight.is_some() {
            State::Loading
        } else {
            State::Idle
        }
    })
}

/// The loaded lists, if the worker has delivered them.
pub fn data() -> Option<Rc<Loaded>> {
    STORE.with(|s| s.borrow().loaded.clone())
}

#[cfg(test)]
pub fn spawned() -> usize {
    STORE.with(|s| s.borrow().spawned)
}

#[cfg(test)]
pub fn reset() {
    STORE.with(|s| *s.borrow_mut() = Store::default());
}

#[cfg(test)]
pub fn install(l: Loaded) {
    STORE.with(|s| s.borrow_mut().loaded = Some(Rc::new(l)));
}

/// A regular file's first `cap` bytes as text, or nothing: a FIFO, a device or a directory is never opened (a FIFO would block the worker), and no
/// more than `cap` bytes are ever read or allocated whatever the file's size.
pub fn read_capped(path: &Path, cap: u64) -> String {
    let Ok(meta) = std::fs::metadata(path) else { return String::new() };
    if !meta.is_file() {
        return String::new();
    }
    let Ok(f) = std::fs::File::open(path) else { return String::new() };
    read_capped_from(f, cap)
}

pub fn read_capped_from(r: impl Read, cap: u64) -> String {
    let mut buf = Vec::new();
    let _ = r.take(cap).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

fn open_path(p: &Path) -> Vec<String> {
    crate::launch::open_argv(&p.to_string_lossy())
}

/// `XDG_*_DIR` values of `user-dirs.dirs`, `$HOME` expanded.
fn user_dirs(text: &str, home: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for line in text.lines().take(256) {
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

/// The row for Home alone, with no file system access: what the tab shows when the worker did not answer in time.
pub fn fallback() -> Loaded {
    let Some(home) = home() else { return Loaded::default() };
    Loaded { computer: vec![Entry { name: "Home".into(), detail: home.display().to_string(), icon: "user-home".into(), open: open_path(&home), at: 0 }], documents: Vec::new() }
}

/// The Computer list: the places that exist on this machine.
pub fn computer(home: &Path, config: &Path, run_media: &Path, trash: &Path, stop: &dyn Fn() -> bool) -> Vec<Entry> {
    let place = |name: &str, icon: &str, path: &Path| Entry { name: name.into(), detail: path.display().to_string(), icon: icon.into(), open: open_path(path), at: 0 };
    let mut out = vec![place("Home", "user-home", home)];
    let dirs = user_dirs(&read_capped(&config.join("user-dirs.dirs"), USER_DIRS_CAP), home);
    // KFilePlacesModel's default order after Home
    for (key, title, icon) in [
        ("DESKTOP", "Desktop", "user-desktop"),
        ("DOCUMENTS", "Documents", "folder-documents"),
        ("DOWNLOAD", "Downloads", "folder-download"),
        ("MUSIC", "Music", "folder-music"),
        ("PICTURES", "Pictures", "folder-pictures"),
        ("VIDEOS", "Videos", "folder-videos"),
    ] {
        if stop() {
            return out;
        }
        let path = dirs.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone()).unwrap_or_else(|| home.join(title));
        if path.is_dir() && path != home {
            out.push(place(title, icon, &path));
        }
    }
    if stop() {
        return out;
    }
    if trash.is_dir() {
        let full = std::fs::read_dir(trash).is_ok_and(|mut d| d.next().is_some());
        out.push(place("Trash", if full { "user-trash-full" } else { "user-trash" }, trash));
    }
    // removable media mounted for this user (a fixed internal disk is filtered out in Plasma too)
    let mut media: Vec<PathBuf> = std::fs::read_dir(run_media).into_iter().flatten().flatten().take(MAX_MEDIA_SCAN).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    media.sort();
    for m in media.into_iter().take(MAX_MEDIA) {
        if stop() {
            break;
        }
        let name = m.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(place(&name, "drive-removable-media", &m));
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------------ recently-used.xbel

/// The five predefined XML entities and numeric character references (`&amp;`, `&#38;`, `&#x26;`) of an attribute value. Anything else stays as written.
pub fn xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let end = tail.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i);
        let decoded = end.and_then(|e| {
            let name = &tail[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name
                    .strip_prefix('#')
                    .and_then(|n| match n.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok(),
                        None => n.parse::<u32>().ok(),
                    })
                    .and_then(char::from_u32)
                    .filter(|c| *c != '\0'),
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
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
    let mut from = 0;
    // an attribute name must start at a word boundary (`href` is not the end of `xhref`)
    while let Some(i) = tag[from..].find(&key) {
        let at = from + i;
        if at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace() {
            let start = at + key.len();
            let end = tag[start..].find('"')?;
            return Some(&tag[start..start + end]);
        }
        from = at + key.len();
    }
    None
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// Seconds since the epoch of an `xbel` timestamp (`2026-10-31T11:37:00Z`, `...:00.5+02:00`). Every field is range-checked (a real calendar date, 1970-9999,
/// hour 0-23, minute 0-59, second 0-60, offset within a day) and every step of the arithmetic is checked, so no input can panic or wrap; `None` otherwise.
pub fn parse_time(t: &str) -> Option<u64> {
    let t = t.trim();
    if t.len() > 40 {
        return None;
    }
    let (date, time) = t.split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    if d.next().is_some() || !(1970..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=days_in_month(y, m)).contains(&day) {
        return None;
    }
    // the clock and an optional `Z` / `+hh:mm` / `-hh:mm` zone
    let split = time.find(['Z', 'z', '+', '-']).unwrap_or(time.len());
    let (clock, zone) = time.split_at(split);
    let mut c = clock.split(':');
    let hh: i64 = c.next()?.parse().ok()?;
    let mm: i64 = c.next()?.parse().ok()?;
    // RFC 3339, which `xbel` uses, always has the seconds
    let ss: i64 = c.next()?.split('.').next().filter(|v| !v.is_empty())?.parse().ok()?;
    if c.next().is_some() || !(0..24).contains(&hh) || !(0..60).contains(&mm) || !(0..=60).contains(&ss) {
        return None;
    }
    let offset: i64 = match zone.chars().next() {
        None | Some('Z' | 'z') => 0,
        Some(sign @ ('+' | '-')) => {
            let z = &zone[1..];
            let (oh, om) = match z.split_once(':') {
                Some((h, m)) => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?),
                None if z.len() == 4 => (z[..2].parse::<i64>().ok()?, z[2..].parse::<i64>().ok()?),
                None => return None,
            };
            if !(0..24).contains(&oh) || !(0..60).contains(&om) {
                return None;
            }
            let o = oh * 3600 + om * 60;
            if sign == '+' {
                o
            } else {
                -o
            }
        }
        Some(_) => return None,
    };
    // days from civil (Howard Hinnant), every operation checked
    let yy = if m <= 2 { y.checked_sub(1)? } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy.checked_sub(era.checked_mul(400)?)?;
    let doy = (153i64.checked_mul(if m > 2 { m - 3 } else { m + 9 })?.checked_add(2)?) / 5 + day - 1;
    let doe = yoe.checked_mul(365)?.checked_add(yoe / 4)?.checked_sub(yoe / 100)?.checked_add(doy)?;
    let days = era.checked_mul(146097)?.checked_add(doe)?.checked_sub(719468)?;
    let secs = days.checked_mul(86400)?.checked_add(hh * 3600 + mm * 60 + ss)?.checked_sub(offset)?;
    u64::try_from(secs).ok()
}

/// One bookmark of the file. `Err` = skip it.
fn bookmark(chunk: &str) -> Option<(String, u64, String)> {
    let end = chunk.find("</bookmark>").unwrap_or(chunk.len());
    let chunk = &chunk[..end];
    // GBookmarkFile marks an item private (`<bookmark:private/>`): other applications must not list it
    if chunk.contains("<bookmark:private") {
        return None;
    }
    let head = chunk.split('>').next().unwrap_or("");
    let href = xml_unescape(attr(head, "href")?);
    let path = percent_decode(href.strip_prefix("file://")?);
    if !path.starts_with('/') || path.contains('\0') {
        return None;
    }
    let at = attr(head, "modified").or_else(|| attr(head, "visited")).map(xml_unescape).and_then(|t| parse_time(&t)).unwrap_or(0);
    let mime = chunk
        .split("mime-type type=\"")
        .nth(1)
        .and_then(|m| m.split('"').next())
        .map(xml_unescape)
        .filter(|m| m.len() < 80 && m.contains('/') && !m.contains(char::is_whitespace))
        .unwrap_or_else(|| "text/plain".into());
    Some((path, at, mime))
}

/// Candidate documents of one window of text, newest first: no file system access, bounded by `MAX_BOOKMARKS` and `MAX_BOOKMARK_BYTES`.
fn candidates(window: &str, seen: &mut HashSet<String>, out: &mut Vec<(u64, String, String)>) {
    for chunk in window.split("<bookmark ").skip(1).take(MAX_BOOKMARKS) {
        if chunk.len() > MAX_BOOKMARK_BYTES {
            continue;
        }
        if let Some((path, at, mime)) = bookmark(chunk) {
            if seen.insert(path.clone()) {
                out.push((at, path, mime));
            }
        }
    }
}

/// The two windows of a file of this length: its first and its last bytes (the same one when it is small), so the result does not depend on which end
/// the writer appends to.
pub fn windows(len: u64) -> Vec<(u64, u64)> {
    if len <= 2 * XBEL_WINDOW {
        vec![(0, len)]
    } else {
        vec![(0, XBEL_WINDOW), (len - XBEL_WINDOW, XBEL_WINDOW)]
    }
}

fn read_window(f: &mut std::fs::File, at: u64, n: u64) -> String {
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(at)).is_ok() {
        let _ = f.by_ref().take(n).read_to_end(&mut buf);
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    // a window that starts inside a bookmark begins at the next one
    if at > 0 {
        text.find("<bookmark ").map(|i| text[i..].to_string()).unwrap_or_default()
    } else {
        text
    }
}

/// Documents from the text of the file (already limited), newest first, those `exists` accepts; at most `MAX_STATS` existence checks.
pub fn documents_from(windows: &[String], home: &Path, exists: &dyn Fn(&Path) -> bool) -> Vec<Entry> {
    let (mut seen, mut found) = (HashSet::new(), Vec::new());
    for w in windows {
        candidates(w, &mut seen, &mut found);
    }
    found.sort_by_key(|c| std::cmp::Reverse(c.0));
    let mut out = Vec::new();
    for (at, path, mime) in found.into_iter().take(MAX_STATS) {
        let p = PathBuf::from(&path);
        if !exists(&p) {
            continue;
        }
        let parent = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        let detail = home.to_str().and_then(|h| parent.strip_prefix(h)).map(|r| format!("~{r}")).unwrap_or(parent);
        out.push(Entry { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), detail, icon: mime.replace('/', "-"), open: open_path(&p), at });
        if out.len() == MAX_DOCUMENTS {
            break;
        }
    }
    out
}

/// Reads and parses `recently-used.xbel` within the bounds above.
pub fn recent_documents(file: &Path, home: &Path, stop: &dyn Fn() -> bool) -> Vec<Entry> {
    // regular files only: opening a FIFO here would block the worker for ever
    let Ok(meta) = std::fs::metadata(file) else { return Vec::new() };
    if !meta.is_file() {
        return Vec::new();
    }
    let Ok(mut f) = std::fs::File::open(file) else { return Vec::new() };
    let mut texts = Vec::new();
    for (at, n) in windows(meta.len()) {
        if stop() {
            return Vec::new();
        }
        texts.push(read_window(&mut f, at, n));
    }
    documents_from(&texts, home, &|p| !stop() && p.exists())
}

/// Everything the Places tab reads from disk. Runs on the worker; `cancel` is checked between steps.
pub fn load(cancel: &AtomicBool) -> Loaded {
    let stop = || cancel.load(Ordering::Acquire);
    let Some(home) = home() else { return Loaded::default() };
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let data = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let user = std::env::var("USER").unwrap_or_default();
    let media = PathBuf::from("/run/media").join(user);
    Loaded { computer: computer(&home, &config, &media, &data.join("Trash/files"), &stop), documents: recent_documents(&data.join("recently-used.xbel"), &home, &stop) }
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
        let list = computer(&home, &cfg, &media, &trash, &|| false);
        let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Home", "Desktop", "Documents", "Downloads", "Music", "Trash", "STICK"], "Pictures and Videos do not exist, Documents follows user-dirs.dirs");
        assert!(list[2].detail.ends_with("h/Docs") && list[5].icon == "user-trash" && list[6].icon == "drive-removable-media");
        std::fs::write(trash.join("old.txt"), "x").unwrap();
        assert_eq!(computer(&home, &cfg, &media, &trash, &|| false)[5].icon, "user-trash-full");
        assert!(list.iter().all(|e| !e.open.is_empty()), "every row opens something");
    }

    #[test]
    fn media_are_capped_and_a_hostile_user_dirs_file_costs_nothing() {
        let root = scratch("media");
        let (home, cfg, media, trash) = (root.join("h"), root.join("c"), root.join("m"), root.join("t"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cfg).unwrap();
        for i in 0..200 {
            std::fs::create_dir_all(media.join(format!("d{i:03}"))).unwrap();
        }
        std::fs::write(cfg.join("user-dirs.dirs"), "XDG_DESKTOP_DIR=\"/\"\n".repeat(200_000)).unwrap();
        let t = std::time::Instant::now();
        let list = computer(&home, &cfg, &media, &trash, &|| false);
        assert!(list.iter().filter(|e| e.icon == "drive-removable-media").count() <= MAX_MEDIA, "{}", list.len());
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    fn xbel(body: &str) -> String {
        format!("<?xml version=\"1.0\"?><xbel>{body}</xbel>")
    }

    #[test]
    fn recent_documents_come_newest_first_decoded_and_only_if_they_exist() {
        let root = scratch("recent");
        let home = root.join("h");
        std::fs::create_dir_all(home.join("my docs")).unwrap();
        std::fs::write(home.join("my docs/a b & c.txt"), "x").unwrap();
        std::fs::write(home.join("new.png"), "x").unwrap();
        let h = home.display();
        let text = xbel(&format!(
            r#"<bookmark href="file://{h}/my%20docs/a%20b%20&amp;%20c.txt" added="2026-10-01T10:00:00Z" modified="2026-10-02T10:00:00Z"><info><metadata><mime:mime-type type="text/plain"/></metadata></info></bookmark>
<bookmark href="file://{h}/new.png" modified="2026-10-30T12:30:15.123456Z"><info><metadata><mime:mime-type type="image/png"/></metadata></info></bookmark>
<bookmark href="file://{h}/gone.txt" modified="2026-10-31T00:00:00Z"/>
<bookmark href="https://example.org/" modified="2026-10-31T00:00:00Z"/>"#
        ));
        let docs = documents_from(&[text], &home, &|p| p.exists());
        assert_eq!(docs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["new.png", "a b & c.txt"], "entities and percent-encoding are both decoded");
        assert_eq!((docs[0].icon.as_str(), docs[1].detail.as_str()), ("image-png", "~/my docs"));
        assert!(docs[0].at > docs[1].at && docs[1].open.last().unwrap().ends_with("a b & c.txt"));
    }

    #[test]
    fn private_items_are_not_listed_and_numeric_entities_decode() {
        let h = Path::new("/h");
        let text = xbel(
            r#"<bookmark href="file:///h/secret.txt" modified="2026-10-02T10:00:00Z"><info><metadata><bookmark:private/></metadata></info></bookmark>
<bookmark href="file:///h/pub&#108;ic&#x2E;txt" modified="2026-10-03T10:00:00Z"><info><metadata/></info></bookmark>"#,
        );
        let docs = documents_from(&[text], h, &|_| true);
        assert_eq!(docs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["public.txt"], "GBookmarkFile's private flag hides an item from other applications");
        assert_eq!(xml_unescape("a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos; &#65;&#x42; &unknown; &#0; &#x110000; & &amp"), "a & b <c> \"d\" 'e' AB &unknown; &#0; &#x110000; & &amp");
    }

    #[test]
    fn a_huge_history_is_read_through_two_bounded_windows_and_finishes_quickly() {
        let root = scratch("huge");
        let (home, file) = (root.join("h"), root.join("recently-used.xbel"));
        std::fs::create_dir_all(&home).unwrap();
        // 60 MB: one real, newest item at each end among tens of thousands of junk ones and a few hostile ones
        let mut text = String::from("<?xml version=\"1.0\"?><xbel>");
        let (first, last) = (root.join("first.txt"), root.join("last.txt"));
        std::fs::write(&first, "x").unwrap();
        std::fs::write(&last, "x").unwrap();
        text += &format!("<bookmark href=\"file://{}\" modified=\"2026-10-05T00:00:00Z\"/>", first.display());
        while text.len() < 60 * 1024 * 1024 {
            text += "<bookmark href=\"file:///nope/junk&#x41;.txt\" modified=\"2026-13-45T25:61:61Z\"><info/></bookmark>\n";
        }
        text += &"<bookmark href=\"".repeat(3000);
        text += &"A".repeat(2 * 1024 * 1024);
        text += &format!("<bookmark href=\"file://{}\" modified=\"2026-10-06T00:00:00Z\"/></xbel>", last.display());
        std::fs::write(&file, &text).unwrap();
        let t = std::time::Instant::now();
        let docs = recent_documents(&file, &home, &|| false);
        let took = t.elapsed();
        assert_eq!(docs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["last.txt", "first.txt"], "both ends are read");
        assert!(took < Duration::from_secs(3), "{took:?}");
        assert_eq!(windows(text.len() as u64).iter().map(|w| w.1).sum::<u64>(), 2 * XBEL_WINDOW, "never more than two windows");
        assert_eq!(windows(100), vec![(0, 100)]);
    }

    #[test]
    fn malformed_files_never_panic_and_work_is_bounded() {
        // unterminated, binary, nested, an enormous attribute, a million tiny chunks
        let home = Path::new("/h");
        for junk in [
            "",
            "<bookmark ",
            "<bookmark href=\"",
            "<bookmark href=\"file://\"",
            "\u{0}\u{1}\u{2}<bookmark href=\"file:///a\u{0}b\" modified=\"x\"/>",
            "<bookmark href=\"file:///%\"/><bookmark href=\"file:///%4\"/><bookmark href=\"file:///%ZZ\"/>",
            "<bookmark xhref=\"file:///a\"/>",
        ] {
            let _ = documents_from(&[junk.to_string()], home, &|_| true);
        }
        let big = "<bookmark href=\"file:///x\"/>".repeat(1_000_000);
        let checks = std::cell::Cell::new(0usize);
        let docs = documents_from(&[big], home, &|_| {
            checks.set(checks.get() + 1);
            false
        });
        assert!(docs.is_empty() && checks.get() <= 1, "one distinct path is checked once: {}", checks.get());
        // many distinct paths: still at most MAX_STATS existence checks, and at most MAX_DOCUMENTS rows
        let many: String = (0..50_000).map(|i| format!("<bookmark href=\"file:///d/{i}.txt\" modified=\"2026-10-01T00:00:{:02}Z\"/>", i % 60)).collect();
        let counted = std::cell::Cell::new(0usize);
        let none = documents_from(&[many.clone()], home, &|_| {
            counted.set(counted.get() + 1);
            false
        });
        assert!(none.is_empty() && counted.get() <= MAX_STATS, "{}", counted.get());
        assert_eq!(documents_from(&[many], home, &|_| true).len(), MAX_DOCUMENTS);
        let huge_attr = format!("<bookmark href=\"file:///{}\"/>", "a".repeat(MAX_BOOKMARK_BYTES * 4));
        assert!(documents_from(&[huge_attr], home, &|_| true).is_empty(), "an oversized bookmark is skipped");
    }

    #[test]
    fn xbel_times_are_validated_calendar_dates_and_never_wrap_or_panic() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-10-31T11:37:00Z"), Some(1793446620));
        assert_eq!(parse_time("2024-02-29T23:59:59.5Z"), Some(1709251199));
        assert_eq!(parse_time("2026-10-31T13:37:00+02:00"), Some(1793446620), "the offset is applied");
        assert_eq!(parse_time("2026-10-31T09:37:00-0200"), Some(1793446620));
        assert_eq!(parse_time("9999-12-31T23:59:59Z"), Some(253402300799));
        for bad in [
            "garbage",
            "",
            "2026-13-01T00:00:00Z",
            "2026-00-10T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2025-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-10-31T24:00:00Z",
            "2026-10-31T23:60:00Z",
            "2026-10-31T23:59:61Z",
            "1969-12-31T23:59:59Z",
            "10000-01-01T00:00:00Z",
            "99999999999999999999-01-01T00:00:00Z",
            "2026-01-01T00:00:00+99:00",
            "2026-01-01T00:00:00+02:99",
            "2026-01-01T00:00:00Q",
            "2026-01-01",
            "2026-01-01T",
            "2026-01-01T00:00",
            "-1-01-01T00:00:00Z",
            "1970-01-01T00:00:00+01:00",
        ] {
            assert_eq!(parse_time(bad), None, "{bad:?}");
        }
        assert_eq!(parse_time("2000-02-29T00:00:00Z"), Some(951782400), "a leap year");
        // arbitrary byte soup of every length never panics
        let alphabet = ['0', '9', '-', ':', 'T', 'Z', '+', '.', ' ', '8', 'x', '\u{1F600}'];
        let mut state = 0x1234_5678_u64;
        for _ in 0..20_000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let n = (state >> 59) as usize + 1;
            let s: String = (0..n).map(|i| alphabet[((state >> ((i * 3) % 60)) as usize ^ i) % alphabet.len()]).collect();
            let _ = parse_time(&s);
        }
    }

    // ---------------------------------------------------------------------------------------------------- single flight, by handshake

    use std::sync::mpsc;
    use std::time::Duration;

    /// A waker that signals a channel: the deterministic "the worker finished" handshake (the timeout only turns a hang into a failure).
    fn waker() -> (crate::window_app::Waker, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        (Arc::new(move || drop(tx.lock().unwrap().send(()))), rx)
    }

    fn named(n: &str) -> Loaded {
        Loaded { computer: vec![Entry { name: n.into(), open: vec!["x".into()], ..Default::default() }], documents: Vec::new() }
    }

    const LONG: Duration = Duration::from_secs(20);

    #[test]
    fn however_often_the_tab_is_opened_while_a_worker_is_blocked_exactly_one_thread_exists() {
        reset();
        let (release, held) = mpsc::channel::<()>();
        assert_eq!(
            request(move |_| {
                drop(held.recv_timeout(LONG));
                named("first")
            }),
            State::Loading
        );
        for _ in 0..500 {
            // none of these closures may ever run: a second worker would panic the test thread's check below
            assert_eq!(request(|_| panic!("a second worker was started")), State::Loading);
            cancel();
        }
        assert_eq!(spawned(), 1, "one worker, however many times the tab was opened and closed");
        assert!(!poll() && data().is_none() && state() == State::Loading);
        drop(release);
    }

    #[test]
    fn the_worker_announces_itself_and_the_answer_lands_in_the_store_only() {
        reset();
        let (wake, woken) = waker();
        set_waker(wake);
        let (release, held) = mpsc::channel::<()>();
        request(move |_| {
            drop(held.recv_timeout(LONG));
            named("answer")
        });
        assert!(data().is_none() && state() == State::Loading, "nothing yet");
        release.send(()).unwrap();
        woken.recv_timeout(LONG).expect("the worker did not announce its answer");
        assert!(poll(), "the finished answer is taken in");
        assert_eq!(data().unwrap().computer[0].name, "answer");
        assert_eq!(state(), State::Ready);
        assert!(!poll(), "nothing more to take");
        assert_eq!(request(|_| panic!("loaded lists are not read again")), State::Ready);
        assert_eq!(spawned(), 1);
    }

    #[test]
    fn leaving_the_tab_stops_the_worker_and_drops_its_answer_and_the_next_opening_starts_a_fresh_one() {
        reset();
        let (wake, woken) = waker();
        set_waker(wake);
        let (started, running) = mpsc::channel::<()>();
        request(move |cancel| {
            started.send(()).unwrap();
            while !cancel.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            named("abandoned")
        });
        running.recv_timeout(LONG).unwrap();
        cancel();
        woken.recv_timeout(LONG).unwrap();
        assert!(!poll(), "an abandoned answer is not taken");
        assert!(data().is_none() && state() == State::Idle);
        assert_eq!(request(|_| named("fresh")), State::Loading);
        assert_eq!(spawned(), 2, "the stopped worker is finished, so one new one may start");
        woken.recv_timeout(LONG).unwrap();
        assert!(poll());
        assert_eq!(data().unwrap().computer[0].name, "fresh");
    }

    #[test]
    fn coming_back_before_the_worker_finishes_keeps_its_answer() {
        reset();
        let (wake, woken) = waker();
        set_waker(wake);
        let (release, held) = mpsc::channel::<()>();
        request(move |_| {
            drop(held.recv_timeout(LONG));
            named("kept")
        });
        cancel();
        assert_eq!(request(|_| panic!("no second worker")), State::Loading, "the same worker is wanted again");
        release.send(()).unwrap();
        woken.recv_timeout(LONG).unwrap();
        assert!(poll() && data().unwrap().computer[0].name == "kept");
    }

    #[test]
    fn a_worker_that_panics_frees_the_flight_and_delivers_nothing() {
        reset();
        let (wake, woken) = waker();
        set_waker(wake);
        request(|_| panic!("boom"));
        woken.recv_timeout(LONG).unwrap();
        assert!(!poll() && data().is_none() && state() == State::Idle);
        assert_eq!(request(|_| named("retry")), State::Loading);
        assert_eq!(spawned(), 2);
    }

    // ------------------------------------------------------------------------------------------------------- reads are bounded before allocation

    struct Endless {
        served: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.served.set(self.served.get() + buf.len());
            buf.fill(b'a');
            Ok(buf.len())
        }
    }

    #[test]
    fn a_reader_is_never_asked_for_more_than_the_cap() {
        let served = std::rc::Rc::new(std::cell::Cell::new(0));
        let text = read_capped_from(Endless { served: served.clone() }, 1000);
        assert_eq!(text.len(), 1000);
        assert!(served.get() <= 1000 + 32, "{} bytes were requested of an endless reader", served.get());
    }

    #[test]
    fn oversize_user_data_costs_at_most_the_cap_and_special_files_are_never_opened() {
        let root = scratch("cap");
        // a sparse 256 MiB user-dirs.dirs and a sparse 256 MiB history file
        let (big, history) = (root.join("user-dirs.dirs"), root.join("recently-used.xbel"));
        for f in [&big, &history] {
            let h = std::fs::File::create(f).unwrap();
            h.set_len(256 * 1024 * 1024).unwrap();
        }
        assert_eq!(read_capped(&big, USER_DIRS_CAP).len() as u64, USER_DIRS_CAP);
        assert!(read_capped(&root, USER_DIRS_CAP).is_empty(), "a directory is no file");
        assert!(read_capped(&root.join("missing"), USER_DIRS_CAP).is_empty());
        let home = root.join("h");
        std::fs::create_dir_all(&home).unwrap();
        let t = std::time::Instant::now();
        let list = computer(&home, &root, &root.join("no-media"), &root.join("no-trash"), &|| false);
        assert_eq!(list[0].name, "Home");
        let docs = recent_documents(&history, &home, &|| false);
        assert!(docs.is_empty() && t.elapsed() < Duration::from_secs(5));
        // a FIFO named like the file: opening it would block for ever, so it must be skipped without being opened
        let fifo = root.join("fifo.xbel");
        if std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
            assert!(read_capped(&fifo, USER_DIRS_CAP).is_empty());
            assert!(recent_documents(&fifo, &home, &|| false).is_empty());
        }
    }

    #[test]
    fn a_stop_request_ends_the_reads_between_steps() {
        let root = scratch("stop");
        let (home, cfg, media, trash) = (root.join("h"), root.join("c"), root.join("m"), root.join("t"));
        for d in ["h/Desktop", "h/Documents", "c", "m/STICK", "t"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let steps = std::cell::Cell::new(0usize);
        // stop after the first check: only Home is listed
        let list = computer(&home, &cfg, &media, &trash, &|| {
            steps.set(steps.get() + 1);
            true
        });
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Home"]);
        assert_eq!(steps.get(), 1);
    }
}
