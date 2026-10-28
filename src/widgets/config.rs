//! Which widgets run where: the `widgets` object of `decebal.json`, and finding each instance's definition.
//!
//! ```json
//! "widgets": { "allow_exec": false,
//!              "items": [ { "use": "clock-card", "source": "sunset", "place": "desktop", "anchor": "top-right", "x": -40, "y": 60,
//!                           "monitor": "primary", "enabled": true, "options": {} } ] }
//! ```
//! `source` names the theme package a bare `use` lives in (set by `theme apply`); without it a bare name is looked up in the user's
//! widget folder (`$DECEBAL_WIDGETS`, else `~/.config/decebal/widgets`) and then among the shipped samples. `builtin:<id>` is a sample.

use super::dsl::{self, WidgetDef};
use super::{samples, Trust};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Desktop,
    Bar,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub use_: String,
    pub source: Option<String>,
    pub place: Place,
    pub anchor: String,
    pub x: i32,
    pub y: i32,
    pub monitor: String,
    pub enabled: bool,
    pub options: Map<String, Value>,
    pub slot: String,
    pub order: i32,
}

impl Instance {
    pub fn desktop(use_: &str, anchor: &str, x: i32, y: i32) -> Instance {
        Instance { use_: use_.into(), source: None, place: Place::Desktop, anchor: anchor.into(), x, y, monitor: "primary".into(), enabled: true, options: Map::new(), slot: "right".into(), order: 0 }
    }

    pub fn from_json(v: &Value) -> Option<Instance> {
        let o = v.as_object()?;
        let s = |k: &str| o.get(k).and_then(Value::as_str).map(String::from);
        let n = |k: &str| o.get(k).and_then(Value::as_i64).unwrap_or(0).clamp(-4000, 4000) as i32;
        Some(Instance {
            use_: s("use").filter(|u| !u.is_empty() && u.len() <= 80)?,
            source: s("source").filter(|x| crate::theme::engine::schema::valid_ident(x)),
            place: if s("place").as_deref() == Some("bar") { Place::Bar } else { Place::Desktop },
            anchor: s("anchor").filter(|a| crate::theme::engine::schema::ANCHORS.contains(&a.as_str())).unwrap_or_else(|| "top-left".into()),
            x: n("x"),
            y: n("y"),
            monitor: s("monitor").unwrap_or_else(|| "primary".into()),
            enabled: o.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            options: o.get("options").and_then(Value::as_object).cloned().unwrap_or_default(),
            slot: s("slot").unwrap_or_else(|| "right".into()),
            order: n("order"),
        })
    }

    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("use".into(), json!(self.use_));
        if let Some(s) = &self.source {
            o.insert("source".into(), json!(s));
        }
        o.insert("place".into(), json!(if self.place == Place::Bar { "bar" } else { "desktop" }));
        if self.place == Place::Desktop {
            o.insert("anchor".into(), json!(self.anchor));
            o.insert("x".into(), json!(self.x));
            o.insert("y".into(), json!(self.y));
            o.insert("monitor".into(), json!(self.monitor));
        } else {
            o.insert("slot".into(), json!(self.slot));
            o.insert("order".into(), json!(self.order));
        }
        if !self.enabled {
            o.insert("enabled".into(), json!(false));
        }
        if !self.options.is_empty() {
            o.insert("options".into(), Value::Object(self.options.clone()));
        }
        Value::Object(o)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetsConfig {
    pub allow_exec: bool,
    pub items: Vec<Instance>,
}

impl WidgetsConfig {
    /// From the parsed `decebal.json`. Bad items are skipped.
    pub fn from_json(root: &Value) -> WidgetsConfig {
        let w = root.get("widgets");
        WidgetsConfig {
            allow_exec: w.and_then(|w| w.get("allow_exec")).and_then(Value::as_bool).unwrap_or(false),
            items: w.and_then(|w| w.get("items")).and_then(Value::as_array).map(|a| a.iter().filter_map(Instance::from_json).take(32).collect()).unwrap_or_default(),
        }
    }

    pub fn load(shell_json: &Path) -> WidgetsConfig {
        WidgetsConfig::from_json(&crate::settings::io::json_load(shell_json))
    }

    /// The `widgets` object, keeping the `allow_exec` switch the user set.
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        if self.allow_exec {
            o.insert("allow_exec".into(), json!(true));
        }
        o.insert("items".into(), Value::Array(self.items.iter().map(Instance::to_json).collect()));
        Value::Object(o)
    }
}

pub fn user_dir() -> PathBuf {
    std::env::var_os("DECEBAL_WIDGETS")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(|x| PathBuf::from(x).join("decebal/widgets")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/decebal/widgets")))
        .unwrap_or_else(|| PathBuf::from("widgets"))
}

/// A widget definition and where it came from.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub def: WidgetDef,
    pub trust: Trust,
    /// Folder that `image` files of the widget are relative to.
    pub base: PathBuf,
}

/// Reads a small regular file, never through a symlink.
fn read_regular(path: &Path) -> Result<String, String> {
    let m = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !m.is_file() || m.len() > 64 << 10 {
        return Err(format!("{}: not a regular file of at most 64 KiB", path.display()));
    }
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The folder of an installed theme package: the user's, then the system's.
pub fn package_dir(themes_dir: &Path, id: &str) -> Option<PathBuf> {
    crate::theme::engine::schema::valid_ident(id).then_some(())?;
    [themes_dir.to_path_buf(), crate::theme::engine::store::system_dir()].into_iter().map(|d| d.join(id)).find(|d| d.join("theme.toml").is_file())
}

/// Finds and parses the widget an instance uses. Theme and sample widgets are parsed as [`Trust::Theme`]; the user's own as
/// [`Trust::UserExec`] when `allow_exec`, else [`Trust::User`].
pub fn resolve(inst: &Instance, themes_dir: &Path, allow_exec: bool) -> Result<Resolved, String> {
    let name = inst.use_.as_str();
    if let Some(id) = name.strip_prefix("builtin:") {
        let def = samples::source(id).ok_or_else(|| format!("no shipped widget `{id}`"))?;
        return Ok(Resolved { def: dsl::parse(def, Trust::Theme)?, trust: Trust::Theme, base: PathBuf::new() });
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err(format!("invalid widget name `{name}`"));
    }
    if let Some(src) = &inst.source {
        let dir = package_dir(themes_dir, src).ok_or_else(|| format!("theme `{src}` is not installed"))?;
        let text = read_regular(&dir.join("widgets").join(format!("{name}.toml")))?;
        return Ok(Resolved { def: dsl::parse(&text, Trust::Theme)?, trust: Trust::Theme, base: dir });
    }
    let user = user_dir();
    let file = user.join(format!("{name}.toml"));
    if std::fs::symlink_metadata(&file).is_ok() {
        let trust = if allow_exec { Trust::UserExec } else { Trust::User };
        let text = read_regular(&file)?;
        return Ok(Resolved { def: dsl::parse(&text, trust).map_err(|e| format!("{}: {e}", file.display()))?, trust, base: user });
    }
    if let Some(s) = samples::source(name) {
        return Ok(Resolved { def: dsl::parse(s, Trust::Theme)?, trust: Trust::Theme, base: PathBuf::new() });
    }
    Err(format!("widget `{name}` not found (looked in {} and the shipped samples)", user.display()))
}

/// Top-left corner of a `w` x `h` widget placed by anchor and offsets on the monitor (x, y, w, h). Positive offsets move
/// towards the screen centre from the anchor, negative ones outwards, so `anchor = "top-right", x = -40` is 40 px from the right edge.
pub fn place_at(anchor: &str, x: i32, y: i32, mon: (i32, i32, i32, i32), w: i32, h: i32) -> (i32, i32) {
    let (mx, my, mw, mh) = mon;
    let px = match anchor {
        "top-left" | "left" | "bottom-left" => mx + x,
        "top-right" | "right" | "bottom-right" => mx + mw - w + x,
        _ => mx + (mw - w) / 2 + x,
    };
    let py = match anchor {
        "top-left" | "top" | "top-right" => my + y,
        "bottom-left" | "bottom" | "bottom-right" => my + mh - h + y,
        _ => my + (mh - h) / 2 + y,
    };
    (px, py)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_round_trip_and_bad_ones_are_skipped() {
        let v = json!({"widgets": {"allow_exec": true, "items": [
            {"use": "clock-card", "source": "sunset", "anchor": "top-right", "x": -40, "y": 60},
            {"use": ""}, 5, {"use": "x", "anchor": "nowhere", "enabled": false, "options": {"a": 1}}]}});
        let c = WidgetsConfig::from_json(&v);
        assert!(c.allow_exec);
        assert_eq!(c.items.len(), 2);
        assert_eq!((c.items[0].anchor.as_str(), c.items[0].x, c.items[0].source.as_deref()), ("top-right", -40, Some("sunset")));
        assert_eq!((c.items[1].anchor.as_str(), c.items[1].enabled), ("top-left", false));
        assert_eq!(WidgetsConfig::from_json(&json!({"widgets": c.to_json()})), c);
        assert_eq!(WidgetsConfig::from_json(&json!({})), WidgetsConfig::default());
    }

    #[test]
    fn anchors_place_relative_to_the_right_edges() {
        let mon = (0, 0, 1600, 900);
        assert_eq!(place_at("top-left", 20, 30, mon, 200, 100), (20, 30));
        assert_eq!(place_at("top-right", -40, 60, mon, 200, 100), (1360, 60));
        assert_eq!(place_at("bottom-right", -10, -10, mon, 200, 100), (1390, 790));
        assert_eq!(place_at("center", 0, 0, mon, 200, 100), (700, 400));
        assert_eq!(place_at("top-left", 0, 0, (1920, 0, 1600, 900), 10, 10), (1920, 0), "second monitor offsets");
    }

    #[test]
    fn theme_package_widgets_are_parsed_as_theme_widgets() {
        let d = std::env::temp_dir().join(format!("decebal-wcfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("t1/widgets")).unwrap();
        std::fs::write(d.join("t1/theme.toml"), "format = 1\nname = \"t\"\n").unwrap();
        std::fs::write(d.join("t1/widgets/evil.toml"), "format = 1\n[root]\ntype = \"button\"\non_click = \"exec touch /tmp/pwned\"\n").unwrap();
        std::fs::write(d.join("t1/widgets/ok.toml"), "format = 1\n[root]\ntype = \"spacer\"\n").unwrap();
        let mut i = Instance::desktop("evil", "top-left", 0, 0);
        i.source = Some("t1".into());
        assert!(resolve(&i, &d, true).unwrap_err().contains("cannot run commands"), "even with allow_exec on: theme widgets never exec");
        i.use_ = "ok".into();
        assert_eq!(resolve(&i, &d, false).unwrap().trust, Trust::Theme);
        i.use_ = "../../etc/passwd".into();
        assert!(resolve(&i, &d, false).is_err());
        i.use_ = "builtin:clock-card".into();
        i.source = None;
        assert!(resolve(&i, &d, false).is_ok());
        i.source = Some("missing".into());
        i.use_ = "ok".into();
        assert!(resolve(&i, &d, false).unwrap_err().contains("not installed"));
        // a symlinked widget file is refused
        std::os::unix::fs::symlink("/etc/hostname", d.join("t1/widgets/link.toml")).unwrap();
        i.source = Some("t1".into());
        i.use_ = "link".into();
        assert!(resolve(&i, &d, false).is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}
