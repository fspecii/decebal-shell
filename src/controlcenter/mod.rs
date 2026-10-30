//! The Control Center (quick settings) panel, ported from the original `ambsd.mac-control`.
//!
//! `layout` turns a [`Snapshot`] of system state into a flat display list; `draw` paints it and
//! `hit` maps pointer positions back to [`Target`]s, so drawing and input share one layout.

pub mod settings;
pub mod sys;

use self::settings::{Style, Tiles};
use crate::gfx::{color, fill_circle, fill_rounded, stroke_rounded};
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use tiny_skia::Pixmap;

pub const PANEL_W: f32 = 340.0;

/// Shared native layout contract; section two supplies the Dux surface implementation.
pub fn panel_width(_style: Style) -> f32 {
    PANEL_W
}

pub fn layout_styled(s: &Snapshot, tiles: &Tiles, _style: Style) -> (Vec<Item>, f32) {
    layout_with(s, tiles)
}
const PAD: f32 = 12.0;
const GAP: f32 = 10.0;

/// Everything the panel shows, gathered off the UI thread.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub net: Option<sys::Net>,
    pub bluetooth: Option<sys::Bluetooth>,
    pub volume: Option<sys::Volume>,
    pub sink: String,
    pub brightness: Option<u32>,
    pub night_shift: Option<bool>,
    pub dnd: Option<bool>,
    pub media: Option<sys::Media>,
    pub awake: bool,
    pub light: bool,
    pub battery: Option<sys::Battery>,
    /// `decutil` toggles; `None` when decutil is not installed (the row is hidden).
    pub util: Option<sys::Utils>,
}

impl Snapshot {
    /// Collects every probe. Slow and blocking: call from a worker thread only.
    pub fn collect() -> Snapshot {
        Snapshot {
            net: sys::probe_net(),
            bluetooth: sys::probe_bluetooth(),
            volume: None,
            sink: String::new(),
            brightness: sys::probe_brightness(),
            night_shift: sys::probe_night_shift(),
            dnd: sys::probe_dnd(),
            media: sys::probe_media(),
            awake: sys::stay_awake(),
            light: sys::is_light(),
            battery: sys::probe_battery(),
            util: sys::probe_utils(),
        }
        .with_volume()
    }

    fn with_volume(mut self) -> Self {
        if let Some((v, name)) = sys::probe_volume() {
            self.volume = Some(v);
            self.sink = name;
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Wifi,
    Bluetooth,
    Network,
    Focus,
    NightShift,
    DarkMode,
    Brightness,
    Volume,
    MediaPrev,
    MediaToggle,
    MediaNext,
    StayAwake,
    Screenshot,
    Record,
    IdleInhibit,
    GameMode,
    Dnd,
    Notifications,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Primary,
    Secondary,
    White,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Draw {
    /// Frosted tile.
    Tile,
    /// Round icon toggle, accent when `on`.
    Circle {
        icon: &'static str,
        on: bool,
    },
    /// Small icon (no background).
    Glyph {
        icon: &'static str,
        size: f32,
        ink: Ink,
    },
    Text {
        text: String,
        size: f32,
        weight: Weight,
        ink: Ink,
        align: Align,
    },
    /// Invisible click area.
    Hit,
    /// Pill slider with its icon inside.
    Slider {
        value: f32,
        icon: &'static str,
    },
    /// Square action tile background (accent when `on`).
    SmallTile {
        on: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub r: R,
    pub draw: Draw,
    pub target: Option<Target>,
}

fn item(x: f32, y: f32, w: f32, h: f32, draw: Draw, target: Option<Target>) -> Item {
    Item { r: R { x, y, w, h }, draw, target }
}

fn text(x: f32, y: f32, w: f32, t: &str, size: f32, weight: Weight, ink: Ink) -> Item {
    item(x, y, w, size + 4.0, Draw::Text { text: t.to_string(), size, weight, ink, align: Align::Left }, None)
}

fn caption(x: f32, y: f32, w: f32, t: &str, ink: Ink) -> Item {
    item(x, y, w, 15.0, Draw::Text { text: t.to_string(), size: 11.0, weight: Weight::Regular, ink, align: Align::Center }, None)
}

pub fn speaker_icon(v: &sys::Volume) -> &'static str {
    if v.muted || v.level <= 0.0 {
        "speaker-muted"
    } else if v.level >= 0.67 {
        "speaker-3"
    } else if v.level >= 0.34 {
        "speaker-2"
    } else {
        "speaker-1"
    }
}

/// Maps a pointer x inside a slider to 0..=1.
pub fn slider_value(px: f32, x: f32, w: f32) -> f32 {
    if w <= 0.0 {
        return 0.0;
    }
    ((px - x) / w).clamp(0.0, 1.0)
}

/// One connectivity row: a toggle circle, a title and a status line.
struct ConnRow<'a> {
    icon: &'static str,
    on: bool,
    title: &'a str,
    status: &'a str,
    toggle: Option<Target>,
    /// Clicking the name opens this (an invisible hit area).
    open: Option<Target>,
}

fn conn_row(out: &mut Vec<Item>, x: f32, y: f32, w: f32, row: ConnRow) {
    out.push(item(x, y + 3.0, 30.0, 30.0, Draw::Circle { icon: row.icon, on: row.on }, row.toggle));
    if row.open.is_some() {
        out.push(item(x + 40.0, y, w - 40.0, 36.0, Draw::Hit, row.open));
    }
    out.push(text(x + 40.0, y + 3.0, w - 40.0, row.title, 13.0, Weight::SemiBold, Ink::Primary));
    out.push(text(x + 40.0, y + 19.0, w - 40.0, row.status, 11.0, Weight::Regular, Ink::Secondary));
}

const ROW_H: f32 = 128.0;

/// The connectivity tile (Wi-Fi, Bluetooth, wired network) at `y`, `left_w` wide.
fn conn_tiles(out: &mut Vec<Item>, s: &Snapshot, y: f32, left_w: f32) {
    out.push(item(PAD, y, left_w, ROW_H, Draw::Tile, None));
    let mut cy = y + 10.0;
    let cw = left_w - 20.0;
    if let Some(net) = &s.net {
        let status = if !net.wifi_enabled {
            "Off".to_string()
        } else if net.wifi_name.is_empty() {
            "Not Connected".into()
        } else {
            net.wifi_name.clone()
        };
        conn_row(
            out,
            PAD + 10.0,
            cy,
            cw,
            ConnRow { icon: if net.wifi_enabled { "wifi-3" } else { "wifi-off" }, on: net.wifi_enabled, title: "Wi-Fi", status: &status, toggle: Some(Target::Wifi), open: None },
        );
        cy += 40.0;
    }
    if let Some(bt) = &s.bluetooth {
        let icon = if !bt.powered {
            "bluetooth-off"
        } else if bt.connected > 0 {
            "bluetooth-connected"
        } else {
            "bluetooth"
        };
        let status = if !bt.powered {
            "Off".to_string()
        } else if bt.connected > 0 {
            format!("{} connected", bt.connected)
        } else {
            "On".into()
        };
        conn_row(out, PAD + 10.0, cy, cw, ConnRow { icon, on: bt.powered, title: "Bluetooth", status: &status, toggle: Some(Target::Bluetooth), open: None });
        cy += 40.0;
    }
    if let Some(net) = &s.net {
        let status = if net.wired_name.is_empty() { "Not Connected".to_string() } else { net.wired_name.clone() };
        conn_row(out, PAD + 10.0, cy, cw, ConnRow { icon: "ethernet", on: !net.wired_name.is_empty(), title: "Network", status: &status, toggle: None, open: Some(Target::Network) });
    }
}

/// Focus, Night Shift and Dark Mode at `y`, in the column that starts at `right_x` and is `right_w` wide.
fn focus_tiles(out: &mut Vec<Item>, s: &Snapshot, y: f32, right_x: f32, right_w: f32) {
    // Focus tile.
    out.push(item(right_x, y, right_w, 59.0, Draw::Tile, None));
    let focus_on = s.dnd == Some(true);
    out.push(item(right_x + 12.0, y + 14.5, 30.0, 30.0, Draw::Circle { icon: "moon", on: focus_on }, s.dnd.map(|_| Target::Focus)));
    out.push(text(right_x + 52.0, y + 13.0, right_w - 60.0, "Focus", 13.0, Weight::SemiBold, Ink::Primary));
    let focus_status = match s.dnd {
        Some(true) => "Do Not Disturb",
        Some(false) => "Off",
        None => "Unavailable",
    };
    out.push(text(right_x + 52.0, y + 29.0, right_w - 60.0, focus_status, 11.0, Weight::Regular, Ink::Secondary));

    // Night Shift and Dark/Light tiles.
    let small_w = (right_w - GAP) / 2.0;
    let sy = y + 59.0 + GAP;
    let night_on = s.night_shift == Some(true);
    out.push(item(right_x, sy, small_w, 59.0, Draw::SmallTile { on: night_on }, s.night_shift.map(|_| Target::NightShift)));
    out.push(item(right_x + small_w / 2.0 - 10.0, sy + 13.0, 20.0, 20.0, Draw::Glyph { icon: "night-shift", size: 20.0, ink: if night_on { Ink::White } else { Ink::Primary } }, None));
    out.push(caption(right_x + 4.0, sy + 36.0, small_w - 8.0, "Night Shift", if night_on { Ink::White } else { Ink::Primary }));
    let dark_on = !s.light;
    let dx = right_x + small_w + GAP;
    out.push(item(dx, sy, small_w, 59.0, Draw::SmallTile { on: dark_on }, Some(Target::DarkMode)));
    out.push(item(dx + small_w / 2.0 - 10.0, sy + 13.0, 20.0, 20.0, Draw::Glyph { icon: "dark-mode", size: 20.0, ink: if dark_on { Ink::White } else { Ink::Primary } }, None));
    out.push(caption(dx + 4.0, sy + 36.0, small_w - 8.0, if s.light { "Light" } else { "Dark" }, if dark_on { Ink::White } else { Ink::Primary }));
}

/// Builds the display list for `s` with every tile group, the panel's top-left at the origin. Returns the list and the panel height.
pub fn layout(s: &Snapshot) -> (Vec<Item>, f32) {
    layout_with(s, &Tiles::ALL)
}

/// [`layout`] with only the tile groups `tiles` names (a theme's or the user's `control_center.tiles`). When just one half of the
/// first row is wanted it takes the whole width.
pub fn layout_with(s: &Snapshot, tiles: &Tiles) -> (Vec<Item>, f32) {
    let mut out = Vec::new();
    let inner = PANEL_W - 2.0 * PAD;
    let mut y = PAD;

    // Row 1: connectivity (left) and Focus / Night Shift / Dark Mode (right).
    let (left_w, right_w, right_x) = match (tiles.connectivity, tiles.focus) {
        (true, true) => ((inner - GAP) * 0.55, (inner - GAP) * 0.45, PAD + (inner - GAP) * 0.55 + GAP),
        (true, false) => (inner, 0.0, PAD),
        _ => (0.0, inner, PAD),
    };
    if tiles.connectivity {
        conn_tiles(&mut out, s, y, left_w);
    }
    if tiles.focus {
        focus_tiles(&mut out, s, y, right_x, right_w);
    }
    if tiles.connectivity || tiles.focus {
        y += ROW_H + GAP;
    }

    // Display brightness.
    if let Some(b) = s.brightness.filter(|_| tiles.display) {
        out.push(item(PAD, y, inner, 64.0, Draw::Tile, None));
        out.push(text(PAD + 12.0, y + 9.0, inner - 24.0, "Display", 13.0, Weight::SemiBold, Ink::Primary));
        out.push(item(PAD + 12.0, y + 30.0, inner - 24.0, 26.0, Draw::Slider { value: b as f32 / 100.0, icon: "sun-small" }, Some(Target::Brightness)));
        y += 64.0 + GAP;
    }

    // Sound.
    if let Some(v) = s.volume.as_ref().filter(|_| tiles.sound) {
        out.push(item(PAD, y, inner, 64.0, Draw::Tile, None));
        out.push(text(PAD + 12.0, y + 9.0, inner * 0.4, "Sound", 13.0, Weight::SemiBold, Ink::Primary));
        let name = if s.sink.is_empty() { "Output" } else { s.sink.as_str() };
        out.push(item(
            PAD + inner * 0.4,
            y + 11.0,
            inner * 0.6 - 12.0,
            15.0,
            Draw::Text { text: name.to_string(), size: 11.0, weight: Weight::Regular, ink: Ink::Secondary, align: Align::Right },
            None,
        ));
        let shown = if v.muted { 0.0 } else { v.level.clamp(0.0, 1.0) };
        out.push(item(PAD + 12.0, y + 30.0, inner - 24.0, 26.0, Draw::Slider { value: shown, icon: speaker_icon(v) }, Some(Target::Volume)));
        y += 64.0 + GAP;
    }

    // Now Playing.
    if let Some(m) = s.media.as_ref().filter(|_| tiles.media) {
        out.push(item(PAD, y, inner, 64.0, Draw::Tile, None));
        out.push(item(PAD + 10.0, y + 10.0, 44.0, 44.0, Draw::SmallTile { on: false }, None));
        let controls_x = PAD + inner - 10.0 - 3.0 * 28.0 - 8.0;
        let tw = controls_x - (PAD + 64.0) - 8.0;
        let title = if m.title.is_empty() { "Not Playing" } else { m.title.as_str() };
        out.push(text(PAD + 64.0, y + 15.0, tw, title, 13.0, Weight::SemiBold, Ink::Primary));
        out.push(text(PAD + 64.0, y + 32.0, tw, &m.artist, 11.0, Weight::Regular, Ink::Secondary));
        for (i, (icon, t)) in [("backward", Target::MediaPrev), (if m.playing { "pause" } else { "play" }, Target::MediaToggle), ("forward", Target::MediaNext)].into_iter().enumerate() {
            let bx = controls_x + i as f32 * 32.0;
            out.push(item(bx, y + 18.0, 28.0, 28.0, Draw::Glyph { icon, size: 16.0, ink: Ink::Primary }, Some(t)));
        }
        y += 64.0 + GAP;
    }

    // decutil quick toggles: Recording, Idle inhibit, Game mode, Do not disturb.
    let qw = (inner - 3.0 * GAP) / 4.0;
    if let Some(u) = s.util.as_ref().filter(|_| tiles.toggles) {
        let toggles: [(&'static str, &str, bool, Option<Target>); 4] = [
            ("record", if u.record { "Recording" } else { "Record" }, u.record, Some(Target::Record)),
            ("eye", "Inhibit Idle", u.idle, Some(Target::IdleInhibit)),
            ("gamepad", "Game Mode", u.game, Some(Target::GameMode)),
            (if s.dnd == Some(true) { "bell-off" } else { "bell" }, "DND", s.dnd == Some(true), s.dnd.map(|_| Target::Dnd)),
        ];
        for (i, (icon, label, on, t)) in toggles.into_iter().enumerate() {
            let x = PAD + i as f32 * (qw + GAP);
            out.push(item(x, y, qw, 66.0, Draw::SmallTile { on }, t));
            out.push(item(x + qw / 2.0 - 10.0, y + 13.0, 20.0, 20.0, Draw::Glyph { icon, size: 20.0, ink: if on { Ink::White } else { Ink::Primary } }, None));
            out.push(caption(x + 2.0, y + 43.0, qw - 4.0, label, if on { Ink::White } else { Ink::Primary }));
        }
        y += 66.0 + GAP;
    }

    // Quick actions. With decutil the third tile opens the notification history (Record moved into the row above).
    let third: (&'static str, &str, Target) = if s.util.is_some() { ("bell", "Notifications", Target::Notifications) } else { ("record", "Record", Target::Record) };
    let actions: [(&'static str, &str, bool, Target); 4] = [
        ("cup", "Stay Awake", s.awake, Target::StayAwake),
        ("screenshot", "Screenshot", false, Target::Screenshot),
        (third.0, third.1, false, third.2),
        ("switches", "Settings", false, Target::Settings),
    ];
    if tiles.actions {
        for (i, (icon, label, on, t)) in actions.into_iter().enumerate() {
            let x = PAD + i as f32 * (qw + GAP);
            out.push(item(x, y, qw, 66.0, Draw::SmallTile { on }, Some(t)));
            out.push(item(x + qw / 2.0 - 10.0, y + 13.0, 20.0, 20.0, Draw::Glyph { icon, size: 20.0, ink: if on { Ink::White } else { Ink::Primary } }, None));
            out.push(caption(x + 2.0, y + 43.0, qw - 4.0, label, if on { Ink::White } else { Ink::Primary }));
        }
        y += 66.0;
    } else {
        // the last group above ended with a gap
        y = (y - GAP).max(PAD);
    }
    if out.is_empty() && *tiles != Tiles::ALL {
        // every chosen group is unavailable on this machine (no brightness control, no player): never show an empty box
        return layout_with(s, &Tiles::ALL);
    }

    (out, y + PAD)
}

/// Topmost interactive item under the point (panel-local coordinates).
pub fn hit(items: &[Item], px: f32, py: f32) -> Option<(Target, R)> {
    items.iter().rev().find(|i| i.target.is_some() && i.r.contains(px, py)).and_then(|i| i.target.map(|t| (t, i.r)))
}

fn ink_rgba(ink: Ink) -> (u8, u8, u8, f32) {
    let th = crate::theme::th();
    match ink {
        Ink::Primary => th.text.a(1.0),
        Ink::Secondary => th.text.a(0.55),
        Ink::White => th.accent_text.a(1.0),
    }
}

/// Where and how the panel is painted.
pub struct View<'a> {
    /// Top-left of the panel in the destination pixmap.
    pub origin: (f32, f32),
    pub items: &'a [Item],
    pub height: f32,
    pub hover: Option<Target>,
    /// Already resolved ([`Style::resolve`]): `Auto` is painted as `Card`.
    pub style: Style,
}

/// Paints the panel described by `view` into `pm`.
pub fn draw(pm: &mut Pixmap, view: &View, fonts: &mut Fonts, symbols: &mut SymbolCache) {
    let ((ox, oy), items, height, hover) = (view.origin, view.items, view.height, view.hover);
    let (panel_r, tile_r, fluent) = look(view.style);
    fill_rounded(pm, ox, oy, PANEL_W, height, panel_r, crate::theme::th().overlay.color(crate::theme::th().panel_alpha * 0.9));
    stroke_rounded(pm, ox + 0.5, oy + 0.5, PANEL_W - 1.0, height - 1.0, (panel_r - 0.5).max(0.0), crate::theme::th().outline_c(), 1.0);

    for it in items {
        let (x, y, w, h) = (ox + it.r.x, oy + it.r.y, it.r.w, it.r.h);
        let _ = h;
        let hovered = it.target.is_some() && it.target == hover;
        match &it.draw {
            Draw::Tile => {
                fill_rounded(pm, x, y, w, h, tile_r, crate::theme::th().ink(0.09));
                stroke_rounded(pm, x + 0.5, y + 0.5, w - 1.0, h - 1.0, (tile_r - 0.5).max(0.0), crate::theme::th().ink(0.07), 1.0);
            }
            Draw::SmallTile { on } => {
                let c = if *on { crate::theme::th().accent_c() } else { crate::theme::th().ink(if hovered { 0.14 } else { 0.09 }) };
                fill_rounded(pm, x, y, w, h, tile_r, c);
                if !on {
                    stroke_rounded(pm, x + 0.5, y + 0.5, w - 1.0, h - 1.0, (tile_r - 0.5).max(0.0), crate::theme::th().ink(0.07), 1.0);
                }
            }
            Draw::Circle { icon, on } => {
                let c = if *on { crate::theme::th().accent_c() } else { crate::theme::th().ink(if hovered { 0.2 } else { 0.14 }) };
                if fluent {
                    fill_rounded(pm, x, y, w, h, tile_r.min(w * 0.3), c);
                } else {
                    fill_circle(pm, x + w / 2.0, y + h / 2.0, w / 2.0, c);
                }
                let ink = if *on { crate::theme::th().accent_text.t() } else { crate::theme::th().text.t() };
                blit_symbol(pm, symbols, icon, ink, 17.0, x + w / 2.0, y + h / 2.0);
            }
            Draw::Hit => {}
            Draw::Glyph { icon, size, ink } => {
                if hovered && it.target.is_some() && *size >= 16.0 && w <= 28.0 {
                    fill_circle(pm, x + w / 2.0, y + h / 2.0, w / 2.0, crate::theme::th().ink(0.14));
                }
                let (r, g, b, _) = ink_rgba(*ink);
                blit_symbol(pm, symbols, icon, (r, g, b), *size, x + w / 2.0, y + h / 2.0);
            }
            Draw::Text { text, size, weight, ink, align } => {
                let (r, g, b, a) = ink_rgba(*ink);
                let shown = fit(text, w, *size, *weight, fonts);
                let tw = fonts.measure(&shown, *size, *weight);
                let tx = match align {
                    Align::Left => x,
                    Align::Center => x + (w - tw) / 2.0,
                    Align::Right => x + w - tw,
                };
                fonts.draw(pm, &shown, tx, y, *size, *weight, color(r, g, b, a));
            }
            Draw::Slider { value, icon } => {
                fill_rounded(pm, x, y, w, h, h / 2.0, crate::theme::th().ink(0.14));
                let fw = (w * value.clamp(0.0, 1.0)).max(h);
                fill_rounded(pm, x, y, fw, h, h / 2.0, crate::theme::th().text.color(0.92));
                blit_symbol(pm, symbols, icon, crate::theme::th().overlay.t(), 15.0, x + 6.0 + 7.5, y + h / 2.0);
            }
        }
    }
}

/// (panel radius, tile radius, fluent) of a resolved style: the card look doubles the theme radius for the panel and rounds the
/// tiles to 16 px; the Windows 11 flyout takes the theme radius as it is (8 px there) for both and squares the toggles off.
pub fn look(style: Style) -> (f32, f32, bool) {
    let th = crate::theme::th();
    match style {
        Style::Fluent => (th.r(1.0), th.r(0.75).clamp(3.0, 12.0), true),
        _ => (th.r(2.0), 16.0, false),
    }
}

/// Truncates `s` with an ellipsis so it fits in `max_w`.
pub fn fit(s: &str, max_w: f32, size: f32, weight: Weight, fonts: &Fonts) -> String {
    if fonts.measure(s, size, weight) <= max_w {
        return s.to_string();
    }
    let mut out: String = s.to_string();
    while !out.is_empty() && fonts.measure(&format!("{out}…"), size, weight) > max_w {
        out.pop();
    }
    format!("{}…", out.trim_end())
}

fn blit_symbol(pm: &mut Pixmap, symbols: &mut SymbolCache, name: &str, rgb: (u8, u8, u8), size: f32, cx: f32, cy: f32) {
    let px = (size * 2.0).ceil() as u32;
    if let Some(sym) = symbols.get(name, rgb, px) {
        crate::gfx::blit_scaled(pm, sym, cx - size / 2.0, cy - size / 2.0, size, size, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Snapshot {
        Snapshot {
            net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: "Wired".into() }),
            bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
            volume: Some(sys::Volume { level: 0.5, muted: false }),
            sink: "Headset".into(),
            brightness: Some(40),
            night_shift: Some(false),
            dnd: Some(false),
            media: Some(sys::Media { playing: true, title: "Song".into(), artist: "Artist".into() }),
            awake: false,
            light: false,
            battery: None,
            util: Some(sys::Utils::default()),
        }
    }

    #[test]
    fn full_panel_height_matches_the_original_stack() {
        // 12 pad + 128 + 10 + 64 + 10 + 64 + 10 + 64 + 10 + 66 (decutil toggles) + 10 + 66 + 12 pad
        let (_, h) = layout(&full());
        assert_eq!(h, 12.0 + 128.0 + 10.0 + 64.0 + 10.0 + 64.0 + 10.0 + 64.0 + 10.0 + 66.0 + 10.0 + 66.0 + 12.0);
        // Without decutil the panel is the original stack.
        let (_, h) = layout(&Snapshot { util: None, ..full() });
        assert_eq!(h, 12.0 + 128.0 + 10.0 + 64.0 + 10.0 + 64.0 + 10.0 + 64.0 + 10.0 + 66.0 + 12.0);
    }

    #[test]
    fn missing_tools_hide_their_tiles() {
        let bare = Snapshot::default();
        let (items, h) = layout(&bare);
        assert!(!items.iter().any(|i| matches!(i.target, Some(Target::Brightness | Target::Volume | Target::MediaToggle | Target::Wifi))));
        assert_eq!(h, 12.0 + 128.0 + 10.0 + 66.0 + 12.0);
    }

    #[test]
    fn every_target_is_reachable_with_a_full_snapshot() {
        let (items, _) = layout(&full());
        for t in [
            Target::Wifi,
            Target::Bluetooth,
            Target::Network,
            Target::Focus,
            Target::NightShift,
            Target::DarkMode,
            Target::Brightness,
            Target::Volume,
            Target::MediaPrev,
            Target::MediaToggle,
            Target::MediaNext,
            Target::StayAwake,
            Target::Screenshot,
            Target::Record,
            Target::IdleInhibit,
            Target::GameMode,
            Target::Dnd,
            Target::Notifications,
            Target::Settings,
        ] {
            assert!(items.iter().any(|i| i.target == Some(t)), "{t:?} missing");
        }
    }

    #[test]
    fn utility_tiles_show_live_state() {
        let mut s = full();
        s.util = Some(sys::Utils { record: true, idle: false, game: true });
        s.dnd = Some(true);
        let (items, _) = layout(&s);
        let on = |t: Target| {
            let tile = items.iter().position(|i| i.target == Some(t)).unwrap();
            matches!(items[tile].draw, Draw::SmallTile { on: true })
        };
        assert!(on(Target::Record) && on(Target::GameMode) && on(Target::Dnd));
        assert!(!on(Target::IdleInhibit));
        assert!(items.iter().any(|i| matches!(&i.draw, Draw::Text { text, .. } if text == "Recording")));
        s.util = Some(sys::Utils::default());
        let (items, _) = layout(&s);
        assert!(items.iter().any(|i| matches!(&i.draw, Draw::Text { text, .. } if text == "Record")));
        // The tile of an unavailable DND daemon is not clickable.
        s.dnd = None;
        let (items, _) = layout(&s);
        assert!(!items.iter().any(|i| i.target == Some(Target::Dnd)));
    }

    #[test]
    fn without_decutil_the_legacy_record_tile_stays() {
        let (items, _) = layout(&Snapshot { util: None, ..full() });
        assert!(items.iter().any(|i| i.target == Some(Target::Record)));
        assert!(!items.iter().any(|i| matches!(i.target, Some(Target::IdleInhibit | Target::GameMode | Target::Dnd | Target::Notifications))));
    }

    #[test]
    fn hit_finds_the_slider_and_maps_values() {
        let (items, _) = layout(&full());
        let (t, r) = items.iter().find(|i| i.target == Some(Target::Volume)).map(|i| (i.target.unwrap(), i.r)).unwrap();
        assert_eq!(hit(&items, r.x + 5.0, r.y + 5.0).map(|h| h.0), Some(t));
        assert_eq!(slider_value(r.x, r.x, r.w), 0.0);
        assert_eq!(slider_value(r.x + r.w, r.x, r.w), 1.0);
        assert!((slider_value(r.x + r.w / 2.0, r.x, r.w) - 0.5).abs() < 1e-6);
        assert_eq!(slider_value(r.x - 50.0, r.x, r.w), 0.0);
        assert_eq!(slider_value(r.x + r.w + 50.0, r.x, r.w), 1.0);
        assert_eq!(slider_value(5.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn hit_outside_everything_is_none() {
        let (items, h) = layout(&full());
        assert!(hit(&items, -5.0, 5.0).is_none());
        assert!(hit(&items, 5.0, h + 20.0).is_none());
    }

    #[test]
    fn speaker_icon_follows_level_and_mute() {
        let v = |level, muted| sys::Volume { level, muted };
        assert_eq!(speaker_icon(&v(0.0, false)), "speaker-muted");
        assert_eq!(speaker_icon(&v(0.9, true)), "speaker-muted");
        assert_eq!(speaker_icon(&v(0.2, false)), "speaker-1");
        assert_eq!(speaker_icon(&v(0.5, false)), "speaker-2");
        assert_eq!(speaker_icon(&v(0.8, false)), "speaker-3");
    }

    #[test]
    fn muted_slider_shows_empty() {
        let mut s = full();
        s.volume = Some(sys::Volume { level: 0.8, muted: true });
        let (items, _) = layout(&s);
        let slider = items.iter().find(|i| i.target == Some(Target::Volume)).unwrap();
        assert!(matches!(slider.draw, Draw::Slider { value, .. } if value == 0.0));
    }

    #[test]
    fn wifi_status_text_reflects_state() {
        let mut s = full();
        s.net = Some(sys::Net { wifi_enabled: false, ..Default::default() });
        let (items, _) = layout(&s);
        assert!(items.iter().any(|i| matches!(&i.draw, Draw::Text { text, .. } if text == "Off")));
        s.net = Some(sys::Net { wifi_enabled: true, ..Default::default() });
        let (items, _) = layout(&s);
        assert!(items.iter().any(|i| matches!(&i.draw, Draw::Text { text, .. } if text == "Not Connected")));
    }

    #[test]
    fn dark_tile_label_follows_appearance() {
        let mut s = full();
        s.light = true;
        let (items, _) = layout(&s);
        assert!(items.iter().any(|i| matches!(&i.draw, Draw::Text { text, .. } if text == "Light")));
    }

    #[test]
    fn items_stay_inside_the_panel() {
        let (items, h) = layout(&full());
        for i in &items {
            assert!(i.r.x >= 0.0 && i.r.x + i.r.w <= PANEL_W + 0.01, "{:?} outside width", i);
            assert!(i.r.y >= 0.0 && i.r.y + i.r.h <= h + 0.01, "{:?} outside height", i);
        }
    }
}
