//! Mapping a theme onto rdwm's `config.toml`: titlebars, window borders and rdwm's own bar.
//!
//! rdwm knows nothing about macshell themes. It already has every colour key needed
//! (`[colors]`, `[decoration] *_bg/fg/border`, `[decoration.bar]`), so applying a theme is a
//! comment-preserving edit of those keys followed by one reload.

use super::model::Theme;
use crate::settings::io::{self, Paths};
use crate::settings::tomlcfg::{Cfg, Val};

/// One key: `Some` sets it, `None` removes it (back to rdwm's default).
pub type Edit = (&'static str, &'static str, Option<Val>);

fn hex(c: super::model::Rgb) -> Val {
    Val::Str(c.hex())
}

/// The keys a theme owns. Everything else in the file is left alone.
pub fn edits(t: &Theme) -> Vec<Edit> {
    let size = (13.0 * t.font_scale * 2.0).round() as f64 / 2.0;
    vec![
        ("colors", "focused", Some(hex(t.accent))),
        ("colors", "normal", Some(hex(t.border))),
        ("colors", "urgent", Some(hex(t.danger))),
        ("decoration", "style", Some(Val::Str(t.titlebar_style.clone()))),
        ("decoration", "height", Some(Val::Int(t.titlebar_height))),
        ("decoration", "radius", Some(Val::Int(t.radius.round() as i64))),
        ("decoration", "focused_bg", Some(hex(t.titlebar))),
        ("decoration", "focused_fg", Some(hex(t.text))),
        ("decoration", "focused_border", Some(hex(t.border))),
        ("decoration", "unfocused_bg", Some(hex(t.titlebar_inactive))),
        ("decoration", "unfocused_fg", Some(hex(t.text_secondary))),
        ("decoration", "unfocused_border", Some(hex(t.border))),
        ("decoration", "inactive_button_color", Some(hex(t.text_faint.over(t.titlebar_inactive, 0.6)))),
        ("decoration", "font", if t.font_family.is_empty() { None } else { Some(Val::Str(t.font_family.clone())) }),
        ("decoration", "font_size", Some(Val::Float(size))),
        ("decoration.bar", "font_size", Some(Val::Float(size))),
        ("decoration.bar", "norm", Some(Val::List(vec![t.text_secondary.hex(), t.bar.hex(), t.border.hex()]))),
        ("decoration.bar", "sel", Some(Val::List(vec![t.accent_text.hex(), t.accent.hex(), t.accent.hex()]))),
        ("decoration.bar", "urgent", Some(Val::List(vec![t.danger.readable_on().hex(), t.danger.hex(), t.danger.hex()]))),
    ]
}

/// Applies [`edits`] to a parsed config in memory.
pub fn apply_to(cfg: &mut Cfg, t: &Theme) -> Result<(), String> {
    for (sec, key, val) in edits(t) {
        match val {
            Some(v) => cfg.set(sec, key, &v)?,
            None => {
                cfg.remove(sec, key);
            }
        }
    }
    Ok(())
}

/// Result of [`write_config`].
#[derive(Debug, PartialEq, Eq)]
pub enum Wrote {
    Changed,
    Unchanged,
}

/// Edits `paths.rdwm_config` (validated with `rdwmctl check-config`, atomic, one-time backup like macsettings).
/// A missing file is created. Nothing is written when validation fails.
pub fn write_config(paths: &Paths, t: &Theme) -> Result<Wrote, String> {
    let old = std::fs::read_to_string(&paths.rdwm_config).unwrap_or_default();
    let mut cfg = Cfg::parse(&old).map_err(|e| format!("{}: {e}", paths.rdwm_config.display()))?;
    apply_to(&mut cfg, t)?;
    let text = cfg.text();
    if text == old {
        return Ok(Wrote::Unchanged);
    }
    io::validate(&paths.rdwmctl, &paths.rdwm_config, &text)?;
    let bak = paths.rdwm_config.with_file_name(format!("{}.{}", paths.rdwm_config.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), io::BACKUP_SUFFIX));
    if paths.rdwm_config.exists() && !bak.exists() {
        let _ = std::fs::copy(&paths.rdwm_config, &bak);
    }
    io::write_atomic(&paths.rdwm_config, &text)?;
    Ok(Wrote::Changed)
}

/// Asks the running rdwm to re-read its config. `Ok(false)` when it is not running.
pub fn reload(paths: &Paths) -> Result<bool, String> {
    if !paths.socket.exists() {
        return Ok(false);
    }
    io::ipc_call(&paths.socket, &serde_json::json!({"cmd": "reload"})).map(|_| true)
}

/// The whole "apply" transaction used by `macthemectl` and the settings pane:
/// resolve -> validate rdwm's file -> write rdwm config -> write the selection -> reload rdwm once.
/// Validation happens before anything is written, so a failure leaves both files untouched.
pub fn apply(src: &super::live::Source, paths: &Paths, sel: &super::model::Selection) -> Result<Report, String> {
    let (theme, problem) = src.build(sel, super::live::system_dark());
    if let Some(p) = problem {
        return Err(p);
    }
    let mut rdwm = None;
    if sel.sync_rdwm {
        let old = std::fs::read_to_string(&paths.rdwm_config).unwrap_or_default();
        let mut cfg = Cfg::parse(&old).map_err(|e| format!("{}: {e}", paths.rdwm_config.display()))?;
        apply_to(&mut cfg, &theme)?;
        io::validate(&paths.rdwmctl, &paths.rdwm_config, &cfg.text())?;
        rdwm = Some(write_config(paths, &theme)?);
    }
    src.write_selection(sel)?;
    let reloaded = if rdwm == Some(Wrote::Changed) { reload(paths).unwrap_or(false) } else { false };
    Ok(Report { theme_id: theme.id.clone(), dark: theme.dark, rdwm, reloaded })
}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub theme_id: String,
    pub dark: bool,
    pub rdwm: Option<Wrote>,
    pub reloaded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::model::Selection;

    fn theme(name: &str, dark: bool) -> Theme {
        let d = crate::theme::builtin::def(name).unwrap();
        d.resolve(name, dark, &Selection::default())
    }

    #[test]
    fn edits_keep_comments_and_foreign_keys() {
        let mut cfg = Cfg::parse("# my config\n[general]\ntags = [\"a\"]\n\n[decoration]\nenabled = true  # keep\nfocused_bg = \"#000000\" # old\nbuttons = false\n\n[decoration.bar]\nnorm = [\"#111111\", \"#222222\", \"#333333\"]\n").unwrap();
        apply_to(&mut cfg, &theme("nord", true)).unwrap();
        let text = cfg.text();
        assert!(text.contains("# my config") && text.contains("enabled = true  # keep") && text.contains("buttons = false"));
        assert!(text.contains("focused_bg = \"#3b4252\" # old"), "{text}");
        assert!(text.contains("[colors]") && text.contains("focused = \"#88c0d0\""));
        assert!(text.contains("tags = [\"a\"]"));
        let again = Cfg::parse(&text).unwrap();
        assert_eq!(again.get("decoration.bar", "sel"), Some(Val::List(vec!["#1f2430".into(), "#88c0d0".into(), "#88c0d0".into()])));
    }

    #[test]
    fn theme_owned_font_is_removed_when_the_next_theme_has_none() {
        let mut cfg = Cfg::default();
        apply_to(&mut cfg, &theme("windows11", true)).unwrap();
        assert_eq!(cfg.get("decoration", "font"), Some(Val::Str("Segoe UI".into())));
        assert_eq!(cfg.get("decoration", "style"), Some(Val::Str("classic".into())));
        apply_to(&mut cfg, &theme("macos", true)).unwrap();
        assert_eq!(cfg.get("decoration", "font"), None);
        assert_eq!(cfg.get("decoration", "style"), Some(Val::Str("macos".into())));
    }

    #[test]
    fn every_written_key_exists_in_rdwms_key_table() {
        let schema = crate::settings::schema::Schema::snapshot();
        for (sec, key, _) in edits(&theme("macos", true)) {
            // `[decoration.bar]` is one `bar` table in the key table
            let (sec, key) = if sec == "decoration.bar" { ("decoration", "bar") } else { (sec, key) };
            assert!(schema.has(sec, key), "{sec}.{key} is not in rdwm's schema");
        }
    }

    #[test]
    fn write_config_is_atomic_noop_when_unchanged_and_backs_up_once() {
        let d = std::env::temp_dir().join(format!("macshell-theme-rdwm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.clone(), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        std::fs::write(&paths.rdwm_config, "# hi\n[general]\ntags = [\"x\"]\n").unwrap();
        let t = theme("dracula", true);
        assert_eq!(write_config(&paths, &t).unwrap(), Wrote::Changed);
        assert_eq!(write_config(&paths, &t).unwrap(), Wrote::Unchanged);
        let bak = std::fs::read_to_string(d.join("config.toml.bak-macsettings")).unwrap();
        assert_eq!(bak, "# hi\n[general]\ntags = [\"x\"]\n");
        write_config(&paths, &theme("nord", true)).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("config.toml.bak-macsettings")).unwrap(), bak, "backup is made once");
        assert!(std::fs::read_to_string(&paths.rdwm_config).unwrap().starts_with("# hi\n"));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 2, "no temp files left");
    }

    #[test]
    fn apply_writes_nothing_when_the_config_is_broken() {
        let d = std::env::temp_dir().join(format!("macshell-theme-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.clone(), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        std::fs::write(&paths.rdwm_config, "this is [not toml").unwrap();
        let src = crate::theme::live::Source { shell_json: d.join("macshell.json"), themes_dir: d.join("themes") };
        let sel = Selection { name: "nord".into(), ..Selection::default() };
        assert!(apply(&src, &paths, &sel).is_err());
        assert!(!d.join("macshell.json").exists(), "selection untouched when rdwm's file cannot be edited");
        let ok = Selection { sync_rdwm: false, ..sel };
        let r = apply(&src, &paths, &ok).unwrap();
        assert_eq!((r.rdwm, r.reloaded), (None, false));
        assert_eq!(src.selection().name, "nord");
    }
}
