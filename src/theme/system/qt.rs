//! Qt 5 / Qt 6: a qt5ct / qt6ct colour scheme (`colors/macshell.conf`) selected in `qt5ct.conf` / `qt6ct.conf`, and an optional Kvantum
//! colour theme. Qt apps read the scheme when `QT_QPA_PLATFORMTHEME=qt5ct` (Qt 5) or `qt6ct` (Qt 6) is set in the session environment.

use super::files::{ini_get, Edit};
use super::{Config, Env, Palette, Plan, Target};
use crate::theme::model::Rgb;
use std::path::PathBuf;

pub struct Qt;
pub struct Kvantum;

fn argb(c: Rgb) -> String {
    format!("#ff{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// The 21 `QPalette::ColorRole`s of one colour group, in Qt's order.
fn roles(p: &Palette, group: &str) -> Vec<Rgb> {
    let (white, black) = (Rgb(255, 255, 255), Rgb(0, 0, 0));
    let button = p.surface;
    let (light, midlight, dark, mid) = (white.over(button, 0.18), white.over(button, 0.08), black.over(button, 0.35), black.over(button, 0.18));
    let (text, hl, hl_text) = match group {
        "disabled" => (p.text_faint, p.border.over(p.bg, 0.8), p.text_faint),
        "inactive" => (p.text_secondary, p.accent.over(p.bg, 0.55), p.accent_text),
        _ => (p.text, p.accent, p.accent_text),
    };
    vec![
        text,             // WindowText
        button,           // Button
        light,            // Light
        midlight,         // Midlight
        dark,             // Dark
        mid,              // Mid
        text,             // Text
        p.text,           // BrightText
        text,             // ButtonText
        p.bg,             // Base
        p.bg,             // Window
        black,            // Shadow
        hl,               // Highlight
        hl_text,          // HighlightedText
        p.accent_on_bg(), // Link
        p.magenta,        // LinkVisited
        p.surface,        // AlternateBase
        p.bg,             // NoRole
        p.popup,          // ToolTipBase
        p.text,           // ToolTipText
        p.text_faint,     // PlaceholderText
    ]
}

/// The qt5ct / qt6ct colour scheme file.
pub fn scheme(p: &Palette) -> String {
    let line = |group: &str| roles(p, group).into_iter().map(argb).collect::<Vec<_>>().join(", ");
    format!("[ColorScheme]\nactive_colors={}\ndisabled_colors={}\ninactive_colors={}\n", line("active"), line("disabled"), line("inactive"))
}

pub fn kvconfig(p: &Palette) -> String {
    let h = |c: Rgb| c.hex();
    let rows = [
        ("window.color", p.bg),
        ("base.color", p.bg),
        ("alt.base.color", p.surface),
        ("button.color", p.surface),
        ("light.color", Rgb(255, 255, 255).over(p.surface, 0.18)),
        ("mid.light.color", Rgb(255, 255, 255).over(p.surface, 0.08)),
        ("dark.color", Rgb(0, 0, 0).over(p.surface, 0.35)),
        ("mid.dark.color", Rgb(0, 0, 0).over(p.surface, 0.18)),
        ("highlight.color", p.accent),
        ("inactive.highlight.color", p.accent.over(p.bg, 0.55)),
        ("text.color", p.text),
        ("window.text.color", p.text),
        ("button.text.color", p.text),
        ("disabled.text.color", p.text_faint),
        ("tooltip.base.color", p.popup),
        ("tooltip.text.color", p.text),
        ("highlight.text.color", p.accent_text),
        ("link.color", p.accent_on_bg()),
        ("link.visited.color", p.magenta),
        ("progress.indicator.text.color", p.accent_text),
    ];
    let mut s = format!("[%General]\nauthor=macshell\ncomment={} ({}) colours from macshell\n\n[GeneralColors]\n", p.name, if p.dark { "dark" } else { "light" });
    for (k, c) in rows {
        s.push_str(&format!("{k}={}\n", h(c)));
    }
    s
}

/// qt5ct and qt6ct: (config folder name, command).
const FLAVOURS: [(&str, &str); 2] = [("qt5ct", "qt5ct"), ("qt6ct", "qt6ct")];

fn flavours(env: &Env) -> Vec<&'static str> {
    FLAVOURS.iter().filter(|(dir, cmd)| env.has(cmd) || env.config_home.join(dir).is_dir()).map(|(d, _)| *d).collect()
}

impl Target for Qt {
    fn id(&self) -> &'static str {
        "qt"
    }
    fn title(&self) -> &'static str {
        "Qt apps"
    }
    fn detail(&self) -> &'static str {
        "qt5ct / qt6ct colour scheme (needs QT_QPA_PLATFORMTHEME=qt6ct in the session)"
    }
    fn available(&self, env: &Env) -> Result<(), String> {
        if flavours(env).is_empty() {
            Err("qt5ct / qt6ct not installed".into())
        } else {
            Ok(())
        }
    }
    fn plan(&self, p: &Palette, cfg: &Config, env: &Env) -> Plan {
        let mut plan = Plan::default();
        for dir in flavours(env) {
            let base = env.config_home.join(dir);
            let scheme_path: PathBuf = base.join("colors/macshell.conf");
            plan.edits.push(Edit::Whole { path: scheme_path.clone(), text: scheme(p) });
            let conf = base.join(format!("{dir}.conf"));
            let existing = std::fs::read_to_string(&conf).unwrap_or_default();
            let mut sets = vec![("Appearance".to_string(), "color_scheme_path".to_string(), scheme_path.to_string_lossy().into_owned()), ("Appearance".into(), "custom_palette".into(), "true".into())];
            if ini_get(&existing, "Appearance", "style").is_none() {
                sets.push(("Appearance".into(), "style".into(), "Fusion".into()));
            }
            if let Some(i) = &cfg.icon_theme {
                sets.push(("Appearance".into(), "icon_theme".into(), i.clone()));
            }
            plan.edits.push(Edit::Ini { path: conf, sets });
        }
        plan
    }
}

impl Target for Kvantum {
    fn id(&self) -> &'static str {
        "kvantum"
    }
    fn title(&self) -> &'static str {
        "Kvantum"
    }
    fn detail(&self) -> &'static str {
        "A colours-only Kvantum theme, selected in kvantum.kvconfig"
    }
    fn available(&self, env: &Env) -> Result<(), String> {
        if env.has("kvantummanager") || env.config_home.join("Kvantum").is_dir() {
            Ok(())
        } else {
            Err("Kvantum not installed".into())
        }
    }
    fn default_on(&self, _env: &Env) -> bool {
        false // replaces the user's Kvantum theme; opt in
    }
    fn plan(&self, p: &Palette, _cfg: &Config, env: &Env) -> Plan {
        let base = env.config_home.join("Kvantum");
        Plan {
            edits: vec![
                Edit::Whole { path: base.join("macshell/macshell.kvconfig"), text: kvconfig(p) },
                Edit::Ini { path: base.join("kvantum.kvconfig"), sets: vec![("General".into(), "theme".into(), "macshell".into())] },
            ],
            gsettings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    #[test]
    fn scheme_has_three_groups_of_twenty_one_roles() {
        for dark in [true, false] {
            let s = scheme(&palette(dark));
            let lines: Vec<&str> = s.lines().collect();
            assert_eq!(lines[0], "[ColorScheme]");
            for (i, g) in ["active_colors=", "disabled_colors=", "inactive_colors="].iter().enumerate() {
                let l = lines[i + 1];
                assert!(l.starts_with(g));
                let cols: Vec<&str> = l[g.len()..].split(", ").collect();
                assert_eq!(cols.len(), 21, "{l}");
                assert!(cols.iter().all(|c| c.len() == 9 && c.starts_with("#ff")));
            }
        }
    }

    #[test]
    fn scheme_golden_for_the_macos_dark_palette() {
        let p = palette(true);
        let s = scheme(&p);
        let active = s.lines().nth(1).unwrap();
        let cols: Vec<&str> = active["active_colors=".len()..].split(", ").collect();
        assert_eq!(cols[0], argb(p.text), "WindowText");
        assert_eq!(cols[9], argb(p.bg), "Base");
        assert_eq!(cols[10], argb(p.bg), "Window");
        assert_eq!(cols[12], argb(p.accent), "Highlight");
        assert_eq!(cols[13], argb(p.accent_text), "HighlightedText");
        assert_eq!(cols[11], "#ff000000", "Shadow");
    }

    #[test]
    fn plan_writes_only_for_installed_flavours_and_keeps_the_users_style() {
        let d = tmp("qtplan");
        let (env, _) = env(&d, &["qt6ct"]);
        let plan = Qt.plan(&palette(true), &Config::default(), &env);
        let paths: Vec<String> = plan.edits.iter().map(|e| e.path().strip_prefix(&env.config_home).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(paths, ["qt6ct/colors/macshell.conf", "qt6ct/qt6ct.conf"]);
        let Edit::Ini { sets, .. } = &plan.edits[1] else { panic!() };
        assert!(sets.iter().any(|(_, k, v)| k == "style" && v == "Fusion"));
        assert!(sets.iter().any(|(_, k, v)| k == "color_scheme_path" && v.ends_with("qt6ct/colors/macshell.conf")));
        std::fs::create_dir_all(env.config_home.join("qt6ct")).unwrap();
        std::fs::write(env.config_home.join("qt6ct/qt6ct.conf"), "[Appearance]\nstyle=Breeze\n").unwrap();
        let plan = Qt.plan(&palette(true), &Config::default(), &env);
        let Edit::Ini { sets, .. } = &plan.edits[1] else { panic!() };
        assert!(!sets.iter().any(|(_, k, _)| k == "style"), "an existing style is not replaced");
    }

    #[test]
    fn kvantum_is_opt_in() {
        let d = tmp("kv");
        let (env, _) = env(&d, &["kvantummanager"]);
        assert!(Kvantum.available(&env).is_ok());
        assert!(!Kvantum.default_on(&env));
        let k = kvconfig(&palette(false));
        assert!(k.contains("[GeneralColors]\nwindow.color=") && k.contains("highlight.color="));
    }
}
