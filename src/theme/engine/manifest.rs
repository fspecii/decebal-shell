//! `theme.toml` (format 1): parsing and validation with line numbers, and conversion to the shell's [`Def`].
//!
//! [`analyze`] never stops at the first mistake: it returns every problem with the line of the offending key, so a theme author
//! fixes a file in one pass. [`parse_def`] is the `Result` form the loader (and `Def::parse`) uses.

use super::schema::{self, AssetKind, Elem, Field, Status, Ty};
use crate::theme::model::{Def, Kind, Rgb, Tokens, TOKENS, V};
use serde_json::{json, Map, Value};
use toml_edit::{Document, Item, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

/// One finding: where, which key, what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub level: Level,
    /// 1-based line in the file, 0 when unknown.
    pub line: usize,
    /// Dotted key path, e.g. `decoration.style` or `widgets[2].use`.
    pub key: String,
    pub msg: String,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lvl = if self.level == Level::Error { "error" } else { "warning" };
        if self.line > 0 {
            write!(f, "line {}: {lvl}: `{}`: {}", self.line, self.key, self.msg)
        } else {
            write!(f, "{lvl}: `{}`: {}", self.key, self.msg)
        }
    }
}

/// True when the text is a format-1 manifest (a top-level `format` key).
pub fn is_v1(text: &str) -> bool {
    text.lines().take(60).any(|l| {
        let l = l.trim_start();
        l.starts_with("format") && l[6..].trim_start().starts_with('=')
    })
}

struct Ctx<'a> {
    text: &'a str,
    diags: Vec<Diag>,
}

impl Ctx<'_> {
    fn line(&self, item: &Item) -> usize {
        item.span().map(|s| self.text[..s.start.min(self.text.len())].matches('\n').count() + 1).unwrap_or(0)
    }
    fn err(&mut self, item: &Item, key: &str, msg: impl Into<String>) {
        let line = self.line(item);
        self.diags.push(Diag { level: Level::Error, line, key: key.into(), msg: msg.into() });
    }
    fn warn(&mut self, item: &Item, key: &str, msg: impl Into<String>) {
        let line = self.line(item);
        self.diags.push(Diag { level: Level::Warning, line, key: key.into(), msg: msg.into() });
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn suggest(key: &str, known: impl Iterator<Item = String>) -> String {
    known.filter(|k| edit_distance(key, k) <= 2.max(key.len() / 4)).min_by_key(|k| edit_distance(key, k)).map(|k| format!(" (did you mean `{k}`?)")).unwrap_or_default()
}

/// Checks `item` against `ty`; returns the normalised JSON value.
fn check(ty: &Ty, item: &Item) -> Result<Value, String> {
    let v = item.as_value().ok_or("must be a value, not a table")?;
    match ty {
        Ty::Color => {
            let s = v.as_str().ok_or("must be a colour string like \"#1e1e1e\"")?;
            Rgb::parse(s).ok_or_else(|| format!("`{s}` is not a colour (#rgb or #rrggbb)"))?;
            Ok(json!(s))
        }
        Ty::Num(lo, hi) => {
            let n = v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).ok_or("must be a number")?;
            if !(*lo..=*hi).contains(&n) {
                return Err(format!("{n} is outside {lo}..{hi}"));
            }
            Ok(json!(n))
        }
        Ty::Int(lo, hi) => {
            let n = v.as_integer().ok_or("must be a whole number")?;
            if !(*lo..=*hi).contains(&n) {
                return Err(format!("{n} is outside {lo}..{hi}"));
            }
            Ok(json!(n))
        }
        Ty::Text(max) => {
            let s = v.as_str().ok_or("must be a string")?;
            if s.chars().count() > *max {
                return Err(format!("is longer than {max} characters"));
            }
            if s.chars().any(|c| c.is_control()) {
                return Err("contains a control character".into());
            }
            Ok(json!(s))
        }
        Ty::Bool => Ok(json!(v.as_bool().ok_or("must be true or false")?)),
        Ty::Tri => match (v.as_bool(), v.as_str()) {
            (Some(b), _) => Ok(json!(b)),
            (_, Some("auto")) => Ok(json!("auto")),
            _ => Err("must be true, false or \"auto\"".into()),
        },
        Ty::Choice(opts) => {
            let s = v.as_str().ok_or("must be a string")?;
            if !opts.contains(&s) {
                return Err(format!("must be one of {}", opts.join(", ")));
            }
            Ok(json!(s))
        }
        Ty::Ident => {
            let s = v.as_str().ok_or("must be a string")?;
            if !schema::valid_ident(s) {
                return Err(format!("`{s}` is not a valid id (lower-case letters, digits, `.`, `_`, `-`; at most 64)"));
            }
            Ok(json!(s))
        }
        Ty::Version => {
            let s = v.as_str().ok_or("must be a string like \"1.0.0\"")?;
            schema::parse_version(s).ok_or_else(|| format!("`{s}` is not a version (MAJOR.MINOR.PATCH)"))?;
            Ok(json!(s))
        }
        Ty::Asset(kind) => {
            let s = v.as_str().ok_or("must be a file path inside the package")?;
            check_asset(s, *kind)?;
            Ok(json!(s))
        }
        Ty::Assets(kind, max) => {
            let a = v.as_array().ok_or("must be a list of file paths")?;
            if a.len() > *max {
                return Err(format!("has {} entries, at most {max}", a.len()));
            }
            let mut out = Vec::new();
            for x in a.iter() {
                let s = x.as_str().ok_or("entries must be file paths")?;
                check_asset(s, *kind)?;
                out.push(json!(s));
            }
            Ok(Value::Array(out))
        }
        Ty::List(elem, max) => {
            let a = v.as_array().ok_or("must be a list")?;
            if a.len() > *max {
                return Err(format!("has {} entries, at most {max}", a.len()));
            }
            let mut out = Vec::new();
            for x in a.iter() {
                let s = x.as_str().ok_or("entries must be strings")?;
                match elem {
                    Elem::Text(n) if s.chars().count() > *n || s.chars().any(|c| c.is_control()) => return Err(format!("entry `{s}` is too long or has control characters")),
                    Elem::Ident if !valid_item(s) => return Err(format!("`{s}` is not an item name (letters, digits, `-`, `_`, `.`, or `widget:<id>`)")),
                    Elem::Choice(opts) if !opts.contains(&s) => return Err(format!("`{s}` is not one of {}", opts.join(", "))),
                    _ => {}
                }
                out.push(json!(s));
            }
            Ok(Value::Array(out))
        }
        Ty::Chain => {
            let names: Vec<String> = match (v.as_str(), v.as_array()) {
                (Some(s), _) => vec![s.to_string()],
                (_, Some(a)) => a.iter().map(|x| x.as_str().map(String::from).ok_or("entries must be font family names")).collect::<Result<_, _>>()?,
                _ => return Err("must be a font family or a list of families".into()),
            };
            if names.is_empty() || names.len() > 8 || names.iter().any(|n| n.trim().is_empty() || n.chars().count() > 80 || n.chars().any(|c| c.is_control())) {
                return Err("needs 1 to 8 family names of at most 80 characters".into());
            }
            Ok(json!(names))
        }
        Ty::Action => {
            let s = v.as_str().ok_or("must be a string")?;
            let ok = schema::safe_hot_actions().contains(&s.to_ascii_lowercase().replace('_', "-").as_str());
            if !ok {
                return Err(if s.to_ascii_lowercase().starts_with("custom") {
                    "themes cannot run commands; pick one of the built-in actions".into()
                } else {
                    format!("`{s}` is not an action; use one of {}", schema::safe_hot_actions().join(", "))
                });
            }
            Ok(json!(s.to_ascii_lowercase().replace('_', "-")))
        }
    }
}

fn valid_item(s: &str) -> bool {
    let id = s.strip_prefix("widget:").unwrap_or(s);
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Syntax of a package-relative asset path (existence and content are checked when the package directory is validated).
fn check_asset(s: &str, kind: AssetKind) -> Result<(), String> {
    super::archive::safe_rel_path(s)?;
    let ext = s.rsplit_once('.').map(|x| x.1.to_ascii_lowercase()).unwrap_or_default();
    if !kind.extensions().contains(&ext.as_str()) {
        return Err(format!("`{s}` must be a {} file", kind.extensions().join("/")));
    }
    Ok(())
}

fn token_kind(label: &str) -> Option<&'static Kind> {
    TOKENS.iter().find(|t| t.0 == label).map(|t| &t.1)
}

fn token_value(kind: &Kind, item: &Item) -> Result<V, String> {
    match kind {
        Kind::Color => check(&Ty::Color, item).map(|v| V::S(v.as_str().unwrap_or_default().to_string())),
        Kind::Num(lo, hi) => check(&Ty::Num(*lo, *hi), item).map(|v| V::N(v.as_f64().unwrap_or_default())),
        Kind::Str => check(&Ty::Text(80), item).map(|v| V::S(v.as_str().unwrap_or_default().to_string())),
        Kind::List => check(&Ty::Chain, item).map(|v| V::L(v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default())),
        Kind::Choice(c) => check(&Ty::Choice(c), item).map(|v| V::S(v.as_str().unwrap_or_default().to_string())),
    }
}

/// Sections whose keys are all listed in [`schema::FIELDS`].
const SECTIONS: &[&str] = &["integrity", "metrics", "font", "wallpaper", "decoration", "bar", "dock", "icons", "hot_corners", "control_center", "dashboard", "desktop", "launcher", "rdwm"];

const LEGACY: &[(&str, &str)] = &[
    ("inherit", "use `extends`"),
    ("[dark]", "variants live in `[colors.dark]` / `[colors.light]`"),
    ("[light]", "variants live in `[colors.dark]` / `[colors.light]`"),
    ("font.family", "the interface font is `font.ui`"),
];

/// The result of reading a manifest.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub def: Option<Def>,
    pub diags: Vec<Diag>,
}

impl Analysis {
    pub fn errors(&self) -> impl Iterator<Item = &Diag> {
        self.diags.iter().filter(|d| d.level == Level::Error)
    }
    pub fn ok(&self) -> bool {
        self.errors().next().is_none()
    }
}

fn read_tokens(cx: &mut Ctx, t: &Table, path: &str, want_color: bool, out: &mut Tokens) {
    for (k, item) in t.iter() {
        let full = format!("{path}.{k}");
        if want_color && item.is_table() && matches!(k, "dark" | "light") {
            continue;
        }
        match token_kind(k) {
            Some(kind) if matches!(kind, Kind::Color) == want_color => match token_value(kind, item) {
                Ok(v) => {
                    out.insert(k.to_string(), v);
                }
                Err(e) => cx.err(item, &full, e),
            },
            _ => {
                let known = TOKENS.iter().filter(|t| matches!(t.1, Kind::Color) == want_color).map(|t| t.0.to_string());
                cx.err(item, &full, format!("unknown {} key{}", if want_color { "colour" } else { "metric" }, suggest(k, known)))
            }
        }
    }
}

fn section_field(section: &str, key: &str) -> Option<&'static Field> {
    schema::field(&format!("{section}.{key}"))
}

/// Reads one `[section]` of ordinary keys into `extra`.
fn read_section(cx: &mut Ctx, name: &str, t: &Table, extra: &mut Map<String, Value>) {
    let mut sec = Map::new();
    for (k, item) in t.iter() {
        let full = format!("{name}.{k}");
        if let Some((_, hint)) = LEGACY.iter().find(|l| l.0 == full) {
            cx.err(item, &full, format!("not part of format 1: {hint}"));
            continue;
        }
        let Some(f) = section_field(name, k) else {
            // metrics also accepts every numeric token
            if name == "metrics" {
                continue;
            }
            let known = schema::FIELDS.iter().filter_map(|f| f.path.strip_prefix(&format!("{name}."))).map(String::from);
            cx.err(item, &full, format!("unknown key{}", suggest(k, known)));
            continue;
        };
        match check(&f.ty, item) {
            Ok(v) => {
                sec.insert(k.to_string(), v);
                if let Status::Pending(why) = f.status {
                    cx.warn(item, &full, format!("accepted but not applied yet: {why}"));
                }
            }
            Err(e) => cx.err(item, &full, e),
        }
    }
    if !sec.is_empty() {
        extra.insert(name.to_string(), Value::Object(sec));
    }
}

fn read_widgets(cx: &mut Ctx, item: &Item, extra: &mut Map<String, Value>) {
    let Some(arr) = item.as_array_of_tables() else {
        cx.err(item, "widgets", "must be a list of `[[widgets]]` tables");
        return;
    };
    if arr.len() > 32 {
        cx.err(item, "widgets", "more than 32 widget instances");
    }
    let mut out = Vec::new();
    for (i, t) in arr.iter().enumerate() {
        let mut w = Map::new();
        for (k, it) in t.iter() {
            let full = format!("widgets[{i}].{k}");
            let Some(f) = schema::WIDGET_FIELDS.iter().find(|f| f.path == k) else {
                cx.err(it, &full, format!("unknown key{}", suggest(k, schema::WIDGET_FIELDS.iter().map(|f| f.path.to_string()))));
                continue;
            };
            if k == "options" {
                match it.as_table_like() {
                    Some(opts) => {
                        let mut m = Map::new();
                        for (ok, ov) in opts.iter() {
                            let val = ov.as_value();
                            let j = val.and_then(|v| {
                                v.as_str().map(|s| json!(s)).or_else(|| v.as_integer().map(|n| json!(n))).or_else(|| v.as_float().map(|n| json!(n))).or_else(|| v.as_bool().map(|n| json!(n)))
                            });
                            match j {
                                Some(j) if m.len() < 32 => {
                                    m.insert(ok.to_string(), j);
                                }
                                _ => cx.err(ov, &format!("{full}.{ok}"), "options are strings, numbers or booleans (at most 32)"),
                            }
                        }
                        w.insert(k.into(), Value::Object(m));
                    }
                    None => cx.err(it, &full, "must be a table"),
                }
                continue;
            }
            if k == "use" {
                match it.as_str() {
                    Some(s) if s.strip_prefix("builtin:").unwrap_or(s).chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !s.is_empty() && s.len() <= 80 => {
                        w.insert(k.into(), json!(s));
                    }
                    _ => cx.err(it, &full, "must be a widget name (`clock-card`) or `builtin:<name>`"),
                }
                continue;
            }
            match check(&f.ty, it) {
                Ok(v) => {
                    w.insert(k.into(), v);
                }
                Err(e) => cx.err(it, &full, e),
            }
        }
        if !w.contains_key("use") {
            cx.err(&Item::Table(t.clone()), &format!("widgets[{i}]"), "missing `use`");
        }
        out.push(Value::Object(w));
    }
    extra.insert("widgets".into(), Value::Array(out));
}

/// Maps the few keys that live in the legacy token tables so the existing `Def::resolve` sees them.
fn lift_tokens(extra: &Map<String, Value>, common: &mut Tokens) {
    let get = |s: &str, k: &str| extra.get(s).and_then(|v| v.get(k));
    let num = |s: &str, k: &str, t: &str, c: &mut Tokens| {
        if let Some(n) = get(s, k).and_then(Value::as_f64) {
            c.insert(t.into(), V::N(n));
        }
    };
    let text = |s: &str, k: &str, t: &str, c: &mut Tokens| {
        if let Some(n) = get(s, k).and_then(Value::as_str) {
            c.insert(t.into(), V::S(n.into()));
        }
    };
    text("font", "preset", "font_preset", common);
    for (role, token) in [("ui", "font_ui"), ("title", "font_title"), ("mono", "font_mono"), ("display", "font_display")] {
        if let Some(Value::Array(a)) = get("font", role) {
            let mut chain: Vec<String> = a.iter().filter_map(|x| x.as_str().map(String::from)).collect();
            // `fallback` follows every chain the file spells out
            if let Some(Value::Array(f)) = get("font", "fallback") {
                chain.extend(f.iter().filter_map(|x| x.as_str().map(String::from)));
            }
            common.insert(token.into(), V::L(chain));
        }
    }
    num("font", "scale", "font_scale", common);
    num("metrics", "blur", "blur", common);
    text("bar", "style", "bar_style", common);
    text("dashboard", "style", "dashboard_style", common);
    text("dashboard", "profile", "dashboard_profile", common);
    text("launcher", "style", "launcher_style", common);
    num("bar", "height", "bar_height", common);
    num("dock", "size", "dock_icon_size", common);
    num("dock", "roundness", "dock_roundness", common);
    num("decoration", "height", "titlebar_height", common);
    if let Some(s) = get("decoration", "style").and_then(Value::as_str).filter(|s| matches!(*s, "macos" | "classic")) {
        common.insert("titlebar_style".into(), V::S(s.into()));
    }
}

/// Reads and validates a format-1 manifest.
pub fn analyze(text: &str) -> Analysis {
    let mut cx = Ctx { text, diags: Vec::new() };
    let doc = match Document::parse(text.to_string()) {
        Ok(d) => d,
        Err(e) => {
            let line = e.span().map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1).unwrap_or(0);
            return Analysis { def: None, diags: vec![Diag { level: Level::Error, line, key: "toml".into(), msg: e.message().to_string() }] };
        }
    };
    let root = doc.as_table();
    let mut extra = Map::new();
    let mut meta = Map::new();
    let (mut name, mut description, mut extends) = (String::new(), String::new(), None);
    let (mut common, mut dark, mut light) = (Tokens::new(), None::<Tokens>, None::<Tokens>);
    let mut dark_default = true;
    let mut have_format = false;

    for (k, item) in root.iter() {
        match k {
            "colors" => {
                let Some(t) = item.as_table() else {
                    cx.err(item, "colors", "must be a table");
                    continue;
                };
                read_tokens(&mut cx, t, "colors", true, &mut common);
                for (variant, slot) in [("dark", &mut dark), ("light", &mut light)] {
                    if let Some(vt) = t.get(variant) {
                        let mut toks = Tokens::new();
                        match vt.as_table() {
                            Some(vt) => read_tokens(&mut cx, vt, &format!("colors.{variant}"), true, &mut toks),
                            None => cx.err(vt, &format!("colors.{variant}"), "must be a table"),
                        }
                        *slot = Some(toks);
                    }
                }
            }
            "metrics" => {
                let Some(t) = item.as_table() else {
                    cx.err(item, "metrics", "must be a table");
                    continue;
                };
                read_metric_tokens(&mut cx, t, &mut common);
                read_section(&mut cx, "metrics", t, &mut extra);
            }
            "widgets" => read_widgets(&mut cx, item, &mut extra),
            s if SECTIONS.contains(&s) => match item.as_table() {
                Some(t) => read_section(&mut cx, s, t, &mut extra),
                None => cx.err(item, s, "must be a table"),
            },
            "dark" | "light" if item.is_table() => cx.err(item, &format!("[{k}]"), "not part of format 1: variants live in `[colors.dark]` / `[colors.light]`"),
            _ => match schema::field(k) {
                Some(f) => match check(&f.ty, item) {
                    Ok(v) => match k {
                        "format" => have_format = true,
                        "name" => name = v.as_str().unwrap_or_default().to_string(),
                        "description" => description = v.as_str().unwrap_or_default().to_string(),
                        "extends" => extends = v.as_str().map(String::from),
                        "dark" => dark_default = v.as_bool().unwrap_or(true),
                        _ => {
                            meta.insert(k.to_string(), v);
                        }
                    },
                    Err(e) => cx.err(item, k, e),
                },
                None => {
                    let msg = LEGACY.iter().find(|l| l.0 == k).map(|l| format!("not part of format 1: {}", l.1)).unwrap_or_else(|| {
                        let known = schema::FIELDS
                            .iter()
                            .filter(|f| !f.path.contains('.'))
                            .map(|f| f.path.to_string())
                            .chain(SECTIONS.iter().map(|s| s.to_string()))
                            .chain(["colors".to_string(), "widgets".to_string()]);
                        format!("unknown key{}", suggest(k, known))
                    });
                    cx.err(item, k, msg)
                }
            },
        }
    }
    if !have_format {
        cx.diags.push(Diag { level: Level::Error, line: 0, key: "format".into(), msg: "missing; a theme package starts with `format = 1`".into() });
    }
    if name.trim().is_empty() {
        cx.diags.push(Diag { level: Level::Error, line: 0, key: "name".into(), msg: "required".into() });
    }
    if let Some(min) = meta.get("min_shell_version").and_then(Value::as_str).and_then(schema::parse_version) {
        let cur = schema::parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0));
        if min > cur {
            cx.diags.push(Diag {
                level: Level::Warning,
                line: 0,
                key: "min_shell_version".into(),
                msg: format!("needs decebal {}.{}.{} or newer (this is {})", min.0, min.1, min.2, env!("CARGO_PKG_VERSION")),
            });
        }
    }
    if extra.get("desktop").and_then(|v| v.get("widgets")).and_then(Value::as_str) == Some("none") && extra.get("widgets").and_then(Value::as_array).is_some_and(|items| !items.is_empty()) {
        cx.diags.push(Diag { level: Level::Error, line: 0, key: "desktop.widgets".into(), msg: "an explicit empty desktop profile cannot also declare widgets".into() });
    }
    if extra.get("bar").and_then(|b| b.get("style")).and_then(Value::as_str) == Some("dux") {
        if let Some(bar) = doc.get("bar").and_then(Item::as_table) {
            for slot in ["left", "center", "right"] {
                if let Some(list) = bar.get(slot).and_then(Item::as_array) {
                    for value in list.iter() {
                        if let Some(id) = value.as_str() {
                            if !crate::config::DUX_BAR_ITEMS.contains(&id) {
                                if let Some(item) = bar.get(slot) {
                                    cx.err(item, &format!("bar.{slot}"), format!("unknown native Dux item `{id}`; no arbitrary widgets or commands"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if !cx.diags.iter().any(|d| d.level == Level::Error) {
        // the shipped `[[widgets]]` and other sections are validated; now assemble
        extra.insert("meta".into(), Value::Object(meta));
        lift_tokens(&extra, &mut common);
        let def = Def { name, description, inherit: extends, dark_default, common, dark, light, extra };
        return Analysis { def: Some(def), diags: cx.diags };
    }
    Analysis { def: None, diags: cx.diags }
}

/// Numeric tokens of `[metrics]` (radius, alphas...); the schema-only keys (gap, blur) are read by `read_section`.
fn read_metric_tokens(cx: &mut Ctx, t: &Table, out: &mut Tokens) {
    for (k, item) in t.iter() {
        if section_field("metrics", k).is_some() {
            continue;
        }
        let full = format!("metrics.{k}");
        match token_kind(k) {
            Some(kind @ Kind::Num(..)) if !matches!(k, "font_scale" | "titlebar_height") => match token_value(kind, item) {
                Ok(v) => {
                    out.insert(k.to_string(), v);
                }
                Err(e) => cx.err(item, &full, e),
            },
            _ => {
                let known = TOKENS.iter().filter(|t| matches!(t.1, Kind::Num(..))).map(|t| t.0.to_string()).chain(["gap".to_string(), "blur".to_string()]);
                cx.err(item, &full, format!("unknown metric{}", suggest(k, known)))
            }
        }
    }
}

/// Validated `Def` of a format-1 manifest, errors joined for `Def::parse`.
pub fn parse_def(text: &str) -> Result<Def, String> {
    let a = analyze(text);
    match a.def {
        Some(d) => Ok(d),
        None => Err(a.errors().map(|d| d.to_string()).collect::<Vec<_>>().join("; ")),
    }
}

// ---------------------------------------------------------------- inheritance of the non-token sections

/// `child` over `base`: objects merge key by key, everything else (lists, scalars) is replaced; `meta` and `integrity`
/// belong to the child alone (a theme does not inherit its parent's version, screenshots or hash).
pub fn merge_extra(base: &Map<String, Value>, child: &Map<String, Value>) -> Map<String, Value> {
    fn deep(a: &mut Value, b: &Value) {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for (k, v) in y {
                    match x.get_mut(k) {
                        Some(slot) => deep(slot, v),
                        None => {
                            x.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            (slot, v) => *slot = v.clone(),
        }
    }
    let mut out = base.clone();
    out.shift_remove("meta");
    out.shift_remove("integrity");
    for (k, v) in child {
        match out.get_mut(k) {
            Some(slot) if k != "meta" && k != "integrity" => deep(slot, v),
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::model::Selection;

    pub const GOOD: &str = r##"
format = 1
id = "sunset"
name = "Sunset"
version = "1.2.0"
author = "Ada"
license = "MIT"
description = "Warm."
tags = ["warm", "dark"]
extends = "nord"

[colors.dark]
background = "#201018"
text = "#ffeedd"
accent = "#ff7849"

[colors.light]
background = "#fff4ec"
text = "#2a1810"
accent = "#c2410c"

[metrics]
radius = 14
dock_alpha = 0.7
gap = 6

[font]
ui = "Inter"
scale = 1.1

[decoration]
style = "windows"
height = 34
buttons = "right"

[bar]
style = "floating"
left = ["apple", "workspaces"]
right = ["widget:cpu-ring", "clock"]

[dock]
style = "mac"
size = 52

[[widgets]]
use = "clock-card"
place = "desktop"
anchor = "top-right"
x = -40
y = 60
[widgets.options]
format = "%H:%M"
"##;

    fn errs(text: &str) -> Vec<String> {
        analyze(text).errors().map(|d| d.to_string()).collect()
    }

    #[test]
    fn a_complete_manifest_parses_into_tokens_and_extras() {
        let a = analyze(GOOD);
        assert!(a.ok(), "{:?}", a.diags);
        let d = a.def.unwrap();
        assert_eq!((d.name.as_str(), d.inherit.as_deref()), ("Sunset", Some("nord")));
        let t = d.resolve("sunset", true, &Selection::default());
        assert_eq!((t.accent, t.radius, t.font_family.as_str(), t.bar_style.as_str(), t.dock_icon_size, t.titlebar_height), (Rgb(0xff, 0x78, 0x49), 14.0, "Inter", "floating", 52.0, 34));
        assert!((t.font_scale - 1.1).abs() < 1e-6 && (t.dock_alpha - 0.7).abs() < 1e-6);
        assert_eq!(d.extra["decoration"]["style"], "windows");
        assert_eq!(d.extra["bar"]["right"][0], "widget:cpu-ring");
        assert_eq!(d.extra["widgets"][0]["options"]["format"], "%H:%M");
        assert_eq!(d.extra["metrics"]["gap"], 6.0);
        assert_eq!(d.extra["meta"]["version"], "1.2.0");
        assert!(d.has_dark() && d.has_light());
        // pending keys are reported as warnings, not errors
        assert!(a.diags.iter().any(|d| d.key == "decoration.buttons" && d.level == Level::Warning));
    }

    #[test]
    fn errors_name_the_line_and_key() {
        let e = errs("format = 1\nname = \"x\"\n\n[colors.dark]\nbackground = \"red\"\naccent = \"#12\"\n");
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(e[0].starts_with("line 5:") && e[0].contains("colors.dark.background") && e[0].contains("not a colour"), "{}", e[0]);
        assert!(e[1].starts_with("line 6:") && e[1].contains("colors.dark.accent"));
        let e = errs("format = 1\nname = \"x\"\n[decoration]\nheight = 99\nstyl = \"x\"\n[bar]\nposition = \"middle\"\n");
        assert!(e[0].contains("line 4") && e[0].contains("outside 16..60"), "{e:?}");
        assert!(e[1].contains("line 5") && e[1].contains("did you mean `style`"), "{e:?}");
        assert!(e[2].contains("bar.position") && e[2].contains("one of top, bottom, left, right, split"), "{e:?}");
    }

    #[test]
    fn missing_format_name_and_syntax_errors() {
        assert!(errs("name = \"x\"\n").iter().any(|e| e.contains("`format`")));
        assert!(errs("format = 1\n").iter().any(|e| e.contains("`name`")));
        let e = errs("format = 1\nname = \n");
        assert!(e[0].contains("toml") && e[0].contains("line 2"), "{e:?}");
        assert!(errs("format = 2\nname = \"x\"\n")[0].contains("format"));
    }

    #[test]
    fn legacy_keys_get_a_hint_and_unknown_sections_fail() {
        let e = errs("format = 1\nname = \"x\"\ninherit = \"nord\"\n[dark]\ntext = \"#fff\"\n[font]\nfamily = \"X\"\n[wat]\na = 1\n");
        assert!(e.iter().any(|m| m.contains("use `extends`")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("[colors.dark]")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("font.ui")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("`wat`") && m.contains("unknown key")), "{e:?}");
    }

    #[test]
    fn control_center_section_is_validated() {
        let ok = analyze("format = 1\nname = \"x\"\n[control_center]\nenabled = true\nposition = \"bottom-right\"\nstyle = \"fluent\"\ntiles = [\"sound\", \"actions\"]\n");
        assert!(ok.ok(), "{:?}", ok.diags);
        let def = ok.def.unwrap();
        assert_eq!(def.extra["control_center"]["style"], "fluent");
        assert_eq!(def.extra["control_center"]["tiles"], json!(["sound", "actions"]));
        for (bad, key) in [
            ("enabled = \"yes\"", "control_center.enabled"),
            ("position = \"middle\"", "control_center.position"),
            ("style = \"glass\"", "control_center.style"),
            ("tiles = [\"sound\", \"wifi\"]", "control_center.tiles"),
            ("colour = \"red\"", "control_center.colour"),
        ] {
            let a = analyze(&format!("format = 1\nname = \"x\"\n[control_center]\n{bad}\n"));
            assert!(a.errors().any(|d| d.key.starts_with(key) || d.key == key), "{bad}: {:?}", a.diags);
        }
        // a theme without the section is valid: the shell default (present, every tile) applies
        assert!(analyze("format = 1\nname = \"x\"\n").ok());
    }

    #[test]
    fn themes_cannot_carry_commands() {
        let e = errs("format = 1\nname = \"x\"\n[hot_corners]\ntop_left = \"custom:rm -rf ~\"\ntop_right = \"lock\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("cannot run commands"));
        // no key in the format takes a command line
        for f in schema::FIELDS.iter().chain(schema::WIDGET_FIELDS) {
            let last = f.path.rsplit('.').next().unwrap_or("");
            assert!(!matches!(last, "exec" | "command" | "cmd" | "script" | "run" | "shell" | "spawn"), "{}", f.path);
        }
        let e = errs("format = 1\nname = \"x\"\n[rdwm]\nbind = \"x\"\n[[rdwm.rule]]\nx=1\n");
        assert!(!e.is_empty());
    }

    #[test]
    fn asset_paths_cannot_escape_or_have_wrong_types() {
        let e = errs("format = 1\nname = \"x\"\nscreenshots = [\"../../etc/passwd.png\", \"/abs.png\", \"a.exe\", \"ok/a.png\"]\n");
        assert!(e[0].contains("escapes"), "{e:?}");
        let e = errs("format = 1\nname = \"x\"\n[wallpaper]\nimage = \"w/a.svg\"\n");
        assert!(e[0].contains("png/jpg/jpeg/webp"), "{e:?}");
        let e = errs("format = 1\nname = \"x\"\n[font]\nfiles = [\"f/a.woff2\"]\n");
        assert!(e[0].contains("ttf/otf"), "{e:?}");
    }

    #[test]
    fn widget_instances_are_validated() {
        let e = errs("format = 1\nname = \"x\"\n[[widgets]]\nplace = \"desktop\"\n[[widgets]]\nuse = \"a b\"\n[[widgets]]\nuse = \"ok\"\nexec = \"ls\"\n");
        assert!(e.iter().any(|m| m.contains("widgets[0]") && m.contains("missing `use`")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("widgets[1].use")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("widgets[2].exec") && m.contains("unknown key")), "{e:?}");
    }

    #[test]
    fn extends_merges_extras_but_not_identity() {
        let base =
            analyze("format = 1\nid = \"base\"\nname = \"Base\"\nversion = \"1.0.0\"\nscreenshots = [\"a.png\"]\n[bar]\nstyle = \"floating\"\nleft = [\"a\", \"b\"]\n[dock]\nsize = 40\n").def.unwrap();
        let child = analyze("format = 1\nid = \"child\"\nname = \"Child\"\nversion = \"2.0.0\"\nextends = \"base\"\n[bar]\nleft = [\"c\"]\n").def.unwrap();
        let m = child.over(&base);
        assert_eq!(m.extra["bar"]["left"], json!(["c"]), "lists are replaced");
        assert_eq!(m.extra["bar"]["style"], "floating", "other keys are inherited");
        assert_eq!(m.extra["dock"]["size"], 40);
        assert_eq!(m.extra["meta"]["version"], "2.0.0");
        assert!(m.extra["meta"].get("screenshots").is_none(), "screenshots are the theme's own");
    }

    #[test]
    fn is_v1_detects_the_format_key_only_at_line_start() {
        assert!(is_v1("# c\nformat = 1\n"));
        assert!(is_v1("format=1"));
        assert!(!is_v1("name = \"x\"\n[metrics]\nformat_x = 1\n"));
        assert!(!is_v1("name = \"format = 1\"\n"));
    }
    #[test]
    fn native_dux_contract_is_opt_in_and_rejects_script_or_unknown_items() {
        let source = "format=1\nname=\"Dux native fixture\"\n[bar]\nstyle=\"dux\"\nheight=38\nmargin=8\nspacing=4\nradius=14\nleft=[\"launcherButton\",\"workspaceSwitcher\",\"focusedWindow\"]\ncenter=[\"music\",\"clock\",\"weather\"]\nright=[\"systemTray\",\"clipboard\",\"cpuUsage\",\"memUsage\",\"notificationButton\",\"battery\",\"controlCenterButton\"]\n[dashboard]\nstyle=\"dux\"\n";
        let a = analyze(source);
        assert!(a.diags.is_empty(), "{:?}", a.diags);
        let def = a.def.unwrap();
        let theme = def.resolve("fixture", true, &crate::theme::model::Selection::default());
        assert_eq!(theme.bar_style, "dux");
        assert_eq!(theme.bar_height, 38.0);
        assert_eq!(theme.dashboard_style, "dux");
        for bad in ["exec", "widget:untrusted", "typo"] {
            let invalid = analyze(&source.replace("launcherButton", bad));
            assert!(invalid.def.is_none());
            assert!(invalid.diags.iter().any(|d| d.key == "bar.left"));
        }
        // Earlier styles retain their previously accepted declarative lists.
        assert!(analyze(&source.replace("style=\"dux\"", "style=\"floating\"").replace("launcherButton", "legacy-item").replace("[dashboard]\nstyle=\"floating\"", "[dashboard]\nstyle=\"default\""))
            .def
            .is_some());
    }
}
