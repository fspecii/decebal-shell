//! Applying a theme package to the shell: one transaction over every file the theme owns, with undo.
//!
//! [`plan`] computes the new text of each file without touching anything and checks it (rdwm's config goes through
//! `rdwmctl check-config`). [`execute`] saves the old files as an undo record, writes them atomically one by one and puts everything
//! back if any write fails. [`undo`] restores the latest record; [`preview`] is apply, wait, undo.
//!
//! Files: rdwm's `config.toml` (colours, titlebars, gaps, border colours, layouts), `dock-settings.json` (dock style and
//! alignment) and `decebal.json` (theme selection, `bar`, `dock`, `hot_corners`, `widgets`, `icons`).
//! A key the running rdwm does not know yet is skipped and reported in [`Applied::notes`], not an error.

use super::schema::Status;
use crate::settings::io::{self, Paths};
use crate::settings::schema::Schema;
use crate::settings::tomlcfg::{Cfg, Val};
use crate::theme::live::{system_dark, Source};
use crate::theme::model::Selection;
use crate::theme::rdwm;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// One file's change.
#[derive(Debug, Clone)]
pub struct Write {
    pub path: PathBuf,
    pub old: Option<String>,
    pub new: String,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub id: String,
    pub dark: bool,
    pub selection: Selection,
    pub writes: Vec<Write>,
    pub notes: Vec<String>,
    pub rdwm_changed: bool,
    /// Opt-in curated background request, suppressed for previews.
    pub follow_wallpaper: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub theme_id: String,
    pub dark: bool,
    /// Names of the files that changed.
    pub files: Vec<String>,
    /// Keys that were left out and why.
    pub notes: Vec<String>,
    pub reloaded: bool,
    /// The undo record this created (empty when nothing changed).
    pub undo_id: String,
}

/// Where undo records live: next to `decebal.json` (so a test or a private HOME never touches the real one).
pub fn undo_dir(paths: &Paths) -> PathBuf {
    paths.shell_dir.join("theme-undo")
}

// ---------------------------------------------------------------- JSON helpers

fn text_of(v: &Value) -> String {
    let mut t = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into());
    t.push('\n');
    t
}

fn obj_mut<'a>(doc: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let o = doc.as_object_mut().expect("json documents are objects");
    let slot = o.entry(key.to_string()).or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    slot.as_object_mut().expect("just made an object")
}

fn str_of<'a>(extra: &'a Map<String, Value>, sec: &str, key: &str) -> Option<&'a str> {
    extra.get(sec)?.get(key)?.as_str()
}

// ---------------------------------------------------------------- rdwm edits

/// The extras → rdwm key table: (extras section, key, rdwm section, rdwm key, how to convert).
#[derive(Clone, Copy)]
enum Conv {
    Str,
    Int,
    Float,
    Bool,
    List,
    /// `buttons`: "none" turns the buttons off, "left" / "right" keep them on.
    ButtonsOn,
}

const RDWM_MAP: &[(&str, &str, &str, &str, Conv)] = &[
    ("decoration", "style", "decoration", "style", Conv::Str),
    ("decoration", "height", "decoration", "height", Conv::Int),
    ("decoration", "radius", "decoration", "radius", Conv::Int),
    ("decoration", "title_align", "decoration", "title_align", Conv::Str),
    ("decoration", "buttons", "decoration", "buttons", Conv::ButtonsOn),
    ("rdwm", "gap_inner", "general", "gap_inner", Conv::Int),
    ("rdwm", "gap_outer", "general", "gap_outer", Conv::Int),
    ("rdwm", "border_px", "general", "border_px", Conv::Int),
    ("rdwm", "smart_gaps", "general", "smart_gaps", Conv::Bool),
    ("rdwm", "mfact", "general", "mfact", Conv::Float),
    ("rdwm", "nmaster", "general", "nmaster", Conv::Int),
    ("rdwm", "layouts", "general", "layouts", Conv::List),
    ("rdwm", "focused_border", "colors", "focused", Conv::Str),
    ("rdwm", "normal_border", "colors", "normal", Conv::Str),
    ("rdwm", "urgent_border", "colors", "urgent", Conv::Str),
];

/// rdwm key names the side / order of window buttons may have once the decoration-styles branch lands.
const BUTTON_SIDE_KEYS: &[&str] = &["button_side", "buttons_side", "button_position", "buttons_position", "button_align"];
const BUTTON_ORDER_KEYS: &[&str] = &["button_order", "buttons_order"];

fn to_val(v: &Value, c: Conv) -> Option<Val> {
    Some(match c {
        Conv::Str => Val::Str(v.as_str()?.to_string()),
        Conv::Int => Val::Int(v.as_f64()?.round() as i64),
        Conv::Float => Val::Float(v.as_f64()?),
        Conv::Bool => Val::Bool(v.as_bool()?),
        Conv::List => Val::List(v.as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect()),
        Conv::ButtonsOn => Val::Bool(v.as_str()? != "none"),
    })
}

/// Edits rdwm's config in memory from the theme's sections. Unknown keys / values rdwm would reject go to `notes`.
fn rdwm_extras(cfg: &mut Cfg, extra: &Map<String, Value>, schema: &Schema, notes: &mut Vec<String>) -> Result<(), String> {
    let set = |cfg: &mut Cfg, sec: &str, key: &str, v: Val, label: &str, notes: &mut Vec<String>| -> Result<(), String> {
        match schema.get(sec, key) {
            None => notes.push(format!("{label}: this rdwm has no `[{sec}] {key}` yet, left out")),
            Some(m) => {
                // a bundled snapshot of the key table can be older than the installed rdwm: for the titlebar style only the
                // real `rdwmctl check-config` decides (a rejected style is dropped below)
                let stale_style = !schema.live && sec == "decoration" && key == "style";
                if let (Val::Str(s), false, false) = (&v, m.choices.is_empty(), stale_style) {
                    if !m.choices.contains(s) {
                        notes.push(format!("{label}: this rdwm does not know `{s}` (it has {}), left out", m.choices.join(", ")));
                        return Ok(());
                    }
                }
                cfg.set(sec, key, &v)?;
            }
        }
        Ok(())
    };
    for (esec, ekey, sec, key, conv) in RDWM_MAP {
        let Some(v) = extra.get(*esec).and_then(|s| s.get(*ekey)) else { continue };
        let Some(val) = to_val(v, *conv) else { continue };
        set(cfg, sec, key, val, &format!("{esec}.{ekey}"), notes)?;
    }
    // window button side / order: the key names come with the rdwm decoration-styles work
    if let Some(side) = str_of(extra, "decoration", "buttons").filter(|s| *s != "none") {
        match BUTTON_SIDE_KEYS.iter().find(|k| schema.has("decoration", k)) {
            Some(k) => set(cfg, "decoration", k, Val::Str(side.into()), "decoration.buttons", notes)?,
            None => notes.push("decoration.buttons: this rdwm has no button side setting yet, left out".into()),
        }
    }
    if let Some(order) = extra.get("decoration").and_then(|d| d.get("button_order")).and_then(Value::as_array) {
        match BUTTON_ORDER_KEYS.iter().find(|k| schema.has("decoration", k)) {
            Some(k) => set(cfg, "decoration", k, Val::List(order.iter().filter_map(|x| x.as_str().map(String::from)).collect()), "decoration.button_order", notes)?,
            None => notes.push("decoration.button_order: this rdwm has no button order setting yet, left out".into()),
        }
    }
    Ok(())
}

/// rdwm accepts the file when `check-config` does; an rdwmctl too old to know the command counts as "syntax only".
fn check_rdwm(paths: &Paths, text: &str) -> Result<(), String> {
    match io::validate(&paths.rdwmctl, &paths.rdwm_config, text) {
        Err(e) if e.contains("usage: rdwmctl") => Ok(()),
        r => r,
    }
}

// ---------------------------------------------------------------- where a widget file lives

/// The package (this theme or one it extends) that has `widgets/<name>.toml`.
fn widget_source(src: &Source, id: &str, name: &str) -> Option<String> {
    let mut cur = Some(id.to_string());
    let mut depth = 0;
    while let Some(c) = cur.take() {
        if depth > 8 {
            break;
        }
        depth += 1;
        if crate::widgets::config::package_dir(&src.themes_dir, &c).is_some_and(|d| d.join("widgets").join(format!("{name}.toml")).is_file()) {
            return Some(c);
        }
        cur = crate::widgets::config::package_dir(&src.themes_dir, &c)
            .and_then(|d| std::fs::read_to_string(d.join("theme.toml")).ok())
            .and_then(|t| crate::theme::model::Def::parse(&t).ok())
            .and_then(|d| d.inherit);
    }
    None
}

// ---------------------------------------------------------------- plan

/// How to handle a custom desktop layout when a theme supplies widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetChoice {
    Ask,
    Keep,
    Replace,
}

fn widget_layout(w: &Value) -> Value {
    json!({"enabled":w.get("enabled").and_then(Value::as_bool).unwrap_or(true),"items":w.get("items").cloned().unwrap_or_else(||json!([]))})
}

pub fn custom_widgets(w: &Value) -> bool {
    w.get("items").and_then(Value::as_array).is_some_and(|items| !items.is_empty()) && w.get("theme_baseline") != Some(&widget_layout(w))
}

pub fn widgets_need_confirmation(src: &Source, paths: &Paths, name: &str) -> Result<bool, String> {
    let (id, _) = src.find(name).ok_or("Unknown theme")?;
    let def = src.load_def(&id)?;
    Ok(def.extra.get("widgets").and_then(Value::as_array).is_some_and(|items| items.iter().any(|w| w.get("enabled").and_then(Value::as_bool) != Some(false)))
        && custom_widgets(&io::json_load(&paths.shell_json())["widgets"]))
}

pub fn plan_with_widgets(src: &Source, paths: &Paths, name: &str, base: &Selection, choice: WidgetChoice) -> Result<Plan, String> {
    plan_inner(src, paths, name, base, true, choice)
}

pub fn plan_selection_with_widgets(src: &Source, paths: &Paths, sel: &Selection, choice: WidgetChoice) -> Result<Plan, String> {
    plan_inner(src, paths, &sel.name, sel, false, choice)
}

/// Computes everything `apply` would write, touching nothing.
pub fn plan(src: &Source, paths: &Paths, name: &str, base: &Selection) -> Result<Plan, String> {
    plan_inner(src, paths, name, base, true, WidgetChoice::Ask)
}

/// Changes mode/palette/customisation without discarding the selection's colours or package structure.
pub fn plan_selection(src: &Source, paths: &Paths, selection: &Selection) -> Result<Plan, String> {
    plan_inner(src, paths, &selection.name, selection, false, WidgetChoice::Ask)
}

fn plan_inner(src: &Source, paths: &Paths, name: &str, base: &Selection, reset_colors: bool, widget_choice: WidgetChoice) -> Result<Plan, String> {
    let (id, implied) = src.find(name).ok_or_else(|| format!("no theme `{name}` (decthemectl theme list)"))?;
    if let Some(e) = src.list().into_iter().find(|e| e.id == id).and_then(|e| e.error) {
        return Err(format!("theme `{id}` is broken: {e}"));
    }
    let mut notes: Vec<String> = Vec::new();
    let def = src.load_def(&id)?;
    let extra = def.extra.clone();

    let mut sel = base.clone();
    sel.name = id.clone();
    // Applying a complete design restores its own colour scheme. Palettes remain
    // independently selectable afterwards.
    if reset_colors {
        sel.palette = None;
        sel.accent = None;
        sel.dynamic = None;
    }
    if let Some(m) = implied {
        sel.mode = m;
    }

    let previous = src.selection();
    let same_theme = src.find(&previous.name).is_some_and(|(old, _)| old == id);
    // Wallpaper is an independent user choice. Package artwork and colour-matching
    // directives are available to previews, but never change the applied background.

    let (theme, problem) = src.build(&sel, system_dark());
    if let Some(p) = problem {
        return Err(p);
    }

    let mut writes: Vec<Write> = Vec::new();
    fn add(writes: &mut Vec<Write>, path: &Path, new: String) {
        let old = std::fs::read_to_string(path).ok();
        if old.as_deref() != Some(new.as_str()) {
            writes.push(Write { path: path.to_path_buf(), old, new });
        }
    }

    // ---- decebal.json
    let mut shell = io::json_load(&paths.shell_json());
    shell.as_object_mut().expect("object").insert("theme".into(), sel.to_json());
    if let Some(Value::Object(bar)) = extra.get("bar") {
        let o = obj_mut(&mut shell, "bar");
        for k in ["enabled", "position", "left", "center", "right", "margin", "spacing", "radius", "background", "band_radius"] {
            if let Some(v) = bar.get(k) {
                o.insert(k.into(), v.clone());
            }
        }
    }
    if let Some(Value::Object(dock)) = extra.get("dock") {
        let o = obj_mut(&mut shell, "dock");
        for k in ["enabled", "position"] {
            if let Some(v) = dock.get(k) {
                o.insert(k.into(), v.clone());
            }
        }
    }
    if let Some(Value::Object(hc)) = extra.get("hot_corners") {
        let o = obj_mut(&mut shell, "hot_corners");
        for (k, v) in hc {
            o.insert(k.clone(), v.clone());
        }
    }
    if let Some(Value::Object(cc)) = extra.get("control_center") {
        obj_mut(&mut shell, "control_center").extend(cc.clone());
    }
    if let Some(Value::Object(ic)) = extra.get("icons") {
        obj_mut(&mut shell, "icons").extend(ic.clone());
    }
    if reset_colors || !same_theme || shell.get("widgets").is_none() {
        let previous_widgets = shell.get("widgets").cloned().unwrap_or_else(|| json!({}));
        let custom = custom_widgets(&previous_widgets);
        let w = obj_mut(&mut shell, "widgets");
        let mut items: Vec<Value> = Vec::new();
        for inst in extra.get("widgets").and_then(Value::as_array).into_iter().flatten() {
            let mut inst = inst.clone();
            let Some(o) = inst.as_object_mut() else { continue };
            let use_ = o.get("use").and_then(Value::as_str).unwrap_or("").to_string();
            if !use_.starts_with("builtin:") {
                match widget_source(src, &id, &use_) {
                    Some(s) => {
                        o.insert("source".into(), json!(s));
                    }
                    None if crate::widgets::samples::source(&use_).is_some() => {
                        o.insert("use".into(), json!(format!("builtin:{use_}")));
                    }
                    None => {
                        notes.push(format!("widget `{use_}` is not in the package or its parents, left out"));
                        continue;
                    }
                }
            }
            o.insert("from_theme".into(), json!(true));
            if o.get("place").and_then(Value::as_str) == Some("bar") {
                notes.push(format!("widget `{use_}`: bar placement is not available yet; use desktop placement in Widgets"));
            }
            items.push(inst);
        }
        if !items.iter().any(|w| w.get("enabled").and_then(Value::as_bool) != Some(false)) {
            if custom {
                let enabled = previous_widgets.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                w.entry("custom_enabled_before_theme").or_insert(json!(enabled));
                w.shift_remove("theme_baseline");
            }
            w.insert("enabled".into(), json!(false));
            w.insert("theme_widgets_absent".into(), json!(true));
            if !custom {
                w.insert("theme_baseline".into(), widget_layout(&Value::Object(w.clone())));
            }
        } else if custom && widget_choice == WidgetChoice::Ask {
            return Err("This theme supplies widgets and you have a custom layout. Choose --keep-widgets or --replace-widgets, or apply it in Settings to review the choice.".into());
        } else if custom && widget_choice == WidgetChoice::Keep {
            if let Some(enabled) = w.shift_remove("custom_enabled_before_theme") {
                w.insert("enabled".into(), enabled);
            }
            w.shift_remove("theme_baseline");
            w.shift_remove("theme_widgets_absent");
        } else {
            w.insert("items".into(), Value::Array(items));
            w.insert("enabled".into(), json!(true));
            w.shift_remove("custom_enabled_before_theme");
            w.shift_remove("theme_widgets_absent");
            w.insert("theme_baseline".into(), widget_layout(&Value::Object(w.clone())));
        }
        w.insert("theme_source".into(), json!(id));
    }
    add(&mut writes, &paths.shell_json(), text_of(&shell));

    // ---- dock-settings.json
    if let Some(Value::Object(dock)) = extra.get("dock") {
        let mut d = io::json_load(&paths.dock_settings());
        let o = d.as_object_mut().expect("object");
        if let Some(s) = dock.get("style") {
            o.insert("style".into(), s.clone());
        }
        if let Some(a) = dock.get("align") {
            o.insert("windowsAlign".into(), a.clone());
        }
        match dock.get("position").and_then(Value::as_str) {
            Some(p @ ("top" | "bottom")) => {
                o.insert("windowsPosition".into(), json!(p));
            }
            Some(p) => notes.push(format!("dock.position = {p}: only top and bottom are supported by the dock today, left out")),
            None => {}
        }
        if let Some(m) = dock.get("magnification") {
            o.insert("magnification".into(), m.clone());
        }
        // the theme's own roundness rules unless this theme sets one
        match dock.get("roundness") {
            Some(r) => {
                o.insert("roundness".into(), r.clone());
            }
            None if dock.contains_key("style") => {
                o.shift_remove("roundness");
            }
            None => {}
        }
        add(&mut writes, &paths.dock_settings(), text_of(&d));
    }

    // ---- rdwm config.toml
    let mut rdwm_changed = false;
    if sel.sync_rdwm {
        let old = std::fs::read_to_string(&paths.rdwm_config).unwrap_or_default();
        let mut cfg = Cfg::parse(&old).map_err(|e| format!("{}: {e}", paths.rdwm_config.display()))?;
        rdwm::apply_to(&mut cfg, &theme)?;
        let schema = Schema::load(&paths.rdwmctl);
        if !rdwm::apply_shadows(&mut cfg, &theme, paths, &schema)? && theme.shadow > 0.0 {
            notes.push("Window shadows require a supporting rdwm session; current configuration retained".into());
        }
        let mut attempt = extra.clone();
        if let Some(radius) = sel.radius {
            if let Some(Value::Object(d)) = attempt.get_mut("decoration") {
                d.insert("radius".into(), json!(radius));
            }
        }
        if sel.palette.is_some() || sel.accent.is_some() {
            // a palette colours the borders too: the theme's hard-coded border colours would fight it
            if let Some(Value::Object(r)) = attempt.get_mut("rdwm") {
                for k in ["focused_border", "normal_border", "urgent_border"] {
                    r.shift_remove(k);
                }
            }
        }
        let mut text;
        loop {
            let mut c = cfg.clone();
            rdwm_extras(&mut c, &attempt, &schema, &mut notes)?;
            if let Some(radius) = rdwm::clamp_decoration_radius(&mut c)? {
                let note = format!("Window corner radius limited to the {radius}px titlebar; your saved radius preference is unchanged");
                if !notes.contains(&note) {
                    notes.push(note);
                }
            }
            text = c.text();
            if text == old {
                break;
            }
            match check_rdwm(paths, &text) {
                Ok(()) => break,
                Err(e) => {
                    // an rdwm older than the theme may not know its titlebar style: keep the rest, say so
                    let style = attempt.get("decoration").and_then(|d| d.get("style")).and_then(Value::as_str).map(String::from);
                    match style {
                        Some(st) => {
                            notes.push(format!("decoration.style `{st}`: rdwm rejected it ({e}), left out"));
                            if let Some(Value::Object(d)) = attempt.get_mut("decoration") {
                                d.shift_remove("style");
                            }
                        }
                        None => return Err(e),
                    }
                }
            }
        }
        if text != old {
            rdwm_changed = true;
            writes.push(Write { path: paths.rdwm_config.clone(), old: paths.rdwm_config.exists().then_some(old), new: text });
        }
    }

    // pending keys the theme sets, for the report
    for f in super::schema::FIELDS {
        if let Status::Pending(why) = f.status {
            let (sec, key) = f.path.split_once('.').unwrap_or(("", f.path));
            if extra.get(sec).is_some_and(|s| s.get(key).is_some()) && !notes.iter().any(|n| n.starts_with(&format!("{sec}.{key}"))) && !matches!(f.path, "integrity.signature" | "integrity.signer") {
                notes.push(format!("{}: not applied yet ({why})", f.path));
            }
        }
    }
    Ok(Plan { id, dark: theme.dark, selection: sel, writes, notes, rdwm_changed, follow_wallpaper: reset_colors || !same_theme || src.build(&previous, system_dark()).0.dark != theme.dark })
}

// ---------------------------------------------------------------- execute and undo

fn stamp_id() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{:013}-{:06}", d.as_millis(), std::process::id() % 1_000_000)
}

/// Writes the plan: undo record first, then each file atomically; any failure restores what was already written.
pub fn execute(_src: &Source, paths: &Paths, plan: Plan) -> Result<Applied, String> {
    // An older photo must not commit while this transaction changes theme files.
    if plan.follow_wallpaper && plan.selection.sync_rdwm && crate::theme::wallpaper_follow::enabled(paths) {
        crate::theme::wallpaper_follow::cancel(paths)?;
    }
    let mut applied = Applied { theme_id: plan.id.clone(), dark: plan.dark, files: Vec::new(), notes: plan.notes.clone(), reloaded: false, undo_id: String::new() };
    if plan.writes.is_empty() {
        if let Err(e) = crate::dock_lifecycle::reconcile(&paths.shell_json()) {
            crate::log_warn!("dock: {e}");
        }
        crate::theme::system::request(&paths.shell_json());
        follow_wallpaper(paths, &plan, &mut applied);
        return Ok(applied);
    }
    let uid = stamp_id();
    let rec = undo_dir(paths).join(&uid);
    std::fs::create_dir_all(&rec).map_err(|e| format!("{}: {e}", rec.display()))?;
    let mut files = Vec::new();
    for (i, w) in plan.writes.iter().enumerate() {
        if let Some(old) = &w.old {
            std::fs::write(rec.join(format!("{i}.bak")), old).map_err(|e| format!("undo record: {e}"))?;
        }
        files.push(json!({"path": w.path.display().to_string(), "existed": w.old.is_some(), "bak": format!("{i}.bak")}));
    }
    std::fs::write(rec.join("meta.json"), text_of(&json!({"theme": plan.id, "files": files}))).map_err(|e| format!("undo record: {e}"))?;
    let mut done: Vec<&Write> = Vec::new();
    for w in &plan.writes {
        if let Err(e) = io::write_atomic(&w.path, &w.new) {
            for d in done.iter().rev() {
                let _ = restore(d);
            }
            let _ = std::fs::remove_dir_all(&rec);
            return Err(format!("{e} (nothing was changed)"));
        }
        done.push(w);
        applied.files.push(w.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }
    applied.undo_id = uid;
    prune(&undo_dir(paths), 10);
    if plan.rdwm_changed {
        applied.reloaded = rdwm::reload(paths).unwrap_or(false);
    }
    crate::theme::live::reload();
    if let Err(e) = crate::dock_lifecycle::reconcile(&paths.shell_json()) {
        crate::log_warn!("dock: {e}");
    }
    crate::theme::system::request(&paths.shell_json()); // "Apply to all apps", when the user switched it on
    #[cfg(feature = "x11")]
    {
        let cfg = crate::widgets::config::WidgetsConfig::load(&paths.shell_json());
        if cfg.enabled && cfg.items.iter().any(|i| i.enabled && i.place == crate::widgets::config::Place::Desktop) {
            if let Err(e) = crate::widgets::host::ensure_running() {
                crate::log_warn!("widgets: {e}");
            }
        }
    }
    follow_wallpaper(paths, &plan, &mut applied);
    Ok(applied)
}

fn follow_wallpaper(paths: &Paths, plan: &Plan, applied: &mut Applied) {
    if plan.follow_wallpaper && plan.selection.sync_rdwm {
        if let Err(error) = crate::theme::wallpaper_follow::request(paths, &plan.id, plan.dark) {
            applied.notes.push(error);
        }
    }
}

fn restore(w: &Write) -> Result<(), String> {
    match &w.old {
        Some(old) => io::write_atomic(&w.path, old),
        None => std::fs::remove_file(&w.path).map_err(|e| e.to_string()),
    }
}

fn prune(dir: &Path, keep: usize) {
    let mut names: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    names.sort();
    while names.len() > keep {
        let _ = std::fs::remove_dir_all(names.remove(0));
    }
}

/// Plan and execute.
pub fn apply(src: &Source, paths: &Paths, name: &str, base: &Selection) -> Result<Applied, String> {
    crate::log_info!("theme apply `{name}`");
    let plan = plan(src, paths, name, base).inspect_err(|e| crate::log_warn!("theme `{name}` not applied: {e}"))?;
    for n in &plan.notes {
        crate::log_info!("theme `{}` note: {n}", plan.id);
    }
    crate::log_debug!("theme `{}` ({}): {} file(s) to write, rdwm reload {}", plan.id, if plan.dark { "dark" } else { "light" }, plan.writes.len(), plan.rdwm_changed);
    let r = execute(src, paths, plan);
    match &r {
        Ok(a) => crate::log_info!("theme `{}` applied: files {:?}, rdwm reloaded {}, undo `{}`", a.theme_id, a.files, a.reloaded, a.undo_id),
        Err(e) => crate::log_error!("theme apply failed: {e}"),
    }
    r
}

/// Apply a selection as the same atomic package transaction, preserving explicit colours.
pub fn apply_selection(src: &Source, paths: &Paths, selection: &Selection) -> Result<Applied, String> {
    let plan = plan_selection(src, paths, selection)?;
    execute(src, paths, plan)
}

/// The undo records, oldest first.
pub fn undo_ids(paths: &Paths) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(undo_dir(paths)).into_iter().flatten().flatten().filter(|e| e.path().join("meta.json").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

// Theme undo restores theme-owned state while retaining the independent wallpaper.
// Other settings retain the existing whole-file undo behavior.
fn restore_theme(w: &Write, paths: &Paths) -> Result<(), String> {
    let live = std::fs::read_to_string(&w.path).unwrap_or_default();
    if w.path == paths.rdwm_config {
        let mut old = w.old.as_deref().unwrap_or_default().parse::<toml_edit::DocumentMut>().map_err(|e| e.to_string())?;
        let current = live.parse::<toml_edit::DocumentMut>().map_err(|e| e.to_string())?;
        let wallpaper = current.get("wallpaper").cloned();
        if old.get("wallpaper").map(ToString::to_string) != wallpaper.as_ref().map(ToString::to_string) {
            old.remove("wallpaper");
            if let Some(wallpaper) = wallpaper {
                old.insert("wallpaper", wallpaper);
            }
            if w.old.is_some() || !old.is_empty() {
                return io::write_atomic(&w.path, &old.to_string());
            }
        }
    } else if w.path == paths.shell_json() {
        let mut old: Value = serde_json::from_str(w.old.as_deref().unwrap_or("{}")).map_err(|e| e.to_string())?;
        let current: Value = serde_json::from_str(if live.is_empty() { "{}" } else { &live }).map_err(|e| e.to_string())?;
        let mut changed = false;
        let object = old.as_object_mut().ok_or("undo shell settings must be an object")?;
        for key in ["theme_wallpaper", "wallpaper"] {
            if object.get(key) != current.get(key) {
                changed = true;
                object.shift_remove(key);
                if let Some(value) = current.get(key) {
                    object.insert(key.into(), value.clone());
                }
            }
        }
        if changed && (w.old.is_some() || !object.is_empty()) {
            return io::write_atomic(&w.path, &text_of(&old));
        }
    }
    restore(w)
}

/// Restores the files of the latest apply (or of `only`, when it is still the latest) and removes the record.
pub fn undo(_src: &Source, paths: &Paths, only: Option<&str>) -> Result<String, String> {
    crate::log_info!("theme undo");
    let ids = undo_ids(paths);
    let Some(last) = ids.last() else { return Err("nothing to undo".into()) };
    if only.is_some_and(|o| o != last) {
        return Err("the theme was changed again since; not undoing".into());
    }
    let rec = undo_dir(paths).join(last);
    let meta: Value = serde_json::from_str(&std::fs::read_to_string(rec.join("meta.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut rdwm_touched = false;
    for f in meta["files"].as_array().into_iter().flatten() {
        let path = PathBuf::from(f["path"].as_str().unwrap_or_default());
        if path == paths.rdwm_config {
            rdwm_touched = true;
        }
        let w = Write { path, old: f["existed"].as_bool().unwrap_or(false).then(|| std::fs::read_to_string(rec.join(f["bak"].as_str().unwrap_or_default())).unwrap_or_default()), new: String::new() };
        restore_theme(&w, paths)?;
    }
    let theme = meta["theme"].as_str().unwrap_or("").to_string();
    let _ = std::fs::remove_dir_all(&rec);
    if rdwm_touched {
        let _ = rdwm::reload(paths);
    }
    crate::theme::live::reload();
    if let Err(e) = crate::dock_lifecycle::reconcile(&paths.shell_json()) {
        crate::log_warn!("dock: {e}");
    }
    crate::theme::system::request(&paths.shell_json());
    Ok(theme)
}

/// Applies a theme now and puts everything back after `secs` (or on `cancel`). Blocks; run it on a thread.
pub fn preview(src: &Source, paths: &Paths, name: &str, secs: u64, cancel: &std::sync::atomic::AtomicBool) -> Result<Applied, String> {
    let mut plan = plan_with_widgets(src, paths, name, &src.selection(), WidgetChoice::Keep)?;
    plan.follow_wallpaper = false;
    let a = execute(src, paths, plan)?;
    if a.undo_id.is_empty() {
        return Ok(a);
    }
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end && !cancel.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = undo(src, paths, Some(&a.undo_id));
    Ok(a)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::theme::engine::archive::{Entry, Limits};
    use crate::theme::engine::package::{install_entries, tests::png};

    pub fn sandbox(name: &str) -> (PathBuf, Source, Paths) {
        let d = std::env::temp_dir().join(format!("decebal-apply-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        std::env::set_var("DECEBAL_SYSTEM_THEMES", d.join("nosystem"));
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: d.join("cfg/themes") };
        (d, src, paths)
    }

    #[test]
    fn applying_a_theme_restores_its_colours_and_updates_bar_dock_and_windows() {
        let (dir, src, paths) = sandbox("palette-design-switch");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        let mut base = crate::theme::palette::select(&Selection::default(), Some("nord"));
        std::fs::write(paths.dock_settings(), r#"{"magnification":1.0}"#).unwrap();
        for id in ["windows11", "caelestia", "macos"] {
            apply(&src, &paths, id, &base).unwrap();
            base = src.selection();
            assert_eq!(base.name, id);
            assert_eq!(base.palette, None);
            let theme = src.build(&base, true).0;
            assert_eq!(theme.background, crate::theme::builtin::def(id).unwrap().resolve(id, true, &Selection::default()).background);
            assert_eq!(theme.titlebar_style, crate::theme::builtin::def(id).unwrap().resolve(id, true, &Selection::default()).titlebar_style);
            let shell = io::json_load(&paths.shell_json());
            let dock = io::json_load(&paths.dock_settings());
            assert_eq!(dock["style"], if id == "windows11" { "windows" } else { "mac" });
            if id == "macos" {
                assert_eq!(dock["magnification"], 1.6, "macOS must restore hover growth after a flat dock theme");
            }
            let shell: crate::config::ShellSettings = serde_json::from_value(shell).unwrap();
            let dock_style = crate::theme::style::dock_style(&dock);
            assert_eq!(shell.bar_visible(dock_style), id != "windows11");
            assert_eq!(shell.dock_visible(theme.bar_vertical()), id != "caelestia");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nextkde_restores_hover_after_flat_themes_and_reapply() {
        let (dir, src, paths) = sandbox("nextkde-hover-switch");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        std::fs::write(paths.dock_settings(), r#"{"magnification":1.0,"appSpacing":9,"custom":"keep"}"#).unwrap();
        for id in ["nextkde", "tidal-loom", "nextkde", "nextkde"] {
            apply(&src, &paths, id, &src.selection()).unwrap();
            let value = io::json_load(&paths.dock_settings());
            assert_eq!(value["magnification"], if id == "nextkde" { 1.4 } else { 1.0 });
            assert_eq!(value["appSpacing"], 9);
            assert_eq!(value["custom"], "keep");
            if id == "nextkde" {
                let settings: crate::config::DockSettings = serde_json::from_value(value).unwrap();
                assert_eq!(settings.style, crate::config::DockStyle::Mac);
                let mut dock = crate::dock::Dock::with_preferences(
                    vec![crate::dock::DockItem::pinned("launchpad", false)],
                    settings,
                    crate::accessibility::Preferences { reduce_motion: true, ..Default::default() },
                );
                dock.start_bounce("launchpad");
                assert!(!dock.bouncing(), "accessibility still suppresses animation");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn applying_a_dynamic_theme_resets_palette_without_enabling_wallpaper_colors() {
        let (dir, src, paths) = sandbox("palette-dynamic-priority");
        let base = crate::theme::palette::select(&Selection::default(), Some("nord"));
        let p = plan(&src, &paths, "material-you-dynamic", &Selection { sync_rdwm: false, ..base }).unwrap();
        assert_eq!(p.selection.palette, None);
        assert!(p.selection.dynamic.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    pub const SUNSET: &str = r##"format = 1
id = "sunset"
name = "Sunset"
version = "1.0.0"
author = "Ada"
license = "MIT"
[colors.dark]
background = "#201018"
text = "#ffeedd"
accent = "#ff7849"
[metrics]
radius = 14
[font]
ui = "Inter"
[wallpaper]
image = "wallpapers/sunset.png"
mode = "fit"
[decoration]
style = "classic"
height = 36
buttons = "right"
[rdwm]
gap_inner = 9
border_px = 3
layouts = ["tile", "monocle"]
[bar]
enabled = true
left = ["workspaces"]
[dock]
style = "windows"
align = "left"
position = "top"
[hot_corners]
top_left = "dashboard"
[icons]
theme = "Papirus"
[[widgets]]
use = "clock-card"
anchor = "top-right"
x = -40
y = 60
[[widgets]]
use = "builtin:cpu-ram-ring"
anchor = "bottom-left"
x = 30
y = -30
"##;

    pub fn install_sunset(src: &Source) {
        install_sunset_manifest(src, SUNSET);
    }

    fn install_sunset_manifest(src: &Source, manifest: &str) {
        let widget = "format = 1\n[root]\ntype = \"text\"\ntext = \"{clock:%H:%M}\"\n";
        let entries = vec![
            Entry { path: "theme.toml".into(), data: manifest.as_bytes().to_vec() },
            Entry { path: "wallpapers/sunset.png".into(), data: png(32, 18, [200, 80, 40]) },
            Entry { path: "widgets/clock-card.toml".into(), data: widget.as_bytes().to_vec() },
        ];
        install_entries(&entries, &src.themes_dir, false, &Limits::default()).unwrap();
    }

    #[test]
    fn selection_edits_keep_package_structure_palette_and_custom_wallpaper() {
        let (dir, src, paths) = sandbox("selection-structure");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        install_sunset(&src);
        apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        let widgets = io::json_load(&paths.shell_json())["widgets"].clone();
        let mut cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
        cfg.set("wallpaper", "image", &Val::Str("/my/wall.png".into())).unwrap();
        cfg.set("wallpaper", "mode", &Val::Str("center".into())).unwrap();
        std::fs::write(&paths.rdwm_config, cfg.text()).unwrap();
        let mut sel = crate::theme::palette::select(&src.selection(), Some("nord"));
        sel.accent = crate::theme::model::Rgb::parse("#aabbcc");
        sel.radius = Some(7.0);
        apply_selection(&src, &paths, &sel).unwrap();
        assert_eq!(src.selection().palette.as_deref(), Some("nord"));
        assert_eq!(src.selection().accent, sel.accent);
        let cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
        assert_eq!(cfg.get("decoration", "style"), Some(Val::Str("classic".into())));
        assert_eq!(cfg.get("decoration", "height"), Some(Val::Int(36)));
        assert_eq!(cfg.get("decoration", "radius"), Some(Val::Int(7)));
        assert_eq!(cfg.get("wallpaper", "image"), Some(Val::Str("/my/wall.png".into())));
        assert_eq!(cfg.get("wallpaper", "mode"), Some(Val::Str("center".into())));
        assert_eq!(io::json_load(&paths.dock_settings())["style"], "windows");
        assert_eq!(io::json_load(&paths.shell_json())["widgets"], widgets);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn theme_apply_and_undo_preserve_independent_background() {
        let (dir, src, paths) = sandbox("independent-background");
        install_sunset(&src);
        let package = src.themes_dir.join("sunset");
        let manifest = SUNSET.replace("[wallpaper]", "[wallpaper]\ndark = \"wallpapers/sunset.png\"\nlight = \"wallpapers/day.png\"\ncolor = \"#112233\"\nslideshow = true\nmatch_colors = true");
        std::fs::write(package.join("theme.toml"), manifest).unwrap();
        std::fs::write(package.join("wallpapers/day.png"), png(32, 18, [230, 230, 220])).unwrap();
        for image in ["/my/custom.png", ""] {
            let initial = format!("[general]\n[wallpaper]\nimage = {image:?}\nmode = \"center\"\ncolor = \"#abcdef\"\n");
            std::fs::write(&paths.rdwm_config, &initial).unwrap();
            let slideshow = json!({"images":["/my/first.png"], "interval":42});
            io::json_set(&paths.shell_json(), "theme_wallpaper", Some(slideshow.clone())).unwrap();
            let applied = apply(&src, &paths, "sunset", &Selection::default()).unwrap();
            let cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
            for key in ["image", "mode", "color"] {
                assert_eq!(cfg.get("wallpaper", key), Cfg::parse(&initial).unwrap().get("wallpaper", key));
            }
            assert_eq!(io::json_load(&paths.shell_json())["theme_wallpaper"], slideshow);
            // A later Wallpaper choice belongs to the user, even when undoing this theme.
            let mut changed = cfg;
            changed.set("wallpaper", "image", &Val::Str("/my/later.png".into())).unwrap();
            std::fs::write(&paths.rdwm_config, changed.text()).unwrap();
            let later = json!({"images":["/my/later.png"], "shuffle":true});
            io::json_set(&paths.shell_json(), "theme_wallpaper", Some(later.clone())).unwrap();
            undo(&src, &paths, Some(&applied.undo_id)).unwrap();
            let cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
            assert_eq!(cfg.get("wallpaper", "image"), Some(Val::Str("/my/later.png".into())));
            assert_eq!(io::json_load(&paths.shell_json())["theme_wallpaper"], later);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mode_variant_preserves_independent_wallpaper() {
        let (dir, src, paths) = sandbox("selection-mode-wallpaper");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        install_sunset(&src);
        let package = src.themes_dir.join("sunset");
        let text = SUNSET
            .replace("[metrics]", "[colors.light]\nbackground = \"#f0eeee\"\ntext = \"#111111\"\n[metrics]")
            .replace("[wallpaper]", "[wallpaper]\ndark = \"wallpapers/sunset.png\"\nlight = \"wallpapers/day.png\"");
        std::fs::write(package.join("theme.toml"), text).unwrap();
        std::fs::write(package.join("wallpapers/day.png"), png(32, 18, [230, 230, 220])).unwrap();
        apply(&src, &paths, "sunset", &Selection { mode: crate::theme::model::Mode::Dark, ..Selection::default() }).unwrap();
        let mut sel = src.selection();
        sel.mode = crate::theme::model::Mode::Light;
        apply_selection(&src, &paths, &sel).unwrap();
        let cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
        assert_eq!(cfg.get("wallpaper", "image"), None);
        let mut cfg = cfg;
        cfg.set("wallpaper", "image", &Val::Str("/my/custom.png".into())).unwrap();
        std::fs::write(&paths.rdwm_config, cfg.text()).unwrap();
        sel.mode = crate::theme::model::Mode::Dark;
        apply_selection(&src, &paths, &sel).unwrap();
        let cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
        assert_eq!(cfg.get("wallpaper", "image"), Some(Val::Str("/my/custom.png".into())));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn apply_copies_the_control_center_section_over_the_users_keys() {
        let (d, src, paths) = sandbox("cc");
        std::fs::write(paths.shell_json(), r#"{"control_center": {"enabled": false, "position": "top-left"}}"#).unwrap();
        let text = SUNSET.replace("[icons]", "[control_center]\nenabled = true\nstyle = \"fluent\"\ntiles = [\"sound\"]\n\n[icons]");
        assert_ne!(text, SUNSET, "the fixture has an [icons] section to put it before");
        let widget = "format = 1\n[root]\ntype = \"text\"\ntext = \"{clock:%H:%M}\"\n";
        let entries = vec![
            Entry { path: "theme.toml".into(), data: text.into_bytes() },
            Entry { path: "wallpapers/sunset.png".into(), data: png(32, 18, [200, 80, 40]) },
            Entry { path: "widgets/clock-card.toml".into(), data: widget.as_bytes().to_vec() },
        ];
        install_entries(&entries, &src.themes_dir, false, &Limits::default()).unwrap();
        apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        let cc = io::json_load(&paths.shell_json())["control_center"].clone();
        assert_eq!((cc["enabled"].clone(), cc["style"].clone(), cc["tiles"].clone(), cc["position"].clone()), (json!(true), json!("fluent"), json!(["sound"]), json!("top-left")));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn apply_sets_every_file_the_theme_owns() {
        let (d, src, paths) = sandbox("all");
        std::fs::write(&paths.rdwm_config, "# mine\n[general]\ntags = [\"a\"]\n").unwrap();
        std::fs::write(paths.shell_json(), r#"{"workspaces": "off", "bar": {"x": 1}, "hot_corners": {"top_right": "lock", "dwell_ms": 300}}"#).unwrap();
        std::fs::write(paths.dock_settings(), r#"{"magnification": 1.8, "roundness": 0.2}"#).unwrap();
        // Exercise ownership with an installed face, rather than depending on
        // one user's optional Inter installation. Unavailable faces are correctly
        // omitted by the production renderer and have separate fallback tests.
        let family = crate::theme::fontdb::get().picker_families().into_iter().next().expect("theme font integration requires an installed font");
        let face = toml_edit::Value::from(family.as_str());
        let manifest = SUNSET.replacen("ui = \"Inter\"", &format!("ui = {face}\ntitle = {face}"), 1);
        install_sunset_manifest(&src, &manifest);
        let a = apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        assert_eq!((a.theme_id.as_str(), a.dark), ("sunset", true));
        assert!(!a.undo_id.is_empty());

        let cfg = std::fs::read_to_string(&paths.rdwm_config).unwrap();
        assert!(cfg.starts_with("# mine\n"), "comments survive:\n{cfg}");
        for needle in ["style = \"classic\"", "height = 36", "gap_inner = 9", "border_px = 3", "layouts = [\"tile\", \"monocle\"]", "focused = \"#ff7849\"", "buttons = true"] {
            assert!(cfg.contains(needle), "{needle} in\n{cfg}");
        }
        let font = format!("font = {}", toml_edit::Value::from(family));
        assert!(cfg.contains(&font), "{font} in\n{cfg}");
        let shell = io::json_load(&paths.shell_json());
        assert_eq!(shell["theme"]["name"], "sunset");
        assert_eq!(shell["workspaces"], "off");
        assert_eq!((shell["bar"]["x"].clone(), shell["bar"]["left"][0].clone()), (json!(1), json!("workspaces")), "bar keys merge");
        assert_eq!((shell["hot_corners"]["top_left"].clone(), shell["hot_corners"]["top_right"].clone(), shell["hot_corners"]["dwell_ms"].clone()), (json!("dashboard"), json!("lock"), json!(300)));
        assert_eq!(shell["icons"]["theme"], "Papirus");
        assert!(shell.get("control_center").is_none(), "a theme without the section leaves the shell default (present everywhere)");
        let items = shell["widgets"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!((items[0]["source"].as_str(), items[0]["from_theme"].as_bool()), (Some("sunset"), Some(true)));
        assert!(items[1].get("source").is_none() && items[1]["use"] == "builtin:cpu-ram-ring");
        let dock = io::json_load(&paths.dock_settings());
        assert_eq!((dock["style"].as_str(), dock["windowsAlign"].as_str(), dock["windowsPosition"].as_str()), (Some("windows"), Some("left"), Some("top")));
        assert_eq!(dock["magnification"], 1.8);
        assert!(dock.get("roundness").is_none(), "the theme's roundness rules when the theme names a dock style");
        // a window-button side the running rdwm cannot place is reported, not fatal
        assert!(a.notes.iter().any(|n| n.contains("button side")), "{:?}", a.notes);
        assert!(src.selection().name == "sunset");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn undo_restores_every_byte_and_removes_new_files() {
        let (d, src, paths) = sandbox("undo");
        std::fs::write(&paths.rdwm_config, "[general]\ntags = [\"a\"]\n").unwrap();
        let shell_before = r#"{"workspaces": "off"}"#;
        std::fs::write(paths.shell_json(), shell_before).unwrap();
        install_sunset(&src);
        let before_cfg = std::fs::read_to_string(&paths.rdwm_config).unwrap();
        assert!(!paths.dock_settings().exists());
        apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        assert_ne!(std::fs::read_to_string(&paths.rdwm_config).unwrap(), before_cfg);
        assert!(paths.dock_settings().exists());
        let who = undo(&src, &paths, None).unwrap();
        assert_eq!(who, "sunset");
        assert_eq!(std::fs::read_to_string(&paths.rdwm_config).unwrap(), before_cfg);
        assert_eq!(std::fs::read_to_string(paths.shell_json()).unwrap(), shell_before);
        assert!(!paths.dock_settings().exists(), "a file the apply created is removed again");
        assert!(undo(&src, &paths, None).unwrap_err().contains("nothing to undo"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn undo_steps_back_through_several_applies() {
        let (d, src, paths) = sandbox("steps");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        apply(&src, &paths, "nord", &Selection::default()).unwrap();
        apply(&src, &paths, "dracula", &Selection::default()).unwrap();
        assert_eq!(src.selection().name, "dracula");
        assert_eq!(undo(&src, &paths, None).unwrap(), "dracula");
        assert_eq!(src.selection().name, "nord");
        undo(&src, &paths, None).unwrap();
        assert_eq!(src.selection().name, "macos", "back to nothing selected");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_failing_write_rolls_everything_back() {
        let (d, src, paths) = sandbox("rollback");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        std::fs::write(paths.shell_json(), "{}").unwrap();
        install_sunset(&src);
        let mut p = plan(&src, &paths, "sunset", &Selection::default()).unwrap();
        assert!(p.writes.len() >= 3);
        // make the last file unwritable: its directory is a regular file
        std::fs::write(d.join("blocker"), "x").unwrap();
        let last = p.writes.len() - 1;
        p.writes[last].path = d.join("blocker/inner.json");
        let before: Vec<_> = [&paths.rdwm_config, &paths.shell_json()].iter().map(|f| std::fs::read_to_string(f).unwrap()).collect();
        let e = execute(&src, &paths, p).unwrap_err();
        assert!(e.contains("nothing was changed"), "{e}");
        assert_eq!(std::fs::read_to_string(&paths.rdwm_config).unwrap(), before[0]);
        assert_eq!(std::fs::read_to_string(paths.shell_json()).unwrap(), before[1]);
        assert!(undo_ids(&paths).is_empty(), "no stale record");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_rejected_rdwm_file_blocks_the_whole_apply() {
        let (d, src, mut paths) = sandbox("reject");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        std::fs::write(paths.shell_json(), "{}").unwrap();
        install_sunset(&src);
        paths.rdwmctl = "/bin/false".into();
        let e = plan(&src, &paths, "sunset", &Selection::default());
        // /bin/false prints nothing: validation fails without a message
        assert!(e.is_err());
        assert_eq!(std::fs::read_to_string(paths.shell_json()).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn broken_or_unknown_themes_change_nothing() {
        let (d, src, paths) = sandbox("broken");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        assert!(apply(&src, &paths, "nope", &Selection::default()).unwrap_err().contains("no theme"));
        std::fs::create_dir_all(&src.themes_dir).unwrap();
        std::fs::write(src.themes_dir.join("bad.toml"), "format = 1\nname = \"x\"\n[colors.dark]\nbackground = \"red\"\n").unwrap();
        assert!(apply(&src, &paths, "bad", &Selection::default()).unwrap_err().contains("broken"));
        assert!(!paths.shell_json().exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn custom_widgets_require_a_choice_and_survive_widgetless_themes() {
        let (d, src, paths) = sandbox("widgets-choice");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        install_sunset(&src);
        let widgets = json!({"enabled": true, "allow_exec": false, "future": 42,
            "items": [{"use": "builtin:clock-card", "x": 123}]});
        io::json_set(&paths.shell_json(), "widgets", Some(widgets.clone())).unwrap();
        let before = std::fs::read(paths.shell_json()).unwrap();
        assert!(widgets_need_confirmation(&src, &paths, "sunset").unwrap());
        assert!(plan(&src, &paths, "sunset", &Selection::default()).is_err());
        assert_eq!(std::fs::read(paths.shell_json()).unwrap(), before);
        apply(&src, &paths, "nord", &Selection::default()).unwrap();
        let hidden = io::json_load(&paths.shell_json())["widgets"].clone();
        assert_eq!(hidden["enabled"], false);
        assert_eq!(hidden["items"], widgets["items"]);
        assert!(widgets_need_confirmation(&src, &paths, "sunset").unwrap());
        let p = plan_with_widgets(&src, &paths, "sunset", &src.selection(), WidgetChoice::Keep).unwrap();
        execute(&src, &paths, p).unwrap();
        let kept = io::json_load(&paths.shell_json())["widgets"].clone();
        assert_eq!(kept["enabled"], true);
        assert_eq!(kept["items"], widgets["items"]);
        assert_eq!(kept["future"], 42);
        let p = plan_with_widgets(&src, &paths, "sunset", &src.selection(), WidgetChoice::Replace).unwrap();
        execute(&src, &paths, p).unwrap();
        let replaced = io::json_load(&paths.shell_json())["widgets"].clone();
        assert_eq!(replaced["items"].as_array().unwrap().len(), 2);
        assert_eq!(replaced["allow_exec"], false);
        assert!(!custom_widgets(&replaced));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn theme_widgets_follow_the_theme_and_empty_layouts_accept_defaults() {
        let (d, src, paths) = sandbox("widgets-theme");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        install_sunset(&src);
        apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        let seeded = io::json_load(&paths.shell_json())["widgets"].clone();
        assert_eq!(seeded["items"].as_array().unwrap().len(), 2);
        apply(&src, &paths, "nord", &src.selection()).unwrap();
        assert_eq!(io::json_load(&paths.shell_json())["widgets"]["enabled"], false);
        assert!(!widgets_need_confirmation(&src, &paths, "sunset").unwrap());
        apply(&src, &paths, "sunset", &src.selection()).unwrap();
        assert_eq!(io::json_load(&paths.shell_json())["widgets"], seeded);
        io::json_set(&paths.shell_json(), "widgets", Some(json!({"enabled": true, "items": []}))).unwrap();
        apply(&src, &paths, "sunset", &src.selection()).unwrap();
        assert_eq!(io::json_load(&paths.shell_json())["widgets"]["items"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn package_match_colors_does_not_enable_dynamic_colors() {
        let (d, src, paths) = sandbox("match");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        let text = SUNSET.replace("[wallpaper]\n", "[wallpaper]\nmatch_colors = true\n");
        let entries = vec![
            Entry { path: "theme.toml".into(), data: text.into_bytes() },
            Entry { path: "wallpapers/sunset.png".into(), data: png(32, 18, [30, 150, 90]) },
            Entry { path: "widgets/clock-card.toml".into(), data: b"format = 1\n[root]\ntype = \"spacer\"\n".to_vec() },
        ];
        install_entries(&entries, &src.themes_dir, false, &Limits::default()).unwrap();
        apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        let sel = src.selection();
        assert!(sel.dynamic.is_none());
        assert!(io::json_load(&paths.shell_json()).get("theme_dynamic").is_none());
        let (t, _) = src.build(&sel, true);
        assert_eq!(t.accent, crate::theme::Rgb(0xff, 0x78, 0x49));
        // another theme drops it again
        apply(&src, &paths, "nord", &src.selection()).unwrap();
        assert!(src.selection().dynamic.is_none_or(|d| !d.enabled), "the next theme does not keep it on");
        assert!(io::json_load(&paths.shell_json()).get("theme_dynamic").is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn preview_reverts_by_itself() {
        let (d, src, paths) = sandbox("preview");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let handle = std::thread::scope(|s| {
            let h = s.spawn(|| preview(&src, &paths, "nord", 1, &cancel));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while src.selection().name != "nord" && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(src.selection().name, "nord", "applied during the preview");
            let mut cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
            cfg.set("wallpaper", "image", &Val::Str("/my/during-preview.png".into())).unwrap();
            std::fs::write(&paths.rdwm_config, cfg.text()).unwrap();
            io::json_set(&paths.shell_json(), "theme_wallpaper", Some(json!({"images":["/my/during-preview.png"]}))).unwrap();
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            h.join().unwrap()
        });
        assert!(handle.is_ok());
        assert_eq!(src.selection().name, "macos", "reverted after the preview");
        let cfg = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap()).unwrap();
        assert_eq!(cfg.get("wallpaper", "image"), Some(Val::Str("/my/during-preview.png".into())));
        assert_eq!(io::json_load(&paths.shell_json())["theme_wallpaper"]["images"], json!(["/my/during-preview.png"]));
        let _ = std::fs::remove_dir_all(d);
    }
}
