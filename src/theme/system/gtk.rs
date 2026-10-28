//! GTK 3 and GTK 4 / libadwaita: a managed block in `gtk.css` that redefines the named colours, `settings.ini` and gsettings.
//!
//! libadwaita (Nautilus, Text Editor, ...) reads `window_bg_color`, `accent_bg_color`, `headerbar_*`, `sidebar_*` and friends from the
//! user's `gtk-4.0/gtk.css`; plain GTK 3 and GTK 4 Adwaita use the older `theme_*` names, so both families are defined. The block is
//! appended to whatever the user already has and cut out again by `macthemectl system off`.

use super::files::{Comment, Edit};
use super::{Config, Env, Palette, Plan, Target};
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
    if !gtk4 {
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

pub fn settings(p: &Palette, cfg: &Config) -> Vec<(String, String, String)> {
    let s = |k: &str, v: String| ("Settings".to_string(), k.to_string(), v);
    let mut v = vec![s("gtk-application-prefer-dark-theme", if p.dark { "1" } else { "0" }.into()), s("gtk-decoration-layout", layout(p).into())];
    if let Some(f) = &p.font {
        v.push(s("gtk-font-name", format!("{f} {}", fmt_pt(p.font_pt))));
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
        for (dir, gtk4) in [("gtk-3.0", false), ("gtk-4.0", true)] {
            plan.edits.push(Edit::Block { path: env.config_home.join(dir).join("gtk.css"), comment: Comment::Css, text: css(p, gtk4) });
            plan.edits.push(Edit::Ini { path: env.config_home.join(dir).join("settings.ini"), sets: settings(p, cfg) });
        }
        let g = |k: &str, v: String| ("org.gnome.desktop.interface".to_string(), k.to_string(), v);
        plan.gsettings.push(g("color-scheme", if p.dark { "'prefer-dark'" } else { "'prefer-light'" }.into()));
        if let Some(f) = &p.font {
            plan.gsettings.push(g("font-name", format!("'{f} {}'", fmt_pt(p.font_pt))));
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
        assert_eq!(g4.lines().filter(|l| l.starts_with("@define-color")).count(), g4.matches(";\n").count());
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
        let s = settings(&p, &Config::default());
        assert!(s.contains(&("Settings".into(), "gtk-application-prefer-dark-theme".into(), "1".into())));
        assert!(!s.iter().any(|(_, k, _)| k == "gtk-icon-theme-name" || k == "gtk-font-name"), "unset optional keys are not written");
        p.classic_buttons = true;
        p.dark = false;
        p.font = Some("Segoe UI".into());
        p.font_pt = 10.5;
        let cfg = Config { icon_theme: Some("Papirus".into()), cursor_theme: Some("Bibata".into()), ..Config::default() };
        let s = settings(&p, &cfg);
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
    fn plan_covers_both_gtk_versions_and_gsettings() {
        let d = tmp("gtkplan");
        let (env, _) = env(&d, &[]);
        let plan = Gtk.plan(&palette(false), &Config::default(), &env);
        let paths: Vec<String> = plan.edits.iter().map(|e| e.path().strip_prefix(&env.config_home).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(paths, ["gtk-3.0/gtk.css", "gtk-3.0/settings.ini", "gtk-4.0/gtk.css", "gtk-4.0/settings.ini"]);
        assert!(plan.gsettings.contains(&("org.gnome.desktop.interface".into(), "color-scheme".into(), "'prefer-light'".into())));
    }
}
