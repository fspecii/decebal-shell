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
    pub shadow: f32,
    pub outline: f32,
    pub dock_icon_size: f32,
    pub dock_padding: f32,
    pub bar_height: f32,
    pub titlebar_style: String,
    pub titlebar_height: i64,
    /// `flat` (a full-width strip), `floating` (a horizontal pill with a margin around it) or `vertical`
    /// (caelestia's layout: a bar down the left screen edge, no dock).
    pub bar_style: String,
    /// The interface font as written (`fonts.primary_ui()`; empty = automatic), kept for older readers.
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
    /// The bar runs down the left screen edge (caelestia's layout) instead of across the top.
    pub fn bar_vertical(&self) -> bool {
        self.bar_style == "vertical"
    }
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
    ("dock_alpha", Kind::Num(0.2, 1.0), "dock panel opacity; dock-settings.json `transparency` overrides"),
    ("popup_alpha", Kind::Num(0.5, 1.0), "menu opacity"),
    ("panel_alpha", Kind::Num(0.5, 1.0), "Spotlight / Launchpad / Control Center opacity"),
    ("bar_alpha", Kind::Num(0.5, 1.0), "menu bar opacity (needs a compositor below 1)"),
    ("shadow", Kind::Num(0.0, 1.5), "drop shadow strength"),
    ("outline", Kind::Num(0.0, 1.0), "alpha of the 1px outline around popups"),
    ("dock_icon_size", Kind::Num(24.0, 128.0), "dock icon size in px (consumed by the dock layout)"),
    ("dock_padding", Kind::Num(4.0, 40.0), "dock panel padding in px (consumed by the dock layout)"),
    ("bar_height", Kind::Num(20.0, 40.0), "menu bar height in px (consumed by the bar layout)"),
    ("bar_style", Kind::Choice(&["flat", "floating", "vertical"]), "menu bar shape: a full-width strip, a floating pill, or a vertical bar on the left screen edge (the dock hides with it)"),
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
        }
    }

    /// This theme with its colours replaced by the Material palette of `d` (both variants; metrics stay).
    pub fn with_dynamic(&self, d: &Dynamic) -> Def {
        let mut out = self.clone();
        for dark in [true, false] {
            let colours = super::material::generate(d.seed, d.variant, dark).tokens();
            let slot = if dark { &mut out.dark } else { &mut out.light };
            slot.get_or_insert_with(Tokens::new).extend(colours);
        }
        out
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
            dock_alpha: clampf(num("dock_alpha", 0.6) * opacity as f64, 0.2, 1.0),
            popup_alpha: clampf(num("popup_alpha", 0.96) * opacity as f64, 0.3, 1.0),
            panel_alpha: clampf(num("panel_alpha", 0.97) * opacity as f64, 0.3, 1.0),
            bar_alpha: clampf(num("bar_alpha", 1.0), 0.5, 1.0),
            shadow: clampf(num("shadow", 1.0), 0.0, 1.5),
            outline: clampf(num("outline", 0.14), 0.0, 1.0),
            dock_icon_size: clampf(num("dock_icon_size", 47.0), 24.0, 128.0),
            dock_padding: clampf(num("dock_padding", 16.0), 4.0, 40.0),
            bar_height: clampf(num("bar_height", 26.0), 20.0, 40.0),
            bar_style: text_of("bar_style").unwrap_or_else(|| "flat".into()),
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

/// "Colours from wallpaper": the Material palette of a seed colour replaces the Caelestia theme's colours.
/// The seed is taken from the wallpaper once (by `macthemectl dynamic` or the Theme pane) and stored here,
/// so every program derives the same palette from three words of JSON instead of decoding an image.
#[derive(Debug, Clone, PartialEq)]
pub struct Dynamic {
    pub seed: Rgb,
    pub variant: super::material::Variant,
    /// The image the seed came from (shown in the UI; the seed is not re-read from it by the programs).
    pub wallpaper: Option<String>,
    /// Re-derive the seed when macshell changes the wallpaper (machaven, the settings Wallpaper pane).
    pub follow: bool,
}

impl Dynamic {
    pub fn from_json(v: &serde_json::Value) -> Option<Dynamic> {
        let o = v.as_object()?;
        Some(Dynamic {
            seed: o.get("seed").and_then(|x| x.as_str()).and_then(Rgb::parse)?,
            variant: o.get("scheme").and_then(|x| x.as_str()).and_then(super::material::Variant::parse).unwrap_or_default(),
            wallpaper: o.get("wallpaper").and_then(|x| x.as_str()).filter(|w| !w.is_empty()).map(String::from),
            follow: o.get("follow").and_then(|x| x.as_bool()).unwrap_or(true),
        })
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert("seed".into(), self.seed.hex().into());
        o.insert("scheme".into(), self.variant.as_str().into());
        if let Some(w) = &self.wallpaper {
            o.insert("wallpaper".into(), w.clone().into());
        }
        o.insert("follow".into(), self.follow.into());
        serde_json::Value::Object(o)
    }
}

/// The user's choice: a theme, a mode, and optional per-key overrides. Stored under `"theme"` in `macshell.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub name: String,
    pub mode: Mode,
    pub accent: Option<Rgb>,
    /// Multiplies the translucency-capable surfaces (0.4..1).
    pub opacity: Option<f32>,
    pub radius: Option<f32>,
    /// The old single font family (`"font": "Inter"`): the font of text, titles and display numbers.
    pub font: Option<String>,
    pub font_scale: Option<f32>,
    /// Font preset and per-role chains (the `font` object).
    pub fonts: super::fonts::FontChoice,
    /// Also restyle rdwm's titlebars when applying (macthemectl / macsettings).
    pub sync_rdwm: bool,
    /// Colours derived from the wallpaper; only the Caelestia theme uses them.
    pub dynamic: Option<Dynamic>,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            name: crate::theme::builtin::DEFAULT_ID.into(),
            mode: Mode::Dark,
            accent: None,
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
    /// From the `"theme"` value of macshell.json: a bare name or an object. Bad fields are ignored.
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
        s.dynamic = o.get("dynamic").and_then(Dynamic::from_json);
        s
    }

    /// Object form for macshell.json. Unset overrides are omitted.
    pub fn to_json(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert("name".into(), self.name.clone().into());
        o.insert("mode".into(), self.mode.as_str().into());
        if let Some(a) = self.accent {
            o.insert("accent".into(), a.hex().into());
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
        // macshell.json from before font roles: `font` is a string, `fontScale` a top-level number
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
            opacity: Some(0.85),
            radius: Some(6.0),
            font: Some("Inter".into()),
            font_scale: Some(1.1),
            fonts: Default::default(),
            sync_rdwm: false,
            dynamic: Some(Dynamic { seed: Rgb(1, 2, 3), variant: crate::theme::material::Variant::Vibrant, wallpaper: Some("/w.png".into()), follow: false }),
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
        assert_eq!(Selection::from_json(&serde_json::json!("dracula")).name, "dracula");
        assert_eq!(Selection::from_json(&serde_json::json!(5)), Selection::default());
        let junk = Selection::from_json(&serde_json::json!({"name": "", "mode": "purple", "accent": "nope", "opacity": 9}));
        assert_eq!((junk.name.as_str(), junk.mode, junk.accent, junk.opacity), ("macos", Mode::Dark, None, Some(1.0)));
    }
}
