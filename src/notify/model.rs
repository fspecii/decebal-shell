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

/// `~/.config/macshell/notifications.toml`. Schema (shared with macsettings):
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
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("macshell/notifications.toml")
}

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state"))).unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("macshell")
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
                "image-path" | "image_path" => {
                    if n.icon.is_empty() {
                        n.icon = v.as_str().unwrap_or("").to_string();
                    }
                }
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

#[derive(Default)]
pub struct History {
    /// Newest first.
    pub items: Vec<HistoryItem>,
    path: Option<PathBuf>,
}

impl History {
    pub fn load() -> History {
        let path = state_dir().join("notification-history.json");
        let items = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Vec<HistoryItem>>(&t).ok()).unwrap_or_default();
        History { items: items.into_iter().take(MAX_HISTORY).collect(), path: Some(path) }
    }

    pub fn in_memory() -> History {
        History::default()
    }

    /// Adds (or, for a replaced id, updates) an entry at the top.
    pub fn push(&mut self, item: HistoryItem) {
        self.items.retain(|i| i.id != item.id);
        self.items.insert(0, item);
        self.items.truncate(MAX_HISTORY);
        self.save();
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        let changed = self.items.len() != before;
        if changed {
            self.save();
        }
        changed
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.items) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
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
        h.clear();
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
