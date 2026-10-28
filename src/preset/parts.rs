//! Which part of the configuration a key belongs to, and the pure functions that cut a preset's share out of a
//! file ([`extract_json`], [`extract_toml`]), put it back ([`merge_json`], [`merge_toml`]), flatten a file for diffs
//! ([`flatten_json`], [`flatten_toml`]) and find or remove everything that makes the desktop run a command
//! ([`commands`], [`strip_commands`]).

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, Key, Table, TableLike};

/// One selectable share of the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    /// `decebal.json`: theme selection, menu bar and dock switches, workspaces, clock (everything not listed below).
    Shell,
    /// `decebal.json` -> `theme.font`, `theme.fontScale`.
    Fonts,
    /// `decebal.json` -> `icons` / `iconTheme` (only when the file has them).
    Icons,
    /// `decebal.json` -> `hot_corners`.
    HotCorners,
    /// `dock-settings.json`.
    Dock,
    /// `dock-pinned-macos.json`: the apps pinned to the dock.
    Pinned,
    /// rdwm `config.toml`: general, colours, bar, decoration, snap, layouts, rules.
    Rdwm,
    /// rdwm `[[bind]]` and `[[mode]]`.
    Keys,
    /// rdwm `[[autostart]]`, `[[plugin]]`, `[[scratchpad]]`: they run programs.
    Autostart,
    /// rdwm `[wallpaper]`.
    Wallpaper,
}

impl Part {
    pub const ALL: [Part; 10] = [Part::Shell, Part::Fonts, Part::Icons, Part::HotCorners, Part::Dock, Part::Pinned, Part::Rdwm, Part::Keys, Part::Autostart, Part::Wallpaper];

    pub fn id(self) -> &'static str {
        match self {
            Part::Shell => "shell",
            Part::Fonts => "fonts",
            Part::Icons => "icons",
            Part::HotCorners => "hotcorners",
            Part::Dock => "dock",
            Part::Pinned => "pinned",
            Part::Rdwm => "rdwm",
            Part::Keys => "keys",
            Part::Autostart => "autostart",
            Part::Wallpaper => "wallpaper",
        }
    }

    pub fn parse(s: &str) -> Option<Part> {
        let s = s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "");
        Part::ALL.into_iter().find(|p| p.id() == s)
    }

    pub fn title(self) -> &'static str {
        match self {
            Part::Shell => "Shell and theme",
            Part::Fonts => "Fonts",
            Part::Icons => "Icons",
            Part::HotCorners => "Hot corners",
            Part::Dock => "Dock",
            Part::Pinned => "Pinned apps",
            Part::Rdwm => "Window manager",
            Part::Keys => "Keyboard shortcuts",
            Part::Autostart => "Autostart and plugins",
            Part::Wallpaper => "Wallpaper",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Part::Shell => "Theme, mode, accent, menu bar and dock switches, workspaces, clock (decebal.json)",
            Part::Fonts => "Interface font and text size",
            Part::Icons => "Icon theme, when the shell has one",
            Part::HotCorners => "What the four screen corners do",
            Part::Dock => "Dock style, size, magnification, taskbar options (dock-settings.json)",
            Part::Pinned => "The apps pinned to the dock (machine specific)",
            Part::Rdwm => "Gaps, borders, titlebars, colours, layouts, window rules (config.toml)",
            Part::Keys => "rdwm [[bind]] and [[mode]] tables",
            Part::Autostart => "Programs rdwm starts: autostart, plugins, scratchpads (they run commands)",
            Part::Wallpaper => "The [wallpaper] table; the image can travel inside the preset",
        }
    }

    /// Ticked when a new preset is saved.
    pub fn default_on(self) -> bool {
        !matches!(self, Part::Pinned | Part::Autostart | Part::Icons)
    }

    /// The bundle file that carries this part.
    pub fn file(self) -> &'static str {
        match self {
            Part::Shell | Part::Fonts | Part::Icons | Part::HotCorners => "decebal.json",
            Part::Dock => "dock-settings.json",
            Part::Pinned => "dock-pinned-macos.json",
            Part::Rdwm | Part::Keys | Part::Autostart | Part::Wallpaper => "rdwm.toml",
        }
    }
}

/// A set of parts in `Part::ALL` order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartSet(pub Vec<Part>);

impl PartSet {
    pub fn new(parts: impl IntoIterator<Item = Part>) -> PartSet {
        let mut v: Vec<Part> = parts.into_iter().collect();
        v.sort();
        v.dedup();
        PartSet(v)
    }
    pub fn defaults() -> PartSet {
        PartSet::new(Part::ALL.into_iter().filter(|p| p.default_on()))
    }
    pub fn has(&self, p: Part) -> bool {
        self.0.contains(&p)
    }
    pub fn toggle(&mut self, p: Part, on: bool) {
        if on {
            if !self.has(p) {
                self.0.push(p);
                self.0.sort();
            }
        } else {
            self.0.retain(|x| *x != p);
        }
    }
    pub fn any_in(&self, file: &str) -> bool {
        self.0.iter().any(|p| p.file() == file)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn ids(&self) -> Vec<&'static str> {
        self.0.iter().map(|p| p.id()).collect()
    }
}

// ---------------------------------------------------------------- decebal.json

const ICON_KEYS: [&str; 5] = ["icons", "iconTheme", "icon_theme", "icon-theme", "iconPack"];
const FONT_KEYS: [&str; 3] = ["font", "fontScale", "font_scale"];

fn json_top_owner(key: &str) -> Part {
    if key == "hot_corners" {
        Part::HotCorners
    } else if ICON_KEYS.contains(&key) {
        Part::Icons
    } else {
        Part::Shell
    }
}

fn theme_sub_owner(key: &str) -> Part {
    if FONT_KEYS.contains(&key) {
        Part::Fonts
    } else if ICON_KEYS.contains(&key) {
        Part::Icons
    } else {
        Part::Shell
    }
}

/// The keys of `doc` that belong to `parts` (everything else is left out).
pub fn extract_json(doc: &Value, parts: &PartSet) -> Value {
    let mut out = Map::new();
    let Some(obj) = doc.as_object() else { return Value::Object(out) };
    for (k, v) in obj {
        if k == "theme" {
            match v {
                Value::Object(t) => {
                    let sub: Map<String, Value> = t.iter().filter(|(sk, _)| parts.has(theme_sub_owner(sk))).map(|(a, b)| (a.clone(), b.clone())).collect();
                    if !sub.is_empty() {
                        out.insert(k.clone(), Value::Object(sub));
                    }
                }
                other if parts.has(Part::Shell) => {
                    out.insert(k.clone(), other.clone());
                }
                _ => {}
            }
        } else if parts.has(json_top_owner(k)) {
            out.insert(k.clone(), v.clone());
        }
    }
    let mut out = Value::Object(out);
    strip_secrets_json(&mut out);
    out
}

/// Applies a preset's `decebal.json` share on top of `current`.
/// Replace mode first removes everything `parts` own, so the result is exactly what the preset says for those parts.
/// Merge mode only overlays: objects merge key by key, `null` removes a key, everything else is replaced.
pub fn merge_json(current: &Value, preset: &Value, parts: &PartSet, merge: bool) -> Value {
    let mut out = current.as_object().cloned().unwrap_or_default();
    if !merge {
        let keys: Vec<String> = out.keys().cloned().collect();
        for k in keys {
            if k == "theme" {
                match out.get_mut("theme") {
                    Some(Value::Object(t)) => {
                        let drop: Vec<String> = t.keys().filter(|sk| parts.has(theme_sub_owner(sk))).cloned().collect();
                        for d in drop {
                            t.shift_remove(&d);
                        }
                        if t.is_empty() {
                            out.shift_remove("theme");
                        }
                    }
                    Some(_) if parts.has(Part::Shell) => {
                        out.shift_remove("theme");
                    }
                    _ => {}
                }
            } else if parts.has(json_top_owner(&k)) {
                out.shift_remove(&k);
            }
        }
    }
    let Some(p) = preset.as_object() else { return Value::Object(out) };
    for (k, v) in p {
        if k == "theme" {
            let Value::Object(pt) = v else {
                if parts.has(Part::Shell) {
                    out.insert(k.clone(), v.clone());
                }
                continue;
            };
            let mut cur = match out.get("theme") {
                Some(Value::Object(o)) => o.clone(),
                Some(Value::String(name)) => Map::from_iter([("name".to_string(), Value::String(name.clone()))]),
                _ => Map::new(),
            };
            for (sk, sv) in pt {
                if parts.has(theme_sub_owner(sk)) {
                    overlay_json(&mut cur, sk, sv, merge);
                }
            }
            if !cur.is_empty() {
                out.insert(k.clone(), Value::Object(cur));
            }
        } else if parts.has(json_top_owner(k)) {
            overlay_json(&mut out, k, v, merge);
        }
    }
    Value::Object(out)
}

/// Sets `key` in `obj`: deep merge for objects in merge mode, `null` removes in merge mode, replacement otherwise.
pub fn overlay_json(obj: &mut Map<String, Value>, key: &str, v: &Value, merge: bool) {
    if merge {
        if v.is_null() {
            obj.shift_remove(key);
            return;
        }
        if let (Some(Value::Object(old)), Value::Object(new)) = (obj.get_mut(key), v) {
            for (k, nv) in new {
                overlay_json(old, k, nv, true);
            }
            return;
        }
        // a nested null inside a fresh object is dropped as well
        if let Value::Object(new) = v {
            let mut clean = Map::new();
            for (k, nv) in new {
                overlay_json(&mut clean, k, nv, true);
            }
            obj.insert(key.to_string(), Value::Object(clean));
            return;
        }
    }
    obj.insert(key.to_string(), v.clone());
}

/// Whole-file overlay used for `dock-settings.json`: the preset's keys win, `null` removes a key, the rest is kept.
pub fn overlay_file_json(current: &Value, preset: &Value) -> Value {
    let mut out = current.as_object().cloned().unwrap_or_default();
    if let Some(p) = preset.as_object() {
        for (k, v) in p {
            overlay_json(&mut out, k, v, true);
        }
    }
    Value::Object(out)
}

pub fn pretty_json(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

// ---------------------------------------------------------------- rdwm config.toml

/// `None` = machine specific and never part of a preset (`[[output]]`).
fn toml_top_owner(key: &str) -> Option<Part> {
    Some(match key {
        "wallpaper" => Part::Wallpaper,
        "bind" | "mode" => Part::Keys,
        "autostart" | "plugin" | "scratchpad" => Part::Autostart,
        "output" => return None,
        _ => Part::Rdwm,
    })
}

fn owned(key: &str, parts: &PartSet) -> bool {
    toml_top_owner(key).is_some_and(|p| parts.has(p))
}

pub fn parse_toml(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>().map_err(|e| e.to_string())
}

/// The top-level keys of `text` that belong to `parts`, as a standalone document.
pub fn extract_toml(text: &str, parts: &PartSet) -> Result<String, String> {
    let doc = parse_toml(text)?;
    let mut out = DocumentMut::new();
    let mut next = 0isize;
    for (k, item) in doc.as_table().iter() {
        if !owned(k, parts) {
            continue;
        }
        let mut item = item.clone();
        renumber(&mut item, &mut next, 0);
        let key = doc.as_table().get_key_value(k).map(|(key, _)| key.clone()).unwrap_or_else(|| Key::new(k));
        out.as_table_mut().insert_formatted(&key, item);
    }
    strip_secrets_table(out.as_table_mut());
    Ok(out.to_string())
}

/// Largest table position in `item` (document order of `[headers]`).
fn max_position(item: &Item) -> isize {
    let mut m = 0;
    match item {
        Item::Table(t) => {
            m = t.position().unwrap_or(0);
            for (_, i) in t.iter() {
                m = m.max(max_position(i));
            }
        }
        Item::ArrayOfTables(a) => {
            for t in a.iter() {
                m = m.max(t.position().unwrap_or(0));
                for (_, i) in t.iter() {
                    m = m.max(max_position(i));
                }
            }
        }
        _ => {}
    }
    m
}

/// Moves every table position of `item` up by `base` so a table inserted from another document sorts after the existing ones.
fn renumber(item: &mut Item, base: &mut isize, offset: isize) {
    fn shift(t: &mut Table, offset: isize) {
        if let Some(p) = t.position() {
            t.set_position(Some(p + offset));
        }
        for (_, i) in t.iter_mut() {
            renumber(i, &mut 0, offset);
        }
    }
    let _ = base;
    match item {
        Item::Table(t) => shift(t, offset),
        Item::ArrayOfTables(a) => {
            for t in a.iter_mut() {
                shift(t, offset);
            }
        }
        _ => {}
    }
}

/// Puts the preset's rdwm share on top of `current` (see [`merge_json`] for the two modes). Output always parses again.
pub fn merge_toml(current: &str, preset: &str, parts: &PartSet, merge: bool) -> Result<String, String> {
    let mut cur = parse_toml(current)?;
    let pre = parse_toml(preset)?;
    if !merge {
        let drop: Vec<String> = cur.as_table().iter().map(|(k, _)| k.to_string()).filter(|k| owned(k, parts)).collect();
        for k in drop {
            cur.as_table_mut().remove(&k);
        }
    }
    let base = cur.as_table().iter().map(|(_, i)| max_position(i)).max().unwrap_or(0) + 1;
    for (k, item) in pre.as_table().iter() {
        if !owned(k, parts) {
            continue;
        }
        let mut item = item.clone();
        renumber(&mut item, &mut 0, base);
        if merge {
            if let (Some(Item::Table(old)), Item::Table(new)) = (cur.as_table_mut().get_mut(k), &item) {
                overlay_table(old, new);
                continue;
            }
        }
        let key = pre.as_table().get_key_value(k).map(|(key, _)| key.clone()).unwrap_or_else(|| Key::new(k));
        cur.as_table_mut().insert_formatted(&key, item);
    }
    let text = cur.to_string();
    parse_toml(&text).map_err(|e| format!("merged config would not parse: {e}"))?;
    Ok(text)
}

fn overlay_table(old: &mut Table, new: &Table) {
    for (k, v) in new.iter() {
        match (old.get_mut(k), v) {
            (Some(Item::Table(o)), Item::Table(n)) => overlay_table(o, n),
            _ => {
                let key = new.get_key_value(k).map(|(key, _)| key.clone()).unwrap_or_else(|| Key::new(k));
                old.insert_formatted(&key, v.clone());
            }
        }
    }
}

// ---------------------------------------------------------------- flatten (diffs)

fn json_scalar(v: &Value) -> String {
    match v {
        Value::String(s) => format!("{s:?}"),
        other => other.to_string(),
    }
}

pub fn flatten_json(v: &Value) -> BTreeMap<String, String> {
    fn walk(prefix: &str, v: &Value, out: &mut BTreeMap<String, String>) {
        match v {
            Value::Object(o) if !o.is_empty() => {
                for (k, x) in o {
                    let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    walk(&p, x, out);
                }
            }
            Value::Array(a) if a.iter().any(|x| x.is_object()) => {
                for (i, x) in a.iter().enumerate() {
                    walk(&format!("{prefix}[{i}]"), x, out);
                }
            }
            other => {
                out.insert(prefix.to_string(), json_scalar(other));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk("", v, &mut out);
    out.remove("");
    out
}

pub fn flatten_toml(text: &str) -> BTreeMap<String, String> {
    fn walk_table(prefix: &str, t: &dyn TableLike, out: &mut BTreeMap<String, String>) {
        for (k, item) in t.iter() {
            let p = if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
            walk(&p, item, out);
        }
    }
    fn walk(prefix: &str, item: &Item, out: &mut BTreeMap<String, String>) {
        match item {
            Item::Table(t) => walk_table(prefix, t, out),
            Item::ArrayOfTables(a) => {
                for (i, t) in a.iter().enumerate() {
                    walk_table(&format!("{prefix}[{i}]"), t, out);
                }
            }
            Item::Value(toml_edit::Value::InlineTable(t)) => walk_table(prefix, t, out),
            Item::Value(toml_edit::Value::Array(a)) if a.iter().any(|x| x.is_inline_table()) => {
                for (i, x) in a.iter().enumerate() {
                    if let Some(t) = x.as_inline_table() {
                        walk_table(&format!("{prefix}[{i}]"), t, out);
                    }
                }
            }
            Item::Value(v) => {
                out.insert(prefix.to_string(), v.to_string().trim().to_string());
            }
            Item::None => {}
        }
    }
    let mut out = BTreeMap::new();
    if let Ok(doc) = parse_toml(text) {
        walk_table("", doc.as_table(), &mut out);
    }
    out
}

/// Key-level difference between two flattened files, as short readable lines (capped).
pub fn diff_lines(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>, cap: usize) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    for (k, nv) in new {
        match old.get(k) {
            None => lines.push(format!("+ {k} = {nv}")),
            Some(ov) if ov != nv => lines.push(format!("~ {k}: {ov}  ->  {nv}")),
            _ => {}
        }
    }
    for (k, ov) in old {
        if !new.contains_key(k) {
            lines.push(format!("- {k} (was {ov})"));
        }
    }
    let total = lines.len();
    lines.truncate(cap);
    (lines, total)
}

// ---------------------------------------------------------------- secrets

/// Key names that hold credentials (API keys, tokens, passwords). They never go into a preset.
pub fn is_secret_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase().replace(['-', '_', ' '], "");
    ["apikey", "token", "password", "passwd", "passphrase", "secret", "credential", "privatekey", "accesskey"].iter().any(|w| k.contains(w))
}

/// Removes every secret-looking key (at any depth) from a JSON value. Returns how many were removed.
pub fn strip_secrets_json(v: &mut Value) -> usize {
    match v {
        Value::Object(o) => {
            let secret: Vec<String> = o.keys().filter(|k| is_secret_key(k)).cloned().collect();
            let mut n = secret.len();
            for k in secret {
                o.shift_remove(&k);
            }
            for x in o.values_mut() {
                n += strip_secrets_json(x);
            }
            n
        }
        Value::Array(a) => a.iter_mut().map(strip_secrets_json).sum(),
        _ => 0,
    }
}

fn strip_secrets_table(t: &mut dyn TableLike) -> usize {
    let secret: Vec<String> = t.iter().map(|(k, _)| k.to_string()).filter(|k| is_secret_key(k)).collect();
    let mut n = secret.len();
    for k in secret {
        t.remove(&k);
    }
    for (_, item) in t.iter_mut() {
        n += strip_secrets_item(item);
    }
    n
}

fn strip_secrets_item(item: &mut Item) -> usize {
    match item {
        Item::Table(t) => strip_secrets_table(t),
        Item::ArrayOfTables(a) => a.iter_mut().map(|t| strip_secrets_table(t)).sum(),
        Item::Value(toml_edit::Value::InlineTable(t)) => strip_secrets_table(t),
        Item::Value(toml_edit::Value::Array(a)) => a.iter_mut().filter_map(|v| v.as_inline_table_mut()).map(|t| strip_secrets_table(t)).sum(),
        _ => 0,
    }
}

/// [`strip_secrets_json`] for TOML text. Returns the text without secrets (unchanged when there are none).
pub fn strip_secrets_toml(text: &str) -> Result<String, String> {
    let mut doc = parse_toml(text)?;
    let n = strip_secrets_table(doc.as_table_mut());
    Ok(if n == 0 { text.to_string() } else { doc.to_string() })
}

// ---------------------------------------------------------------- commands

/// Something that makes the desktop run a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunItem {
    /// Where it is configured, e.g. `rdwm autostart #2`.
    pub source: String,
    /// The command line.
    pub command: String,
}

const COMMAND_KEYS: [&str; 5] = ["command", "exec", "cmd", "script", "run"];

fn join_args(t: &dyn TableLike) -> String {
    let arr = t.get("args").and_then(|a| a.as_array());
    let mut words: Vec<String> = Vec::new();
    if let Some(a) = arr {
        words.extend(a.iter().map(|w| w.as_str().map(String::from).unwrap_or_else(|| w.to_string().trim().to_string())));
    } else if let Some(s) = t.get("args").and_then(|a| a.as_str()) {
        words.push(s.to_string());
    }
    words.join(" ")
}

fn entries(item: &Item) -> Vec<&dyn TableLike> {
    match item {
        Item::ArrayOfTables(a) => a.iter().map(|t| t as &dyn TableLike).collect(),
        Item::Value(toml_edit::Value::Array(a)) => a.iter().filter_map(|v| v.as_inline_table().map(|t| t as &dyn TableLike)).collect(),
        _ => Vec::new(),
    }
}

/// Every command rdwm would run for this `rdwm.toml` text: autostart, plugins, scratchpads and `spawn` binds.
pub fn toml_commands(text: &str) -> Vec<RunItem> {
    let Ok(doc) = parse_toml(text) else { return Vec::new() };
    let mut out = Vec::new();
    let root = doc.as_table();
    for (section, label) in [("autostart", "autostart"), ("plugin", "plugin"), ("scratchpad", "scratchpad")] {
        let Some(item) = root.get(section) else { continue };
        if let Some(arr) = item.as_array() {
            for (i, v) in arr.iter().enumerate() {
                if let Some(s) = v.as_str() {
                    out.push(RunItem { source: format!("rdwm {label} #{}", i + 1), command: s.to_string() });
                }
            }
        }
        for (i, t) in entries(item).into_iter().enumerate() {
            let cmd = t.get("command").map(|c| match c.as_str() {
                Some(s) => s.to_string(),
                None => c.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default(),
            });
            let args = join_args(t);
            let name = t.get("name").and_then(|n| n.as_str()).map(|n| format!(" \"{n}\"")).unwrap_or_default();
            if let Some(c) = cmd.filter(|c| !c.is_empty()) {
                out.push(RunItem { source: format!("rdwm {label} #{}{name}", i + 1), command: if args.is_empty() { c } else { format!("{c} {args}") } });
            }
        }
    }
    let mut binds: Vec<(String, &dyn TableLike)> = Vec::new();
    if let Some(b) = root.get("bind") {
        for (i, t) in entries(b).into_iter().enumerate() {
            binds.push((format!("rdwm shortcut #{}", i + 1), t));
        }
    }
    if let Some(m) = root.get("mode") {
        for (mi, mt) in entries(m).into_iter().enumerate() {
            let name = mt.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
            if let Some(b) = mt.get("bind") {
                for (i, t) in entries(b).into_iter().enumerate() {
                    binds.push((format!("rdwm mode \"{name}\" shortcut #{}", i + 1), t));
                }
            }
            let _ = mi;
        }
    }
    for (src, t) in binds {
        if t.get("action").and_then(|a| a.as_str()) == Some("spawn") {
            let keys = t.get("keys").map(|k| k.to_string().trim().to_string()).or_else(|| t.get("key").map(|k| k.to_string().trim().to_string())).unwrap_or_default();
            let a = join_args(t);
            out.push(RunItem { source: format!("{src} ({keys})"), command: a });
        }
    }
    out
}

/// Commands hidden in `decebal.json` / `dock-settings.json`: `custom:` hot corners and any `command`-like key.
pub fn json_commands(file: &str, v: &Value) -> Vec<RunItem> {
    let mut out = Vec::new();
    if let Some(hc) = v.get("hot_corners").and_then(Value::as_object) {
        for (corner, val) in hc {
            if let Some(s) = val.as_str() {
                if crate::hot::Action::parse(s).is_some_and(|a| matches!(a, crate::hot::Action::Custom(_))) {
                    let cmd = s.trim()[7..].trim().to_string();
                    out.push(RunItem { source: format!("hot corner {corner}"), command: cmd });
                }
            }
        }
    }
    fn walk(file: &str, path: &str, v: &Value, out: &mut Vec<RunItem>) {
        match v {
            Value::Object(o) => {
                for (k, x) in o {
                    let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    if COMMAND_KEYS.contains(&k.as_str()) {
                        if let Some(s) = x.as_str().filter(|s| !s.trim().is_empty()) {
                            out.push(RunItem { source: format!("{file}: {p}"), command: s.to_string() });
                        }
                    }
                    walk(file, &p, x, out);
                }
            }
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    walk(file, &format!("{path}[{i}]"), x, out);
                }
            }
            _ => {}
        }
    }
    walk(file, "", v, &mut out);
    out
}

/// Removes everything [`toml_commands`] reports from `text`: the autostart, plugin and scratchpad tables and all
/// `spawn` shortcuts. Returns the new text.
pub fn strip_toml_commands(text: &str) -> Result<String, String> {
    let mut doc = parse_toml(text)?;
    for k in ["autostart", "plugin", "scratchpad"] {
        doc.as_table_mut().remove(k);
    }
    fn drop_spawn(item: &mut Item) {
        match item {
            Item::ArrayOfTables(a) => {
                let mut i = 0;
                while i < a.len() {
                    if a.get(i).and_then(|t| t.get("action")).and_then(|v| v.as_str()) == Some("spawn") {
                        a.remove(i);
                    } else {
                        i += 1;
                    }
                }
            }
            Item::Value(toml_edit::Value::Array(a)) => {
                a.retain(|v| v.as_inline_table().and_then(|t| t.get("action")).and_then(|v| v.as_str()) != Some("spawn"));
            }
            _ => {}
        }
    }
    if let Some(b) = doc.as_table_mut().get_mut("bind") {
        drop_spawn(b);
    }
    if let Some(m) = doc.as_table_mut().get_mut("mode") {
        match m {
            Item::ArrayOfTables(a) => {
                for t in a.iter_mut() {
                    if let Some(b) = t.get_mut("bind") {
                        drop_spawn(b);
                    }
                }
            }
            Item::Value(toml_edit::Value::Array(a)) => {
                for v in a.iter_mut() {
                    if let Some(toml_edit::Value::Array(binds)) = v.as_inline_table_mut().and_then(|t| t.get_mut("bind")) {
                        binds.retain(|b| b.as_inline_table().and_then(|t| t.get("action")).and_then(|v| v.as_str()) != Some("spawn"));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(doc.to_string())
}

/// Removes `custom:` hot corners (set to `none`) and `command`-like keys from a JSON value.
pub fn strip_json_commands(v: &mut Value) {
    if let Some(hc) = v.get_mut("hot_corners").and_then(Value::as_object_mut) {
        for (_, val) in hc.iter_mut() {
            if val.as_str().and_then(crate::hot::Action::parse).is_some_and(|a| matches!(a, crate::hot::Action::Custom(_))) {
                *val = Value::String("none".into());
            }
        }
    }
    fn walk(v: &mut Value) {
        match v {
            Value::Object(o) => {
                for k in COMMAND_KEYS {
                    o.shift_remove(k);
                }
                for (_, x) in o.iter_mut() {
                    walk(x);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(v);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RDWM: &str = "# my config\n[general]\nborder_px = 2\ngap_inner = 6\n\n[decoration]\nenabled = true\n\n[wallpaper]\nimage = \"/a/b.png\"\nmode = \"fill\"\n\n[[bind]]\nmods = [\"super\"]\nkey = \"Return\"\naction = \"spawn\"\nargs = [\"foot\", \"-e\", \"htop\"]\n\n[[bind]]\nkeys = [\"super+q\"]\naction = \"close\"\n\n[[autostart]]\ncommand = \"decbar &\"\n\n[[scratchpad]]\nname = \"term\"\ncommand = \"foot -a scratch\"\n\n[[output]]\nname = \"DP-1\"\n\n[[mode]]\nname = \"resize\"\n[[mode.bind]]\nkeys = \"h\"\naction = \"spawn\"\nargs = [\"notify-send\", \"hi\"]\n";

    fn ps(p: &[Part]) -> PartSet {
        PartSet::new(p.iter().copied())
    }

    #[test]
    fn parts_parse_and_files() {
        for p in Part::ALL {
            assert_eq!(Part::parse(p.id()), Some(p));
            assert!(!p.title().is_empty() && !p.detail().is_empty());
        }
        assert_eq!(Part::parse("Hot-Corners"), Some(Part::HotCorners));
        assert_eq!(Part::parse("zzz"), None);
        assert!(PartSet::defaults().has(Part::Shell) && !PartSet::defaults().has(Part::Autostart));
    }

    #[test]
    fn extract_json_splits_by_part() {
        let doc = json!({"theme": {"name": "nord", "mode": "dark", "font": "Inter", "fontScale": 1.1}, "bar": {"enabled": true}, "hot_corners": {"top_left": "dashboard"}, "iconTheme": "Papirus"});
        let shell = extract_json(&doc, &ps(&[Part::Shell]));
        assert_eq!(shell, json!({"theme": {"name": "nord", "mode": "dark"}, "bar": {"enabled": true}}));
        assert_eq!(extract_json(&doc, &ps(&[Part::Fonts])), json!({"theme": {"font": "Inter", "fontScale": 1.1}}));
        assert_eq!(extract_json(&doc, &ps(&[Part::HotCorners])), json!({"hot_corners": {"top_left": "dashboard"}}));
        assert_eq!(extract_json(&doc, &ps(&[Part::Icons])), json!({"iconTheme": "Papirus"}));
        assert_eq!(extract_json(&doc, &ps(&[Part::Dock])), json!({}));
        // a bare theme name belongs to Shell
        assert_eq!(extract_json(&json!({"theme": "nord"}), &ps(&[Part::Shell])), json!({"theme": "nord"}));
        assert_eq!(extract_json(&json!({"theme": "nord"}), &ps(&[Part::Fonts])), json!({}));
    }

    #[test]
    fn merge_json_replace_removes_only_owned_keys() {
        let cur = json!({"theme": {"name": "macos", "accent": "#ff0000", "font": "Inter"}, "workspaces": "off", "hot_corners": {"top_left": "lock"}, "keepme": 1});
        let preset = json!({"theme": {"name": "nord"}, "bar": {"enabled": false}});
        let out = merge_json(&cur, &preset, &ps(&[Part::Shell]), false);
        assert_eq!(out, json!({"theme": {"font": "Inter", "name": "nord"}, "hot_corners": {"top_left": "lock"}, "bar": {"enabled": false}}));
        assert!(out.get("workspaces").is_none() && out.get("keepme").is_none(), "Shell owns every other key and replace removes it");
        // an unselected part is untouched even when the preset has keys for it
        let out = merge_json(&cur, &json!({"hot_corners": {"top_left": "none"}}), &ps(&[Part::Shell]), false);
        assert_eq!(out["hot_corners"], json!({"top_left": "lock"}));
        // fonts only: the theme name stays
        let out = merge_json(&cur, &json!({"theme": {"font": "Noto"}}), &ps(&[Part::Fonts]), false);
        assert_eq!(out["theme"], json!({"name": "macos", "accent": "#ff0000", "font": "Noto"}));
        assert_eq!(out["keepme"], 1);
        // the preset has no fonts: replace clears them
        let out = merge_json(&cur, &json!({}), &ps(&[Part::Fonts]), false);
        assert_eq!(out["theme"], json!({"name": "macos", "accent": "#ff0000"}));
    }

    #[test]
    fn merge_json_merge_mode_overlays_and_null_removes() {
        let cur = json!({"theme": {"name": "macos", "accent": "#ff0000"}, "bar": {"enabled": false, "x": 1}, "workspaces": "off"});
        let preset = json!({"theme": {"name": "nord", "accent": null}, "bar": {"enabled": true}});
        let out = merge_json(&cur, &preset, &ps(&[Part::Shell]), true);
        assert_eq!(out, json!({"theme": {"name": "nord"}, "bar": {"enabled": true, "x": 1}, "workspaces": "off"}));
        let out = overlay_file_json(&json!({"style": "windows", "roundness": 0.2, "magnification": 1.8}), &json!({"style": "mac", "roundness": null}));
        assert_eq!(out, json!({"style": "mac", "magnification": 1.8}));
    }

    #[test]
    fn extract_toml_picks_tables_by_part() {
        let rd = extract_toml(RDWM, &ps(&[Part::Rdwm])).unwrap();
        assert!(rd.contains("[general]") && rd.contains("[decoration]"));
        assert!(!rd.contains("wallpaper") && !rd.contains("bind") && !rd.contains("autostart") && !rd.contains("DP-1"), "{rd}");
        let w = extract_toml(RDWM, &ps(&[Part::Wallpaper])).unwrap();
        assert!(w.contains("image = \"/a/b.png\"") && !w.contains("[general]"));
        let k = extract_toml(RDWM, &ps(&[Part::Keys])).unwrap();
        assert!(k.contains("super+q") && k.contains("[[mode.bind]]") && !k.contains("decbar"));
        let a = extract_toml(RDWM, &ps(&[Part::Autostart])).unwrap();
        assert!(a.contains("decbar &") && a.contains("[[scratchpad]]"));
        assert!(extract_toml("[general\n", &ps(&[Part::Rdwm])).is_err());
    }

    #[test]
    fn merge_toml_replaces_owned_tables_and_keeps_the_rest() {
        let preset = "[general]\nborder_px = 5\n\n[colors]\nfocused = \"#112233\"\n";
        let out = merge_toml(RDWM, preset, &ps(&[Part::Rdwm]), false).unwrap();
        let flat = flatten_toml(&out);
        assert_eq!(flat.get("general.border_px").map(String::as_str), Some("5"));
        assert!(!flat.contains_key("general.gap_inner"), "the whole [general] was replaced");
        assert!(!flat.contains_key("decoration.enabled"), "decoration belongs to Rdwm and the preset has none");
        assert_eq!(flat.get("colors.focused").map(String::as_str), Some("\"#112233\""));
        assert_eq!(flat.get("wallpaper.image").map(String::as_str), Some("\"/a/b.png\""), "other parts stay");
        assert!(flat.contains_key("bind[0].action") && flat.contains_key("autostart[0].command") && flat.contains_key("output[0].name"));
        // merge mode overlays single keys
        let out = merge_toml(RDWM, "[general]\nborder_px = 9\n", &ps(&[Part::Rdwm]), true).unwrap();
        let flat = flatten_toml(&out);
        assert_eq!(flat.get("general.border_px").map(String::as_str), Some("9"));
        assert_eq!(flat.get("general.gap_inner").map(String::as_str), Some("6"));
        // [[output]] is never touched, whatever the preset says
        let out = merge_toml(RDWM, "[[output]]\nname = \"EVIL\"\n", &ps(&[Part::Rdwm, Part::Keys, Part::Autostart, Part::Wallpaper]), false).unwrap();
        assert!(out.contains("DP-1") && !out.contains("EVIL"));
        // keys part replaces binds, modes included
        let out = merge_toml(RDWM, "[[bind]]\nkeys = [\"super+w\"]\naction = \"close\"\n", &ps(&[Part::Keys]), false).unwrap();
        assert!(out.contains("super+w") && !out.contains("super+q") && !out.contains("[[mode.bind]]") && out.contains("[general]"));
    }

    #[test]
    fn toml_round_trip_through_extract_and_merge_is_lossless_for_the_parts() {
        let all = PartSet::new([Part::Rdwm, Part::Keys, Part::Autostart, Part::Wallpaper]);
        let share = extract_toml(RDWM, &all).unwrap();
        let back = merge_toml("", &share, &all, false).unwrap();
        let (a, b) = (flatten_toml(&share), flatten_toml(&back));
        assert_eq!(a, b);
        assert!(a.len() > 10);
    }

    #[test]
    fn commands_are_found_everywhere() {
        let c = toml_commands(RDWM);
        let cmds: Vec<&str> = c.iter().map(|r| r.command.as_str()).collect();
        assert!(cmds.contains(&"decbar &") && cmds.contains(&"foot -a scratch") && cmds.contains(&"foot -e htop") && cmds.contains(&"notify-send hi"), "{c:?}");
        assert_eq!(c.len(), 4);
        assert!(c.iter().any(|r| r.source.contains("scratchpad") && r.source.contains("term")));
        assert!(toml_commands("[general]\nborder_px = 1\n").is_empty());
        let hc = json!({"hot_corners": {"top_left": "dashboard", "bottom_right": "custom:rm -rf ~"}, "widgets": [{"command": "date"}]});
        let j = json_commands("decebal.json", &hc);
        assert_eq!(j.len(), 2, "{j:?}");
        assert!(j.iter().any(|r| r.command == "rm -rf ~"));
    }

    #[test]
    fn stripping_removes_every_command() {
        let out = strip_toml_commands(RDWM).unwrap();
        assert!(toml_commands(&out).is_empty(), "{out}");
        assert!(out.contains("super+q") && out.contains("border_px"), "ordinary keys stay");
        assert!(!out.contains("autostart") && !out.contains("scratchpad"));
        let mut j = json!({"hot_corners": {"top_left": "dashboard", "bottom_right": "custom:rm -rf ~"}, "widgets": [{"command": "date", "n": 1}]});
        strip_json_commands(&mut j);
        assert!(json_commands("x", &j).is_empty());
        assert_eq!(j["hot_corners"]["top_left"], "dashboard");
        assert_eq!(j["hot_corners"]["bottom_right"], "none");
        assert_eq!(j["widgets"][0]["n"], 1);
        // inline-array spelling
        let out = strip_toml_commands("bind = [{ key = \"a\", action = \"spawn\", args = [\"x\"] }, { key = \"b\", action = \"close\" }]\n").unwrap();
        assert!(out.contains("close") && !out.contains("spawn"), "{out}");
    }

    #[test]
    fn secrets_never_leave() {
        let doc = json!({"theme": {"name": "x"}, "wallhaven": {"apiKey": "SECRET1", "q": "cats"}, "widgets": [{"auth_token": "SECRET2", "n": 1}], "password": "SECRET3"});
        let out = extract_json(&doc, &ps(&[Part::Shell]));
        assert!(!out.to_string().contains("SECRET"), "{out}");
        assert_eq!(out["wallhaven"]["q"], "cats");
        assert_eq!(out["widgets"][0]["n"], 1);
        let t = "[general]\nborder_px = 1\n[net]\napi_key = \"SECRET\"\nhost = \"h\"\n[[bind]]\nkey = \"Return\"\naction = \"spawn\"\n";
        let out = extract_toml(t, &ps(&[Part::Rdwm, Part::Keys])).unwrap();
        assert!(!out.contains("SECRET") && out.contains("host") && out.contains("key = \"Return\""), "{out}");
        assert_eq!(strip_secrets_toml("a = 1\n").unwrap(), "a = 1\n");
        assert!(!strip_secrets_toml("[x]\ntoken = \"t\"\nok = 1\n").unwrap().contains("token"));
        assert!(is_secret_key("API-Key") && is_secret_key("wallhavenToken") && !is_secret_key("key") && !is_secret_key("keys"));
    }

    #[test]
    fn flatten_and_diff() {
        let a = flatten_json(&json!({"theme": {"name": "macos"}, "x": [1, 2], "list": [{"a": 1}]}));
        assert_eq!(a.get("theme.name").map(String::as_str), Some("\"macos\""));
        assert_eq!(a.get("x").map(String::as_str), Some("[1,2]"));
        assert_eq!(a.get("list[0].a").map(String::as_str), Some("1"));
        let b = flatten_json(&json!({"theme": {"name": "nord"}, "y": true}));
        let (d, total) = diff_lines(&a, &b, 100);
        assert_eq!(total, d.len());
        assert!(d.iter().any(|l| l.starts_with("~ theme.name")) && d.iter().any(|l| l.starts_with("+ y")) && d.iter().any(|l| l.starts_with("- x")));
        assert_eq!(diff_lines(&a, &b, 1).0.len(), 1);
    }
}
