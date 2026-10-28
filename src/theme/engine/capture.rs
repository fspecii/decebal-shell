//! "Create theme from current settings": a snapshot of the live look as a new package in the user's theme folder.
//!
//! Colours and metrics come from the resolved theme (your accent, radius and text size included); the rest is read back from the
//! files [`super::apply`] writes: rdwm's decoration, gaps and wallpaper, the dock settings, the bar and hot-corner switches, the
//! widgets you placed. A bundled wallpaper is copied into the package when it is an ordinary image under the size cap. Widgets
//! that could run commands are left out: a package is data.

use super::archive::{Entry, Limits};
use super::package;
use super::writer::{def_to_manifest, Header};
use crate::settings::io::{self, Paths};
use crate::settings::tomlcfg::Cfg;
use crate::theme::live::{system_dark, Source};
use crate::theme::model::{Kind, Tokens, TOKENS, V};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

#[derive(Debug)]
pub struct Captured {
    pub dir: PathBuf,
    pub notes: Vec<String>,
}

fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest)).unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

/// Resolved tokens of one variant as a [`Tokens`] map (colours only).
fn colour_tokens(t: &crate::theme::Theme) -> Tokens {
    let mut m = Tokens::new();
    for (k, c) in [
        ("background", t.background),
        ("sidebar", t.sidebar),
        ("surface", t.surface),
        ("bar", t.bar),
        ("bar_text", t.bar_text),
        ("popup", t.popup),
        ("dock", t.dock),
        ("overlay", t.overlay),
        ("titlebar", t.titlebar),
        ("titlebar_inactive", t.titlebar_inactive),
        ("border", t.border),
        ("text", t.text),
        ("text_secondary", t.text_secondary),
        ("text_faint", t.text_faint),
        ("accent", t.accent),
        ("accent_text", t.accent_text),
        ("danger", t.danger),
        ("ok", t.ok),
        ("warn", t.warn),
    ] {
        m.insert(k.into(), V::S(c.hex()));
    }
    m
}

fn r3(x: f32) -> f64 {
    (f64::from(x) * 1000.0).round() / 1000.0
}

/// Writes `<themes>/<id>/` from the current settings. Fails when `id` exists.
pub fn from_current(src: &Source, paths: &Paths, id: &str, name: &str) -> Result<Captured, String> {
    if !super::schema::valid_ident(id) {
        return Err(format!("`{id}` is not a valid theme id (lower-case letters, digits, `.`, `_`, `-`)"));
    }
    if src.themes_dir.join(id).exists() || src.themes_dir.join(format!("{id}.toml")).exists() || crate::theme::builtin::source(id).is_some() {
        return Err(format!("a theme called `{id}` already exists; pick another id"));
    }
    let lim = Limits::default();
    let mut notes: Vec<String> = Vec::new();
    let mut files: Vec<Entry> = Vec::new();
    let sel = src.selection();
    let (cur, problem) = src.build(&sel, system_dark());
    if let Some(p) = problem {
        return Err(format!("the current theme is broken ({p}); fix it first"));
    }
    let cur_id = src.find(&sel.name).map(|f| f.0).unwrap_or_else(|| cur.id.clone());
    let mut def = src.load_def(&cur_id)?;
    def.inherit = None;
    def.name = name.to_string();
    def.description = String::new();
    def.extra.shift_remove("meta");
    def.extra.shift_remove("integrity");

    // colours: every colour of each variant, with the user's overrides applied
    def.common.retain(|k, _| !matches!(TOKENS.iter().find(|t| t.0 == k).map(|t| &t.1), Some(Kind::Color)));
    for dark in [true, false] {
        let slot_present = if dark { def.has_dark() } else { def.has_light() };
        if !slot_present {
            continue;
        }
        let t = def.resolve(id, dark, &sel);
        let toks = colour_tokens(&t);
        if dark {
            def.dark = Some(toks);
        } else {
            def.light = Some(toks);
        }
    }
    def.dark_default = cur.dark;
    // metrics of the variant in use
    for (k, v) in [
        ("radius", cur.radius),
        ("dock_roundness", cur.dock_roundness),
        ("dock_alpha", cur.dock_alpha),
        ("popup_alpha", cur.popup_alpha),
        ("panel_alpha", cur.panel_alpha),
        ("bar_alpha", cur.bar_alpha),
        ("shadow", cur.shadow),
        ("outline", cur.outline),
        ("dock_icon_size", cur.dock_icon_size),
        ("dock_padding", cur.dock_padding),
        ("bar_height", cur.bar_height),
        ("font_scale", cur.font_scale),
    ] {
        def.common.insert(k.into(), V::N(r3(v)));
    }
    def.common.insert("bar_style".into(), V::S(cur.bar_style.clone()));
    def.common.insert("titlebar_style".into(), V::S(cur.titlebar_style.clone()));
    def.common.insert("titlebar_height".into(), V::N(cur.titlebar_height as f64));
    // fonts: the theme's own, with the user's preset / per-role / family choices on top
    if let Some(p) = &sel.fonts.preset {
        def.common.insert("font_preset".into(), V::S(p.clone()));
    }
    for r in crate::theme::fonts::Role::ALL {
        if let Some(chain) = sel.fonts.role(r) {
            def.common.insert(format!("font_{}", r.key()), V::L(chain.clone()));
        }
    }
    if let Some(f) = &sel.font {
        def.common.insert("font_family".into(), V::S(f.clone()));
    }
    // these tokens now live in the sections; drop stale section copies so the token values win
    for (sec, key) in [("font", "scale"), ("bar", "style"), ("bar", "height"), ("dock", "size"), ("dock", "roundness"), ("decoration", "style"), ("decoration", "height")] {
        if let Some(Value::Object(o)) = def.extra.get_mut(sec) {
            o.shift_remove(key);
        }
    }
    // everything below is read back from the live files, so the theme's own copies of these sections go
    for sec in ["widgets", "wallpaper", "decoration", "rdwm", "dock", "bar", "hot_corners", "control_center", "icons"] {
        def.extra.shift_remove(sec);
    }

    // ---- rdwm: decoration, gaps, wallpaper
    let rd = std::fs::read_to_string(&paths.rdwm_config).ok().and_then(|t| Cfg::parse(&t).ok());
    let mut deco = Map::new();
    let mut rd_sec = Map::new();
    let mut wall = Map::new();
    if let Some(cfg) = &rd {
        for (key, conv) in [("title_align", "s"), ("radius", "i")] {
            if let Some(v) = cfg.get("decoration", key) {
                match (conv, v.as_str(), v.as_i64()) {
                    ("s", Some(s), _) => deco.insert(key.into(), json!(s)),
                    (_, _, Some(i)) => deco.insert(key.into(), json!(i)),
                    _ => None,
                };
            }
        }
        if cfg.get("decoration", "buttons").and_then(|v| v.as_bool()) == Some(false) {
            deco.insert("buttons".into(), json!("none"));
        }
        for key in ["gap_inner", "gap_outer", "border_px", "nmaster"] {
            if let Some(i) = cfg.get("general", key).and_then(|v| v.as_i64()) {
                rd_sec.insert(key.into(), json!(i));
            }
        }
        if let Some(b) = cfg.get("general", "smart_gaps").and_then(|v| v.as_bool()) {
            rd_sec.insert("smart_gaps".into(), json!(b));
        }
        if let Some(f) = cfg.get("general", "mfact").and_then(|v| v.as_f64()) {
            rd_sec.insert("mfact".into(), json!(f));
        }
        if let Some(crate::settings::tomlcfg::Val::List(l)) = cfg.get("general", "layouts") {
            let ok: Vec<&String> = l.iter().filter(|x| super::schema::LAYOUTS.contains(&x.as_str())).collect();
            if !ok.is_empty() {
                rd_sec.insert("layouts".into(), json!(ok));
            }
        }
        if let Some(m) = cfg.get("wallpaper", "mode").and_then(|v| v.as_str().map(String::from)) {
            wall.insert("mode".into(), json!(m));
        }
        if let Some(c) = cfg.get("wallpaper", "color").and_then(|v| v.as_str().map(String::from)).filter(|c| crate::theme::Rgb::parse(c).is_some()) {
            wall.insert("color".into(), json!(c));
        }
        if let Some(img) = cfg.get("wallpaper", "image").and_then(|v| v.as_str().map(String::from)) {
            let p = expand(&img);
            let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_file() && m.len() <= lim.max_file && ["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) => {
                    let data = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    match package::check_image(&format!("w.{ext}"), &data) {
                        None => {
                            let rel = format!("wallpapers/wallpaper.{ext}");
                            files.push(Entry { path: rel.clone(), data });
                            wall.insert("image".into(), json!(rel));
                        }
                        Some(e) => notes.push(format!("wallpaper not bundled: {e}")),
                    }
                }
                Ok(_) => notes.push(format!("wallpaper {} not bundled (not a png/jpg/webp file under {} MiB)", p.display(), lim.max_file >> 20)),
                Err(_) => notes.push(format!("wallpaper {} not found, not bundled", p.display())),
            }
        }
    }
    if !deco.is_empty() {
        def.extra.insert("decoration".into(), Value::Object(deco));
    }
    if !rd_sec.is_empty() {
        def.extra.insert("rdwm".into(), Value::Object(rd_sec));
    }
    if !wall.is_empty() {
        def.extra.insert("wallpaper".into(), Value::Object(wall));
    }

    // ---- dock, bar, hot corners, icons
    let dock_json = io::json_load(&paths.dock_settings());
    let shell = io::json_load(&paths.shell_json());
    let mut dock = Map::new();
    if let Some(s) = dock_json.get("style").and_then(Value::as_str).filter(|s| matches!(*s, "mac" | "windows")) {
        dock.insert("style".into(), json!(s));
    }
    for (jk, k) in [("windowsAlign", "align"), ("windowsPosition", "position")] {
        if let Some(s) = dock_json.get(jk).and_then(Value::as_str) {
            dock.insert(k.into(), json!(s));
        }
    }
    if let Some(m) = dock_json.get("magnification").and_then(Value::as_f64).filter(|m| (1.0..=2.5).contains(m)) {
        dock.insert("magnification".into(), json!(m));
    }
    if let Some(r) = dock_json.get("roundness").and_then(Value::as_f64).filter(|m| (0.0..=1.0).contains(m)) {
        dock.insert("roundness".into(), json!(r));
    }
    if let Some(e) = shell.get("dock").and_then(|d| d.get("enabled")).filter(|e| e.is_boolean() || e.as_str() == Some("auto")) {
        dock.insert("enabled".into(), e.clone());
    }
    if !dock.is_empty() {
        def.extra.insert("dock".into(), Value::Object(dock));
    }
    let mut bar = def.extra.get("bar").and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(sb) = shell.get("bar").and_then(Value::as_object) {
        if let Some(e) = sb.get("enabled").filter(|e| e.is_boolean() || e.as_str() == Some("auto")) {
            bar.insert("enabled".into(), e.clone());
        }
        if let Some(p) = sb.get("position").and_then(Value::as_str).filter(|p| super::schema::POSITIONS.contains(p)) {
            bar.insert("position".into(), json!(p));
        }
        for k in ["left", "center", "right"] {
            if let Some(a) = sb.get(k).and_then(Value::as_array) {
                bar.insert(k.into(), Value::Array(a.iter().filter(|x| x.is_string()).cloned().collect()));
            }
        }
    }
    if !bar.is_empty() {
        def.extra.insert("bar".into(), Value::Object(bar));
    }
    if let Some(hc) = shell.get("hot_corners").and_then(Value::as_object) {
        let mut safe = Map::new();
        for (k, v) in hc {
            match (k.as_str(), v) {
                ("dwell_ms", v) if v.is_u64() => {
                    safe.insert(k.clone(), v.clone());
                }
                ("top_left" | "top_right" | "bottom_left" | "bottom_right", Value::String(a)) if super::schema::safe_hot_actions().contains(&a.as_str()) => {
                    safe.insert(k.clone(), v.clone());
                }
                (_, Value::String(a)) if a.to_ascii_lowercase().starts_with("custom") => notes.push(format!("hot corner {k}: a custom command is not exported (themes are data)")),
                _ => {}
            }
        }
        if !safe.is_empty() {
            def.extra.insert("hot_corners".into(), Value::Object(safe));
        }
    }
    if let Some(cc) = shell.get("control_center").filter(|c| c.is_object()) {
        // canonical and validated: only the keys the format knows, with understood values
        def.extra.insert("control_center".into(), crate::controlcenter::settings::Settings::from_json(cc).to_json());
    }
    if let Some(t) = shell.get("icons").and_then(|i| i.get("theme")).and_then(Value::as_str) {
        def.extra.insert("icons".into(), json!({"theme": t}));
    }

    // ---- widgets: copy the ones that are safe data, with their definition files
    let mut instances: Vec<Value> = Vec::new();
    let wcfg = crate::widgets::config::WidgetsConfig::from_json(&shell);
    for inst in &wcfg.items {
        let mut out = inst.clone();
        out.source = None;
        let label = inst.use_.clone();
        if inst.use_.starts_with("builtin:")
            || (inst.source.is_none() && crate::widgets::samples::source(&inst.use_).is_some() && !crate::widgets::config::user_dir().join(format!("{}.toml", inst.use_)).exists())
        {
            instances.push(out.to_json());
            continue;
        }
        let resolved = crate::widgets::config::resolve(inst, &src.themes_dir, false);
        let text = match &resolved {
            Ok(r) => {
                let file = match &inst.source {
                    Some(s) => crate::widgets::config::package_dir(&src.themes_dir, s).map(|d| d.join("widgets").join(format!("{}.toml", inst.use_))),
                    None => Some(crate::widgets::config::user_dir().join(format!("{}.toml", inst.use_))),
                };
                if !r.def.assets.is_empty() {
                    notes.push(format!("widget {label}: uses image files, left out"));
                    continue;
                }
                file.and_then(|f| std::fs::read_to_string(f).ok())
            }
            Err(e) => {
                notes.push(format!("widget {label}: left out ({})", e.lines().next().unwrap_or("")));
                continue;
            }
        };
        let Some(text) = text else { continue };
        let path = format!("widgets/{}.toml", inst.use_);
        if !files.iter().any(|f| f.path == path) {
            files.push(Entry { path, data: text.into_bytes() });
        }
        instances.push(out.to_json());
    }
    if !instances.is_empty() {
        def.extra.insert("widgets".into(), Value::Array(instances));
    }

    let header = Header {
        id: id.into(),
        name: name.into(),
        version: Some("1.0.0".into()),
        author: Some(std::env::var("USER").unwrap_or_default()).filter(|a| !a.is_empty()),
        license: Some("MIT".into()),
        description: Some(format!("Snapshot of my desktop, based on {}.", cur.name)),
        tags: vec!["snapshot".into()],
        extends: None,
        screenshots: vec![],
    };
    let manifest = def_to_manifest(&def, &header);
    files.push(Entry { path: "theme.toml".into(), data: manifest.into_bytes() });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let (installed, report) = package::install_entries(&files, &src.themes_dir, false, &lim)?;
    notes.extend(report.all_warnings().into_iter().filter(|w| !w.contains("author") && !w.contains("license")));
    Ok(Captured { dir: installed.dir, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::engine::apply::{self, tests::sandbox};
    use crate::theme::model::Selection;

    #[test]
    fn a_snapshot_reproduces_the_look_when_applied_elsewhere() {
        let (d, src, paths) = sandbox("capture");
        std::fs::write(&paths.rdwm_config, "[general]\ngap_inner = 11\n[decoration]\nradius = 4\n[wallpaper]\nmode = \"center\"\n").unwrap();
        let wall = d.join("w.png");
        image::RgbImage::from_pixel(40, 24, image::Rgb([10, 80, 160])).save(&wall).unwrap();
        std::fs::write(&paths.rdwm_config, format!("[general]\ngap_inner = 11\n[decoration]\nradius = 4\n[wallpaper]\nimage = \"{}\"\nmode = \"center\"\n", wall.display())).unwrap();
        std::fs::write(
            paths.shell_json(),
            r#"{"hot_corners": {"top_left": "lock", "top_right": "custom:rm -rf ~"}, "widgets": {"items": [{"use": "builtin:clock-card", "anchor": "top-right", "x": -20, "y": 40}]}}"#,
        )
        .unwrap();
        std::fs::write(paths.dock_settings(), r#"{"style": "windows", "windowsAlign": "left"}"#).unwrap();
        // the look: nord with my accent, radius and text size
        let sel = Selection { name: "nord".into(), accent: crate::theme::Rgb::parse("#ff9f0a"), radius: Some(5.0), font_scale: Some(1.1), ..Selection::default() };
        src.write_selection(&sel).unwrap();

        let c = from_current(&src, &paths, "snap", "Snap").unwrap();
        assert!(c.dir.join("wallpapers/wallpaper.png").is_file());
        assert!(c.notes.iter().any(|n| n.contains("custom command")), "{:?}", c.notes);
        let text = std::fs::read_to_string(c.dir.join("theme.toml")).unwrap();
        assert!(!text.contains("rm -rf"), "{text}");
        let report = package::check(&package::read_dir(&c.dir, &Limits::default()).unwrap(), &Limits::default());
        assert!(report.ok(), "{:?}", report.errors());

        // the package resolves to the same colours and metrics as the live look
        let (live, _) = src.build(&sel, true);
        let (snap, p) = src.build(&Selection { name: "snap".into(), ..Selection::default() }, true);
        assert!(p.is_none(), "{p:?}");
        assert_eq!((snap.accent, snap.background, snap.popup, snap.radius, snap.dock_alpha), (live.accent, live.background, live.popup, live.radius, live.dock_alpha));
        assert!((snap.font_scale - live.font_scale).abs() < 1e-3);

        // apply on a fresh desktop: dock, gaps, wallpaper, hot corners and the widget come along
        let (d2, src2, paths2) = sandbox("capture2");
        std::fs::create_dir_all(&src2.themes_dir).unwrap();
        let (entries, _) = (package::read_dir(&c.dir, &Limits::default()).unwrap(), 0);
        package::install_entries(&entries, &src2.themes_dir, false, &Limits::default()).unwrap();
        std::fs::write(&paths2.rdwm_config, "[general]\n").unwrap();
        apply::apply(&src2, &paths2, "snap", &Selection::default()).unwrap();
        let cfg = std::fs::read_to_string(&paths2.rdwm_config).unwrap();
        for n in ["gap_inner = 11", "mode = \"center\"", "radius = 4", "wallpapers/wallpaper.png"] {
            assert!(cfg.contains(n), "{n} in\n{cfg}");
        }
        let shell = io::json_load(&paths2.shell_json());
        assert_eq!(shell["hot_corners"]["top_left"], "lock");
        assert!(shell["hot_corners"].get("top_right").is_none());
        assert_eq!(shell["widgets"]["items"][0]["use"], "builtin:clock-card");
        assert_eq!(io::json_load(&paths2.dock_settings())["windowsAlign"], "left");
        assert!(c.dir.starts_with(&src.themes_dir));
        assert!(from_current(&src, &paths, "snap", "Again").unwrap_err().contains("already exists"));
        assert!(from_current(&src, &paths, "nord", "x").unwrap_err().contains("already exists"));
        let _ = (std::fs::remove_dir_all(d), std::fs::remove_dir_all(d2));
    }
}
