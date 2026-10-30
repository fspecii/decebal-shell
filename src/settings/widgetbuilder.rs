//! "Create widget": a no-code way to make a user widget from a few safe templates (clock, analog clock, system metrics, note).
//!
//! The form edits a [`Draft`] kept in the model. [`Draft::source`] turns it into an ordinary widget file of the safe DSL (no
//! `exec`, no `[commands]`), which is validated with the same parser the desktop uses, saved under [`config::user_dir`] with a
//! name that is unique and never overwrites anything, and added to `decebal.json` as an enabled desktop instance through the same
//! save path as the catalog. The result is a plain user widget: it appears in the lists like any other and can be edited by hand
//! (the file starts with a comment saying so) or removed by deleting the file.

use super::model::{CtlVal, Model};
use super::ui::{pal, Ctl, Item, Kind, Opt, Painter, Row};
use crate::text::{Fonts, Weight};
use crate::theme::Theme;
use crate::widgets::config::{self, Instance, Place};
use crate::widgets::dsl::{self, WidgetDef};
use crate::widgets::layout::{layout, Ctx, Fixed};
use crate::widgets::{data, paint, samples, Trust};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

pub const NAME_MAX: usize = 40;
pub const TEXT_MAX: usize = 200;
const NOTE_LINES: usize = 6;
/// Note card geometry at size 1 (everything scales together): inner width and the body text size, in px.
const NOTE_INNER: f32 = 260.0 - 32.0;
const NOTE_TEXT: f32 = 13.0;
/// The screen the suggested position is checked against (the real monitor is only known to the desktop host).
const SCREEN: (i32, i32, i32, i32) = (0, 0, 1920, 1080);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Clock,
    Analog,
    Metrics,
    Note,
}

impl Template {
    pub const ALL: [Template; 4] = [Template::Clock, Template::Analog, Template::Metrics, Template::Note];

    fn key(self) -> &'static str {
        match self {
            Template::Clock => "clock",
            Template::Analog => "analog-clock",
            Template::Metrics => "system-metrics",
            Template::Note => "note",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Template::Clock => "Digital clock",
            Template::Analog => "Analog clock",
            Template::Metrics => "System metrics",
            Template::Note => "Note",
        }
    }

    fn default_name(self) -> &'static str {
        match self {
            Template::Clock => "My clock",
            Template::Analog => "My analog clock",
            Template::Metrics => "System monitor",
            Template::Note => "My note",
        }
    }

    fn from_key(k: &str) -> Option<Template> {
        Template::ALL.into_iter().find(|t| t.key() == k)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    const ALL: [(Size, &'static str, &'static str); 3] = [(Size::Small, "Small", "small"), (Size::Medium, "Medium", "medium"), (Size::Large, "Large", "large")];

    fn scale(self) -> f32 {
        match self {
            Size::Small => 0.8,
            Size::Medium => 1.0,
            Size::Large => 1.3,
        }
    }

    fn from_key(k: &str) -> Option<Size> {
        Size::ALL.iter().find(|s| s.2 == k).map(|s| s.0)
    }
}

/// What the form currently describes. Nothing is written until Create.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub template: Template,
    pub name: String,
    pub text: String,
    pub size: Size,
    pub anchor: String,
    pub x: i32,
    pub y: i32,
    /// The user typed an offset: stop suggesting one.
    pub placed: bool,
    pub h24: bool,
    pub date: bool,
    pub seconds: bool,
    pub cpu: bool,
    pub mem: bool,
    pub disk: bool,
    pub net: bool,
}

impl Draft {
    pub fn new() -> Draft {
        let (anchor, x, y) = default_place("top-right");
        Draft {
            template: Template::Clock,
            name: Template::Clock.default_name().into(),
            text: String::new(),
            size: Size::Medium,
            anchor: anchor.into(),
            x,
            y,
            placed: false,
            h24: true,
            date: true,
            seconds: true,
            cpu: true,
            mem: true,
            disk: false,
            net: false,
        }
    }

    /// Everything that stops Create, in the order the form shows it. Empty = ready.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let n = self.name.chars().count();
        if n == 0 {
            out.push("Give the widget a name.".to_string());
        } else if n > NAME_MAX {
            out.push(format!("The name is {n} characters; use at most {NAME_MAX}."));
        }
        match self.template {
            Template::Metrics if !(self.cpu || self.mem || self.disk || self.net) => out.push("Choose at least one thing to show.".into()),
            Template::Note => {
                let t = self.text.chars().count();
                if t == 0 {
                    out.push("Type the note to show.".into());
                } else if t > TEXT_MAX {
                    out.push(format!("The note is {t} characters; use at most {TEXT_MAX}."));
                } else if self.note_lines().len() > NOTE_LINES {
                    out.push(format!("The note is too long for the card (at most {NOTE_LINES} lines); shorten it."));
                }
            }
            _ => {}
        }
        out
    }

    fn note_lines(&self) -> Vec<String> {
        wrap(&self.text, NOTE_INNER / NOTE_TEXT)
    }

    /// The widget file for this draft under the id `id`. Fails with the first problem, or with the parser's message.
    pub fn source(&self, id: &str) -> Result<String, String> {
        if let Some(p) = self.problems().into_iter().next() {
            return Err(p);
        }
        self.build(id, &self.name, &self.text)
    }

    /// Like [`Self::source`] for the live preview: a missing name or note shows a placeholder instead of an error.
    pub fn preview_source(&self) -> Result<String, String> {
        let mut d = self.clone();
        if d.name.is_empty() {
            d.name = "My widget".into();
        }
        if d.template == Template::Note && d.text.is_empty() {
            d.text = "Your note appears here.".into();
        }
        if d.template == Template::Metrics && !(d.cpu || d.mem || d.disk || d.net) {
            d.cpu = true;
        }
        let id = slug(&d.name, d.template.key());
        let (name, text) = (d.name.clone(), d.text.clone());
        d.build(&id, &name, &text)
    }

    fn build(&self, id: &str, name: &str, text: &str) -> Result<String, String> {
        let mut doc = Doc(format!(
            "# Created with Decebal Settings > Widgets > Create widget ({}).\n# This is a normal user widget: edit it with any text editor, or delete the file to remove it.\nformat = 1\nid = {}\nname = {}\ndescription = {}\n",
            self.template.key(),
            q(id),
            q(name),
            q(&format!("{} created in Decebal Settings.", self.template.label()))
        ));
        let k = self.size.scale();
        let card = |width: Option<f32>, pad: [f32; 2], gap: f32, align: &str, click: bool| {
            let mut p = vec![("type", q("col")), ("padding", format!("[{}, {}]", num(pad[0] * k), num(pad[1] * k))), ("gap", num(gap * k)), ("align", q(align))];
            if let Some(w) = width {
                p.push(("width", num(w * k)));
            }
            p.extend([("background", q("popup")), ("radius", q("theme")), ("border", q("border@0.6"))]);
            if click {
                p.push(("on_click", q("open dashboard")));
            }
            p
        };
        match self.template {
            Template::Clock => {
                let (fmt, fs) = if self.h24 { ("{clock:%H:%M}", 54.0) } else { ("{clock:%I:%M %p}", 40.0) };
                let mut root = card(Some(236.0), [14.0, 20.0], 2.0, "center", false);
                root.push(("justify", q("center")));
                doc.node(0, &root);
                doc.node(1, &[("type", q("text")), ("text", q(fmt)), ("size", num(fs * k)), ("weight", q("bold"))]);
                if self.date {
                    doc.node(1, &[("type", q("text")), ("text", q("{clock:%A, %e %B}")), ("size", num(14.0 * k)), ("color", q("text_secondary"))]);
                }
            }
            Template::Analog => {
                doc.node(0, &card(None, [14.0, 14.0], 0.0, "center", false));
                doc.node(
                    1,
                    &[("type", q("analog-clock")), ("size", num(128.0 * k)), ("color", q("text")), ("second_color", q("accent")), ("numerals", "true".into()), ("seconds", self.seconds.to_string())],
                );
            }
            Template::Metrics => {
                doc.node(0, &card(Some(280.0), [16.0, 16.0], 6.0, "stretch", true));
                doc.node(1, &[("type", q("text")), ("text", q(&esc(&fit(name, (280.0 - 32.0) / (15.0 * 1.1))))), ("size", num(15.0 * k)), ("weight", q("bold")), ("color", q("text"))]);
                let mut meter = |label: &str, value: &str, path: &str, color: &str| {
                    doc.node(1, &[("type", q("row")), ("align", q("center"))]);
                    doc.node(2, &[("type", q("text")), ("text", q(label)), ("size", num(12.0 * k)), ("color", q("text_secondary"))]);
                    doc.node(2, &[("type", q("spacer"))]);
                    doc.node(2, &[("type", q("text")), ("text", q(value)), ("size", num(12.0 * k)), ("weight", q("bold")), ("color", q("text"))]);
                    doc.node(1, &[("type", q("bar")), ("value", q(path)), ("height", num(6.0 * k)), ("color", q(color)), ("track", q("border"))]);
                };
                if self.cpu {
                    meter("Processor", "{cpu.usage:pct|--}", "cpu.usage", "accent");
                }
                if self.mem {
                    meter("Memory", "{mem.usage:pct|--}", "mem.usage", "ok");
                }
                if self.disk {
                    meter("Disk (root)", "{disk.usage:pct|--}", "disk.usage", "warn");
                }
                if self.net {
                    for (label, value) in [("Download", "{net.rx:rate|--}"), ("Upload", "{net.tx:rate|--}")] {
                        doc.node(1, &[("type", q("row")), ("align", q("center"))]);
                        doc.node(2, &[("type", q("text")), ("text", q(label)), ("size", num(12.0 * k)), ("color", q("text_secondary"))]);
                        doc.node(2, &[("type", q("spacer"))]);
                        doc.node(2, &[("type", q("text")), ("text", q(value)), ("size", num(12.0 * k)), ("weight", q("bold")), ("color", q("text"))]);
                    }
                }
            }
            Template::Note => {
                doc.node(0, &card(Some(260.0), [16.0, 16.0], 6.0, "stretch", false));
                doc.node(1, &[("type", q("text")), ("text", q(&esc(&fit(name, NOTE_INNER / (15.0 * 1.1))))), ("size", num(15.0 * k)), ("weight", q("bold")), ("color", q("text"))]);
                for line in wrap(text, NOTE_INNER / NOTE_TEXT) {
                    doc.node(1, &[("type", q("text")), ("text", q(&esc(&line))), ("size", num(13.0 * k)), ("color", q("text"))]);
                }
            }
        }
        // The file we write must be one the desktop accepts without `allow_exec`.
        let def = dsl::parse(&doc.0, Trust::User).map_err(|e| format!("Internal widget template error: {e}"))?;
        if !def.commands.is_empty() || def.actions.iter().any(|a| matches!(a, crate::widgets::actions::Action::Exec(_))) {
            return Err("Internal widget template error: templates cannot run commands".into());
        }
        Ok(doc.0)
    }

    /// Moves the suggested position clear of the cards already on the desktop, unless the user chose one.
    pub fn auto_place(&mut self, others: &[(Instance, (i32, i32))]) {
        if self.placed {
            return;
        }
        let Ok(def) = self.preview_source().and_then(|s| dsl::parse(&s, Trust::User)) else { return };
        let (w, h) = natural_size(&def);
        let (anchor, x, mut y) = default_place(&self.anchor);
        let dir = if anchor.starts_with("bottom") { -1 } else { 1 };
        let free = |y: i32| {
            let (px, py) = config::place_at(anchor, x, y, SCREEN, w, h);
            let inside = py >= SCREEN.1 && py + h <= SCREEN.1 + SCREEN.3;
            let clear = others.iter().all(|(o, (ow, oh))| {
                let (ox, oy) = config::place_at(&o.anchor, o.x, o.y, SCREEN, *ow, *oh);
                px + w <= ox || ox + ow <= px || py + h <= oy || oy + oh <= py
            });
            (inside, clear)
        };
        let start = y;
        for _ in 0..16 {
            let (inside, clear) = free(y);
            if !inside {
                y = start;
                break;
            }
            if clear {
                break;
            }
            y += dir * (h + 16);
        }
        self.x = x;
        self.y = y.clamp(-4000, 4000);
    }
}

impl Default for Draft {
    fn default() -> Self {
        Draft::new()
    }
}

/// Where a card goes by default for `anchor`: 40 px in from the edges it is attached to.
fn default_place(anchor: &str) -> (&'static str, i32, i32) {
    let a = crate::theme::engine::schema::ANCHORS.iter().find(|a| **a == anchor).copied().unwrap_or("top-right");
    let x = if a.ends_with("left") || a == "left" {
        40
    } else if a.ends_with("right") || a == "right" {
        -40
    } else {
        0
    };
    let y = if a.starts_with("top") {
        70
    } else if a.starts_with("bottom") {
        -70
    } else {
        0
    };
    (a, x, y)
}

fn natural_size(def: &WidgetDef) -> (i32, i32) {
    let it = layout(def, &Ctx { data: &data::sample(), metrics: &Fixed, theme_radius: 10.0 }, None);
    (it.rect.w.ceil() as i32, it.rect.h.ceil() as i32)
}

/// Enabled desktop cards with the size they will have, for [`Draft::auto_place`].
fn occupied(widgets: &Value, themes_dir: &Path) -> Vec<(Instance, (i32, i32))> {
    let allow_exec = widgets.get("allow_exec").and_then(Value::as_bool).unwrap_or(false);
    let raw = widgets.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    raw.iter()
        .filter_map(Instance::from_json)
        .filter(|i| i.enabled && i.place == Place::Desktop)
        .map(|i| {
            let size = config::resolve(&i, themes_dir, allow_exec).map(|r| r.def.size.map(|(w, h)| (w as i32, h as i32)).unwrap_or_else(|| natural_size(&r.def))).unwrap_or((240, 140));
            (i, size)
        })
        .collect()
}

// ---------------------------------------------------------------- widget file text

struct Doc(String);

impl Doc {
    /// Appends the node table at `depth` (0 = `[root]`, 1 = `[[root.children]]`, ...).
    fn node(&mut self, depth: usize, props: &[(&str, String)]) {
        self.0.push('\n');
        self.0.push_str(&if depth == 0 { "[root]".to_string() } else { format!("[[root{}]]", ".children".repeat(depth)) });
        self.0.push('\n');
        for (k, v) in props {
            self.0.push_str(&format!("{k} = {v}\n"));
        }
    }
}

/// A TOML string, quoted and escaped.
fn q(s: &str) -> String {
    toml_edit::Value::from(s).to_string().trim().to_string()
}

/// Literal text inside a widget format string: braces would start a data reference.
fn esc(s: &str) -> String {
    s.replace('{', "{{").replace('}', "}}")
}

fn num(v: f32) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r:.1}")
    }
}

/// A deliberately generous estimate of a character's width in em (the widest common fonts, so text never leaves its card).
fn em(c: char) -> f32 {
    match c {
        'i' | 'j' | 'l' | 't' | 'f' | 'I' | '!' | '.' | ',' | ':' | ';' | '\'' | '|' | ' ' => 0.32,
        'm' | 'w' | 'M' | 'W' | '@' => 0.95,
        c if matches!(c as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD) => {
            1.0
        }
        c if c.is_uppercase() => 0.74,
        c if c.is_ascii_digit() => 0.6,
        _ => 0.58,
    }
}

fn em_width(s: &str) -> f32 {
    s.chars().map(em).sum()
}

/// `s` cut to at most `budget` em, ending in an ellipsis when something was left out.
fn fit(s: &str, budget: f32) -> String {
    if em_width(s) <= budget {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = em('…');
    for c in s.chars() {
        w += em(c);
        if w > budget {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// Greedy word wrap into lines of at most `budget` em; a word wider than a line is split.
fn wrap(text: &str, budget: f32) -> Vec<String> {
    let budget = budget.max(2.0);
    let mut lines = Vec::new();
    let (mut cur, mut w) = (String::new(), 0.0f32);
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let ww = em_width(word);
        if ww > budget {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                w = 0.0;
            }
            for c in word.chars() {
                if w + em(c) > budget {
                    lines.push(std::mem::take(&mut cur));
                    w = 0.0;
                }
                cur.push(c);
                w += em(c);
            }
        } else if cur.is_empty() {
            cur.push_str(word);
            w = ww;
        } else if w + em(' ') + ww > budget {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
            w = ww;
        } else {
            cur.push(' ');
            cur.push_str(word);
            w += em(' ') + ww;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Control characters and runs of whitespace become single spaces, so a pasted newline cannot break a line or the file.
fn clean(s: &str, cap: usize) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").chars().take(cap).collect()
}

/// File-name stem for a widget: lower-case ASCII letters, digits and single dashes; `fallback` if nothing is left.
pub fn slug(name: &str, fallback: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.is_empty() && !s.ends_with('-') {
            s.push('-');
        }
    }
    s.truncate(32);
    let s = s.trim_end_matches('-');
    if s.is_empty() {
        fallback.to_string()
    } else {
        s.to_string()
    }
}

/// Creates `<dir>/<stem>.toml` (`stem`, `stem-2`, ...) holding `make(stem)`, using the first name that is free. An existing file,
/// symlink or shipped widget name is never reused or replaced: the file is made with `create_new`.
fn write_new(dir: &Path, stem: &str, taken: &dyn Fn(&str) -> bool, make: &dyn Fn(&str) -> Result<String, String>) -> Result<(String, PathBuf), String> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create the widgets folder {}: {e}", dir.display()))?;
    for n in 1..=99 {
        let id = if n == 1 { stem.to_string() } else { format!("{stem}-{n}") };
        let path = dir.join(format!("{id}.toml"));
        if samples::source(&id).is_some() || taken(&id) || std::fs::symlink_metadata(&path).is_ok() {
            continue;
        }
        let text = make(&id)?;
        let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not save {}: {e}", path.display())),
        };
        if let Err(e) = file.write_all(text.as_bytes()).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(format!("Could not save {}: {e}", path.display()));
        }
        return Ok((id, path));
    }
    Err("There are too many widgets with this name; choose a different name.".into())
}

// ---------------------------------------------------------------- the form

const PREVIEW_NOTE: &str = "Preview with sample data (Friday 23 October, 14:07; CPU 37%, memory 62%). Your widget shows live values.";

pub fn items(m: &Model) -> Vec<Item> {
    let mut out = vec![Item::Header("Create a widget".into())];
    let Some(d) = &m.widget_builder else {
        out.push(Item::Card(vec![Row::new("Make your own widget")
            .detail("Pick a clock, system meters or a note, choose its size and place, and add it to your desktop. No code needed.")
            .ctl(Ctl::new("wg:new:open", Kind::Button { label: "Create widget…".into(), danger: false }))]));
        return out;
    };
    match d.preview_source() {
        Ok(toml) => out.push(Item::WidgetPreview { toml, caption: PREVIEW_NOTE.into() }),
        Err(e) => out.push(Item::Note { text: e, warn: true }),
    }
    let opts = |list: Vec<(&str, &str)>| -> Vec<Opt> { list.into_iter().map(|(l, v)| (l.to_string(), v.to_string())).collect() };
    let toggle = |id: &str, label: &str, detail: &str, on: bool| Row::new(label).detail(detail).ctl(Ctl::new(format!("wg:new:{id}"), Kind::Toggle(on)));
    let types = opts(Template::ALL.iter().map(|t| (t.label(), t.key())).collect());
    let mut rows = vec![
        Row::new("Type").detail("What the widget shows").ctl(Ctl::new("wg:new:template", Kind::Popup(types, Template::ALL.iter().position(|t| *t == d.template)))),
        Row::new("Name")
            .detail(format!("Shown in your widget list{}", if matches!(d.template, Template::Metrics | Template::Note) { " and as the card title" } else { "" }))
            .ctl(Ctl::new("wg:new:name", Kind::Text { val: d.name.clone(), hint: "For example: Desk clock".into() })),
    ];
    match d.template {
        Template::Clock => {
            rows.push(toggle("h24", "24-hour time", "Off shows 12-hour time with AM/PM", d.h24));
            rows.push(toggle("date", "Show the date", "Weekday, day and month under the time", d.date));
        }
        Template::Analog => rows.push(toggle("seconds", "Seconds hand", "A thin hand that sweeps once a minute", d.seconds)),
        Template::Metrics => {
            rows.push(toggle("cpu", "Processor", "CPU load", d.cpu));
            rows.push(toggle("mem", "Memory", "RAM in use", d.mem));
            rows.push(toggle("disk", "Disk", "How full the root drive is", d.disk));
            rows.push(toggle("net", "Network", "Download and upload speed", d.net));
        }
        Template::Note => rows.push(
            Row::new("Note")
                .detail(format!("Up to {TEXT_MAX} characters, shown as typed; long notes wrap onto several lines"))
                .ctl(Ctl::new("wg:new:text", Kind::Text { val: d.text.clone(), hint: "Type your note".into() })),
        ),
    }
    rows.push(
        Row::new("Size")
            .detail("Scales the text and the card")
            .ctl(Ctl::new("wg:new:size", Kind::Seg(opts(Size::ALL.iter().map(|s| (s.1, s.2)).collect()), Size::ALL.iter().position(|s| s.0 == d.size)))),
    );
    out.push(Item::Card(rows));
    out.push(Item::Header("Position".into()));
    let anchors: Vec<Opt> = crate::theme::engine::schema::ANCHORS.iter().map(|a| (a.replace('-', " "), (*a).to_string())).collect();
    let selected = anchors.iter().position(|(_, a)| *a == d.anchor);
    let stepper = |id: &str, v: i32| Ctl::new(id, Kind::Stepper { v: v.into(), min: -4000.0, max: 4000.0, step: 4.0, unit: " px".into() });
    out.push(Item::Card(vec![
        Row::new("Position").detail("Anchor the widget to a screen edge or corner; it is placed clear of your other widgets").ctl(Ctl::new("wg:new:anchor", Kind::Popup(anchors, selected))),
        Row::new("Horizontal offset").detail("Move left with negative values, right with positive values").ctl(stepper("wg:new:x", d.x)),
        Row::new("Vertical offset").detail("Move up with negative values, down with positive values").ctl(stepper("wg:new:y", d.y)),
    ]));
    let problems = d.problems();
    if let Some(p) = problems.first() {
        out.push(Item::Note { text: p.clone(), warn: true });
    }
    let mut create = Ctl::new("wg:new:create", Kind::Button { label: "Create widget".into(), danger: false });
    create.enabled = problems.is_empty();
    out.push(Item::Card(vec![Row::new("Add to desktop")
        .detail("Saves the widget file in your widgets folder and shows it on the desktop")
        .ctl(create)
        .ctl(Ctl::new("wg:new:cancel", Kind::Button { label: "Cancel".into(), danger: false }))]));
    out
}

fn read_widgets(m: &Model) -> Result<Value, String> {
    super::widgetspane::read_widgets(&m.paths.shell_json())
}

fn suggest(m: &mut Model) {
    let widgets = read_widgets(m).ok();
    if let Some(d) = m.widget_builder.as_mut() {
        if let Some(w) = widgets {
            d.auto_place(&occupied(&w, &crate::theme::live::Source::discover().themes_dir));
        }
    }
}

/// Handles `wg:new:<action>`.
pub fn apply(m: &mut Model, action: &str, value: CtlVal) {
    m.dirty = true;
    match (action, &value) {
        ("open", CtlVal::Click) => {
            m.widget_builder = Some(Draft::new());
            suggest(m);
            return;
        }
        ("cancel", CtlVal::Click) => {
            m.widget_builder = None;
            return;
        }
        ("create", CtlVal::Click) => return create(m),
        _ => {}
    }
    let Some(d) = m.widget_builder.as_mut() else { return };
    let mut moved = true;
    match (action, value) {
        ("template", CtlVal::Str(k)) => match Template::from_key(&k) {
            Some(t) => {
                if d.name.is_empty() || Template::ALL.iter().any(|o| o.default_name() == d.name) {
                    d.name = t.default_name().into();
                }
                d.template = t;
            }
            None => return m.say("That widget type is not available", true),
        },
        ("name", CtlVal::Str(s)) => {
            d.name = clean(&s, NAME_MAX * 4);
            moved = false;
        }
        ("text", CtlVal::Str(s)) => d.text = clean(&s, TEXT_MAX * 4),
        ("size", CtlVal::Str(k)) => match Size::from_key(&k) {
            Some(s) => d.size = s,
            None => return m.say("That size is not available", true),
        },
        ("anchor", CtlVal::Str(a)) if crate::theme::engine::schema::ANCHORS.contains(&a.as_str()) => {
            d.anchor = a;
            d.placed = false;
        }
        (key @ ("x" | "y"), CtlVal::Num(n)) if n.is_finite() && (-4000.0..=4000.0).contains(&n) => {
            if key == "x" {
                d.x = n.round() as i32;
            } else {
                d.y = n.round() as i32;
            }
            d.placed = true;
            moved = false;
        }
        ("h24", CtlVal::Bool(b)) => d.h24 = b,
        ("date", CtlVal::Bool(b)) => d.date = b,
        ("seconds", CtlVal::Bool(b)) => d.seconds = b,
        ("cpu", CtlVal::Bool(b)) => d.cpu = b,
        ("mem", CtlVal::Bool(b)) => d.mem = b,
        ("disk", CtlVal::Bool(b)) => d.disk = b,
        ("net", CtlVal::Bool(b)) => d.net = b,
        _ => return m.say("That widget setting is not valid", true),
    }
    if moved {
        suggest(m);
    }
}

/// Saves the widget file, then adds it as an enabled desktop card. On any failure nothing is left behind.
fn create(m: &mut Model) {
    let Some(d) = m.widget_builder.clone() else { return };
    if let Some(p) = d.problems().into_iter().next() {
        return m.say(p, true);
    }
    let mut widgets = match read_widgets(m) {
        Ok(w) => w,
        Err(e) => return m.say(e, true),
    };
    let items = widgets.as_object_mut().expect("checked object").entry("items").or_insert_with(|| json!([])).as_array_mut().expect("checked array");
    if items.len() >= super::widgetspane::LIMIT {
        return m.say(format!("You can add up to {} widgets. Remove one before adding another.", super::widgetspane::LIMIT), true);
    }
    // a saved card that names a user widget which has no file yet must not silently pick up the new one
    let used: Vec<String> = items.iter().filter_map(Instance::from_json).filter(|i| i.source.is_none()).map(|i| i.use_).collect();
    let (id, path) = match write_new(&config::user_dir(), &slug(&d.name, d.template.key()), &|id| used.iter().any(|u| u == id), &|id| d.source(id)) {
        Ok(v) => v,
        Err(e) => return m.say(e, true),
    };
    let mut inst = Instance::desktop(&id, &d.anchor, d.x, d.y);
    inst.enabled = true;
    items.push(inst.to_json());
    widgets["enabled"] = json!(true);
    let done = format!("Created “{}” and added it to your desktop. Its file is {}", d.name, path.display());
    if super::widgetspane::save(m, widgets, &done) {
        m.widget_builder = None;
    } else {
        let _ = std::fs::remove_file(&path);
    }
}

// ---------------------------------------------------------------- preview

thread_local! {
    static PREVIEW: RefCell<Option<(String, Theme, Rc<Pixmap>)>> = const { RefCell::new(None) };
}

/// The widget drawn by the desktop's own painter with sample data, cached until the widget text or the theme changes.
fn render(toml: &str, fonts: &mut Fonts) -> Option<Rc<Pixmap>> {
    let th = crate::theme::th();
    if let Some(hit) = PREVIEW.with(|c| c.borrow().as_ref().filter(|(t, theme, _)| t == toml && theme == &*th).map(|(_, _, p)| p.clone())) {
        return Some(hit);
    }
    let def = dsl::parse(toml, Trust::User).ok()?;
    let pm = Rc::new(paint::render(&def, &data::sample(), &th, fonts, Path::new("."), 2)?);
    PREVIEW.with(|c| *c.borrow_mut() = Some((toml.to_string(), (*th).clone(), pm.clone())));
    Some(pm)
}

/// Draws the preview card at `(x0, y)` over `cw` pixels and returns the y below it.
pub fn draw_preview(pm: &mut Pixmap, p: &mut Painter, toml: &str, caption: &str, x0: f32, y: f32, cw: f32) -> f32 {
    let scale = p.s;
    let u = |v: f32| v * scale;
    let pad = u(16.0);
    let img = p.fonts.as_deref_mut().and_then(|f| render(toml, f));
    let (iw, ih) = img.as_ref().map(|i| (i.width() as f32, i.height() as f32)).unwrap_or((0.0, u(48.0)));
    let k = if iw > 0.0 { scale.min((cw - 2.0 * pad) / iw) } else { 1.0 };
    let h = ih * if iw > 0.0 { k } else { 1.0 } + 2.0 * pad;
    crate::gfx::fill_rounded(pm, x0, y, cw, h, u(10.0), pal::card());
    match img {
        Some(img) => {
            let ts = Transform::from_scale(k, k).post_translate((x0 + (cw - iw * k) / 2.0).round(), (y + pad).round());
            pm.draw_pixmap(0, 0, Pixmap::as_ref(&img), &PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() }, ts, None);
        }
        None => p.text_center(pm, "Preview unavailable", x0 + cw / 2.0, y + h / 2.0, u(12.0), Weight::Regular, pal::faint()),
    }
    let caption = p.fit(caption, cw, u(11.5), Weight::Regular);
    p.text_center(pm, &caption, x0 + cw / 2.0, y + h + u(14.0), u(11.5), Weight::Regular, pal::faint());
    y + h + u(34.0)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};

    /// `DECEBAL_WIDGETS` is process-wide: tests that point it at a sandbox take turns, and it is restored afterwards.
    static ENV: Mutex<()> = Mutex::new(());

    pub struct Sandbox {
        _turn: MutexGuard<'static, ()>,
        before: Option<std::ffi::OsString>,
        pub dir: PathBuf,
    }

    /// An empty widgets folder (not yet created) under `parent` used as the user widget folder until the guard drops.
    pub fn sandbox(parent: &Path) -> Sandbox {
        let turn = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = parent.join("user-widgets");
        let _ = std::fs::remove_dir_all(&dir);
        let before = std::env::var_os("DECEBAL_WIDGETS");
        std::env::set_var("DECEBAL_WIDGETS", &dir);
        Sandbox { _turn: turn, before, dir }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            match &self.before {
                Some(v) => std::env::set_var("DECEBAL_WIDGETS", v),
                None => std::env::remove_var("DECEBAL_WIDGETS"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::sandbox;
    use super::*;
    use crate::settings::model::tests_support::model_with;

    fn save(m: &Model, widgets: Value) {
        std::fs::write(m.paths.shell_json(), json!({"theme": {"name": "windows11", "palette": "nord"}, "widgets": widgets}).to_string()).unwrap();
    }

    fn load(m: &Model) -> Value {
        serde_json::from_str(&std::fs::read_to_string(m.paths.shell_json()).unwrap()).unwrap()
    }

    fn all_drafts() -> Vec<Draft> {
        let mut out = Vec::new();
        for template in Template::ALL {
            for (size, _, _) in Size::ALL {
                for flag in [false, true] {
                    out.push(Draft {
                        template,
                        size,
                        h24: flag,
                        date: flag,
                        seconds: flag,
                        cpu: true,
                        mem: flag,
                        disk: flag,
                        net: flag,
                        text: "Pick up the parcel & call Ana {today}: \"bring\" the café keys 日本語のメモ".into(),
                        ..Draft::new()
                    });
                }
            }
        }
        out
    }

    #[test]
    fn every_template_makes_a_valid_safe_widget_with_the_chosen_id_and_name() {
        for d in all_drafts() {
            let src = d.source("my-widget").unwrap_or_else(|e| panic!("{:?}: {e}", d.template));
            for trust in [Trust::User, Trust::Theme, Trust::UserExec] {
                let def = dsl::parse(&src, trust).unwrap_or_else(|e| panic!("{:?} {trust:?}: {e}\n{src}", d.template));
                assert_eq!((def.id.as_str(), def.name.as_str()), ("my-widget", d.name.as_str()));
                assert!(def.commands.is_empty() && def.actions.iter().all(|a| !matches!(a, crate::widgets::actions::Action::Exec(_))));
            }
            assert!(!src.contains("exec") && !src.contains("[commands"), "{src}");
        }
    }

    #[test]
    fn user_text_is_shown_literally_and_cannot_inject_data_references_or_toml() {
        let tricky = "{cpu.usage} \"]\n[root]\ntype = \"button\"\non_click = \"exec rm -rf ~\" }}";
        let d = Draft { template: Template::Note, name: "{mem.usage} \"quoted\" \\ name".into(), text: clean(tricky, TEXT_MAX), ..Draft::new() };
        let src = d.source("note").unwrap();
        let def = dsl::parse(&src, Trust::Theme).unwrap();
        assert!(def.paths.is_empty(), "text must not read data: {:?}", def.paths);
        let it = layout(&def, &Ctx { data: &data::sample(), metrics: &Fixed, theme_radius: 10.0 }, None);
        let drawn = crate::widgets::layout::dump(&it);
        assert!(drawn.contains("{mem.usage}") && drawn.contains("{cpu.usage}"), "{drawn}");
        assert!(!drawn.contains("37%"));
        assert_eq!(def.name, d.name);
    }

    #[test]
    fn names_and_text_are_validated() {
        let ok = Draft::new();
        assert!(ok.problems().is_empty());
        assert!(Draft { name: String::new(), ..ok.clone() }.source("x").is_err());
        assert!(Draft { name: "x".repeat(NAME_MAX + 1), ..ok.clone() }.problems()[0].contains("at most"));
        let note = Draft { template: Template::Note, ..ok.clone() };
        assert_eq!(note.problems(), vec!["Type the note to show.".to_string()]);
        assert!(Draft { text: "a".repeat(TEXT_MAX + 1), ..note.clone() }.problems()[0].contains("at most"));
        assert!(Draft { text: "word ".repeat(20), ..note.clone() }.problems().is_empty());
        assert!(Draft { text: "word ".repeat(35), ..note }.problems()[0].contains("too long for the card"));
        assert!(Draft { template: Template::Metrics, cpu: false, mem: false, ..ok }.problems()[0].contains("at least one"));
        assert_eq!(clean("  a\tb\n\u{7}c  ", 10), "a b c");
    }

    #[test]
    fn wrapping_keeps_words_whole_splits_long_ones_and_counts_wide_characters() {
        assert_eq!(wrap("alpha beta gamma delta", 7.0), ["alpha beta", "gamma delta"]);
        assert_eq!(wrap("abcdefgh", 2.4), ["abcd", "efgh"]);
        assert_eq!(wrap("日本語のメモ", 4.0), ["日本語の", "メモ"]);
        assert!(wrap("", 10.0).is_empty());
        assert_eq!(fit("short", 10.0), "short");
        assert!(fit(&"W".repeat(30), 5.0).ends_with('…') && em_width(&fit(&"W".repeat(30), 5.0)) <= 5.0);
    }

    #[test]
    fn file_names_are_safe_and_never_collide_or_replace_anything() {
        assert_eq!(slug("Desk Clock!", "clock"), "desk-clock");
        assert_eq!(slug("../../etc/passwd", "note"), "etc-passwd");
        assert_eq!(slug("日本語", "note"), "note");
        assert_eq!(slug(&"ab ".repeat(40), "x").len(), 32);
        let dir = std::env::temp_dir().join(format!("decebal-builder-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.toml"), "precious").unwrap();
        std::os::unix::fs::symlink("/nonexistent/target", dir.join("mine-2.toml")).unwrap();
        std::os::unix::fs::symlink(dir.join("mine.toml"), dir.join("mine-3.toml")).unwrap();
        let d = Draft::new();
        let none = |_: &str| false;
        let (id, path) = write_new(&dir, "mine", &none, &|id| d.source(id)).unwrap();
        assert_eq!(id, "mine-4");
        assert_eq!(std::fs::read_to_string(dir.join("mine.toml")).unwrap(), "precious");
        assert!(std::fs::read_to_string(path).unwrap().contains("id = \"mine-4\""));
        // shipped widget names and names a saved card already uses are skipped too
        assert_eq!(write_new(&dir, "clock-card", &none, &|id| d.source(id)).unwrap().0, "clock-card-2");
        assert_eq!(write_new(&dir, "taken", &|id| id == "taken", &|id| d.source(id)).unwrap().0, "taken-2");
        // a failing template leaves no file behind
        assert!(write_new(&dir, "fails", &none, &|_| Err("no".into())).is_err());
        assert!(!dir.join("fails.toml").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn creating_saves_the_file_adds_an_enabled_card_and_keeps_everything_else() {
        let (mut m, _) = model_with("[general]\n");
        let sb = sandbox(&m.paths.shell_dir);
        let dir = sb.dir.clone();
        let kept = json!({"use":"builtin:cpu-ram-ring","enabled":false,"anchor":"bottom-left","x":9,"future":{"a":1},"options":{"o":2}});
        save(&m, json!({"enabled":false,"allow_exec":false,"items":[kept.clone()],"future":"kept","theme_baseline":{"enabled":true,"items":[kept.clone()]},"theme_widgets_absent":true}));
        m.apply("wg:new:open", CtlVal::Click);
        m.apply("wg:new:template", CtlVal::Str("note".into()));
        m.apply("wg:new:name", CtlVal::Str("  Shopping   list ".into()));
        m.apply("wg:new:text", CtlVal::Str("Milk, eggs and a very long item that needs to wrap onto another line".into()));
        m.apply("wg:new:size", CtlVal::Str("large".into()));
        m.apply("wg:new:anchor", CtlVal::Str("bottom-left".into()));
        m.apply("wg:new:create", CtlVal::Click);
        assert!(m.notice.as_ref().is_some_and(|n| !n.error), "{:?}", m.notice.as_ref().map(|n| &n.text));
        assert!(m.widget_builder.is_none());
        let file = dir.join("shopping-list.toml");
        let text = std::fs::read_to_string(&file).unwrap();
        let def = dsl::parse(&text, Trust::User).unwrap();
        assert_eq!(def.name, "Shopping list");
        let doc = load(&m);
        let w = &doc["widgets"];
        assert_eq!(w["items"][0], kept, "existing card is untouched");
        let added = Instance::from_json(&w["items"][1]).unwrap();
        assert_eq!((added.use_.as_str(), added.source.as_deref(), added.enabled, added.place, added.anchor.as_str()), ("shopping-list", None, true, Place::Desktop, "bottom-left"));
        assert_eq!(w["enabled"], true);
        assert_eq!(w["future"], "kept");
        assert_eq!(w["theme_baseline"]["items"][0], kept);
        assert!(w.get("theme_widgets_absent").is_none(), "visible widgets clear the hidden-by-theme marker");
        assert_eq!(doc["theme"]["palette"], "nord");
        assert!(config::resolve(&added, Path::new("/nonexistent"), false).is_ok(), "the desktop host can load it");
        // the new card makes the layout custom, so a theme with widgets asks before replacing it
        assert!(crate::theme::engine::apply::custom_widgets(w));
        // a second widget with the same name gets its own file and does not replace the first
        m.apply("wg:new:open", CtlVal::Click);
        m.apply("wg:new:name", CtlVal::Str("Shopping list".into()));
        m.apply("wg:new:create", CtlVal::Click);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        assert!(dir.join("shopping-list-2.toml").is_file());
        assert_eq!(load(&m)["widgets"]["items"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn a_failed_save_leaves_no_widget_file_and_invalid_config_is_not_replaced() {
        let (mut m, _) = model_with("[general]\n");
        let sb = sandbox(&m.paths.shell_dir);
        let dir = sb.dir.clone();
        std::fs::write(m.paths.shell_json(), "{not json").unwrap();
        m.apply("wg:new:open", CtlVal::Click);
        m.apply("wg:new:create", CtlVal::Click);
        assert_eq!(std::fs::read_to_string(m.paths.shell_json()).unwrap(), "{not json");
        assert!(m.notice.as_ref().unwrap().error && !dir.exists());
        // too many widgets
        save(&m, json!({"items": (0..super::super::widgetspane::LIMIT).map(|_| json!({"use":"builtin:clock-card"})).collect::<Vec<_>>()}));
        m.apply("wg:new:create", CtlVal::Click);
        assert!(m.notice.as_ref().unwrap().text.contains("up to") && !dir.exists());
        // the settings file cannot be written: the new file is removed again
        save(&m, json!({"items": []}));
        std::fs::create_dir_all(&dir).unwrap();
        let before = std::fs::read(m.paths.shell_json()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&m.paths.shell_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::File::create(m.paths.shell_dir.join("probe")).is_ok() {
            std::fs::set_permissions(&m.paths.shell_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            return; // running as a user that ignores permissions
        }
        m.apply("wg:new:create", CtlVal::Click);
        std::fs::set_permissions(&m.paths.shell_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(m.notice.as_ref().unwrap().error);
        assert_eq!(std::fs::read(m.paths.shell_json()).unwrap(), before);
        assert!(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) == 0, "no orphan widget file");
        assert!(m.widget_builder.is_some(), "the form stays open so nothing typed is lost");
    }

    #[test]
    fn new_cards_are_placed_clear_of_existing_ones_until_the_user_picks_a_spot() {
        let (mut m, _) = model_with("[general]\n");
        let _sb = sandbox(&m.paths.shell_dir);
        save(&m, json!({"items":[]}));
        m.apply("wg:new:open", CtlVal::Click);
        let first = m.widget_builder.clone().unwrap();
        assert_eq!((first.anchor.as_str(), first.x, first.y), ("top-right", -40, 70));
        m.apply("wg:new:create", CtlVal::Click);
        m.apply("wg:new:open", CtlVal::Click);
        let second = m.widget_builder.clone().unwrap();
        assert!(second.y > first.y + 100, "second card is below the first: {}", second.y);
        m.apply("wg:new:y", CtlVal::Num(12.0));
        m.apply("wg:new:size", CtlVal::Str("small".into()));
        assert_eq!(m.widget_builder.as_ref().unwrap().y, 12, "a chosen offset is kept");
        m.apply("wg:new:anchor", CtlVal::Str("bottom".into()));
        let d = m.widget_builder.as_ref().unwrap();
        assert_eq!((d.x, d.y), (0, -70), "changing the anchor suggests a spot again");
        m.apply("wg:new:x", CtlVal::Num(5000.0));
        assert_eq!(m.widget_builder.as_ref().unwrap().x, 0, "out of range is refused");
    }

    #[test]
    fn the_form_lists_each_templates_controls_and_blocks_create_until_valid() {
        let (mut m, _) = model_with("[general]\n");
        let ids = |m: &Model| -> Vec<String> {
            items(m).iter().flat_map(|i| if let Item::Card(rows) = i { rows.iter().flat_map(|r| r.ctls.iter().map(|c| c.id.clone())).collect() } else { vec![] }).collect()
        };
        assert_eq!(ids(&m), ["wg:new:open"]);
        m.apply("wg:new:open", CtlVal::Click);
        assert!(ids(&m).contains(&"wg:new:h24".to_string()));
        m.apply("wg:new:template", CtlVal::Str("system-metrics".into()));
        assert_eq!(m.widget_builder.as_ref().unwrap().name, "System monitor", "the default name follows the type");
        m.apply("wg:new:name", CtlVal::Str("Server".into()));
        m.apply("wg:new:template", CtlVal::Str("note".into()));
        assert_eq!(m.widget_builder.as_ref().unwrap().name, "Server", "a typed name is kept");
        assert!(ids(&m).contains(&"wg:new:text".to_string()));
        let create =
            |m: &Model| items(m).iter().find_map(|i| if let Item::Card(rows) = i { rows.iter().flat_map(|r| &r.ctls).find(|c| c.id == "wg:new:create").map(|c| c.enabled) } else { None }).unwrap();
        assert!(!create(&m));
        m.apply("wg:new:text", CtlVal::Str("hello".into()));
        assert!(create(&m));
        assert!(items(&m).iter().any(|i| matches!(i, Item::WidgetPreview { .. })));
        m.apply("wg:new:template", CtlVal::Str("shell".into()));
        assert!(m.notice.as_ref().unwrap().error, "unknown types are refused");
        m.apply("wg:new:cancel", CtlVal::Click);
        assert!(m.widget_builder.is_none());
    }

    #[test]
    fn previews_draw_with_the_desktop_painter_and_fit_their_templates() {
        use crate::widgets::layout::{Item as Node, Leaf};
        let mut fonts = Fonts::load().expect("render verification requires fonts");
        let th = crate::theme::th();
        fn texts(it: &Node, out: &mut Vec<(String, f32, f32)>) {
            if let Leaf::Text { s, size, bold, .. } = &it.leaf {
                out.push((s.clone(), *size, if *bold { 1.0 } else { 0.0 }));
                assert!(it.rect.w > 0.0);
            }
            for c in &it.children {
                texts(c, out);
            }
        }
        for d in all_drafts() {
            let src = d.preview_source().unwrap();
            let def = dsl::parse(&src, Trust::User).unwrap();
            let it = layout(&def, &Ctx { data: &data::sample(), metrics: &paint::FontMetrics(&fonts), theme_radius: th.radius }, None);
            // every line fits its card: the card width is what the template promised
            let mut t = Vec::new();
            texts(&it, &mut t);
            for child in &it.children {
                assert!(child.rect.x >= -0.01 && child.rect.x + child.rect.w <= it.rect.w + 0.01, "{:?} {:?}: {:?} in {:?}", d.template, d.size, child.rect, it.rect);
            }
            let px = paint::render(&def, &data::sample(), &th, &mut fonts, Path::new("."), 2).unwrap();
            assert!(px.data().chunks_exact(4).any(|p| p[3] > 0));
            // Set WIDGET_EVIDENCE_DIR to keep the renders for visual review.
            if let Some(dir) = std::env::var_os("WIDGET_EVIDENCE_DIR").filter(|_| d.h24 && d.mem) {
                std::fs::create_dir_all(&dir).unwrap();
                px.save_png(Path::new(&dir).join(format!("created-{}-{:?}.png", d.template.key(), d.size).to_lowercase())).unwrap();
            }
        }
        // the widest text the form accepts still stays inside the card at every size
        {
            use crate::widgets::layout::Metrics;
            let metrics = paint::FontMetrics(&fonts);
            let worst = [
                "W".repeat(40),
                "WWWWWWWW MMMMMMMM @@@@@@@@ ".repeat(8),
                "ÅÅÅÅ ÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖÖ 日本語日本語日本語日本語日本語日本語日本語日本語日本語日本語 mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm".repeat(2),
            ];
            for text in worst {
                for template in [Template::Note, Template::Metrics] {
                    for (size, _, _) in Size::ALL {
                        let d = Draft { template, size, name: clean(&text, NAME_MAX), text: clean(&text, TEXT_MAX), ..Draft::new() };
                        let src = d.preview_source().unwrap();
                        let def = dsl::parse(&src, Trust::User).unwrap();
                        let it = layout(&def, &Ctx { data: &data::sample(), metrics: &metrics, theme_radius: th.radius }, None);
                        fn check(n: &Node, metrics: &dyn Metrics, limit: f32, what: &str) {
                            if let Leaf::Text { s, size, bold, .. } = &n.leaf {
                                assert!(metrics.text_width(s, *size, *bold) <= limit + 0.5, "{what}: {s:?} is wider than {limit}");
                            }
                            for c in &n.children {
                                check(c, metrics, limit, what);
                            }
                        }
                        check(&it, &metrics, it.rect.w - 32.0 * size.scale(), &format!("{template:?} {size:?}"));
                    }
                }
            }
        }
        // through the settings painter: the preview item paints inside its card and reserves space
        let mut pm = Pixmap::new(700, 400).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));
        let mut p = Painter { fonts: Some(&mut fonts), s: 1.0 };
        let toml = Draft::new().preview_source().unwrap();
        let below = draw_preview(&mut pm, &mut p, &toml, "caption", 20.0, 10.0, 600.0);
        assert!(below > 100.0 && below < 400.0, "{below}");
        let changed = pm.data().chunks_exact(4).filter(|px| px[..3] != [30, 30, 30]).count();
        assert!(changed > 2000, "card and widget were painted ({changed} pixels)");
        let again = draw_preview(&mut pm, &mut p, &toml, "caption", 20.0, 10.0, 600.0);
        assert_eq!(below, again, "cached render keeps the same size");
        let mut no_fonts = Painter { fonts: None, s: 1.0 };
        assert!(draw_preview(&mut pm, &mut no_fonts, &toml, "caption", 20.0, 10.0, 600.0) > 10.0);
    }
}
