//! Applying a theme package to the shell: one transaction over every file the theme owns, with undo.
//!
//! [`plan`] computes the new text of each file without touching anything and checks it (rdwm's config goes through
//! `rdwmctl check-config`). [`execute`] saves the old files as an undo record, writes them atomically one by one and puts everything
//! back if any write fails. [`undo`] restores the latest record; [`preview`] is apply, wait, undo.
//!
//! Files: rdwm's `config.toml` (colours, titlebars, gaps, border colours, layouts, wallpaper), `dock-settings.json` (dock style and
//! alignment) and `decebal.json` (theme selection, `bar`, `dock`, `hot_corners`, `widgets`, `icons`, `theme_wallpaper`).
//! A key the running rdwm does not know yet is skipped and reported in [`Applied::notes`], not an error.

use super::schema::Status;
use crate::settings::io::{self, Paths};
use crate::settings::schema::Schema;
use crate::settings::tomlcfg::{Cfg, Val};
use crate::theme::live::{system_dark, Source};
use crate::theme::model::{Origin, Selection, Surfaces};
use crate::theme::{dynamic, rdwm};
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
    ("wallpaper", "mode", "wallpaper", "mode", Conv::Str),
    ("wallpaper", "color", "wallpaper", "color", Conv::Str),
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
fn rdwm_extras(cfg: &mut Cfg, extra: &Map<String, Value>, wallpaper: Option<&Path>, schema: &Schema, notes: &mut Vec<String>) -> Result<(), String> {
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
    if let Some(p) = wallpaper {
        set(cfg, "wallpaper", "image", Val::Str(p.display().to_string()), "wallpaper.image", notes)?;
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

/// Computes everything `apply` would write, touching nothing.
pub fn plan(src: &Source, paths: &Paths, name: &str, base: &Selection) -> Result<Plan, String> {
    let (id, implied) = src.find(name).ok_or_else(|| format!("no theme `{name}` (decthemectl theme list)"))?;
    if let Some(e) = src.list().into_iter().find(|e| e.id == id).and_then(|e| e.error) {
        return Err(format!("theme `{id}` is broken: {e}"));
    }
    let mut notes: Vec<String> = Vec::new();
    let def = src.load_def(&id)?;
    let extra = def.extra.clone();
    let pkg_dir = crate::widgets::config::package_dir(&src.themes_dir, &id);

    let mut sel = base.clone();
    sel.name = id.clone();
    if let Some(m) = implied {
        sel.mode = m;
    }

    // wallpaper: the first bundled image, resolved inside the package folder
    let wp = extra.get("wallpaper");
    let images: Vec<String> = wp
        .map(|w| {
            let mut v: Vec<String> = w.get("images").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            if let Some(i) = w.get("image").and_then(Value::as_str) {
                v.insert(0, i.to_string());
            }
            v
        })
        .unwrap_or_default();
    let image_path = match (&pkg_dir, images.first()) {
        (Some(d), Some(i)) => {
            super::archive::safe_rel_path(i)?;
            let p = d.join(i);
            if std::fs::symlink_metadata(&p).map(|m| m.is_file()).unwrap_or(false) {
                Some(p)
            } else {
                notes.push(format!("wallpaper `{i}` is missing from the package"));
                None
            }
        }
        (None, Some(_)) => {
            notes.push("a theme without a package folder cannot bundle a wallpaper".into());
            None
        }
        _ => None,
    };
    let match_colors = wp.and_then(|w| w.get("match_colors")).and_then(Value::as_bool) == Some(true);
    let mut shell_marks: Vec<(&str, Option<Value>)> = Vec::new();
    if match_colors {
        let w = wp.cloned().unwrap_or(Value::Null);
        let scheme = w.get("scheme").and_then(Value::as_str).and_then(crate::theme::material::Variant::parse);
        let surfaces = w.get("surfaces").and_then(Value::as_str).and_then(Surfaces::parse);
        let (strength, follow) = (w.get("strength").and_then(Value::as_f64), w.get("follow_mode").and_then(Value::as_bool));
        let src_origin = image_path.as_ref().map(|p| Origin::File(p.display().to_string()));
        match dynamic::update_with(&sel, paths, image_path.as_deref(), |d| {
            d.enabled = true;
            if let Some(s) = scheme {
                d.scheme = s;
            }
            if let Some(s) = surfaces {
                d.surfaces = s;
            }
            if let Some(s) = strength {
                d.strength = s as f32;
            }
            if let Some(f) = follow {
                d.follow_mode = f;
            }
            if let Some(o) = src_origin {
                d.source = o;
            }
        }) {
            Ok(s) => {
                sel = s;
                shell_marks.push(("theme_dynamic", Some(json!(id))));
            }
            Err(e) => notes.push(format!("wallpaper.match_colors: {e}")),
        }
    } else if io::json_load(&paths.shell_json()).get("theme_dynamic").is_some() {
        // the previous theme switched "Match wallpaper" on; this one does not ask for it, so it goes off again
        sel = dynamic::disable(&sel);
        shell_marks.push(("theme_dynamic", None));
    }

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
    for (k, v) in &shell_marks {
        match v {
            Some(v) => {
                shell.as_object_mut().expect("object").insert((*k).into(), v.clone());
            }
            None => {
                shell.as_object_mut().expect("object").shift_remove(*k);
            }
        }
    }
    if let Some(Value::Object(bar)) = extra.get("bar") {
        let o = obj_mut(&mut shell, "bar");
        for k in ["enabled", "position", "left", "center", "right"] {
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
    if let Some(Value::Object(ic)) = extra.get("icons") {
        obj_mut(&mut shell, "icons").extend(ic.clone());
    }
    {
        // widgets: this theme's instances replace the previous theme's; the user's own stay
        let w = obj_mut(&mut shell, "widgets");
        let mut items: Vec<Value> = w.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        items.retain(|i| i.get("from_theme").and_then(Value::as_bool) != Some(true));
        for inst in extra.get("widgets").and_then(Value::as_array).into_iter().flatten() {
            let mut inst = inst.clone();
            let Some(o) = inst.as_object_mut() else { continue };
            let use_ = o.get("use").and_then(Value::as_str).unwrap_or("").to_string();
            if !use_.starts_with("builtin:") {
                match widget_source(src, &id, &use_) {
                    Some(s) => {
                        o.insert("source".into(), json!(s));
                    }
                    None if crate::widgets::samples::source(&use_).is_some() => {}
                    None => {
                        notes.push(format!("widget `{use_}` is not in the package or its parents, left out"));
                        continue;
                    }
                }
            }
            o.insert("from_theme".into(), json!(true));
            items.push(inst);
        }
        w.insert("items".into(), Value::Array(items));
    }
    if wp.is_some_and(|w| w.get("slideshow").and_then(Value::as_bool) == Some(true)) {
        let dir = pkg_dir.as_deref().unwrap_or(Path::new(""));
        let mut s = Map::new();
        s.insert("images".into(), Value::Array(images.iter().map(|i| json!(dir.join(i).display().to_string())).collect()));
        for k in ["slideshow", "interval", "shuffle"] {
            if let Some(v) = wp.and_then(|w| w.get(k)) {
                s.insert(k.into(), v.clone());
            }
        }
        shell.as_object_mut().expect("object").insert("theme_wallpaper".into(), Value::Object(s));
        notes.push("wallpaper.slideshow: stored in decebal.json `theme_wallpaper`; nothing rotates the images yet".into());
    } else {
        shell.as_object_mut().expect("object").shift_remove("theme_wallpaper");
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
        let mut attempt = extra.clone();
        let mut text;
        loop {
            let mut c = cfg.clone();
            rdwm_extras(&mut c, &attempt, image_path.as_deref(), &schema, &mut notes)?;
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
    Ok(Plan { id, dark: theme.dark, selection: sel, writes, notes, rdwm_changed })
}

// ---------------------------------------------------------------- execute and undo

fn stamp_id() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{:013}-{:06}", d.as_millis(), std::process::id() % 1_000_000)
}

/// Writes the plan: undo record first, then each file atomically; any failure restores what was already written.
pub fn execute(_src: &Source, paths: &Paths, plan: Plan) -> Result<Applied, String> {
    let mut applied = Applied { theme_id: plan.id.clone(), dark: plan.dark, files: Vec::new(), notes: plan.notes.clone(), reloaded: false, undo_id: String::new() };
    if plan.writes.is_empty() {
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
    crate::theme::system::request(&paths.shell_json()); // "Apply to all apps", when the user switched it on
    Ok(applied)
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

/// The undo records, oldest first.
pub fn undo_ids(paths: &Paths) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(undo_dir(paths)).into_iter().flatten().flatten().filter(|e| e.path().join("meta.json").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
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
        restore(&w)?;
    }
    let theme = meta["theme"].as_str().unwrap_or("").to_string();
    let _ = std::fs::remove_dir_all(&rec);
    if rdwm_touched {
        let _ = rdwm::reload(paths);
    }
    crate::theme::live::reload();
    crate::theme::system::request(&paths.shell_json());
    Ok(theme)
}

/// Applies a theme now and puts everything back after `secs` (or on `cancel`). Blocks; run it on a thread.
pub fn preview(src: &Source, paths: &Paths, name: &str, secs: u64, cancel: &std::sync::atomic::AtomicBool) -> Result<Applied, String> {
    let a = apply(src, paths, name, &src.selection())?;
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
        let widget = "format = 1\n[root]\ntype = \"text\"\ntext = \"{clock:%H:%M}\"\n";
        let entries = vec![
            Entry { path: "theme.toml".into(), data: SUNSET.as_bytes().to_vec() },
            Entry { path: "wallpapers/sunset.png".into(), data: png(32, 18, [200, 80, 40]) },
            Entry { path: "widgets/clock-card.toml".into(), data: widget.as_bytes().to_vec() },
        ];
        install_entries(&entries, &src.themes_dir, false, &Limits::default()).unwrap();
    }

    #[test]
    fn apply_sets_every_file_the_theme_owns() {
        let (d, src, paths) = sandbox("all");
        std::fs::write(&paths.rdwm_config, "# mine\n[general]\ntags = [\"a\"]\n").unwrap();
        std::fs::write(paths.shell_json(), r#"{"workspaces": "off", "bar": {"x": 1}, "hot_corners": {"top_right": "lock", "dwell_ms": 300}}"#).unwrap();
        std::fs::write(paths.dock_settings(), r#"{"magnification": 1.8, "roundness": 0.2}"#).unwrap();
        install_sunset(&src);
        let a = apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        assert_eq!((a.theme_id.as_str(), a.dark), ("sunset", true));
        assert!(!a.undo_id.is_empty());

        let cfg = std::fs::read_to_string(&paths.rdwm_config).unwrap();
        assert!(cfg.starts_with("# mine\n"), "comments survive:\n{cfg}");
        for needle in [
            "style = \"classic\"",
            "height = 36",
            "gap_inner = 9",
            "border_px = 3",
            "layouts = [\"tile\", \"monocle\"]",
            "mode = \"fit\"",
            "wallpapers/sunset.png",
            "font = \"Inter\"",
            "focused = \"#ff7849\"",
            "buttons = true",
        ] {
            assert!(cfg.contains(needle), "{needle} in\n{cfg}");
        }
        let shell = io::json_load(&paths.shell_json());
        assert_eq!(shell["theme"]["name"], "sunset");
        assert_eq!(shell["workspaces"], "off");
        assert_eq!((shell["bar"]["x"].clone(), shell["bar"]["left"][0].clone()), (json!(1), json!("workspaces")), "bar keys merge");
        assert_eq!((shell["hot_corners"]["top_left"].clone(), shell["hot_corners"]["top_right"].clone(), shell["hot_corners"]["dwell_ms"].clone()), (json!("dashboard"), json!("lock"), json!(300)));
        assert_eq!(shell["icons"]["theme"], "Papirus");
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
    fn re_applying_replaces_the_previous_themes_widgets_but_keeps_the_users() {
        let (d, src, paths) = sandbox("widgets");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        std::fs::write(paths.shell_json(), r#"{"widgets": {"items": [{"use": "mine", "x": 5}]}}"#).unwrap();
        install_sunset(&src);
        apply(&src, &paths, "sunset", &Selection::default()).unwrap();
        assert_eq!(io::json_load(&paths.shell_json())["widgets"]["items"].as_array().unwrap().len(), 3);
        apply(&src, &paths, "nord", &Selection::default()).unwrap();
        let items = io::json_load(&paths.shell_json())["widgets"]["items"].clone();
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["use"], "mine");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn match_colors_derives_the_palette_from_the_bundled_wallpaper() {
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
        assert!(sel.dynamic.as_ref().is_some_and(|d| d.enabled), "Match wallpaper is on, with colours read from the bundled image");
        assert_eq!(io::json_load(&paths.shell_json())["theme_dynamic"], "sunset");
        let (t, _) = src.build(&sel, true);
        assert_ne!(t.accent, crate::theme::Rgb(0xff, 0x78, 0x49), "the palette replaced the manifest's accent");
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
            std::thread::sleep(std::time::Duration::from_millis(400));
            assert_eq!(src.selection().name, "nord", "applied during the preview");
            h.join().unwrap()
        });
        assert!(handle.is_ok());
        assert_eq!(src.selection().name, "macos", "reverted after the preview");
        let _ = std::fs::remove_dir_all(d);
    }
}
