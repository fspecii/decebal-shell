//! `decthemectl icons ...`: the command line of the icon settings (also what the tests drive).

use super::config::IconConfig;
use super::live::State;
use super::resolve::{self, Roots, ThemeInfo};
use serde_json::json;
use std::path::Path;

pub const HELP: &str = "decthemectl icons list [--json]           installed icon themes (* = in use; symbolic-only / cursor-only ones cannot be chosen)
decthemectl icons set NAME|auto          use a theme (directory or display name); auto follows GSettings / GTK
decthemectl icons status [--json]        configured theme, what is in use, lookup chain, scale, tint
decthemectl icons reset                  remove the `icons` settings (automatic, scale 1, no tint, fallback Adwaita)
decthemectl icons scale 0.6-1.25         size of the artwork inside its square (1 = fill it)
decthemectl icons tint on|off            repaint symbolic icons in the theme's text colour
decthemectl icons fallback NAME|none     theme tried after the chosen one, its parents and hicolor
Stored in decebal.json under \"icons\"; running programs follow within half a second.";

/// Hands the persisted icon selection of `file` (decebal.json) to app integration: the narrow icon / cursor publication
/// (`theme::system::publish_assets`: GTK `settings.ini` icon / cursor keys, gsettings, qt5ct / qt6ct `icon_theme`, and the
/// X11 settings manager), never a theme, colour or font apply. Call it only after the selection was written. It never blocks:
/// the request goes to the system queue's dispatcher, and explicit opt-outs (`system.enabled = false`, the GTK and Qt
/// targets off) answer "not published". Returns one line for the user.
pub fn publish_selection(file: &Path) -> String {
    #[cfg(test)]
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).push(file.to_path_buf());
    match crate::theme::system::publish_assets(file) {
        crate::theme::system::Publication::Requested => "apps: GTK / Qt icon publication queued".into(),
        crate::theme::system::Publication::Off(why) => format!("apps: not published ({why})"),
    }
}

/// Test record of the `decebal.json` files handed to [`publish_selection`] (tests check their own sandbox path).
#[cfg(test)]
pub static PUBLISHED: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub fn was_published(file: &Path) -> usize {
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|p| p.as_path() == file).count()
}

/// Runs one subcommand against `file` (decebal.json) and `roots`; the text to print, or an error message.
pub fn run(file: &Path, roots: &Roots, args: &[String]) -> Result<String, String> {
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--json").collect();
    let themes = resolve::list_themes(roots);
    let mut cfg = IconConfig::read(file);
    match args.first().copied() {
        Some("list") => Ok(list(file, roots, &themes, json)),
        Some("status") => Ok(status(file, roots, json)),
        Some("set") => {
            let name = args.get(1).ok_or("set needs a theme name (decthemectl icons list)")?;
            if name.eq_ignore_ascii_case("auto") {
                cfg.theme = "auto".into();
            } else {
                let id = resolve::find_installed(&themes, name).ok_or_else(|| format!("no icon theme `{name}` (decthemectl icons list)"))?;
                let t = themes.iter().find(|t| t.id == id).expect("listed");
                if !t.kind.selectable() {
                    return Err(format!("`{}` cannot be used for application icons ({})", t.id, t.kind.label()));
                }
                cfg.theme = id;
            }
            let choice = if cfg.is_auto() { crate::desktop_assets::policy::Choice::Auto } else { crate::desktop_assets::policy::Choice::Name(cfg.theme.clone()) };
            // One commit: a background theme completion must never observe the new choice with the old ownership.
            crate::desktop_assets::policy::choose_in_file(file, crate::desktop_assets::Provides::Icons, &choice)?;
            // only once the choice and its ownership record are on disk
            Ok(format!("icon theme: {}\n{}", if cfg.is_auto() { "auto".to_string() } else { cfg.theme.clone() }, publish_selection(file)))
        }
        Some("reset") => {
            IconConfig::reset(file)?;
            Ok(format!("icon settings reset\n{}", publish_selection(file)))
        }
        Some("scale") => {
            let v: f32 = args.get(1).and_then(|v| v.parse().ok()).ok_or("scale needs a number")?;
            if !(super::config::SCALE_MIN..=super::config::SCALE_MAX).contains(&v) {
                return Err(format!("scale must be between {} and {}", super::config::SCALE_MIN, super::config::SCALE_MAX));
            }
            cfg.size_scale = v;
            cfg.write(file)?;
            Ok(format!("icon size scale: {v}"))
        }
        Some("tint") => {
            cfg.monochrome_tint = match args.get(1).copied() {
                Some("on" | "true" | "1") => true,
                Some("off" | "false" | "0") => false,
                _ => return Err("tint needs on or off".into()),
            };
            cfg.write(file)?;
            Ok(format!("symbolic icon tint: {}", if cfg.monochrome_tint { "on" } else { "off" }))
        }
        Some("fallback") => {
            let name = args.get(1).ok_or("fallback needs a theme name or none")?;
            cfg.fallback =
                if name.eq_ignore_ascii_case("none") { "none".into() } else { resolve::find_installed(&themes, name).ok_or_else(|| format!("no icon theme `{name}` (decthemectl icons list)"))? };
            cfg.write(file)?;
            Ok(format!("fallback theme: {}", cfg.fallback))
        }
        Some("-h" | "--help" | "help") | None => Ok(HELP.into()),
        Some(other) => Err(format!("unknown icons command `{other}`\n{HELP}")),
    }
}

fn current(file: &Path, roots: &Roots) -> Option<String> {
    State::new(file.to_path_buf(), roots.clone()).choice.active
}

fn list(file: &Path, roots: &Roots, themes: &[ThemeInfo], json: bool) -> String {
    let cur = current(file, roots);
    let cfg = IconConfig::read(file);
    if json {
        let v: Vec<_> = themes
            .iter()
            .map(|t| {
                json!({"id": t.id, "name": t.name, "comment": t.comment, "kind": t.kind.label(), "selectable": t.kind.selectable(), "inherits": t.inherits, "path": t.path,
                       "current": cur.as_deref() == Some(t.id.as_str()), "configured": !cfg.is_auto() && resolve::find_installed(themes, &cfg.theme).as_deref() == Some(t.id.as_str())})
            })
            .collect();
        return serde_json::to_string_pretty(&v).unwrap_or_default();
    }
    let mut out = String::new();
    for t in themes {
        let mark = if cur.as_deref() == Some(t.id.as_str()) { '*' } else { ' ' };
        let note = if t.kind.selectable() { String::new() } else { format!("  [{}: not selectable]", t.kind.label()) };
        out.push_str(&format!("{mark} {:<28} {:<28}{note}\n", t.id, t.name));
    }
    out.trim_end().to_string()
}

fn status(file: &Path, roots: &Roots, json: bool) -> String {
    let s = State::new(file.to_path_buf(), roots.clone());
    if json {
        return serde_json::to_string_pretty(&json!({
            "configured": if s.cfg.is_auto() { "auto" } else { s.cfg.theme.as_str() },
            "desktop": s.desktop, "active": s.choice.active, "problem": s.choice.problem, "chain": s.chain(),
            "size_scale": s.cfg.size_scale, "monochrome_tint": s.cfg.monochrome_tint, "fallback": s.cfg.fallback,
        }))
        .unwrap_or_default();
    }
    let mut out = format!(
        "theme:      {} (configured: {}{})\nchain:      {}\nsize scale: {}\ntint:       {}\nfallback:   {}",
        s.choice.active.as_deref().unwrap_or("none (hicolor only)"),
        if s.cfg.is_auto() { "auto" } else { &s.cfg.theme },
        if s.cfg.is_auto() { format!(", desktop says {}", s.desktop.as_deref().unwrap_or("nothing")) } else { String::new() },
        s.chain().join(" > "),
        s.cfg.size_scale,
        if s.cfg.monochrome_tint { "on" } else { "off" },
        s.cfg.fallback
    );
    if let Some(p) = &s.choice.problem {
        out.push_str(&format!("\nproblem:    {p}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::resolve::testkit::*;
    use super::*;

    fn setup() -> (Tree, std::path::PathBuf) {
        let t = Tree::new("ctl");
        t.theme("hicolor", PLAIN).theme("Adwaita", PLAIN).theme("Tela", &PLAIN.replace("Plain", "Tela blue"));
        t.theme("Mono", "[Icon Theme]\nName=Mono\nDirectories=symbolic/apps\n[symbolic/apps]\nSize=16\nType=Scalable\n");
        let f = t.root.join("decebal.json");
        (t, f)
    }

    fn go(t: &Tree, f: &Path, a: &[&str]) -> Result<String, String> {
        run(f, &t.roots(), &a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn set_status_reset_cycle() {
        let (t, f) = setup();
        assert_eq!(go(&t, &f, &["set", "tela blue"]).unwrap().lines().next(), Some("icon theme: Tela"));
        assert_eq!(IconConfig::read(&f).theme, "Tela");
        let st = go(&t, &f, &["status"]).unwrap();
        assert!(st.contains("theme:      Tela") && st.contains("Tela > hicolor > Adwaita"), "{st}");
        let j: serde_json::Value = serde_json::from_str(&go(&t, &f, &["status", "--json"]).unwrap()).unwrap();
        assert_eq!((j["active"].as_str(), j["configured"].as_str()), (Some("Tela"), Some("Tela")));
        go(&t, &f, &["scale", "0.9"]).unwrap();
        go(&t, &f, &["tint", "on"]).unwrap();
        go(&t, &f, &["fallback", "none"]).unwrap();
        let c = IconConfig::read(&f);
        assert_eq!((c.size_scale, c.monochrome_tint, c.fallback.as_str(), c.theme.as_str()), (0.9, true, "none", "Tela"));
        assert!(go(&t, &f, &["status"]).unwrap().contains("Tela > hicolor\n"));
        go(&t, &f, &["set", "auto"]).unwrap();
        assert!(IconConfig::read(&f).is_auto());
        assert_eq!(IconConfig::read(&f).size_scale, 0.9, "the other settings stay");
        go(&t, &f, &["reset"]).unwrap();
        assert_eq!(IconConfig::read(&f), IconConfig::default());
    }

    /// A persisted manual choice (a named theme, auto) and a reset are handed to the narrow app publication; size, tint and
    /// fallback (shell-only) and refused commands are not. Unit tests never queue real work: the answer says why.
    #[test]
    fn manual_choices_and_reset_are_published_after_they_are_written() {
        let (t, f) = setup();
        let out = go(&t, &f, &["set", "Tela"]).unwrap();
        assert_eq!(was_published(&f), 1);
        assert!(out.ends_with("apps: not published (automatic app integration is suppressed in this process)"), "{out}");
        assert_eq!(IconConfig::read(&f).theme, "Tela", "written before publication");
        go(&t, &f, &["scale", "0.9"]).unwrap();
        go(&t, &f, &["tint", "on"]).unwrap();
        go(&t, &f, &["fallback", "none"]).unwrap();
        assert_eq!(was_published(&f), 1, "shell-only settings are not app publications");
        assert!(go(&t, &f, &["set", "Nope"]).is_err() && go(&t, &f, &["set", "Mono"]).is_err());
        assert_eq!(was_published(&f), 1, "a refused choice publishes nothing");
        go(&t, &f, &["set", "auto"]).unwrap();
        go(&t, &f, &["reset"]).unwrap();
        assert_eq!(was_published(&f), 3);
    }

    #[test]
    fn manual_choice_and_ownership_are_published_in_one_commit_preserving_other_icon_keys() {
        let (t, f) = setup();
        let initial = json!({"icons": {"theme": "Adwaita", "size_scale": 0.9, "future": {"keep": 7}}, "asset_packs": {"generation": 5}, "foreign": true});
        std::fs::write(&f, initial.to_string()).unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let snapshots = seen.clone();
        let watched = f.clone();
        struct ResetHook;
        impl Drop for ResetHook {
            fn drop(&mut self) {
                crate::settings::io::fault::set_after_commit_hook(None);
            }
        }
        let _reset = ResetHook;
        crate::settings::io::fault::set_after_commit_hook(Some(std::sync::Arc::new(move |path| {
            if path == watched {
                snapshots.lock().unwrap().push(serde_json::from_slice(&std::fs::read(path)?)?);
            }
            Ok(())
        })));
        go(&t, &f, &["set", "Tela"]).unwrap();
        let docs = seen.lock().unwrap();
        assert_eq!(docs.len(), 1, "no intermediate choice with stale automatic ownership");
        let doc = &docs[0];
        assert_eq!(doc["icons"]["theme"], "Tela");
        assert_eq!(doc["icons"]["size_scale"], 0.9);
        assert_eq!(doc["icons"]["future"], initial["icons"]["future"]);
        assert_eq!(doc["foreign"], true);
        assert!(crate::desktop_assets::policy::state(doc, crate::desktop_assets::Provides::Icons).is_custom());
        assert_eq!(crate::desktop_assets::policy::generation(doc), 6);
    }

    #[test]
    fn refusals_change_nothing() {
        let (t, f) = setup();
        std::fs::write(&f, "{}").unwrap();
        assert!(go(&t, &f, &["set", "Nope"]).unwrap_err().contains("no icon theme"));
        assert!(go(&t, &f, &["set", "Mono"]).unwrap_err().contains("symbolic"));
        assert!(go(&t, &f, &["set"]).is_err());
        assert!(go(&t, &f, &["scale", "5"]).is_err() && go(&t, &f, &["scale", "x"]).is_err());
        assert!(go(&t, &f, &["tint", "maybe"]).is_err());
        assert!(go(&t, &f, &["fallback", "Nope"]).is_err());
        assert!(go(&t, &f, &["bogus"]).unwrap_err().contains("unknown"));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}");
    }

    #[test]
    fn list_marks_the_current_theme_and_flags_unusable_ones() {
        let (t, f) = setup();
        go(&t, &f, &["set", "Tela"]).unwrap();
        let l = go(&t, &f, &["list"]).unwrap();
        let tela = l.lines().find(|x| x.contains("Tela")).unwrap();
        assert!(tela.starts_with('*'), "{l}");
        assert!(l.lines().find(|x| x.contains("Mono")).unwrap().contains("not selectable"));
        let j: serde_json::Value = serde_json::from_str(&go(&t, &f, &["list", "--json"]).unwrap()).unwrap();
        let tela = j.as_array().unwrap().iter().find(|x| x["id"] == "Tela").unwrap();
        assert_eq!((tela["current"].as_bool(), tela["selectable"].as_bool()), (Some(true), Some(true)));
        assert!(go(&t, &f, &["help"]).unwrap().contains("icons set"));
    }
}
