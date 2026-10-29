//! Sidebar sections and the search index (panes + individual settings).
//!
//! The panes themselves are listed in `panes::PANES` (ids below must exist there). Settings of the
//! system panes are indexed statically here; settings of the older (rdwm) panes are harvested from
//! their rows by the caller and passed in as `extra`.

use crate::settings::panes::PANES;

pub struct Section {
    pub title: &'static str,
    pub panes: &'static [&'static str],
}

pub const SECTIONS: &[Section] = &[
    Section { title: "Network & Connectivity", panes: &["network", "bluetooth"] },
    Section { title: "Appearance", panes: &["appearance", "wallpaper", "widgets", "config"] },
    Section { title: "Displays", panes: &["displays"] },
    Section { title: "Sound", panes: &["sound"] },
    Section { title: "Notifications", panes: &["notifications"] },
    Section { title: "Keyboard & Input", panes: &["inputdev", "keyboard"] },
    Section { title: "Windows", panes: &["tiling", "windows", "rules"] },
    Section { title: "Apps", panes: &["defaults", "autostart"] },
    Section { title: "System", panes: &["general", "power", "datetime", "users", "privacy", "accessibility", "storage"] },
];

/// (pane id, setting, extra words)
pub const SETTINGS: &[(&str, &str, &str)] = &[
    ("widgets", "Show widgets", "desktop cards global visible enable disable hide themes"),
    ("widgets", "Your widgets", "enabled saved instances remove clock weather cpu ram music battery"),
    ("widgets", "Available widgets", "collection add install theme user shared universal"),
    ("widgets", "Position", "desktop widget anchor corner edge centre center placement"),
    ("widgets", "Horizontal offset", "widget left right x pixels"),
    ("widgets", "Vertical offset", "widget top bottom y pixels"),
    ("sound", "Output device", "speakers headphones default sink"),
    ("sound", "Input device", "microphone mic default source"),
    ("sound", "Volume", "loudness level slider"),
    ("sound", "Mute", "silence"),
    ("sound", "Applications", "per app volume streams"),
    ("network", "Wi-Fi", "wireless wlan on off radio"),
    ("network", "Available networks", "scan ssid connect join password"),
    ("network", "Saved networks", "connections forget profile"),
    ("network", "VPN", "virtual private network wireguard openvpn"),
    ("network", "Ethernet", "wired cable lan"),
    ("network", "IP address", "dns gateway dhcp"),
    ("bluetooth", "Bluetooth", "power on off adapter"),
    ("bluetooth", "Discoverable", "visible"),
    ("bluetooth", "Devices", "pair connect forget scan headset"),
    ("displays", "Resolution", "mode size"),
    ("displays", "Refresh rate", "hz frequency"),
    ("displays", "Rotation", "orientation portrait landscape"),
    ("displays", "Position", "arrange left right above below"),
    ("displays", "Mirror displays", "extended duplicate clone"),
    ("displays", "Primary display", "main"),
    ("displays", "Scale", "dpi hidpi size text"),
    ("displays", "Night Shift", "color temperature warm gammastep redshift blue light"),
    ("inputdev", "Keyboard layout", "language us de fr xkb"),
    ("inputdev", "Key repeat delay", "typematic"),
    ("inputdev", "Key repeat rate", "typematic speed"),
    ("inputdev", "Caps Lock behaviour", "ctrl escape nocaps xkb option"),
    ("inputdev", "Pointer speed", "mouse acceleration sensitivity"),
    ("inputdev", "Natural scrolling", "touchpad reverse"),
    ("inputdev", "Tap to click", "touchpad tapping"),
    ("notifications", "Do Not Disturb", "dnd focus silence"),
    ("notifications", "Banner duration", "timeout seconds"),
    ("notifications", "Muted apps", "per app mute"),
    ("power", "Power profile", "performance balanced power saver"),
    ("power", "Turn screen off", "blank dpms idle timeout display sleep"),
    ("power", "Screen saver", "blank timeout"),
    ("power", "Battery", "charge level percent laptop"),
    ("power", "Lid and suspend", "close lid sleep hibernate logind"),
    ("defaults", "Web browser", "firefox chromium default"),
    ("defaults", "Email", "mail client mailto"),
    ("defaults", "Terminal", "console shell emulator rdwm-terminal"),
    ("defaults", "File manager", "files folders nautilus thunar"),
    ("defaults", "Media player", "video audio music"),
    ("defaults", "Image viewer", "pictures photos"),
    ("defaults", "Text editor", "notes"),
    ("defaults", "PDF viewer", "documents"),
    ("datetime", "Time zone", "timezone region city"),
    ("datetime", "24-hour clock", "12 hour am pm format"),
    ("datetime", "Date format", "menu bar clock"),
    ("datetime", "Set time automatically", "ntp network time sync"),
    ("users", "Account", "user name login"),
    ("users", "Picture", "avatar face photo"),
    ("users", "Groups", "sudo wheel"),
    ("users", "Login screen", "sddm greeter theme"),
    ("privacy", "Lock screen command", "locker hyprlock swaylock i3lock"),
    ("privacy", "Lock after", "screen lock timeout idle"),
    ("privacy", "Lock on suspend", "sleep"),
    ("storage", "Distribution", "os linux version about"),
    ("storage", "Kernel", "linux version"),
    ("storage", "Processor", "cpu cores"),
    ("storage", "Graphics", "gpu video card lspci"),
    ("storage", "Memory", "ram"),
    ("storage", "Disk usage", "storage space free df"),
    ("storage", "Software versions", "rdwm decebal decbar decdock"),
    ("storage", "Check configuration", "validate config.toml"),
    ("config", "Edit configuration files", "text editor json toml raw decebal.json dock-settings config.toml"),
    ("config", "Presets", "profiles save as preset apply export import share backup restore undo"),
    ("config", "Revert", "previous version backup restore file"),
    ("accessibility", "Cursor size", "large pointer xcursor"),
    ("accessibility", "Reduce motion", "animations"),
    ("accessibility", "Text size", "font scale larger"),
    ("accessibility", "High contrast", "theme"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// Index into `PANES`.
    pub pane: usize,
    /// None = the pane itself, Some = one setting inside it.
    pub label: Option<String>,
}

fn terms(q: &str) -> Vec<String> {
    q.split_whitespace().map(|t| t.to_lowercase()).collect()
}

fn all_match(hay: &str, ts: &[String]) -> bool {
    let h = hay.to_lowercase();
    ts.iter().all(|t| h.contains(t.as_str()))
}

pub fn pane_index(id: &str) -> Option<usize> {
    PANES.iter().position(|p| p.id == id)
}

/// Pane hits first (title, then keywords), then individual settings (at most 4 per pane).
/// `extra` = (pane index, row label + detail) for panes whose settings are not in [`SETTINGS`].
pub fn search(q: &str, extra: &[(usize, String)]) -> Vec<Hit> {
    let ts = terms(q);
    if ts.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (i, p) in PANES.iter().enumerate() {
        if all_match(p.title, &ts) {
            hits.push(Hit { pane: i, label: None });
        }
    }
    for (i, p) in PANES.iter().enumerate() {
        if !hits.iter().any(|h| h.pane == i) && all_match(&format!("{} {}", p.title, p.keywords), &ts) {
            hits.push(Hit { pane: i, label: None });
        }
    }
    let mut per_pane = std::collections::HashMap::<usize, usize>::new();
    let mut add = |hits: &mut Vec<Hit>, pane: usize, label: &str| {
        let n = per_pane.entry(pane).or_default();
        let limit = if PANES[pane].id == "appearance" { 8 } else { 4 };
        if *n >= limit || hits.iter().any(|h| h.pane == pane && h.label.as_deref() == Some(label)) {
            return;
        }
        *n += 1;
        hits.push(Hit { pane, label: Some(label.to_string()) });
    };
    for (pid, label, words) in SETTINGS {
        if let Some(pane) = pane_index(pid) {
            if all_match(&format!("{label} {words}"), &ts) && !PANES[pane].title.eq_ignore_ascii_case(label) {
                add(&mut hits, pane, label);
            }
        }
    }
    if let Some(pane) = pane_index("appearance") {
        for (tab, _, label, words) in crate::settings::appearance::INDEX {
            let pane = if *tab == crate::settings::appearance::Tab::Wallpaper { pane_index("wallpaper").unwrap_or(pane) } else { pane };
            if all_match(&format!("{label} {words}"), &ts) && !PANES[pane].title.eq_ignore_ascii_case(label) {
                add(&mut hits, pane, label);
            }
        }
    }
    // The Appearance pane's keywords match almost anything about looks; when its own settings match, show those (the first
    // result then opens the right tab and section) instead of a bare pane row.
    if let Some(app) = pane_index("appearance") {
        let by_title = all_match(PANES[app].title, &ts);
        if !by_title && hits.iter().any(|h| h.pane == app && h.label.is_some()) {
            hits.retain(|h| !(h.pane == app && h.label.is_none()));
        }
    }
    for (pane, text) in extra {
        if all_match(text, &ts) {
            let label = text.split('\n').next().unwrap_or(text).to_string();
            add(&mut hits, *pane, &label);
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_pane_exists_exactly_once() {
        let mut seen = std::collections::HashSet::new();
        for s in SECTIONS {
            for id in s.panes {
                assert!(pane_index(id).is_some(), "section pane {id} missing from PANES");
                assert!(seen.insert(*id), "{id} listed twice");
            }
        }
        for p in PANES {
            assert!(seen.contains(p.id), "pane {} is not in any sidebar section", p.id);
        }
    }

    #[test]
    fn every_indexed_setting_points_at_a_real_pane() {
        for (pid, label, _) in SETTINGS {
            assert!(pane_index(pid).is_some(), "{pid}: {label}");
        }
    }

    #[test]
    fn volume_finds_sound() {
        let h = search("volume", &[]);
        let sound = pane_index("sound").unwrap();
        assert!(h.iter().any(|x| x.pane == sound && x.label.as_deref() == Some("Volume")), "{h:?}");
        assert_eq!(h[0].pane, sound, "the best hit is the Sound pane's Volume row: {h:?}");
    }

    #[test]
    fn pane_title_hits_come_first_and_terms_are_anded() {
        let h = search("sound", &[]);
        assert_eq!(h[0], Hit { pane: pane_index("sound").unwrap(), label: None });
        let h = search("night shift", &[]);
        assert_eq!(h[0].pane, pane_index("displays").unwrap());
        assert!(search("zzzz", &[]).is_empty());
        assert!(search("   ", &[]).is_empty());
        // both words must match
        assert!(search("wifi banner", &[]).is_empty());
    }

    #[test]
    fn legacy_rows_are_searchable() {
        let tiling = pane_index("tiling").unwrap();
        let h = search("gap", &[(tiling, "Gap between windows".into())]);
        assert!(h.iter().any(|x| x.pane == tiling && x.label.as_deref() == Some("Gap between windows")));
    }

    #[test]
    fn per_pane_limit_and_dedup() {
        let tiling = pane_index("tiling").unwrap();
        let extra: Vec<_> = (0..10).map(|i| (tiling, format!("gap {i}"))).collect();
        let h = search("gap", &extra);
        assert_eq!(h.iter().filter(|x| x.pane == tiling && x.label.is_some()).count(), 4);
    }
}
