//! Where the Control Center lives and what it shows: `decebal.json` -> `"control_center"` (and the theme's `[control_center]`
//! section, which `decthemectl theme apply` copies there).
//!
//! ```json
//! "control_center": { "enabled": true, "position": "auto", "style": "auto", "tiles": ["connectivity", "focus", "display", "sound", "media", "toggles", "actions"] }
//! ```
//!
//! Everything is optional and lenient (a misspelt value means the default; the theme validator is the strict one), and the
//! defaults make the panel present in every theme. The pure placement maths of a summoned panel ([`place`]) lives here too, so
//! it is tested without a screen.

use crate::config::BarPosition;
use serde_json::{json, Value};

/// Names of the tile groups a theme or the user can switch off, in panel order.
pub const TILE_NAMES: [&str; 7] = ["connectivity", "focus", "display", "sound", "media", "toggles", "actions"];
/// Values of `position`.
pub const POSITION_NAMES: [&str; 5] = ["auto", "top-right", "top-left", "bottom-right", "bottom-left"];
/// Values of `style`.
pub const STYLE_NAMES: [&str; 5] = ["auto", "card", "fluent", "dux", "gnome"];

/// Which tile groups the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tiles {
    /// Wi-Fi, Bluetooth and the wired network.
    pub connectivity: bool,
    /// Focus, Night Shift and Dark Mode.
    pub focus: bool,
    pub display: bool,
    pub sound: bool,
    /// Now Playing.
    pub media: bool,
    /// The decutil toggles: Record, Inhibit Idle, Game Mode, DND.
    pub toggles: bool,
    /// Stay Awake, Screenshot, Notifications, Settings.
    pub actions: bool,
}

impl Tiles {
    pub const ALL: Tiles = Tiles { connectivity: true, focus: true, display: true, sound: true, media: true, toggles: true, actions: true };

    pub fn get(&self, name: &str) -> Option<bool> {
        Some(match name {
            "connectivity" => self.connectivity,
            "focus" => self.focus,
            "display" => self.display,
            "sound" => self.sound,
            "media" => self.media,
            "toggles" => self.toggles,
            "actions" => self.actions,
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, on: bool) {
        match name {
            "connectivity" => self.connectivity = on,
            "focus" => self.focus = on,
            "display" => self.display = on,
            "sound" => self.sound = on,
            "media" => self.media = on,
            "toggles" => self.toggles = on,
            "actions" => self.actions = on,
            _ => {}
        }
    }

    /// Exactly the named groups; names it does not know are ignored. An empty (or all-unknown) list means every group, so a
    /// typo never leaves an empty panel.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Tiles {
        let mut t = Tiles { connectivity: false, focus: false, display: false, sound: false, media: false, toggles: false, actions: false };
        let mut any = false;
        for n in names {
            if t.get(n).is_some() {
                t.set(n, true);
                any = true;
            }
        }
        if any {
            t
        } else {
            Tiles::ALL
        }
    }

    /// The enabled names, in panel order.
    pub fn names(&self) -> Vec<&'static str> {
        TILE_NAMES.into_iter().filter(|n| self.get(n) == Some(true)).collect()
    }
}

/// The corner a summoned panel opens in (`decutil control-center`, a hot corner, a bar that has no button).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    /// Next to the bar's own button: under the menu bar, above the taskbar, beside the pill bar.
    #[default]
    Auto,
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

impl Corner {
    pub fn parse(s: &str) -> Option<Corner> {
        Some(match s.trim().to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "auto" => Corner::Auto,
            "top-right" | "right-top" => Corner::TopRight,
            "top-left" | "left-top" => Corner::TopLeft,
            "bottom-right" | "right-bottom" => Corner::BottomRight,
            "bottom-left" | "left-bottom" => Corner::BottomLeft,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Corner::Auto => "auto",
            Corner::TopRight => "top-right",
            Corner::TopLeft => "top-left",
            Corner::BottomRight => "bottom-right",
            Corner::BottomLeft => "bottom-left",
        }
    }
}

/// How the panel is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// `fluent` under the Windows taskbar, `card` everywhere else.
    #[default]
    Auto,
    /// The macOS / Caelestia look: big rounded tiles, the theme radius doubled for the panel.
    Card,
    /// The Windows 11 flyout: tiles and panel take the theme's radius as it is (8 px in the Windows 11 theme).
    Fluent,
    /// Source-measured Dux two-column tiles and tonal strip sliders.
    Dux,
    /// GNOME Shell 51.1 quick settings: pill toggles in two columns, slider rows and an icon-button system row (`gnome.rs`).
    /// Selectable under any theme; the content and actions are the common controller's.
    Gnome,
}

impl Style {
    pub fn parse(s: &str) -> Option<Style> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Style::Auto,
            "card" => Style::Card,
            "fluent" => Style::Fluent,
            "dux" => Style::Dux,
            "gnome" => Style::Gnome,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Style::Auto => "auto",
            Style::Card => "card",
            Style::Fluent => "fluent",
            Style::Dux => "dux",
            Style::Gnome => "gnome",
        }
    }

    /// The looks the hosts give a clipped scrolling viewport and keyboard focus (Up/Down/Enter, a focus ring, wheel scrolling on a short
    /// screen): Dux and GNOME. The macOS/Windows looks are short enough to need neither.
    pub fn scrolls(self) -> bool {
        matches!(self, Style::Dux | Style::Gnome)
    }

    /// `auto` resolved for a panel that hangs from a Windows-style taskbar (or not).
    pub fn resolve(self, windows_taskbar: bool) -> Style {
        match self {
            Style::Auto if windows_taskbar => Style::Fluent,
            Style::Auto => Style::Card,
            s => s,
        }
    }
}

/// `decebal.json` -> `"control_center"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Off hides every entry point (the bar button, the taskbar button) and makes the command and hot-corner action do nothing.
    pub enabled: bool,
    pub position: Corner,
    pub style: Style,
    pub tiles: Tiles,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: true, position: Corner::Auto, style: Style::Auto, tiles: Tiles::ALL }
    }
}

impl Settings {
    /// Reads the object leniently; anything that is not understood keeps its default.
    pub fn from_json(v: &Value) -> Settings {
        let mut s = Settings::default();
        let Some(o) = v.as_object() else { return s };
        if let Some(b) = o.get("enabled").and_then(Value::as_bool) {
            s.enabled = b;
        }
        if let Some(c) = o.get("position").and_then(Value::as_str).and_then(Corner::parse) {
            s.position = c;
        }
        if let Some(st) = o.get("style").and_then(Value::as_str).and_then(Style::parse) {
            s.style = st;
        }
        if let Some(a) = o.get("tiles").and_then(Value::as_array) {
            s.tiles = Tiles::from_names(a.iter().filter_map(Value::as_str));
        }
        s
    }

    /// The canonical object written back to `decebal.json`.
    pub fn to_json(&self) -> Value {
        json!({"enabled": self.enabled, "position": self.position.as_str(), "style": self.style.as_str(), "tiles": self.tiles.names()})
    }
}

impl<'de> serde::Deserialize<'de> for Settings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Settings::from_json(&Value::deserialize(d)?))
    }
}

// ------------------------------------------------------------------ placement

/// Gap between a bar and a summoned panel, and between the panel and a screen edge that has no bar.
pub const GAP: f32 = 8.0;

/// What is on screen that the panel must keep clear of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Host {
    /// The macOS menu bar across the top (`thickness` px).
    MenuBar { thickness: f32 },
    /// The Windows taskbar along the bottom (or, `top`, the top).
    Taskbar { top: bool, thickness: f32 },
    /// The Caelestia pill bar on `position`; `thickness` is its width (height when horizontal).
    Pill { position: BarPosition, thickness: f32 },
    /// A bar or taskbar on an edge the variants above cannot name (a bottom or side menu, floating or Dux bar; a side
    /// taskbar), from the shared panel resolution. `thickness` is in surface pixels; `taskbar` picks the `fluent` look.
    Panel { edge: crate::panel::Edge, thickness: f32, taskbar: bool },
    /// No bar at all.
    Nothing,
}

impl Host {
    /// The host that is on screen for these settings and theme: the taskbar when the dock is in the Windows style and shown,
    /// else the pill bar or the menu bar when `bar.enabled` allows it.
    ///
    /// The edge is the resolved one (Settings → Panels, the theme, the old `windowsPosition`), and the thickness is in
    /// surface pixels: the taskbar's metrics are already, a bar's logical thickness is multiplied by
    /// [`crate::backend::surface_scale`] since decbar draws at the display scale on X11.
    pub fn detect(shell: &crate::config::ShellSettings, dock: &crate::config::DockSettings, pill: Option<BarPosition>, scale: f32) -> Host {
        Self::detect_for(shell, dock, pill, scale, crate::backend::surface_scale(scale))
    }

    /// [`Host::detect`] with the bar's surface scale given (tests).
    pub fn detect_for(shell: &crate::config::ShellSettings, dock: &crate::config::DockSettings, pill: Option<BarPosition>, scale: f32, bar_scale: f32) -> Host {
        use crate::config::DockStyle;
        use crate::panel::Edge;
        let windows = dock.style == DockStyle::Windows;
        if windows && shell.dock_visible(pill.is_some()) {
            let m = crate::taskbar::Metrics::new(scale, dock.windows_align);
            return match crate::theme::th().taskbar_panel(dock).edge {
                Edge::Top => Host::Taskbar { top: true, thickness: m.bar_h },
                Edge::Bottom => Host::Taskbar { top: false, thickness: m.bar_h },
                edge => Host::Panel { edge, thickness: m.bar_h, taskbar: true },
            };
        }
        if shell.bar_visible(dock.style) {
            return match pill {
                Some(position) => Host::Pill { position, thickness: crate::theme::pill_bar_thickness(position) * bar_scale },
                None => {
                    let th = crate::theme::th();
                    let edge = th.bar_panel().edge;
                    let thickness = th.bar_thickness(edge) * bar_scale;
                    match edge {
                        Edge::Top => Host::MenuBar { thickness },
                        edge => Host::Panel { edge, thickness, taskbar: false },
                    }
                }
            };
        }
        Host::Nothing
    }

    /// The Windows-style taskbar is what the panel hangs from (picks the `fluent` look for `style = "auto"`).
    pub fn is_taskbar(&self) -> bool {
        matches!(self, Host::Taskbar { .. } | Host::Panel { taskbar: true, .. })
    }

    /// Thickness this host takes from each screen edge: (left, right, top, bottom).
    fn insets(&self) -> (f32, f32, f32, f32) {
        match *self {
            Host::MenuBar { thickness } => (0.0, 0.0, thickness, 0.0),
            Host::Taskbar { top: true, thickness } => (0.0, 0.0, thickness, 0.0),
            Host::Taskbar { top: false, thickness } => (0.0, 0.0, 0.0, thickness),
            Host::Pill { position, thickness } => match position {
                BarPosition::Left => (thickness, 0.0, 0.0, 0.0),
                BarPosition::Right => (0.0, thickness, 0.0, 0.0),
                BarPosition::Top => (0.0, 0.0, thickness, 0.0),
                BarPosition::Bottom => (0.0, 0.0, 0.0, thickness),
                BarPosition::Split => (0.0, 0.0, thickness, thickness),
            },
            Host::Panel { edge, thickness, .. } => match edge {
                crate::panel::Edge::Left => (thickness, 0.0, 0.0, 0.0),
                crate::panel::Edge::Right => (0.0, thickness, 0.0, 0.0),
                crate::panel::Edge::Top => (0.0, 0.0, thickness, 0.0),
                crate::panel::Edge::Bottom => (0.0, 0.0, 0.0, thickness),
            },
            Host::Nothing => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// The corner `auto` means for this host: where its own button is.
    fn auto_corner(&self) -> Corner {
        match *self {
            Host::MenuBar { .. } | Host::Nothing => Corner::TopRight,
            Host::Taskbar { top, .. } => {
                if top {
                    Corner::TopRight
                } else {
                    Corner::BottomRight
                }
            }
            Host::Pill { position, .. } => match position {
                BarPosition::Left => Corner::TopLeft,
                BarPosition::Right | BarPosition::Top => Corner::TopRight,
                // the tray, with the Control Center tile, is in the bottom half of a split bar
                BarPosition::Bottom | BarPosition::Split => Corner::BottomRight,
            },
            // the tray, with the Control Center button, ends a bottom or side panel (the foot of a side one)
            Host::Panel { edge, .. } => match edge {
                crate::panel::Edge::Top => Corner::TopRight,
                crate::panel::Edge::Left => Corner::BottomLeft,
                crate::panel::Edge::Right | crate::panel::Edge::Bottom => Corner::BottomRight,
            },
        }
    }
}

/// Where a summoned panel goes: which corner of the monitor, and how far from each of its two edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub corner: Corner,
    /// Distance from the left or right edge (whichever the corner names).
    pub inset_x: f32,
    /// Distance from the top or bottom edge.
    pub inset_y: f32,
}

impl Placement {
    pub fn is_bottom(&self) -> bool {
        matches!(self.corner, Corner::BottomRight | Corner::BottomLeft)
    }
    pub fn is_right(&self) -> bool {
        matches!(self.corner, Corner::TopRight | Corner::BottomRight)
    }

    /// Top-left of a `pw` x `ph` panel on a `w` x `h` monitor, always inside it.
    pub fn origin(&self, w: f32, h: f32, pw: f32, ph: f32) -> (f32, f32) {
        let x = if self.is_right() { w - self.inset_x - pw } else { self.inset_x };
        let y = if self.is_bottom() { h - self.inset_y - ph } else { self.inset_y };
        (x.clamp(0.0, (w - pw).max(0.0)), y.clamp(0.0, (h - ph).max(0.0)))
    }
}

/// The corner (`auto` follows the host) and the clearances for it: the host's thickness plus [`GAP`] on the edge it sits on,
/// [`GAP`] on the others.
pub fn place(host: &Host, want: Corner) -> Placement {
    let corner = if want == Corner::Auto { host.auto_corner() } else { want };
    let (l, r, t, b) = host.insets();
    let side = |inset: f32| if inset > 0.0 { inset + GAP } else { GAP };
    let p = Placement { corner, inset_x: 0.0, inset_y: 0.0 };
    Placement { inset_x: side(if p.is_right() { r } else { l }), inset_y: side(if corner == Corner::BottomRight || corner == Corner::BottomLeft { b } else { t }), ..p }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_hosts_clear_the_slimmer_strip_and_side_hosts_keep_their_width() {
        let shell = crate::config::ShellSettings::default();
        let dock = crate::config::DockSettings::default();
        for position in BarPosition::ALL {
            let thickness = match position {
                BarPosition::Left | BarPosition::Right => 52.0,
                _ => 40.0,
            };
            let host = Host::detect(&shell, &dock, Some(position), 1.0);
            assert_eq!(host, Host::Pill { position, thickness });
            let p = place(&host, Corner::Auto);
            let inset = match position {
                BarPosition::Left | BarPosition::Right => p.inset_x,
                _ => p.inset_y,
            };
            assert_eq!(inset, thickness + GAP);
        }
    }

    #[test]
    fn defaults_make_the_panel_present_with_every_tile() {
        let s = Settings::default();
        assert!(s.enabled && s.position == Corner::Auto && s.style == Style::Auto && s.tiles == Tiles::ALL);
        for v in [json!(null), json!({}), json!("yes"), json!(3)] {
            assert_eq!(Settings::from_json(&v), s, "{v}");
        }
    }

    #[test]
    fn reading_is_lenient_and_writing_round_trips() {
        let v = json!({"enabled": false, "position": "Bottom_Left", "style": "fluent", "tiles": ["sound", "bogus", "actions"]});
        let s = Settings::from_json(&v);
        assert!(!s.enabled);
        assert_eq!((s.position, s.style), (Corner::BottomLeft, Style::Fluent));
        assert_eq!(s.tiles.names(), vec!["sound", "actions"]);
        assert_eq!(Settings::from_json(&s.to_json()), s);
        let bad = Settings::from_json(&json!({"enabled": "no", "position": "middle", "style": 4, "tiles": []}));
        assert_eq!(bad, Settings::default(), "nonsense keeps the defaults; an empty list means every tile");
        assert_eq!(Tiles::from_names(["nope"]), Tiles::ALL);
    }

    #[test]
    fn serde_goes_through_the_lenient_reader() {
        let s: Settings = serde_json::from_str(r#"{"position": "top-left", "tiles": ["media"]}"#).unwrap();
        assert_eq!((s.position, s.tiles.names()), (Corner::TopLeft, vec!["media"]));
    }

    #[test]
    fn every_name_parses_and_round_trips() {
        for n in POSITION_NAMES {
            assert_eq!(Corner::parse(n).map(Corner::as_str), Some(n));
        }
        for n in STYLE_NAMES {
            assert_eq!(Style::parse(n).map(Style::as_str), Some(n));
        }
        for n in TILE_NAMES {
            assert_eq!(Tiles::from_names([n]).names(), vec![n]);
        }
        assert_eq!(Style::Auto.resolve(true), Style::Fluent);
        assert_eq!(Style::Auto.resolve(false), Style::Card);
        assert_eq!(Style::Card.resolve(true), Style::Card, "an explicit style wins");
    }

    #[test]
    fn auto_goes_where_each_hosts_own_button_is() {
        let at = |h: Host| place(&h, Corner::Auto);
        let menu = at(Host::MenuBar { thickness: 26.0 });
        assert_eq!((menu.corner, menu.inset_y, menu.inset_x), (Corner::TopRight, 26.0 + GAP, GAP));
        let task = at(Host::Taskbar { top: false, thickness: 48.0 });
        assert_eq!((task.corner, task.inset_y, task.inset_x), (Corner::BottomRight, 48.0 + GAP, GAP), "above the taskbar");
        assert_eq!(at(Host::Taskbar { top: true, thickness: 48.0 }).corner, Corner::TopRight);
        let t = 52.0;
        let left = at(Host::Pill { position: BarPosition::Left, thickness: t });
        assert_eq!((left.corner, left.inset_x, left.inset_y), (Corner::TopLeft, t + GAP, GAP), "beside the left bar");
        let right = at(Host::Pill { position: BarPosition::Right, thickness: t });
        assert_eq!((right.corner, right.inset_x), (Corner::TopRight, t + GAP));
        assert_eq!(at(Host::Pill { position: BarPosition::Top, thickness: t }).inset_y, t + GAP);
        let bottom = at(Host::Pill { position: BarPosition::Bottom, thickness: t });
        assert_eq!((bottom.corner, bottom.inset_y), (Corner::BottomRight, t + GAP));
        assert_eq!(at(Host::Pill { position: BarPosition::Split, thickness: t }).corner, Corner::BottomRight, "the tray is in the bottom half");
        assert_eq!(at(Host::Nothing), Placement { corner: Corner::TopRight, inset_x: GAP, inset_y: GAP });
    }

    #[test]
    fn an_explicit_corner_still_clears_the_bar_on_its_edge_only() {
        let task = Host::Taskbar { top: false, thickness: 48.0 };
        let p = place(&task, Corner::TopLeft);
        assert_eq!((p.corner, p.inset_x, p.inset_y), (Corner::TopLeft, GAP, GAP), "the taskbar is at the bottom: no clearance on top");
        let p = place(&task, Corner::BottomLeft);
        assert_eq!(p.inset_y, 48.0 + GAP);
        let left = Host::Pill { position: BarPosition::Left, thickness: 52.0 };
        assert_eq!(place(&left, Corner::BottomLeft).inset_x, 52.0 + GAP);
        assert_eq!(place(&left, Corner::BottomRight).inset_x, GAP, "the far side is free");
        let split = Host::Pill { position: BarPosition::Split, thickness: 52.0 };
        assert_eq!(place(&split, Corner::TopRight).inset_y, 52.0 + GAP, "both halves of a split bar are cleared");
        assert_eq!(place(&split, Corner::BottomLeft).inset_y, 52.0 + GAP);
    }

    #[test]
    fn the_panel_stays_inside_the_monitor_in_every_corner() {
        let hosts = [
            Host::MenuBar { thickness: 26.0 },
            Host::Taskbar { top: false, thickness: 48.0 },
            Host::Taskbar { top: true, thickness: 48.0 },
            Host::Pill { position: BarPosition::Left, thickness: 52.0 },
            Host::Pill { position: BarPosition::Right, thickness: 52.0 },
            Host::Pill { position: BarPosition::Split, thickness: 52.0 },
            Host::Nothing,
        ];
        for h in hosts {
            for c in [Corner::Auto, Corner::TopRight, Corner::TopLeft, Corner::BottomRight, Corner::BottomLeft] {
                for (w, sh) in [(1920.0, 1080.0), (1280.0, 800.0), (360.0, 400.0)] {
                    let (x, y) = place(&h, c).origin(w, sh, 340.0, 520.0);
                    assert!(x >= 0.0 && y >= 0.0 && x + 340.0 <= w.max(340.0) && y + 520.0 <= sh.max(520.0), "{h:?} {c:?} {w}x{sh}: {x},{y}");
                }
            }
        }
        let p = place(&Host::Taskbar { top: false, thickness: 48.0 }, Corner::Auto);
        assert_eq!(p.origin(1920.0, 1080.0, 340.0, 520.0), (1920.0 - GAP - 340.0, 1080.0 - 48.0 - GAP - 520.0));
    }

    #[test]
    fn a_summoned_panel_clears_a_taskbar_or_bar_on_any_edge_at_the_display_scale() {
        use crate::config::{DockSettings, DockStyle, ShellSettings};
        use crate::panel::Edge;
        let shell = ShellSettings::default();
        let mut dock = DockSettings { style: DockStyle::Windows, ..DockSettings::default() };
        // the taskbar on each edge (Settings → Panels writes `edge`); auto opens at its tray end, clear of its strip
        for (edge, corner) in [(Edge::Bottom, Corner::BottomRight), (Edge::Top, Corner::TopRight), (Edge::Left, Corner::BottomLeft), (Edge::Right, Corner::BottomRight)] {
            dock.edge = Some(edge);
            for scale in [1.0, 1.25, 2.0] {
                let host = Host::detect_for(&shell, &dock, None, scale, scale);
                assert!(host.is_taskbar(), "{edge:?}");
                let thick = crate::taskbar::Metrics::new(scale, dock.windows_align).bar_h;
                let p = place(&host, Corner::Auto);
                assert_eq!(p.corner, corner, "{edge:?}");
                let (x, y) = p.origin(1600.0, 1000.0, 360.0, 500.0);
                let clear = match edge {
                    Edge::Bottom => y + 500.0 <= 1000.0 - thick - GAP + 0.01,
                    Edge::Top => y >= thick + GAP - 0.01,
                    Edge::Left => x >= thick + GAP - 0.01,
                    Edge::Right => x + 360.0 <= 1600.0 - thick - GAP + 0.01,
                };
                assert!(clear, "{edge:?} {scale}: panel at {x},{y} overlaps the {thick} px taskbar");
                // an explicit corner on the far side still keeps clear of the strip
                for want in [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight] {
                    let (x, y) = place(&host, want).origin(1600.0, 1000.0, 360.0, 500.0);
                    let overlaps = match edge {
                        Edge::Bottom => y + 500.0 > 1000.0 - thick,
                        Edge::Top => y < thick,
                        Edge::Left => x < thick,
                        Edge::Right => x + 360.0 > 1600.0 - thick,
                    };
                    assert!(!overlaps, "{edge:?} {want:?}");
                }
            }
        }
        // bars on the other edges: their surface thickness is cleared, and a bar drawn at 2x is twice as thick
        for edge in [Edge::Bottom, Edge::Left, Edge::Right] {
            let host = Host::Panel { edge, thickness: 26.0 * 2.0, taskbar: false };
            assert!(!host.is_taskbar());
            let p = place(&host, Corner::Auto);
            let inset = if edge == Edge::Bottom { p.inset_y } else { p.inset_x };
            assert_eq!(inset, 52.0 + GAP, "{edge:?}");
        }
        let one = Host::detect_for(&shell, &DockSettings::default(), Some(BarPosition::Left), 1.0, 1.0);
        let two = Host::detect_for(&shell, &DockSettings::default(), Some(BarPosition::Left), 2.0, 2.0);
        assert!(matches!((one, two), (Host::Pill { thickness: a, .. }, Host::Pill { thickness: b, .. }) if b == 2.0 * a));
    }

    #[test]
    fn detection_follows_the_dock_style_and_the_bar_gate() {
        use crate::config::{DockSettings, DockStyle, ShellSettings, Toggle};
        let mut dock = DockSettings::default();
        let mut shell = ShellSettings::default();
        assert!(matches!(Host::detect(&shell, &dock, None, 1.0), Host::MenuBar { .. }));
        assert!(matches!(Host::detect(&shell, &dock, Some(BarPosition::Top), 1.0), Host::Pill { position: BarPosition::Top, .. }));
        shell.bar.enabled = Toggle::Flag(false);
        assert_eq!(Host::detect(&shell, &dock, None, 1.0), Host::Nothing);
        shell.bar.enabled = Toggle::Flag(true);
        dock.style = DockStyle::Windows;
        let h = Host::detect(&shell, &dock, None, 1.0);
        assert!(matches!(h, Host::Taskbar { top: false, thickness } if thickness == 48.0), "{h:?}");
        assert!(h.is_taskbar());
        shell.dock.enabled = Toggle::Flag(false);
        assert!(matches!(Host::detect(&shell, &dock, None, 1.0), Host::MenuBar { .. }), "no dock: the (forced on) bar is the host");
    }
}
