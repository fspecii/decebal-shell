//! The Kickoff footer and Places tab: geometry against the identified original capture, and every control by pointer and keyboard.

use super::super::tests::{fixture_apps, styled};
use super::system::{Sys, RAN};
use super::*;
use crate::app::Button;
use crate::fuzzy::History;
use crate::launcher::catalog::Catalog;
use crate::launcher::style::LauncherStyle;
use crate::window_app::{Action, Key, KeyEvent};

fn kde(n: usize) -> super::super::Styled {
    let mut st = styled(LauncherStyle::Kde, n);
    st.full = false;
    st.scale = 1.0;
    st.m.ui.insert("kde.hibernate", 1);
    // the size of Kickoff in the original capture (655 x 516)
    st.resize(655, 516);
    RAN.with(|r| r.borrow_mut().clear());
    OPENED.with(|o| o.borrow_mut().clear());
    st
}

fn key(k: Key) -> KeyEvent {
    KeyEvent { key: k, text: String::new(), ch: None, ctrl: false, shift: false, alt: false }
}

fn zone<'a>(st: &'a super::super::Styled, id: &str) -> &'a Zone {
    st.zones.iter().find(|z| z.id == id).unwrap_or_else(|| panic!("no zone {id}: {:?}", st.zones.iter().map(|z| z.id).collect::<Vec<_>>()))
}

fn centre(st: &super::super::Styled, id: &str, target: Target) -> (f32, f32) {
    let z = zone(st, id);
    let i = z.items.iter().position(|t| *t == target).unwrap_or_else(|| panic!("{id} has no {target:?}: {:?}", z.items));
    let r = z.rect(i, st.m.offset(id)).unwrap();
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn click(st: &mut super::super::Styled, id: &str, target: Target) -> crate::window_app::Response {
    let (x, y) = centre(st, id, target);
    st.button(x, y, Button::Left, true)
}

fn ran() -> Vec<Sys> {
    RAN.with(|r| r.borrow().clone())
}

fn closes(r: &crate::window_app::Response) -> bool {
    r.action == Some(Action::Close)
}

#[test]
fn the_footer_matches_the_original_capture() {
    let st = kde(30);
    let footer = zone(&st, "footer");
    // tabs, then Sleep, Hibernate, Restart, Shut Down, then the Session button, left to right on one row
    let ids: Vec<_> = footer
        .items
        .iter()
        .map(|t| match t {
            Target::Style(k, i) => (*k, *i),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(ids, [(TAB, 0), (TAB, 1), (SYS, 4), (SYS, 5), (SYS, 6), (SYS, 7), (SESSION, 0)]);
    let rects: Vec<_> = (0..ids.len()).map(|i| footer.rect(i, 0).unwrap()).collect();
    assert!(rects.windows(2).all(|w| w[0].right() <= w[1].x + 0.5), "no overlap, left to right");
    // original (1920x1080 capture, card at x=8, y=504, 655 x 516), logical px relative to the card: the first tab 7..114, the Places tab 120..227,
    // Sleep's button from 256, Shut Down's button ending at 613, the Session button 621..651
    let near = |a: f32, b: f32| (a - b).abs() <= 2.5;
    assert!(near(rects[0].x, 7.0) && near(rects[0].w, 107.0), "{:?}", rects[0]);
    assert!(near(rects[1].x, 120.0), "{:?}", rects[1]);
    assert!(near(rects[2].x, 256.0), "Sleep {:?}", rects[2]);
    assert!(near(rects[5].right(), 613.0), "Shut Down {:?}", rects[5]);
    assert!(near(rects[6].x, 621.0) && near(rects[6].w, 30.0), "Session {:?}", rects[6]);
    // the footer is 45 px high and ends at the card's bottom edge
    let f = frame(st.card, 1.0).footer;
    assert!(near(f.h, 45.0) && near(f.bottom(), 516.0));
}

#[test]
fn power_buttons_confirm_leaving_actions_and_act_at_once_for_sleep() {
    let mut st = kde(30);
    let r = click(&mut st, "footer", Target::Style(SYS, 4));
    assert!(closes(&r), "Sleep closes the launcher");
    assert_eq!(ran(), [Sys::Suspend]);
    // Restart: the first press only arms it
    let r = click(&mut st, "footer", Target::Style(SYS, 6));
    assert!(!closes(&r) && ran() == [Sys::Suspend], "armed, nothing run");
    assert_eq!(armed(&st.m), Some(Sys::Reboot));
    // Escape takes the confirmation back and the launcher stays open
    let r = st.key(&key(Key::Escape));
    assert!(!closes(&r) && armed(&st.m).is_none());
    // a press elsewhere disarms too
    click(&mut st, "footer", Target::Style(SYS, 7));
    assert_eq!(armed(&st.m), Some(Sys::Shutdown));
    click(&mut st, "footer", Target::Style(TAB, 1));
    assert!(armed(&st.m).is_none() && ran() == [Sys::Suspend], "switching tab drops it");
    // armed, then the second press acts
    click(&mut st, "footer", Target::Style(SYS, 7));
    let r = click(&mut st, "footer", Target::Style(SYS, 7));
    assert!(closes(&r));
    assert_eq!(ran(), [Sys::Suspend, Sys::Shutdown]);
}

#[test]
fn hibernate_is_offered_only_when_the_machine_can_do_it() {
    let mut st = kde(30);
    assert!(zone(&st, "footer").items.contains(&Target::Style(SYS, 5)));
    st.m.ui.insert("kde.hibernate", 0);
    st.relayout();
    assert!(!zone(&st, "footer").items.contains(&Target::Style(SYS, 5)), "Plasma hides what it cannot do");
}

#[test]
fn the_session_menu_holds_the_other_actions_and_closes_before_the_launcher_does() {
    let mut st = kde(30);
    assert!(st.zones.iter().all(|z| z.id != "menu"));
    click(&mut st, "footer", Target::Style(SESSION, 0));
    let menu = zone(&st, "menu");
    assert_eq!(menu.items, [Target::Style(MENU, 0), Target::Style(MENU, 1)], "Lock and Log Out (Save Session and Switch User cannot be offered here)");
    assert_eq!(st.m.zone, "menu", "the keyboard focus moves into the menu");
    // the popup lies above the Session button and inside the card
    let (b, row) = (zone(&st, "footer").rect(6, 0).unwrap(), menu.rect(0, 0).unwrap());
    assert!(row.bottom() <= b.y && row.x >= 0.0 && row.right() <= st.card.right());
    // the pointer finds the menu before the lists it covers
    let (x, y) = centre(&st, "menu", Target::Style(MENU, 0));
    assert_eq!(st.hit(x, y).map(|h| h.0), Some("menu"));
    // Escape closes the menu first
    let r = st.key(&key(Key::Escape));
    assert!(!closes(&r) && st.zones.iter().all(|z| z.id != "menu") && st.m.zone == "footer");
    // a press outside only closes it
    click(&mut st, "footer", Target::Style(SESSION, 0));
    let r = st.button(300.0, 200.0, Button::Left, true);
    assert!(!closes(&r) && st.zones.iter().all(|z| z.id != "menu") && ran().is_empty(), "the press that closes a menu does nothing else");
    // Enter on Lock runs it
    click(&mut st, "footer", Target::Style(SESSION, 0));
    let r = st.key(&key(Key::Enter));
    assert!(closes(&r));
    assert_eq!(ran(), [Sys::Lock]);
    // Log Out asks twice, from the menu too
    let mut st = kde(30);
    click(&mut st, "footer", Target::Style(SESSION, 0));
    let r = click(&mut st, "menu", Target::Style(MENU, 1));
    assert!(!closes(&r) && ran().is_empty() && armed(&st.m) == Some(Sys::Logout));
    assert!(closes(&click(&mut st, "menu", Target::Style(MENU, 1))));
    assert_eq!(ran(), [Sys::Logout]);
}

#[test]
fn a_narrow_card_moves_every_action_behind_the_session_button() {
    let mut st = kde(30);
    st.resize(420, 516);
    let footer = zone(&st, "footer");
    assert!(footer.items.iter().all(|t| !matches!(t, Target::Style(SYS, _))), "no buttons: {:?}", footer.items);
    click(&mut st, "footer", Target::Style(SESSION, 0));
    let menu = zone(&st, "menu");
    assert_eq!(menu.items.len(), 6, "Lock, Log Out, Sleep, Hibernate, Restart, Shut Down");
    for i in 0..menu.items.len() {
        let r = menu.rect(i, 0).unwrap();
        assert!(r.x >= 0.0 && r.y >= 0.0 && r.right() <= st.card.right() && r.bottom() <= st.card.bottom(), "row {i} {r:?}");
    }
    let r = click(&mut st, "menu", Target::Style(MENU, 4));
    assert!(closes(&r) && ran() == [Sys::Suspend]);
}

#[test]
fn the_places_tab_lists_computer_history_and_frequently_used_and_opens_them() {
    let mut st = kde(12);
    st.m.entries.insert(
        "computer",
        vec![
            Entry { name: "Show KRunner".into(), detail: "Search, calculate, or run a command".into(), icon: "krunner".into(), open: vec!["decspot".into()], at: 0 },
            Entry { name: "Home".into(), detail: "/home/t".into(), icon: "user-home".into(), open: vec!["xdg-open".into(), "/home/t".into()], at: 0 },
        ],
    );
    st.m.entries.insert(
        "documents",
        vec![Entry { name: "notes.txt".into(), detail: "~/Documents".into(), icon: "text-plain".into(), open: vec!["xdg-open".into(), "/home/t/Documents/notes.txt".into()], at: 2000 }],
    );
    assert!(zone(&st, "cats").items.contains(&Target::View(View::Favorites)), "Applications tab first");
    let r = click(&mut st, "footer", Target::Style(TAB, 1));
    assert!(!closes(&r) && tab(&st.m) == 1);
    assert_eq!(zone(&st, "cats").items, [Target::Style(PLACE, 0), Target::Style(PLACE, 1), Target::Style(PLACE, 2)]);
    // Computer: Show KRunner, then the places
    assert_eq!(zone(&st, "apps").items, [Target::Style(COMPUTER_ROW, 0), Target::Style(COMPUTER_ROW, 1)]);
    let r = click(&mut st, "apps", Target::Style(COMPUTER_ROW, 1));
    assert!(closes(&r));
    assert_eq!(OPENED.with(|o| o.borrow().clone()), [vec!["xdg-open".to_string(), "/home/t".to_string()]]);
    // History: the document and the apps launched from here, newest first
    let mut st = kde(12);
    st.m.entries.insert("documents", vec![Entry { name: "notes.txt".into(), detail: "~/Documents".into(), icon: "text-plain".into(), open: vec!["xdg-open".into(), "/n".into()], at: 2000 }]);
    let apps = {
        let mut a = fixture_apps(12);
        a.sort_by_key(|e| e.name.to_lowercase());
        a
    };
    let id = |i: usize| format!("appid:{}", crate::launcher::catalog::app_id(&apps[i]));
    let history = History::parse(&format!("5\t1000\t{}\n1\t3000\t{}\n9\t500\t{}\n", id(1), id(2), id(3)));
    st.m.cat = Catalog::with(apps.clone(), Some(vec![]), history);
    click(&mut st, "footer", Target::Style(TAB, 1));
    click(&mut st, "cats", Target::Style(PLACE, 1));
    assert_eq!(place(&st.m), 1);
    assert_eq!(zone(&st, "apps").items, [Target::App(2), Target::Style(DOCUMENT_ROW, 0), Target::App(1), Target::App(3)], "by last use: 3000, 2000, 1000, 500");
    // Frequently Used: by count
    click(&mut st, "cats", Target::Style(PLACE, 2));
    assert_eq!(zone(&st, "apps").items, [Target::App(3), Target::App(1), Target::App(2)], "9, 5, 1 launches");
    // back to Applications
    click(&mut st, "footer", Target::Style(TAB, 0));
    assert!(zone(&st, "cats").items.contains(&Target::View(View::Favorites)) && tab(&st.m) == 0);
}

#[test]
fn the_keyboard_walks_tabs_buttons_and_the_session_menu() {
    let mut st = kde(30);
    st.m.zone = "footer";
    st.m.sel.insert("footer", 0);
    // Right moves through the tabs and on to the buttons
    for _ in 0..2 {
        st.key(&key(Key::Right));
    }
    assert_eq!(st.current(), Some(Target::Style(SYS, 4)), "Sleep after the two tabs");
    st.key(&key(Key::Left));
    assert_eq!(st.current(), Some(Target::Style(TAB, 1)));
    // Enter on the Places tab opens it
    st.key(&key(Key::Enter));
    assert_eq!(tab(&st.m), 1);
    // End reaches the Session button, Enter opens the menu with the focus in it, Down moves, Escape returns
    st.key(&key(Key::End));
    assert_eq!(st.current(), Some(Target::Style(SESSION, 0)));
    st.key(&key(Key::Enter));
    assert_eq!((st.m.zone, st.current()), ("menu", Some(Target::Style(MENU, 0))));
    st.key(&key(Key::Down));
    assert_eq!(st.current(), Some(Target::Style(MENU, 1)));
    st.key(&key(Key::Escape));
    assert_eq!(st.current(), Some(Target::Style(SESSION, 0)));
}

#[test]
fn every_footer_control_stays_inside_the_card_at_every_scale_and_small_card() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for (w, h) in [(655.0, 516.0), (500.0, 400.0), (360.0, 300.0)] {
            let mut st = styled(LauncherStyle::Kde, 20);
            st.full = false;
            st.scale = scale;
            st.resize((w * scale) as u32, (h * scale) as u32);
            st.m.ui.insert("kde.menu", 1);
            st.relayout();
            for z in &st.zones {
                for i in z.visible(0) {
                    let r = z.rect(i, 0).unwrap();
                    assert!(
                        r.x >= -0.5 && r.y >= -0.5 && r.right() <= st.card.right() + 0.5 && r.bottom() <= st.card.bottom() + 0.5,
                        "{} item {i} {r:?} leaves {:?} at {scale} {w}x{h}",
                        z.id,
                        st.card
                    );
                }
            }
        }
    }
}

/// Evidence tool, not a gate: renders the native card at the original capture's size, scale and mode (655 x 516 at 100 %, Breeze Dark and Breeze,
/// the colours of `themes/plasma-breeze`) into `$DECEBAL_KICKOFF_CAPTURE_DIR`, for the original-versus-native comparison in
/// docs/plasma-theme-ports/kickoff-fidelity.md. `cargo test --lib kickoff_capture_dark -- --ignored` (and `_light`, a separate process each because the icon choice is global).
#[test]
#[ignore = "writes PNGs for the comparison into /var/tmp/decebal-kickoff-capture (or $DECEBAL_KICKOFF_CAPTURE_DIR)"]
fn kickoff_capture_dark() {
    capture(true);
}

#[test]
#[ignore = "writes PNGs for the comparison into /var/tmp/decebal-kickoff-capture (or $DECEBAL_KICKOFF_CAPTURE_DIR)"]
fn kickoff_capture_light() {
    capture(false);
}

fn capture(dark: bool) {
    use crate::apps::AppEntry;
    use crate::theme::model::Selection;
    // the clean-environment wrapper drops variables, so the default is a fixed private folder
    let dir = std::env::var_os("DECEBAL_KICKOFF_CAPTURE_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| "/var/tmp/decebal-kickoff-capture".into());
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/themes/plasma-breeze/theme.toml")).unwrap();
    let def = crate::theme::engine::manifest::analyze(&text).def.expect("the Breeze package parses");
    let app = |name: &str, icon: &str, cats: &str| AppEntry {
        name: name.into(),
        exec: "true".into(),
        icon: icon.into(),
        categories: cats.into(),
        file: format!("/fixture/{}.desktop", name.to_lowercase().replace(' ', "")),
        ..Default::default()
    };
    let apps = {
        let mut a = vec![
            app("System Settings", "preferences-system", "Settings;System;"),
            app("Dolphin", "system-file-manager", "System;FileManager;"),
            app("Discover", "plasmadiscover", "System;PackageManager;"),
            app("Kate", "kate", "Utility;TextEditor;"),
            app("Okular", "okular", "Office;Viewer;"),
            app("Krita", "krita", "Graphics;"),
            app("Kdevelop", "kdevelop", "Development;IDE;"),
            app("Kalzium", "kalzium", "Education;"),
            app("Kmahjongg", "kmahjongg", "Game;"),
            app("Firefox", "firefox", "Network;WebBrowser;"),
            app("VLC", "vlc", "AudioVideo;Player;"),
        ];
        a.sort_by_key(|e| e.name.to_lowercase());
        a
    };
    let pins: Vec<String> = ["systemsettings", "dolphin", "discover"].iter().map(|s| s.to_string()).collect();
    struct Restore(std::sync::Arc<crate::theme::Theme>);
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::theme::live::publish_theme((*self.0).clone());
        }
    }
    let _restore = Restore(crate::theme::th());
    std::fs::create_dir_all(&dir).unwrap();
    // the icon theme the original uses for this mode; unit tests read the icon choice from this process's own config folder
    let config = std::env::temp_dir().join(format!("decebal-test-config-{}", std::process::id()));
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("decebal.json"), format!(r#"{{"icons":{{"theme":"{}","fallback":"breeze"}}}}"#, if dark { "breeze-dark" } else { "breeze" })).unwrap();
    for mode in [if dark { "dark" } else { "light" }] {
        crate::theme::live::publish_theme(def.resolve("plasma-breeze", dark, &Selection::default()));
        for (tab, name) in [(0, "applications"), (1, "places")] {
            let mut st = super::super::tests::styled(LauncherStyle::Kde, 0);
            st.m.cat = Catalog::with(apps.clone(), Some(pins.clone()), History::default());
            st.fonts = crate::text::Fonts::load();
            st.m.user = "Konqi".into();
            st.m.ui.insert("kde.hibernate", 1);
            st.m.ui.insert("kde.tab", tab);
            st.full = false;
            st.scale = 1.0;
            st.resize(655, 516);
            // the Computer list as Kickoff builds it on a machine with this home
            if tab == 1 {
                if let Some(c) = st.m.entries.get_mut("computer") {
                    c.truncate(3);
                }
            }
            let mut pm = tiny_skia::Pixmap::new(655, 516).unwrap();
            st.draw(&mut pm);
            pm.save_png(dir.join(format!("native-{mode}-{name}.png"))).unwrap();
        }
    }
}
