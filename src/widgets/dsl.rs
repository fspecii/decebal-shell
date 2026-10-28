//! The widget file format (`widgets/<name>.toml`) and its validator.
//!
//! ```toml
//! format = 1
//! id = "clock-card"
//! name = "Clock card"
//! size = [220, 120]            # optional; the natural size of the content otherwise
//! refresh = 1000               # ms between redraws, 250..60000 (default 1000)
//!
//! [root]                       # one root node; children are [[root.children]] tables
//! type = "col"
//! padding = 14
//! background = "popup"         # a theme colour, or "#rrggbb", optionally "@alpha": "text@0.1"
//! radius = 18
//! [[root.children]]
//! type = "text"
//! text = "{clock:%H:%M}"
//! size = 44
//! ```
//! Node types: `row` `col` `stack` `spacer` `button` (containers); `text` `icon` `bar` `ring` `graph` `image` `workspaces` (leaves).
//! Every error carries the line and the key path. See docs/theme-engine.md for the property tables.

use super::actions::Action;
use super::fmt::Fmt;
use super::Trust;
use crate::theme::model::Rgb;
use std::collections::BTreeSet;
use toml_edit::{Document, Item, Table};

pub const MAX_NODES: usize = 200;
pub const MAX_DEPTH: usize = 8;

/// A theme colour by name (resolved when drawing) or a fixed colour, with an alpha.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorRef {
    Token(String, f32),
    Fixed(Rgb, f32),
}

pub const TOKENS: &[&str] = &[
    "background",
    "sidebar",
    "surface",
    "bar",
    "bar_text",
    "popup",
    "dock",
    "overlay",
    "titlebar",
    "titlebar_inactive",
    "border",
    "text",
    "text_secondary",
    "text_faint",
    "accent",
    "accent_text",
    "danger",
    "ok",
    "warn",
];

impl ColorRef {
    pub fn parse(s: &str) -> Result<ColorRef, String> {
        let (name, alpha) = match s.split_once('@') {
            Some((n, a)) => (n.trim(), a.trim().parse::<f32>().ok().filter(|a| (0.0..=1.0).contains(a)).ok_or_else(|| format!("alpha in `{s}` must be 0..1"))?),
            None => (s.trim(), 1.0),
        };
        if let Some(c) = Rgb::parse(name) {
            return Ok(ColorRef::Fixed(c, alpha));
        }
        if TOKENS.contains(&name) {
            return Ok(ColorRef::Token(name.into(), alpha));
        }
        Err(format!("`{s}` is not a colour: use #rrggbb or a theme colour ({})", TOKENS.join(", ")))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Lit(f64),
    Path(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    Between,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Common {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub grow: f32,
    /// left, top, right, bottom
    pub padding: [f32; 4],
    pub background: Option<ColorRef>,
    /// px; `None` = no rounding. `radius = "theme"` is stored as a negative factor (-1 = the theme's popup radius).
    pub radius: Option<f32>,
    pub border: Option<ColorRef>,
    pub border_width: f32,
    pub opacity: f32,
    /// Cross-axis alignment of the children (containers); `None` = start for rows and columns, stretch for stacks.
    pub align: Option<Align>,
    pub show_if: Option<String>,
    pub on_click: Option<Action>,
    pub on_scroll_up: Option<Action>,
    pub on_scroll_down: Option<Action>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Row { gap: f32, justify: Justify },
    Col { gap: f32, justify: Justify },
    Stack,
    Spacer,
    Button { gap: f32 },
    Text { text: Fmt, size: f32, bold: bool, color: ColorRef, align: Align, max_chars: usize },
    Icon { name: Fmt, size: f32, color: ColorRef },
    Bar { value: Value, min: f64, max: f64, color: ColorRef, track: ColorRef },
    Ring { value: Value, max: f64, size: f32, thickness: f32, color: ColorRef, track: ColorRef },
    Graph { series: String, max: Option<f32>, color: ColorRef, fill: bool },
    Image { file: Option<String>, source: Option<String>, radius: f32, cover: bool },
    Workspaces { size: f32, gap: f32, color: ColorRef, active: ColorRef },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub c: Common,
    pub children: Vec<Node>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDef {
    pub name: String,
    pub run: String,
    pub interval: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size: Option<(f32, f32)>,
    pub refresh_ms: u32,
    pub commands: Vec<CommandDef>,
    pub root: Node,
    /// Package files the widget draws (`image` nodes with `file`).
    pub assets: Vec<String>,
    /// Every data path read.
    pub paths: BTreeSet<String>,
    /// Every action a node can run.
    pub actions: Vec<Action>,
}

/// Anything with a source span, so errors can name a line.
trait Sp {
    fn sp(&self) -> Option<std::ops::Range<usize>>;
}
impl Sp for Item {
    fn sp(&self) -> Option<std::ops::Range<usize>> {
        self.span()
    }
}
impl Sp for Table {
    fn sp(&self) -> Option<std::ops::Range<usize>> {
        self.span()
    }
}

struct Cx<'a> {
    text: &'a str,
    trust: Trust,
    errs: Vec<String>,
    paths: BTreeSet<String>,
    assets: Vec<String>,
    actions: Vec<Action>,
    commands: BTreeSet<String>,
    nodes: usize,
}

impl Cx<'_> {
    fn line(&self, item: &impl Sp) -> usize {
        item.sp().map(|s| self.text[..s.start.min(self.text.len())].matches('\n').count() + 1).unwrap_or(0)
    }
    fn err(&mut self, item: &impl Sp, key: &str, msg: impl std::fmt::Display) {
        let l = self.line(item);
        self.errs.push(if l > 0 { format!("line {l}: `{key}`: {msg}") } else { format!("`{key}`: {msg}") });
    }
    fn path(&mut self, item: &impl Sp, key: &str, p: &str) -> bool {
        let ok = super::data::known(p) || p.strip_prefix("cmd.").is_some_and(|c| self.commands.contains(c));
        if ok {
            self.paths.insert(p.to_string());
        } else if let Some(c) = p.strip_prefix("cmd.") {
            self.err(item, key, format!("`{p}` is not a declared command (add [commands.{c}])"));
        } else {
            self.err(item, key, format!("unknown data `{p}`{}", super::data::suggest(p)));
        }
        ok
    }
}

fn num(cx: &mut Cx, t: &Table, path: &str, key: &str, lo: f64, hi: f64) -> Option<f64> {
    let item = t.get(key)?;
    let n = item.as_float().or_else(|| item.as_integer().map(|i| i as f64));
    match n {
        Some(n) if (lo..=hi).contains(&n) => Some(n),
        Some(n) => {
            cx.err(item, &format!("{path}.{key}"), format!("{n} is outside {lo}..{hi}"));
            None
        }
        None => {
            cx.err(item, &format!("{path}.{key}"), "must be a number");
            None
        }
    }
}

fn string<'a>(cx: &mut Cx, t: &'a Table, path: &str, key: &str, max: usize) -> Option<&'a str> {
    let item = t.get(key)?;
    match item.as_str() {
        Some(s) if s.chars().count() <= max && !s.chars().any(|c| c.is_control()) => Some(s),
        Some(_) => {
            cx.err(item, &format!("{path}.{key}"), format!("longer than {max} characters or has control characters"));
            None
        }
        None => {
            cx.err(item, &format!("{path}.{key}"), "must be a string");
            None
        }
    }
}

fn flag(cx: &mut Cx, t: &Table, path: &str, key: &str) -> Option<bool> {
    let item = t.get(key)?;
    let b = item.as_bool();
    if b.is_none() {
        cx.err(item, &format!("{path}.{key}"), "must be true or false");
    }
    b
}

fn color(cx: &mut Cx, t: &Table, path: &str, key: &str) -> Option<ColorRef> {
    let s = string(cx, t, path, key, 40)?;
    match ColorRef::parse(s) {
        Ok(c) => Some(c),
        Err(e) => {
            cx.err(t.get(key)?, &format!("{path}.{key}"), e);
            None
        }
    }
}

fn choice<T: Copy>(cx: &mut Cx, t: &Table, path: &str, key: &str, opts: &[(&str, T)]) -> Option<T> {
    let s = string(cx, t, path, key, 20)?;
    match opts.iter().find(|(n, _)| *n == s) {
        Some((_, v)) => Some(*v),
        None => {
            cx.err(t.get(key)?, &format!("{path}.{key}"), format!("must be one of {}", opts.iter().map(|o| o.0).collect::<Vec<_>>().join(", ")));
            None
        }
    }
}

fn action(cx: &mut Cx, t: &Table, path: &str, key: &str) -> Option<Action> {
    let s = string(cx, t, path, key, 420)?;
    match Action::parse(s, cx.trust) {
        Ok(a) => {
            cx.actions.push(a.clone());
            Some(a)
        }
        Err(e) => {
            cx.err(t.get(key)?, &format!("{path}.{key}"), e);
            None
        }
    }
}

fn value(cx: &mut Cx, t: &Table, path: &str, key: &str) -> Option<Value> {
    let item = t.get(key)?;
    if let Some(s) = item.as_str() {
        return cx.path(item, &format!("{path}.{key}"), s).then(|| Value::Path(s.to_string()));
    }
    num(cx, t, path, key, -1e9, 1e9).map(Value::Lit)
}

fn fmt(cx: &mut Cx, t: &Table, path: &str, key: &str) -> Option<Fmt> {
    let s = string(cx, t, path, key, 200)?;
    match Fmt::parse(s) {
        Ok(f) => {
            for p in f.paths() {
                let item = t.get(key)?;
                cx.path(item, &format!("{path}.{key}"), p);
            }
            Some(f)
        }
        Err(e) => {
            cx.err(t.get(key)?, &format!("{path}.{key}"), e);
            None
        }
    }
}

const COMMON: &[&str] = &["type", "width", "height", "grow", "padding", "background", "radius", "border", "border_width", "opacity", "align", "show_if", "on_click", "on_scroll_up", "on_scroll_down"];

fn kind_keys(ty: &str) -> &'static [&'static str] {
    match ty {
        "row" | "col" => &["gap", "justify", "children"],
        "stack" => &["children"],
        "button" => &["gap", "children"],
        "spacer" => &[],
        "text" => &["text", "size", "weight", "color", "text_align", "max_chars"],
        "icon" => &["name", "size", "color"],
        "bar" => &["value", "min", "max", "color", "track"],
        "ring" => &["value", "max", "size", "thickness", "color", "track"],
        "graph" => &["series", "max", "color", "fill"],
        "image" => &["file", "source", "radius_px", "fit"],
        "workspaces" => &["size", "gap", "color", "active_color"],
        _ => &[],
    }
}

fn parse_common(cx: &mut Cx, t: &Table, p: &str) -> Common {
    let mut c = Common { opacity: 1.0, ..Common::default() };
    c.width = num(cx, t, p, "width", 0.0, 2000.0).map(|v| v as f32);
    c.height = num(cx, t, p, "height", 0.0, 2000.0).map(|v| v as f32);
    c.grow = num(cx, t, p, "grow", 0.0, 100.0).unwrap_or(0.0) as f32;
    if let Some(item) = t.get("padding") {
        let arr: Option<Vec<f64>> = item.as_array().map(|a| a.iter().filter_map(|x| x.as_float().or_else(|| x.as_integer().map(|i| i as f64))).collect());
        let single = item.as_float().or_else(|| item.as_integer().map(|i| i as f64));
        let pads = match (single, arr) {
            (Some(n), _) => Some([n, n, n, n]),
            (_, Some(a)) if a.len() == 2 => Some([a[1], a[0], a[1], a[0]]),
            (_, Some(a)) if a.len() == 4 => Some([a[0], a[1], a[2], a[3]]),
            _ => None,
        };
        match pads {
            Some(p4) if p4.iter().all(|v| (0.0..=200.0).contains(v)) => c.padding = p4.map(|v| v as f32),
            _ => cx.err(item, &format!("{p}.padding"), "must be a number, [vertical, horizontal] or [left, top, right, bottom], each 0..200"),
        }
    }
    c.background = color(cx, t, p, "background");
    if let Some(item) = t.get("radius") {
        if item.as_str() == Some("theme") {
            c.radius = Some(-1.0);
        } else {
            c.radius = num(cx, t, p, "radius", 0.0, 200.0).map(|v| v as f32);
        }
    }
    c.border = color(cx, t, p, "border");
    c.border_width = num(cx, t, p, "border_width", 0.0, 20.0).unwrap_or(1.0) as f32;
    c.opacity = num(cx, t, p, "opacity", 0.0, 1.0).unwrap_or(1.0) as f32;
    c.align = choice(cx, t, p, "align", &[("start", Align::Start), ("center", Align::Center), ("end", Align::End), ("stretch", Align::Stretch)]);
    if let Some(s) = string(cx, t, p, "show_if", 80) {
        let path = s.strip_prefix('!').unwrap_or(s);
        if cx.path(t.get("show_if").unwrap_or(&Item::None), &format!("{p}.show_if"), path) {
            c.show_if = Some(s.to_string());
        }
    }
    c.on_click = action(cx, t, p, "on_click");
    c.on_scroll_up = action(cx, t, p, "on_scroll_up");
    c.on_scroll_down = action(cx, t, p, "on_scroll_down");
    c
}

fn parse_node(cx: &mut Cx, t: &Table, p: &str, depth: usize) -> Option<Node> {
    cx.nodes += 1;
    if cx.nodes > MAX_NODES {
        if cx.nodes == MAX_NODES + 1 {
            cx.err(t, p, format!("more than {MAX_NODES} nodes"));
        }
        return None;
    }
    if depth > MAX_DEPTH {
        cx.err(t, p, format!("nested deeper than {MAX_DEPTH} levels"));
        return None;
    }
    let Some(ty) = t.get("type").and_then(Item::as_str) else {
        cx.err(t, p, "missing `type` (row, col, stack, spacer, button, text, icon, bar, ring, graph, image, workspaces)");
        return None;
    };
    if !matches!(ty, "row" | "col" | "stack" | "spacer" | "button" | "text" | "icon" | "bar" | "ring" | "graph" | "image" | "workspaces") {
        cx.err(t.get("type")?, &format!("{p}.type"), format!("unknown node type `{ty}`"));
        return None;
    }
    let allowed = kind_keys(ty);
    for (k, v) in t.iter() {
        if !COMMON.contains(&k) && !allowed.contains(&k) {
            let hint = if k == "exec" || k == "command" || k == "run" || k == "script" { " (widgets cannot run commands; use `on_click` with an allowed action)" } else { "" };
            cx.err(v, &format!("{p}.{k}"), format!("unknown key for a `{ty}` node{hint}"));
        }
    }
    let c = parse_common(cx, t, p);
    let ts = |cx: &mut Cx, k: &str, lo, hi, d: f64| num(cx, t, p, k, lo, hi).unwrap_or(d) as f32;
    let col = |cx: &mut Cx, k: &str, d: ColorRef| color(cx, t, p, k).unwrap_or(d);
    let text_c = ColorRef::Token("text".into(), 1.0);
    let track_c = ColorRef::Token("text".into(), 0.14);
    let accent = ColorRef::Token("accent".into(), 1.0);
    let kind = match ty {
        "row" | "col" => {
            let gap = ts(cx, "gap", 0.0, 200.0, 0.0);
            let justify = choice(cx, t, p, "justify", &[("start", Justify::Start), ("center", Justify::Center), ("end", Justify::End), ("between", Justify::Between)]).unwrap_or_default();
            if ty == "row" {
                Kind::Row { gap, justify }
            } else {
                Kind::Col { gap, justify }
            }
        }
        "stack" => Kind::Stack,
        "button" => Kind::Button { gap: ts(cx, "gap", 0.0, 200.0, 6.0) },
        "spacer" => Kind::Spacer,
        "text" => {
            let text = fmt(cx, t, p, "text").unwrap_or_default();
            if t.get("text").is_none() {
                cx.err(t, &format!("{p}.text"), "a text node needs `text`");
            }
            let bold = match string(cx, t, p, "weight", 10) {
                Some("bold") => true,
                Some("regular") | None => false,
                Some(_) => {
                    cx.err(t.get("weight")?, &format!("{p}.weight"), "must be regular or bold");
                    false
                }
            };
            let align = choice(cx, t, p, "text_align", &[("left", Align::Start), ("center", Align::Center), ("right", Align::End)]).unwrap_or_default();
            Kind::Text { text, size: ts(cx, "size", 6.0, 300.0, 14.0), bold, color: col(cx, "color", text_c), align, max_chars: ts(cx, "max_chars", 1.0, 400.0, 400.0) as usize }
        }
        "icon" => {
            let name = fmt(cx, t, p, "name").unwrap_or_default();
            if t.get("name").is_none() {
                cx.err(t, &format!("{p}.name"), "an icon node needs `name`");
            } else if let Some(n) = string(cx, t, p, "name", 60).filter(|n| !n.contains('{')) {
                if crate::menubar::symbols::shapes(n).is_none() && !n.starts_with("nerd:") && !n.starts_with("wx:") {
                    cx.err(t.get("name")?, &format!("{p}.name"), format!("no symbol named `{n}`"));
                }
            }
            Kind::Icon { name, size: ts(cx, "size", 6.0, 300.0, 18.0), color: col(cx, "color", text_c) }
        }
        "bar" | "ring" => {
            let Some(v) = value(cx, t, p, "value") else {
                if t.get("value").is_none() {
                    cx.err(t, &format!("{p}.value"), format!("a {ty} node needs `value` (a number or a data path like \"cpu.usage\")"));
                }
                return None;
            };
            let max = num(cx, t, p, "max", 0.0001, 1e12).unwrap_or(1.0);
            if ty == "bar" {
                Kind::Bar { value: v, min: num(cx, t, p, "min", -1e9, 1e9).unwrap_or(0.0), max, color: col(cx, "color", accent), track: col(cx, "track", track_c) }
            } else {
                Kind::Ring { value: v, max, size: ts(cx, "size", 10.0, 600.0, 64.0), thickness: ts(cx, "thickness", 1.0, 80.0, 7.0), color: col(cx, "color", accent), track: col(cx, "track", track_c) }
            }
        }
        "graph" => {
            let series = string(cx, t, p, "series", 60).unwrap_or("").to_string();
            if series.is_empty() {
                cx.err(t, &format!("{p}.series"), "a graph node needs `series` (a series path such as \"cpu.history\")");
            } else {
                cx.path(t.get("series")?, &format!("{p}.series"), &series);
            }
            Kind::Graph { series, max: num(cx, t, p, "max", 0.0001, 1e15).map(|v| v as f32), color: col(cx, "color", accent), fill: flag(cx, t, p, "fill").unwrap_or(true) }
        }
        "image" => {
            let file = string(cx, t, p, "file", 160).map(String::from);
            let source = string(cx, t, p, "source", 60).map(String::from);
            match (&file, &source) {
                (None, None) => cx.err(t, &format!("{p}.file"), "an image node needs `file` (a file in the package) or `source` (a data path such as \"media.art\")"),
                (Some(_), Some(_)) => cx.err(t, &format!("{p}.file"), "use either `file` or `source`, not both"),
                (Some(f), None) => {
                    let ext = f.rsplit_once('.').map(|x| x.1.to_ascii_lowercase()).unwrap_or_default();
                    match super::super::theme::engine::archive::safe_rel_path(f) {
                        Err(e) => cx.err(t.get("file")?, &format!("{p}.file"), e),
                        Ok(_) if !["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) => cx.err(t.get("file")?, &format!("{p}.file"), "must be a png, jpg or webp file"),
                        Ok(_) => cx.assets.push(f.clone()),
                    }
                }
                (None, Some(s)) => {
                    cx.path(t.get("source")?, &format!("{p}.source"), s);
                }
            }
            let cover = match string(cx, t, p, "fit", 10) {
                Some("cover") | None => true,
                Some("contain") => false,
                Some(_) => {
                    cx.err(t.get("fit")?, &format!("{p}.fit"), "must be cover or contain");
                    true
                }
            };
            Kind::Image { file, source, radius: ts(cx, "radius_px", 0.0, 400.0, 0.0), cover }
        }
        "workspaces" => {
            cx.paths.insert("workspace.count".into());
            Kind::Workspaces {
                size: ts(cx, "size", 3.0, 60.0, 8.0),
                gap: ts(cx, "gap", 0.0, 60.0, 6.0),
                color: col(cx, "color", ColorRef::Token("text".into(), 0.3)),
                active: col(cx, "active_color", accent),
            }
        }
        _ => unreachable!(),
    };
    let mut children = Vec::new();
    if let Some(ch) = t.get("children") {
        match ch.as_array_of_tables() {
            Some(arr) if matches!(ty, "row" | "col" | "stack" | "button") => {
                if arr.len() > 64 {
                    cx.err(ch, &format!("{p}.children"), "more than 64 children");
                }
                for (i, ct) in arr.iter().enumerate() {
                    if let Some(n) = parse_node(cx, ct, &format!("{p}.children[{i}]"), depth + 1) {
                        children.push(n);
                    }
                }
            }
            _ => cx.err(ch, &format!("{p}.children"), "must be a list of `[[...children]]` tables"),
        }
    }
    Some(Node { kind, c, children, line: cx.line(t) })
}

/// Parses and validates a widget. `trust` decides whether commands are allowed.
pub fn parse(text: &str, trust: Trust) -> Result<WidgetDef, String> {
    if text.len() > 64 << 10 {
        return Err("widget file is larger than 64 KiB".into());
    }
    let doc = Document::parse(text.to_string()).map_err(|e| {
        let line = e.span().map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1).unwrap_or(0);
        format!("line {line}: invalid TOML: {}", e.message())
    })?;
    let root = doc.as_table();
    let mut cx = Cx { text, trust, errs: Vec::new(), paths: BTreeSet::new(), assets: Vec::new(), actions: Vec::new(), commands: BTreeSet::new(), nodes: 0 };
    for (k, item) in root.iter() {
        if !matches!(k, "format" | "id" | "name" | "description" | "size" | "refresh" | "commands" | "root") {
            cx.err(item, k, "unknown key (format, id, name, description, size, refresh, commands, root)");
        }
    }
    if root.get("format").and_then(Item::as_integer) != Some(1) {
        cx.errs.push("`format`: a widget file starts with `format = 1`".into());
    }
    let id = string(&mut cx, root, "", "id", 64).unwrap_or("").to_string();
    if !id.is_empty() && !crate::theme::engine::schema::valid_ident(&id) {
        cx.err(root.get("id").unwrap_or(&Item::None), "id", "must be lower-case letters, digits, `.`, `_`, `-`");
    }
    let name = string(&mut cx, root, "", "name", 80).unwrap_or("").to_string();
    let description = string(&mut cx, root, "", "description", 300).unwrap_or("").to_string();
    let size = root.get("size").and_then(|i| {
        let a: Vec<f64> = i.as_array()?.iter().filter_map(|x| x.as_float().or_else(|| x.as_integer().map(|n| n as f64))).collect();
        (a.len() == 2 && a.iter().all(|v| (16.0..=2000.0).contains(v))).then(|| (a[0] as f32, a[1] as f32))
    });
    if root.get("size").is_some() && size.is_none() {
        cx.err(root.get("size").unwrap_or(&Item::None), "size", "must be [width, height], each 16..2000");
    }
    let refresh_ms = num(&mut cx, root, "", "refresh", 250.0, 60000.0).unwrap_or(1000.0) as u32;
    let mut commands = Vec::new();
    if let Some(c) = root.get("commands") {
        match (trust, c.as_table()) {
            (Trust::UserExec, Some(t)) => {
                for (cname, ci) in t.iter() {
                    let Some(ct) = ci.as_table() else { continue };
                    if !crate::theme::engine::schema::valid_ident(cname) {
                        cx.err(ci, &format!("commands.{cname}"), "invalid command name");
                        continue;
                    }
                    let run = string(&mut cx, ct, &format!("commands.{cname}"), "run", 400).unwrap_or("").to_string();
                    let interval = num(&mut cx, ct, &format!("commands.{cname}"), "interval", 1.0, 3600.0).unwrap_or(5.0) as u32;
                    if run.is_empty() {
                        cx.err(ci, &format!("commands.{cname}.run"), "missing");
                    }
                    cx.commands.insert(cname.to_string());
                    commands.push(CommandDef { name: cname.to_string(), run, interval });
                }
            }
            (Trust::User, _) => cx.err(c, "commands", "`[commands]` needs `widgets.allow_exec = true` in decebal.json"),
            (Trust::Theme, _) => cx.err(c, "commands", "widgets that come with a theme cannot run commands"),
            _ => cx.err(c, "commands", "must be a table"),
        }
    }
    let node = match root.get("root") {
        Some(r) => match r.as_table() {
            Some(rt) => parse_node(&mut cx, rt, "root", 0),
            None => {
                cx.err(r, "root", "must be a table");
                None
            }
        },
        None => {
            cx.errs.push("`root`: missing; a widget has a `[root]` node".into());
            None
        }
    };
    if !cx.errs.is_empty() {
        return Err(cx.errs.join("\n"));
    }
    let root = node.ok_or("`root`: invalid")?;
    Ok(WidgetDef { id, name, description, size, refresh_ms, commands, root, assets: cx.assets, paths: cx.paths, actions: cx.actions })
}

impl Node {
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = r##"
format = 1
id = "card"
name = "Card"
size = [200, 100]
[root]
type = "col"
padding = 12
gap = 4
background = "popup"
radius = "theme"
on_click = "open dashboard"
[[root.children]]
type = "text"
text = "{clock:%H:%M}"
size = 40
weight = "bold"
[[root.children]]
type = "row"
gap = 6
[[root.children.children]]
type = "ring"
value = "cpu.usage"
size = 30
[[root.children.children]]
type = "bar"
value = "mem.usage"
width = 80
"##;

    #[test]
    fn a_nested_widget_parses() {
        let w = parse(CARD, Trust::Theme).unwrap();
        assert_eq!((w.id.as_str(), w.size), ("card", Some((200.0, 100.0))));
        assert_eq!(w.root.children.len(), 2);
        assert_eq!(w.root.children[1].children.len(), 2);
        assert!(matches!(w.root.children[0].kind, Kind::Text { bold: true, .. }));
        assert_eq!(w.root.c.radius, Some(-1.0));
        assert_eq!(w.paths.iter().map(String::as_str).collect::<Vec<_>>(), ["clock", "cpu.usage", "mem.usage"]);
        assert_eq!(w.actions.len(), 1);
    }

    fn err(text: &str) -> String {
        parse(text, Trust::Theme).unwrap_err()
    }

    #[test]
    fn errors_carry_line_and_key() {
        let e = err("format = 1\n[root]\ntype = \"text\"\ntext = \"{cpu.usag}\"\nsiez = 3\n");
        assert!(e.contains("line 4") && e.contains("root.text") && e.contains("did you mean `cpu.usage`"), "{e}");
        assert!(e.contains("line 5") && e.contains("root.siez") && e.contains("unknown key for a `text` node"), "{e}");
        let e = err("format = 1\n[root]\ntype = \"ring\"\n");
        assert!(e.contains("needs `value`"), "{e}");
        let e = err("format = 1\n[root]\ntype = \"blob\"\n");
        assert!(e.contains("unknown node type `blob`"), "{e}");
        let e = err("format = 1\n[root]\ntype = \"text\"\ntext = \"x\"\ncolor = \"mauve\"\n");
        assert!(e.contains("is not a colour"), "{e}");
        let e = err("name = \"x\"\n[root]\ntype = \"spacer\"\n");
        assert!(e.contains("format = 1"), "{e}");
        let e = err("format = 1\n");
        assert!(e.contains("`root`: missing"), "{e}");
    }

    #[test]
    fn theme_widgets_cannot_exec_or_declare_commands() {
        let w = |extra: &str| format!("format = 1\n{extra}\n[root]\ntype = \"button\"\non_click = \"exec rm -rf ~\"\n");
        let e = err(&w(""));
        assert!(e.contains("cannot run commands"), "{e}");
        let e = err("format = 1\n[commands.x]\nrun = \"id\"\n[root]\ntype = \"spacer\"\n");
        assert!(e.contains("commands") && e.contains("cannot run commands"), "{e}");
        let e = err("format = 1\n[root]\ntype = \"button\"\nexec = \"id\"\n");
        assert!(e.contains("cannot run commands") || e.contains("widgets cannot run commands"), "{e}");
        let e = err("format = 1\n[root]\ntype = \"text\"\ntext = \"{cmd.x}\"\n");
        assert!(e.contains("not a declared command"), "{e}");
        // a user widget without the switch gets a pointer to it
        let e = parse(&w(""), Trust::User).unwrap_err();
        assert!(e.contains("allow_exec"), "{e}");
        // and with the switch it works
        let ok = parse(&w(""), Trust::UserExec).unwrap();
        assert_eq!(ok.actions, [Action::Exec("rm -rf ~".into())]);
        let cmd = "format = 1\n[commands.up]\nrun = \"uptime -p\"\ninterval = 10\n[root]\ntype = \"text\"\ntext = \"{cmd.up}\"\n";
        assert_eq!(parse(cmd, Trust::UserExec).unwrap().commands, [CommandDef { name: "up".into(), run: "uptime -p".into(), interval: 10 }]);
        assert!(parse(cmd, Trust::User).is_err());
    }

    #[test]
    fn limits_hold() {
        let mut deep = String::from("format = 1\n[root]\ntype = \"col\"\n");
        let mut path = "root".to_string();
        for _ in 0..10 {
            path.push_str(".children");
            deep.push_str(&format!("[[{path}]]\ntype = \"col\"\n"));
        }
        assert!(err(&deep).contains("nested deeper"));
        let mut wide = String::from("format = 1\n[root]\ntype = \"col\"\n");
        for _ in 0..70 {
            wide.push_str("[[root.children]]\ntype = \"spacer\"\n");
        }
        assert!(err(&wide).contains("more than 64 children"));
        assert!(parse(&"#".repeat(70_000), Trust::Theme).unwrap_err().contains("64 KiB"));
    }

    #[test]
    fn images_must_stay_inside_the_package() {
        for (f, needle) in [("../x.png", "escapes"), ("/etc/x.png", "absolute"), ("a.svg", "png, jpg or webp")] {
            let e = err(&format!("format = 1\n[root]\ntype = \"image\"\nfile = \"{f}\"\n"));
            assert!(e.contains(needle), "{f}: {e}");
        }
        let w = parse("format = 1\n[root]\ntype = \"image\"\nfile = \"img/logo.png\"\n", Trust::Theme).unwrap();
        assert_eq!(w.assets, ["img/logo.png"]);
        assert!(parse("format = 1\n[root]\ntype = \"image\"\nsource = \"media.art\"\n", Trust::Theme).is_ok());
    }

    #[test]
    fn colours() {
        assert_eq!(ColorRef::parse("accent").unwrap(), ColorRef::Token("accent".into(), 1.0));
        assert_eq!(ColorRef::parse("text@0.5").unwrap(), ColorRef::Token("text".into(), 0.5));
        assert_eq!(ColorRef::parse("#ff0000@0.25").unwrap(), ColorRef::Fixed(Rgb(255, 0, 0), 0.25));
        assert!(ColorRef::parse("text@2").is_err() && ColorRef::parse("nope").is_err());
    }
}
