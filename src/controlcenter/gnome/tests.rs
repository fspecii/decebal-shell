use super::*;
use crate::theme::builtin;
use crate::theme::model::Selection;

fn full() -> Snapshot {
    Snapshot {
        user: None,
        net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: "Wired connection 1".into() }),
        bluetooth: Some(sys::Bluetooth { powered: true, connected: 2 }),
        volume: Some(sys::Volume { level: 0.5, muted: false }),
        sink: "Built-in Audio Analog Stereo".into(),
        brightness: Some(40),
        night_shift: Some(false),
        dnd: Some(false),
        media: Some(sys::Media { playing: true, title: "Song".into(), artist: "Artist".into() }),
        awake: false,
        light: false,
        battery: Some(sys::Battery { percent: 82, charging: false, status: "Discharging".into() }),
        util: Some(sys::Utils::default()),
        lock: true,
        power: true,
    }
}

fn theme(id: &str, dark: bool, scale: f32) -> crate::theme::model::Theme {
    let mut t = builtin::def(id).unwrap().resolve(id, dark, &Selection { mode: if dark { crate::theme::model::Mode::Dark } else { crate::theme::model::Mode::Light }, ..Selection::default() });
    t.font_scale = scale;
    t.popup_alpha = 1.0;
    t
}

const ALL_TARGETS: [Target; 22] = [
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
    Target::Mute,
    Target::Lock,
    Target::Power,
];

fn toggles(items: &[Item]) -> Vec<&Item> {
    items.iter().filter(|i| matches!(i.draw, Draw::Toggle { .. })).collect()
}

#[test]
fn metrics_are_the_pinned_gnome_shell_51_1_values_at_100_percent() {
    // _quick-settings.scss: padding 18, grid gaps 12, toggle min-width 12em (11 pt = 14.67 px) and min-height 3 icons (48 px).
    assert!((panel_width_at(1.0) - 400.0).abs() < 0.01, "2 * 176 + 12 + 2 * 18");
    assert_eq!(PANEL_RADIUS, 36.0, "$modal_radius * 2.25");
    let (items, h) = layout_at(&full(), &Tiles::ALL, 1.0);
    let pills: Vec<_> = toggles(&items).into_iter().filter(|i| matches!(i.draw, Draw::Toggle { caps: Caps::Pill, .. })).collect();
    assert!(pills.len() >= 8);
    for t in &pills {
        assert!((t.r.w - 176.0).abs() < 0.01 && (t.r.h - 48.0).abs() < 0.01, "{:?}", t.r);
        assert!(t.r.x == 18.0 || (t.r.x - 206.0).abs() < 0.01, "two columns at 18 and 18 + 176 + 12");
    }
    // the split Wi-Fi tile is one 176 px pill made of a toggle and a 37 px menu button
    let split: Vec<_> = items.iter().filter(|i| matches!(i.draw, Draw::Toggle { caps: Caps::Left | Caps::Right, .. })).collect();
    assert_eq!(split.len(), 2);
    assert!((split[0].r.w + split[1].r.w - 176.0).abs() < 0.01 && (split[1].r.w - 37.0).abs() < 0.01);
    assert_eq!(split[0].target, Some(Target::Wifi));
    assert_eq!(split[1].target, Some(Target::Network));
    assert!(h > 400.0 && h < 900.0, "{h}");
}

#[test]
fn every_common_control_is_reachable_and_nothing_is_a_placeholder() {
    let (items, _) = layout_at(&full(), &Tiles::ALL, 1.0);
    for t in ALL_TARGETS {
        assert!(items.iter().any(|i| i.target == Some(t)), "{t:?} missing");
    }
    // Lock and Power exist only while the common controller found their programs: a snapshot that says so draws no such button
    let (items, _) = layout_at(&Snapshot { lock: false, power: false, ..full() }, &Tiles::ALL, 1.0);
    for t in [Target::Lock, Target::Power] {
        assert!(!items.iter().any(|i| i.target == Some(t)), "{t:?} must not be offered without its program");
    }
    let (items, _) = layout_at(&Snapshot { lock: true, power: false, ..full() }, &Tiles::ALL, 1.0);
    assert!(items.iter().any(|i| i.target == Some(Target::Lock)) && !items.iter().any(|i| i.target == Some(Target::Power)));
}

#[test]
fn disabled_groups_remove_exactly_their_controls() {
    let groups: [(&str, &[Target]); 7] = [
        ("connectivity", &[Target::Wifi, Target::Bluetooth, Target::Network]),
        ("focus", &[Target::Focus, Target::NightShift, Target::DarkMode]),
        ("display", &[Target::Brightness]),
        ("sound", &[Target::Volume, Target::Mute]),
        ("media", &[Target::MediaPrev, Target::MediaToggle, Target::MediaNext]),
        ("toggles", &[Target::IdleInhibit, Target::GameMode, Target::Dnd]),
        ("actions", &[Target::StayAwake, Target::Screenshot, Target::Settings, Target::Notifications, Target::Lock, Target::Power]),
    ];
    for (name, gone) in groups {
        let mut tiles = Tiles::ALL;
        tiles.set(name, false);
        let (items, _) = layout_at(&full(), &tiles, 1.0);
        for t in gone {
            assert!(!items.iter().any(|i| i.target == Some(*t)), "{name}: {t:?} must be gone");
        }
        for t in ALL_TARGETS.iter().filter(|t| !gone.contains(t) && **t != Target::Record) {
            assert!(items.iter().any(|i| i.target == Some(*t)), "{name} off must keep {t:?}");
        }
        // Record lives in the decutil toggles group
        assert_eq!(items.iter().any(|i| i.target == Some(Target::Record)), name != "toggles" || false, "{name}");
    }
    // An unavailable selection remains visible and inert without restoring other groups.
    let bare = Snapshot::default();
    let (items, h) = layout_at(&bare, &Tiles::from_names(["display"]), 1.0);
    assert!(!items.is_empty() && h > 0.0);
    assert!(items.iter().all(|item| item.target.is_none()));
    assert!(targets_in_order(&items).is_empty());
}

#[test]
fn missing_providers_are_hidden_or_marked_unavailable_never_invented() {
    let (items, _) = layout_at(&Snapshot::default(), &Tiles::ALL, 1.0);
    for t in [
        Target::Wifi,
        Target::Bluetooth,
        Target::Network,
        Target::Brightness,
        Target::Volume,
        Target::MediaToggle,
        Target::IdleInhibit,
        Target::GameMode,
        Target::Dnd,
        Target::NightShift,
        Target::Focus,
        Target::Notifications,
    ] {
        assert!(!items.iter().any(|i| i.target == Some(t)), "{t:?} has no provider and must not be clickable");
    }
    let texts: Vec<_> = items.iter().filter_map(|i| if let Draw::Text { text, .. } = &i.draw { Some(text.as_str()) } else { None }).collect();
    assert_eq!(texts.iter().filter(|t| **t == "Unavailable").count(), 2, "Night Light and Do Not Disturb say so: {texts:?}");
    assert!(!texts.iter().any(|t| t.contains('%')), "no battery readout without a battery");
    // what stays: dark style, Stay Awake, Record (no decutil), Screenshot, Settings
    for t in [Target::DarkMode, Target::StayAwake, Target::Record, Target::Screenshot, Target::Settings] {
        assert!(items.iter().any(|i| i.target == Some(t)), "{t:?}");
    }
}

#[test]
fn state_changes_text_icons_and_order() {
    let mut s = full();
    s.net = Some(sys::Net { wifi_enabled: false, ..Default::default() });
    s.bluetooth = Some(sys::Bluetooth { powered: false, connected: 0 });
    s.dnd = Some(true);
    s.light = true;
    s.volume = Some(sys::Volume { level: 0.9, muted: true });
    let (items, _) = layout_at(&s, &Tiles::ALL, 1.0);
    let texts: Vec<String> = items.iter().filter_map(|i| if let Draw::Text { text, .. } = &i.draw { Some(text.clone()) } else { None }).collect();
    assert!(texts.iter().any(|t| t == "Off"), "wifi off");
    assert!(!texts.iter().any(|t| t.contains("Connected")), "powered-off bluetooth shows no device count");
    let slider = items.iter().find(|i| matches!(i.draw, Draw::Slider { .. }) && i.target == Some(Target::Volume)).unwrap();
    assert!(matches!(slider.draw, Draw::Slider { value, .. } if value == 0.0), "muted shows empty");
    assert!(items.iter().any(|i| i.target == Some(Target::Mute) && matches!(&i.draw, Draw::Circle { icon: "audio-volume-muted-symbolic", .. })));
    let on = |t: Target| matches!(items.iter().find(|i| i.target == Some(t) && matches!(i.draw, Draw::Toggle { .. })).unwrap().draw, Draw::Toggle { on: true, .. });
    assert!(on(Target::Focus) && !on(Target::DarkMode) && !on(Target::Wifi) && !on(Target::Bluetooth));
    // order: system row, sound, display, then the grid, then the media card (gnome-shell panel.js)
    let y = |t: Target| items.iter().find(|i| i.target == Some(t)).unwrap().r.y;
    assert!(y(Target::Settings) < y(Target::Volume) && y(Target::Volume) < y(Target::Brightness) && y(Target::Brightness) < y(Target::Wifi) && y(Target::Wifi) < y(Target::MediaToggle));
}

#[test]
fn layout_is_responsive_at_every_text_scale() {
    for k in [0.8_f32, 1.0, 1.25, 1.5, 2.0, 3.0] {
        let (items, h) = layout_at(&full(), &Tiles::ALL, k);
        let w = panel_width_at(k);
        for i in &items {
            assert!(i.r.x >= 0.0 && i.r.x + i.r.w <= w + 0.01, "{k}: {:?} outside width {w}", i);
            assert!(i.r.y >= -6.01 && i.r.y + i.r.h <= h + 0.01, "{k}: {:?} outside height {h}", i);
        }
        // toggles never overlap and keep the 12 px gaps; they scale with the text
        let t = toggles(&items);
        for (a, b) in t.iter().enumerate().flat_map(|(n, a)| t.iter().skip(n + 1).map(move |b| (*a, *b))) {
            let apart = a.r.x + a.r.w <= b.r.x + 0.01 || b.r.x + b.r.w <= a.r.x + 0.01 || a.r.y + a.r.h <= b.r.y + 0.01 || b.r.y + b.r.h <= a.r.y + 0.01;
            assert!(apart, "{k}: {:?} overlaps {:?}", a.r, b.r);
        }
        let full_pill = t.iter().find(|i| matches!(i.draw, Draw::Toggle { caps: Caps::Pill, .. })).unwrap();
        assert!((full_pill.r.h - 48.0 * k).abs() < 0.01 && (full_pill.r.w - 176.0 * k).abs() < 0.01);
        // touch targets: every control is at least a 16 px icon with 6 px padding (28 px at 100 %); sliders add 6 px of hit area
        for i in items.iter().filter(|i| i.target.is_some()) {
            let min = 16.0 * k + 12.0;
            assert!(i.r.h + 0.01 >= min && i.r.w + 0.01 >= min, "{k}: {:?}", i);
        }
        assert!(h >= panel_width_at(1.0) * 0.9 * k.min(1.0), "{k}: {h}");
    }
}

#[test]
fn slider_hit_areas_cover_the_row_and_map_like_the_common_controller() {
    let (items, _) = layout_at(&full(), &Tiles::ALL, 1.0);
    for t in [Target::Volume, Target::Brightness] {
        let (hit_t, r) = hit(&items, 300.0, items.iter().find(|i| i.target == Some(t)).unwrap().r.y + 1.0).unwrap();
        assert_eq!(hit_t, t);
        let row = items.iter().find(|i| i.target == Some(t) && matches!(i.draw, Draw::Slider { .. })).unwrap().r;
        assert!(r.h >= row.h + 12.0 - 0.01, "6 px above and below");
        assert_eq!((r.x, r.w), (row.x, row.w), "pointer maps over exactly the visible track");
        assert_eq!(slider_value(row.x, r.x, r.w), 0.0);
        assert_eq!(slider_value(row.x + row.w, r.x, r.w), 1.0);
        // 6 px above the visible track still adjusts the same slider; a point beyond that does not
        assert_eq!(hit(&items, row.x + 40.0, row.y - 5.0).map(|h| h.0), Some(t));
        assert_ne!(hit(&items, row.x + 40.0, row.y - 9.0).map(|h| h.0), Some(t));
    }
}

#[test]
fn keyboard_order_visits_each_control_once_top_to_bottom_left_to_right() {
    let (items, _) = layout_at(&full(), &Tiles::ALL, 1.0);
    let mut seen = Vec::new();
    for i in &items {
        if let Some(t) = i.target {
            if !seen.iter().any(|(s, _): &(Target, R)| *s == t) {
                seen.push((t, i.r));
            }
        }
    }
    assert_eq!(seen.len(), ALL_TARGETS.len(), "{seen:?}");
    // the first target of every control is its visible hit rectangle, in reading order
    let order: Vec<Target> = seen.iter().map(|s| s.0).collect();
    let pos = |t| order.iter().position(|o| *o == t).unwrap();
    assert!(pos(Target::Screenshot) < pos(Target::Settings) && pos(Target::Settings) < pos(Target::Lock) && pos(Target::Lock) < pos(Target::Power));
    assert!(pos(Target::Power) < pos(Target::Mute) && pos(Target::Mute) < pos(Target::Volume), "the mute button precedes its slider");
    assert!(pos(Target::Volume) < pos(Target::Brightness) && pos(Target::Brightness) < pos(Target::Wifi));
    assert!(pos(Target::Wifi) < pos(Target::Network) && pos(Target::Network) < pos(Target::Bluetooth), "the Wi-Fi menu button follows its toggle");
}

#[test]
fn colours_follow_the_shell_button_formulas() {
    // dark: $bg_color #36363a, white text: button = mix(fg, bg, 9 %), hover lighten 4 %, default = accent
    let dark = Palette { dark: true, bg: Rgb(0x36, 0x36, 0x3a), fg: Rgb(255, 255, 255), accent: Rgb(0x35, 0x84, 0xe4), accent_fg: Rgb(255, 255, 255) };
    assert_eq!(dark.button(dark.bg), Rgb(0x48, 0x48, 0x4c));
    assert!(hsl(dark.button_hover(dark.bg)).2 > hsl(dark.button(dark.bg)).2 + 0.039);
    assert_eq!(dark.default_button(false), dark.accent);
    assert!(hsl(dark.default_button(true)).2 > hsl(dark.accent).2);
    assert_eq!(dark.insensitive_fg(), Rgb(0x9b, 0x9b, 0x9d), "50 % mix: the dark text_secondary token of gnome-adwaita");
    // light: darken for hover, 12 % mix, 60 % insensitive
    let light = Palette { dark: false, bg: Rgb(0xfa, 0xfa, 0xfb), fg: Rgb(0x22, 0x22, 0x26), accent: dark.accent, accent_fg: Rgb(255, 255, 255) };
    assert_eq!(light.button(light.bg), mix(light.fg, light.bg, 0.12));
    assert!(hsl(light.button_hover(light.bg)).2 < hsl(light.button(light.bg)).2 - 0.039);
    assert_eq!(light.insensitive_fg(), Rgb(0x78, 0x78, 0x7b));
    // HSL round trip and Sass mix sanity
    for c in [Rgb(10, 200, 90), Rgb(0, 0, 0), Rgb(255, 255, 255), Rgb(0x35, 0x84, 0xe4)] {
        let (h, s, l) = hsl(c);
        assert_eq!(from_hsl(h, s, l), c);
    }
    assert_eq!(mix(Rgb(255, 255, 255), Rgb(0, 0, 0), 0.5), Rgb(128, 128, 128));
}

#[test]
fn authentic_adwaita_icons_load_from_the_installed_theme_and_are_recoloured_else_builtin_glyphs_are_used() {
    let dir = std::env::temp_dir().join(format!("gnome-cc-icons-{}", std::process::id()));
    let status = dir.join("icons/Adwaita/symbolic/status");
    std::fs::create_dir_all(&status).unwrap();
    std::fs::write(
        status.join("night-light-symbolic.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16px" height="16px" viewBox="0 0 16 16"><g fill="#2e3436"><rect x="2" y="2" width="12" height="12"/></g></svg>"##,
    )
    .unwrap();
    let roots = vec![dir.join("icons")];
    assert!(locate_icon(&roots, "night-light-symbolic").is_some());
    assert!(locate_icon(&roots, "../night-light-symbolic").is_none(), "no traversal");
    assert!(locate_icon(&roots, "audio-volume-high-symbolic").is_none());
    let pm = render_authentic(&roots, "night-light-symbolic", (255, 0, 0), 16).unwrap();
    let centre = pm.pixel(8, 8).unwrap();
    assert_eq!((centre.red(), centre.green(), centre.blue(), centre.alpha()), (255, 0, 0, 255), "recoloured to the requested ink");
    assert!(render_authentic(&[], "night-light-symbolic", (0, 0, 0), 16).is_none());
    assert_eq!(tint_svg("fill=\"#2e3436\" fill=\"#33d17a\" style=\"fill:#222\"", (1, 2, 3)), "fill=\"#010203\" fill=\"#33d17a\" style=\"fill:#010203\"");
    // without the theme every icon the layout asks for has a built-in glyph, except the battery levels (readout stays as text)
    let mut symbols = SymbolCache::default();
    let (items, _) = layout_at(&full(), &Tiles::ALL, 1.0);
    for i in &items {
        let name = match &i.draw {
            Draw::Glyph { icon, .. } | Draw::Circle { icon, .. } => *icon,
            _ => continue,
        };
        if name.starts_with("battery-level-") {
            assert!(fallback_glyph(name).is_none());
            continue;
        }
        let glyph = fallback_glyph(name).unwrap_or_else(|| panic!("no built-in glyph for {name}"));
        assert!(symbols.get(glyph, (255, 255, 255), 32).is_some_and(|p| p.data().chunks_exact(4).any(|px| px[3] != 0)), "{glyph}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn distinct_icons_per_function() {
    let (items, _) = layout_at(&full(), &Tiles::ALL, 1.0);
    let mut icons: Vec<&str> = items
        .iter()
        .filter_map(|i| match &i.draw {
            Draw::Glyph { icon, .. } | Draw::Circle { icon, .. } if i.target.is_some() || matches!(i.draw, Draw::Glyph { .. }) => Some(*icon),
            _ => None,
        })
        .collect();
    icons.sort();
    let before = icons.len();
    icons.dedup();
    // media skip / volume / brightness etc. are all different names; the only repeat allowed is the DND bell shared by two tiles
    assert!(before - icons.len() <= 2, "{icons:?}");
}

fn render(items: &[Item], height: f32, theme: &crate::theme::model::Theme, k: f32, hover: Option<Target>) -> Pixmap {
    let mut fonts = Fonts::load().expect("native render needs an installed fallback font");
    let mut symbols = SymbolCache::default();
    let width = panel_width_at(k);
    let mut pm = Pixmap::new(width.ceil() as u32 + 40, height.ceil() as u32 + 40).unwrap();
    pm.fill(tiny_skia::Color::from_rgba8(0x80, 0x80, 0x90, 255));
    let view = View { origin: (20.0, 20.0), items, height, hover, style: Style::Gnome };
    draw_with(&mut pm, &view, &mut fonts, &mut symbols, theme, k);
    pm
}

fn px(pm: &Pixmap, x: f32, y: f32) -> [u8; 3] {
    let p = pm.pixel(x as u32, y as u32).unwrap();
    [p.red(), p.green(), p.blue()]
}

#[test]
fn painter_draws_real_pill_toggles_sliders_and_text_in_light_and_dark() {
    for (id, dark) in [("nord", true), ("macos", false)] {
        let th = theme(id, dark, 1.0);
        let _theme = crate::theme::live::thread_theme(th.clone());
        let pal = Palette::of(&th);
        let mut s = full();
        s.night_shift = Some(true);
        let (items, h) = layout_at(&s, &Tiles::ALL, 1.0);
        let pm = render(&items, h, &th, 1.0, None);
        // outside the 36 px corner the backdrop shows; inside the panel is the menu surface
        assert_eq!(px(&pm, 20.0, 20.0), [0x80, 0x80, 0x90], "{id}: rounded corner");
        let inside = px(&pm, 20.0 + 9.0, 20.0 + h - 40.0);
        assert_eq!(inside, [pal.bg.0, pal.bg.1, pal.bg.2], "{id}: panel surface");
        let centre = |t: Target| {
            let it = items.iter().find(|i| i.target == Some(t) && matches!(i.draw, Draw::Toggle { .. })).unwrap();
            (20.0 + it.r.x + it.r.w - 24.0, 20.0 + it.r.y + it.r.h / 2.0)
        };
        // checked toggle: the accent fill
        let (x, y) = centre(Target::NightShift);
        assert_eq!(px(&pm, x, y), [pal.accent.0, pal.accent.1, pal.accent.2], "{id}: checked pill");
        // Dark Style is checked while the system is dark (`light == false` in the snapshot); Do Not Disturb is off: mix(fg, bg, 9/12 %)
        let (x, y) = centre(Target::DarkMode);
        assert_eq!(px(&pm, x, y), [pal.accent.0, pal.accent.1, pal.accent.2], "{id}: dark style checked");
        let (x, y) = centre(Target::Focus);
        let off = pal.button(pal.bg);
        assert_eq!(px(&pm, x, y), [off.0, off.1, off.2], "{id}: unchecked pill");
        // the pill is round: the corner of the 176 x 48 box is still the panel colour
        let it = items.iter().find(|i| i.target == Some(Target::Bluetooth)).unwrap();
        assert_eq!(px(&pm, 20.0 + it.r.x + 1.0, 20.0 + it.r.y + 1.0), [pal.bg.0, pal.bg.1, pal.bg.2], "{id}: pill corner");
        // slider: accent fill left of the handle, trough to the right
        let v = items.iter().find(|i| i.target == Some(Target::Brightness) && matches!(i.draw, Draw::Slider { .. })).unwrap();
        assert_eq!(px(&pm, 20.0 + v.r.x + 20.0, 20.0 + v.r.y + v.r.h / 2.0), [pal.accent.0, pal.accent.1, pal.accent.2], "{id}: slider fill");
        let trough = px(&pm, 20.0 + v.r.x + v.r.w - 10.0, 20.0 + v.r.y + v.r.h / 2.0);
        assert_ne!(trough, [pal.accent.0, pal.accent.1, pal.accent.2]);
        // text and glyph ink exist
        let ink = pm.data().chunks_exact(4).filter(|p| p[3] == 255 && (p[0] as i32 - pal.fg.0 as i32).abs() < 40 && (p[1] as i32 - pal.fg.1 as i32).abs() < 40).count();
        assert!(ink > 300, "{id}: foreground ink {ink}");
        if let Some(dir) = std::env::var_os("GNOME_CC_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            pm.save_png(std::path::Path::new(&dir).join(format!("{id}-{}-100.png", if dark { "dark" } else { "light" }))).unwrap();
        }
    }
}

#[test]
fn hover_and_unavailable_states_change_pixels() {
    let th = theme("macos", false, 1.0);
    let _theme = crate::theme::live::thread_theme(th.clone());
    let mut s = full();
    s.dnd = None;
    let (items, h) = layout_at(&s, &Tiles::ALL, 1.0);
    let plain = render(&items, h, &th, 1.0, None);
    let hovered = render(&items, h, &th, 1.0, Some(Target::Bluetooth));
    let b = items.iter().find(|i| i.target == Some(Target::Bluetooth) && matches!(i.draw, Draw::Toggle { .. })).unwrap();
    let (x, y) = (20.0 + b.r.x + b.r.w - 24.0, 20.0 + b.r.y + b.r.h / 2.0);
    assert_ne!(px(&plain, x, y), px(&hovered, x, y), "hover changes the toggle");
    // an unavailable provider is drawn insensitive and cannot be hovered
    let dnd = items.iter().find(|i| matches!(&i.draw, Draw::Toggle { .. }) && i.target.is_none()).unwrap();
    let (x, y) = (20.0 + dnd.r.x + dnd.r.w - 24.0, 20.0 + dnd.r.y + dnd.r.h / 2.0);
    assert_ne!(px(&plain, x, y), [0x80, 0x80, 0x90]);
    assert_eq!(px(&plain, x, y), px(&render(&items, h, &th, 1.0, Some(Target::Focus)), x, y), "no target, no hover");
}

#[test]
fn long_labels_stay_inside_their_pill_at_200_percent() {
    let fonts = Fonts::load().expect("test fonts");
    for k in [1.0_f32, 2.0] {
        let th = theme("nord", true, k);
        let _theme = crate::theme::live::thread_theme(th);
        let mut s = full();
        s.net = Some(sys::Net { wifi_enabled: true, wifi_name: "A very long wireless network name 日本語 that must be ellipsised".into(), wired_name: String::new() });
        s.media = Some(sys::Media { playing: false, title: "An extremely long track title that cannot fit the card".into(), artist: "An equally long artist name — feat. everybody".into() });
        let (items, _) = layout_at(&s, &Tiles::ALL, k);
        for i in &items {
            if let Draw::Text { text, size, weight, .. } = &i.draw {
                // `Fonts` multiplies by the process-wide text scale (1.0 here), so emulate scale `k` with the size
                let px = *size * k;
                let shown = fit(text, i.r.w, px, *weight, &fonts);
                assert!(fonts.measure(&shown, px, *weight) <= i.r.w + 0.01, "{k}: {text:?} in {}", i.r.w);
                if text.len() > 30 {
                    assert!(shown.ends_with('…'), "{k}: {text:?} -> {shown:?}");
                }
            }
        }
    }
}

#[test]
fn evidence_matrix_renders_every_scale_and_mode() {
    let Some(dir) = std::env::var_os("GNOME_CC_EVIDENCE") else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let mut report = Vec::new();
    for (id, dark) in [("nord", true), ("macos", false)] {
        for k in [1.0_f32, 1.25, 2.0] {
            let th = theme(id, dark, k);
            let _theme = crate::theme::live::thread_theme(th.clone());
            for (label, tiles) in [("all", Tiles::ALL), ("sliders", Tiles::from_names(["display", "sound"])), ("grid", Tiles::from_names(["connectivity", "focus", "toggles", "actions"]))] {
                let (items, h) = layout_at(&full(), &tiles, k);
                let pm = render(&items, h, &th, k, Some(Target::Wifi));
                let name = format!("{}-{}-{}-{label}.png", id, if dark { "dark" } else { "light" }, (k * 100.0) as u32);
                pm.save_png(std::path::Path::new(&dir).join(&name)).unwrap();
                report.push(format!("{name} {}x{}", pm.width(), pm.height()));
            }
        }
    }
    std::fs::write(std::path::Path::new(&dir).join("matrix.txt"), report.join("\n")).unwrap();
}

#[test]
fn keyboard_focus_is_a_distinct_accent_ring_around_the_control_not_a_hover_fill() {
    let th = theme("nord", true, 1.0);
    let _theme = crate::theme::live::thread_theme(th.clone());
    let (items, h) = layout_at(&full(), &Tiles::ALL, 1.0);
    let mut fonts = Fonts::load().expect("native render needs an installed fallback font");
    let mut symbols = SymbolCache::default();
    let accent = th.accent.t();
    let ring_around = |pm: &Pixmap, r: R| {
        let mut count = 0;
        for y in (r.y + 20.0 - 4.0) as u32..(r.y + 20.0 + r.h + 4.0) as u32 {
            for x in (r.x + 20.0 - 4.0) as u32..(r.x + 20.0 + r.w + 4.0) as u32 {
                let (fx, fy) = (x as f32, y as f32);
                let inside = fx >= r.x + 19.0 && fx < r.x + 20.0 + r.w + 1.0 && fy >= r.y + 19.0 && fy < r.y + 20.0 + r.h + 1.0;
                let p = pm.pixel(x, y).unwrap();
                if !inside && (p.red() as i32 - accent.0 as i32).abs() + (p.green() as i32 - accent.1 as i32).abs() + (p.blue() as i32 - accent.2 as i32).abs() < 30 {
                    count += 1;
                }
            }
        }
        count
    };
    let mut paint = |focus: Option<Target>, hover: Option<Target>| {
        let mut pm = Pixmap::new(panel_width_at(1.0) as u32 + 40, h as u32 + 40).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(0x80, 0x80, 0x90, 255));
        let view = View { origin: (20.0, 20.0), items: &items, height: h, hover, style: Style::Gnome };
        // draw_focused goes through the style dispatch the hosts use
        draw_focused(&mut pm, &view, focus, &mut fonts, &mut symbols);
        pm
    };
    for target in [Target::Wifi, Target::NightShift, Target::Volume, Target::Mute, Target::MediaNext, Target::Power] {
        let r = items.iter().find(|i| i.target == Some(target)).unwrap().r;
        let focused = ring_around(&paint(Some(target), None), r);
        let plain = ring_around(&paint(None, None), r); // neighbouring accent-filled toggles are accent-coloured too
        let hovered = ring_around(&paint(None, Some(target)), r);
        assert!(focused > plain + 40, "{target:?}: ring pixels {focused} vs {plain} without focus");
        // hover alone never draws the ring; the ring stands off the control so it shows on accent-filled toggles too
        assert!(hovered < plain + 10, "{target:?}: hover is not focus ({hovered} vs {plain})");
    }
    // a control that is not in the display list (scrolled out of the viewport) gets no ring and nothing breaks
    let _ = paint(Some(Target::Brightness), None);
    let partial: Vec<Item> = items.iter().filter(|i| i.target != Some(Target::MediaNext)).cloned().collect();
    let view = View { origin: (20.0, 20.0), items: &partial, height: h, hover: None, style: Style::Gnome };
    let mut pm = Pixmap::new(panel_width_at(1.0) as u32 + 40, h as u32 + 40).unwrap();
    draw_focused(&mut pm, &view, Some(Target::MediaNext), &mut fonts, &mut symbols);
    // every look clips and scrolls in a short host (card and fluent too, since the responsive viewport)
    assert!(Style::Gnome.scrolls() && Style::Dux.scrolls() && Style::Card.scrolls() && Style::Fluent.scrolls() && Style::Auto.scrolls());
    assert_eq!(targets_in_order(&items).len(), 22);
}

// ------------------------------------------------------------------ source reference: gnome-shell 51.1 compiled by sassc
//
// tests/fixtures/gnome-theme-ports/gnome-shell-51.1-quick-settings-reference.json holds the numbers of the pinned stylesheet as compiled by the
// compiler upstream uses (`tests/e2e/gnome-shell-reference.py` builds it; its provenance is inside). These tests compare the implementation with it
// and list every difference that is intended, so nothing here is a "pixel replica" claim: no GNOME Shell runtime is available to diff against.

fn reference() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gnome-theme-ports/gnome-shell-51.1-quick-settings-reference.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// One CSS length of the reference in px at 100 % (the first value of a shorthand unless `index` picks another).
fn ref_len(reference: &serde_json::Value, selector: &str, property: &str, index: Option<usize>) -> f32 {
    let v = &reference["lengths"][selector][property]["px"];
    let v = match index {
        Some(i) => &v[i],
        None if v.is_array() => &v[0],
        None => v,
    };
    v.as_f64().unwrap_or_else(|| panic!("{selector} {property}: {v}")) as f32
}

#[test]
fn lengths_match_the_compiled_gnome_shell_stylesheet() {
    let r = reference();
    assert_eq!(r["provenance"]["gnome_shell_commit"], "33da06854e229c256afd5872fedd83c28cbfa176");
    assert!(r["provenance"]["compiler"].as_str().unwrap().contains("sassc"));
    // (selector, property, shorthand index, the implementation's value at 100 %, tolerance in px): all `em` lengths are converted at 1em = 11 pt = 14.667 px
    let rows: Vec<(&str, &str, Option<usize>, f32, f32)> = vec![
        (".quick-settings", "padding", None, PANEL_PAD, 0.01),
        (".quick-settings", "border-radius", None, PANEL_RADIUS, 0.01),
        (".quick-settings-grid", "spacing-rows", None, GRID_GAP, 0.01),
        (".quick-settings-grid", "spacing-columns", None, GRID_GAP, 0.01),
        (".quick-toggle", "min-width", None, toggle_w(1.0), 0.01),
        (".quick-toggle", "max-width", None, toggle_w(1.0), 0.01),
        (".quick-toggle", "min-height", None, toggle_h(1.0), 0.02),
        (".quick-toggle .quick-toggle-icon", "icon-size", None, icon_px(1.0), 0.01),
        (".quick-toggle .quick-toggle-title", "font-size", None, HEADING_PX, 0.01),
        (".quick-toggle .quick-toggle-subtitle", "font-size", None, CAPTION_PX, 0.01),
        (".quick-toggle > StBoxLayout", "spacing", None, TOGGLE_SPACING, 0.01),
        (".quick-toggle > StBoxLayout", "padding", Some(1), TOGGLE_PAD_RIGHT, 0.01),
        (".quick-toggle:ltr > StBoxLayout", "padding-left", None, TOGGLE_PAD_LEFT, 0.01),
        (".quick-toggle-has-menu .quick-toggle:ltr > StBoxLayout", "padding-right", None, SPLIT_TOGGLE_PAD_RIGHT, 0.01),
        (".quick-toggle-has-menu .quick-toggle-menu-button", "padding", Some(1), MENU_BUTTON_PAD, 0.01),
        (".quick-toggle-has-menu .quick-toggle-separator", "width", None, 1.0, 0.0),
        (".quick-settings .icon-button", "padding", None, ICON_BUTTON_PAD, 0.01),
        (".quick-slider .icon-button", "padding", None, SLIDER_PAD, 0.01),
        (".quick-slider .slider-bin", "padding", None, SLIDER_PAD, 0.01),
        (".quick-slider > StBoxLayout", "spacing", None, 6.0, 0.0),
        (".quick-settings-system-item > StBoxLayout", "spacing", None, GRID_GAP, 0.01),
        (".slider", "-barlevel-height", None, SLIDER_TROUGH, 0.0),
        (".slider", "-slider-handle-radius", None, icon_px(1.0) / 2.0, 0.01),
        (".message", "border-radius", None, 16.0, 0.01),
    ];
    for (selector, property, index, mine, tolerance) in rows {
        let want = ref_len(&r, selector, property, index);
        assert!((want - mine).abs() <= tolerance + 1e-4, "{selector} {property}: gnome-shell {want} px, implementation {mine} px");
    }
    // the em-scaled lengths follow the text scale in both (the toggle is 12em wide, its menu button padding 0.71575em)
    for k in [1.25_f32, 2.0] {
        assert!((toggle_w(k) - 12.0 * 14.6667 * k).abs() < 0.2 * k);
        let (items, _) = layout_at(&full(), &Tiles::ALL, k);
        let menu = items.iter().find(|i| matches!(i.draw, Draw::Toggle { caps: Caps::Right, .. })).unwrap();
        assert!((menu.r.w - (16.0 + 2.0 * 10.4977) * k).abs() < 0.3 * k, "menu button width at {k}: {}", menu.r.w);
    }
}

/// Differences kept on purpose (or forced by the token model), each with its reason: the reference is the Shell, not a target to copy blindly.
const KNOWN_DIFFERENCES: &[(&str, &str)] = &[
    ("rounding", "St truncates the byte of every mixed colour, Decebal rounds: channels may differ by 1/255"),
    ("light panel background", "the Shell's light $bg_color is #fafafb; the gnome-adwaita light `popup` token is libadwaita's popover white #ffffff, and the look uses the theme's popup colour"),
    ("focus ring", "the Shell draws an inset 2 px ring in a lightened accent at 80 % alpha; Decebal draws a 2 px accent ring standing 3 px off the control, which stays visible on accent-filled toggles and on the panel"),
    ("pressed state", "the pointer protocol gives the panel no pressed state, so `:active` colours are not drawn"),
    ("message card padding", "the card is laid out with a fixed 12 px inset (message 6 + message-box 6)"),
];

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn near(a: &str, b: Rgb, tolerance: i32) -> bool {
    let a = Rgb::parse(a).unwrap();
    [(a.0, b.0), (a.1, b.1), (a.2, b.2)].iter().all(|(x, y)| (*x as i32 - *y as i32).abs() <= tolerance)
}

#[test]
fn colour_formulas_match_the_shell_colours_evaluated_with_the_st_algorithms() {
    let r = reference();
    assert_eq!(KNOWN_DIFFERENCES.len(), 5);
    let accent = Rgb(0x35, 0x84, 0xe4);
    // the Shell's own inputs: dark `$bg_color` #36363a / `$fg_color` white, light `#fafafb` / `#222226`
    let variants = [
        ("dark", Palette { dark: true, bg: Rgb(0x36, 0x36, 0x3a), fg: Rgb(255, 255, 255), accent, accent_fg: Rgb(255, 255, 255) }),
        ("light", Palette { dark: false, bg: Rgb(0xfa, 0xfa, 0xfb), fg: Rgb(0x22, 0x22, 0x26), accent, accent_fg: Rgb(255, 255, 255) }),
    ];
    for (name, pal) in variants {
        let c = &r["colours"][name];
        let get = |key: &str| c[key].as_str().unwrap_or_else(|| panic!("{name}: reference has no {key}"));
        let checks: Vec<(&str, Rgb)> = vec![
            ("button/normal/background-color", pal.button(pal.bg)),
            ("button/hover/background-color", pal.button_hover(pal.bg)),
            ("button/insensitive/background-color", pal.button_insensitive(pal.bg)),
            ("toggle/checked/background-color", pal.default_button(false)),
            ("toggle/checked-hover/background-color", pal.default_button(true)),
            ("menu-button/normal/background-color", pal.button(pal.menu_base())),
            ("menu-button/checked/background-color", pal.menu_checked(false)),
            ("message-card/normal/background-color", pal.card()),
        ];
        for (key, mine) in checks {
            assert!(near(get(key), mine, 2), "{name} {key}: gnome-shell {} vs implementation {}", get(key), hex(mine));
        }
        // text colours and the panel surface
        assert_eq!(get("button/normal/color"), hex(pal.fg), "{name}");
        assert_eq!(r["colour_expressions"][name]["panel/background-color"], hex(pal.bg), "{name}");
        // the panel border: dark `lighten(bg, 5 %)`, light `darken(bg, 7 %)` as `1px solid <colour>`
        let border = r["colour_expressions"][name]["panel/border"].as_str().unwrap();
        assert!(border.starts_with("1px solid ") && near(&border["1px solid ".len()..], pal.outer_border(), 2), "{name}: {border} vs {}", hex(pal.outer_border()));
        // the separators: unchecked is 25 % foreground, checked `st-mix(accent_fg, accent, 30 %)` (dark; the light variant is documented in the fixture)
        assert!(r["colour_expressions"][name]["separator"].as_str().is_some());
    }
    // the focus ring is a 2 px ring in both (the offset differs, see KNOWN_DIFFERENCES)
    assert!(r["colour_expressions"]["dark"]["focus/box-shadow"].as_str().unwrap().starts_with("inset 0 0 0 2px"));
    // the light variant of the checked-menu-button base differs from the dark by design: 92 % vs 85 % accent
    assert!(near(r["colours"]["light"]["menu-button/checked/background-color"].as_str().unwrap(), variants_light_menu_checked(), 2));
}

fn variants_light_menu_checked() -> Rgb {
    Palette { dark: false, bg: Rgb(0xfa, 0xfa, 0xfb), fg: Rgb(0x22, 0x22, 0x26), accent: Rgb(0x35, 0x84, 0xe4), accent_fg: Rgb(255, 255, 255) }.menu_checked(false)
}
