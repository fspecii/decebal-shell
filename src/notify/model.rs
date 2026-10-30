//! Notification data: parsing the `Notify` call, markup stripping, the `notifications.toml` settings
//! file (shared with the settings app) and the persisted history list.

use crate::dbus::Value;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const MAX_HISTORY: usize = 100;
/// Longest edge accepted for an `image-data` hint.
const MAX_IMAGE_EDGE: i64 = 1024;

// ------------------------------------------------------------------ settings

/// `~/.config/decebal/notifications.toml`. Schema (shared with decsettings):
/// `dnd = false`, `banner_seconds = 5`, `muted_apps = []`.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub dnd: bool,
    pub banner_seconds: u32,
    pub muted_apps: Vec<String>,
    mtime: Option<SystemTime>,
}

impl Default for Config {
    fn default() -> Self {
        Self { dnd: false, banner_seconds: 5, muted_apps: Vec::new(), mtime: None }
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("decebal/notifications.toml")
}

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("decebal")
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else { return c };
        if let Some(b) = doc.get("dnd").and_then(|v| v.as_bool()) {
            c.dnd = b;
        }
        if let Some(n) = doc.get("banner_seconds").and_then(|v| v.as_integer()) {
            c.banner_seconds = n.clamp(1, 120) as u32;
        }
        if let Some(a) = doc.get("muted_apps").and_then(|v| v.as_array()) {
            c.muted_apps = a.iter().filter_map(|v| v.as_str()).map(|s| s.to_lowercase()).collect();
        }
        c
    }

    pub fn load() -> Config {
        let path = config_path();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let mut c = std::fs::read_to_string(&path).map(|t| Config::parse(&t)).unwrap_or_default();
        c.mtime = mtime;
        c
    }

    /// Re-reads the file when it changed on disk since the last load.
    pub fn refresh(&mut self) {
        let now = std::fs::metadata(config_path()).and_then(|m| m.modified()).ok();
        if now != self.mtime {
            *self = Config::load();
        }
    }

    pub fn is_muted(&self, app: &str, entry: &str) -> bool {
        let (a, e) = (app.to_lowercase(), entry.to_lowercase());
        self.muted_apps.iter().any(|m| *m == a || (!e.is_empty() && *m == e))
    }

    /// Writes `dnd` back, keeping every other key and comment the settings app may have put there.
    pub fn save_dnd(on: bool) -> std::io::Result<()> {
        let path = config_path();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut doc = text.parse::<toml_edit::DocumentMut>().unwrap_or_default();
        doc["dnd"] = toml_edit::value(on);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, doc.to_string())?;
        std::fs::rename(&tmp, &path)
    }
}

// ------------------------------------------------------------- notification

/// Raw `image-data` hint: straight (non-premultiplied) RGBA, 8 bits per channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: u32,
    pub replaces: u32,
    pub app: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    /// (key, label) pairs; the key "default" is the banner click.
    pub actions: Vec<(String, String)>,
    /// 0 low, 1 normal, 2 critical.
    pub urgency: u8,
    /// -1 server default, 0 never, else milliseconds.
    pub timeout_ms: i32,
    pub entry: String,
    pub image: Option<Image>,
    pub transient: bool,
    pub resident: bool,
}

impl Notification {
    /// None when the banner stays until dismissed.
    pub fn lifetime(&self, default_secs: u32) -> Option<std::time::Duration> {
        if self.urgency >= 2 && self.timeout_ms <= 0 || self.timeout_ms == 0 {
            return None;
        }
        let ms = if self.timeout_ms > 0 { self.timeout_ms as u64 } else { u64::from(default_secs) * 1000 };
        Some(std::time::Duration::from_millis(ms.min(10 * 60 * 1000)))
    }

    pub fn deadline(&self, default_secs: u32) -> Option<Instant> {
        self.lifetime(default_secs).map(|d| Instant::now() + d)
    }
}

/// Removes tags and decodes the handful of entities the notification markup subset uses.
pub fn plain_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' if chars.peek().is_some_and(|n| n.is_ascii_alphabetic() || *n == '/') => {
                in_tag = true;
                // <br> is a line break, everything else vanishes
                let rest: String = chars.clone().take(3).collect();
                if rest.eq_ignore_ascii_case("br>") || rest.eq_ignore_ascii_case("br/") {
                    out.push('\n');
                }
            }
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&amp;", "&")
}

fn image_from_struct(v: &Value) -> Option<Image> {
    let Value::Struct(f) = v else { return None };
    let (w, h, stride, alpha, bits, ch) = (f.first()?.as_int()?, f.get(1)?.as_int()?, f.get(2)?.as_int()?, f.get(3)?.as_bool()?, f.get(4)?.as_int()?, f.get(5)?.as_int()?);
    let Some(Value::Bytes(data)) = f.get(6) else { return None };
    if bits != 8 || !(ch == 3 || ch == 4) || w <= 0 || h <= 0 || w > MAX_IMAGE_EDGE || h > MAX_IMAGE_EDGE || stride < w * ch {
        return None;
    }
    let (w, h, stride, ch) = (w as usize, h as usize, stride as usize, ch as usize);
    if data.len() < stride * (h - 1) + w * ch {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let p = &data[y * stride + x * ch..];
            rgba.extend_from_slice(&[p[0], p[1], p[2], if alpha && ch == 4 { p[3] } else { 255 }]);
        }
    }
    Some(Image { w: w as u32, h: h as u32, rgba })
}

/// Builds a notification from the arguments of `Notify(susssasa{sv}i)`; `id` is assigned by the caller.
pub fn parse_notify(args: &[Value]) -> Option<Notification> {
    if args.len() != 8 {
        return None;
    }
    let mut n = Notification {
        id: 0,
        replaces: args[1].as_u32()?,
        app: args[0].as_str()?.to_string(),
        icon: args[2].as_str()?.to_string(),
        summary: plain_text(args[3].as_str()?),
        body: plain_text(args[4].as_str()?),
        actions: Vec::new(),
        urgency: 1,
        timeout_ms: args[7].as_int()? as i32,
        entry: String::new(),
        image: None,
        transient: false,
        resident: false,
    };
    if let Value::Array(a) = &args[5] {
        for pair in a.chunks_exact(2) {
            if let (Some(k), Some(l)) = (pair[0].as_str(), pair[1].as_str()) {
                n.actions.push((k.to_string(), l.to_string()));
            }
        }
    }
    if let Value::Dict(hints) = &args[6] {
        let mut legacy_image = None;
        for (k, v) in hints {
            let inner = if let Value::Variant(i) = v { &**i } else { v };
            match k.as_str().unwrap_or("") {
                "urgency" => n.urgency = v.as_int().unwrap_or(1).clamp(0, 2) as u8,
                "desktop-entry" => n.entry = v.as_str().unwrap_or("").trim_end_matches(".desktop").to_string(),
                "transient" => n.transient = v.as_bool().unwrap_or(false),
                "resident" => n.resident = v.as_bool().unwrap_or(false),
                "image-data" => n.image = image_from_struct(inner),
                "image_data" | "icon_data" => legacy_image = legacy_image.or_else(|| image_from_struct(inner)),
                "image-path" | "image_path" if n.icon.is_empty() => n.icon = v.as_str().unwrap_or("").to_string(),
                _ => {}
            }
        }
        if n.image.is_none() {
            n.image = legacy_image;
        }
    }
    Some(n)
}

// ------------------------------------------------------------------ history

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryItem {
    pub id: u32,
    pub app: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    #[serde(default)]
    pub entry: String,
    /// Unix seconds.
    pub time: u64,
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl HistoryItem {
    pub fn from(n: &Notification) -> HistoryItem {
        HistoryItem { id: n.id, app: n.app.clone(), icon: n.icon.clone(), summary: n.summary.clone(), body: n.body.clone(), entry: n.entry.clone(), time: unix_now() }
    }
}

#[path = "history_store.rs"]
mod history_store;
#[derive(Default)]
pub struct History {
    /// Newest first, including unsaved changes when persistence fails.
    pub items: Vec<HistoryItem>,
    path: Option<PathBuf>,
    base: Vec<HistoryItem>,
    upserts: Vec<HistoryItem>,
    removed: std::collections::BTreeMap<u32, HistoryItem>,
    clearing: bool,
    error: Option<String>,
}
impl History {
    pub fn load() -> Self {
        Self::at_path(state_dir().join("notification-history.json"))
    }
    fn at_path(path: PathBuf) -> Self {
        let result = history_store::Store::open(&path).and_then(|s| s.read());
        match result {
            Ok(items) => Self { base: items.clone(), items, path: Some(path), ..Self::default() },
            Err(error) => Self { path: Some(path), error: Some(error), ..Self::default() },
        }
    }
    pub fn in_memory() -> Self {
        Self::default()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn pending(&self) -> bool {
        self.clearing || !self.upserts.is_empty() || !self.removed.is_empty()
    }
    pub fn clear_pending(&self) -> bool {
        self.clearing
    }
    /// Incoming delivery remains available even when durable history is unavailable.
    pub fn push(&mut self, item: HistoryItem) {
        let item = bounded_item(item);
        self.items.retain(|i| i.id != item.id);
        self.items.insert(0, item.clone());
        self.items.truncate(MAX_HISTORY);
        self.removed.remove(&item.id);
        self.upserts.retain(|i| i.id != item.id);
        self.upserts.insert(0, item);
        self.upserts.truncate(MAX_HISTORY);
        let _ = self.retry();
    }
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        if before == self.items.len() {
            return false;
        }
        self.upserts.retain(|i| i.id != id);
        if let Some(base) = self.base.iter().find(|i| i.id == id) {
            self.removed.insert(id, base.clone());
        }
        let _ = self.retry();
        true
    }
    /// An unsuccessful clear keeps its explicit intent pending for Retry, but never
    /// replaces a corrupt/unsafe file. The UI must report that the clear is unsaved.
    pub fn clear(&mut self) -> Result<(), String> {
        self.items.clear();
        self.upserts.clear();
        self.removed.clear();
        self.clearing = true;
        self.retry()
    }
    pub fn retry(&mut self) -> Result<(), String> {
        let result = self.persist();
        self.error = result.as_ref().err().cloned();
        result
    }
    fn persist(&mut self) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            self.base = self.items.clone();
            self.upserts.clear();
            self.removed.clear();
            self.clearing = false;
            return Ok(());
        };
        let store = history_store::Store::open(&path)?;
        let mut disk = store.read()?;
        if self.clearing {
            disk.clear();
        } else {
            disk.retain(|i| self.removed.get(&i.id) != Some(i));
        }
        let mut used: std::collections::BTreeSet<_> = disk.iter().chain(self.items.iter()).chain(self.upserts.iter()).map(|i| i.id).collect();
        for item in self.upserts.iter().rev() {
            if let Some(old) = disk.iter_mut().find(|i| i.id == item.id) {
                if self.base.iter().find(|i| i.id == item.id) != Some(old) && old != item {
                    // Recovery can reveal an older disk entry using an active RAM ID.
                    // Rekey only the old record; active notification IDs stay stable.
                    let id = (1..=u32::MAX).find(|id| !used.contains(id)).ok_or("No history ID is available for recovery")?;
                    used.insert(id);
                    old.id = id;
                }
            }
            disk.retain(|i| i.id != item.id);
            disk.insert(0, item.clone());
        }
        disk.truncate(MAX_HISTORY);
        // Retry after a load error with no mutations is a safe reload, not an overwrite.
        if self.pending() {
            store.write(&disk)?;
        }
        self.items = disk;
        self.base = self.items.clone();
        self.upserts.clear();
        self.removed.clear();
        self.clearing = false;
        Ok(())
    }
}
fn shorten(s: &mut String, limit: usize) {
    if s.len() <= limit {
        return;
    }
    let mut end = limit.saturating_sub(3).min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push('…');
}
fn bounded_item(mut item: HistoryItem) -> HistoryItem {
    for (s, n) in [(&mut item.app, 256), (&mut item.icon, 1024), (&mut item.summary, 1024), (&mut item.body, 4096), (&mut item.entry, 256)] {
        shorten(s, n);
    }
    // JSON escaping can expand control characters six-fold. Bound encoded size as
    // well as source text, without ever splitting a UTF-8 character.
    while serde_json::to_vec(&item).is_ok_and(|v| v.len() > 8192) {
        let s = if item.body.len() > 64 {
            &mut item.body
        } else if item.summary.len() > 64 {
            &mut item.summary
        } else if item.icon.len() > 64 {
            &mut item.icon
        } else if item.app.len() > 64 {
            &mut item.app
        } else {
            &mut item.entry
        };
        shorten(s, (s.len() / 2).max(4));
    }
    item
}

/// "now", "5m ago", "2h ago", "3d ago".
pub fn ago(then: u64, now: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbus::{Message, Writer};

    #[test]
    fn config_defaults_and_overrides() {
        assert_eq!(Config::parse(""), Config::default());
        let c = Config::parse("dnd = true\nbanner_seconds = 9\nmuted_apps = [\"Spotify\", \"org.x.Y\"]\n");
        assert!(c.dnd);
        assert_eq!(c.banner_seconds, 9);
        assert!(c.is_muted("spotify", ""));
        assert!(c.is_muted("Other", "org.x.Y"));
        assert!(!c.is_muted("Other", ""));
        assert_eq!(Config::parse("banner_seconds = 0").banner_seconds, 1);
        assert_eq!(Config::parse("this is = not [toml").banner_seconds, 5);
    }

    #[test]
    fn markup_is_stripped() {
        assert_eq!(plain_text("<b>Hi</b> &amp; <a href=\"x\">there</a>"), "Hi & there");
        assert_eq!(plain_text("a < b and c > d"), "a < b and c > d");
        assert_eq!(plain_text("one<br>two"), "one\ntwo");
    }

    fn notify_args(timeout: i32, urgency: Option<u8>) -> Vec<Value> {
        let mut w = Writer::default();
        w.string("Mail");
        w.u32(0);
        w.string("mail-icon");
        w.string("New <b>mail</b>");
        w.string("From Ann");
        w.array(4, |w| {
            for s in ["default", "Open", "reply", "Reply"] {
                w.string(s);
            }
        });
        w.array(8, |w| {
            if let Some(u) = urgency {
                w.align(8);
                w.string("urgency");
                w.sig("y");
                w.u8(u);
            }
            w.align(8);
            w.string("desktop-entry");
            w.sig("s");
            w.string("org.mail.App.desktop");
        });
        w.u32(timeout as u32);
        Message::call("d", "/", "i", "Notify", "susssasa{sv}i", w.buf).args().unwrap()
    }

    #[test]
    fn parses_a_full_notify_call() {
        let n = parse_notify(&notify_args(-1, Some(2))).unwrap();
        assert_eq!((n.app.as_str(), n.icon.as_str(), n.summary.as_str(), n.body.as_str()), ("Mail", "mail-icon", "New mail", "From Ann"));
        assert_eq!(n.actions, vec![("default".into(), "Open".into()), ("reply".into(), "Reply".into())]);
        assert_eq!((n.urgency, n.timeout_ms, n.entry.as_str()), (2, -1, "org.mail.App"));
        assert!(parse_notify(&[]).is_none());
    }

    #[test]
    fn lifetimes_follow_urgency_and_timeout() {
        let mut n = parse_notify(&notify_args(-1, None)).unwrap();
        assert_eq!(n.lifetime(5), Some(std::time::Duration::from_secs(5)));
        n.timeout_ms = 1500;
        assert_eq!(n.lifetime(5), Some(std::time::Duration::from_millis(1500)));
        n.timeout_ms = 0;
        assert_eq!(n.lifetime(5), None);
        n.timeout_ms = -1;
        n.urgency = 2;
        assert_eq!(n.lifetime(5), None, "critical stays until dismissed");
    }

    #[test]
    fn image_data_validation() {
        let good = Value::Struct(vec![Value::I32(1), Value::I32(1), Value::I32(3), Value::Bool(false), Value::I32(8), Value::I32(3), Value::Bytes(vec![10, 20, 30])]);
        assert_eq!(image_from_struct(&good).unwrap().rgba, vec![10, 20, 30, 255]);
        let short = Value::Struct(vec![Value::I32(2), Value::I32(2), Value::I32(8), Value::Bool(true), Value::I32(8), Value::I32(4), Value::Bytes(vec![0; 5])]);
        assert!(image_from_struct(&short).is_none());
        let huge = Value::Struct(vec![Value::I32(100000), Value::I32(1), Value::I32(4), Value::Bool(true), Value::I32(8), Value::I32(4), Value::Bytes(vec![])]);
        assert!(image_from_struct(&huge).is_none());
    }

    #[test]
    fn history_replaces_by_id_and_is_bounded() {
        let mut h = History::in_memory();
        let item = |id| HistoryItem { id, app: "a".into(), icon: String::new(), summary: format!("s{id}"), body: String::new(), entry: String::new(), time: 0 };
        for i in 0..(MAX_HISTORY as u32 + 20) {
            h.push(item(i));
        }
        assert_eq!(h.items.len(), MAX_HISTORY);
        h.push(item(110));
        assert_eq!(h.items[0].id, 110);
        assert_eq!(h.items.iter().filter(|i| i.id == 110).count(), 1);
        assert!(h.remove(110) && !h.remove(110));
        h.clear().unwrap();
        assert!(h.items.is_empty());
    }

    #[test]
    fn relative_times() {
        assert_eq!(ago(100, 110), "now");
        assert_eq!(ago(0, 300), "5m ago");
        assert_eq!(ago(0, 7300), "2h ago");
        assert_eq!(ago(0, 200_000), "2d ago");
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("decebal-history-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> PathBuf {
            self.0.join("decebal/notification-history.json")
        }
        fn history(&self) -> History {
            History::at_path(self.path())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            if self.path().parent().is_some_and(|p| p.is_dir()) {
                let _ = fs::set_permissions(self.path().parent().unwrap(), fs::Permissions::from_mode(0o700));
            }
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn item(id: u32, summary: &str) -> HistoryItem {
        HistoryItem { id, app: "Fixture".into(), icon: String::new(), summary: summary.into(), body: "Private body".into(), entry: String::new(), time: 123 }
    }
    fn disk(path: &std::path::Path, items: &[HistoryItem]) {
        fs::write(path, serde_json::to_vec(items).unwrap()).unwrap();
    }
    #[test]
    fn private_atomic_history_survives_restart_and_ignores_legacy_temporary_file() {
        let t = Temp::new();
        let mut h = t.history();
        assert!(h.error().is_none());
        let legacy = t.path().with_extension("json.tmp");
        fs::write(&legacy, b"unrelated draft").unwrap();
        h.push(item(1, "Saved"));
        assert!(h.error().is_none());
        assert!(!h.pending());
        assert_eq!(fs::metadata(t.path()).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(t.path().parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::read(&legacy).unwrap(), b"unrelated draft");
        assert_eq!(t.history().items, h.items);
        assert!(!fs::read_dir(t.path().parent().unwrap()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".notification-history-")));
    }
    #[test]
    fn corrupt_history_is_preserved_and_retry_merges_repaired_id_collision() {
        let t = Temp::new();
        let _ = t.history();
        fs::write(t.path(), b"private sentinel broken json").unwrap();
        let mut h = t.history();
        assert!(h.error().unwrap().contains("valid JSON"));
        h.push(item(1, "New live record"));
        assert!(h.pending());
        assert!(!h.error().unwrap().contains("sentinel"));
        assert_eq!(fs::read(t.path()).unwrap(), b"private sentinel broken json");
        disk(&t.path(), &[item(1, "Recovered older record")]);
        h.retry().unwrap();
        assert_eq!(h.items[0].id, 1);
        assert_eq!(h.items[0].summary, "New live record");
        assert_eq!(h.items[1].summary, "Recovered older record");
        assert_ne!(h.items[1].id, 1);
        assert!(!h.pending());
        assert!(h.error().is_none());
        assert_eq!(t.history().items, h.items);
    }
    #[test]
    fn symbolic_file_directory_and_nonregular_targets_are_never_followed() {
        let t = Temp::new();
        let mut h = t.history();
        let victim = t.0.join("victim");
        fs::write(&victim, b"private untouched").unwrap();
        std::os::unix::fs::symlink(&victim, t.path()).unwrap();
        h.push(item(1, "Pending"));
        assert!(h.error().is_some());
        assert_eq!(fs::read(&victim).unwrap(), b"private untouched");
        fs::remove_file(t.path()).unwrap();
        h.retry().unwrap();
        assert_eq!(t.history().items.len(), 1);
        fs::remove_file(t.path()).unwrap();
        fs::create_dir(t.path()).unwrap();
        h.push(item(2, "Pending again"));
        assert!(h.error().is_some());
        assert!(t.path().is_dir());
        let t2 = Temp::new();
        let external = t2.0.join("external");
        fs::create_dir(&external).unwrap();
        std::os::unix::fs::symlink(&external, t2.0.join("decebal")).unwrap();
        assert!(t2.history().error().is_some());
        assert!(fs::read_dir(external).unwrap().next().is_none());
    }
    #[test]
    fn read_only_directory_keeps_failed_removal_and_clear_pending_until_retry() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let t = Temp::new();
        let mut h = t.history();
        h.push(item(1, "Existing"));
        let before = fs::read(t.path()).unwrap();
        let dir = t.path().parent().unwrap().to_path_buf();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(h.remove(1));
        assert!(h.pending());
        assert!(h.error().is_some());
        assert_eq!(fs::read(t.path()).unwrap(), before);
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o500);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        disk(&t.path(), &[item(1, "Existing"), item(2, "External record")]);
        h.retry().unwrap();
        assert_eq!(h.items, vec![item(2, "External record")]);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(h.clear().is_err());
        assert!(h.items.is_empty());
        assert!(h.clear_pending());
        assert_eq!(t.history().items.len(), 1);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        h.retry().unwrap();
        assert!(t.history().items.is_empty());
        assert!(!h.pending());
    }
    #[test]
    fn unreadable_file_is_preserved_and_private_permissions_do_not_add_owner_access() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let t = Temp::new();
        let mut h = t.history();
        h.push(item(1, "Saved"));
        let before = fs::read(t.path()).unwrap();
        fs::set_permissions(t.path(), fs::Permissions::from_mode(0o0)).unwrap();
        h.push(item(2, "Unsaved"));
        assert!(h.error().is_some());
        assert_eq!(fs::metadata(t.path()).unwrap().permissions().mode() & 0o777, 0);
        fs::set_permissions(t.path(), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::read(t.path()).unwrap(), before);
        h.retry().unwrap();
        assert_eq!(t.history().items.len(), 2);
    }
    #[test]
    fn oversized_and_duplicate_id_history_stays_untouched() {
        let t = Temp::new();
        let mut h = t.history();
        let oversized = vec![b'x'; (1 << 20) + 1];
        fs::write(t.path(), &oversized).unwrap();
        h.push(item(1, "Pending"));
        assert!(h.error().unwrap().contains("1 MiB"));
        assert_eq!(fs::read(t.path()).unwrap(), oversized);
        disk(&t.path(), &[item(1, "One"), item(1, "Two")]);
        let before = fs::read(t.path()).unwrap();
        assert!(h.retry().is_err());
        assert_eq!(fs::read(t.path()).unwrap(), before);
    }
    #[test]
    fn huge_remote_text_is_bounded_without_splitting_unicode_or_poisoning_history() {
        let t = Temp::new();
        let mut h = t.history();
        let mut huge = item(1, &"🦀".repeat(30_000));
        huge.body = "\u{0001}".repeat(30_000);
        h.push(huge);
        assert!(h.error().is_none());
        assert!(serde_json::to_vec(&h.items[0]).unwrap().len() <= 8192);
        assert!(h.items[0].summary.ends_with('…'));
        for id in 2..=110 {
            h.push(item(id, "Next"));
            assert!(h.error().is_none(), "push {id} failed: {:?}", h.error());
        }
        assert_eq!(t.history().items.len(), MAX_HISTORY);
        assert_eq!(h.items[0].id, 110);
        assert!(fs::metadata(t.path()).unwrap().len() < 1 << 20);
    }
    #[test]
    fn contended_lock_is_recoverable_instead_of_blocking_delivery() {
        let t = Temp::new();
        let mut h = t.history();
        let lock = history_store::Store::open(&t.path()).unwrap();
        h.push(item(1, "Pending"));
        assert!(h.error().is_some());
        assert!(h.pending());
        drop(lock);
        h.retry().unwrap();
        assert!(h.error().is_none());
    }
}
