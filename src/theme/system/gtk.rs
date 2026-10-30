//! GTK 3 and GTK 4 / libadwaita: a managed block in `gtk.css` that redefines the named colours, `settings.ini` and gsettings.
//!
//! libadwaita (Nautilus, Text Editor, ...) reads `window_bg_color`, `accent_bg_color`, `headerbar_*`, `sidebar_*` and friends from the
//! user's `gtk-4.0/gtk.css`; plain GTK 3 and GTK 4 Adwaita use the older `theme_*` names, so both families are defined. The block is
//! appended to whatever the user already has and cut out again by `decthemectl system off`.

use super::files::{Comment, Edit};
use super::{Config, Env, Palette, Plan, Target};
use crate::theme::fontdb::{self, norm, FontDb};
use crate::theme::model::Rgb;
use std::fmt::Write;

pub struct Gtk;

fn rgba(c: Rgb, a: f32) -> String {
    format!("rgba({}, {}, {}, {})", c.0, c.1, c.2, a)
}

/// The named colours, shared by GTK 3 and 4.
pub fn colors(p: &Palette) -> String {
    let mut s = String::new();
    let mut d = |name: &str, v: &str| {
        let _ = writeln!(s, "@define-color {name} {v};");
    };
    let shade = if p.dark { rgba(Rgb(0, 0, 0), 0.36) } else { rgba(Rgb(0, 0, 0), 0.07) };
    let on = |c: Rgb| c.readable_on().hex();
    // libadwaita
    d("accent_bg_color", &p.accent.hex());
    d("accent_fg_color", &p.accent_text.hex());
    d("accent_color", &p.accent_on_bg().hex());
    d("destructive_bg_color", &p.red.hex());
    d("destructive_fg_color", &on(p.red));
    d("destructive_color", &p.red.hex());
    d("success_bg_color", &p.green.hex());
    d("success_fg_color", &on(p.green));
    d("success_color", &p.green.hex());
    d("warning_bg_color", &p.orange.hex());
    d("warning_fg_color", &on(p.orange));
    d("warning_color", &p.orange.hex());
    d("error_bg_color", &p.red.hex());
    d("error_fg_color", &on(p.red));
    d("error_color", &p.red.hex());
    d("window_bg_color", &p.bg.hex());
    d("window_fg_color", &p.text.hex());
    d("view_bg_color", &p.bg.hex());
    d("view_fg_color", &p.text.hex());
    d("headerbar_bg_color", &p.titlebar.hex());
    d("headerbar_fg_color", &p.text.hex());
    d("headerbar_border_color", &p.border.hex());
    d("headerbar_backdrop_color", &p.titlebar_inactive.hex());
    d("headerbar_shade_color", &shade);
    d("headerbar_darker_shade_color", &shade);
    d("sidebar_bg_color", &p.sidebar.hex());
    d("sidebar_fg_color", &p.text.hex());
    d("sidebar_backdrop_color", &p.sidebar.hex());
    d("sidebar_shade_color", &shade);
    d("secondary_sidebar_bg_color", &p.bg.hex());
    d("secondary_sidebar_fg_color", &p.text.hex());
    d("secondary_sidebar_backdrop_color", &p.bg.hex());
    d("secondary_sidebar_shade_color", &shade);
    d("card_bg_color", &p.surface.hex());
    d("card_fg_color", &p.text.hex());
    d("card_shade_color", &shade);
    d("dialog_bg_color", &p.popup.hex());
    d("dialog_fg_color", &p.text.hex());
    d("popover_bg_color", &p.popup.hex());
    d("popover_fg_color", &p.text.hex());
    d("popover_shade_color", &shade);
    d("thumbnail_bg_color", &p.surface.hex());
    d("thumbnail_fg_color", &p.text.hex());
    d("shade_color", &shade);
    d("scrollbar_outline_color", &p.bg.hex());
    // GTK 3 and the GTK 4 Adwaita theme without libadwaita
    d("theme_bg_color", &p.bg.hex());
    d("theme_fg_color", &p.text.hex());
    d("theme_base_color", &p.bg.hex());
    d("theme_text_color", &p.text.hex());
    d("theme_selected_bg_color", &p.accent.hex());
    d("theme_selected_fg_color", &p.accent_text.hex());
    d("theme_unfocused_bg_color", &p.bg.hex());
    d("theme_unfocused_fg_color", &p.text_secondary.hex());
    d("theme_unfocused_base_color", &p.bg.hex());
    d("theme_unfocused_text_color", &p.text_secondary.hex());
    d("theme_unfocused_selected_bg_color", &p.accent.over(p.bg, 0.6).hex());
    d("theme_unfocused_selected_fg_color", &p.accent_text.hex());
    d("insensitive_bg_color", &p.surface.hex());
    d("insensitive_fg_color", &p.text_faint.hex());
    d("insensitive_base_color", &p.bg.hex());
    d("theme_hovering_selected_bg_color", &p.accent.hex());
    d("borders", &p.border.hex());
    d("unfocused_borders", &p.border.hex());
    d("content_view_bg", &p.bg.hex());
    d("link_color", &p.accent_on_bg().hex());
    d("visited_link_color", &p.magenta.hex());
    d("tooltip_bg_color", &p.popup.hex());
    d("tooltip_fg_color", &p.text.hex());
    d("osd_bg_color", &p.popup.hex());
    d("osd_fg_color", &p.text.hex());
    d("wm_title", &p.text.hex());
    d("wm_unfocused_title", &p.text_secondary.hex());
    d("wm_bg", &p.titlebar.hex());
    d("wm_border", &p.border.hex());
    s
}

/// GTK 3's Adwaita theme exports the `theme_*` names but paints widgets with literal colours, so named colours alone change little:
/// these rules recolour the common widgets (window, views, entries, buttons, selection, menus, header bars, scrollbars).
fn gtk3_extras(p: &Palette) -> String {
    let hover = p.text.over(p.surface, 0.08).hex();
    let active = p.text.over(p.surface, 0.16).hex();
    let rules = [
        ("window, .background", format!("background-color: {}; color: {};", p.bg.hex(), p.text.hex())),
        (".view, textview text, iconview, treeview.view", format!("background-color: {}; color: {};", p.bg.hex(), p.text.hex())),
        (".sidebar, placessidebar, .sidebar row", format!("background-color: {};", p.sidebar.hex())),
        ("headerbar, .titlebar", format!("background-color: {}; background-image: none; color: {}; border-color: {}; box-shadow: none;", p.titlebar.hex(), p.text.hex(), p.border.hex())),
        ("headerbar:backdrop, .titlebar:backdrop", format!("background-color: {}; color: {};", p.titlebar_inactive.hex(), p.text_secondary.hex())),
        ("entry, spinbutton", format!("background-color: {}; background-image: none; color: {}; border-color: {};", p.surface.hex(), p.text.hex(), p.border.hex())),
        ("button", format!("background-color: {}; background-image: none; color: {}; border-color: {}; box-shadow: none; text-shadow: none;", p.surface.hex(), p.text.hex(), p.border.hex())),
        ("button:hover", format!("background-color: {hover};")),
        ("button:active, button:checked", format!("background-color: {active};")),
        ("button.suggested-action", format!("background-color: {}; color: {};", p.accent.hex(), p.accent_text.hex())),
        ("button.destructive-action", format!("background-color: {}; color: {};", p.red.hex(), p.red.readable_on().hex())),
        ("selection, *:selected, row:selected, .view:selected", format!("background-color: {}; color: {};", p.accent.hex(), p.accent_text.hex())),
        ("progressbar progress, scale highlight, switch:checked", format!("background-color: {}; background-image: none;", p.accent.hex())),
        ("progressbar trough, scale trough, switch", format!("background-color: {}; background-image: none; border-color: {};", p.surface.hex(), p.border.hex())),
        ("menu, .menu, .popup, popover.background, popover.background > contents", format!("background-color: {}; color: {}; border-color: {};", p.popup.hex(), p.text.hex(), p.border.hex())),
        ("menuitem:hover, modelbutton:hover", format!("background-color: {};", p.accent.hex())),
        ("tooltip.background", format!("background-color: {}; color: {};", p.popup.hex(), p.text.hex())),
        ("separator", format!("background-color: {};", p.border.hex())),
        ("scrollbar slider", format!("background-color: {};", p.text_faint.hex())),
        ("notebook > header", format!("background-color: {}; border-color: {};", p.surface.hex(), p.border.hex())),
        (".dim-label", format!("color: {};", p.text_secondary.hex())),
    ];
    rules.iter().map(|(sel, body)| format!("{sel} {{ {body} }}\n")).collect()
}

pub fn css(p: &Palette, gtk4: bool) -> String {
    let mut s = format!("/* {}: {} */\n", p.name, if p.dark { "dark" } else { "light" });
    s.push_str(&colors(p));
    if gtk4 {
        // libadwaita 1.6+ migrated to native CSS variables. Named colour aliases
        // remain for older GTK; current Files reads the hyphenated variables.
        s.push_str(":root {\n");
        for line in colors(p).lines() {
            if let Some(rest) = line.strip_prefix("@define-color ") {
                if let Some((name, value)) = rest.split_once(' ') {
                    let _ = writeln!(s, "  --{}: {}", name.replace('_', "-"), value);
                }
            }
        }
        s.push_str("}\n");
    } else {
        s.push_str(&gtk3_extras(p));
    }
    s
}

/// `gtk-decoration-layout`: the window buttons on the left for the macOS titlebar style, on the right for classic.
pub fn layout(p: &Palette) -> &'static str {
    if p.classic_buttons {
        "appmenu:minimize,maximize,close"
    } else {
        "close,minimize,maximize:"
    }
}

/// The `gtk-font-name` value ("Inter 11"), or `None` to leave the user's GTK font alone: the option `gtk.font` is off, the theme names no
/// font, it names a generic alias (`sans-serif`, `system-ui`: nothing explicit to carry over) or the family is not installed.
pub fn font_name(p: &Palette, cfg: &Config, db: &FontDb) -> Option<String> {
    if !cfg.option("gtk.font") {
        return None;
    }
    let wanted = p.font.as_deref()?;
    if matches!(norm(wanted).as_str(), "sansserif" | "sans" | "systemui" | "system" | "default" | "monospace" | "mono" | "uimonospace") {
        return None;
    }
    let family = db.resolve(wanted)?;
    Some(format!("{family} {}", fmt_pt(p.font_pt)))
}

/// `settings.ini`: `font` is [`font_name`].
pub fn settings(p: &Palette, cfg: &Config, font: Option<&str>) -> Vec<(String, String, String)> {
    let s = |k: &str, v: String| ("Settings".to_string(), k.to_string(), v);
    let mut v = vec![s("gtk-application-prefer-dark-theme", if p.dark { "1" } else { "0" }.into()), s("gtk-decoration-layout", layout(p).into())];
    if let Some(f) = font {
        v.push(s("gtk-font-name", f.to_string()));
    }
    if let Some(i) = &cfg.icon_theme {
        v.push(s("gtk-icon-theme-name", i.clone()));
    }
    if let Some(c) = &cfg.cursor_theme {
        v.push(s("gtk-cursor-theme-name", c.clone()));
    }
    v
}

fn fmt_pt(pt: f32) -> String {
    if pt.fract() == 0.0 {
        format!("{}", pt as i32)
    } else {
        format!("{pt}")
    }
}

// Older Decebal desktop styles linked an entire hardcoded GTK theme here.
// Appending palette variables cannot override its literal widget colours.
fn legacy_stylesheet(path: &std::path::Path, env: &Env) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())
        && path.canonicalize().ok().zip(env.config_home.join("omarchy/desktop-styles").canonicalize().ok()).is_some_and(|(target, root)| target.starts_with(root))
}

impl Target for Gtk {
    fn id(&self) -> &'static str {
        "gtk"
    }
    fn title(&self) -> &'static str {
        "GTK apps"
    }
    fn detail(&self) -> &'static str {
        "Nautilus and every GTK 3 / GTK 4 / libadwaita app: colours, dark or light, window buttons"
    }
    fn available(&self, _env: &Env) -> Result<(), String> {
        Ok(())
    }

    fn plan(&self, p: &Palette, cfg: &Config, env: &Env) -> Plan {
        let mut plan = Plan::default();
        let font = if cfg.option("gtk.font") && p.font.is_some() { font_name(p, cfg, &fontdb::get()) } else { None };
        for (dir, gtk4) in [("gtk-3.0", false), ("gtk-4.0", true)] {
            let path = env.config_home.join(dir).join("gtk.css");
            if legacy_stylesheet(&path, env) {
                // Whole-file edits participate in the same original-byte backup/undo transaction.
                plan.edits.push(Edit::Whole { path, text: css(p, gtk4) });
            } else {
                plan.edits.push(Edit::Block { path, comment: Comment::Css, text: css(p, gtk4) });
            }
            plan.edits.push(Edit::Ini { path: env.config_home.join(dir).join("settings.ini"), sets: settings(p, cfg, font.as_deref()) });
        }
        let g = |k: &str, v: String| ("org.gnome.desktop.interface".to_string(), k.to_string(), v);
        plan.gsettings.push(g("gtk-theme", if p.dark { "'Adwaita-dark'" } else { "'Adwaita'" }.into()));
        plan.gsettings.push(g("color-scheme", if p.dark { "'prefer-dark'" } else { "'prefer-light'" }.into()));
        if let Some(f) = &font {
            plan.gsettings.push(g("font-name", format!("'{f}'")));
        }
        if let Some(i) = &cfg.icon_theme {
            plan.gsettings.push(g("icon-theme", format!("'{i}'")));
        }
        if let Some(c) = &cfg.cursor_theme {
            plan.gsettings.push(g("cursor-theme", format!("'{c}'")));
        }
        plan.gsettings.push(("org.gnome.desktop.wm.preferences".into(), "button-layout".into(), format!("'{}'", layout(p))));
        plan
    }

    fn undo(&self, cx: &mut super::Ctx) {
        super::gsettings_restore(cx, "gtk");
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    #[test]
    fn css_defines_the_libadwaita_and_gtk3_names_from_the_palette() {
        let p = palette(true);
        let g4 = css(&p, true);
        let g3 = css(&p, false);
        for name in ["accent_bg_color", "window_bg_color", "view_bg_color", "headerbar_bg_color", "sidebar_bg_color", "card_bg_color", "popover_bg_color", "theme_bg_color", "theme_selected_bg_color"]
        {
            assert!(g4.contains(&format!("@define-color {name} ")), "{name}");
        }
        assert!(g4.contains(&format!("@define-color window_bg_color {};", p.bg.hex())));
        assert!(g4.contains(&format!("@define-color accent_bg_color {};", p.accent.hex())));
        assert!(g4.contains(&format!("@define-color headerbar_bg_color {};", p.titlebar.hex())));
        assert!(!g4.contains("headerbar, .titlebar"), "gtk 4 keeps libadwaita's own header bar rules");
        assert!(g3.contains(&format!("headerbar, .titlebar {{ background-color: {}; background-image: none;", p.titlebar.hex())));
        assert!(g3.contains(&format!("window, .background {{ background-color: {};", p.bg.hex())));
        assert!(g4.contains(&format!("--window-bg-color: {};", p.bg.hex())));
        assert!(g4.contains(&format!("--sidebar-bg-color: {};", p.sidebar.hex())));
    }

    #[test]
    fn golden_first_lines_for_the_macos_dark_palette() {
        let g = css(&palette(true), true);
        let head: Vec<&str> = g.lines().take(4).collect();
        assert_eq!(head, ["/* macOS: dark */", "@define-color accent_bg_color #0a84ff;", "@define-color accent_fg_color #ffffff;", "@define-color accent_color #0a84ff;"]);
    }

    #[test]
    fn settings_and_layout_follow_the_titlebar_style() {
        let mut p = palette(true);
        assert_eq!(layout(&p), "close,minimize,maximize:");
        let s = settings(&p, &Config::default(), None);
        assert!(s.contains(&("Settings".into(), "gtk-application-prefer-dark-theme".into(), "1".into())));
        assert!(!s.iter().any(|(_, k, _)| k == "gtk-icon-theme-name" || k == "gtk-font-name"), "unset optional keys are not written");
        p.classic_buttons = true;
        p.dark = false;
        p.font = Some("Segoe UI".into());
        p.font_pt = 10.5;
        let cfg = Config { icon_theme: Some("Papirus".into()), cursor_theme: Some("Bibata".into()), ..Config::default() };
        let s = settings(&p, &cfg, Some("Segoe UI 10.5"));
        for want in [
            ("gtk-application-prefer-dark-theme", "0"),
            ("gtk-decoration-layout", "appmenu:minimize,maximize,close"),
            ("gtk-font-name", "Segoe UI 10.5"),
            ("gtk-icon-theme-name", "Papirus"),
            ("gtk-cursor-theme-name", "Bibata"),
        ] {
            assert!(s.iter().any(|(_, k, v)| k == want.0 && v == want.1), "{want:?} in {s:?}");
        }
    }

    #[test]
    fn the_gtk_font_needs_an_installed_named_family_and_the_option() {
        let db = crate::theme::fontdb::fake::db();
        let mut p = palette(true);
        p.font_pt = 11.0;
        let mut cfg = Config::default();
        p.font = Some("inter".into());
        assert_eq!(font_name(&p, &cfg, &db).as_deref(), Some("Inter 11"), "on by default, the installed spelling");
        p.font = Some("Segoe UI".into());
        assert_eq!(font_name(&p, &cfg, &db), None, "not installed");
        for generic in ["sans-serif", "system-ui", "monospace"] {
            p.font = Some(generic.into());
            assert_eq!(font_name(&p, &cfg, &db), None, "{generic} names nothing explicit");
        }
        p.font = None;
        assert_eq!(font_name(&p, &cfg, &db), None);
        p.font = Some("Inter".into());
        cfg.set_option("gtk.font", false);
        assert_eq!(font_name(&p, &cfg, &db), None, "the user turned it off");
        assert!(cfg.options.contains_key("gtk.font"));
        cfg.set_option("gtk.font", true);
        assert!(cfg.options.is_empty(), "the default is not stored");
        assert_eq!(font_name(&p, &cfg, &db).as_deref(), Some("Inter 11"));
    }

    #[test]
    fn legacy_linked_full_theme_is_replaced_and_undo_restores_it() {
        let d = tmp("gtk-legacy");
        let (env, _) = env(&d, &[]);
        let target = env.config_home.join("omarchy/desktop-styles/gtk4-mac-dark/gtk.css");
        let linked = env.config_home.join("gtk-4.0/gtk.css");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        let original = "window { background-color: #123456; }\n";
        std::fs::write(&target, original).unwrap();
        std::os::unix::fs::symlink(&target, &linked).unwrap();
        let cfg = Config::default();
        let opts = super::super::Opts { only: Some(vec!["gtk".into()]), ..Default::default() };
        super::super::apply(&env, &cfg, &palette(true), &opts).unwrap();
        assert!(!std::fs::read_to_string(&linked).unwrap().contains("#123456"));
        assert!(linked.symlink_metadata().unwrap().file_type().is_symlink());
        super::super::off(&env, &cfg, &palette(true)).unwrap();
        assert_eq!(std::fs::read_to_string(&linked).unwrap(), original);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn plan_covers_both_gtk_versions_and_gsettings() {
        let d = tmp("gtkplan");
        let (env, _) = env(&d, &[]);
        let plan = Gtk.plan(&palette(false), &Config::default(), &env);
        let paths: Vec<String> = plan.edits.iter().map(|e| e.path().strip_prefix(&env.config_home).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(paths, ["gtk-3.0/gtk.css", "gtk-3.0/settings.ini", "gtk-4.0/gtk.css", "gtk-4.0/settings.ini"]);
        assert!(plan.gsettings.contains(&("org.gnome.desktop.interface".into(), "color-scheme".into(), "'prefer-light'".into())));
    }
}
