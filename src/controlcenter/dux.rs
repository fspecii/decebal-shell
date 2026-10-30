//! Dux expanded tile grid: CcMetrics (48px target + 8px padding per side),
//! CcTileContent (leading tonal icon box, primary/supporting labels), CcSliderRow.
//! Snapshot-only: no probes, files, images or network requests occur here.
use super::*;

pub(super) fn layout(s: &Snapshot, tiles: &Tiles) -> (Vec<Item>, f32) {
    let mut out = Vec::new();
    let scale = crate::theme::live::font_scale().max(1.0);
    let pad = 16.0;
    let gap = 8.0;
    let inner = panel_width(Style::Dux) - pad * 2.0;
    let half = (inner - gap) / 2.0;
    let row = 64.0 + (scale - 1.0) * 36.0;
    let header = 72.0 + (scale - 1.0) * 24.0;
    out.push(item(pad, pad, inner, header, Draw::Tile, None));
    let (name, uptime) = s.user.as_ref().map(|(name, uptime)| (name.as_str(), uptime.as_str())).unwrap_or(("Control Center", "Session information unavailable"));
    out.push(item(pad + 12.0, pad + (header - 48.0) / 2.0, 48.0, 48.0, Draw::Circle { icon: "person", on: true }, None));
    out.push(text(pad + 72.0, pad + 10.0, inner - 136.0, name, 16.0, Weight::Medium, Ink::Primary));
    out.push(text(pad + 72.0, pad + 12.0 + 21.0 * scale, inner - 136.0, uptime, 12.0, Weight::Regular, Ink::Secondary));
    out.push(item(pad + inner - 48.0, pad + (header - 40.0) / 2.0, 40.0, 40.0, Draw::Glyph { icon: "switches", size: 22.0, ink: Ink::Primary }, Some(Target::Settings)));
    let mut y = pad + header + gap;
    let mut column = 0;
    let mut tile = |icon: &'static str, title: &str, status: &str, on: bool, target: Option<Target>| {
        let x = pad + column as f32 * (half + gap);
        out.push(item(x, y, half, row, Draw::Tile, target));
        out.push(item(x + 8.0, y + (row - 48.0) / 2.0, 48.0, 48.0, Draw::Circle { icon, on }, target));
        out.push(text(x + 68.0, y + 9.0, half - 78.0, title, 14.0, Weight::Medium, Ink::Primary));
        out.push(text(x + 68.0, y + 11.0 + 19.0 * scale, half - 78.0, status, 12.0, Weight::Regular, Ink::Secondary));
        column += 1;
        if column == 2 {
            column = 0;
            y += row + gap;
        }
    };
    if tiles.connectivity {
        let network = s.net.as_ref();
        let status = network
            .map(|n| {
                if !n.wired_name.is_empty() {
                    n.wired_name.as_str()
                } else if !n.wifi_name.is_empty() {
                    n.wifi_name.as_str()
                } else {
                    "Disconnected"
                }
            })
            .unwrap_or("Unavailable");
        tile("wifi-3", "Network", status, network.is_some_and(|n| n.wifi_enabled), network.map(|_| Target::Network));
        let bt = s.bluetooth.as_ref();
        let status = match bt {
            Some(b) if !b.powered => "Disabled",
            Some(b) if b.connected > 0 => "Connected",
            Some(_) => "No devices",
            None => "Unavailable",
        };
        tile("bluetooth", "Bluetooth", status, bt.is_some_and(|b| b.powered), bt.map(|_| Target::Bluetooth));
    }
    if tiles.focus {
        tile(
            "moon",
            "Do Not Disturb",
            match s.dnd {
                Some(true) => "Enabled",
                Some(false) => "Disabled",
                None => "Unavailable",
            },
            s.dnd == Some(true),
            s.dnd.map(|_| Target::Focus),
        );
        tile("dark-mode", "Appearance", if s.light { "Light" } else { "Dark" }, !s.light, Some(Target::DarkMode));
        tile(
            "night-shift",
            "Night Mode",
            match s.night_shift {
                Some(true) => "Enabled",
                Some(false) => "Blue light filter",
                None => "Unavailable",
            },
            s.night_shift == Some(true),
            s.night_shift.map(|_| Target::NightShift),
        );
        tile("cup", "Keep Awake", if s.awake { "Enabled" } else { "Prevent screen timeout" }, s.awake, Some(Target::StayAwake));
    }
    if tiles.toggles {
        if let Some(utils) = &s.util {
            tile("record", "Recording", if utils.record { "Active" } else { "Inactive" }, utils.record, Some(Target::Record));
            tile("gamepad", "Game Mode", if utils.game { "Enabled" } else { "Disabled" }, utils.game, Some(Target::GameMode));
            tile("eye", "Idle Inhibitor", if utils.idle { "Enabled" } else { "Disabled" }, utils.idle, Some(Target::IdleInhibit));
        }
    }
    if column > 0 {
        y += row + gap;
    }
    if tiles.sound || tiles.display {
        let slider_h = 48.0 + (scale - 1.0) * 20.0;
        for (label, value, icon, target) in [
            ("Volume", s.volume.as_ref().map(|v| if v.muted { 0.0 } else { v.level }), "speaker-2", Target::Volume),
            ("Brightness", s.brightness.map(|v| v as f32 / 100.0), "sun-small", Target::Brightness),
        ] {
            if (target == Target::Volume && !tiles.sound) || (target == Target::Brightness && !tiles.display) {
                continue;
            }
            out.push(item(pad, y, inner, slider_h, Draw::Tile, None));
            out.push(text(pad + 12.0, y + 8.0, 116.0 * scale.min(1.5), label, 12.0, Weight::Medium, Ink::Primary));
            let x = pad + 132.0 * scale.min(1.5);
            if let Some(value) = value {
                out.push(item(x, y + (slider_h - 14.0) / 2.0, inner - (x - pad) - 16.0, 14.0, Draw::Slider { value, icon }, Some(target)));
            } else {
                out.push(text(x, y + 8.0, inner - (x - pad) - 12.0, "Unavailable", 12.0, Weight::Regular, Ink::Secondary));
            }
            y += slider_h + gap;
        }
    }
    if tiles.media {
        let height = 64.0 + (scale - 1.0) * 36.0;
        out.push(item(pad, y, inner, height, Draw::Tile, None));
        let media = s.media.as_ref();
        out.push(text(pad + 12.0, y + 10.0, inner - 140.0, media.map(|m| m.title.as_str()).filter(|s| !s.is_empty()).unwrap_or("No media playing"), 14.0, Weight::Medium, Ink::Primary));
        out.push(text(pad + 12.0, y + 12.0 + 19.0 * scale, inner - 140.0, media.map(|m| m.artist.as_str()).unwrap_or("Media unavailable"), 12.0, Weight::Regular, Ink::Secondary));
        if let Some(media) = media {
            for (i, (icon, target)) in [("backward", Target::MediaPrev), (if media.playing { "pause" } else { "play" }, Target::MediaToggle), ("forward", Target::MediaNext)].into_iter().enumerate() {
                out.push(item(pad + inner - 120.0 + i as f32 * 36.0, y + (height - 32.0) / 2.0, 32.0, 32.0, Draw::Glyph { icon, size: 18.0, ink: Ink::Primary }, Some(target)));
            }
        }
        y += height + gap;
    }
    if tiles.actions {
        let width = (inner - gap * 3.0) / 4.0;
        for (i, (icon, label, target)) in
            [("screenshot", "Capture", Target::Screenshot), ("bell", "Alerts", Target::Notifications), ("switches", "Settings", Target::Settings), ("cup", "Awake", Target::StayAwake)]
                .into_iter()
                .enumerate()
        {
            let x = pad + i as f32 * (width + gap);
            out.push(item(x, y, width, row, Draw::SmallTile { on: target == Target::StayAwake && s.awake }, Some(target)));
            out.push(item(x + width / 2.0 - 10.0, y + 8.0, 20.0, 20.0, Draw::Glyph { icon, size: 20.0, ink: Ink::Primary }, None));
            out.push(caption(x + 4.0, y + 32.0, width - 8.0, label, Ink::Primary));
        }
        y += row + gap;
    }
    (out, y + pad - gap)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_tiles_are_honest_and_inert_and_targets_share_the_paint_grid() {
        let (items, height) = layout(&Snapshot::default(), &Tiles::ALL);
        assert!(items.iter().any(|i| matches!(&i.draw, Draw::Text {text, ..} if text == "Unavailable")));
        assert!(!items.iter().any(|i| matches!(i.target, Some(Target::Bluetooth | Target::Network | Target::Volume | Target::Brightness))));
        for it in &items {
            assert!(it.r.x >= 0.0 && it.r.y >= 0.0 && it.r.x + it.r.w <= panel_width(Style::Dux));
            assert!(it.r.y + it.r.h <= height);
            if let Some(target) = it.target {
                assert_eq!(hit(&items, it.r.x + it.r.w / 2.0, it.r.y + it.r.h / 2.0).map(|h| h.0), Some(target));
            }
        }
    }
}
