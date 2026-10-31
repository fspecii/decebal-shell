//! The theme data model: tokens, TOML parsing and validation, variant resolution and overrides.
//!
//! A theme file is a [`Def`]: flat token tables for the colours, metrics and font, optionally split
//! into `[dark]` / `[light]` variants. [`Def::resolve`] picks a variant and fills every missing token
//! from the ones given, so a theme only has to name `background`, `text` and `accent`.

use std::collections::BTreeMap;
use tiny_skia::Color;

// ---------------------------------------------------------------- colours

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rgb`, `#rrggbb` (a trailing alpha pair is accepted and ignored; alpha lives in the metrics).
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.trim().strip_prefix('#').or_else(|| s.trim().strip_prefix("0x"))?;
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        match h.len() {
            3 => {
                let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
                Some(Rgb(d(0)?, d(1)?, d(2)?))
            }
            6 | 8 => Some(Rgb(p(0)?, p(2)?, p(4)?)),
            _ => None,
        }
    }

    /// `(r, g, b)` for the tuple-based drawing helpers.
    pub fn t(self) -> (u8, u8, u8) {
        (self.0, self.1, self.2)
    }

    /// `(r, g, b, alpha)` for the tuple-based drawing helpers.
    pub fn a(self, alpha: f32) -> (u8, u8, u8, f32) {
        (self.0, self.1, self.2, alpha)
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    pub fn color(self, alpha: f32) -> Color {
        crate::gfx::color(self.0, self.1, self.2, alpha)
    }

    /// `self` blended over `other`: `t` = 0 gives `other`, 1 gives `self`.
    pub fn over(self, other: Rgb, t: f32) -> Rgb {
        let m = |a: u8, b: u8| (b as f32 + (a as f32 - b as f32) * t.clamp(0.0, 1.0)).round() as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f32 {
        let l = |v: u8| {
            let c = v as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(self.0) + 0.7152 * l(self.1) + 0.0722 * l(self.2)
    }

    /// Black or white, whichever reads better on `self`.
    pub fn readable_on(self) -> Rgb {
        let (b, w) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        if contrast(self, b) >= contrast(self, w) {
            b
        } else {
            w
        }
    }
}

/// WCAG contrast ratio (1..21).
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (x, y) = (a.luminance(), b.luminance());
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

// ---------------------------------------------------------------- the resolved theme

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub dark: bool,
    /// Window / settings background.
    pub background: Rgb,
    /// Settings sidebar.
    pub sidebar: Rgb,
    /// Cards, fields, raised areas.
    pub surface: Rgb,
    pub bar: Rgb,
    pub bar_text: Rgb,
    /// Menus and popups.
    pub popup: Rgb,
    pub dock: Rgb,
    /// Spotlight / Launchpad card.
    pub overlay: Rgb,
    pub titlebar: Rgb,
    pub titlebar_inactive: Rgb,
    /// Hairlines between regions (solid, for rdwm; the shell draws its own with `ink`).
    pub border: Rgb,
    pub text: Rgb,
    pub text_secondary: Rgb,
    pub text_faint: Rgb,
    pub accent: Rgb,
    pub accent_text: Rgb,
    pub danger: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    // metrics
    pub radius: f32,
    pub dock_roundness: f32,
    pub dock_alpha: f32,
    pub popup_alpha: f32,
    pub panel_alpha: f32,
    pub bar_alpha: f32,
    pub blur: f32,
    pub glass_refraction: f32,
    pub glass_edge: f32,
    pub glass_highlight: f32,
    pub glass_grain: f32,
    pub glass_blur: f32,
    pub shadow: f32,
    pub outline: f32,
    pub dock_icon_size: f32,
    pub dock_padding: f32,
    pub bar_height: f32,
    pub titlebar_style: String,
    pub window_buttons_right: bool,
    pub titlebar_height: i64,
    /// `flat` (a full-width strip), `floating` (a horizontal pill with a margin around it) or `vertical`
    /// (caelestia's layout: a bar down the left screen edge, no dock).
    pub bar_style: String,
    pub bar_layout: crate::config::BarLayout,
    pub dashboard_style: String,
    pub dashboard_profile: String,
    pub launcher_style: String,
    /// The interface font as written (`fonts.primary_ui()`; empty = automatic), kept for older readers.
    /// Where the Caelestia pill bar sits (`decebal.json` `bar.position`); only meaningful while `bar_style` is `vertical`.
    /// Not part of the theme file: [`super::live::Source::build`] fills it in from the settings, so a change reaches every program through the theme watcher.
    pub bar_position: crate::config::BarPosition,
    /// `bar.position` as written, or `None` when `decebal.json` has none (then the theme package decides).
    pub bar_position_written: Option<crate::config::BarPosition>,
    /// The user's panel choices (`decebal.json` `bar.edge`, `bar.split`, `bar.applets`), filled in like `bar_position`.
    pub panel_user: crate::panel::UserPanel,
    /// The old `decebal.json` `workspaces` switch.
    pub panel_workspaces: crate::config::Toggle,
    /// What the theme package declares for its panels (`[bar] position` / `applets`, `[dock] position`).
    pub panel_theme: crate::panel::ThemePanel,
    /// The theme's logo and labels for the panels (`[branding]`); a package image is an absolute path inside the package.
    pub branding: crate::panel::branding::Branding,
    /// The Control Center's settings (`decebal.json` `control_center`, which a theme's `[control_center]` fills): filled in by
    /// [`super::live::Source::build`] like `bar_position`, so a change reaches every program through the theme watcher.
    pub control_center: crate::controlcenter::settings::Settings,
    pub font_family: String,
    pub font_scale: f32,
    /// The fonts per role with their fallback chains (see [`super::fonts`]).
    pub fonts: super::fonts::FontPlan,
}

impl Theme {
    /// Translucent overlay in the text colour: hover fills, hairlines, disabled text.
    pub fn ink(&self, a: f32) -> Color {
        self.text.color(a)
    }
    pub fn sep(&self) -> Color {
        self.ink(0.12)
    }
    pub fn hover(&self) -> Color {
        self.ink(0.10)
    }
    /// A dock may deliberately remain dark in a light desktop (GNOME's dash).
    /// Retain the palette foreground when readable, otherwise use the dock's contrasting ink.
    pub fn dock_foreground(&self) -> Rgb {
        if contrast(self.text, self.dock) >= 4.5 {
            self.text
        } else {
            self.dock.readable_on()
        }
    }
    pub fn dock_secondary(&self) -> Rgb {
        if contrast(self.text_secondary, self.dock) >= 3.0 {
            return self.text_secondary;
        }
        let ink = self.dock_foreground();
        let soft = ink.over(self.dock, 0.75);
        if contrast(soft, self.dock) >= 3.0 {
            soft
        } else {
            ink
        }
    }
    pub fn dock_ink(&self, alpha: f32) -> Color {
        self.dock_foreground().color(alpha)
    }
    pub fn outline_c(&self) -> Color {
        self.ink(self.outline)
    }
    pub fn text_c(&self) -> Color {
        self.text.color(1.0)
    }
    pub fn sub(&self) -> Color {
        self.text_secondary.color(1.0)
    }
    pub fn faint(&self) -> Color {
        self.text_faint.color(1.0)
    }
    pub fn accent_c(&self) -> Color {
        self.accent.color(1.0)
    }
    pub fn accent_text_c(&self) -> Color {
        self.accent_text.color(1.0)
    }
    pub fn popup_c(&self) -> Color {
        self.popup.color(self.popup_alpha)
    }
    pub fn overlay_c(&self) -> Color {
        self.overlay.color(self.panel_alpha)
    }
    pub fn bar_c(&self) -> Color {
        self.bar.color(self.bar_alpha)
    }
    /// Corner radius for a surface that is `factor` times a popup (popup = 1.0).
    pub fn r(&self, factor: f32) -> f32 {
        self.radius * factor
    }
    /// The menu bar is a floating pill instead of a full-width strip.
    pub fn bar_floating(&self) -> bool {
        self.bar_style == "floating"
    }
    /// The bar is the pill rail (caelestia's layout) instead of a strip across the screen.
    pub fn bar_vertical(&self) -> bool {
        self.bar_style == "vertical"
    }
    /// Which renderer draws decbar.
    pub fn bar_renderer(&self) -> crate::panel::Renderer {
        crate::panel::Renderer::for_bar_style(&self.bar_style)
    }
    /// decbar's effective panel: its edge and the applets it draws (see [`crate::panel::resolve`]).
    pub fn bar_panel(&self) -> crate::panel::Effective {
        let renderer = self.bar_renderer();
        crate::panel::resolve(&crate::panel::Inputs {
            renderer,
            user: &self.panel_user,
            theme: &self.panel_theme,
            legacy_position: self.bar_position_written,
            legacy_workspaces: crate::panel::legacy_workspaces(self.panel_workspaces),
            dux: Some(&self.bar_layout),
            dock_user_edge: None,
        })
    }
    /// The Windows taskbar's effective panel, for the dock settings `dock`.
    pub fn taskbar_panel(&self, dock: &crate::config::DockSettings) -> crate::panel::Effective {
        crate::panel::resolve(&crate::panel::Inputs {
            renderer: crate::panel::Renderer::Taskbar,
            user: &self.panel_user,
            theme: &self.panel_theme,
            // `windowsPosition` only ever held top or bottom; bottom is also its default, so only top says anything
            legacy_position: (dock.windows_position == crate::config::WinPosition::Top).then_some(crate::config::BarPosition::Top),
            legacy_workspaces: None,
            dux: None,
            dock_user_edge: dock.edge,
        })
    }
    /// The edge of decbar.
    pub fn bar_edge(&self) -> crate::panel::Edge {
        self.bar_panel().edge
    }
    /// Where the Caelestia pill bar sits, or `None` while the theme has another bar. (`bar_style = "vertical"` is
    /// the theme's name for the pill bar; the edge comes from [`Theme::bar_panel`].)
    pub fn pill_bar(&self) -> Option<crate::config::BarPosition> {
        if !self.bar_vertical() {
            return None;
        }
        let p = self.bar_panel();
        Some(if p.split { crate::config::BarPosition::Split } else { p.edge.position() })
    }
    /// Thickness of decbar's strip on `edge` (what it reserves).
    pub fn bar_thickness(&self, edge: crate::panel::Edge) -> f32 {
        use crate::panel::Renderer as R;
        match self.bar_renderer() {
            R::Pill => crate::theme::pill_bar_thickness(edge.position()),
            R::Dux => self.bar_height + 2.0 * self.bar_layout.margin,
            _ if edge.is_vertical() => crate::theme::CLASSIC_RAIL_THICKNESS,
            _ => crate::theme::BAR_HEIGHT,
        }
    }
    /// Pixels decbar takes from each screen edge: where other programs must keep clear of it. A classic or Dux bar on the
    /// top edge reports nothing here (its strut is what every program already expects, see [`crate::theme::bar_bottom_for`]).
    pub fn bar_insets(&self) -> Insets {
        use crate::panel::Edge as E;
        let p = self.bar_panel();
        let t = self.bar_thickness(p.edge) as i32;
        if self.bar_vertical() {
            if p.split {
                return Insets { top: t, bottom: t, ..Insets::default() };
            }
        } else if p.edge == E::Top {
            return Insets::default();
        }
        match p.edge {
            E::Left => Insets { left: t, ..Insets::default() },
            E::Right => Insets { right: t, ..Insets::default() },
            E::Top => Insets { top: t, ..Insets::default() },
            E::Bottom => Insets { bottom: t, ..Insets::default() },
        }
    }
}

/// Space reserved at the edges of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

// ---------------------------------------------------------------- raw definition (a theme file)

#[derive(Debug, Clone, PartialEq)]
pub enum V {
    S(String),
    N(f64),
    B(bool),
    /// A list of names (font chains).
    L(Vec<String>),
}

pub type Tokens = BTreeMap<String, V>;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Color,
    Num(f64, f64),
    Str,
    /// A string or an array of strings (font chains).
    List,
    Choice(&'static [&'static str]),
}

/// Every token a theme may set: (key, kind, description). The documentation and the validator share this table.
pub const TOKENS: &[(&str, Kind, &str)] = &[
    ("background", Kind::Color, "window and settings background"),
    ("sidebar", Kind::Color, "settings sidebar"),
    ("surface", Kind::Color, "cards, fields, raised areas"),
    ("bar", Kind::Color, "menu bar"),
    ("bar_text", Kind::Color, "menu bar text (default: text)"),
    ("popup", Kind::Color, "menus and popups"),
    ("dock", Kind::Color, "dock panel"),
    ("overlay", Kind::Color, "Spotlight and Launchpad card"),
    ("titlebar", Kind::Color, "focused window titlebar (rdwm)"),
    ("titlebar_inactive", Kind::Color, "unfocused window titlebar (rdwm)"),
    ("border", Kind::Color, "window border / hairline colour (rdwm)"),
    ("text", Kind::Color, "primary text; also the base of hover/separator overlays"),
    ("text_secondary", Kind::Color, "secondary text (default: text mixed 60% into the background)"),
    ("text_faint", Kind::Color, "hints and disabled text"),
    ("accent", Kind::Color, "selection / focus colour"),
    ("accent_text", Kind::Color, "text on the accent (default: black or white by contrast)"),
    ("danger", Kind::Color, "destructive actions, errors"),
    ("ok", Kind::Color, "success"),
    ("warn", Kind::Color, "warnings"),
    ("radius", Kind::Num(0.0, 24.0), "popup corner radius in px; windows and the control centre scale from it"),
    ("dock_roundness", Kind::Num(0.0, 1.0), "dock corner roundness (0 square, 1 pill); dock-settings.json overrides"),
    ("dock_alpha", Kind::Num(0.0, 1.0), "dock panel opacity; dock-settings.json `transparency` overrides"),
    ("popup_alpha", Kind::Num(0.5, 1.0), "menu opacity"),
    ("panel_alpha", Kind::Num(0.5, 1.0), "Spotlight / Launchpad / Control Center opacity"),
    ("bar_alpha", Kind::Num(0.0, 1.0), "menu bar opacity (needs a compositor below 1)"),
    ("blur", Kind::Num(0.0, 1.0), "frosted background on X11 with configured picom"),
    ("glass_refraction", Kind::Num(0.0, 32.0), "edge lens displacement in pixels; enables native X11 glass"),
    ("glass_edge", Kind::Num(2.0, 40.0), "width of the rounded glass bezel in pixels"),
    ("glass_highlight", Kind::Num(0.0, 1.0), "directional glass rim highlight strength"),
    ("glass_grain", Kind::Num(0.0, 0.03), "subtle deterministic material grain"),
    ("glass_blur", Kind::Num(0.0, 24.0), "native glass backdrop softening radius"),
    ("shadow", Kind::Num(0.0, 1.5), "drop shadow strength"),
    ("outline", Kind::Num(0.0, 1.0), "alpha of the 1px outline around popups"),
    ("dock_icon_size", Kind::Num(24.0, 128.0), "dock icon size in px (consumed by the dock layout)"),
    ("dock_padding", Kind::Num(4.0, 40.0), "dock panel padding in px (consumed by the dock layout)"),
    ("bar_height", Kind::Num(20.0, 80.0), "menu bar height in px (consumed by the bar layout)"),
    (
        "bar_style",
        Kind::Choice(&["flat", "floating", "vertical", "dux"]),
        "menu bar shape: a full-width strip, a floating pill, or the Caelestia pill bar on a screen edge (left by default, see `bar.position`; the dock hides with it)",
    ),
    ("dashboard_style", Kind::Choice(&["default", "dux"]), "native dashboard composition"),
    ("launcher_style", Kind::Choice(crate::launcher::style::KEYS), "native application launcher composition (the user's Settings choice overrides it)"),
    ("dashboard_profile", Kind::Choice(&["readme"]), "configured Dux README dashboard profile; not current upstream defaults"),
    ("titlebar_style", Kind::Choice(&["macos", "classic"]), "rdwm titlebar style"),
    ("titlebar_height", Kind::Num(16.0, 60.0), "rdwm titlebar height in px"),
    ("font_family", Kind::Str, "the font of text, titles and display numbers (fontconfig family); unavailable fonts fall back to the preset, Inter and sans-serif"),
    ("font_scale", Kind::Num(0.8, 1.4), "text size multiplier"),
    (
        "font_preset",
        Kind::Choice(super::fonts::PRESET_IDS),
        "font preset: the fonts and fallbacks of every role (system, inter, segoe, cantarell, rubik, jetbrains, ibm-plex, fira, noto, monospace-ui)",
    ),
    ("font_ui", Kind::List, "default text: a family or a list (first installed wins, later ones draw characters it lacks)"),
    ("font_title", Kind::List, "window and popup titles (default: the preset's)"),
    ("font_mono", Kind::List, "terminal-like text, aligned numbers"),
    ("font_display", Kind::List, "large numbers: clock and workspace digits"),
];

fn kind_of(key: &str) -> Option<&'static Kind> {
    TOKENS.iter().find(|t| t.0 == key).map(|t| &t.1)
}

/// A parsed theme file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Def {
    pub name: String,
    pub description: String,
    pub inherit: Option<String>,
    /// Used when the file has neither `[dark]` nor `[light]`.
    pub dark_default: bool,
    pub common: Tokens,
    pub dark: Option<Tokens>,
    pub light: Option<Tokens>,
    /// Everything a format-1 theme package says beyond colours and metrics (`decoration`, `bar`, `dock`, `widgets`...),
    /// as JSON; see `theme::engine`. Empty for legacy theme files.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn read_value(table: &toml_edit::Table, key: &str, label: &str, out: &mut Tokens) -> Result<(), String> {
    let Some(item) = table.get(key) else { return Ok(()) };
    let kind = kind_of(label).ok_or_else(|| format!("unknown key `{label}`"))?;
    let v = item.as_value().ok_or_else(|| format!("`{label}` must be a value"))?;
    let tok = match kind {
        Kind::Color => {
            let s = v.as_str().ok_or_else(|| format!("`{label}` must be a colour string like \"#1e1e1e\""))?;
            Rgb::parse(s).ok_or_else(|| format!("`{label}`: `{s}` is not a colour (#rgb or #rrggbb)"))?;
            V::S(s.to_string())
        }
        Kind::Num(lo, hi) => {
            let n = v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).ok_or_else(|| format!("`{label}` must be a number"))?;
            if !(*lo..=*hi).contains(&n) {
                return Err(format!("`{label}` = {n} is outside {lo}..{hi}"));
            }
            V::N(n)
        }
        Kind::Str => V::S(v.as_str().ok_or_else(|| format!("`{label}` must be a string"))?.to_string()),
        Kind::List => {
            let bad = || format!("`{label}` must be a font name or a list of font names");
            let list: Vec<String> = match (v.as_str(), v.as_array()) {
                (Some(s), _) => super::fonts::parse_chain(s),
                (_, Some(a)) => a.iter().map(|x| x.as_str().map(super::fonts::parse_chain).ok_or_else(bad)).collect::<Result<Vec<_>, _>>()?.into_iter().flatten().collect(),
                _ => return Err(bad()),
            };
            if list.is_empty() {
                return Err(format!("`{label}` needs at least one font name"));
            }
            V::L(list)
        }
        Kind::Choice(opts) => {
            let s = v.as_str().ok_or_else(|| format!("`{label}` must be a string"))?;
            if !opts.contains(&s) {
                return Err(format!("`{label}` must be one of {}", opts.join(", ")));
            }
            V::S(s.to_string())
        }
    };
    out.insert(label.to_string(), tok);
    Ok(())
}

/// Reads every key of `table` as a token, `prefix` mapping `[font] family` to `font_family`.
fn read_table(table: &toml_edit::Table, prefix: &str, out: &mut Tokens) -> Result<(), String> {
    for (k, item) in table.iter() {
        if prefix.is_empty() && k == "font" && item.is_table() {
            continue; // a font table inside [dark] / [light]: read by the caller
        }
        let label = if prefix.is_empty() { k.to_string() } else { format!("{prefix}_{k}") };
        read_value(table, k, &label, out)?;
    }
    Ok(())
}

impl Def {
    pub fn parse(text: &str) -> Result<Def, String> {
        if super::engine::manifest::is_v1(text) {
            return super::engine::manifest::parse_def(text);
        }
        let doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| format!("invalid TOML: {e}"))?;
        let root = doc.as_table();
        let mut d = Def { dark_default: true, ..Def::default() };
        for (k, item) in root.iter() {
            match k {
                "name" => d.name = item.as_str().ok_or("`name` must be a string")?.to_string(),
                "description" => d.description = item.as_str().ok_or("`description` must be a string")?.to_string(),
                "inherit" => d.inherit = Some(item.as_str().ok_or("`inherit` must be a theme id")?.to_string()),
                "dark" if item.is_value() => d.dark_default = item.as_bool().ok_or("`dark` must be true or false")?,
                "colors" | "metrics" => read_table(item.as_table().ok_or_else(|| format!("[{k}] must be a table"))?, "", &mut d.common)?,
                "font" => read_table(item.as_table().ok_or("[font] must be a table")?, "font", &mut d.common)?,
                "dark" | "light" => {
                    let mut t = Tokens::new();
                    let tab = item.as_table().ok_or_else(|| format!("[{k}] must be a table"))?;
                    read_table(tab, "", &mut t)?;
                    if let Some(f) = tab.get("font").and_then(|f| f.as_table()) {
                        t.remove("font");
                        read_table(f, "font", &mut t)?;
                    }
                    if k == "dark" {
                        d.dark = Some(t);
                    } else {
                        d.light = Some(t);
                    }
                }
                other => return Err(format!("unknown section or key `{other}`")),
            }
        }
        if d.name.trim().is_empty() {
            return Err("`name` is required".into());
        }
        Ok(d)
    }

    /// `base` underneath `self`: tokens of `self` win, variants merge key by key.
    pub fn over(&self, base: &Def) -> Def {
        let merge = |a: &Option<Tokens>, b: &Option<Tokens>| match (a, b) {
            (None, None) => None,
            (a, b) => {
                let mut m = b.clone().unwrap_or_default();
                m.extend(a.clone().unwrap_or_default());
                Some(m)
            }
        };
        let mut common = base.common.clone();
        common.extend(self.common.clone());
        Def {
            name: self.name.clone(),
            description: if self.description.is_empty() { base.description.clone() } else { self.description.clone() },
            inherit: None,
            dark_default: self.dark_default,
            common,
            dark: merge(&self.dark, &base.dark),
            light: merge(&self.light, &base.light),
            extra: super::engine::manifest::merge_extra(&base.extra, &self.extra),
        }
    }

    pub fn has_dark(&self) -> bool {
        self.dark.is_some() || (self.light.is_none() && self.dark_default)
    }
    pub fn has_light(&self) -> bool {
        self.light.is_some() || (self.dark.is_none() && !self.dark_default)
    }

    /// Which variant to use for `mode`; `system_dark` answers "auto". Single-variant themes ignore the mode.
    pub fn pick_dark(&self, mode: Mode, system_dark: bool) -> bool {
        match (self.has_dark(), self.has_light()) {
            (true, false) => true,
            (false, true) => false,
            _ => match mode {
                Mode::Dark => true,
                Mode::Light => false,
                Mode::Auto => system_dark,
            },
        }
    }

    /// Builds the theme for the chosen variant; `id` names it. Missing colours are derived.
    pub fn resolve(&self, id: &str, dark: bool, o: &Selection) -> Theme {
        // a palette replaces the colours, the theme keeps everything else
        if let Some(p) = o.palette.as_deref().and_then(super::palette::load) {
            let o2 = Selection { palette: None, ..o.clone() };
            return self.with_palette(&p).resolve(id, dark, &o2);
        }
        let var = if dark { &self.dark } else { &self.light };
        let get = |k: &str| var.as_ref().and_then(|t| t.get(k)).or_else(|| self.common.get(k));
        let col = |k: &str| match get(k) {
            Some(V::S(s)) => Rgb::parse(s),
            _ => None,
        };
        let num = |k: &str, def: f64| match get(k) {
            Some(V::N(n)) => *n,
            _ => def,
        };
        let text_of = |k: &str| match get(k) {
            Some(V::S(s)) => Some(s.clone()),
            _ => None,
        };
        let (fb_bg, fb_text, fb_accent) =
            if dark { (Rgb(0x1e, 0x1e, 0x1e), Rgb(0xf5, 0xf5, 0xf7), Rgb(0x0a, 0x84, 0xff)) } else { (Rgb(0xf2, 0xf2, 0xf4), Rgb(0x1c, 0x1c, 0x1e), Rgb(0x00, 0x62, 0xcc)) };
        let background = col("background").unwrap_or(fb_bg);
        let text = col("text").unwrap_or(fb_text);
        let mut accent = col("accent").unwrap_or(fb_accent);
        let mut accent_text = col("accent_text");
        if let Some(a) = o.accent {
            accent = a;
            accent_text = None; // re-derived for the new colour
        }
        let accent_text = accent_text.unwrap_or_else(|| accent.readable_on());
        let surface = col("surface").unwrap_or_else(|| text.over(background, 0.07));
        let popup = col("popup").unwrap_or(surface);
        let titlebar = col("titlebar").unwrap_or(surface);
        let opacity = o.opacity.unwrap_or(1.0).clamp(0.4, 1.0);
        let clampf = |v: f64, lo: f64, hi: f64| v.clamp(lo, hi) as f32;
        let list_of = |k: &str| match get(k) {
            Some(V::L(l)) => Some(l.clone()),
            _ => None,
        };
        let theme_fonts = super::fonts::ThemeFonts {
            preset: text_of("font_preset"),
            family: text_of("font_family").filter(|f| !f.is_empty()),
            roles: [list_of("font_ui"), list_of("font_title"), list_of("font_mono"), list_of("font_display")],
        };
        let fonts = super::fonts::FontPlan::build(&theme_fonts, &o.fonts, o.font.as_deref().filter(|f| !f.is_empty()));
        let font_scale = o.font_scale.map(f64::from).unwrap_or_else(|| num("font_scale", 1.0));
        Theme {
            id: id.to_string(),
            name: self.name.clone(),
            dark,
            background,
            sidebar: col("sidebar").unwrap_or_else(|| if dark { Rgb(0, 0, 0).over(background, 0.15) } else { text.over(background, 0.05) }),
            surface,
            bar: col("bar").unwrap_or(background),
            bar_text: col("bar_text").unwrap_or(text),
            popup,
            dock: col("dock").unwrap_or(background),
            overlay: col("overlay").unwrap_or(popup),
            titlebar,
            titlebar_inactive: col("titlebar_inactive").unwrap_or(background),
            border: col("border").unwrap_or_else(|| text.over(background, 0.16)),
            text,
            text_secondary: col("text_secondary").unwrap_or_else(|| text.over(background, 0.62)),
            text_faint: col("text_faint").unwrap_or_else(|| text.over(background, 0.42)),
            accent,
            accent_text,
            danger: col("danger").unwrap_or(if dark { Rgb(0xff, 0x45, 0x3a) } else { Rgb(0xd7, 0x00, 0x15) }),
            ok: col("ok").unwrap_or(if dark { Rgb(0x30, 0xd1, 0x58) } else { Rgb(0x1d, 0x8f, 0x3c) }),
            warn: col("warn").unwrap_or(if dark { Rgb(0xff, 0x9f, 0x0a) } else { Rgb(0xb2, 0x50, 0x00) }),
            radius: o.radius.unwrap_or_else(|| num("radius", 10.0) as f32).clamp(0.0, 24.0),
            dock_roundness: clampf(num("dock_roundness", 0.45), 0.0, 1.0),
            dock_alpha: clampf(num("dock_alpha", 0.6) * opacity as f64, 0.0, 1.0),
            popup_alpha: clampf(num("popup_alpha", 0.96) * opacity as f64, 0.3, 1.0),
            panel_alpha: clampf(num("panel_alpha", 0.97) * opacity as f64, 0.3, 1.0),
            bar_alpha: clampf(num("bar_alpha", 1.0), 0.0, 1.0),
            blur: clampf(num("blur", 0.0), 0.0, 1.0),
            glass_refraction: clampf(num("glass_refraction", 0.0), 0.0, 32.0),
            glass_edge: clampf(num("glass_edge", 12.0), 2.0, 40.0),
            glass_highlight: clampf(num("glass_highlight", 0.35), 0.0, 1.0),
            glass_grain: clampf(num("glass_grain", 0.003), 0.0, 0.03),
            glass_blur: clampf(num("glass_blur", 4.0), 0.0, 24.0),
            shadow: clampf(num("shadow", 1.0), 0.0, 1.5),
            outline: clampf(num("outline", 0.14), 0.0, 1.0),
            dock_icon_size: clampf(num("dock_icon_size", 47.0), 24.0, 128.0),
            dock_padding: clampf(num("dock_padding", 16.0), 4.0, 40.0),
            bar_height: clampf(num("bar_height", if text_of("bar_style").as_deref() == Some("dux") { 38.0 } else { 26.0 }), 20.0, 80.0),
            bar_style: text_of("bar_style").unwrap_or_else(|| "flat".into()),
            bar_layout: crate::config::BarLayout::default(),
            dashboard_style: text_of("dashboard_style").unwrap_or_else(|| "default".into()),
            dashboard_profile: text_of("dashboard_profile").unwrap_or_else(|| "readme".into()),
            launcher_style: text_of("launcher_style").unwrap_or_else(|| "default".into()),
            bar_position: crate::config::BarPosition::default(),
            bar_position_written: None,
            panel_user: Default::default(),
            panel_workspaces: crate::config::Toggle::Word(crate::config::Word::Auto),
            panel_theme: crate::panel::ThemePanel::from_extra(&self.extra),
            branding: crate::panel::branding::Branding::from_extra(&self.extra),
            control_center: Default::default(),
            window_buttons_right: self
                .extra
                .get("decoration")
                .and_then(|v| v.get("buttons"))
                .and_then(serde_json::Value::as_str)
                .filter(|s| matches!(*s, "left" | "right"))
                .map(|s| s == "right")
                .unwrap_or_else(|| matches!(get("titlebar_style"),Some(V::S(s)) if s=="classic")),
            titlebar_style: text_of("titlebar_style").unwrap_or_else(|| "macos".into()),
            titlebar_height: num("titlebar_height", 28.0).round() as i64,
            font_family: fonts.primary_ui().to_string(),
            font_scale: (font_scale as f32).clamp(0.8, 1.4),
            fonts,
        }
    }
}

// ---------------------------------------------------------------- selection (what the user chose)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    Light,
    #[default]
    Dark,
    Auto,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Mode::Light),
            "dark" => Some(Mode::Dark),
            "auto" | "system" => Some(Mode::Auto),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
            Mode::Auto => "auto",
        }
    }
}

/// How far "match wallpaper" reaches into the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Surfaces {
    /// Only the accent (and the text on it); every surface keeps the theme's own colour.
    Accent,
    /// The accent plus surfaces, borders and text tinted towards the wallpaper's hue: the theme keeps its own
    /// light and dark tones (a white Windows 11 stays white) and gains a wallpaper-coloured cast.
    #[default]
    Tint,
    /// The theme's colours blend into the wallpaper's whole Material palette (at full strength: replaced by it).
    Full,
}

impl Surfaces {
    pub const ALL: [Surfaces; 3] = [Surfaces::Accent, Surfaces::Tint, Surfaces::Full];
    pub fn parse(s: &str) -> Option<Surfaces> {
        match s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "accent" | "accentonly" => Some(Surfaces::Accent),
            "tint" | "tinted" => Some(Surfaces::Tint),
            "full" | "all" | "palette" | "fullsurface" => Some(Surfaces::Full),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Surfaces::Accent => "accent",
            Surfaces::Tint => "tint",
            Surfaces::Full => "full",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Surfaces::Accent => "Accent only",
            Surfaces::Tint => "Tinted surfaces",
            Surfaces::Full => "Full palette",
        }
    }
}

/// Where the colours come from.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Origin {
    /// Whatever wallpaper rdwm shows (else the one in rdwm's config, else Omarchy's current background).
    #[default]
    Wallpaper,
    /// A fixed image file.
    File(String),
    /// A fixed colour.
    Color(Rgb),
}

impl Origin {
    pub fn parse(s: &str) -> Option<Origin> {
        let s = s.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("wallpaper") || s.eq_ignore_ascii_case("current") {
            return Some(Origin::Wallpaper);
        }
        if s.starts_with('#') {
            return Rgb::parse(s).map(Origin::Color);
        }
        Some(Origin::File(s.to_string()))
    }
    pub fn to_text(&self) -> String {
        match self {
            Origin::Wallpaper => "wallpaper".into(),
            Origin::File(p) => p.clone(),
            Origin::Color(c) => c.hex(),
        }
    }
}

/// "Match wallpaper": an optional layer on top of any theme. The colours of a wallpaper (or an image, or one colour)
/// are turned into a Material-style palette that recolours the accent and, depending on [`Surfaces`], the surfaces.
///
/// What is stored is the *result of looking at the picture* (`colors`) next to the settings, so every program
/// derives the same palette from a few words of JSON instead of decoding an image. Whoever changes the wallpaper
/// (`decthemectl watch`, `refresh`, the Theme pane, dechaven) refreshes `colors`; the settings can be changed
/// without touching the image again. Off by default: with no `dynamic` key, or `enabled: false`, nothing changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Dynamic {
    pub enabled: bool,
    pub scheme: super::material::Variant,
    pub source: Origin,
    pub surfaces: Surfaces,
    /// 0..1: how far surfaces move (towards the tint or the full palette). Accent-only ignores it.
    pub strength: f32,
    /// -1..1: Material's contrast level for the derived palette.
    pub contrast: f32,
    /// Light or dark follows the picture's brightness instead of the Mode setting.
    pub follow_mode: bool,
    /// What the picture looked like when it was last read.
    pub colors: super::extract::Extracted,
    /// The image `colors` came from.
    pub image: Option<String>,
}

impl Default for Dynamic {
    fn default() -> Self {
        Dynamic {
            enabled: false,
            scheme: Default::default(),
            source: Origin::Wallpaper,
            surfaces: Surfaces::Tint,
            strength: 0.5,
            contrast: 0.0,
            follow_mode: false,
            colors: Default::default(),
            image: None,
        }
    }
}

impl Dynamic {
    /// `legacy_on` is what an old file (written before the feature could be switched off: it has no `enabled` key)
    /// means for the theme it sits next to: it only ever recoloured Caelestia.
    pub fn from_json(v: &serde_json::Value, legacy_on: bool) -> Option<Dynamic> {
        let o = v.as_object()?;
        let colors = super::extract::Extracted::from_json_fields(o)?;
        let legacy = !o.contains_key("enabled") && !o.contains_key("surfaces");
        let mut source = o.get("source").and_then(|x| x.as_str()).and_then(Origin::parse).unwrap_or_default();
        // old files: `follow: false` pinned the seed to the image it came from
        if o.get("follow").and_then(|x| x.as_bool()) == Some(false) && source == Origin::Wallpaper {
            source = o.get("wallpaper").and_then(|x| x.as_str()).filter(|w| !w.is_empty()).map(|w| Origin::File(w.into())).unwrap_or(Origin::Color(colors.seed));
        }
        Some(Dynamic {
            enabled: o.get("enabled").and_then(|x| x.as_bool()).unwrap_or(legacy && legacy_on),
            scheme: o.get("scheme").and_then(|x| x.as_str()).and_then(super::material::Variant::parse).unwrap_or_default(),
            source,
            surfaces: o.get("surfaces").and_then(|x| x.as_str()).and_then(Surfaces::parse).unwrap_or(if legacy { Surfaces::Full } else { Surfaces::Tint }),
            strength: o.get("strength").and_then(|x| x.as_f64()).map_or(if legacy { 1.0 } else { 0.5 }, |x| x.clamp(0.0, 1.0) as f32),
            contrast: o.get("contrast").and_then(|x| x.as_f64()).map_or(0.0, |x| x.clamp(-1.0, 1.0) as f32),
            follow_mode: o.get("mode").and_then(|x| x.as_str()) == Some("wallpaper"),
            image: o.get("image").or_else(|| o.get("wallpaper")).and_then(|x| x.as_str()).filter(|w| !w.is_empty()).map(String::from),
            colors,
        })
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert("enabled".into(), self.enabled.into());
        o.insert("scheme".into(), self.scheme.as_str().into());
        o.insert("source".into(), self.source.to_text().into());
        o.insert("surfaces".into(), self.surfaces.as_str().into());
        let r2 = |x: f32| (f64::from(x) * 100.0).round() / 100.0;
        o.insert("strength".into(), r2(self.strength).into());
        o.insert("contrast".into(), r2(self.contrast).into());
        o.insert("mode".into(), (if self.follow_mode { "wallpaper" } else { "keep" }).into());
        self.colors.to_json_fields(&mut o);
        if let Some(w) = &self.image {
            o.insert("image".into(), w.clone().into());
        }
        serde_json::Value::Object(o)
    }

    /// The palette of the stored colours for one variant: what the Theme pane previews and what recolours the theme.
    pub fn roles(&self, dark: bool) -> super::material::Roles {
        // a hand-picked colour is never "grey"; a grey picture only matters for the monochrome scheme
        super::material::generate_with(self.colors.seed, &self.colors.others, self.scheme, dark, f64::from(self.contrast))
    }

    /// True when the picture has colour to match, or the scheme needs none (monochrome).
    pub fn has_colour(&self) -> bool {
        !self.colors.grey || self.scheme == super::material::Variant::Monochrome
    }
}

/// The user's choice: a theme, a mode, and optional per-key overrides. Stored under `"theme"` in `decebal.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub name: String,
    pub mode: Mode,
    pub accent: Option<Rgb>,
    /// A colour palette laid over the theme (see `theme::palette`); `None` = the theme's own colours.
    pub palette: Option<String>,
    /// Multiplies the translucency-capable surfaces (0.4..1).
    pub opacity: Option<f32>,
    pub radius: Option<f32>,
    /// The old single font family (`"font": "Inter"`): the font of text, titles and display numbers.
    pub font: Option<String>,
    pub font_scale: Option<f32>,
    /// Font preset and per-role chains (the `font` object).
    pub fonts: super::fonts::FontChoice,
    /// Also restyle rdwm's titlebars when applying (decthemectl / decsettings).
    pub sync_rdwm: bool,
    /// "Match wallpaper" (any theme); `None` or `enabled: false` = off, the default.
    pub dynamic: Option<Dynamic>,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            name: crate::theme::builtin::DEFAULT_ID.into(),
            mode: Mode::Dark,
            accent: None,
            palette: None,
            opacity: None,
            radius: None,
            font: None,
            font_scale: None,
            fonts: Default::default(),
            sync_rdwm: true,
            dynamic: None,
        }
    }
}

impl Selection {
    /// From the `"theme"` value of decebal.json: a bare name or an object. Bad fields are ignored.
    pub fn from_json(v: &serde_json::Value) -> Selection {
        let mut s = Selection::default();
        if let Some(name) = v.as_str() {
            s.name = name.to_string();
            return s;
        }
        let Some(o) = v.as_object() else { return s };
        if let Some(n) = o.get("name").and_then(|x| x.as_str()).filter(|n| !n.is_empty()) {
            s.name = n.to_string();
        }
        if let Some(m) = o.get("mode").and_then(|x| x.as_str()).and_then(Mode::parse) {
            s.mode = m;
        }
        s.accent = o.get("accent").and_then(|x| x.as_str()).and_then(Rgb::parse);
        s.palette = o.get("palette").and_then(|x| x.as_str()).filter(|p| !p.is_empty() && *p != "none").map(String::from);
        s.opacity = o.get("opacity").and_then(|x| x.as_f64()).map(|x| x.clamp(0.4, 1.0) as f32);
        s.radius = o.get("radius").and_then(|x| x.as_f64()).map(|x| x.clamp(0.0, 24.0) as f32);
        // `"font": "Inter"` (old) or `"font": {"preset": "segoe", "ui": [...], "title": ..., "mono": ..., "display": ..., "scale": 1.1}`
        let mut scale = o.get("fontScale").and_then(|x| x.as_f64());
        match o.get("font") {
            Some(serde_json::Value::String(f)) => s.font = Some(f.clone()).filter(|f| !f.is_empty()),
            Some(serde_json::Value::Object(f)) => {
                s.font = f.get("family").and_then(|x| x.as_str()).filter(|f| !f.is_empty()).map(String::from);
                s.fonts = super::fonts::FontChoice::from_json(f);
                scale = f.get("scale").and_then(|x| x.as_f64()).or(scale);
            }
            _ => {}
        }
        s.font_scale = scale.map(|x| x.clamp(0.8, 1.4) as f32);
        if let Some(b) = o.get("syncRdwm").and_then(|x| x.as_bool()) {
            s.sync_rdwm = b;
        }
        let on_caelestia = crate::theme::builtin::norm(&s.name) == crate::theme::builtin::DYNAMIC_THEME;
        s.dynamic = o.get("dynamic").and_then(|d| Dynamic::from_json(d, on_caelestia));
        s
    }

    /// Object form for decebal.json. Unset overrides are omitted.
    pub fn to_json(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert("name".into(), self.name.clone().into());
        o.insert("mode".into(), self.mode.as_str().into());
        if let Some(a) = self.accent {
            o.insert("accent".into(), a.hex().into());
        }
        if let Some(p) = &self.palette {
            o.insert("palette".into(), p.clone().into());
        }
        let r3 = |x: f32| (x as f64 * 1000.0).round() / 1000.0;
        if let Some(x) = self.opacity {
            o.insert("opacity".into(), r3(x).into());
        }
        if let Some(x) = self.radius {
            o.insert("radius".into(), r3(x).into());
        }
        if self.fonts.is_empty() {
            // old form: stays readable by older builds
            if let Some(f) = &self.font {
                o.insert("font".into(), f.clone().into());
            }
            if let Some(x) = self.font_scale {
                o.insert("fontScale".into(), r3(x).into());
            }
        } else {
            let mut f = serde_json::Map::new();
            self.fonts.write_json(&mut f);
            if let Some(fam) = &self.font {
                f.insert("family".into(), fam.clone().into());
            }
            if let Some(x) = self.font_scale {
                f.insert("scale".into(), r3(x).into());
            }
            o.insert("font".into(), f.into());
        }
        if !self.sync_rdwm {
            o.insert("syncRdwm".into(), false.into());
        }
        if let Some(d) = &self.dynamic {
            o.insert("dynamic".into(), d.to_json());
        }
        serde_json::Value::Object(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse() {
        assert_eq!(Rgb::parse("#0a84ff"), Some(Rgb(10, 132, 255)));
        assert_eq!(Rgb::parse("#fff"), Some(Rgb(255, 255, 255)));
        assert_eq!(Rgb::parse("0x112233"), Some(Rgb(0x11, 0x22, 0x33)));
        assert_eq!(Rgb::parse("#11223344"), Some(Rgb(0x11, 0x22, 0x33)));
        assert_eq!(Rgb::parse("blue"), None);
        assert_eq!(Rgb::parse("#12"), None);
        assert_eq!(Rgb::parse("#gggggg"), None);
        assert_eq!(Rgb(1, 2, 255).hex(), "#0102ff");
    }

    #[test]
    fn contrast_extremes() {
        assert!((contrast(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((contrast(Rgb(9, 9, 9), Rgb(9, 9, 9)) - 1.0).abs() < 1e-3);
        assert_eq!(Rgb(255, 255, 0).readable_on(), Rgb(0, 0, 0));
        assert_eq!(Rgb(0, 0, 80).readable_on(), Rgb(255, 255, 255));
    }

    const MINI: &str = "name = \"Mini\"\n[dark]\nbackground = \"#101010\"\ntext = \"#eeeeee\"\naccent = \"#ff0000\"\n";

    #[test]
    fn minimal_theme_derives_everything() {
        let d = Def::parse(MINI).unwrap();
        let t = d.resolve("mini", true, &Selection::default());
        assert_eq!(t.background, Rgb(16, 16, 16));
        assert_eq!(t.bar, t.background);
        assert_eq!(t.popup, t.surface);
        assert_ne!(t.surface, t.background);
        assert!(contrast(t.text_secondary, t.background) > 4.0);
        assert_eq!(t.accent_text, t.accent.readable_on());
    }

    #[test]
    fn bar_style_defaults_to_flat_and_only_floating_themes_float() {
        let d = Def::parse(MINI).unwrap();
        assert!(!d.resolve("m", true, &Selection::default()).bar_floating());
        let f = Def::parse("name = \"f\"\n[metrics]\nbar_style = \"floating\"\n").unwrap();
        assert!(f.resolve("f", true, &Selection::default()).bar_floating());
        let v = Def::parse("name = \"v\"\n[metrics]\nbar_style = \"vertical\"\n").unwrap().resolve("v", true, &Selection::default());
        assert!(v.bar_vertical() && !v.bar_floating());
        assert!(!d.resolve("m", true, &Selection::default()).bar_vertical());
    }

    #[test]
    fn rejects_bad_files() {
        assert!(Def::parse("name = \"x\"\n[dark]\nbackground = \"red\"\n").unwrap_err().contains("not a colour"));
        assert!(Def::parse("name = \"x\"\n[metrics]\nradius = 99\n").unwrap_err().contains("outside"));
        assert!(Def::parse("name = \"x\"\n[metrics]\nbogus = 1\n").unwrap_err().contains("unknown key"));
        assert!(Def::parse("name = \"x\"\nfoo = 1\n").unwrap_err().contains("unknown section"));
        assert!(Def::parse("[dark]\ntext = \"#fff\"\n").unwrap_err().contains("name"));
        assert!(Def::parse("name = \"x\"\n[metrics]\ntitlebar_style = \"gnome\"\n").unwrap_err().contains("one of"));
        assert!(Def::parse("name = \"x\"\n[metrics]\nbar_style = \"round\"\n").unwrap_err().contains("flat, floating, vertical"));
        assert!(Def::parse("name = ").unwrap_err().contains("invalid TOML"));
    }

    #[test]
    fn variants_and_modes() {
        let both = Def::parse("name = \"b\"\n[dark]\ntext = \"#fff\"\n[light]\ntext = \"#000\"\n").unwrap();
        assert!(both.pick_dark(Mode::Dark, false));
        assert!(!both.pick_dark(Mode::Light, true));
        assert!(both.pick_dark(Mode::Auto, true));
        assert!(!both.pick_dark(Mode::Auto, false));
        let dark_only = Def::parse(MINI).unwrap();
        assert!(dark_only.pick_dark(Mode::Light, false), "single-variant themes ignore the mode");
        let light_only = Def::parse("name = \"l\"\n[light]\ntext = \"#000\"\n").unwrap();
        assert!(!light_only.pick_dark(Mode::Dark, true));
        let flat = Def::parse("name = \"f\"\ndark = false\n[colors]\nbackground = \"#ffffff\"\n").unwrap();
        assert!(!flat.pick_dark(Mode::Auto, true));
        assert_eq!(flat.resolve("f", false, &Selection::default()).background, Rgb(255, 255, 255));
    }

    #[test]
    fn inherit_merges_key_by_key() {
        let base = Def::parse("name = \"base\"\n[metrics]\nradius = 12\n[dark]\ntext = \"#ffffff\"\naccent = \"#111111\"\n[light]\ntext = \"#000000\"\n").unwrap();
        let child = Def::parse("name = \"child\"\ninherit = \"base\"\n[dark]\naccent = \"#222222\"\n").unwrap();
        let m = child.over(&base);
        let t = m.resolve("c", true, &Selection::default());
        assert_eq!((t.accent, t.text, t.radius, t.name.as_str()), (Rgb(0x22, 0x22, 0x22), Rgb(255, 255, 255), 12.0, "child"));
        assert!(m.has_light());
    }

    #[test]
    fn overrides_win_and_are_clamped() {
        let d = Def::parse(MINI).unwrap();
        let sel = Selection { accent: Rgb::parse("#ffff00"), opacity: Some(0.1), radius: Some(40.0), font: Some("Foo".into()), font_scale: Some(2.0), ..Selection::default() };
        let t = d.resolve("m", true, &sel);
        assert_eq!(t.accent, Rgb(255, 255, 0));
        assert_eq!(t.accent_text, Rgb(0, 0, 0), "accent text follows the overridden accent");
        assert_eq!(t.radius, 24.0);
        assert!(t.popup_alpha < 0.5 && t.popup_alpha >= 0.3);
        assert_eq!((t.font_family.as_str(), t.font_scale), ("Foo", 1.4));
    }

    #[test]
    fn old_font_keys_still_work() {
        // decebal.json from before font roles: `font` is a string, `fontScale` a top-level number
        let sel = Selection::from_json(&serde_json::json!({"name": "nord", "font": "Rubik", "fontScale": 1.2}));
        assert_eq!((sel.font.as_deref(), sel.font_scale, sel.fonts.is_empty()), (Some("Rubik"), Some(1.2), true));
        let d = Def::parse(MINI).unwrap();
        let t = d.resolve("m", true, &sel);
        assert_eq!(t.font_family, "Rubik");
        assert_eq!((t.fonts.chain(crate::theme::Role::Ui)[0].as_str(), t.fonts.chain(crate::theme::Role::Title)[0].as_str()), ("Rubik", "Rubik"), "the single font leads text and titles");
        assert_ne!(t.fonts.chain(crate::theme::Role::Mono)[0], "Rubik");
        assert_eq!(t.fonts.preset, "inter", "a theme with no font keys keeps the original Inter look");
        // theme files: `[font] family` is the same single font
        let old = Def::parse("name = \"o\"\n[font]\nfamily = \"Segoe UI\"\nscale = 1.1\n").unwrap().resolve("o", true, &Selection::default());
        assert_eq!((old.font_family.as_str(), old.font_scale), ("Segoe UI", 1.1));
        // the new object form
        let sel = Selection::from_json(&serde_json::json!({"name": "nord", "font": {"preset": "rubik", "ui": ["Inter", "Noto Sans CJK"], "scale": 1.3}}));
        assert_eq!((sel.fonts.preset.as_deref(), sel.font_scale), (Some("rubik"), Some(1.3)));
        let t = d.resolve("m", true, &sel);
        assert_eq!(&t.fonts.chain(crate::theme::Role::Ui)[..3], ["Inter", "Noto Sans CJK", "Rubik"]);
    }

    #[test]
    fn theme_files_accept_font_roles_and_presets() {
        let d = Def::parse("name = \"f\"\n[font]\npreset = \"segoe\"\nui = [\"Foo\", \"Bar\"]\nmono = \"Baz, Qux\"\n").unwrap();
        let t = d.resolve("f", true, &Selection::default());
        assert_eq!(t.fonts.preset, "segoe");
        assert_eq!(&t.fonts.chain(crate::theme::Role::Ui)[..3], ["Foo", "Bar", "Segoe UI Variable"]);
        assert_eq!(&t.fonts.chain(crate::theme::Role::Mono)[..2], ["Baz", "Qux"]);
        assert!(Def::parse("name = \"x\"\n[font]\npreset = \"comic\"\n").unwrap_err().contains("one of"));
        assert!(Def::parse("name = \"x\"\n[font]\nui = 5\n").unwrap_err().contains("font name"));
        assert!(Def::parse("name = \"x\"\n[font]\nui = []\n").unwrap_err().contains("at least one"));
        // variants can have their own fonts
        let d = Def::parse("name = \"v\"\n[dark.font]\npreset = \"fira\"\n[light.font]\npreset = \"noto\"\n").unwrap();
        assert_eq!(d.resolve("v", true, &Selection::default()).fonts.preset, "fira");
        assert_eq!(d.resolve("v", false, &Selection::default()).fonts.preset, "noto");
    }

    #[test]
    fn every_builtin_theme_has_a_sensible_font_preset() {
        let want = [
            ("macos", "inter"),
            ("graphite", "inter"),
            ("nord", "inter"),
            ("dracula", "jetbrains"),
            ("solarized", "ibm-plex"),
            ("windows11", "segoe"),
            ("caelestia", "rubik"),
            ("high-contrast", "system"),
        ];
        for (id, preset) in want {
            let t = crate::theme::builtin::def(id).unwrap().resolve(id, true, &Selection::default());
            assert_eq!(t.fonts.preset, preset, "{id}");
        }
        // the user's choice beats the theme's
        let sel = Selection { fonts: crate::theme::fonts::FontChoice { preset: Some("fira".into()), ..Default::default() }, ..Selection::default() };
        assert_eq!(crate::theme::builtin::def("windows11").unwrap().resolve("windows11", true, &sel).fonts.preset, "fira");
    }

    #[test]
    fn selection_json_round_trip() {
        let sel = Selection {
            name: "nord".into(),
            mode: Mode::Auto,
            accent: Rgb::parse("#123456"),
            palette: Some("catppuccin".into()),
            opacity: Some(0.85),
            radius: Some(6.0),
            font: Some("Inter".into()),
            font_scale: Some(1.1),
            fonts: Default::default(),
            sync_rdwm: false,
            dynamic: Some(Dynamic {
                enabled: true,
                scheme: crate::theme::material::Variant::Palette,
                source: Origin::File("/w.png".into()),
                surfaces: Surfaces::Full,
                strength: 0.75,
                contrast: -0.5,
                follow_mode: true,
                colors: crate::theme::extract::Extracted { seed: Rgb(1, 2, 3), others: vec![Rgb(4, 5, 6)], tone: 33.5, grey: false },
                image: Some("/w.png".into()),
            }),
        };
        assert_eq!(Selection::from_json(&sel.to_json()), sel);
        // old form stays old: a plain string and a top-level fontScale
        let j = sel.to_json();
        assert_eq!((j["font"].as_str(), j["fontScale"].as_f64()), (Some("Inter"), Some(1.1)));
        // new form: a font object with the scale inside
        let mut with_fonts = sel.clone();
        with_fonts.fonts.preset = Some("segoe".into());
        with_fonts.fonts.roles[2] = Some(vec!["Fira Code".into()]);
        let j = with_fonts.to_json();
        assert_eq!(j["font"]["preset"], "segoe");
        assert_eq!((j["font"]["family"].as_str(), j["font"]["mono"].as_str(), j["font"]["scale"].as_f64()), (Some("Inter"), Some("Fira Code"), Some(1.1)));
        assert!(j.get("fontScale").is_none());
        assert_eq!(Selection::from_json(&j), with_fonts);
        // off and default: nothing is written until it is turned on
        assert!(Selection::default().dynamic.is_none());
        assert!(!Dynamic::default().enabled);
        // files written before the feature could be switched off: on only for Caelestia, as a full palette
        let old = serde_json::json!({"name": "caelestia", "dynamic": {"seed": "#2a9d8f", "scheme": "vibrant", "wallpaper": "/old.png", "follow": true}});
        let d = Selection::from_json(&old).dynamic.unwrap();
        assert!(d.enabled && d.surfaces == Surfaces::Full && d.strength == 1.0 && d.scheme == crate::theme::material::Variant::Vibrant);
        assert_eq!((d.colors.seed, d.image.as_deref(), &d.source), (Rgb(0x2a, 0x9d, 0x8f), Some("/old.png"), &Origin::Wallpaper));
        let old_nord = serde_json::json!({"name": "nord", "dynamic": {"seed": "#2a9d8f", "follow": false, "wallpaper": "/old.png"}});
        let d = Selection::from_json(&old_nord).dynamic.unwrap();
        assert!(!d.enabled, "an old file never recoloured nord");
        assert_eq!(d.source, Origin::File("/old.png".into()), "follow: false pinned the image");
        // sources
        assert_eq!(Origin::parse("wallpaper"), Some(Origin::Wallpaper));
        assert_eq!(Origin::parse("#112233"), Some(Origin::Color(Rgb(0x11, 0x22, 0x33))));
        assert_eq!(Origin::parse("~/Pictures/a.png"), Some(Origin::File("~/Pictures/a.png".into())));
        assert_eq!(Origin::parse("#nope"), None);
        assert_eq!(Surfaces::parse("Accent-only"), Some(Surfaces::Accent));
        assert_eq!(Surfaces::parse("nope"), None);
        assert_eq!(Selection::from_json(&serde_json::json!("dracula")).name, "dracula");
        assert_eq!(Selection::from_json(&serde_json::json!(5)), Selection::default());
        let junk = Selection::from_json(&serde_json::json!({"name": "", "mode": "purple", "accent": "nope", "opacity": 9}));
        assert_eq!((junk.name.as_str(), junk.mode, junk.accent, junk.opacity), ("macos", Mode::Dark, None, Some(1.0)));
    }
}
