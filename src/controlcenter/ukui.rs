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

/// One icon shortcut (`IconButton.qml`).
struct Shortcut {
    icon: &'static str,
    name: &'static str,
    on: bool,
    target: Target,
}

/// The panel for `tiles`; a selection that leaves nothing to show (no group has a control on this machine) falls back to every group, as the other styles do.
pub(super) fn layout(s: &Snapshot, tiles: &Tiles) -> (Vec<Item>, f32) {
    let (out, height, body) = build(s, tiles);
    if body || *tiles == Tiles::ALL {
        return (out, height);
    }
    let (out, height, _) = build(s, &Tiles::ALL);
    (out, height)
}

/// The icon shortcuts of the enabled groups. Source order (`shortcut-config.cpp` `SHORTCUT_ADDED_LIST`): Wi-Fi, Bluetooth, status (quiet), eye care, dark mode ... screenshot.
/// Not in the original's default list, available here as disclosed extensions: the wired Network editor, Stay Awake, Notifications, and the `decutil` toggles.
fn shortcuts(s: &Snapshot, tiles: &Tiles) -> Vec<Shortcut> {
    let mut out = Vec::new();
    if tiles.connectivity {
        if let Some(n) = s.net.as_ref() {
            out.push(Shortcut { icon: "wifi-3", name: "WLAN", on: n.wifi_enabled, target: Target::Wifi });
        }
        if let Some(b) = s.bluetooth.as_ref() {
            out.push(Shortcut { icon: "bluetooth", name: "Bluetooth", on: b.powered, target: Target::Bluetooth });
        }
    }
    if tiles.focus {
        if let Some(d) = s.dnd {
            out.push(Shortcut { icon: "do_not_disturb_on", name: "Quiet", on: d, target: Target::Focus });
        }
        if let Some(n) = s.night_shift {
            out.push(Shortcut { icon: "night-shift", name: "Eye Care", on: n, target: Target::NightShift });
        }
        out.push(Shortcut { icon: "contrast", name: "Dark Mode", on: !s.light, target: Target::DarkMode });
    }
    if tiles.actions {
        out.push(Shortcut { icon: "screenshot", name: "Screenshot", on: false, target: Target::Screenshot });
        out.push(Shortcut { icon: "cup", name: "Awake", on: s.awake, target: Target::StayAwake });
        out.push(Shortcut { icon: "bell", name: "Notices", on: false, target: Target::Notifications });
    }
    if tiles.connectivity && s.net.is_some() {
        out.push(Shortcut { icon: "ethernet", name: "Network", on: false, target: Target::Network });
    }
    if tiles.toggles {
        if let Some(u) = s.util.as_ref() {
            out.push(Shortcut { icon: "record", name: if u.record { "Recording" } else { "Record" }, on: u.record, target: Target::Record });
            out.push(Shortcut { icon: "eye", name: "Idle", on: u.idle, target: Target::IdleInhibit });
            out.push(Shortcut { icon: "gamepad", name: "Game", on: u.game, target: Target::GameMode });
            if let Some(d) = s.dnd {
                out.push(Shortcut { icon: if d { "bell-off" } else { "bell" }, name: "DND", on: d, target: Target::Dnd });
            }
        }
    }
    out
}

fn build(s: &Snapshot, tiles: &Tiles) -> (Vec<Item>, f32, bool) {
    let mut out = Vec::new();
    let mut y = 14.0;
    // header: the bold title (20 px line); the settings button (32 px) on the right. The original's edit button has no edit mode behind it here, so it is not drawn.
    out.push(item(SIDE, y + 6.0, 200.0, 20.0, Draw::Text { text: "Shortcuts".into(), size: 14.0, weight: Weight::Bold, ink: Ink::Primary, align: Align::Left }, None));
    if tiles.actions {
        out.push(item(PANEL_W - SIDE - 32.0, y, 32.0, 32.0, Draw::SmallTile { on: false }, Some(Target::Settings)));
        out.push(item(PANEL_W - SIDE - 32.0 + 4.0, y + 4.0, 24.0, 24.0, Draw::Glyph { icon: "switches", size: 20.0, ink: Ink::Primary }, None));
    }
    y += 32.0 + ROW_GAP;
    let mut body = false;

    let cells = shortcuts(s, tiles);
    let mut rows = 0;
    for (i, c) in cells.iter().enumerate() {
        let (col, row) = (i % GRID_COLUMNS, i / GRID_COLUMNS);
        let x = SIDE + col as f32 * (CELL.0 + COLUMN_GAP);
        let top = y + row as f32 * (CELL.1 + ROW_GAP);
        rows = row + 1;
        out.push(item(x, top, CELL.0, CELL.1, Draw::SmallTile { on: c.on }, Some(c.target)));
        // IconButton.qml: the 24 px icon 8 px from the top, then the name 4 px below it
        out.push(item(x + (CELL.0 - 24.0) / 2.0, top + 8.0 + 4.0, 24.0, 24.0, Draw::Glyph { icon: c.icon, size: 22.0, ink: if c.on { Ink::White } else { Ink::Primary } }, None));
        out.push(item(x + 4.0, top + 8.0 + 4.0 + 24.0 + 4.0, CELL.0 - 8.0, 15.0, Draw::Text { text: c.name.to_string(), size: 12.0, weight: Weight::Regular, ink: if c.on { Ink::White } else { Ink::Primary }, align: Align::Center }, None));
    }
    if rows > 0 {
        body = true;
        y += rows as f32 * CELL.1 + (rows - 1) as f32 * ROW_GAP + ROW_GAP;
    }

    // the progress-bar shortcuts (`ProgressBar.qml`): a 356 x 56 row, 12 px margins, 8 px spacing: a 24 px icon button (the volume one mutes), the 16 px high slider
    // filling the rest. The optional 24 px sub-page arrow of the original needs a plugin page and is not drawn.
    let volume = s.volume.as_ref();
    for (value, icon, mute, target, shown) in [
        (volume.map(|v| if v.muted { 0.0 } else { v.level }), volume.map_or("speaker-2", super::speaker_icon), true, Target::Volume, tiles.sound),
        (s.brightness.map(|v| v as f32 / 100.0), "sun-small", false, Target::Brightness, tiles.display),
    ] {
        if !shown {
            continue;
        }
        body = true;
        out.push(item(SIDE, y, BAR.0, BAR.1, Draw::Tile, None));
        match value {
            Some(value) => {
                let ix = SIDE + 12.0;
                out.push(item(ix, y + (BAR.1 - 24.0) / 2.0, 24.0, 24.0, Draw::Glyph { icon, size: 20.0, ink: Ink::Primary }, mute.then_some(Target::Mute)));
                let tx = ix + 24.0 + 8.0;
                out.push(item(tx, y + (BAR.1 - 16.0) / 2.0, SIDE + BAR.0 - 12.0 - tx, 16.0, Draw::Track { value }, Some(target)));
            }
            None => out.push(item(SIDE + 12.0, y + (BAR.1 - 16.0) / 2.0, BAR.0 - 24.0, 16.0, Draw::Text { text: "Unavailable".into(), size: 12.0, weight: Weight::Regular, ink: Ink::Secondary, align: Align::Left }, None)),
        }
        y += BAR.1 + ROW_GAP;
    }

    // now playing (the Media group): not part of the original's shortcut panel, a disclosed extension row of the same 356 x 56 size
    if let Some(m) = s.media.as_ref().filter(|_| tiles.media) {
        body = true;
        out.push(item(SIDE, y, BAR.0, BAR.1, Draw::Tile, None));
        let title = if m.title.is_empty() { "Not Playing" } else { m.title.as_str() };
        let controls = SIDE + BAR.0 - 12.0 - 3.0 * 32.0;
        out.push(item(SIDE + 12.0, y + 9.0, controls - SIDE - 12.0 - 8.0, 20.0, Draw::Text { text: title.to_string(), size: 14.0, weight: Weight::Bold, ink: Ink::Primary, align: Align::Left }, None));
        out.push(item(SIDE + 12.0, y + 31.0, controls - SIDE - 12.0 - 8.0, 16.0, Draw::Text { text: m.artist.clone(), size: 12.0, weight: Weight::Regular, ink: Ink::Secondary, align: Align::Left }, None));
        for (i, (icon, t)) in [("backward", Target::MediaPrev), (if m.playing { "pause" } else { "play" }, Target::MediaToggle), ("forward", Target::MediaNext)].into_iter().enumerate() {
            out.push(item(controls + i as f32 * 32.0, y + (BAR.1 - 32.0) / 2.0, 32.0, 32.0, Draw::Glyph { icon, size: 18.0, ink: Ink::Primary }, Some(t)));
        }
        y += BAR.1 + ROW_GAP;
    }
    // ShortcuPanel: `height: panelLayout.height + 38` = 14 top + content + 24 bottom
    (out, y - ROW_GAP + 24.0, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            user: None,
            net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: "Wired".into() }),
            bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
            volume: Some(sys::Volume { level: 0.5, muted: false }),
            sink: "Headset".into(),
            brightness: Some(40),
            night_shift: Some(false),
            dnd: Some(false),
            media: None,
            awake: false,
            light: false,
            battery: None,
            util: None,
            lock: false,
            power: false,
        }
    }

    fn shortcut_cells(items: &[Item]) -> Vec<&Item> {
        items.iter().filter(|i| matches!(i.draw, Draw::SmallTile { .. }) && i.r.w == CELL.0).collect()
    }

    #[test]
    fn the_shortcuts_are_a_four_column_grid_of_80_by_72_cells() {
        let (items, _) = layout(&snapshot(), &Tiles::ALL);
        let cells = shortcut_cells(&items);
        // Wi-Fi, Bluetooth, quiet, eye care, dark mode, screenshot, awake, notices, then the wired Network extension
        let targets: Vec<_> = cells.iter().map(|c| c.target.unwrap()).collect();
        assert_eq!(targets, [Target::Wifi, Target::Bluetooth, Target::Focus, Target::NightShift, Target::DarkMode, Target::Screenshot, Target::StayAwake, Target::Notifications, Target::Network]);
        for (i, c) in cells.iter().enumerate() {
            assert_eq!((c.r.w, c.r.h), (80.0, 72.0));
            assert_eq!(c.r.x, SIDE + (i % 4) as f32 * 92.0, "column {i}");
        }
        assert_eq!(cells[4].r.y - cells[0].r.y, 72.0 + 16.0, "rows are 16 apart");
    }

    #[test]
    fn the_progress_rows_reserve_and_draw_the_icon_button_then_the_slider() {
        let (items, h) = layout(&snapshot(), &Tiles::ALL);
        let bars: Vec<_> = items.iter().filter(|i| matches!(i.draw, Draw::Tile)).collect();
        assert_eq!(bars.len(), 2);
        assert!(bars.iter().all(|b| b.r.w == 356.0 && b.r.h == 56.0));
        assert_eq!(bars[1].r.y - bars[0].r.y, 56.0 + 16.0);
        assert_eq!(PANEL_W, SIDE * 2.0 + BAR.0, "20 px side padding around the 356 px rows");
        assert_eq!(h, bars[1].r.y + 56.0 + 24.0, "24 px below the last row");
        for i in &items {
            assert!(i.r.x >= 0.0 && i.r.x + i.r.w <= PANEL_W + 0.01, "{:?} leaves the panel", i.r);
        }
        for (bar, target, mute) in [(bars[0], Target::Volume, true), (bars[1], Target::Brightness, false)] {
            let inside = |i: &&Item| i.r.x >= bar.r.x && (i.r.x + i.r.w) <= (bar.r.x + bar.r.w) && i.r.y >= bar.r.y && (i.r.y + i.r.h) <= (bar.r.y + bar.r.h);
            let icon = items.iter().filter(inside).find(|i| matches!(i.draw, Draw::Glyph { .. })).expect("the 24 px icon button is drawn");
            let track = items.iter().filter(inside).find(|i| matches!(i.draw, Draw::Track { .. })).expect("the track");
            assert_eq!((icon.r.w, icon.r.h, icon.r.x - bar.r.x), (24.0, 24.0, 12.0));
            assert_eq!(icon.target, mute.then_some(Target::Mute), "only the volume button is clickable (it mutes)");
            assert_eq!(track.r.x - (icon.r.x + icon.r.w), 8.0, "8 px spacing");
            assert_eq!((bar.r.x + bar.r.w) - (track.r.x + track.r.w), 12.0, "12 px margin");
            assert_eq!((track.r.h, track.target), (16.0, Some(target)));
            assert!(!items.iter().any(|i| matches!(i.draw, Draw::Slider { .. })), "the icon is never drawn inside the track");
        }
    }

    #[test]
    fn the_media_row_and_toggles_follow_their_groups_and_need_their_hardware() {
        let mut s = snapshot();
        s.media = Some(sys::Media { playing: false, title: "T".into(), artist: "A".into() });
        s.util = Some(sys::Utils::default());
        let (items, _) = layout(&s, &Tiles::ALL);
        for t in [Target::MediaPrev, Target::MediaToggle, Target::MediaNext, Target::Record, Target::IdleInhibit, Target::GameMode, Target::Dnd] {
            assert!(items.iter().any(|i| i.target == Some(t)), "{t:?}");
        }
        let none = Tiles { connectivity: false, focus: false, display: false, sound: false, media: false, toggles: false, actions: false };
        let media_only = Tiles { media: true, ..none };
        let (items, _) = layout(&s, &media_only);
        assert!(items.iter().any(|i| i.target == Some(Target::MediaToggle)) && !items.iter().any(|i| i.target == Some(Target::Settings)));
        let toggles_only = Tiles { toggles: true, ..none };
        let (items, _) = layout(&s, &toggles_only);
        assert!(items.iter().any(|i| i.target == Some(Target::GameMode)) && !items.iter().any(|i| i.target == Some(Target::Settings)));
        s.media = None;
        s.util = None;
        let (items, _) = layout(&s, &media_only);
        assert!(items.iter().any(|i| i.target == Some(Target::Wifi)), "nothing to show for the selection: every group, as the other styles do");
    }

    #[test]
    fn the_settings_button_is_in_the_header_and_every_shortcut_has_a_target() {
        let (items, _) = layout(&snapshot(), &Tiles::ALL);
        let settings = items.iter().find(|i| i.target == Some(Target::Settings)).unwrap();
        assert_eq!((settings.r.w, settings.r.h, settings.r.y), (32.0, 32.0, 14.0));
        assert!(shortcut_cells(&items).iter().all(|c| c.target.is_some()));
        let no_actions = Tiles { actions: false, ..Tiles::ALL };
        assert!(!layout(&snapshot(), &no_actions).0.iter().any(|i| i.target == Some(Target::Settings)), "no Settings button without the actions group");
    }
}
