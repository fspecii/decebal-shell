//! The GNOME quick-settings look driven through the real bar: layout, hit testing, drags, wheel, groups and every common action.
//! Setters are recorded by the existing dry-run hook (`sys::set_dry_run`), never executed. Tests that would run a status probe
//! (`decnotify dnd status`, `brightnessctl -m`, the X11 idle state) or click a setter run only when `GNOME_CC_PRIVATE_SERVICES` is set,
//! which the evidence driver does with a private `PATH` of recording stub services, a private HOME/XDG, no display and one test thread;
//! see `tests/e2e/gnome-cc-native.py`. The layout, hit-test, group and scale tests run everywhere and touch nothing.
use super::*;
use crate::app::NavKey;
use crate::controlcenter::settings::{Settings, Style, Tiles};

fn private_services() -> bool {
    std::env::var_os("GNOME_CC_PRIVATE_SERVICES").is_some()
}

/// Clicking a control reaches a setter. The dry-run flag is process-wide and other tests switch it concurrently, so a click could
/// run the real tool on the developer's machine; those tests run only in the isolated driver (private PATH, one test thread).
fn actions_allowed(test: &str) -> bool {
    if !private_services() {
        eprintln!("{test}: skipped; run tests/e2e/gnome-cc-native.py (private PATH of stub services, one test thread)");
        return false;
    }
    assert!(!std::env::var("PATH").unwrap_or_default().contains("/usr"), "action tests need the private PATH");
    true
}

/// A theme whose Control Center look is GNOME, for this test thread only (the guard removes it): parallel tests that publish other
/// themes can neither disturb nor be disturbed by it.
struct Gnome {
    _guard: crate::theme::live::ThreadTheme,
}

impl Gnome {
    fn new(tiles: Tiles, scale: f32, dark: bool) -> Gnome {
        let mut t = crate::theme::builtin::def("nord").unwrap().resolve("nord", dark, &crate::theme::model::Selection::default());
        t.control_center = Settings { style: Style::Gnome, tiles, ..Settings::default() };
        t.font_scale = scale;
        t.popup_alpha = 1.0;
        Gnome { _guard: crate::theme::live::thread_theme(t) }
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
        battery: Some(sys::Battery { percent: 82, charging: false, status: "Discharging".into() }),
        util: Some(sys::Utils::default()),
        lock: true,
        power: true,
        ..Snapshot::default()
    }
}

fn control_bar() -> DecBar {
    let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
    bar.width = 1920;
    bar.height = 1080; // a tall surface: the panel is not clipped (the keyboard/scroll tests shrink it on purpose)
    bar.update_snapshot(|s| *s = snapshot());
    bar.relayout_control();
    bar.open = Open::Control;
    bar
}

fn centre_of(bar: &DecBar, t: Target) -> (f32, f32, controlcenter::R) {
    let it = bar.cc_items.iter().find(|i| i.target == Some(t)).expect("target present");
    let (ox, oy) = bar.panel_origin();
    (ox + it.r.x + it.r.w / 2.0, oy + it.r.y + it.r.h / 2.0, it.r)
}

fn click(bar: &mut DecBar, t: Target) {
    let (x, y, _) = centre_of(bar, t);
    assert!(bar.pointer_button(x, y, Button::Left), "{t:?} was not hit at ({x}, {y})");
    bar.pointer_release(x, y, Button::Left);
}

#[test]
fn the_bar_opens_the_gnome_panel_at_gnome_width_with_every_control() {
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Gnome::new(Tiles::ALL, 1.0, true);
    let mut bar = control_bar();
    assert_eq!(bar.cc_style(), Style::Gnome);
    assert!((bar.cc_width() - 400.0).abs() < 0.01);
    assert_eq!(bar.input_rects()[1].w, 400, "input region matches the painted panel");
    for t in [
        Target::Wifi,
        Target::Network,
        Target::Bluetooth,
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
        assert!(bar.cc_items.iter().any(|i| i.target == Some(t)), "{t:?}");
    }
    // the panel paints: its rounded corner is transparent, its surface is the theme's menu colour
    let (w, h) = (1920_u32, bar.size().1.max(1));
    let mut pm = Pixmap::new(w, h).unwrap();
    bar.draw(&mut pm);
    let (ox, oy) = bar.panel_origin();
    let inside = pm.pixel((ox + 9.0) as u32, (oy + bar.cc_height - 40.0) as u32).unwrap();
    let popup = crate::theme::th().popup;
    assert_eq!((inside.red(), inside.green(), inside.blue()), (popup.0, popup.1, popup.2), "panel surface");
    assert_eq!(pm.pixel(ox as u32, oy as u32).unwrap().alpha(), 0, "36 px corner radius");
    if let Some(dir) = std::env::var_os("GNOME_CC_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        pm.save_png(std::path::Path::new(&dir).join("decbar-gnome-control-center.png")).unwrap();
    }
    assert!(bar.key(Key::Escape));
    assert_eq!(bar.open, Open::None);
}

#[test]
fn common_actions_run_through_the_dry_run_hook_from_the_gnome_panel() {
    if !actions_allowed("common_actions") {
        return;
    }
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Gnome::new(Tiles::ALL, 1.0, true);
    sys::set_dry_run(true);
    let _ = sys::take_log();
    let mut bar = control_bar();

    click(&mut bar, Target::Wifi);
    assert_eq!(bar.snapshot().net.map(|n| n.wifi_enabled), Some(false), "optimistic update");
    assert!(sys::take_log().iter().any(|l| l == "nmcli radio wifi off"));

    click(&mut bar, Target::Bluetooth);
    assert_eq!(bar.snapshot().bluetooth.map(|b| b.powered), Some(false));
    assert!(sys::take_log().iter().any(|l| l == "bluetoothctl power off"));

    for (target, want) in [(Target::MediaPrev, "playerctl previous"), (Target::MediaToggle, "playerctl play-pause"), (Target::MediaNext, "playerctl next")] {
        click(&mut bar, target);
        assert!(sys::take_log().iter().any(|l| l == want), "{target:?}");
    }

    for t in [Target::IdleInhibit, Target::GameMode, Target::Dnd] {
        click(&mut bar, t);
    }
    let u = bar.snapshot().util.unwrap();
    assert!(u.idle && u.game, "optimistic update");
    assert_eq!(bar.snapshot().dnd, Some(true));
    let log = sys::take_log();
    for want in ["idle-inhibit toggle", "gamemode toggle", "dnd toggle"] {
        assert!(log.iter().any(|l| l.contains("decutil") && l.ends_with(want)), "{want} in {log:?}");
    }

    // the volume slider's mute button, and the system row's Lock and Power buttons (session helpers), through the same hooks
    click(&mut bar, Target::Mute);
    assert_eq!(bar.snapshot().volume.map(|v| v.muted), Some(true), "optimistic update");
    assert!(sys::take_log().iter().any(|l| l == "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"));
    let mut bar = control_bar();
    click(&mut bar, Target::Lock);
    assert_eq!(bar.open, Open::None);
    assert!(sys::take_log().iter().any(|l| l == "loginctl lock-session" || l == "rdwm-lock"));
    let mut bar = control_bar();
    click(&mut bar, Target::Power);
    assert_eq!(bar.open, Open::None);
    assert!(sys::take_log().iter().any(|l| l.ends_with("decsession")));
    // Screenshot used to bypass the dry-run hook; it must now be recorded (and so never start a real capture here)
    let mut bar = control_bar();
    click(&mut bar, Target::Screenshot);
    assert_eq!(bar.open, Open::None);
    assert!(sys::take_log().iter().any(|l| l.contains("decutil") && l.ends_with("screenshot")), "Screenshot goes through the dry-run hook");

    // Record, the notification history and Settings close the panel and start their tool through the application hook
    let mut bar = control_bar();
    click(&mut bar, Target::Record);
    assert_eq!(bar.open, Open::None);
    assert!(sys::take_log().iter().any(|l| l.contains("decutil") && l.contains("record toggle")));
    let mut bar = control_bar();
    click(&mut bar, Target::Notifications);
    assert!(sys::take_log().iter().any(|l| l.contains("decnotify history")));
    let mut bar = control_bar();
    click(&mut bar, Target::Settings);
    assert_eq!(bar.open, Open::None);
    sys::set_dry_run(false);
}

#[test]
fn probe_dependent_actions_run_against_private_fake_services_only() {
    if !actions_allowed("probe_dependent_actions") {
        return;
    }
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Gnome::new(Tiles::ALL, 1.0, true);
    sys::set_dry_run(true);
    let _ = sys::take_log();
    let mut bar = control_bar();
    for (target, wants) in [(Target::Focus, ["decnotify dnd toggle", ""]), (Target::NightShift, ["omarchy toggle nightlight", ""]), (Target::DarkMode, ["set mode", "appearance toggle"])] {
        click(&mut bar, target);
        let log = sys::take_log();
        assert!(log.iter().any(|l| wants.iter().any(|w| !w.is_empty() && l.contains(w))), "{target:?}: {log:?}");
        bar.open = Open::Control;
        bar.relayout_control();
    }
    // Brightness: a click on the slider commits the value under the pointer
    let (_, y, r) = centre_of(&bar, Target::Brightness);
    let (ox, _) = bar.panel_origin();
    assert!(bar.pointer_button(ox + r.x + r.w * 0.30, y, Button::Left));
    assert!(bar.pointer_release(ox + r.x + r.w * 0.30, y, Button::Left));
    assert_eq!(bar.snapshot().brightness, Some(30));
    assert!(sys::take_log().iter().any(|l| l.starts_with("brightnessctl -q -c backlight set 30%")));
    sys::set_dry_run(false);
}

#[test]
fn volume_slider_drags_and_wheel_work_in_the_gnome_look() {
    if !actions_allowed("volume_slider") {
        return;
    }
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Gnome::new(Tiles::ALL, 1.0, true);
    sys::set_dry_run(true);
    let _ = sys::take_log();
    let mut bar = control_bar();
    let (_, y, r) = centre_of(&bar, Target::Volume);
    let (ox, _) = bar.panel_origin();
    assert!(bar.pointer_button(ox + r.x + r.w * 0.25, y, Button::Left));
    assert!((bar.snapshot().volume.unwrap().level - 0.25).abs() < 0.01);
    assert!(bar.pointer_motion(ox + r.x + r.w * 0.8, y + 40.0), "the drag continues off the row");
    assert!((bar.snapshot().volume.unwrap().level - 0.8).abs() < 0.01);
    assert!(bar.pointer_release(ox + r.x + r.w + 100.0, y, Button::Left));
    assert!((bar.snapshot().volume.unwrap().level - 1.0).abs() < 0.01, "clamped at the end");
    // the 6 px above the visible track is still the slider
    assert!(bar.pointer_button(ox + r.x + r.w * 0.5, y - r.h / 2.0 - 4.0, Button::Left));
    bar.pointer_release(ox + r.x + r.w * 0.5, y, Button::Left);
    assert!((bar.snapshot().volume.unwrap().level - 0.5).abs() < 0.02);
    // wheel: 5 % per notch
    let (x, y, _) = centre_of(&bar, Target::Volume);
    assert!(bar.scroll(x, y, -1.0));
    assert!((bar.snapshot().volume.unwrap().level - 0.55).abs() < 0.02);
    sys::set_dry_run(false);
}

#[test]
fn disabled_groups_leave_the_gnome_panel_without_their_controls() {
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    for (group, absent) in [
        ("connectivity", Target::Wifi),
        ("focus", Target::DarkMode),
        ("display", Target::Brightness),
        ("sound", Target::Volume),
        ("media", Target::MediaToggle),
        ("toggles", Target::IdleInhibit),
        ("actions", Target::Settings),
    ] {
        let mut tiles = Tiles::ALL;
        tiles.set(group, false);
        let _theme = Gnome::new(tiles, 1.0, true);
        let bar = control_bar();
        assert!(!bar.cc_items.iter().any(|i| i.target == Some(absent)), "{group}");
        assert!(bar.cc_items.iter().any(|i| i.target == Some(Target::Volume) || i.target == Some(Target::Wifi) || i.target == Some(Target::Settings)), "{group}: the rest stays");
    }
}

#[test]
fn the_panel_follows_the_text_scale_and_stays_inside_the_surface_width() {
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    for scale in [1.0_f32, 1.25, 2.0] {
        let _theme = Gnome::new(Tiles::ALL, scale, false);
        let bar = control_bar();
        assert!((bar.cc_width() - (352.0 * scale + 48.0)).abs() < 0.5, "{scale}: {}", bar.cc_width());
        let (ox, _) = bar.panel_origin();
        assert!(ox >= 0.0 && ox + bar.cc_width() <= bar.width as f32 + 0.5, "{scale}: panel at {ox}");
        for i in &bar.cc_items {
            assert!(i.r.x >= 0.0 && i.r.x + i.r.w <= bar.cc_width() + 0.01, "{scale}: {i:?}");
        }
    }
}

/// Accent-coloured pixels in the 2 px band standing 3 px outside `r` (the GNOME focus ring) of a panel drawn at `origin`.
fn ring_pixels(pm: &Pixmap, origin: (f32, f32), r: controlcenter::R, accent: (u8, u8, u8)) -> usize {
    let (x, y) = (origin.0 + r.x, origin.1 + r.y);
    let mut count = 0;
    for yy in (y - 4.0).max(0.0) as u32..((y + r.h + 4.0) as u32).min(pm.height()) {
        for xx in (x - 4.0).max(0.0) as u32..((x + r.w + 4.0) as u32).min(pm.width()) {
            let (fx, fy) = (xx as f32, yy as f32);
            let inside = fx >= x - 1.0 && fx < x + r.w + 1.0 && fy >= y - 1.0 && fy < y + r.h + 1.0;
            let p = pm.pixel(xx, yy).unwrap();
            if !inside && p.alpha() == 255 && (p.red() as i32 - accent.0 as i32).abs() + (p.green() as i32 - accent.1 as i32).abs() + (p.blue() as i32 - accent.2 as i32).abs() < 40 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn keyboard_walks_every_control_with_a_ring_and_scrolls_the_clipped_panel() {
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Gnome::new(Tiles::ALL, 2.0, true);
    let mut bar = control_bar();
    bar.height = 420; // a short surface at 200 %: the panel is clipped
    bar.relayout_control();
    assert!(bar.cc_clipped() && bar.cc_visible_height() < bar.cc_height, "short surface clips the panel");
    let order: Vec<Target> = controlcenter::targets_in_order(&bar.cc_items);
    assert_eq!(order.len(), 22);
    let accent = crate::theme::th().accent.t();
    let width = 1920;
    for (index, target) in order.iter().enumerate() {
        assert!(bar.nav_key(NavKey::Down), "Down moves focus");
        assert_eq!(bar.cc_viewport.focus, Some(*target), "stop {index}");
        // the focused control is inside the visible viewport and wears the accent ring
        let r = bar.cc_items.iter().find(|i| i.target == Some(*target)).unwrap().r;
        let scroll = bar.cc_viewport.scroll;
        assert!(r.y - scroll >= -0.5 && r.y + r.h - scroll <= bar.cc_visible_height() + 0.5, "{target:?} is revealed: y {} scroll {scroll} viewport {}", r.y, bar.cc_visible_height());
        if index % 7 == 0 || index == order.len() - 1 {
            let mut pm = Pixmap::new(width, 420).unwrap(); // `draw` takes the surface height from the pixmap
            bar.draw(&mut pm);
            let (ox, oy) = bar.panel_origin();
            let ring = ring_pixels(&pm, (ox, oy - scroll), r, accent);
            assert!(ring > 20, "{target:?}: a visible focus ring ({ring} accent pixels)");
        }
    }
    assert!(bar.cc_viewport.scroll > 0.0, "reaching the last control scrolled the panel (viewport {} content {})", bar.cc_visible_height(), bar.cc_height);
    assert!(bar.nav_key(NavKey::Down), "wraps");
    assert_eq!(bar.cc_viewport.focus, Some(order[0]));
    assert!(bar.nav_key(NavKey::Up));
    assert_eq!(bar.cc_viewport.focus, Some(*order.last().unwrap()), "Up wraps to the last");
}

#[test]
fn wheel_scrolls_the_clipped_panel_and_a_point_below_it_reaches_nothing() {
    if !actions_allowed("wheel_scrolls") {
        return;
    }
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _theme = Gnome::new(Tiles::ALL, 2.0, false);
    sys::set_dry_run(true);
    let _ = sys::take_log();
    let mut bar = control_bar();
    bar.height = 420;
    bar.relayout_control();
    let (ox, oy) = bar.panel_origin();
    let (px, py) = (ox + 40.0, oy + 30.0);
    // the wheel over a non-slider scrolls, clamped at both ends
    for _ in 0..80 {
        bar.scroll(px, py, 1.0);
    }
    assert!((bar.cc_viewport.scroll - (bar.cc_height - bar.cc_visible_height())).abs() < 0.5, "clamped at the end: {}", bar.cc_viewport.scroll);
    for _ in 0..80 {
        bar.scroll(px, py, -1.0);
    }
    assert_eq!(bar.cc_viewport.scroll, 0.0);
    // a click below the visible panel is outside it: it closes the panel and starts nothing, even where a clipped control would be
    let hidden = bar.cc_items.iter().find(|i| i.target == Some(Target::MediaNext)).unwrap().r;
    assert!(oy + hidden.y > oy + bar.cc_visible_height(), "MediaNext starts below the fold");
    assert!(bar.pointer_button(ox + hidden.x + hidden.w / 2.0, oy + hidden.y + hidden.h / 2.0, Button::Left));
    assert_eq!(bar.open, Open::None);
    assert!(sys::take_log().is_empty(), "nothing was started: {:?}", sys::take_log());
    sys::set_dry_run(false);
}
