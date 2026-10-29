//! "Apply to all apps" in System Settings > Appearance > All apps: the master switch and, under Advanced, one checkbox per target.
//!
//! The work is done by `decthemectl` (see [`crate::theme::system`]); the pane only writes the `"system"` key of `decebal.json` and asks for
//! an apply (debounced, in a separate process, so a slow `omarchy-theme-set` never blocks the window).

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Row};
use crate::theme::system::{self, Config, Env};

pub fn items(m: &Model) -> Vec<Item> {
    let cfg = Config::load(&m.paths.shell_json());
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
            .detail("Optional: set this icon theme in GTK and Qt apps (empty = leave it alone)")
            .ctl(Ctl::new("sy:icon", Kind::Text { val: cfg.icon_theme.clone().unwrap_or_default(), hint: "Papirus".into() })),
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

fn save(m: &mut Model, cfg: &Config) -> bool {
    match cfg.save(&m.paths.shell_json()) {
        Ok(()) => {
            m.shell = super::io::json_load(&m.paths.shell_json());
            m.dirty = true;
            true
        }
        Err(e) => {
            m.say(format!("Not saved: {e}"), true);
            false
        }
    }
}

pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    let mut cfg = Config::load(&m.paths.shell_json());
    match (parts, v) {
        (["sy", "on"], CtlVal::Bool(on)) => {
            // Save first, then queue one observable apply; opt-out restores the original app settings.
            if cfg.enabled != on {
                cfg.enabled = on;
                if !save(m, &cfg) {
                    return;
                }
            }
            if on {
                system::request(&m.paths.shell_json());
            } else {
                system::spawn_ctl(&["system", "off"]);
            }
            m.say(if on { "Applying the theme to all apps" } else { "Restoring the apps' own settings" }, false);
        }
        (["sy", "t", id], CtlVal::Bool(b)) => {
            if !system::targets().iter().any(|t| t.id() == *id) {
                return;
            }
            cfg.targets.insert((*id).to_string(), b);
            if save(m, &cfg) {
                system::request(&m.paths.shell_json());
            }
        }
        (["sy", "o", key], CtlVal::Bool(b)) => {
            if !system::OPTIONS.iter().any(|o| o.0 == *key) {
                return;
            }
            cfg.set_option(key, b);
            if save(m, &cfg) {
                system::request(&m.paths.shell_json());
            }
        }
        (["sy", "icon"], CtlVal::Str(s)) => {
            cfg.icon_theme = Some(s.trim().to_string()).filter(|s| !s.is_empty());
            if save(m, &cfg) {
                system::request(&m.paths.shell_json());
            }
        }
        (["sy", "cursor"], CtlVal::Str(s)) => {
            cfg.cursor_theme = Some(s.trim().to_string()).filter(|s| !s.is_empty());
            if save(m, &cfg) {
                system::request(&m.paths.shell_json());
            }
        }
        (["sy", "now"], CtlVal::Click) => {
            if cfg.enabled {
                system::request(&m.paths.shell_json());
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
        assert_eq!((c.targets.get("gtk").copied(), c.icon_theme.as_deref()), (Some(false), Some("Papirus")));
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
    fn the_theme_pane_shows_the_section() {
        let (m, _) = model_with("");
        let its = crate::settings::themepane::items(&m);
        assert!(its.iter().any(|i| matches!(i, Item::Header(h) if h == "Apply to all apps")));
    }
}
