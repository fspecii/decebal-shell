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
    disarm();
    places::reset();
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
    places::install(places::Loaded {
        computer: vec![Entry { name: "Home".into(), detail: "/home/t".into(), icon: "user-home".into(), open: vec!["xdg-open".into(), "/home/t".into()], at: 0 }],
        documents: vec![Entry { name: "notes.txt".into(), detail: "~/Documents".into(), icon: "text-plain".into(), open: vec!["xdg-open".into(), "/home/t/Documents/notes.txt".into()], at: 2000 }],
    });
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
    places::install(places::Loaded {
        computer: Vec::new(),
        documents: vec![Entry { name: "notes.txt".into(), detail: "~/Documents".into(), icon: "text-plain".into(), open: vec!["xdg-open".into(), "/n".into()], at: 2000 }],
    });
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
                places::install(places::Loaded {
                    computer: vec![
                        Entry { name: "Home".into(), detail: "/home/konqi".into(), icon: "user-home".into(), open: vec!["xdg-open".into(), "/home/konqi".into()], at: 0 },
                        Entry {
                            name: "Documents".into(),
                            detail: "/home/konqi/Documents".into(),
                            icon: "folder-documents".into(),
                            open: vec!["xdg-open".into(), "/home/konqi/Documents".into()],
                            at: 0,
                        },
                    ],
                    documents: Vec::new(),
                });
            }
            let mut pm = tiny_skia::Pixmap::new(655, 516).unwrap();
            st.draw(&mut pm);
            pm.save_png(dir.join(format!("native-{mode}-{name}.png"))).unwrap();
        }
    }
}

// ---------------------------------------------------------------------------------------------- physical small screens at high display scales

use crate::launcher::placement::{Anchor, Edge};

fn on_edge(edge: Edge, screen: (u32, u32), t: f32) -> Anchor {
    let (w, h) = (screen.0 as f32, screen.1 as f32);
    let point = match edge {
        Edge::Top => (w * 0.1, t / 2.0),
        Edge::Bottom => (w * 0.1, h - t / 2.0),
        Edge::Left => (t / 2.0, h * 0.9),
        Edge::Right => (w - t / 2.0, h * 0.9),
    };
    Anchor { edge, point: Some(point), thickness: t }
}

/// A launcher opened on a PHYSICAL screen of this size at this display scale (the card is placed and shrunk by `placement`, not scaled from a logical size).
fn physical(screen: (u32, u32), scale: f32, edge: Edge) -> super::super::Styled {
    let mut st = styled(LauncherStyle::Kde, 40);
    st.full = true;
    st.scale = scale;
    st.anchor = Some(on_edge(edge, screen, 32.0 * scale));
    st.m.ui.insert("kde.hibernate", 1);
    st.resize(screen.0, screen.1);
    st
}

fn inside_card(st: &super::super::Styled, r: &Rect) -> bool {
    r.x >= st.card.x - 0.5 && r.y >= st.card.y - 0.5 && r.right() <= st.card.right() + 0.5 && r.bottom() <= st.card.bottom() + 0.5
}

#[test]
fn on_small_physical_screens_at_every_scale_and_edge_every_footer_control_fits_hits_and_is_reachable() {
    for screen in [(320u32, 240u32), (400, 300)] {
        for scale in [1.0f32, 1.25, 2.0] {
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                let ctx = format!("{screen:?} @{scale} {edge:?}");
                let mut st = physical(screen, scale, edge);
                assert!(inside_card(&st, &Rect { x: 0.0, y: 0.0, w: screen.0 as f32, h: screen.1 as f32 }) || st.card.w > 0.0, "{ctx}");
                assert!(st.card.right() <= screen.0 as f32 + 0.5 && st.card.bottom() <= screen.1 as f32 + 0.5 && st.card.x >= -0.5 && st.card.y >= -0.5, "card {:?} leaves the screen: {ctx}", st.card);
                // the footer row: inside the card, no two controls overlap, and the pointer finds each where it is drawn
                let footer = zone(&st, "footer").clone();
                let rects: Vec<Rect> = (0..footer.items.len()).map(|i| footer.rect(i, 0).unwrap()).collect();
                for (i, r) in rects.iter().enumerate() {
                    assert!(inside_card(&st, r), "footer item {i} {r:?} leaves the card {:?}: {ctx}", st.card);
                    assert!(r.w >= 1.0 && r.h >= 1.0, "footer item {i} has no area: {ctx}");
                    for (j, o) in rects.iter().enumerate().skip(i + 1) {
                        assert!(r.right() <= o.x + 0.5 || o.right() <= r.x + 0.5, "footer items {i} and {j} overlap: {r:?} {o:?} {ctx}");
                    }
                    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                    assert_eq!(st.hit(x, y).map(|h| (h.0, h.1)), Some(("footer", i)), "the pointer misses footer item {i}: {ctx}");
                }
                // every zone's visible items stay inside the card
                for z in &st.zones {
                    for i in z.visible(st.m.offset(z.id)) {
                        assert!(inside_card(&st, &z.rect(i, st.m.offset(z.id)).unwrap()), "{} item {i} leaves the card: {ctx}", z.id);
                    }
                }
                // keyboard: Right walks the whole footer row, then Left comes back
                st.m.zone = "footer";
                st.m.sel.insert("footer", 0);
                for k in 1..footer.items.len() {
                    st.key(&key(Key::Right));
                    assert_eq!(st.m.sel("footer"), k, "Right from {} does not reach the next footer item: {ctx}", k - 1);
                }
                // the Session menu: it keeps to the room above the button, scrolls, and every row can be reached and run
                let session = footer.items.iter().position(|t| matches!(t, Target::Style(SESSION, _))).expect("a session button");
                st.m.sel.insert("footer", session);
                st.key(&key(Key::Enter));
                let menu = zone(&st, "menu").clone();
                let button = rects[session];
                let area = menu.area();
                assert!(inside_card(&st, &area) && area.bottom() <= button.y + 0.5, "menu rows {area:?} are not above the button {button:?} inside the card: {ctx}");
                assert!(menu.visible(0).len() >= 1 && menu.items.len() == 6, "{ctx}");
                for k in 0..menu.items.len() {
                    assert_eq!(st.m.sel("menu"), k, "{ctx}");
                    let shown = zone(&st, "menu");
                    let r = shown.rect(k, st.m.offset("menu")).unwrap_or_else(|| panic!("menu row {k} is not visible after the focus reached it: {ctx}"));
                    assert!(inside_card(&st, &r), "row {k} {r:?}: {ctx}");
                    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                    assert_eq!(st.hit(x, y).map(|h| h.0), Some("menu"), "the menu does not take its own row {k}: {ctx}");
                    if k + 1 < menu.items.len() {
                        st.key(&key(Key::Down));
                    }
                }
                // a wheel notch over the menu scrolls it when it has more rows than room
                let before = st.m.offset("menu");
                if menu.visible(0).len() < menu.items.len() {
                    let a = menu.area();
                    st.m.offset.insert("menu", 0);
                    st.scroll(a.x + a.w / 2.0, a.y + a.h / 2.0, 100.0);
                    assert!(st.m.offset("menu") > 0 || before > 0, "the wheel does not scroll the menu: {ctx}");
                }
                // Escape closes the menu only
                let r = st.key(&key(Key::Escape));
                assert!(!closes(&r) && st.zones.iter().all(|z| z.id != "menu"), "{ctx}");
            }
        }
    }
}

#[test]
fn a_menu_row_that_scrolled_into_view_runs_its_action() {
    let mut st = physical((320, 240), 2.0, Edge::Bottom);
    let session = zone(&st, "footer").items.iter().position(|t| matches!(t, Target::Style(SESSION, _))).unwrap();
    st.m.zone = "footer";
    st.m.sel.insert("footer", session);
    st.key(&key(Key::Enter));
    let n = zone(&st, "menu").items.len();
    assert!(zone(&st, "menu").visible(0).len() < n, "this card cannot show every row at once");
    for _ in 0..n - 1 {
        st.key(&key(Key::Down));
    }
    // the last row is Shut Down: it arms, a second Enter runs it
    assert_eq!(st.current(), Some(Target::Style(MENU, sys_index(Sys::Shutdown))));
    st.key(&key(Key::Enter));
    assert!(ran().is_empty() && armed(&st.m) == Some(Sys::Shutdown));
    let r = st.key(&key(Key::Enter));
    assert!(closes(&r) && ran() == [Sys::Shutdown]);
}

// ---------------------------------------------------------------------------------------------------------- the confirmation ends with the focus

fn focus_footer(st: &mut super::super::Styled, target: Target) {
    let i = zone(st, "footer").items.iter().position(|t| *t == target).unwrap();
    st.m.zone = "footer";
    st.m.sel.insert("footer", i);
    st.relayout();
}

#[test]
fn an_armed_action_is_disarmed_when_the_keyboard_focus_leaves_it_and_comes_back() {
    let mut st = kde(30);
    let restart = Target::Style(SYS, sys_index(Sys::Reboot));
    focus_footer(&mut st, restart);
    st.key(&key(Key::Enter));
    assert_eq!(armed(&st.m), Some(Sys::Reboot));
    // away and back: not armed any more, Enter arms again instead of running
    st.key(&key(Key::Left));
    assert!(armed(&st.m).is_none(), "moving the focus disarms");
    st.key(&key(Key::Right));
    assert_eq!(st.current(), Some(restart));
    let r = st.key(&key(Key::Enter));
    assert!(!closes(&r) && ran().is_empty(), "Enter after leaving and returning must not run it: {:?}", ran());
    assert_eq!(armed(&st.m), Some(Sys::Reboot), "it armed again");
    // only now does a second Enter on the same control run it
    let r = st.key(&key(Key::Enter));
    assert!(closes(&r) && ran() == [Sys::Reboot]);
}

#[test]
fn pointer_motion_to_another_control_and_back_disarms_too() {
    let mut st = kde(30);
    let (restart, shut) = (Target::Style(SYS, sys_index(Sys::Reboot)), Target::Style(SYS, sys_index(Sys::Shutdown)));
    click(&mut st, "footer", restart);
    assert_eq!(armed(&st.m), Some(Sys::Reboot));
    // the pointer moves within the same button: still armed
    let (x, y) = centre(&st, "footer", restart);
    st.motion(x + 2.0, y);
    assert_eq!(armed(&st.m), Some(Sys::Reboot));
    // over Shut Down and back
    let (sx, sy) = centre(&st, "footer", shut);
    st.motion(sx, sy);
    assert!(armed(&st.m).is_none());
    st.motion(x, y);
    let r = click(&mut st, "footer", restart);
    assert!(!closes(&r) && ran().is_empty(), "the click after returning only arms again");
    assert!(closes(&click(&mut st, "footer", restart)));
    assert_eq!(ran(), [Sys::Reboot]);
}

#[test]
fn typing_a_search_or_tabbing_away_disarms() {
    let mut st = kde(30);
    let shut = Target::Style(SYS, sys_index(Sys::Shutdown));
    focus_footer(&mut st, shut);
    st.key(&key(Key::Enter));
    assert_eq!(armed(&st.m), Some(Sys::Shutdown));
    st.key(&KeyEvent { key: Key::Char, text: "f".into(), ch: Some('f'), ctrl: false, shift: false, alt: false });
    assert!(armed(&st.m).is_none(), "typing moves the focus to the results");
    st.key(&key(Key::Escape));
    // Tab around the whole ring and back to the same control: not armed
    let mut st = kde(30);
    focus_footer(&mut st, shut);
    st.key(&key(Key::Enter));
    assert_eq!(armed(&st.m), Some(Sys::Shutdown));
    let zones = st.zones.iter().filter(|z| z.items.iter().any(|t| t.focusable())).count();
    for _ in 0..zones {
        st.key(&key(Key::Tab));
    }
    assert!(armed(&st.m).is_none() && ran().is_empty());
}

#[test]
fn log_out_in_the_menu_is_disarmed_by_moving_the_selection() {
    let mut st = kde(30);
    click(&mut st, "footer", Target::Style(SESSION, 0));
    st.key(&key(Key::Down));
    assert_eq!(st.current(), Some(Target::Style(MENU, sys_index(Sys::Logout))));
    st.key(&key(Key::Enter));
    assert_eq!(armed(&st.m), Some(Sys::Logout));
    st.key(&key(Key::Up));
    st.key(&key(Key::Down));
    assert!(armed(&st.m).is_none());
    let r = st.key(&key(Key::Enter));
    assert!(!closes(&r) && ran().is_empty(), "armed again, not run");
    assert!(closes(&st.key(&key(Key::Enter))) && ran() == [Sys::Logout]);
}

// ------------------------------------------------------------------------------------------------------------- the Places lists load off the UI path

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

const LONG: std::time::Duration = std::time::Duration::from_secs(20);

/// A waker that signals a channel: the deterministic "the worker finished" handshake (the timeout only turns a hang into a failure).
fn wake_channel() -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    set_waker(Arc::new(move || drop(tx.lock().unwrap().send(()))));
    rx
}

/// A Places worker that holds until `release` is dropped or sent to (or, with `until_cancelled`, until it is asked to stop).
fn held(name: &'static str, until_cancelled: bool) -> (mpsc::Sender<()>, mpsc::Receiver<()>, impl FnOnce(&AtomicBool) -> places::Loaded + Send + 'static) {
    let (release, hold) = mpsc::channel::<()>();
    let (started_tx, started) = mpsc::channel::<()>();
    let worker = move |cancel: &AtomicBool| {
        started_tx.send(()).ok();
        if until_cancelled {
            while !cancel.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        } else {
            drop(hold.recv_timeout(LONG));
        }
        places::Loaded { computer: vec![Entry { name: name.into(), detail: "d".into(), icon: "folder".into(), open: vec!["xdg-open".into(), name.into()], at: 0 }], documents: Vec::new() }
    };
    (release, started, worker)
}

fn snapshot(st: &super::super::Styled) -> (usize, String, &'static str, usize, usize, usize) {
    (tab(&st.m), st.m.query().to_string(), st.m.zone, st.m.sel(st.m.zone), place(&st.m), st.m.ui("kde.menu"))
}

#[test]
fn building_the_launcher_and_opening_places_never_wait_for_the_disk() {
    let mut st = kde(10);
    // nothing is read when the launcher is built
    assert_eq!(places::state(), places::State::Idle);
    assert!(places::data().is_none() && places::spawned() == 0);
    // a worker that is stuck: opening the tab through the real control returns at once, with the rows that need no disk
    let (release, started, worker) = held("Stuck", false);
    places::request(worker);
    started.recv_timeout(LONG).unwrap();
    let r = click(&mut st, "footer", Target::Style(TAB, 1));
    assert!(!closes(&r) && tab(&st.m) == 1);
    assert_eq!(places::state(), places::State::Loading);
    let rows = computer_rows();
    assert_eq!(rows[0].name, "Show KRunner");
    if std::env::var_os("HOME").is_some() {
        assert_eq!(rows.get(1).map(|e| e.name.as_str()), Some("Home"), "Home needs no file system");
    }
    assert_eq!(zone(&st, "apps").items.len(), rows.len());
    // the sidebar, the footer and the keyboard all keep working while it is stuck
    click(&mut st, "cats", Target::Style(PLACE, 1));
    assert_eq!(place(&st.m), 1);
    click(&mut st, "cats", Target::Style(PLACE, 0));
    st.m.zone = "apps";
    st.m.sel.insert("apps", 1);
    st.relayout();
    let opened = st.key(&key(Key::Enter));
    if std::env::var_os("HOME").is_some() {
        assert!(closes(&opened) && !OPENED.with(|o| o.borrow().is_empty()), "Home opens while the rest is still being read");
    }
    drop(release);
}

#[test]
fn opening_and_closing_places_many_times_while_the_worker_is_blocked_never_adds_a_thread() {
    let mut st = kde(10);
    let (release, started, worker) = held("Stuck", false);
    places::request(worker);
    started.recv_timeout(LONG).unwrap();
    for _ in 0..300 {
        click(&mut st, "footer", Target::Style(TAB, 1));
        click(&mut st, "footer", Target::Style(TAB, 0));
    }
    assert_eq!(places::spawned(), 1, "one worker however many times the tab was opened and closed");
    assert!(places::state() == places::State::Loading);
    drop(release);
}

#[test]
fn a_late_answer_changes_the_store_and_nothing_the_user_is_looking_at() {
    let mut st = kde(10);
    let woken = wake_channel();
    let (release, started, worker) = held("Arrived", false);
    places::request(worker);
    started.recv_timeout(LONG).unwrap();
    click(&mut st, "footer", Target::Style(TAB, 1));
    // still on Places, the user moves to History, types a search and focuses a result
    click(&mut st, "cats", Target::Style(PLACE, 1));
    st.key(&KeyEvent { key: Key::Char, text: "a".into(), ch: Some('a'), ctrl: false, shift: false, alt: false });
    st.key(&key(Key::Down));
    let before = (snapshot(&st), st.zones.clone());
    release.send(()).unwrap();
    woken.recv_timeout(LONG).expect("the worker announced itself through the waker");
    assert!(tick(), "the host takes the answer in on its tick");
    st.relayout();
    assert_eq!((snapshot(&st), st.zones.clone()), before, "the page, the query, the focus and the zones are exactly what they were");
    assert_eq!(places::data().unwrap().computer[0].name, "Arrived");
    assert_eq!(places::spawned(), 1);
    // the query is cleared and Computer opened: the answer is there, and it is not read again
    click(&mut st, "footer", Target::Style(TAB, 1));
    click(&mut st, "cats", Target::Style(PLACE, 0));
    assert!(computer_rows().iter().any(|e| e.name == "Arrived"));
    assert_eq!(places::spawned(), 1);
}

#[test]
fn leaving_places_stops_the_worker_and_discards_its_answer_and_the_next_opening_starts_one_new_worker() {
    let mut st = kde(10);
    let woken = wake_channel();
    let (_release, started, worker) = held("Abandoned", true);
    places::request(worker);
    started.recv_timeout(LONG).unwrap();
    click(&mut st, "footer", Target::Style(TAB, 1));
    click(&mut st, "footer", Target::Style(TAB, 0));
    woken.recv_timeout(LONG).expect("the stopped worker finished");
    assert!(!tick(), "its answer is discarded");
    assert!(places::data().is_none() && places::state() == places::State::Idle);
    // the next opening starts exactly one new worker (the real one)
    click(&mut st, "footer", Target::Style(TAB, 1));
    assert_eq!(places::spawned(), 2);
}

#[test]
fn shared_host_waker_and_tick_deliver_places_without_further_input() {
    let mut st = kde(10);
    let (wake, woken) = std::sync::mpsc::channel();
    st.set_waker(std::sync::Arc::new(move || {
        let _ = wake.send(());
    }));
    let (release, started, worker) = held("Ready without pointer motion", false);
    places::request(worker);
    started.recv_timeout(LONG).unwrap();
    click(&mut st, "footer", Target::Style(TAB, 1));
    release.send(()).unwrap();
    woken.recv_timeout(LONG).unwrap();
    assert_eq!(st.tick(), crate::window_app::Response::REDRAW);
    assert_eq!(computer_rows()[0].name, "Ready without pointer motion");
    assert_eq!(zone(&st, "apps").items.len(), computer_rows().len());
    assert_eq!(st.tick(), crate::window_app::Response::NONE);
}
