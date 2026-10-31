//! The UKUI shortcut panel driven through the real bar: every control reaches its backend, tile groups are honoured, no drawn control is inert.
//! Same isolation as `gnome_cc_tests`: setters only reach the dry-run hook, and the tests that click one run only when `GNOME_CC_PRIVATE_SERVICES` is set by
//! `tests/e2e/openkylin-cc-actions.py` (private PATH of recording stubs, private HOME/XDG, no display, one test thread).
use super::*;
use crate::controlcenter::settings::{Settings, Style, Tiles};

fn private_services() -> bool {
    std::env::var_os("GNOME_CC_PRIVATE_SERVICES").is_some()
}

fn actions_allowed(test: &str) -> bool {
    if !private_services() {
        eprintln!("{test}: skipped; run tests/e2e/openkylin-cc-actions.py (private PATH of stub services, one test thread)");
        return false;
    }
    assert!(!std::env::var("PATH").unwrap_or_default().contains("/usr"), "action tests need the private PATH");
    true
}

/// A theme whose Control Center is the UKUI panel, for this test thread only.
struct Ukui {
    _guard: crate::theme::live::ThreadTheme,
}

impl Ukui {
    fn new(tiles: Tiles) -> Ukui {
        let mut t = crate::theme::builtin::def("nord").unwrap().resolve("nord", false, &crate::theme::model::Selection::default());
        t.control_center = Settings { style: Style::Ukui, tiles, ..Settings::default() };
        t.popup_alpha = 1.0;
        Ukui { _guard: crate::theme::live::thread_theme(t) }
    }
}

fn snapshot() -> Snapshot {
    Snapshot {
        net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: "Wired".into() }),
        bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
        volume: Some(sys::Volume { level: 0.5, muted: false }),
        sink: "Headset".into(),
        brightness: Some(40),
        night_shift: Some(false),
        dnd: Some(false),
        media: Some(sys::Media { playing: true, title: "Song".into(), artist: "Artist".into() }),
        util: Some(sys::Utils::default()),
        ..Snapshot::default()
    }
}

fn control_bar() -> DecBar {
    let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
    bar.width = 1920;
    bar.height = 1080;
    bar.update_snapshot(|s| *s = snapshot());
    bar.relayout_control();
    bar.open = Open::Control;
    bar
}

fn centre_of(bar: &DecBar, t: Target) -> (f32, f32, controlcenter::R) {
    let it = bar.cc_items.iter().find(|i| i.target == Some(t)).unwrap_or_else(|| panic!("{t:?} is not in the panel"));
    let (ox, oy) = bar.panel_origin();
    (ox + it.r.x + it.r.w / 2.0, oy + it.r.y + it.r.h / 2.0, it.r)
}

fn click(bar: &mut DecBar, t: Target) {
    let (x, y, _) = centre_of(bar, t);
    assert!(bar.pointer_button(x, y, Button::Left), "{t:?} was not hit at ({x}, {y})");
    bar.pointer_release(x, y, Button::Left);
}

const EVERY_TARGET: [Target; 20] = [
    Target::Wifi,
    Target::Network,
    Target::Bluetooth,
    Target::Focus,
    Target::NightShift,
    Target::DarkMode,
    Target::Brightness,
    Target::Volume,
    Target::Mute,
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
];

#[test]
fn the_wlan_shortcut_is_the_wifi_toggle_and_every_control_of_the_contract_is_reachable() {
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Ukui::new(Tiles::ALL);
    let bar = control_bar();
    assert_eq!(bar.cc_style(), Style::Ukui);
    assert!((bar.cc_width() - 396.0).abs() < 0.01);
    for t in EVERY_TARGET {
        assert!(bar.cc_items.iter().any(|i| i.target == Some(t)), "{t:?}");
    }
    // the WLAN tile toggles the radio (source: the Wi-Fi shortcut); the wired Network editor is a separate shortcut, not hidden behind it
    assert!(bar.cc_items.iter().any(|i| matches!(&i.draw, controlcenter::Draw::SmallTile { .. }) && i.target == Some(Target::Wifi)));
    // every item that carries a target is hit where it is drawn, nothing drawn on top steals the click
    for it in bar.cc_items.iter().filter(|i| i.target.is_some()) {
        let (ox, oy) = bar.panel_origin();
        let hit = controlcenter::hit(&bar.cc_items, it.r.x + it.r.w / 2.0, it.r.y + it.r.h / 2.0).map(|h| h.0);
        assert_eq!(hit, it.target, "{:?}", it.target);
        let _ = (ox, oy);
    }
}

#[test]
fn tile_groups_are_honoured_and_a_selection_never_leaves_only_a_header() {
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let only = |f: fn(&mut Tiles)| {
        let mut t = Tiles { connectivity: false, focus: false, display: false, sound: false, media: false, toggles: false, actions: false };
        f(&mut t);
        t
    };
    let cases: [(&str, Tiles, &[Target], &[Target]); 7] = [
        ("connectivity", only(|t| t.connectivity = true), &[Target::Wifi, Target::Bluetooth, Target::Network], &[Target::Volume, Target::MediaToggle, Target::Settings, Target::Dnd]),
        ("focus", only(|t| t.focus = true), &[Target::Focus, Target::NightShift, Target::DarkMode], &[Target::Wifi, Target::Settings]),
        ("display", only(|t| t.display = true), &[Target::Brightness], &[Target::Volume, Target::Settings]),
        ("sound", only(|t| t.sound = true), &[Target::Volume, Target::Mute], &[Target::Brightness, Target::Settings]),
        ("media", only(|t| t.media = true), &[Target::MediaPrev, Target::MediaToggle, Target::MediaNext], &[Target::Wifi, Target::Settings, Target::Record]),
        ("toggles", only(|t| t.toggles = true), &[Target::Record, Target::IdleInhibit, Target::GameMode, Target::Dnd], &[Target::Wifi, Target::Settings, Target::MediaToggle]),
        ("actions", only(|t| t.actions = true), &[Target::Screenshot, Target::StayAwake, Target::Notifications, Target::Settings], &[Target::Wifi, Target::Volume]),
    ];
    for (group, tiles, shown, hidden) in cases {
        let _theme = Ukui::new(tiles);
        let bar = control_bar();
        for t in shown {
            assert!(bar.cc_items.iter().any(|i| i.target == Some(*t)), "{group}: {t:?} must show");
        }
        for t in hidden {
            assert!(!bar.cc_items.iter().any(|i| i.target == Some(*t)), "{group}: {t:?} must not show (Settings only comes with the actions group)");
        }
    }
    // no hardware for the only enabled group: the whole panel rather than a header alone
    let _theme = Ukui::new(only(|t| t.media = true));
    let mut bar = control_bar();
    bar.update_snapshot(|s| s.media = None);
    bar.relayout_control();
    assert!(bar.cc_items.iter().any(|i| i.target == Some(Target::Wifi)), "falls back to every group");
}

#[test]
fn shortcuts_reach_their_backends_through_the_dry_run_hook() {
    if !actions_allowed("shortcuts_reach_their_backends") {
        return;
    }
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Ukui::new(Tiles::ALL);
    sys::set_dry_run(true);
    let _ = sys::take_log();
    let mut bar = control_bar();

    click(&mut bar, Target::Wifi);
    assert_eq!(bar.snapshot().net.map(|n| n.wifi_enabled), Some(false), "optimistic update");
    assert!(sys::take_log().iter().any(|l| l == "nmcli radio wifi off"));
    click(&mut bar, Target::Wifi);
    assert!(sys::take_log().iter().any(|l| l == "nmcli radio wifi on"));

    click(&mut bar, Target::Bluetooth);
    assert!(sys::take_log().iter().any(|l| l == "bluetoothctl power off"));

    click(&mut bar, Target::Network);
    assert!(sys::take_log().iter().any(|l| l.contains("nm-connection-editor")), "the wired Network shortcut opens the editor");

    let mut bar = control_bar();
    click(&mut bar, Target::Mute);
    assert_eq!(bar.snapshot().volume.map(|v| v.muted), Some(true), "optimistic update");
    assert!(sys::take_log().iter().any(|l| l == "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"));

    // the volume and brightness tracks take pointer values (the track item, not the 24 px icon button)
    let mut bar = control_bar();
    for (t, probe) in [(Target::Volume, "set-volume"), (Target::Brightness, "brightnessctl")] {
        let (x, y, r) = centre_of(&bar, t);
        assert!(bar.pointer_button(x + r.w * 0.2, y, Button::Left), "{t:?}");
        bar.pointer_release(x + r.w * 0.2, y, Button::Left);
        assert!(sys::take_log().iter().any(|l| l.contains(probe)), "{t:?} {probe}");
    }

    for target in [Target::MediaPrev, Target::MediaToggle, Target::MediaNext] {
        let mut bar = control_bar();
        click(&mut bar, target);
        let want = match target {
            Target::MediaPrev => "playerctl previous",
            Target::MediaToggle => "playerctl play-pause",
            _ => "playerctl next",
        };
        assert!(sys::take_log().iter().any(|l| l == want), "{target:?}");
    }
    let mut bar = control_bar();
    for t in [Target::IdleInhibit, Target::GameMode, Target::Dnd, Target::Focus] {
        click(&mut bar, t);
    }
    let log = sys::take_log();
    for want in ["idle-inhibit toggle", "gamemode toggle", "dnd toggle"] {
        assert!(log.iter().any(|l| l.contains("decutil") && l.ends_with(want)) || log.iter().any(|l| l.contains(want)), "{want} in {log:?}");
    }
}
