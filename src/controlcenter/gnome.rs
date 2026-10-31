//! GNOME Shell 51.1-style quick settings for the common Control Center (`control_center.style = "gnome"`).
//!
//! An independent native implementation. Only measured *facts* are used, read from the pinned gnome-shell 51.1
//! (`33da06854e229c256afd5872fedd83c28cbfa176`) sources: `gnome-shell-sass/widgets/_quick-settings.scss`, `_slider.scss`,
//! `_message-list.scss`, `_common.scss` (`%button`, typography), `_drawing.scss` (the `button()` colour mixes) and `js/ui/panel.js` /
//! `status/system.js` (the system row, the two-column grid, the item order). No stylesheet text, JavaScript, SVG or font is copied
//! into this MIT crate; Adwaita symbolic icons are looked up at run time in the *installed* icon theme and recoloured, with the built-in
//! glyph set as the fallback when it is absent. See `docs/gnome-theme-ports.md` for the measured table and the remaining gaps.
//!
//! Content and actions are the common controller's: the same [`Snapshot`], [`Tiles`] groups and [`Target`]s as every other look, so
//! Wi-Fi, Bluetooth, brightness, volume, Focus / DND, dark mode, Night Light, media, Stay Awake, Screenshot, Record, Settings and the
//! decutil toggles all stay functional. Nothing here probes, spawns or paints outside `draw`.
//!
//! Layout (top to bottom, only enabled groups): system row (battery readout, Screenshot, Settings) → Sound slider → Display slider →
//! two-column pill grid (connectivity, focus, decutil toggles, then Stay Awake / Notifications) → Now Playing card.
//! Metrics scale with the theme's text scale (`em` = 11 pt = 14.67 px at 100 %): toggles are `12em` × 48 px, padding 18, gaps 12.

use super::*;
use crate::theme::model::Rgb;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

/// `$modal_radius * 2.25` (`.quick-settings { border-radius }`), fixed in px.
pub const PANEL_RADIUS: f32 = 36.0;
/// `.quick-settings { padding: $base_padding * 3 }`.
const PANEL_PAD: f32 = 18.0;
/// `.quick-settings-grid { spacing-rows / spacing-columns: $base_padding * 2 }`.
const GRID_GAP: f32 = 12.0;
/// 11 pt (`$base_font_size`) at 96 dpi: `1em`.
const BASE_EM: f32 = 11.0 * 96.0 / 72.0;
/// `%heading` is bold 11 pt; `%caption` is regular 9 pt (unscaled px; `Fonts` applies the text scale).
const HEADING_PX: f32 = 11.0 * 96.0 / 72.0;
const CAPTION_PX: f32 = 9.0 * 96.0 / 72.0;
/// `.quick-toggle { & > StBoxLayout { padding: 0 $base_padding * 2; &:ltr { padding-left: $base_padding * 2.5 } } spacing: $base_padding * 1.5 }`.
const TOGGLE_PAD_LEFT: f32 = 15.0;
const TOGGLE_PAD_RIGHT: f32 = 12.0;
const TOGGLE_SPACING: f32 = 9.0;
/// `.quick-settings .icon-button { padding: $base_padding * 1.75 }` (fixed) around a 16 px scalable icon.
const ICON_BUTTON_PAD: f32 = 10.5;
/// `.quick-toggle-has-menu .quick-toggle-menu-button { padding: 0.409em 0.71575em }`: scalable, 10.5 px horizontally at 100 %.
const MENU_BUTTON_PAD: f32 = 10.5;
/// `.quick-toggle-has-menu .quick-toggle:ltr > StBoxLayout { padding-right: 0.6135em }`: 9 px at 100 %, scalable.
const SPLIT_TOGGLE_PAD_RIGHT: f32 = 9.0;
/// `.slider { -barlevel-height: 4px }`.
const SLIDER_TROUGH: f32 = 4.0;
/// `.quick-slider .icon-button { padding: $base_padding }`, `.slider-bin { padding: $base_padding }`.
const SLIDER_PAD: f32 = 6.0;

/// Shape of a [`Draw::Toggle`]: the whole pill, or one segment of the split Wi-Fi tile (toggle + menu button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caps {
    Pill,
    Left,
    Right,
}

/// The current text scale (`1.0` = 100 %), bounded so a bad theme cannot make the panel absurd.
pub fn scale() -> f32 {
    crate::theme::th().font_scale.clamp(0.8, 3.0)
}

fn toggle_w(k: f32) -> f32 {
    12.0 * BASE_EM * k
}
fn toggle_h(k: f32) -> f32 {
    3.0 * 16.0 * k
}
fn icon_px(k: f32) -> f32 {
    16.0 * k
}
fn button_d(k: f32) -> f32 {
    icon_px(k) + 2.0 * ICON_BUTTON_PAD
}
fn slider_row_h(k: f32) -> f32 {
    icon_px(k) + 2.0 * SLIDER_PAD
}
fn line(size: f32, k: f32) -> f32 {
    (size * k * 1.25).ceil()
}

/// Panel width at the live text scale: two `12em` toggles, one gap, the padding on both sides (400 px at 100 %).
pub fn panel_width() -> f32 {
    panel_width_at(scale())
}
pub fn panel_width_at(k: f32) -> f32 {
    2.0 * toggle_w(k) + GRID_GAP + 2.0 * PANEL_PAD
}

/// The panel's inner width with `columns` (2, or 1 when two do not fit the host): one column is one toggle wide, but never
/// narrower than the system row's buttons.
fn inner_at(k: f32, columns: u8) -> f32 {
    if columns >= 2 {
        2.0 * toggle_w(k) + GRID_GAP
    } else {
        toggle_w(k).max(4.0 * button_d(k) + 3.0 * GRID_GAP)
    }
}

/// Panel width with `columns` at text scale `k`.
pub fn panel_width_cols(k: f32, columns: u8) -> f32 {
    inner_at(k, columns) + 2.0 * PANEL_PAD
}

/// Columns the grid gets in a room `room` wide at text scale `k`: two when they fit, else one (a narrower screen, a larger
/// text or display scale). What still does not fit is zoomed by the host's viewport.
pub fn columns_for(room: f32, k: f32) -> u8 {
    if panel_width_at(k) <= room + 0.5 {
        2
    } else {
        1
    }
}

pub(super) fn layout(s: &Snapshot, tiles: &Tiles) -> (Vec<Item>, f32) {
    layout_at(s, tiles, scale())
}

/// One quick toggle of the two-column grid.
struct Tile {
    icon: &'static str,
    title: &'static str,
    subtitle: Option<String>,
    on: bool,
    target: Option<Target>,
    /// The Wi-Fi tile's menu button (opens the network editor).
    menu: Option<Target>,
}

fn tile(icon: &'static str, title: &'static str, subtitle: Option<String>, on: bool, target: Option<Target>) -> Tile {
    Tile { icon, title, subtitle, on, target, menu: None }
}

fn sized(x: f32, y: f32, w: f32, h: f32, draw: Draw, target: Option<Target>) -> Item {
    item(x, y, w, h, draw, target)
}

#[allow(clippy::too_many_arguments)]
fn label(x: f32, y: f32, w: f32, t: &str, size: f32, weight: Weight, ink: Ink, k: f32) -> Item {
    sized(x, y, w.max(0.0), line(size, k), Draw::Text { text: t.to_string(), size, weight, ink, align: Align::Left }, None)
}

/// Builds the display list at text scale `k`. The panel's top-left is the origin; the second value is its height.
pub fn layout_at(s: &Snapshot, tiles: &Tiles, k: f32) -> (Vec<Item>, f32) {
    layout_cols(s, tiles, k, 2)
}

/// [`layout_at`] with `columns` grid columns (2 as the Shell has them; 1 for a narrow host). One column keeps every control:
/// the toggles stack, the battery readout takes its own line above the buttons, and the Now Playing controls a line of
/// their own under the title.
pub fn layout_cols(s: &Snapshot, tiles: &Tiles, k: f32, columns: u8) -> (Vec<Item>, f32) {
    let mut out = Vec::new();
    let one = columns < 2;
    let tw = if one { inner_at(k, 1) } else { toggle_w(k) };
    let th = toggle_h(k);
    let inner = inner_at(k, columns);
    let mut y = PANEL_PAD;
    let mut sections = 0;
    let gap = |y: &mut f32, sections: &mut i32| {
        if *sections > 0 {
            *y += GRID_GAP;
        }
        *sections += 1;
    };

    // ---- system row (status/system.js): the power readout, Screenshot, Settings, then Lock and the power menu. With a battery the
    // spacer sits after the readout and every button is right-aligned; without one Screenshot and Settings stay left and Lock and
    // Power are right-aligned. Lock and Power appear only when the common controller found their programs.
    if tiles.actions {
        gap(&mut y, &mut sections);
        let d = button_d(k);
        let mut buttons: Vec<(&'static str, Target)> = vec![("screenshot-recorder-symbolic", Target::Screenshot), ("emblem-system-symbolic", Target::Settings)];
        let mut tail: Vec<(&'static str, Target)> = Vec::new();
        if s.lock {
            tail.push(("system-lock-screen-symbolic", Target::Lock));
        }
        if s.power {
            tail.push(("system-shutdown-symbolic", Target::Power));
        }
        let right_edge = PANEL_PAD + inner;
        let place = |out: &mut Vec<Item>, list: &[(&'static str, Target)], from_right: bool, start: f32, y: f32| {
            let total = list.len() as f32 * d + (list.len().max(1) - 1) as f32 * GRID_GAP;
            let mut bx = if from_right { right_edge - total } else { start };
            for (icon, target) in list {
                out.push(sized(bx, y, d, d, Draw::Circle { icon, on: false }, Some(*target)));
                bx += d + GRID_GAP;
            }
        };
        if let Some(b) = &s.battery {
            let icon = battery_icon(b);
            out.push(sized(PANEL_PAD, y + (d - icon_px(k)) / 2.0, icon_px(k), icon_px(k), Draw::Glyph { icon, size: icon_px(k), ink: Ink::Primary }, None));
            let text = format!("{} %", b.percent);
            let readout_w = if one { inner - icon_px(k) - 6.0 } else { 110.0 * k };
            out.push(sized(
                PANEL_PAD + icon_px(k) + 6.0,
                y + (d - line(HEADING_PX, k)) / 2.0,
                readout_w,
                line(HEADING_PX, k),
                Draw::Text { text, size: HEADING_PX, weight: Weight::Bold, ink: Ink::Primary, align: Align::Left },
                None,
            ));
            if one {
                // a narrow panel: the readout alone on its line, the buttons right-aligned under it
                y += d + GRID_GAP;
            }
            buttons.extend(tail);
            place(&mut out, &buttons, true, 0.0, y);
        } else {
            place(&mut out, &buttons, false, PANEL_PAD, y);
            place(&mut out, &tail, true, 0.0, y);
        }
        y += d;
    }

    // ---- slider rows: output volume, then brightness (panel.js adds them in that order)
    let slider_row = |out: &mut Vec<Item>, y: &mut f32, sections: &mut i32, icon: &'static str, value: f32, target: Target, name: Option<&str>| {
        gap(y, sections);
        let h = slider_row_h(k);
        let x_track = PANEL_PAD + h + SLIDER_PAD;
        let w_track = PANEL_PAD + inner - x_track;
        // the volume row's icon is a button (mute); brightness has no such action, so its icon stays a plain glyph
        if target == Target::Volume {
            out.push(sized(PANEL_PAD, *y, h, h, Draw::Circle { icon, on: false }, Some(Target::Mute)));
        } else {
            out.push(sized(PANEL_PAD, *y, h, h, Draw::Glyph { icon, size: icon_px(k), ink: Ink::Primary }, None));
        }
        // The visible row comes first (it is the keyboard stop and the focus ring's shape); the touch area, drawn nothing, is later so it wins
        // hits: the whole row plus 6 px above and below, never narrower than the visible track.
        out.push(sized(x_track, *y, w_track, h, Draw::Slider { value, icon }, Some(target)));
        out.push(sized(x_track, *y - 6.0, w_track, h + 12.0, Draw::Hit, Some(target)));
        *y += h;
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            out.push(label(x_track, *y + 6.0, w_track, n, CAPTION_PX, Weight::Regular, Ink::Secondary, k));
            *y += 6.0 + line(CAPTION_PX, k);
        }
    };
    if let Some(v) = s.volume.as_ref().filter(|_| tiles.sound) {
        let shown = if v.muted { 0.0 } else { v.level.clamp(0.0, 1.0) };
        let sink = if s.sink.is_empty() { None } else { Some(s.sink.as_str()) };
        slider_row(&mut out, &mut y, &mut sections, volume_icon(v), shown, Target::Volume, sink);
    }
    if let Some(b) = s.brightness.filter(|_| tiles.display) {
        slider_row(&mut out, &mut y, &mut sections, "display-brightness-symbolic", (b as f32 / 100.0).clamp(0.0, 1.0), Target::Brightness, None);
    }

    // ---- the two-column grid
    let mut grid: Vec<Tile> = Vec::new();
    if tiles.connectivity {
        if let Some(n) = &s.net {
            let sub = if !n.wifi_enabled {
                "Off".to_string()
            } else if n.wifi_name.is_empty() {
                "Not Connected".to_string()
            } else {
                n.wifi_name.clone()
            };
            let icon = if !n.wifi_enabled {
                "network-wireless-disabled-symbolic"
            } else if n.wifi_name.is_empty() {
                "network-wireless-offline-symbolic"
            } else {
                "network-wireless-signal-excellent-symbolic"
            };
            grid.push(Tile { menu: Some(Target::Network), ..tile(icon, "Wi-Fi", Some(sub), n.wifi_enabled, Some(Target::Wifi)) });
        }
        if let Some(b) = &s.bluetooth {
            let sub = (b.powered && b.connected > 0).then(|| format!("{} Connected", b.connected));
            let icon = if b.powered { "bluetooth-active-symbolic" } else { "bluetooth-disabled-symbolic" };
            grid.push(tile(icon, "Bluetooth", sub, b.powered, Some(Target::Bluetooth)));
        }
        // GNOME lists a wired tile only while a wired device is connected; it opens the network editor like the Wi-Fi menu button.
        if let Some(n) = s.net.as_ref().filter(|n| !n.wired_name.is_empty()) {
            grid.push(tile("network-wired-symbolic", "Wired", Some(n.wired_name.clone()), true, Some(Target::Network)));
        }
    }
    if tiles.focus {
        let (night_sub, night_target) = match s.night_shift {
            None => (Some("Unavailable".to_string()), None),
            Some(_) => (None, Some(Target::NightShift)),
        };
        grid.push(tile("night-light-symbolic", "Night Light", night_sub, s.night_shift == Some(true), night_target));
        grid.push(tile("dark-mode-symbolic", "Dark Style", None, !s.light, Some(Target::DarkMode)));
        let (dnd_sub, dnd_target) = match s.dnd {
            None => (Some("Unavailable".to_string()), None),
            Some(_) => (None, Some(Target::Focus)),
        };
        grid.push(tile(if s.dnd == Some(true) { "notifications-disabled-symbolic" } else { "preferences-system-notifications-symbolic" }, "Do Not Disturb", dnd_sub, s.dnd == Some(true), dnd_target));
    }
    if let Some(u) = s.util.as_ref().filter(|_| tiles.toggles) {
        grid.push(tile("media-record-symbolic", if u.record { "Recording" } else { "Record" }, None, u.record, Some(Target::Record)));
        grid.push(tile("view-reveal-symbolic", "Inhibit Idle", None, u.idle, Some(Target::IdleInhibit)));
        grid.push(tile("input-gaming-symbolic", "Game Mode", None, u.game, Some(Target::GameMode)));
        grid.push(tile(
            if s.dnd == Some(true) { "notifications-disabled-symbolic" } else { "preferences-system-notifications-symbolic" },
            "DND",
            s.dnd.is_none().then(|| "Unavailable".to_string()),
            s.dnd == Some(true),
            s.dnd.map(|_| Target::Dnd),
        ));
    }
    if tiles.actions {
        grid.push(tile("caffeine-symbolic", "Stay Awake", None, s.awake, Some(Target::StayAwake)));
        // With decutil the third quick action is the notification history (Record moved into the toggles above).
        if s.util.is_some() {
            grid.push(tile("preferences-system-notifications-symbolic", "Notifications", None, false, Some(Target::Notifications)));
        } else {
            grid.push(tile("media-record-symbolic", "Record", None, false, Some(Target::Record)));
        }
    }
    if !grid.is_empty() {
        gap(&mut y, &mut sections);
        let per_row = if one { 1 } else { 2 };
        for (i, t) in grid.iter().enumerate() {
            let (col, row) = ((i % per_row) as f32, (i / per_row) as f32);
            let x = PANEL_PAD + col * (tw + GRID_GAP);
            let ty = y + row * (th + GRID_GAP);
            push_tile(&mut out, t, x, ty, tw, th, k);
        }
        let rows = grid.len().div_ceil(per_row) as f32;
        y += rows * th + (rows - 1.0) * GRID_GAP;
    }

    // ---- Now Playing, as the Shell's message card
    if let Some(m) = s.media.as_ref().filter(|_| tiles.media) {
        gap(&mut y, &mut sections);
        let art = 48.0 * k;
        let btn = button_d(k) - 9.0; // a flat 16 px icon button with 6 px padding (28 px)
        let controls_w = 3.0 * btn + 2.0 * 4.0;
        // a narrow panel puts the controls on a line of their own under the title, so the title keeps its room
        let h = if one { art + 24.0 + btn + 8.0 } else { art + 24.0 };
        out.push(sized(PANEL_PAD, y, inner, h, Draw::Tile, None));
        out.push(sized(PANEL_PAD + 12.0, y + 12.0, art, art, Draw::SmallTile { on: false }, None));
        let controls_x = PANEL_PAD + inner - 12.0 - controls_w;
        let text_x = PANEL_PAD + 12.0 + art + 12.0;
        let text_w = if one { PANEL_PAD + inner - 12.0 - text_x } else { controls_x - text_x - 8.0 };
        let title = if m.title.is_empty() { "Not Playing" } else { m.title.as_str() };
        let block = line(HEADING_PX, k) + if m.artist.is_empty() { 0.0 } else { line(CAPTION_PX, k) };
        let ty = y + (art + 24.0 - block) / 2.0;
        let controls_y = if one { y + art + 24.0 } else { y + (h - btn) / 2.0 };
        out.push(label(text_x, ty, text_w, title, HEADING_PX, Weight::Bold, Ink::Primary, k));
        if !m.artist.is_empty() {
            out.push(label(text_x, ty + line(HEADING_PX, k), text_w, &m.artist, CAPTION_PX, Weight::Regular, Ink::Secondary, k));
        }
        let icons = ["media-skip-backward-symbolic", if m.playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" }, "media-skip-forward-symbolic"];
        for (i, (icon, t)) in icons.into_iter().zip([Target::MediaPrev, Target::MediaToggle, Target::MediaNext]).enumerate() {
            let bx = controls_x + i as f32 * (btn + 4.0);
            out.push(sized(bx, controls_y, btn, btn, Draw::Glyph { icon, size: icon_px(k), ink: Ink::Primary }, Some(t)));
        }
        y += h;
    }

    if out.is_empty() && *tiles != Tiles::ALL {
        // every chosen group is unavailable on this machine: never show an empty box (the common layouts do the same)
        return layout_cols(s, &Tiles::ALL, k, columns);
    }
    (out, y + PANEL_PAD)
}

fn push_tile(out: &mut Vec<Item>, t: &Tile, x: f32, y: f32, w: f32, h: f32, k: f32) {
    let menu_w = if t.menu.is_some() { icon_px(k) + 2.0 * MENU_BUTTON_PAD * k } else { 0.0 };
    let main_w = w - menu_w;
    let caps = if t.menu.is_some() { Caps::Left } else { Caps::Pill };
    out.push(sized(x, y, main_w, h, Draw::Toggle { on: t.on, caps }, t.target));
    let ink = if t.on {
        Ink::White
    } else if t.target.is_none() {
        Ink::Secondary
    } else {
        Ink::Primary
    };
    let ix = x + TOGGLE_PAD_LEFT;
    out.push(sized(ix, y + (h - icon_px(k)) / 2.0, icon_px(k), icon_px(k), Draw::Glyph { icon: t.icon, size: icon_px(k), ink }, None));
    let tx = ix + icon_px(k) + TOGGLE_SPACING;
    let right_pad = if t.menu.is_some() { SPLIT_TOGGLE_PAD_RIGHT * k } else { TOGGLE_PAD_RIGHT };
    let text_w = (x + main_w - right_pad - tx).max(0.0);
    let (title_h, sub_h) = (line(HEADING_PX, k), line(CAPTION_PX, k));
    match &t.subtitle {
        Some(sub) => {
            let top = y + (h - title_h - sub_h) / 2.0;
            out.push(label(tx, top, text_w, t.title, HEADING_PX, Weight::Bold, ink, k));
            out.push(label(tx, top + title_h, text_w, sub, CAPTION_PX, Weight::Regular, ink, k));
        }
        None => out.push(label(tx, y + (h - title_h) / 2.0, text_w, t.title, HEADING_PX, Weight::Bold, ink, k)),
    }
    if let Some(menu) = t.menu {
        out.push(sized(x + main_w, y, menu_w, h, Draw::Toggle { on: t.on, caps: Caps::Right }, Some(menu)));
        out.push(sized(x + main_w + (menu_w - icon_px(k)) / 2.0, y + (h - icon_px(k)) / 2.0, icon_px(k), icon_px(k), Draw::Glyph { icon: "pan-end-symbolic", size: icon_px(k), ink }, None));
    }
}

fn volume_icon(v: &sys::Volume) -> &'static str {
    if v.muted || v.level <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if v.level >= 0.67 {
        "audio-volume-high-symbolic"
    } else if v.level >= 0.34 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-low-symbolic"
    }
}

fn battery_icon(b: &sys::Battery) -> &'static str {
    const LEVELS: [&str; 11] = [
        "battery-level-0-symbolic",
        "battery-level-10-symbolic",
        "battery-level-20-symbolic",
        "battery-level-30-symbolic",
        "battery-level-40-symbolic",
        "battery-level-50-symbolic",
        "battery-level-60-symbolic",
        "battery-level-70-symbolic",
        "battery-level-80-symbolic",
        "battery-level-90-symbolic",
        "battery-level-100-symbolic",
    ];
    const CHARGING: [&str; 11] = [
        "battery-level-0-charging-symbolic",
        "battery-level-10-charging-symbolic",
        "battery-level-20-charging-symbolic",
        "battery-level-30-charging-symbolic",
        "battery-level-40-charging-symbolic",
        "battery-level-50-charging-symbolic",
        "battery-level-60-charging-symbolic",
        "battery-level-70-charging-symbolic",
        "battery-level-80-charging-symbolic",
        "battery-level-90-charging-symbolic",
        "battery-level-100-charging-symbolic",
    ];
    let i = ((b.percent.min(100) + 5) / 10) as usize;
    if b.charging {
        CHARGING[i]
    } else {
        LEVELS[i]
    }
}

// ------------------------------------------------------------------ colours (`_drawing.scss` `button()`)

fn hsl(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < 1e-6 {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> Rgb {
    let hue = |p: f32, q: f32, mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let (r, g, b) = if s < 1e-6 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (hue(p, q, h + 1.0 / 3.0), hue(p, q, h), hue(p, q, h - 1.0 / 3.0))
    };
    Rgb((r * 255.0).round().clamp(0.0, 255.0) as u8, (g * 255.0).round().clamp(0.0, 255.0) as u8, (b * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Sass `lighten`/`darken` (HSL lightness).
fn lighten(c: Rgb, amount: f32) -> Rgb {
    let (h, s, l) = hsl(c);
    from_hsl(h, s, (l + amount).clamp(0.0, 1.0))
}
fn darken(c: Rgb, amount: f32) -> Rgb {
    lighten(c, -amount)
}
/// Sass `mix(a, b, weight)`: `weight` of `a`.
fn mix(a: Rgb, b: Rgb, weight: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 * weight + y as f32 * (1.0 - weight)).round().clamp(0.0, 255.0) as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// The colours of one painting pass, derived from the theme tokens the way the Shell derives them from `$bg_color` / `$fg_color`.
pub struct Palette {
    pub dark: bool,
    pub bg: Rgb,
    pub fg: Rgb,
    pub accent: Rgb,
    pub accent_fg: Rgb,
}

impl Palette {
    /// The Shell's `$bg_color` is the menu surface (`popup`), `$fg_color` the text colour, the accent fills default buttons.
    pub fn of(th: &crate::theme::model::Theme) -> Palette {
        Palette { dark: th.dark, bg: th.popup, fg: th.text, accent: th.accent, accent_fg: th.accent_text }
    }
    /// Light variants darken, dark variants lighten (`if($variant == 'light', st-darken(...), st-lighten(...))`).
    fn shift(&self, c: Rgb, amount: f32) -> Rgb {
        if self.dark {
            lighten(c, amount)
        } else {
            darken(c, amount)
        }
    }
    /// `$background_mix_factor`: 12 % light, 9 % dark.
    fn mix_factor(&self) -> f32 {
        if self.dark {
            0.09
        } else {
            0.12
        }
    }
    /// `button(normal)` over `base`.
    pub fn button(&self, base: Rgb) -> Rgb {
        mix(self.fg, base, self.mix_factor())
    }
    pub fn button_hover(&self, base: Rgb) -> Rgb {
        self.shift(self.button(base), 0.04)
    }
    pub fn button_insensitive(&self, base: Rgb) -> Rgb {
        if self.dark {
            darken(self.button(base), 0.03)
        } else {
            lighten(self.button(base), 0.03)
        }
    }
    /// `%default_button`: the accent itself, hover `±4 %`.
    pub fn default_button(&self, hover: bool) -> Rgb {
        if hover {
            self.shift(self.accent, 0.04)
        } else {
            self.accent
        }
    }
    /// The menu button's base: `lighten($bg_color, 8%)` / `darken($bg_color, 5%)`.
    pub fn menu_base(&self) -> Rgb {
        if self.dark {
            lighten(self.bg, 0.08)
        } else {
            darken(self.bg, 0.05)
        }
    }
    /// A checked menu button: `st-mix(accent, fg, 85 % / 92 %)`.
    pub fn menu_checked(&self, hover: bool) -> Rgb {
        let base = mix(self.accent, self.fg, if self.dark { 0.85 } else { 0.92 });
        if hover {
            self.shift(base, 0.04)
        } else {
            base
        }
    }
    /// `$insensitive_fg_color`: 60 % (light) / 50 % (dark) of the foreground over the menu background.
    pub fn insensitive_fg(&self) -> Rgb {
        mix(self.fg, self.bg, if self.dark { 0.5 } else { 0.6 })
    }
    /// The 1 px separator between the two halves of a split toggle.
    pub fn separator(&self, checked: bool) -> Rgb {
        if checked {
            mix(self.accent_fg, self.accent, if self.dark { 0.3 } else { 0.2 })
        } else {
            mix(self.fg, self.bg, 0.25)
        }
    }
    /// `.popup-menu-content { border: 1px solid $outer_borders_color }`.
    pub fn outer_border(&self) -> Rgb {
        if self.dark {
            lighten(self.bg, 0.05)
        } else {
            darken(self.bg, 0.07)
        }
    }
    /// `.message` card background.
    pub fn card(&self) -> Rgb {
        if self.dark {
            lighten(lighten(self.bg, 0.07), 0.05)
        } else {
            Rgb(255, 255, 255)
        }
    }
    /// The slider handle follows the foreground (`.slider { color }`: 91 % in dark).
    pub fn handle(&self) -> Rgb {
        if self.dark {
            darken(self.fg, 0.09)
        } else {
            self.fg
        }
    }
}

// ------------------------------------------------------------------ icons

/// Directories searched for the installed Adwaita symbolic icons: the user's and every `XDG_DATA_DIRS` entry.
pub fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))) {
        roots.push(home.join("icons"));
    }
    let dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    roots.extend(dirs.split(':').filter(|d| d.starts_with('/')).map(|d| PathBuf::from(d).join("icons")));
    roots
}

const ADWAITA_SYMBOLIC_DIRS: [&str; 6] = ["status", "devices", "actions", "legacy", "ui", "categories"];

/// `<root>/Adwaita/symbolic/<category>/<name>.svg`, the first that exists.
pub fn locate_icon(roots: &[PathBuf], name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\', '.']) {
        return None;
    }
    roots.iter().flat_map(|root| ADWAITA_SYMBOLIC_DIRS.iter().map(move |dir| root.join("Adwaita/symbolic").join(dir).join(format!("{name}.svg")))).find(|p| p.is_file())
}

/// The built-in glyph that stands in for a symbolic icon when the installed Adwaita theme does not have it.
pub fn fallback_glyph(name: &str) -> Option<&'static str> {
    Some(match name {
        "network-wireless-signal-excellent-symbolic" => "wifi-3",
        "network-wireless-disabled-symbolic" | "network-wireless-offline-symbolic" => "wifi-off",
        "network-wired-symbolic" => "ethernet",
        "bluetooth-active-symbolic" => "bluetooth-connected",
        "bluetooth-disabled-symbolic" => "bluetooth-off",
        "night-light-symbolic" => "night-shift",
        "dark-mode-symbolic" => "dark-mode",
        "notifications-disabled-symbolic" => "bell-off",
        "preferences-system-notifications-symbolic" => "bell",
        "audio-volume-muted-symbolic" => "speaker-muted",
        "audio-volume-low-symbolic" => "speaker-1",
        "audio-volume-medium-symbolic" => "speaker-2",
        "audio-volume-high-symbolic" => "speaker-3",
        "display-brightness-symbolic" => "sun-small",
        "screenshot-recorder-symbolic" => "screenshot",
        "emblem-system-symbolic" => "switches",
        "media-skip-backward-symbolic" => "backward",
        "media-skip-forward-symbolic" => "forward",
        "media-playback-start-symbolic" => "play",
        "media-playback-pause-symbolic" => "pause",
        "media-record-symbolic" => "record",
        "input-gaming-symbolic" => "gamepad",
        "view-reveal-symbolic" => "eye",
        "pan-end-symbolic" => "chevron-right",
        "caffeine-symbolic" => "cup",
        "system-lock-screen-symbolic" => "lock",
        "system-shutdown-symbolic" => "power",
        n if n.starts_with("battery-level-") => return None,
        _ => return None,
    })
}

type IconKey = (String, (u8, u8, u8), u32);

thread_local! {
    static ICONS: RefCell<HashMap<IconKey, Option<Pixmap>>> = RefCell::new(HashMap::new());
    static ROOTS: RefCell<Option<Vec<PathBuf>>> = const { RefCell::new(None) };
}

/// Recolours Adwaita's monochrome fills (`#2e3436`, `#2e3434`, `#222`) to `rgb`; the status colours of some icons stay as shipped.
pub fn tint_svg(svg: &str, rgb: (u8, u8, u8)) -> String {
    let hex = format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2);
    ["#2e3436", "#2e3434", "#222222", "#222"].iter().fold(svg.to_string(), |text, from| text.replace(&format!("\"{from}\""), &format!("\"{hex}\"")).replace(&format!(":{from}"), &format!(":{hex}")))
}

/// Rasterises the installed Adwaita symbolic icon `name` at `px`, recoloured; `None` when the theme is not installed or lacks it.
pub fn render_authentic(roots: &[PathBuf], name: &str, rgb: (u8, u8, u8), px: u32) -> Option<Pixmap> {
    let path = locate_icon(roots, name)?;
    let svg = std::fs::read_to_string(path).ok().filter(|t| t.len() < 64 * 1024)?;
    let tree = resvg::usvg::Tree::from_str(&tint_svg(&svg, rgb), &resvg::usvg::Options::default()).ok()?;
    let mut pm = Pixmap::new(px.max(1), px.max(1))?;
    let scale = px as f32 / tree.size().width().max(1.0);
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pm.as_mut());
    Some(pm)
}

/// Where an icon came from: for tests and the evidence report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Adwaita,
    Builtin,
    Missing,
}

fn blit_icon(pm: &mut Pixmap, symbols: &mut SymbolCache, name: &str, rgb: (u8, u8, u8), size: f32, cx: f32, cy: f32) -> Source {
    let px = size.round().max(1.0) as u32;
    let key = (name.to_string(), rgb, px);
    let hit = ICONS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() > 256 {
            cache.clear();
        }
        let roots = ROOTS.with(|r| r.borrow_mut().get_or_insert_with(icon_roots).clone());
        cache.entry(key).or_insert_with(|| render_authentic(&roots, name, rgb, px)).clone()
    });
    if let Some(icon) = hit {
        crate::gfx::blit_scaled(pm, &icon, cx - size / 2.0, cy - size / 2.0, size, size, 1.0);
        return Source::Adwaita;
    }
    if let Some(glyph) = fallback_glyph(name) {
        if let Some(sym) = symbols.get(glyph, rgb, (size * 2.0).ceil() as u32) {
            crate::gfx::blit_scaled(pm, sym, cx - size / 2.0, cy - size / 2.0, size, size, 1.0);
            return Source::Builtin;
        }
    }
    Source::Missing
}

// ------------------------------------------------------------------ painting

/// The keyboard focus ring: 2 px of accent standing 3 px off the control (a 1 px gap of the panel colour in between), so it is visible
/// on accent-filled toggles and distinct from the hover fill. `r` is the control's rectangle in the destination pixmap.
pub fn draw_focus(pm: &mut Pixmap, r: R) {
    let th = crate::theme::th();
    let radius = ((r.h + 6.0) / 2.0).min((r.w + 6.0) / 2.0);
    stroke_rounded(pm, r.x - 3.0, r.y - 3.0, r.w + 6.0, r.h + 6.0, radius, th.accent_c(), 2.0);
}

fn c(rgb: Rgb) -> tiny_skia::Color {
    rgb.color(1.0)
}

/// A rounded rectangle whose left and/or right end is square (the inner ends of a split toggle).
fn fill_capped(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, caps: Caps, color: tiny_skia::Color) {
    let r = (h / 2.0).min(w / 2.0);
    fill_rounded(pm, x, y, w, h, r, color);
    match caps {
        Caps::Pill => {}
        Caps::Left => crate::gfx::fill_rect(pm, x + w - r, y, r, h, color),
        Caps::Right => crate::gfx::fill_rect(pm, x, y, r, h, color),
    }
}

/// Paints the panel of `view` (style resolved to GNOME).
pub fn draw(pm: &mut Pixmap, view: &View, fonts: &mut Fonts, symbols: &mut SymbolCache) {
    draw_with(pm, view, fonts, symbols, &crate::theme::th(), scale());
}

/// [`draw`] with the theme and text scale given (the text scale must match the one the items were laid out at).
pub fn draw_with(pm: &mut Pixmap, view: &View, fonts: &mut Fonts, symbols: &mut SymbolCache, th: &crate::theme::model::Theme, k: f32) {
    let pal = Palette::of(th);
    let (ox, oy) = view.origin;
    let width = panel_width_at(k);
    fill_rounded(pm, ox, oy, width, view.height, PANEL_RADIUS, pal.bg.color(th.popup_alpha));
    stroke_rounded(pm, ox + 0.5, oy + 0.5, width - 1.0, view.height - 1.0, PANEL_RADIUS - 0.5, c(pal.outer_border()), 1.0);

    for it in view.items {
        let (x, y, w, h) = (ox + it.r.x, oy + it.r.y, it.r.w, it.r.h);
        let enabled = it.target.is_some();
        let hovered = enabled && it.target == view.hover;
        match &it.draw {
            Draw::Toggle { on, caps } => {
                let fill = match (caps, on) {
                    (Caps::Right, true) => pal.menu_checked(hovered),
                    (Caps::Right, false) => {
                        let base = pal.menu_base();
                        if hovered {
                            pal.button_hover(base)
                        } else {
                            pal.button(base)
                        }
                    }
                    (_, true) => pal.default_button(hovered),
                    (_, false) => {
                        if !enabled {
                            pal.button_insensitive(pal.bg)
                        } else if hovered {
                            pal.button_hover(pal.bg)
                        } else {
                            pal.button(pal.bg)
                        }
                    }
                };
                fill_capped(pm, x, y, w, h, *caps, c(fill));
                if *caps == Caps::Right {
                    crate::gfx::fill_rect(pm, x, y, 1.0, h, c(pal.separator(*on)));
                }
            }
            Draw::Circle { icon, on } => {
                let fill = if *on {
                    pal.default_button(hovered)
                } else if hovered {
                    pal.button_hover(pal.bg)
                } else {
                    pal.button(pal.bg)
                };
                fill_circle(pm, x + w / 2.0, y + h / 2.0, w.min(h) / 2.0, c(fill));
                let ink = if *on { pal.accent_fg } else { pal.fg };
                blit_icon(pm, symbols, icon, ink.t(), icon_px(k), x + w / 2.0, y + h / 2.0);
            }
            Draw::Tile => fill_rounded(pm, x, y, w, h, 16.0, c(pal.card())),
            Draw::SmallTile { .. } => fill_rounded(pm, x, y, w, h, 12.0, c(mix(pal.fg, pal.card(), 0.1))),
            Draw::Hit => {}
            Draw::Glyph { icon, size, ink } => {
                let rgb = match ink {
                    Ink::Primary => pal.fg,
                    Ink::Secondary => pal.insensitive_fg(),
                    Ink::White => pal.accent_fg,
                };
                if hovered {
                    fill_circle(pm, x + w / 2.0, y + h / 2.0, w.min(h) / 2.0, pal.fg.color(0.1));
                }
                blit_icon(pm, symbols, icon, rgb.t(), *size, x + w / 2.0, y + h / 2.0);
            }
            Draw::Text { text, size, weight, ink, align } => {
                let rgb = match ink {
                    Ink::Primary => pal.fg,
                    Ink::Secondary => pal.insensitive_fg(),
                    Ink::White => pal.accent_fg,
                };
                let shown = fit(text, w, *size, *weight, fonts);
                let tw = fonts.measure(&shown, *size, *weight);
                let tx = match align {
                    Align::Left => x,
                    Align::Center => x + (w - tw) / 2.0,
                    Align::Right => x + w - tw,
                };
                fonts.draw(pm, &shown, tx, y, *size, *weight, color(rgb.0, rgb.1, rgb.2, 1.0));
            }
            // UKUI's icon-less track is never part of the GNOME layout
            Draw::Track { .. } => {}
            Draw::Slider { value, .. } => {
                // `-barlevel-height: 4px`, trough = 10 % foreground, fill = accent, handle radius 8 px (scalable)
                let handle = icon_px(k) / 2.0;
                let cy = y + h / 2.0;
                let v = value.clamp(0.0, 1.0);
                fill_rounded(pm, x, cy - SLIDER_TROUGH / 2.0, w, SLIDER_TROUGH, SLIDER_TROUGH / 2.0, pal.fg.color(0.10));
                let hx = x + handle + v * (w - 2.0 * handle).max(0.0);
                fill_rounded(pm, x, cy - SLIDER_TROUGH / 2.0, (hx - x).max(0.0), SLIDER_TROUGH, SLIDER_TROUGH / 2.0, c(pal.accent));
                fill_circle(pm, hx, cy, handle, c(pal.handle()));
            }
        }
    }
}

#[cfg(test)]
mod tests;
