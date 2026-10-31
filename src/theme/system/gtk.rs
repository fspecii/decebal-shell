//! GTK 3, GTK 4 and libadwaita: what each toolkit actually reads, rendered from one set of contrast-checked roles ([`Roles`]).
//!
//! * libadwaita (Nautilus, Text Editor, ...) ignores GTK themes (it forces `gtk-theme-name = Adwaita-empty`) and paints from its named
//!   colours, which its `:root` CSS variables are defined from (libadwaita 1.10 `--window-bg-color: @window_bg_color`). A managed block in
//!   the user's `gtk-4.0/gtk.css` (user priority, above libadwaita) redefines both.
//! * Plain GTK 3 (Adwaita) and plain GTK 4 (the built-in Default theme) paint widgets with literal colours, so named colours change
//!   almost nothing there. They get a generated named theme, `$XDG_DATA_HOME/themes/Decebal/gtk-{3,4}.0/gtk.css`: it imports the toolkit's
//!   own stylesheet and then recolours the controls and their states at theme priority, so an application's own CSS still wins over it.
//!   The theme is selected through `settings.ini` and gsettings `gtk-theme`. Two slots (`Decebal` / `Decebal-Alt`) alternate whenever the
//!   colours change, because GTK reloads a theme in running applications only when the theme *name* changes.
//! * `settings.ini` and gsettings carry dark/light, fonts, icon and cursor theme and the window-button layout.
//!
//! Everything is appended or written through the system file transaction (backups, `decthemectl system off` restores).

use super::files::{ini_get, Comment, Edit};
use super::{Config, Env, Palette, Plan, Target};
use crate::theme::fontdb::{self, norm, FontDb};
use crate::theme::model::{contrast, Rgb};
use std::fmt::Write;
use std::path::{Path, PathBuf};

pub struct Gtk;

/// The generated theme names. Running GTK 3 / GTK 4 apps reload a theme only when `gtk-theme-name` changes, so a colour change is
/// written to the slot that is not active and that slot is selected.
pub const THEME_SLOTS: [&str; 2] = ["Decebal", "Decebal-Alt"];

fn rgba(c: Rgb, a: f32) -> String {
    format!("rgba({}, {}, {}, {})", c.0, c.1, c.2, a)
}

/// `c`, moved toward black or white (whichever `bg` reads better with) until it reaches `min` contrast against `bg`.
pub fn ensure(c: Rgb, bg: Rgb, min: f32) -> Rgb {
    if contrast(c, bg) >= min {
        return c;
    }
    let pole = bg.readable_on();
    (1..=20).map(|i| pole.over(c, i as f32 * 0.05)).find(|m| contrast(*m, bg) >= min).unwrap_or(pole)
}

/// A text colour readable on `bg` and on its own tint over `bg` (libadwaita draws destructive buttons as their colour at 15-20 %, up to
/// 30 % pressed, behind text in that colour).
pub fn ensure_on_tint(c: Rgb, bg: Rgb, min: f32) -> Rgb {
    let ok = |c: Rgb| contrast(c, bg) >= min && contrast(c, c.over(bg, 0.3)) >= min;
    if ok(c) {
        return c;
    }
    let pole = bg.readable_on();
    (1..=20).map(|i| pole.over(c, i as f32 * 0.05)).find(|m| ok(*m)).unwrap_or(pole)
}

/// The opacity of a layer (`fg` text on `under`, both faded together) over `backdrop` at which the text still reads `min`:1, from `floor`.
fn layer_opacity(cases: &[(Rgb, Rgb, Rgb)], floor: f32, min: f32) -> f32 {
    (0..=20).map(|i| floor + (1.0 - floor) * i as f32 / 20.0).find(|a| cases.iter().all(|(fg, under, back)| contrast(fg.over(*back, *a), under.over(*back, *a)) >= min)).unwrap_or(1.0)
}

/// libadwaita dims text by opacity (`--dim-opacity`, 55 %): the lowest opacity at which every foreground still reads 4.5:1 on its own
/// background, never below libadwaita's default.
pub fn dim_opacity(pairs: &[(Rgb, Rgb)]) -> f32 {
    (11..=20).map(|i| i as f32 * 0.05).find(|a| pairs.iter().all(|(fg, bg)| contrast(fg.over(*bg, *a), *bg) >= 4.5)).unwrap_or(1.0)
}

/// The colours the stylesheets are written from. Each foreground is guaranteed readable on the background it is drawn on: 4.5:1 for
/// text (WCAG AA), 3:1 for text on accent / status fills (as libadwaita's own accents) and for control outlines and focus rings.
#[derive(Debug, Clone, PartialEq)]
pub struct Roles {
    pub dark: bool,
    pub bg: Rgb,
    pub fg: Rgb,
    pub view_bg: Rgb,
    pub view_fg: Rgb,
    /// Buttons, entries and other raised controls.
    pub control_bg: Rgb,
    pub control_fg: Rgb,
    pub control_hover: Rgb,
    pub control_active: Rgb,
    pub border: Rgb,
    /// Check / radio / switch outlines: they must be found without colour (3:1).
    pub outline: Rgb,
    pub header_bg: Rgb,
    pub header_fg: Rgb,
    pub header_backdrop_bg: Rgb,
    pub header_backdrop_fg: Rgb,
    pub sidebar_bg: Rgb,
    pub sidebar_fg: Rgb,
    pub sidebar_backdrop: Rgb,
    pub accent: Rgb,
    pub accent_fg: Rgb,
    pub accent_hover: Rgb,
    pub accent_active: Rgb,
    /// The accent as text, links and icons on the window and views.
    pub accent_text: Rgb,
    pub focus: Rgb,
    pub dim: Rgb,
    pub disabled_bg: Rgb,
    pub disabled_fg: Rgb,
    pub popover_bg: Rgb,
    pub popover_fg: Rgb,
    pub popover_alpha: f32,
    pub card_bg: Rgb,
    pub card_fg: Rgb,
    pub row_hover: Rgb,
    pub selected_backdrop: Rgb,
    pub trough: Rgb,
    pub knob: Rgb,
    pub red: Rgb,
    pub red_fg: Rgb,
    pub red_text: Rgb,
    pub green: Rgb,
    pub green_fg: Rgb,
    pub green_text: Rgb,
    pub orange: Rgb,
    pub orange_fg: Rgb,
    pub orange_text: Rgb,
    pub visited: Rgb,
}

fn fill_fg(preferred: Rgb, fill: Rgb) -> Rgb {
    if contrast(preferred, fill) >= 3.0 {
        preferred
    } else {
        fill.readable_on()
    }
}

impl Roles {
    pub fn new(p: &Palette) -> Roles {
        let fg = ensure(p.text, p.bg, 4.5);
        // Button text stays readable at rest, hovered and pressed (the fills are mixed from the text colour, so settle both).
        let mut control_fg = ensure(p.text, p.surface, 4.5);
        for _ in 0..4 {
            control_fg = ensure(ensure(control_fg, control_fg.over(p.surface, 0.08), 4.5), control_fg.over(p.surface, 0.16), 4.5);
        }
        let accent_fg = fill_fg(p.accent_text, p.accent);
        let accent_text = ensure(p.accent_on_bg(), p.bg, 4.5);
        let popover_fg = {
            // Translucent menus are read over anything: check the text against the popup blended over black and over white.
            let over_dark = p.popup.over(Rgb(0, 0, 0), p.popup_alpha);
            let over_light = p.popup.over(Rgb(255, 255, 255), p.popup_alpha);
            let c = ensure(p.text, p.popup, 4.5);
            ensure(ensure(c, over_dark, 4.5), over_light, 4.5)
        };
        let status = |c: Rgb| (c, c.readable_on(), ensure_on_tint(c, p.bg, 4.5));
        let (red, red_fg, red_text) = status(p.red);
        let (green, green_fg, green_text) = status(p.green);
        let (orange, orange_fg, orange_text) = status(p.orange);
        let accent_shift = if p.dark { Rgb(255, 255, 255) } else { Rgb(0, 0, 0) };
        Roles {
            dark: p.dark,
            bg: p.bg,
            fg,
            view_bg: p.bg,
            view_fg: fg,
            control_bg: p.surface,
            control_fg,
            control_hover: control_fg.over(p.surface, 0.08),
            control_active: control_fg.over(p.surface, 0.16),
            border: p.border,
            outline: ensure(p.border, p.surface, 3.0),
            header_bg: p.titlebar,
            header_fg: ensure(p.text, p.titlebar, 4.5),
            header_backdrop_bg: p.titlebar_inactive,
            header_backdrop_fg: ensure(p.text_secondary, p.titlebar_inactive, 4.5),
            sidebar_bg: p.sidebar,
            sidebar_fg: ensure(p.text, p.sidebar, 4.5),
            sidebar_backdrop: p.sidebar.over(p.bg, 0.6),
            accent: p.accent,
            accent_fg,
            accent_hover: accent_shift.over(p.accent, 0.1),
            accent_active: Rgb(0, 0, 0).over(p.accent, 0.14),
            accent_text,
            focus: ensure(p.accent, p.bg, 3.0),
            dim: ensure(p.text_secondary, p.bg, 4.5),
            disabled_bg: p.surface.over(p.bg, 0.5),
            disabled_fg: control_fg.over(p.surface.over(p.bg, 0.5), 0.5),
            popover_bg: p.popup,
            popover_fg,
            popover_alpha: p.popup_alpha,
            card_bg: p.surface,
            card_fg: control_fg,
            row_hover: fg.over(p.bg, 0.06),
            selected_backdrop: p.accent.over(p.bg, 0.55),
            trough: control_fg.over(p.surface, 0.2),
            knob: if p.dark { control_fg.over(p.surface, 0.82) } else { Rgb(255, 255, 255) },
            red,
            red_fg,
            red_text,
            green,
            green_fg,
            green_text,
            orange,
            orange_fg,
            orange_text,
            visited: ensure(p.magenta, p.bg, 4.5),
        }
    }

    fn shade(&self) -> String {
        if self.dark {
            rgba(Rgb(0, 0, 0), 0.36)
        } else {
            rgba(Rgb(0, 0, 0), 0.07)
        }
    }

    fn popover(&self) -> String {
        if self.popover_alpha < 1.0 {
            rgba(self.popover_bg, self.popover_alpha)
        } else {
            self.popover_bg.hex()
        }
    }
}

/// libadwaita's named colours (the 1.10 set its `:root` variables are defined from), in its own meaning: `headerbar_border_color` is a
/// foreground mixed at `--border-opacity`, the `*_color` names are standalone (text) colours, the `*_bg_color` / `*_fg_color` pairs fills.
pub fn adw_colors(r: &Roles) -> Vec<(&'static str, String)> {
    let shade = r.shade();
    vec![
        ("accent_bg_color", r.accent.hex()),
        ("accent_fg_color", r.accent_fg.hex()),
        ("accent_color", r.accent_text.hex()),
        ("destructive_bg_color", r.red.hex()),
        ("destructive_fg_color", r.red_fg.hex()),
        ("destructive_color", r.red_text.hex()),
        ("success_bg_color", r.green.hex()),
        ("success_fg_color", r.green_fg.hex()),
        ("success_color", r.green_text.hex()),
        ("warning_bg_color", r.orange.hex()),
        ("warning_fg_color", r.orange_fg.hex()),
        ("warning_color", r.orange_text.hex()),
        ("error_bg_color", r.red.hex()),
        ("error_fg_color", r.red_fg.hex()),
        ("error_color", r.red_text.hex()),
        ("window_bg_color", r.bg.hex()),
        ("window_fg_color", r.fg.hex()),
        ("view_bg_color", r.view_bg.hex()),
        ("view_fg_color", r.view_fg.hex()),
        ("headerbar_bg_color", r.header_bg.hex()),
        ("headerbar_fg_color", r.header_fg.hex()),
        ("headerbar_border_color", r.header_fg.hex()),
        ("headerbar_backdrop_color", r.header_backdrop_bg.hex()),
        ("headerbar_shade_color", shade.clone()),
        ("headerbar_darker_shade_color", shade.clone()),
        ("sidebar_bg_color", r.sidebar_bg.hex()),
        ("sidebar_fg_color", r.sidebar_fg.hex()),
        ("sidebar_backdrop_color", r.sidebar_backdrop.hex()),
        ("sidebar_border_color", r.border.hex()),
        ("sidebar_shade_color", shade.clone()),
        ("secondary_sidebar_bg_color", r.sidebar_bg.over(r.bg, 0.5).hex()),
        ("secondary_sidebar_fg_color", ensure(r.fg, r.sidebar_bg.over(r.bg, 0.5), 4.5).hex()),
        ("secondary_sidebar_backdrop_color", r.sidebar_backdrop.over(r.bg, 0.5).hex()),
        ("secondary_sidebar_border_color", r.border.hex()),
        ("secondary_sidebar_shade_color", shade.clone()),
        ("card_bg_color", r.card_bg.hex()),
        ("card_fg_color", r.card_fg.hex()),
        ("card_shade_color", shade.clone()),
        ("dialog_bg_color", r.popover_bg.hex()),
        ("dialog_fg_color", r.popover_fg.hex()),
        ("popover_bg_color", r.popover()),
        ("popover_fg_color", r.popover_fg.hex()),
        ("popover_shade_color", shade.clone()),
        ("thumbnail_bg_color", r.card_bg.hex()),
        ("thumbnail_fg_color", r.card_fg.hex()),
        ("shade_color", shade),
        ("scrollbar_outline_color", r.bg.hex()),
    ]
}

/// libadwaita variables with no named colour behind them (1.7+).
fn adw_vars(r: &Roles) -> Vec<(&'static str, String)> {
    let toggle = if r.dark { r.fg.over(r.bg, 0.2) } else { Rgb(255, 255, 255).over(r.control_bg, 0.85) };
    let overview = r.sidebar_bg.over(r.bg, 0.5);
    let dim = dim_opacity(&[(r.fg, r.bg), (r.view_fg, r.view_bg), (r.sidebar_fg, r.sidebar_bg), (r.card_fg, r.card_bg), (r.popover_fg, r.popover_bg)]);
    vec![
        ("dim-opacity", format!("{}%", (dim * 100.0).round() as i32)),
        ("active-toggle-bg-color", toggle.hex()),
        ("active-toggle-fg-color", ensure(r.fg, toggle, 4.5).hex()),
        ("overview-bg-color", overview.hex()),
        ("overview-fg-color", ensure(r.fg, overview, 4.5).hex()),
    ]
}

/// The names GTK 3 themes and applications' own CSS use (`@theme_bg_color` ...), plus the `decebal_*` names the generated themes use.
pub fn gtk_colors(r: &Roles) -> Vec<(&'static str, String)> {
    vec![
        ("theme_bg_color", r.bg.hex()),
        ("theme_fg_color", r.fg.hex()),
        ("theme_base_color", r.view_bg.hex()),
        ("theme_text_color", r.view_fg.hex()),
        ("theme_selected_bg_color", r.accent.hex()),
        ("theme_selected_fg_color", r.accent_fg.hex()),
        ("theme_unfocused_bg_color", r.bg.hex()),
        ("theme_unfocused_fg_color", r.fg.hex()),
        ("theme_unfocused_base_color", r.view_bg.hex()),
        ("theme_unfocused_text_color", r.view_fg.hex()),
        ("theme_unfocused_selected_bg_color", r.selected_backdrop.hex()),
        ("theme_unfocused_selected_fg_color", ensure(r.accent_fg, r.selected_backdrop, 3.0).hex()),
        ("insensitive_bg_color", r.disabled_bg.hex()),
        ("insensitive_fg_color", r.disabled_fg.hex()),
        ("insensitive_base_color", r.disabled_bg.hex()),
        ("theme_hovering_selected_bg_color", r.accent_hover.hex()),
        ("borders", r.border.hex()),
        ("unfocused_borders", r.border.hex()),
        ("content_view_bg", r.view_bg.hex()),
        ("link_color", r.accent_text.hex()),
        ("visited_link_color", r.visited.hex()),
        ("tooltip_bg_color", r.popover_bg.hex()),
        ("tooltip_fg_color", r.popover_fg.hex()),
        ("osd_bg_color", r.popover_bg.hex()),
        ("osd_fg_color", r.popover_fg.hex()),
        ("success_color", r.green_text.hex()),
        ("warning_color", r.orange_text.hex()),
        ("error_color", r.red_text.hex()),
        ("wm_title", r.header_fg.hex()),
        ("wm_unfocused_title", r.header_backdrop_fg.hex()),
        ("wm_bg", r.header_bg.hex()),
        ("wm_border", r.border.hex()),
        ("decebal_control_bg", r.control_bg.hex()),
        ("decebal_control_fg", r.control_fg.hex()),
        ("decebal_control_hover", r.control_hover.hex()),
        ("decebal_control_active", r.control_active.hex()),
        ("decebal_outline", r.outline.hex()),
        ("decebal_header_bg", r.header_bg.hex()),
        ("decebal_header_fg", r.header_fg.hex()),
        ("decebal_header_backdrop_bg", r.header_backdrop_bg.hex()),
        ("decebal_header_backdrop_fg", r.header_backdrop_fg.hex()),
        ("decebal_sidebar_bg", r.sidebar_bg.hex()),
        ("decebal_sidebar_fg", r.sidebar_fg.hex()),
        ("decebal_sidebar_backdrop", r.sidebar_backdrop.hex()),
        ("decebal_accent", r.accent.hex()),
        ("decebal_accent_fg", r.accent_fg.hex()),
        ("decebal_accent_hover", r.accent_hover.hex()),
        ("decebal_accent_active", r.accent_active.hex()),
        ("decebal_accent_text", r.accent_text.hex()),
        ("decebal_focus", r.focus.hex()),
        ("decebal_dim", r.dim.hex()),
        ("decebal_popover_bg", r.popover()),
        ("decebal_popover_fg", r.popover_fg.hex()),
        ("decebal_row_hover", r.row_hover.hex()),
        ("decebal_trough", r.trough.hex()),
        ("decebal_knob", r.knob.hex()),
        ("decebal_destructive", r.red.hex()),
        ("decebal_destructive_fg", r.red_fg.hex()),
        ("decebal_destructive_hover", r.red_fg.over(r.red, 0.1).hex()),
    ]
}

fn define(list: &[(&str, String)]) -> String {
    list.iter().map(|(k, v)| format!("@define-color {k} {v};\n")).collect()
}

/// The managed block of the user's `gtk-3.0/gtk.css` / `gtk-4.0/gtk.css`.
///
/// GTK 3: the named colours only (apps' own CSS and GTK 3 themes that use them follow; the controls are the generated theme's job, so
/// an application's own CSS is not overridden at user priority). GTK 4: libadwaita's named colours and `:root` variables, and the one
/// rule libadwaita's palette cannot express: how far a backdrop header fades.
pub fn css(p: &Palette, gtk4: bool) -> String {
    let r = Roles::new(p);
    let mut s = format!("/* {}: {} */\n", p.name, if p.dark { "dark" } else { "light" });
    if gtk4 {
        let adw = adw_colors(&r);
        s.push_str(&define(&adw));
        // GTK 4.16+ CSS variables: libadwaita 1.6+ paints with these.
        s.push_str(":root {\n");
        for (name, value) in adw.iter().map(|(k, v)| (k.replace('_', "-"), v.clone())).chain(adw_vars(&r).into_iter().map(|(k, v)| (k.to_string(), v))) {
            let _ = writeln!(s, "  --{name}: {value};");
        }
        s.push_str("}\n");
        // libadwaita fades the whole header to 50 % in backdrop; fade only as far as its text stays readable (4.5:1).
        let fade = layer_opacity(
            &[
                (r.header_fg, r.header_backdrop_bg, r.header_backdrop_bg),
                (r.fg, r.bg, r.bg),
                (r.sidebar_fg, r.sidebar_bg, r.sidebar_bg),
                (r.control_fg, r.control_bg, r.bg),
                (r.control_fg, r.control_bg, r.header_backdrop_bg),
            ],
            0.5,
            4.5,
        );
        let _ = writeln!(s, "headerbar:backdrop > windowhandle, toolbarview > .top-bar:backdrop > windowhandle, toolbarview > .bottom-bar:backdrop > windowhandle {{ filter: opacity({fade:.2}); }}");
        // Our block outranks libadwaita by priority, so its "do not fade twice" rule for header bars inside top bars is restated here.
        s.push_str("toolbarview > .top-bar headerbar:backdrop > windowhandle, toolbarview > .bottom-bar headerbar:backdrop > windowhandle { filter: none; }\n");
    } else {
        s.push_str(&define(&gtk_colors(&r)));
    }
    s
}

/// The user `gtk-4.0` block plus the Files visual profile (Nautilus-scoped geometry only).
pub fn css_with_profile(p: &Palette, cfg: &Config, gtk4: bool) -> String {
    let mut s = css(p, gtk4);
    if gtk4 {
        s.push_str(&super::file_manager::css(cfg.file_manager_style.resolve(&p.id)));
    }
    s
}

/// The GTK 3 managed block while the generated theme is selected: no colour definitions at all.
pub const GTK3_THEME_BLOCK: &str = "/* Colours come from the generated Decebal GTK theme (gtk-theme-name), so open apps follow theme changes. */\n";

/// The managed block of `gtk-{3,4}.0/gtk.css`.
///
/// GTK 3 resolves `@name` colour references across style providers by priority, and the user `gtk.css` (user priority, loaded once
/// per process) outranks the theme. Named colours defined there would therefore keep pointing every rule of a theme reloaded live
/// (through XSETTINGS) at the old palette. So while the generated theme is selected the GTK 3 block defines nothing and the theme
/// carries the names itself. With the theme option off (the user keeps their own GTK theme) the block carries the named colours, as
/// before. GTK 4's block holds only libadwaita's names, which the generated GTK 4 theme does not use, so it keeps them.
pub fn user_css(p: &Palette, cfg: &Config, gtk4: bool, generated_theme: bool) -> String {
    if !gtk4 && generated_theme {
        return GTK3_THEME_BLOCK.to_string();
    }
    css_with_profile(p, cfg, gtk4)
}

/// The toolkit a generated theme is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kit {
    Gtk3,
    Gtk4,
}

/// Rules shared by GTK 3 Adwaita and GTK 4 Default (same node names), written after the imported toolkit sheet at the same priority.
/// They only colour: sizes, paddings and radii stay the toolkit's so hit targets and layout do not move.
const COMMON_RULES: &str = r#"
window, .background { background-color: @theme_bg_color; color: @theme_fg_color; }
.background:backdrop { background-color: @theme_bg_color; color: @theme_fg_color; }
.dim-label, label.dim-label, .subtitle { color: @decebal_dim; opacity: 1; }
label:disabled, .dim-label:disabled { color: @insensitive_fg_color; }
.view, iconview, .view text, iconview text, textview > text, textview text, treeview.view, gridview, listview, columnview { background-color: @theme_base_color; color: @theme_text_color; }
textview border { background-color: @theme_base_color; }
.view:disabled, .view text:disabled, textview text:disabled { background-color: @insensitive_base_color; color: @insensitive_fg_color; }
.view:selected, iconview:selected, .view text:selected, treeview.view:selected, gridview > child:selected, listview > row:selected, columnview > listview > row:selected, list > row:selected, row:selected, flowbox > flowboxchild:selected, .view:selected:focus { background-color: @theme_selected_bg_color; color: @theme_selected_fg_color; }
row:selected:backdrop, .view:selected:backdrop, treeview.view:selected:backdrop { background-color: @theme_unfocused_selected_bg_color; color: @theme_unfocused_selected_fg_color; }
selection, textview > text > selection, entry > text > selection, label > selection { background-color: alpha(@decebal_accent, 0.4); color: @theme_text_color; }
row.activatable:hover:not(:selected), list > row:hover:not(:selected), listview > row:hover:not(:selected) { background-color: @decebal_row_hover; }
.sidebar, placessidebar, stacksidebar, .navigation-sidebar { background-color: @decebal_sidebar_bg; color: @decebal_sidebar_fg; border-color: @borders; }
.sidebar:backdrop, placessidebar:backdrop, .navigation-sidebar:backdrop { background-color: @decebal_sidebar_backdrop; }
.sidebar row, placessidebar row, stacksidebar row, .navigation-sidebar > row { color: @decebal_sidebar_fg; }
.sidebar row:selected, placessidebar row:selected, stacksidebar row:selected, .navigation-sidebar > row:selected { background-color: @theme_selected_bg_color; color: @theme_selected_fg_color; }
.sidebar row:selected:backdrop, placessidebar row:selected:backdrop, .navigation-sidebar > row:selected:backdrop { background-color: @theme_unfocused_selected_bg_color; color: @theme_unfocused_selected_fg_color; }
headerbar, .titlebar:not(headerbar) { background-color: @decebal_header_bg; background-image: none; color: @decebal_header_fg; border-color: @borders; box-shadow: inset 0 -1px @borders; text-shadow: none; }
headerbar:backdrop, .titlebar:not(headerbar):backdrop { background-color: @decebal_header_backdrop_bg; background-image: none; color: @decebal_header_backdrop_fg; }
headerbar .title, .titlebar .title { color: @decebal_header_fg; }
headerbar:backdrop .title, .titlebar:backdrop .title { color: @decebal_header_backdrop_fg; }
button { background-color: @decebal_control_bg; background-image: none; color: @decebal_control_fg; border-color: @borders; box-shadow: none; text-shadow: none; -gtk-icon-shadow: none; }
button:hover { background-color: @decebal_control_hover; background-image: none; color: @decebal_control_fg; border-color: @borders; }
button:active, button:checked { background-color: @decebal_control_active; background-image: none; color: @decebal_control_fg; box-shadow: none; }
button:backdrop { background-color: @decebal_control_bg; background-image: none; color: @decebal_control_fg; border-color: @borders; }
button:disabled, button:disabled:backdrop { background-color: @insensitive_bg_color; background-image: none; color: @insensitive_fg_color; border-color: @borders; box-shadow: none; }
button:disabled label, button:disabled:backdrop label { color: @insensitive_fg_color; }
button.flat, button.titlebutton, headerbar button.titlebutton, windowcontrols > button, button.flat:backdrop { background-color: transparent; background-image: none; border-color: transparent; box-shadow: none; }
button.flat:hover, button.titlebutton:hover, windowcontrols > button:hover { background-color: @decebal_control_hover; }
button.flat:active, button.flat:checked, button.titlebutton:active, windowcontrols > button:active { background-color: @decebal_control_active; }
button.flat:disabled { background-color: transparent; color: @insensitive_fg_color; }
button.suggested-action, button.suggested-action:backdrop { background-color: @decebal_accent; background-image: none; color: @decebal_accent_fg; border-color: @decebal_accent; }
button.suggested-action:hover { background-color: @decebal_accent_hover; color: @decebal_accent_fg; border-color: @decebal_accent_hover; }
button.suggested-action:active, button.suggested-action:checked { background-color: @decebal_accent_active; color: @decebal_accent_fg; }
button.suggested-action label, button.destructive-action label { color: inherit; }
button.destructive-action, button.destructive-action:backdrop { background-color: @decebal_destructive; background-image: none; color: @decebal_destructive_fg; border-color: @decebal_destructive; }
button.destructive-action:hover, button.destructive-action:active { background-color: @decebal_destructive_hover; color: @decebal_destructive_fg; }
button.suggested-action:disabled, button.destructive-action:disabled { background-color: @insensitive_bg_color; color: @insensitive_fg_color; border-color: @borders; }
entry, spinbutton, spinbutton:not(.vertical) { background-color: @decebal_control_bg; background-image: none; color: @decebal_control_fg; border-color: @borders; box-shadow: none; caret-color: @decebal_accent_text; }
entry:disabled, spinbutton:disabled { background-color: @insensitive_bg_color; color: @insensitive_fg_color; }
entry image, spinbutton button { color: @decebal_control_fg; }
check, radio { background-color: @decebal_control_bg; background-image: none; border-color: @decebal_outline; color: @decebal_control_fg; box-shadow: none; }
check:hover, radio:hover { background-color: @decebal_control_hover; }
check:checked, radio:checked, check:indeterminate, radio:indeterminate { background-color: @decebal_accent; background-image: none; border-color: @decebal_accent; color: @decebal_accent_fg; }
check:checked:hover, radio:checked:hover { background-color: @decebal_accent_hover; }
check:disabled, radio:disabled { background-color: @insensitive_bg_color; border-color: @borders; color: @insensitive_fg_color; }
switch { background-color: @decebal_trough; background-image: none; border-color: @decebal_outline; color: @decebal_control_fg; }
switch:checked { background-color: @decebal_accent; border-color: @decebal_accent; color: @decebal_accent_fg; }
switch slider, switch > slider { background-color: @decebal_knob; background-image: none; border-color: @decebal_outline; box-shadow: none; }
switch:checked slider, switch:checked > slider { border-color: @decebal_accent; }
switch:disabled { background-color: @insensitive_bg_color; border-color: @borders; }
scale trough, progressbar trough, levelbar trough { background-color: @decebal_trough; background-image: none; border-color: @borders; }
scale highlight, progressbar progress, levelbar block.filled { background-color: @decebal_accent; background-image: none; border-color: @decebal_accent; }
scale slider { background-color: @decebal_knob; background-image: none; border-color: @decebal_outline; box-shadow: none; }
scale slider:hover { background-color: @decebal_control_hover; }
scale:disabled highlight, progressbar:disabled progress { background-color: @insensitive_fg_color; }
notebook > header { background-color: @decebal_sidebar_bg; border-color: @borders; }
notebook > header tab { color: @decebal_dim; }
notebook > header tab:hover { color: @theme_fg_color; }
notebook > header tab:checked { color: @theme_fg_color; }
notebook > header.top > tabs > tab:checked { box-shadow: inset 0 -3px @decebal_accent; }
notebook > header.bottom > tabs > tab:checked { box-shadow: inset 0 3px @decebal_accent; }
notebook > header.left > tabs > tab:checked { box-shadow: inset -3px 0 @decebal_accent; }
notebook > header.right > tabs > tab:checked { box-shadow: inset 3px 0 @decebal_accent; }
notebook > stack:not(:only-child) { background-color: @theme_base_color; }
menu, .menu, .context-menu, popover.background, popover > contents, popover.menu > contents { background-color: @decebal_popover_bg; color: @decebal_popover_fg; border-color: @borders; }
popover.background > arrow, popover > arrow { background-color: @decebal_popover_bg; border-color: @borders; }
menuitem:hover, menu menuitem:hover, .menu menuitem:hover, popover modelbutton:hover, popover.menu modelbutton:hover, popover.menu row:hover { background-color: @decebal_accent; color: @decebal_accent_fg; }
menuitem:disabled, modelbutton:disabled { color: @insensitive_fg_color; }
menubar { background-color: @decebal_header_bg; color: @decebal_header_fg; box-shadow: inset 0 -1px @borders; }
menubar > menuitem:hover, menubar > item:hover { box-shadow: inset 0 -3px @decebal_accent; color: @decebal_accent_text; }
tooltip, tooltip.background { background-color: @tooltip_bg_color; color: @tooltip_fg_color; border-color: @borders; }
tooltip * { color: @tooltip_fg_color; }
scrollbar slider { background-color: alpha(@theme_fg_color, 0.4); border-color: transparent; }
scrollbar slider:hover { background-color: alpha(@theme_fg_color, 0.6); }
scrollbar slider:active { background-color: @decebal_accent; }
separator { background-color: @borders; }
actionbar > revealer > box, .toolbar, toolbar, searchbar > revealer > box, statusbar { background-color: @theme_bg_color; color: @theme_fg_color; border-color: @borders; }
infobar.info > revealer > box, infobar.info { background-color: @decebal_accent; color: @decebal_accent_fg; }
infobar.error > revealer > box, infobar.error { background-color: @decebal_destructive; color: @decebal_destructive_fg; }
treeview.view header button, columnview > header > button { background-color: @theme_base_color; background-image: none; color: @decebal_dim; border-color: @borders; }
treeview.view header button:hover, columnview > header > button:hover { background-color: @decebal_control_hover; color: @theme_fg_color; }
link, button.link, *:link, label > link { color: @link_color; }
*:visited, label > link:visited { color: @visited_link_color; }
"#;

/// GTK 3 Adwaita node names and its focus treatment (an inset accent ring on entries).
const GTK3_RULES: &str = r#"
entry:focus, spinbutton:focus, spinbutton:not(.vertical):focus { border-color: @decebal_focus; box-shadow: inset 0 0 0 1px @decebal_focus; }
entry:focus:backdrop { border-color: @borders; box-shadow: none; }
button:focus, check:focus, radio:focus, switch:focus { outline-color: alpha(@decebal_focus, 0.7); }
.titlebar:not(headerbar), headerbar { border-bottom-color: @borders; }
.sidebar row:hover:not(:selected), placessidebar row:hover:not(:selected) { background-color: @decebal_row_hover; }
"#;

/// GTK 4 Default node names and its focus treatment (an outline ring).
const GTK4_RULES: &str = r#"
entry:focus-within, spinbutton:focus-within { border-color: @decebal_focus; outline: 2px solid alpha(@decebal_focus, 0.6); outline-offset: -2px; }
button:focus:focus-visible, checkbutton:focus:focus-visible > check, switch:focus:focus-visible, scale:focus:focus-visible slider { outline-color: alpha(@decebal_focus, 0.7); }
.navigation-sidebar > row:hover:not(:selected) { background-color: @decebal_row_hover; }
checkbutton > check, checkbutton > radio { background-color: @decebal_control_bg; border-color: @decebal_outline; }
checkbutton > check:checked, checkbutton > radio:checked { background-color: @decebal_accent; border-color: @decebal_accent; color: @decebal_accent_fg; }
"#;

/// Every selector also as its `:backdrop` twin, in place. The toolkit sheets repaint most surfaces in their own backdrop colours with
/// rules more specific than ours; the twin keeps the theme's colours when a window loses focus, and the explicit `:backdrop`,
/// `:disabled` and `:selected` rules further down still win by order or specificity.
fn with_backdrop(rules: &str) -> String {
    let mut out = String::new();
    for line in rules.lines() {
        match line.split_once(" { ") {
            Some((selectors, body)) if !selectors.contains(":backdrop") => {
                let both: Vec<String> = selectors.split(", ").flat_map(|s| [s.to_string(), format!("{s}:backdrop")]).collect();
                let _ = writeln!(out, "{} {{ {body}", both.join(", "));
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// A complete generated theme: the toolkit's own stylesheet for the mode, the named colours, then the colour rules.
pub fn theme_css(p: &Palette, kit: Kit) -> String {
    let r = Roles::new(p);
    let import = match (kit, p.dark) {
        (Kit::Gtk3, true) => "resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained-dark.css",
        (Kit::Gtk3, false) => "resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained.css",
        (Kit::Gtk4, true) => "resource:///org/gtk/libgtk/theme/Default/Default-dark.css",
        (Kit::Gtk4, false) => "resource:///org/gtk/libgtk/theme/Default/Default-light.css",
    };
    let mut s = format!(
        "/* Decebal GTK theme, generated from \"{}\" ({}). It is rewritten on every theme change; your own rules belong in ~/.config/gtk-{}.0/gtk.css */\n@import url(\"{import}\");\n",
        p.name,
        if p.dark { "dark" } else { "light" },
        if kit == Kit::Gtk3 { 3 } else { 4 }
    );
    s.push_str(&define(&gtk_colors(&r)));
    s.push_str(&with_backdrop(COMMON_RULES));
    s.push_str(&with_backdrop(if kit == Kit::Gtk3 { GTK3_RULES } else { GTK4_RULES }));
    s
}

fn index_theme(name: &str) -> String {
    format!("[Desktop Entry]\nType=X-GNOME-Metatheme\nName={name}\nComment=Generated by Decebal from the current desktop theme\nEncoding=UTF-8\n\n[X-GNOME-Metatheme]\nGtkTheme={name}\n")
}

fn theme_dir(env: &Env, slot: &str) -> PathBuf {
    env.data_home.join("themes").join(slot)
}

/// The slot to write: the active one when it already holds exactly these stylesheets (nothing changes, nothing reloads), else the other.
pub fn pick_slot(env: &Env, gtk3: &str, gtk4: &str) -> &'static str {
    let active = ["gtk-3.0", "gtk-4.0"]
        .iter()
        .filter_map(|d| std::fs::read_to_string(env.config_home.join(d).join("settings.ini")).ok())
        .find_map(|ini| ini_get(&ini, "Settings", "gtk-theme-name"))
        .and_then(|name| THEME_SLOTS.iter().position(|s| *s == name));
    let Some(i) = active else { return THEME_SLOTS[0] };
    let dir = theme_dir(env, THEME_SLOTS[i]);
    let same = |d: &str, want: &str| std::fs::read_to_string(dir.join(d).join("gtk.css")).ok().as_deref() == Some(want);
    if same("gtk-3.0", gtk3) && same("gtk-4.0", gtk4) {
        THEME_SLOTS[i]
    } else {
        THEME_SLOTS[1 - i]
    }
}

/// `gtk-decoration-layout`: the window buttons on the left for the macOS titlebar style, on the right for classic.
pub fn layout(p: &Palette) -> &'static str {
    if p.classic_buttons {
        "appmenu:minimize,maximize,close"
    } else {
        "close,minimize,maximize:"
    }
}

fn generic(name: &str) -> bool {
    matches!(norm(name).as_str(), "sansserif" | "sans" | "serif" | "systemui" | "system" | "default" | "monospace" | "mono" | "uimonospace" | "emoji")
}

/// The first installed, explicitly named family of `chain` (generic aliases name nothing to carry over).
fn first_installed<'a>(chain: impl IntoIterator<Item = &'a String>, db: &FontDb) -> Option<String> {
    chain.into_iter().filter(|f| !generic(f)).find_map(|f| db.resolve(f))
}

/// The `gtk-font-name` value ("Inter 11"), or `None` to leave the user's GTK font alone: the option `gtk.font` is off, or neither the
/// theme's font nor any explicit family of its interface chain is installed.
pub fn font_name(p: &Palette, cfg: &Config, db: &FontDb) -> Option<String> {
    if !cfg.option("gtk.font") {
        return None;
    }
    let family = first_installed(p.font.iter().chain(p.ui_fonts.iter()), db)?;
    Some(format!("{family} {}", fmt_pt(p.font_pt)))
}

/// gsettings `monospace-font-name` (libadwaita's `--monospace-font-family`, terminals and editors that follow GNOME).
pub fn mono_font_name(p: &Palette, cfg: &Config, db: &FontDb) -> Option<String> {
    if !cfg.option("gtk.font") {
        return None;
    }
    let family = first_installed(p.mono_fonts.iter(), db)?;
    Some(format!("{family} {}", fmt_pt(p.font_pt)))
}

/// Where GTK looks for icon and cursor themes, in its order.
fn icon_dirs(env: &Env) -> Vec<PathBuf> {
    let mut v = vec![env.home.join(".icons"), env.data_home.join("icons")];
    let data_dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    v.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("icons")));
    v.push(PathBuf::from("/usr/share/pixmaps"));
    v
}

/// An installed icon theme (`index.theme`) or cursor theme (`cursors/`) of that name.
pub fn installed(env: &Env, name: &str, cursor: bool) -> bool {
    !name.is_empty() && !name.contains('/') && icon_dirs(env).iter().any(|d| if cursor { d.join(name).join("cursors").is_dir() } else { d.join(name).join("index.theme").is_file() })
}

/// `settings.ini`: `font` is [`font_name`], `theme` the generated theme to select (`None` keeps the user's GTK theme).
pub fn settings(p: &Palette, cfg: &Config, font: Option<&str>) -> Vec<(String, String, String)> {
    settings_for(p, cfg, font, None, true, true)
}

fn settings_for(p: &Palette, cfg: &Config, font: Option<&str>, theme: Option<&str>, icon: bool, cursor: bool) -> Vec<(String, String, String)> {
    let s = |k: &str, v: String| ("Settings".to_string(), k.to_string(), v);
    let mut v = vec![s("gtk-application-prefer-dark-theme", if p.dark { "1" } else { "0" }.into()), s("gtk-decoration-layout", layout(p).into())];
    if let Some(t) = theme {
        v.push(s("gtk-theme-name", t.to_string()));
    }
    if let Some(f) = font {
        v.push(s("gtk-font-name", f.to_string()));
    }
    if let Some(i) = cfg.icon_theme.as_ref().filter(|_| icon) {
        v.push(s("gtk-icon-theme-name", i.clone()));
    }
    if let Some(c) = cfg.cursor_theme.as_ref().filter(|_| cursor) {
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
        "Nautilus and every GTK 3 / GTK 4 / libadwaita app: colours, controls and their states, dark or light, fonts, icons, window buttons (not Qt or DTK apps)"
    }
    fn available(&self, _env: &Env) -> Result<(), String> {
        Ok(())
    }

    fn plan(&self, p: &Palette, cfg: &Config, env: &Env) -> Plan {
        let mut plan = Plan::default();
        let db = (cfg.option("gtk.font") && (p.font.is_some() || !p.ui_fonts.is_empty() || !p.mono_fonts.is_empty())).then(fontdb::get);
        let font = db.as_ref().and_then(|db| font_name(p, cfg, db));
        let mono = db.as_ref().and_then(|db| mono_font_name(p, cfg, db));
        // A named theme that is not installed would leave GTK on hicolor / the X cursor: keep the user's and say why.
        let icon_ok = cfg.icon_theme.as_deref().is_none_or(|i| installed(env, i, false));
        let cursor_ok = cfg.cursor_theme.as_deref().is_none_or(|c| installed(env, c, true));
        let slot = cfg.option("gtk.theme").then(|| {
            let (g3, g4) = (theme_css(p, Kit::Gtk3), theme_css(p, Kit::Gtk4));
            let slot = pick_slot(env, &g3, &g4);
            let dir = theme_dir(env, slot);
            plan.edits.push(Edit::Whole { path: dir.join("index.theme"), text: index_theme(slot) });
            plan.edits.push(Edit::Whole { path: dir.join("gtk-3.0/gtk.css"), text: g3 });
            plan.edits.push(Edit::Whole { path: dir.join("gtk-4.0/gtk.css"), text: g4 });
            slot
        });
        for (dir, gtk4) in [("gtk-3.0", false), ("gtk-4.0", true)] {
            let path = env.config_home.join(dir).join("gtk.css");
            let text = user_css(p, cfg, gtk4, slot.is_some());
            if legacy_stylesheet(&path, env) {
                // Detach the config entry; the shared stylepack is never our output file.
                plan.edits.push(Edit::Detached { path, text });
            } else {
                plan.edits.push(Edit::Block { path, comment: Comment::Css, text });
            }
            plan.edits.push(Edit::Ini { path: env.config_home.join(dir).join("settings.ini"), sets: settings_for(p, cfg, font.as_deref(), slot, icon_ok, cursor_ok) });
        }
        let g = |k: &str, v: String| ("org.gnome.desktop.interface".to_string(), k.to_string(), v);
        if let Some(slot) = slot {
            plan.gsettings.push(g("gtk-theme", format!("'{slot}'")));
        }
        plan.gsettings.push(g("color-scheme", if p.dark { "'prefer-dark'" } else { "'prefer-light'" }.into()));
        if let Some(f) = &font {
            plan.gsettings.push(g("font-name", format!("'{f}'")));
        }
        if let Some(f) = &mono {
            plan.gsettings.push(g("monospace-font-name", format!("'{f}'")));
        }
        if let Some(i) = cfg.icon_theme.as_ref().filter(|_| icon_ok) {
            plan.gsettings.push(g("icon-theme", format!("'{i}'")));
        }
        if let Some(c) = cfg.cursor_theme.as_ref().filter(|_| cursor_ok) {
            plan.gsettings.push(g("cursor-theme", format!("'{c}'")));
        }
        plan.gsettings.push(("org.gnome.desktop.wm.preferences".into(), "button-layout".into(), format!("'{}'", layout(p))));
        plan
    }

    fn after(&self, cx: &mut super::Ctx) {
        cx.report.warnings.extend(missing_themes(cx.cfg, cx.env));
        // Only after the GTK files were committed: the manager publishes what was just written.
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
        let suppressed = std::env::var("DECEBAL_XSETTINGS").is_ok_and(|v| v == "0");
        if let Some(display) = cx.env.display.clone().filter(|d| super::xsettings::wanted(Some(d), wayland, suppressed, true)) {
            if cx.cfg.option("gtk.xsettings") {
                ensure_xsettings(cx, &display);
            } else {
                // Switched off later: Decebal's own manager stops; another manager is never touched.
                stop_xsettings(cx, &display);
            }
        }
    }

    fn undo(&self, cx: &mut super::Ctx) {
        if let Some(display) = cx.env.display.clone().filter(|_| !cfg!(test)) {
            stop_xsettings(cx, &display);
        }
        super::gsettings_restore(cx, "gtk");
    }
}

#[cfg(feature = "x11")]
fn ensure_xsettings(cx: &mut super::Ctx, display: &str) {
    use super::xsettings::Status;
    let ini = cx.env.config_home.join("gtk-3.0/settings.ini");
    match super::xsettings::ensure(display, &ini) {
        Ok(Status::Started { pid }) => cx.report.notes.push(format!("GTK: started Decebal's XSETTINGS manager (pid {pid}); open GTK 3 / GTK 4 apps follow theme changes")),
        Ok(Status::Running { .. }) => {}
        Ok(Status::Foreign) => cx.report.notes.push("GTK: another XSETTINGS manager runs on this display; open GTK apps follow it, Decebal leaves it alone".into()),
        Err(e) => cx.report.warnings.push(format!("GTK: XSETTINGS manager: {e}; open GTK apps keep their theme until reopened")),
    }
}

#[cfg(not(feature = "x11"))]
fn ensure_xsettings(_cx: &mut super::Ctx, _display: &str) {}

/// Stops Decebal's XSETTINGS manager (proved by its marker); any other manager is left alone.
fn stop_xsettings(cx: &mut super::Ctx, display: &str) {
    #[cfg(feature = "x11")]
    {
        match super::xsettings::stop(display) {
            Ok(true) => cx.report.notes.push("GTK: Decebal's XSETTINGS manager stopped".into()),
            Ok(false) => {}
            Err(e) => cx.report.warnings.push(format!("GTK: XSETTINGS manager: {e}")),
        }
    }
    #[cfg(not(feature = "x11"))]
    {
        let _ = (cx, display);
    }
}

/// The icon / cursor publication ([`super::publish_assets`]): `gtk-icon-theme-name` / `gtk-cursor-theme-name` in both `settings.ini`
/// files and gsettings `icon-theme` / `cursor-theme`, for installed themes only. No CSS, colour, font, theme-name or layout key.
pub fn asset_plan(cfg: &Config, env: &Env) -> Plan {
    let mut plan = Plan::default();
    let icon = cfg.icon_theme.as_ref().filter(|i| installed(env, i, false));
    let cursor = cfg.cursor_theme.as_ref().filter(|c| installed(env, c, true));
    let s = |k: &str, v: &String| ("Settings".to_string(), k.to_string(), v.clone());
    let sets: Vec<_> = icon.map(|i| s("gtk-icon-theme-name", i)).into_iter().chain(cursor.map(|c| s("gtk-cursor-theme-name", c))).collect();
    if sets.is_empty() {
        return plan;
    }
    for dir in ["gtk-3.0", "gtk-4.0"] {
        plan.edits.push(Edit::Ini { path: env.config_home.join(dir).join("settings.ini"), sets: sets.clone() });
    }
    let g = |k: &str, v: &String| ("org.gnome.desktop.interface".to_string(), k.to_string(), format!("'{v}'"));
    plan.gsettings.extend(icon.map(|i| g("icon-theme", i)).into_iter().chain(cursor.map(|c| g("cursor-theme", c))));
    plan
}

/// A named icon or cursor theme that is not installed is not published (GTK would fall back to hicolor / the X cursor): say so.
pub fn missing_themes(cfg: &Config, env: &Env) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(i) = cfg.icon_theme.as_deref().filter(|i| !installed(env, i, false)) {
        v.push(format!("GTK: icon theme `{i}` is not installed; GTK apps keep their current icons"));
    }
    if let Some(c) = cfg.cursor_theme.as_deref().filter(|c| !installed(env, c, true)) {
        v.push(format!("GTK: cursor theme `{c}` is not installed; the cursor is left as it is"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;

    /// Every shipped theme directory (compiled-in and package themes) in both modes, through the same resolution the apply uses.
    pub(crate) fn every_palette() -> Vec<Palette> {
        let themes = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        let src = crate::theme::live::Source { shell_json: std::env::temp_dir().join("decebal-gtk-every-palette-none.json"), themes_dir: themes.clone() };
        let mut ids: Vec<String> =
            std::fs::read_dir(&themes).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().join("theme.toml").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        ids.sort();
        let mut v = Vec::new();
        for id in &ids {
            for mode in [crate::theme::Mode::Dark, crate::theme::Mode::Light] {
                let sel = crate::theme::Selection { name: id.clone(), mode, ..Default::default() };
                let t = src.theme_for(id, &sel, mode == crate::theme::Mode::Dark).unwrap_or_else(|e| panic!("{id}: {e}"));
                v.push(Palette::from_theme(&t));
            }
        }
        v
    }

    #[test]
    fn roles_are_readable_for_every_shipped_theme_in_both_modes() {
        let all = every_palette();
        assert!(all.len() >= 88, "44 themes x 2 modes, got {}", all.len());
        for p in &all {
            let r = Roles::new(p);
            let who = format!("{} {}", p.id, if p.dark { "dark" } else { "light" });
            for (what, fg, bg, min) in [
                ("body", r.fg, r.bg, 4.5),
                ("view", r.view_fg, r.view_bg, 4.5),
                ("control", r.control_fg, r.control_bg, 4.5),
                ("hovered control", r.control_fg, r.control_hover, 4.5),
                ("pressed control", r.control_fg, r.control_active, 4.5),
                ("header", r.header_fg, r.header_bg, 4.5),
                ("backdrop header", r.header_backdrop_fg, r.header_backdrop_bg, 4.5),
                ("sidebar", r.sidebar_fg, r.sidebar_bg, 4.5),
                ("dim", r.dim, r.bg, 4.5),
                ("accent text", r.accent_text, r.bg, 4.5),
                ("popover", r.popover_fg, r.popover_bg, 4.5),
                ("destructive text", r.red_text, r.bg, 4.5),
                ("warning text", r.orange_text, r.bg, 4.5),
                ("success text", r.green_text, r.bg, 4.5),
                ("accent fill", r.accent_fg, r.accent, 2.9),
                ("destructive fill", r.red_fg, r.red, 2.9),
                ("focus ring", r.focus, r.bg, 3.0),
                ("check outline", r.outline, r.control_bg, 3.0),
            ] {
                assert!(contrast(fg, bg) >= min, "{who}: {what} {} on {} = {:.2}", fg.hex(), bg.hex(), contrast(fg, bg));
            }
            for (what, c) in [("destructive", r.red_text), ("success", r.green_text), ("warning", r.orange_text)] {
                for tint in [0.15, 0.2, 0.3] {
                    assert!(contrast(c, c.over(r.bg, tint)) >= 4.5, "{who}: {what} text on its {tint} tint");
                }
            }
            let fade = layer_opacity(&[(r.header_fg, r.header_backdrop_bg, r.header_backdrop_bg), (r.control_fg, r.control_bg, r.bg)], 0.5, 4.5);
            assert!(contrast(r.header_fg.over(r.header_backdrop_bg, fade), r.header_backdrop_bg) >= 4.5, "{who}: faded backdrop header");
            let dim = dim_opacity(&[(r.fg, r.bg), (r.sidebar_fg, r.sidebar_bg), (r.card_fg, r.card_bg)]);
            assert!((0.55..=1.0).contains(&dim) && contrast(r.fg.over(r.bg, dim), r.bg) >= 4.5, "{who}: dimmed text at {dim}");
            assert!(contrast(r.disabled_fg, r.disabled_bg) >= 1.4, "{who}: disabled text vanishes");
            assert!(contrast(r.disabled_fg, r.disabled_bg) < contrast(r.control_fg, r.control_bg), "{who}: disabled must look disabled");
            assert_ne!(r.control_hover, r.control_bg, "{who}: hover must show");
        }
    }

    #[test]
    fn libadwaita_block_uses_its_own_meanings_and_variables() {
        let p = palette(true);
        let r = Roles::new(&p);
        let g4 = css(&p, true);
        for name in [
            "accent_bg_color",
            "window_bg_color",
            "view_bg_color",
            "headerbar_bg_color",
            "sidebar_bg_color",
            "sidebar_border_color",
            "card_bg_color",
            "popover_bg_color",
            "secondary_sidebar_border_color",
        ] {
            assert!(g4.contains(&format!("@define-color {name} ")), "{name}");
            assert!(g4.contains(&format!("  --{}: ", name.replace('_', "-"))), "{name} variable");
        }
        // headerbar_border_color is a foreground libadwaita mixes at --border-opacity, not a finished border colour.
        assert!(g4.contains(&format!("@define-color headerbar_border_color {};", r.header_fg.hex())));
        // The standalone colours are the readable variants, not the raw fills.
        assert!(g4.contains(&format!("@define-color destructive_color {};", r.red_text.hex())));
        assert!(g4.contains(&format!("@define-color accent_color {};", r.accent_text.hex())));
        for var in ["--active-toggle-bg-color", "--overview-bg-color", "--dim-opacity"] {
            assert!(g4.contains(var), "{var}");
        }
        assert!(g4.contains(&format!("--window-bg-color: {};", p.bg.hex())));
        assert!(!g4.contains("headerbar {"), "no widget rules at user priority in GTK 4");
        assert_eq!(g4.matches("filter: opacity(").count(), 1, "only the backdrop fade");
        assert!(g4.contains("toolbarview > .top-bar headerbar:backdrop > windowhandle, toolbarview > .bottom-bar headerbar:backdrop > windowhandle { filter: none; }"), "never fades twice");
        let g3 = css(&p, false);
        assert!(g3.contains(&format!("@define-color theme_bg_color {};", p.bg.hex())));
        assert!(!g3.contains('{'), "the GTK 3 user block is colours only; controls live in the generated theme");
    }

    #[test]
    fn golden_first_lines_for_the_macos_dark_palette() {
        let g = css(&palette(true), true);
        let head: Vec<&str> = g.lines().take(4).collect();
        assert_eq!(head, ["/* macOS: dark */", "@define-color accent_bg_color #0a84ff;", "@define-color accent_fg_color #ffffff;", "@define-color accent_color #0a84ff;"]);
    }

    #[test]
    fn generated_themes_import_the_toolkit_sheet_for_the_mode_and_define_every_name_they_use() {
        for dark in [true, false] {
            let p = palette(dark);
            for kit in [Kit::Gtk3, Kit::Gtk4] {
                let t = theme_css(&p, kit);
                let want = match (kit, dark) {
                    (Kit::Gtk3, true) => "Adwaita/gtk-contained-dark.css",
                    (Kit::Gtk3, false) => "Adwaita/gtk-contained.css",
                    (Kit::Gtk4, true) => "Default/Default-dark.css",
                    (Kit::Gtk4, false) => "Default/Default-light.css",
                };
                assert!(t.lines().nth(1).unwrap().contains(want), "{kit:?} {dark}");
                let defined: Vec<&str> = t.lines().filter_map(|l| l.strip_prefix("@define-color ")).filter_map(|l| l.split(' ').next()).collect();
                let mut rest = t.as_str();
                while let Some(i) = rest.find('@') {
                    rest = &rest[i + 1..];
                    let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    if name.is_empty() || name == "import" || name == "define" {
                        continue;
                    }
                    assert!(defined.contains(&name.as_str()), "{kit:?}: @{name} is used but not defined");
                }
                // Colours only: the toolkit keeps sizes, paddings and radii (hit targets do not move).
                for forbidden in ["min-height", "min-width", "padding", "margin", "border-radius", "font-size"] {
                    assert!(!t.contains(forbidden), "{kit:?}: {forbidden}");
                }
                assert_eq!(t.matches('{').count(), t.matches('}').count());
                assert!(t.contains("button, button:backdrop {"), "{kit:?}: backdrop twins");
                assert!(!t.contains(":backdrop:backdrop"));
            }
        }
    }

    #[test]
    fn translucent_menus_use_the_theme_alpha_only_on_their_backgrounds_and_stay_readable() {
        let mut p = palette(true);
        p.popup_alpha = 0.82;
        let r = Roles::new(&p);
        let t = theme_css(&p, Kit::Gtk4);
        assert!(t.contains(&format!("@define-color decebal_popover_bg {};", rgba(p.popup, 0.82))));
        assert!(t.contains(&format!("@define-color theme_bg_color {};", p.bg.hex())), "windows stay opaque");
        for behind in [Rgb(0, 0, 0), Rgb(255, 255, 255)] {
            assert!(contrast(r.popover_fg, p.popup.over(behind, 0.82)) >= 4.5);
        }
        assert!(css(&p, true).contains(&format!("@define-color popover_bg_color {};", rgba(p.popup, 0.82))));
        assert!(css(&p, true).contains(&format!("@define-color dialog_bg_color {};", p.popup.hex())), "dialogs are content: opaque");
    }

    #[test]
    fn gtk4_profile_follows_theme_identity_and_explicit_preferences_win() {
        let cfg = Config::default();
        let mut p = palette(true);
        for (id, profile) in [("windows11", Some("explorer")), ("frutiger-aero", Some("explorer")), ("macos", Some("finder")), ("graphite", Some("finder")), ("nord", None)] {
            p.id = id.into();
            let sheet = css_with_profile(&p, &cfg, true);
            if let Some(profile) = profile {
                assert!(sheet.contains(&format!("Files visual profile: {profile}")));
            } else {
                assert!(!sheet.contains("Files visual profile:"));
            }
            assert!(!css_with_profile(&p, &cfg, false).contains("Files visual profile:"), "GTK3 layout remains native");
            for fixed in [super::super::file_manager::Style::Native, super::super::file_manager::Style::Explorer, super::super::file_manager::Style::Finder] {
                let explicit = Config { file_manager_style: fixed, ..Config::default() };
                let sheet = css_with_profile(&p, &explicit, true);
                if fixed == super::super::file_manager::Style::Native {
                    assert!(!sheet.contains("Files visual profile:"));
                } else {
                    assert!(sheet.contains(&format!("Files visual profile: {}", fixed.id())));
                }
            }
        }
    }

    #[test]
    fn separate_palettes_and_dynamic_colours_preserve_windows_file_profile() {
        struct Sandbox(std::path::PathBuf);
        impl Drop for Sandbox {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let directory = std::env::temp_dir().join(format!("decebal-follow-files-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&directory).unwrap();
        let sandbox = Sandbox(directory);
        let src = crate::theme::live::Source { shell_json: sandbox.0.join("decebal.json"), themes_dir: sandbox.0.join("themes") };
        for palette in ["catppuccin", "nord"] {
            let selection = crate::theme::Selection { name: "windows11".into(), palette: Some(palette.into()), mode: crate::theme::Mode::Dark, ..Default::default() };
            let theme = src.theme_for("windows11", &selection, true).unwrap();
            let p = Palette::from_theme(&theme);
            assert_eq!(p.id, "windows11");
            assert!(css_with_profile(&p, &Config::default(), true).contains("Files visual profile: explorer"));
        }
        let dynamic = crate::theme::Dynamic {
            enabled: true,
            source: crate::theme::model::Origin::Color(crate::theme::Rgb(90, 120, 200)),
            colors: crate::theme::extract::Extracted::from_color(crate::theme::Rgb(90, 120, 200)),
            ..Default::default()
        };
        let selection = crate::theme::Selection { name: "windows11".into(), dynamic: Some(dynamic), mode: crate::theme::Mode::Dark, ..Default::default() };
        let theme = src.theme_for("windows11", &selection, true).unwrap();
        assert_eq!(theme.id, "windows11");
        assert!(css_with_profile(&Palette::from_theme(&theme), &Config::default(), true).contains("Files visual profile: explorer"));
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
    fn the_gtk_font_walks_the_chain_to_an_installed_named_family_and_needs_the_option() {
        let db = crate::theme::fontdb::fake::db();
        let mut p = palette(true);
        p.font_pt = 11.0;
        p.ui_fonts.clear();
        p.mono_fonts.clear();
        let mut cfg = Config::default();
        p.font = Some("inter".into());
        assert_eq!(font_name(&p, &cfg, &db).as_deref(), Some("Inter 11"), "on by default, the installed spelling");
        p.font = Some("Segoe UI".into());
        assert_eq!(font_name(&p, &cfg, &db), None, "not installed");
        p.ui_fonts = vec!["Segoe UI Variable".into(), "sans-serif".into(), "Inter".into()];
        assert_eq!(font_name(&p, &cfg, &db).as_deref(), Some("Inter 11"), "the first installed family of the chain");
        p.ui_fonts.clear();
        for generic in ["sans-serif", "system-ui", "monospace"] {
            p.font = Some(generic.into());
            assert_eq!(font_name(&p, &cfg, &db), None, "{generic} names nothing explicit");
        }
        p.font = None;
        assert_eq!(font_name(&p, &cfg, &db), None);
        p.font = Some("Inter".into());
        cfg.set_option("gtk.font", false);
        assert_eq!(font_name(&p, &cfg, &db), None, "the user turned it off");
        p.mono_fonts = vec!["Inter".into()];
        assert_eq!(mono_font_name(&p, &cfg, &db), None, "the option covers the monospace font too");
        assert!(cfg.options.contains_key("gtk.font"));
        cfg.set_option("gtk.font", true);
        assert!(cfg.options.is_empty(), "the default is not stored");
        assert_eq!(font_name(&p, &cfg, &db).as_deref(), Some("Inter 11"));
        assert_eq!(mono_font_name(&p, &cfg, &db).as_deref(), Some("Inter 11"));
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
        assert!(!linked.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
        super::super::off(&env, &cfg, &palette(true)).unwrap();
        assert_eq!(std::fs::read_to_string(&linked).unwrap(), original);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn plan_covers_both_gtk_versions_the_generated_theme_and_gsettings() {
        let d = tmp("gtkplan");
        let (env, _) = env(&d, &[]);
        let plan = Gtk.plan(&palette(false), &Config::default(), &env);
        let paths: Vec<String> = plan.edits.iter().map(|e| e.path().strip_prefix(&env.home).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(
            paths,
            [
                ".local/share/themes/Decebal/index.theme",
                ".local/share/themes/Decebal/gtk-3.0/gtk.css",
                ".local/share/themes/Decebal/gtk-4.0/gtk.css",
                ".config/gtk-3.0/gtk.css",
                ".config/gtk-3.0/settings.ini",
                ".config/gtk-4.0/gtk.css",
                ".config/gtk-4.0/settings.ini"
            ]
        );
        assert!(plan.gsettings.contains(&("org.gnome.desktop.interface".into(), "color-scheme".into(), "'prefer-light'".into())));
        assert!(plan.gsettings.contains(&("org.gnome.desktop.interface".into(), "gtk-theme".into(), "'Decebal'".into())));
    }

    #[test]
    fn colour_changes_alternate_theme_slots_so_running_apps_reload_and_reapplying_changes_nothing() {
        let d = tmp("gtkslots");
        let (env, _) = env(&d, &[]);
        let cfg = Config::default();
        let only = super::super::Opts { only: Some(vec!["gtk".into()]), ..Default::default() };
        let ini = env.config_home.join("gtk-3.0/settings.ini");
        let name = || ini_get(&std::fs::read_to_string(&ini).unwrap(), "Settings", "gtk-theme-name").unwrap();
        super::super::apply(&env, &cfg, &palette(true), &only).unwrap();
        assert_eq!(name(), "Decebal");
        let again = super::super::apply(&env, &cfg, &palette(true), &only).unwrap();
        assert!(again.wrote.is_empty(), "same colours, same slot: {:?}", again.wrote);
        super::super::apply(&env, &cfg, &palette(false), &only).unwrap();
        assert_eq!(name(), "Decebal-Alt", "new colours go to the other slot so GTK sees a new theme name");
        assert!(std::fs::read_to_string(env.data_home.join("themes/Decebal-Alt/gtk-3.0/gtk.css")).unwrap().contains("gtk-contained.css"));
        super::super::apply(&env, &cfg, &palette(true), &only).unwrap();
        assert_eq!(name(), "Decebal");
        super::super::off(&env, &cfg, &palette(true)).unwrap();
        assert!(!env.data_home.join("themes/Decebal").exists() && !env.data_home.join("themes/Decebal-Alt").exists(), "off removes both generated themes");
    }

    /// GTK 3 live reload: a user-priority `@define-color` would outrank the reloaded theme's own names, so with the generated theme
    /// selected the GTK 3 block defines none; theme A and theme B leave the same block, the slot carries the palette, and `off`
    /// gives the user's own CSS back byte for byte.
    #[test]
    fn the_gtk3_user_block_never_pins_the_palette_while_the_generated_theme_is_selected() {
        let d = tmp("gtk3-live");
        let (env, _) = env(&d, &[]);
        let css3 = env.config_home.join("gtk-3.0/gtk.css");
        std::fs::create_dir_all(css3.parent().unwrap()).unwrap();
        let mine = "/* mine */\nlabel { font-weight: bold; }\n";
        std::fs::write(&css3, mine).unwrap();
        let cfg = Config::default();
        let only = super::super::Opts { only: Some(vec!["gtk".into()]), ..Default::default() };
        let (a, b) = (palette(true), palette(false));
        super::super::apply(&env, &cfg, &a, &only).unwrap();
        let after_a = std::fs::read_to_string(&css3).unwrap();
        assert!(after_a.starts_with(mine) && after_a.contains(GTK3_THEME_BLOCK.trim_end()), "user CSS kept, our block added");
        assert!(!after_a.contains("@define-color"), "no user-priority colour names");
        super::super::apply(&env, &cfg, &b, &only).unwrap();
        assert_eq!(std::fs::read_to_string(&css3).unwrap(), after_a, "theme B leaves the GTK 3 block as it was: nothing of A or B in it");
        let slot = THEME_SLOTS[1];
        assert!(std::fs::read_to_string(env.data_home.join("themes").join(slot).join("gtk-3.0/gtk.css")).unwrap().contains(&format!("@define-color theme_bg_color {};", b.bg.hex())));
        let css4 = std::fs::read_to_string(env.config_home.join("gtk-4.0/gtk.css")).unwrap();
        assert!(css4.contains(&format!("@define-color window_bg_color {};", b.bg.hex())) && !css4.contains("@define-color theme_bg_color"), "GTK 4 keeps libadwaita names only");
        super::super::off(&env, &cfg, &b).unwrap();
        assert_eq!(std::fs::read_to_string(&css3).unwrap(), mine, "off restores the user's own CSS");
        // with the generated theme switched off, the user's own GTK theme gets the named colours as before
        let mut own = Config::default();
        own.set_option("gtk.theme", false);
        assert!(user_css(&a, &own, false, false).contains(&format!("@define-color theme_bg_color {};", a.bg.hex())));
    }

    #[test]
    fn opting_out_of_widget_theming_keeps_the_users_gtk_theme() {
        let d = tmp("gtkoptout");
        let (env, _) = env(&d, &[]);
        let mut cfg = Config::default();
        cfg.set_option("gtk.theme", false);
        let plan = Gtk.plan(&palette(true), &cfg, &env);
        assert!(!plan.edits.iter().any(|e| e.path().starts_with(env.data_home.join("themes"))));
        assert!(!plan.gsettings.iter().any(|(_, k, _)| k == "gtk-theme"));
        for e in &plan.edits {
            if let Edit::Ini { sets, .. } = e {
                assert!(!sets.iter().any(|(_, k, _)| k == "gtk-theme-name"));
            }
        }
    }

    #[test]
    fn missing_icon_and_cursor_themes_are_reported_not_published() {
        let d = tmp("gtkicons");
        let (env, _) = env(&d, &[]);
        let cfg = Config { icon_theme: Some("Surely-Not-Installed".into()), cursor_theme: Some("No-Cursor".into()), ..Config::default() };
        let plan = Gtk.plan(&palette(true), &cfg, &env);
        assert_eq!(missing_themes(&cfg, &env).len(), 2);
        assert!(!plan.gsettings.iter().any(|(_, k, _)| k == "icon-theme" || k == "cursor-theme"));
        std::fs::create_dir_all(env.data_home.join("icons/Surely-Not-Installed")).unwrap();
        std::fs::write(env.data_home.join("icons/Surely-Not-Installed/index.theme"), "[Icon Theme]\nName=x\n").unwrap();
        std::fs::create_dir_all(env.home.join(".icons/No-Cursor/cursors")).unwrap();
        let plan = Gtk.plan(&palette(true), &cfg, &env);
        assert!(missing_themes(&cfg, &env).is_empty());
        assert!(plan.gsettings.contains(&("org.gnome.desktop.interface".into(), "icon-theme".into(), "'Surely-Not-Installed'".into())));
        assert!(plan.gsettings.contains(&("org.gnome.desktop.interface".into(), "cursor-theme".into(), "'No-Cursor'".into())));
    }
}
