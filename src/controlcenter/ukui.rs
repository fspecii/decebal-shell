//! The UKUI sidebar's shortcut panel, from the original QML.
//!
//! Source (gitee.com/openkylin/ukui-sidebar @ 59abed0, GPL-3.0+; measured, no code copied): `qml/Shortcuts.qml` (the panel is 396 wide, `baseActive`, radius
//! `kRadiusWindow` 12), `qml/org/ukui/sidebar/ui/ShortcuPanel.qml` (14 px top padding, 16 px spacing, a 32 px header row with the bold "Shortcuts" title and the
//! 32 px edit / settings buttons at 356 wide, the icon shortcut grid, then the progress-bar shortcuts at 356 x 56 spaced 16, 24 px at the bottom),
//! `EditButtonView.qml` / `EditShortcutFlow.qml` (4 columns of 80 x 72 cells), `IconButton.qml` (radius `kRadiusMax` 8, 1 px border, a 24 px icon 8 px from the top,
//! the name below, highlight = brand fill) and `ProgressBar.qml` (radius 8 container, 12 px side margins, an icon button, 8 px spacing).
//! The default shortcut list (`src/model/shortcut-config.cpp`) is Wi-Fi, Bluetooth, status, auto-rotation, eye care, dark mode, clock, notebook, multi-screen,
//! screenshot ...; the ones Decebal has a control for are used here in that order.
//!
//! Not modelled: the notification column (384 wide, a separate pane here), edit mode and drag re-ordering, tablet mode, the lottie icon animations.
use super::*;

pub const PANEL_W: f32 = 396.0;
const SIDE: f32 = 20.0;
const GRID_COLUMNS: usize = 4;
const CELL: (f32, f32) = (80.0, 72.0);
const COLUMN_GAP: f32 = 12.0;
const ROW_GAP: f32 = 16.0;
const BAR: (f32, f32) = (356.0, 56.0);

pub(super) fn layout(s: &Snapshot, tiles: &Tiles) -> (Vec<Item>, f32) {
    let mut out = Vec::new();
    let mut y = 14.0;
    // header: the bold title (20 px line), the settings button on the right (32 px)
    out.push(item(SIDE, y + 6.0, 200.0, 20.0, Draw::Text { text: "Shortcuts".into(), size: 14.0, weight: Weight::Bold, ink: Ink::Primary, align: Align::Left }, None));
    out.push(item(PANEL_W - SIDE - 32.0, y, 32.0, 32.0, Draw::SmallTile { on: false }, Some(Target::Settings)));
    out.push(item(PANEL_W - SIDE - 32.0 + 4.0, y + 4.0, 24.0, 24.0, Draw::Glyph { icon: "switches", size: 20.0, ink: Ink::Primary }, None));
    y += 32.0 + ROW_GAP;

    // the icon shortcuts, in the original's default order
    let mut shortcuts: Vec<(&'static str, &str, bool, Option<Target>)> = Vec::new();
    if tiles.connectivity {
        shortcuts.push(("wifi-3", "WLAN", s.net.as_ref().is_some_and(|n| n.wifi_enabled), s.net.as_ref().map(|_| Target::Network)));
        shortcuts.push(("bluetooth", "Bluetooth", s.bluetooth.as_ref().is_some_and(|b| b.powered), s.bluetooth.as_ref().map(|_| Target::Bluetooth)));
    }
    if tiles.focus {
        shortcuts.push(("do_not_disturb_on", "Quiet", s.dnd == Some(true), s.dnd.map(|_| Target::Focus)));
        shortcuts.push(("night-shift", "Eye Care", s.night_shift == Some(true), s.night_shift.map(|_| Target::NightShift)));
        shortcuts.push(("contrast", "Dark Mode", !s.light, Some(Target::DarkMode)));
    }
    if tiles.actions {
        shortcuts.push(("screenshot", "Screenshot", false, Some(Target::Screenshot)));
        shortcuts.push(("cup", "Awake", s.awake, Some(Target::StayAwake)));
        shortcuts.push(("bell", "Notices", false, Some(Target::Notifications)));
    }
    let mut rows = 0;
    for (i, (icon, name, on, target)) in shortcuts.iter().enumerate() {
        let (col, row) = (i % GRID_COLUMNS, i / GRID_COLUMNS);
        let x = SIDE + col as f32 * (CELL.0 + COLUMN_GAP);
        let top = y + row as f32 * (CELL.1 + ROW_GAP);
        rows = row + 1;
        out.push(item(x, top, CELL.0, CELL.1, Draw::SmallTile { on: *on }, *target));
        // IconButton.qml: the 24 px icon 8 px from the top, then the name 4 px below it
        out.push(item(x + (CELL.0 - 24.0) / 2.0, top + 8.0 + 4.0, 24.0, 24.0, Draw::Glyph { icon, size: 22.0, ink: if *on { Ink::White } else { Ink::Primary } }, None));
        out.push(item(x + 4.0, top + 8.0 + 4.0 + 24.0 + 4.0, CELL.0 - 8.0, 15.0, Draw::Text { text: (*name).to_string(), size: 12.0, weight: Weight::Regular, ink: if *on { Ink::White } else { Ink::Primary }, align: Align::Center }, None));
    }
    if rows > 0 {
        y += rows as f32 * CELL.1 + (rows - 1) as f32 * ROW_GAP + ROW_GAP;
    }

    // the progress-bar shortcuts: volume and brightness, 356 x 56
    for (value, icon, target, on) in [
        (s.volume.as_ref().map(|v| if v.muted { 0.0 } else { v.level }), "speaker-2", Target::Volume, tiles.sound),
        (s.brightness.map(|v| v as f32 / 100.0), "sun-small", Target::Brightness, tiles.display),
    ] {
        if !on {
            continue;
        }
        out.push(item(SIDE, y, BAR.0, BAR.1, Draw::Tile, None));
        match value {
            Some(value) => out.push(item(SIDE + 12.0 + 32.0 + 8.0, y + (BAR.1 - 24.0) / 2.0, BAR.0 - 12.0 - 32.0 - 8.0 - 12.0, 24.0, Draw::Slider { value, icon }, Some(target))),
            None => out.push(item(SIDE + 12.0, y + (BAR.1 - 16.0) / 2.0, BAR.0 - 24.0, 16.0, Draw::Text { text: "Unavailable".into(), size: 12.0, weight: Weight::Regular, ink: Ink::Secondary, align: Align::Left }, None)),
        }
        y += BAR.1 + ROW_GAP;
    }
    // ShortcuPanel: `height: panelLayout.height + 38` = 14 top + content + 24 bottom
    (out, y - ROW_GAP + 24.0)
}
