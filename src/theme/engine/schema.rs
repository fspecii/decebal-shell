//! The key table of the theme package format (`format = 1`).
//!
//! One table drives the validator ([`super::manifest`]), `macthemectl theme schema` and docs/theme-engine.md, so they cannot
//! drift apart. Colour and metric token keys come from [`crate::theme::model::TOKENS`] (they are listed here by reference).

use crate::theme::model::{Kind, TOKENS};

/// Format version this build reads and writes.
pub const FORMAT: i64 = 1;

/// What an asset path may point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Font,
}

impl AssetKind {
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            AssetKind::Image => &["png", "jpg", "jpeg", "webp"],
            AssetKind::Font => &["ttf", "otf"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Elem {
    Text(usize),
    Ident,
    Choice(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    Color,
    Num(f64, f64),
    Int(i64, i64),
    /// A string of at most this many characters.
    Text(usize),
    Bool,
    /// `true`, `false` or `"auto"`.
    Tri,
    Choice(&'static [&'static str]),
    /// `[a-z0-9][a-z0-9._-]*`, at most 64.
    Ident,
    /// `1.2.3`.
    Version,
    Asset(AssetKind),
    Assets(AssetKind, usize),
    List(Elem, usize),
    /// A safe hot-corner / widget action word (checked against the allowlist).
    Action,
}

/// Whether the shell acts on the key today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Wired,
    /// Validated, stored and exported, but nothing reads it yet (the note says what is missing).
    Pending(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub path: &'static str,
    pub ty: Ty,
    pub desc: &'static str,
    pub status: Status,
}

use AssetKind::{Font, Image};
use Status::{Pending, Wired};

pub const POSITIONS: &[&str] = &["top", "bottom", "left", "right", "split"];
pub const ANCHORS: &[&str] = &["top-left", "top", "top-right", "left", "center", "right", "bottom-left", "bottom", "bottom-right"];
pub const BUTTON_NAMES: &[&str] = &["close", "minimize", "maximize"];
pub const WALLPAPER_MODES: &[&str] = &["fill", "fit", "stretch", "center", "tile"];
/// Layout names rdwm knows (the first `layouts` entry is the default); validated here so a typo is caught at install time.
pub const LAYOUTS: &[&str] = &[
    "tile", "monocle", "floating", "bottomstack", "bstackhoriz", "rtile", "grid", "gaplessgrid", "horizgrid", "nrowgrid", "deck", "centeredmaster", "centeredfloatingmaster", "dwindle", "spiral", "tatami",
    "columns", "rows", "threecol", "bsp", "scroll", "tabbed", "stacked",
];

/// Everything except the colour/metric token tables (those come from `TOKENS`).
pub const FIELDS: &[Field] = &[
    Field { path: "format", ty: Ty::Int(1, 1), desc: "package format version (always 1)", status: Wired },
    Field { path: "id", ty: Ty::Ident, desc: "unique id: lower-case letters, digits, `.`, `_`, `-`; the install folder name", status: Wired },
    Field { path: "name", ty: Ty::Text(80), desc: "display name", status: Wired },
    Field { path: "version", ty: Ty::Version, desc: "this theme's version, MAJOR.MINOR.PATCH", status: Wired },
    Field { path: "author", ty: Ty::Text(80), desc: "who made it", status: Wired },
    Field { path: "license", ty: Ty::Text(60), desc: "SPDX identifier of the licence of the package's contents", status: Wired },
    Field { path: "description", ty: Ty::Text(400), desc: "one or two sentences for the gallery", status: Wired },
    Field { path: "homepage", ty: Ty::Text(200), desc: "web page or repository", status: Wired },
    Field { path: "min_shell_version", ty: Ty::Version, desc: "oldest macshell this theme works with", status: Wired },
    Field { path: "screenshots", ty: Ty::Assets(Image, 8), desc: "preview images shown in the gallery and the market", status: Wired },
    Field { path: "tags", ty: Ty::List(Elem::Text(24), 12), desc: "search words (dark, minimal, material, retro...)", status: Wired },
    Field { path: "extends", ty: Ty::Ident, desc: "id of a theme to start from; only the keys given here are overridden", status: Wired },
    Field { path: "dark", ty: Ty::Bool, desc: "only for a file with a flat [colors] table and no variants: is it the dark one (default true)", status: Wired },
    Field { path: "integrity.content_hash", ty: Ty::Text(80), desc: "`sha256:<hex>` over every file of the package (written by `theme pack`, checked on install)", status: Wired },
    Field { path: "integrity.signature", ty: Ty::Text(600), desc: "detached signature of content_hash, reserved for the theme market", status: Pending("no verifier until the market defines its keys") },
    Field { path: "integrity.signer", ty: Ty::Text(120), desc: "key id of the signer, reserved for the theme market", status: Pending("no verifier until the market defines its keys") },
    Field { path: "metrics.gap", ty: Ty::Num(0.0, 40.0), desc: "spacing unit between shell elements in px", status: Pending("no shell surface reads it yet") },
    Field { path: "metrics.blur", ty: Ty::Num(0.0, 1.0), desc: "background blur strength for translucent surfaces (needs a compositor)", status: Pending("blur is not implemented in the shell") },
    Field { path: "font.ui", ty: Ty::Text(80), desc: "interface font family (fontconfig name)", status: Wired },
    Field { path: "font.title", ty: Ty::Text(80), desc: "window titlebar font family", status: Pending("font roles land with src/theme/fonts.rs") },
    Field { path: "font.mono", ty: Ty::Text(80), desc: "monospace font family", status: Pending("font roles land with src/theme/fonts.rs") },
    Field { path: "font.display", ty: Ty::Text(80), desc: "large text (clock, headings) font family", status: Pending("font roles land with src/theme/fonts.rs") },
    Field { path: "font.preset", ty: Ty::Ident, desc: "named font preset (e.g. system, inter, segoe, sf) the fonts module expands to roles", status: Pending("font roles land with src/theme/fonts.rs") },
    Field { path: "font.scale", ty: Ty::Num(0.8, 1.4), desc: "text size multiplier", status: Wired },
    Field { path: "font.fallback", ty: Ty::List(Elem::Text(80), 8), desc: "families tried when a role's font is missing", status: Pending("font roles land with src/theme/fonts.rs") },
    Field { path: "font.files", ty: Ty::Assets(Font, 8), desc: "font files bundled in the package", status: Pending("bundled fonts are not registered with fontconfig yet") },
    Field { path: "wallpaper.image", ty: Ty::Asset(Image), desc: "a bundled wallpaper", status: Wired },
    Field { path: "wallpaper.images", ty: Ty::Assets(Image, 64), desc: "several bundled wallpapers (the first one is used unless slideshow is on)", status: Wired },
    Field { path: "wallpaper.mode", ty: Ty::Choice(WALLPAPER_MODES), desc: "how the image fills the screen", status: Wired },
    Field { path: "wallpaper.color", ty: Ty::Color, desc: "background colour behind the image", status: Wired },
    Field { path: "wallpaper.slideshow", ty: Ty::Bool, desc: "rotate through `images`", status: Pending("no slideshow runner yet; stored in macshell.json wallpaper.slideshow") },
    Field { path: "wallpaper.interval", ty: Ty::Int(10, 86400), desc: "seconds between slideshow images", status: Pending("no slideshow runner yet") },
    Field { path: "wallpaper.shuffle", ty: Ty::Bool, desc: "random slideshow order", status: Pending("no slideshow runner yet") },
    Field { path: "wallpaper.match_colors", ty: Ty::Bool, desc: "recolour the theme from the wallpaper (Material You), like `macthemectl dynamic`", status: Wired },
    Field { path: "wallpaper.scheme", ty: Ty::Choice(&["tonalspot", "vibrant", "neutral", "content"]), desc: "palette variant used with match_colors", status: Wired },
    Field { path: "decoration.style", ty: Ty::Ident, desc: "rdwm titlebar style by name (macos, classic, windows, adwaita, breeze, arc, minimal...); rdwm decides what it knows", status: Wired },
    Field { path: "decoration.height", ty: Ty::Int(16, 60), desc: "titlebar height in px", status: Wired },
    Field { path: "decoration.radius", ty: Ty::Int(0, 24), desc: "window corner radius in px (default: the theme's radius)", status: Wired },
    Field { path: "decoration.title_align", ty: Ty::Choice(&["center", "left"]), desc: "title text alignment", status: Wired },
    Field { path: "decoration.buttons", ty: Ty::Choice(&["left", "right", "none"]), desc: "which side the window buttons are on; `none` hides them", status: Pending("button side needs the rdwm decoration styles branch; `none` is applied") },
    Field { path: "decoration.button_order", ty: Ty::List(Elem::Choice(BUTTON_NAMES), 3), desc: "order of the buttons from the outer edge in", status: Pending("needs the rdwm decoration styles branch") },
    Field { path: "bar.enabled", ty: Ty::Tri, desc: "show the menu bar (`auto` = hidden while the taskbar-style dock is up)", status: Wired },
    Field { path: "bar.position", ty: Ty::Choice(POSITIONS), desc: "which screen edge the bar is on; `split` draws separate islands", status: Pending("top and left are wired through bar.style; the rest needs the bar layouts branch") },
    Field { path: "bar.style", ty: Ty::Choice(&["flat", "floating", "vertical"]), desc: "flat strip, floating pill or vertical bar down the left edge", status: Wired },
    Field { path: "bar.height", ty: Ty::Int(20, 40), desc: "bar height (width when vertical) in px", status: Wired },
    Field { path: "bar.left", ty: Ty::List(Elem::Ident, 24), desc: "items on the left of the bar, in order (names, or `widget:<id>`)", status: Pending("needs the bar layouts branch; stored in macshell.json bar.left") },
    Field { path: "bar.center", ty: Ty::List(Elem::Ident, 24), desc: "items in the middle of the bar, in order", status: Pending("needs the bar layouts branch; stored in macshell.json bar.center") },
    Field { path: "bar.right", ty: Ty::List(Elem::Ident, 24), desc: "items on the right of the bar, in order", status: Pending("needs the bar layouts branch; stored in macshell.json bar.right") },
    Field { path: "dock.enabled", ty: Ty::Tri, desc: "show the dock (`auto` = hidden under a vertical bar)", status: Wired },
    Field { path: "dock.style", ty: Ty::Choice(&["mac", "windows"]), desc: "macOS floating dock or the Windows 11 taskbar", status: Wired },
    Field { path: "dock.position", ty: Ty::Choice(&["bottom", "top", "left", "right"]), desc: "screen edge of the dock", status: Pending("only bottom/top are wired (windows taskbar); left/right need the dock layouts work") },
    Field { path: "dock.size", ty: Ty::Int(24, 128), desc: "icon size in px", status: Wired },
    Field { path: "dock.align", ty: Ty::Choice(&["center", "left"]), desc: "Windows taskbar icon alignment", status: Wired },
    Field { path: "dock.roundness", ty: Ty::Num(0.0, 1.0), desc: "0 square .. 1 pill", status: Wired },
    Field { path: "dock.magnification", ty: Ty::Num(1.0, 2.5), desc: "hover magnification of the macOS dock", status: Wired },
    Field { path: "icons.theme", ty: Ty::Text(80), desc: "icon theme name (e.g. Papirus, Adwaita)", status: Pending("the shell has no icon theme setting yet; stored in macshell.json icons.theme") },
    Field { path: "hot_corners.top_left", ty: Ty::Action, desc: "hot-corner action (none, dashboard, session, spotlight, launchpad, overview, lock, tile-all, notifications)", status: Wired },
    Field { path: "hot_corners.top_right", ty: Ty::Action, desc: "see top_left", status: Wired },
    Field { path: "hot_corners.bottom_left", ty: Ty::Action, desc: "see top_left", status: Wired },
    Field { path: "hot_corners.bottom_right", ty: Ty::Action, desc: "see top_left", status: Wired },
    Field { path: "hot_corners.dwell_ms", ty: Ty::Int(0, 5000), desc: "how long the pointer rests in a corner", status: Wired },
    Field { path: "rdwm.gap_inner", ty: Ty::Int(0, 64), desc: "gap between tiled windows", status: Wired },
    Field { path: "rdwm.gap_outer", ty: Ty::Int(0, 64), desc: "gap around the work area", status: Wired },
    Field { path: "rdwm.border_px", ty: Ty::Int(0, 16), desc: "window border width", status: Wired },
    Field { path: "rdwm.smart_gaps", ty: Ty::Bool, desc: "no gaps with a single window", status: Wired },
    Field { path: "rdwm.mfact", ty: Ty::Num(0.05, 0.95), desc: "initial master area fraction", status: Wired },
    Field { path: "rdwm.nmaster", ty: Ty::Int(0, 8), desc: "initial number of master windows", status: Wired },
    Field { path: "rdwm.layouts", ty: Ty::List(Elem::Choice(LAYOUTS), 12), desc: "layouts cyclelayout walks; the first is the default", status: Wired },
    Field { path: "rdwm.focused_border", ty: Ty::Color, desc: "border of the focused window (default: the accent)", status: Wired },
    Field { path: "rdwm.normal_border", ty: Ty::Color, desc: "border of other windows (default: the theme's border colour)", status: Wired },
    Field { path: "rdwm.urgent_border", ty: Ty::Color, desc: "border of windows asking for attention (default: danger)", status: Wired },
];

/// Keys of a `[[widgets]]` instance.
pub const WIDGET_FIELDS: &[Field] = &[
    Field { path: "use", ty: Ty::Text(80), desc: "widget to show: a file `widgets/<name>.toml` of this package (just the name) or `builtin:<name>` (samples shipped with macshell)", status: Wired },
    Field { path: "place", ty: Ty::Choice(&["desktop", "bar"]), desc: "on the desktop (macwidgets) or inside the bar", status: Wired },
    Field { path: "anchor", ty: Ty::Choice(ANCHORS), desc: "desktop: which screen corner/edge x and y are measured from", status: Wired },
    Field { path: "x", ty: Ty::Int(-4000, 4000), desc: "desktop: horizontal offset from the anchor in px", status: Wired },
    Field { path: "y", ty: Ty::Int(-4000, 4000), desc: "desktop: vertical offset from the anchor in px", status: Wired },
    Field { path: "monitor", ty: Ty::Text(60), desc: "desktop: output name or `primary`", status: Wired },
    Field { path: "slot", ty: Ty::Choice(&["left", "center", "right"]), desc: "bar: which part of the bar", status: Pending("hosting widgets inside macbar needs the bar layouts branch") },
    Field { path: "order", ty: Ty::Int(-100, 100), desc: "bar: position among the items of the slot (lower first)", status: Pending("hosting widgets inside macbar needs the bar layouts branch") },
    Field { path: "enabled", ty: Ty::Bool, desc: "turn the instance off without deleting it", status: Wired },
    Field { path: "options", ty: Ty::Text(0), desc: "table of values handed to the widget as `opt.<key>`", status: Wired },
];

/// All fields, theme-level plus the colour/metric tokens, for listing.
pub struct Row {
    pub path: String,
    pub ty: String,
    pub desc: String,
    pub status: Status,
}

fn ty_text(t: &Ty) -> String {
    match t {
        Ty::Color => "colour #rgb|#rrggbb".into(),
        Ty::Num(a, b) => format!("number {a}..{b}"),
        Ty::Int(a, b) => format!("integer {a}..{b}"),
        Ty::Text(n) => format!("text (<= {n})"),
        Ty::Bool => "true|false".into(),
        Ty::Tri => "true|false|\"auto\"".into(),
        Ty::Choice(c) => c.join("|"),
        Ty::Ident => "id".into(),
        Ty::Version => "x.y.z".into(),
        Ty::Asset(k) => format!("file ({})", k.extensions().join("/")),
        Ty::Assets(k, n) => format!("files ({}; <= {n})", k.extensions().join("/")),
        Ty::List(Elem::Text(_), n) => format!("list of text (<= {n})"),
        Ty::List(Elem::Ident, n) => format!("list of names (<= {n})"),
        Ty::List(Elem::Choice(c), n) => format!("list of {} (<= {n})", c.join("|")),
        Ty::Action => "safe action".into(),
    }
}

/// Every key the format knows, in document order.
pub fn rows() -> Vec<Row> {
    let mut out = Vec::new();
    let mut push = |path: String, ty: String, desc: &str, status: Status| out.push(Row { path, ty, desc: desc.to_string(), status });
    for f in FIELDS.iter().filter(|f| !f.path.contains('.')) {
        push(f.path.into(), ty_text(&f.ty), f.desc, f.status);
    }
    for f in FIELDS.iter().filter(|f| f.path.starts_with("integrity.")) {
        push(f.path.into(), ty_text(&f.ty), f.desc, f.status);
    }
    for variant in ["colors", "colors.dark", "colors.light"] {
        for (k, kind, desc) in TOKENS.iter().filter(|t| matches!(t.1, Kind::Color)) {
            let _ = kind;
            push(format!("{variant}.{k}"), ty_text(&Ty::Color), desc, Wired);
        }
    }
    for (k, kind, desc) in TOKENS.iter().filter(|t| matches!(t.1, Kind::Num(..)) && !matches!(t.0, "font_scale" | "titlebar_height" | "bar_height" | "dock_icon_size")) {
        if let Kind::Num(a, b) = kind {
            push(format!("metrics.{k}"), ty_text(&Ty::Num(*a, *b)), desc, Wired);
        }
    }
    for section in ["metrics", "font", "wallpaper", "decoration", "bar", "dock", "icons", "hot_corners", "rdwm"] {
        for f in FIELDS.iter().filter(|f| f.path.starts_with(&format!("{section}."))) {
            push(f.path.into(), ty_text(&f.ty), f.desc, f.status);
        }
    }
    for f in WIDGET_FIELDS {
        push(format!("widgets[].{}", f.path), ty_text(&f.ty), f.desc, f.status);
    }
    out
}

pub fn field(path: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.path == path)
}

/// `1.2.3` -> (1, 2, 3); missing parts are 0, anything non-numeric fails.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().splitn(3, '.');
    let n = |p: Option<&str>| p.map(|x| x.parse::<u32>().ok()).unwrap_or(Some(0));
    let (a, b, c) = (n(it.next())?, n(it.next())?, n(it.next())?);
    (!s.trim().is_empty()).then_some((a, b, c))
}

pub fn valid_ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// Hot-corner actions a theme may set. `custom:<command>` is deliberately absent: themes are data, not commands.
pub fn safe_hot_actions() -> &'static [&'static str] {
    &crate::hot::ACTION_NAMES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_ids() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.x"), None);
        assert_eq!(parse_version(""), None);
        assert!(valid_ident("caelestia-2.x"));
        for bad in ["", "Nord", "-x", "a b", "a/b", "../x", &"a".repeat(65)] {
            assert!(!valid_ident(bad), "{bad:?}");
        }
    }

    #[test]
    fn listing_covers_tokens_and_has_unique_paths() {
        let rows = rows();
        let mut seen = std::collections::BTreeSet::new();
        for r in &rows {
            assert!(seen.insert(r.path.clone()), "duplicate {}", r.path);
        }
        for must in ["format", "colors.dark.accent", "metrics.radius", "font.ui", "decoration.style", "bar.left", "widgets[].use", "rdwm.gap_inner", "integrity.content_hash"] {
            assert!(seen.contains(must), "{must}");
        }
    }
}
