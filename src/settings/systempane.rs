//! "Apply to all apps" in System Settings > Appearance > All apps: the master switch and, under Advanced, one checkbox per target.
//!
//! The work is done by `decthemectl` (see [`crate::theme::system`]); the pane only writes the `"system"` key of `decebal.json` and asks for
//! an apply (debounced, in a separate process, so a slow `omarchy-theme-set` never blocks the window).

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Row};
use crate::theme::system::{self, Config, Env};

/// The settings new changes start from: the shown ones while a change is pending, else the file.
fn current(m: &Model) -> Config {
    Config::from_json(&m.json_value_now("shell", "system").unwrap_or_default())
}

pub fn items(m: &Model) -> Vec<Item> {
    let cfg = current(m);
    let icons = m.json_value_now("shell", "icons").unwrap_or_default();
    let icon_name = icons
        .get("theme")
        .and_then(serde_json::Value::as_str)
        .map(|name| if name.eq_ignore_ascii_case("auto") { "" } else { name })
        .unwrap_or_else(|| cfg.icon_theme.as_deref().unwrap_or_default())
        .to_string();
    let env = Env::discover();
    let st = system::status(&env, &cfg);
    let applied = system::applied_theme(&env);
    let completion = system::completion(&env);
    let mut out = vec![
        Item::Header("Apply to all apps".into()),
        Item::Note { text: "Themes automatically carry their colours and fonts to supported apps. Switch this off to restore your apps’ own settings.".into(), warn: false },
    ];
    out.push(Item::Card(vec![Row::new("Apply theme to all apps")
        .detail(if cfg.enabled {
            if completion.pending {
                "Applying app colours…".into()
            } else if completion.error.is_some() {
                "App theme integration needs attention; see the error below".into()
            } else {
                format!("On: theme changes also update supported apps (last completed: {})", applied.as_deref().unwrap_or("not yet"))
            }
        } else {
            "Off: app theme integration is disabled. Turn on for Files, GTK, Qt, terminals and supported app integrations".into()
        })
        .ctl(Ctl::new("sy:on", Kind::Toggle(cfg.enabled)))]));
    out.push(Item::Note { text: "Files and other GTK apps read custom palettes when they launch; open windows follow light/dark mode where supported. st updates live; other terminals use their supported reload or next launch. Apps are never closed or restarted.".into(), warn: false });
    if let Some(error) = completion.error {
        out.push(Item::Note { text: format!("App theme apply failed: {error}"), warn: true });
    }
    for warning in completion.warnings {
        out.push(Item::Note { text: warning, warn: true });
    }
    use system::file_manager::Style;
    let styles = [Style::FollowTheme, Style::Native, Style::Explorer, Style::Finder];
    let selected = styles.iter().position(|style| *style == cfg.file_manager_style);
    let source = super::themepane::source(m);
    let theme_id = source.find(&source.selection().name).map(|(id, _)| id).unwrap_or_else(|| "macos".into());
    let resolved = cfg.file_manager_style.resolve(&theme_id);
    out.push(Item::Card(vec![Row::new("File manager appearance")
        .detail(format!("Currently: {}. Follow theme uses Explorer for Windows themes, Finder for macOS themes and Native for others. Explicit choices stay fixed. Requires app and GTK integration; reopen Files to load changes.",resolved.title()))
        .ctl(Ctl::new("sy:files", Kind::Popup(styles.iter().map(|style| (style.title().into(), style.id().into())).collect(), selected)))]));
    let adv = m.sel.adv;
    out.push(Item::Header("Advanced".into()));
    out.push(Item::Card(vec![Row::new("Choose what is themed")
        .detail("GTK, Qt, terminals, the browser, editors and the icon and cursor themes one by one")
        .ctl(Ctl::new("ap:adv", Kind::Button { label: if adv { "Hide options".into() } else { "Show options".into() }, danger: false }))]));
    if !adv {
        return out;
    }
    let rows: Vec<Row> = st
        .iter()
        .map(|s| {
            let detail = match &s.unavailable {
                Some(why) => format!("Not available here: {why}"),
                None => s.detail.to_string(),
            };
            let mut c = Ctl::new(format!("sy:t:{}", s.id), Kind::Toggle(s.on && s.unavailable.is_none()));
            c.enabled = s.unavailable.is_none();
            Row::new(s.title).detail(detail).ctl(c)
        })
        .collect();
    out.push(Item::Header("Apply to".into()));
    out.push(Item::Card(rows));
    let opts: Vec<Row> = system::OPTIONS.iter().map(|(key, _, title, detail)| Row::new(*title).detail(*detail).ctl(Ctl::new(format!("sy:o:{key}"), Kind::Toggle(cfg.option(key))))).collect();
    out.push(Item::Card(opts));
    out.push(Item::Card(vec![
        Row::new("Icon theme for apps")
            .detail("Use this icon theme for the shell, GTK and Qt apps. Empty uses the toolkit default; choose Follow theme in Icons to restore theme matching.")
            .ctl(Ctl::new("sy:icon", Kind::Text { val: icon_name, hint: "Papirus".into() })),
        Row::new("Cursor theme for apps")
            .detail("Optional: set this cursor theme for GTK and X11 clients")
            .ctl(Ctl::new("sy:cursor", Kind::Text { val: cfg.cursor_theme.clone().unwrap_or_default(), hint: "Bibata-Modern-Classic".into() })),
        Row::new("Apply now").detail("Run it again right away").ctl(Ctl::new("sy:now", Kind::Button { label: "Apply".into(), danger: false })),
    ]));
    out.push(Item::Note {
        text: "Every file is saved before its first change and `decthemectl system off` (or the switch above) puts it back. The same from a terminal: decthemectl system status | on | off, decthemectl apply-system --dry-run. Qt apps need QT_QPA_PLATFORMTHEME=qt6ct in the session environment.".into(),
        warn: false,
    });
    out
}

/// Saves `"system"` through the persistence worker (keys this version does not know are kept); `then` runs once it
/// is on disk (asking for an apply reads the file).
fn save_then(m: &mut Model, cfg: &Config, then: impl FnOnce(&mut Model) + 'static) {
    let base = m.json_value_now("shell", "system");
    let mut obj = base.as_ref().and_then(|v| v.as_object()).cloned().unwrap_or_default();
    // the keys Config owns are replaced (a cleared one is absent from to_json); every other key is kept
    for k in ["enabled", "fileManagerStyle", "targets", "iconTheme", "cursorTheme", "signals", "options"] {
        obj.shift_remove(k);
    }
    if let serde_json::Value::Object(known) = cfg.to_json() {
        obj.extend(known);
    }
    m.edit_json_then("shell", "system", Some(serde_json::Value::Object(obj)), base, None, Some(Box::new(then)));
}

/// Asks for an apply on the persistence worker, after the save (`request` reads the file and writes the pending
/// marker; with DECEBAL_SYSTEM_SYNC=1 it applies inline): never on the UI thread.
fn request(m: &mut Model) {
    let shell = m.paths.shell_json();
    m.run_task_quiet("system-apply-request", move || {
        system::request(&shell);
        Ok(serde_json::Value::Null)
    });
}

pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    let mut cfg = current(m);
    match (parts, v) {
        (["sy", "on"], CtlVal::Bool(on)) => {
            // Save first, then queue one observable apply; opt-out restores the original app settings.
            let then = move |m: &mut Model| {
                if on {
                    request(m);
                } else {
                    m.run_task_quiet("system-off", || {
                        system::spawn_ctl(&["system", "off"]);
                        Ok(serde_json::Value::Null)
                    });
                }
                m.say(if on { "Applying the theme to all apps" } else { "Restoring the apps' own settings" }, false);
            };
            if cfg.enabled != on {
                cfg.enabled = on;
                save_then(m, &cfg, then);
            } else {
                then(m);
            }
        }
        (["sy", "t", id], CtlVal::Bool(b)) => {
            if !system::targets().iter().any(|t| t.id() == *id) {
                return;
            }
            cfg.targets.insert((*id).to_string(), b);
            save_then(m, &cfg, request);
        }
        (["sy", "o", key], CtlVal::Bool(b)) => {
            if !system::OPTIONS.iter().any(|o| o.0 == *key) {
                return;
            }
            cfg.set_option(key, b);
            save_then(m, &cfg, request);
        }
        (["sy", "files"], CtlVal::Str(s)) => {
            let Some(style) = system::file_manager::Style::parse(&s) else { return };
            cfg.file_manager_style = style;
            let enabled = cfg.enabled;
            save_then(m, &cfg, move |m| {
                if enabled {
                    request(m);
                }
            });
        }
        (["sy", "icon"], CtlVal::Str(s)) => {
            let name = s.trim().to_string();
            let pick = if name.is_empty() { crate::desktop_assets::policy::Choice::Auto } else { crate::desktop_assets::policy::Choice::Name(name.clone()) };
            let said = format!("Icon theme for apps: {}", if name.is_empty() { "not set" } else { &name });
            m.edit_shell_doc("system-icons", move |doc| crate::desktop_assets::policy::choose(doc, crate::desktop_assets::Provides::Icons, &pick), said, Some(Box::new(request)));
        }
        (["sy", "cursor"], CtlVal::Str(s)) => {
            cfg.cursor_theme = Some(s.trim().to_string()).filter(|s| !s.is_empty());
            // The cursor and who chose it (`asset_packs`) change in one atomic write; the apply request follows it.
            let pick = match &cfg.cursor_theme {
                Some(t) => crate::desktop_assets::policy::Choice::Name(t.clone()),
                None => crate::desktop_assets::policy::Choice::Auto,
            };
            let said = format!("Cursor theme for apps: {}", cfg.cursor_theme.as_deref().unwrap_or("not set"));
            m.edit_shell_doc("system-cursor", move |doc| crate::desktop_assets::policy::choose(doc, crate::desktop_assets::Provides::Cursors, &pick), said, Some(Box::new(request)));
        }
        (["sy", "now"], CtlVal::Click) => {
            if cfg.enabled {
                request(m);
                m.say("Applying the theme to all apps", false);
            } else {
                m.say("Turn on Apply to all apps first", true);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;

    fn toggle(items: &[Item], id: &str) -> Option<(bool, bool)> {
        items.iter().find_map(|i| match i {
            Item::Card(rows) => rows.iter().flat_map(|r| &r.ctls).find(|c| c.id == id).map(|c| (matches!(c.kind, Kind::Toggle(true)), c.enabled)),
            _ => None,
        })
    }

    #[test]
    fn switch_and_target_boxes_are_written_to_decebal_json() {
        let (mut m, _) = model_with("");
        m.sel.adv = true;
        assert_eq!(toggle(&items(&m), "sy:on"), Some((true, true)), "automatic by default");
        for id in ["omarchy", "browser", "editors", "hooks", "gtk", "qt", "kvantum", "x11", "terminals"] {
            assert!(toggle(&items(&m), &format!("sy:t:{id}")).is_some(), "{id} has a checkbox");
        }
        m.apply("sy:on", CtlVal::Bool(true));
        assert!(Config::load(&m.paths.shell_json()).enabled);
        assert_eq!(toggle(&items(&m), "sy:on"), Some((true, true)));
        m.apply("sy:t:gtk", CtlVal::Bool(false));
        m.apply("sy:icon", CtlVal::Str(" Papirus ".into()));
        let c = Config::load(&m.paths.shell_json());
        assert_eq!(c.targets.get("gtk").copied(), Some(false));
        assert_eq!(c.icon_theme, None, "legacy second authority is retired");
        assert_eq!(m.json_value_now("shell", "icons").unwrap()["theme"], "Papirus");
        assert!(items(&m)
            .iter()
            .any(|item| matches!(item, Item::Card(rows) if rows.iter().flat_map(|row| &row.ctls).any(|ctl| ctl.id == "sy:icon" && matches!(&ctl.kind, Kind::Text { val, .. } if val == "Papirus")))));
        assert_eq!(toggle(&items(&m), "sy:t:gtk").map(|t| t.0), Some(false));
        m.apply("sy:t:nonsense", CtlVal::Bool(true));
        assert!(!Config::load(&m.paths.shell_json()).targets.contains_key("nonsense"));
        m.apply("sy:on", CtlVal::Bool(false));
        assert!(!Config::load(&m.paths.shell_json()).enabled);
    }

    #[test]
    fn the_gtk_font_option_is_on_until_turned_off() {
        let (mut m, _) = model_with("");
        m.sel.adv = true;
        assert_eq!(toggle(&items(&m), "sy:o:gtk.font").map(|t| t.0), Some(true), "on by default");
        m.apply("sy:o:gtk.font", CtlVal::Bool(false));
        assert!(!Config::load(&m.paths.shell_json()).option("gtk.font"));
        assert_eq!(toggle(&items(&m), "sy:o:gtk.font").map(|t| t.0), Some(false));
        m.apply("sy:o:nonsense", CtlVal::Bool(false));
        assert!(!Config::load(&m.paths.shell_json()).options.contains_key("nonsense"));
        m.apply("sy:o:gtk.font", CtlVal::Bool(true));
        assert!(Config::load(&m.paths.shell_json()).options.is_empty());
    }

    #[test]
    fn file_manager_choice_is_visible_without_advanced_and_routes_valid_values() {
        let (mut m, _) = model_with("");
        m.sel.adv = false;
        assert!(items(&m)
            .iter()
            .any(|item| matches!(item, Item::Card(rows) if rows.iter().flat_map(|r| &r.ctls).any(|c| c.id == "sy:files" && matches!(&c.kind, Kind::Popup(opts, Some(0)) if opts.len() == 4)))));
        // Disable propagation to keep this a settings-only routing test.
        let cfg = Config { enabled: false, ..Config::default() };
        cfg.save(&m.paths.shell_json()).unwrap();
        m.apply("sy:files", CtlVal::Str("finder".into()));
        assert_eq!(Config::load(&m.paths.shell_json()).file_manager_style.id(), "finder");
        m.apply("sy:files", CtlVal::Str("invalid".into()));
        assert_eq!(Config::load(&m.paths.shell_json()).file_manager_style.id(), "finder");
        m.apply("sy:files", CtlVal::Str("theme".into()));
        assert_eq!(Config::load(&m.paths.shell_json()).file_manager_style.id(), "theme");
    }

    #[test]
    fn file_manager_row_shows_the_resolved_profile_and_preserves_fixed_choice() {
        let (m, _) = model_with("");
        for (theme, style, expected) in
            [("windows11", "theme", "Windows Explorer inspired"), ("graphite", "theme", "macOS Finder inspired"), ("nord", "theme", "Native Files"), ("windows11", "native", "Native Files")]
        {
            std::fs::write(m.paths.shell_json(), serde_json::json!({"theme":{"name":theme},"system":{"enabled":false,"fileManagerStyle":style}}).to_string()).unwrap();
            let rows = items(&m);
            let row = rows.iter().find_map(|i| if let Item::Card(rows) = i { rows.iter().find(|row| row.ctls.iter().any(|ctl| ctl.id == "sy:files")) } else { None }).unwrap();
            assert!(row.detail.contains(&format!("Currently: {expected}.")), "{}", row.detail);
        }
    }

    #[test]
    fn the_theme_pane_shows_the_section() {
        let (m, _) = model_with("");
        let its = crate::settings::themepane::items(&m);
        assert!(its.iter().any(|i| matches!(i, Item::Header(h) if h == "Apply to all apps")));
    }
}
