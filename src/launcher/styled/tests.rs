//! Behaviour every styled launcher shares, checked for each style, panel edge, UI scale and small screens.

use super::*;
use crate::apps::AppEntry;
use crate::fuzzy::History;
use crate::launcher::placement::{Anchor, Edge};

pub(crate) const STYLES: [LauncherStyle; 6] = [LauncherStyle::Windows, LauncherStyle::Gnome, LauncherStyle::Kde, LauncherStyle::Xfce, LauncherStyle::BlackBerry, LauncherStyle::Ukui];
const CATS: [&str; 6] = ["Network;WebBrowser;", "System;TerminalEmulator;", "Graphics;", "Development;IDE;", "AudioVideo;Player;", "Utility;"];

pub(crate) fn fixture_apps(n: usize) -> Vec<AppEntry> {
    (0..n)
        .map(|i| AppEntry {
            name: format!("{} App {i:02}", ["Alpha", "Beta", "Gamma", "Delta", "Echo", "Kilo"][i % 6]),
            exec: "true".into(),
            categories: CATS[i % CATS.len()].into(),
            file: format!("/fixture/app{i:02}.desktop"),
            ..Default::default()
        })
        .collect()
}

pub(crate) fn styled(style: LauncherStyle, n: usize) -> Styled {
    let apps = {
        let mut a = fixture_apps(n);
        a.sort_by_key(|e| e.name.to_lowercase());
        a
    };
    let pins: Vec<String> = (0..n.min(9)).map(|i| format!("app{i:02}")).collect();
    let mut st = Styled::new(style, Catalog::with(apps, Some(pins), History::default()));
    st.fonts = None;
    st.m.user = "Test User".into();
    st
}

fn open(st: &mut Styled, screen: (u32, u32), scale: f32, anchor: Option<Anchor>) {
    st.full = true;
    st.scale = scale;
    st.anchor = anchor;
    st.resize(screen.0, screen.1);
}

fn key(k: Key, text: &str) -> KeyEvent {
    KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift: false, alt: false }
}

fn edge_anchor(edge: Edge, screen: (u32, u32), t: f32) -> Anchor {
    let (w, h) = (screen.0 as f32, screen.1 as f32);
    let point = match edge {
        Edge::Top => (w * 0.1, t / 2.0),
        Edge::Bottom => (w / 2.0, h - t / 2.0),
        Edge::Left => (t / 2.0, h * 0.9),
        Edge::Right => (w - t / 2.0, h * 0.2),
    };
    Anchor { edge, point: Some(point), thickness: t }
}

fn inside(r: &Rect, outer: &Rect) -> bool {
    r.x >= outer.x - 0.5 && r.y >= outer.y - 0.5 && r.right() <= outer.right() + 0.5 && r.bottom() <= outer.bottom() + 0.5
}

#[test]
fn every_visible_item_stays_inside_the_card_for_all_edges_scales_and_small_screens() {
    for style in STYLES {
        for screen in [(1024, 600), (800, 480), (1920, 1080)] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                    let mut st = styled(style, 60);
                    open(&mut st, screen, scale, Some(edge_anchor(edge, screen, 32.0 * scale)));
                    let ctx = format!("{style:?} {screen:?} @{scale} {edge:?}");
                    assert!(inside(&st.card, &Rect { x: 0.0, y: 0.0, w: screen.0 as f32, h: screen.1 as f32 }), "{ctx}");
                    let content = st.zones.iter().find(|z| z.id == "apps").unwrap_or_else(|| panic!("no app zone: {ctx}"));
                    assert!(!content.visible(0).is_empty(), "at least one app shows: {ctx}");
                    for z in &st.zones {
                        for i in z.visible(st.m.offset(z.id)) {
                            let r = z.rect(i, st.m.offset(z.id)).unwrap();
                            assert!(inside(&r, &st.card), "{} item {i} {r:?} leaves the card {:?}: {ctx}", z.id, st.card);
                        }
                    }
                    if scale != 1.0 && scale != 2.0 {
                        continue; // geometry is checked at every scale; painting at the two extremes is enough
                    }
                    let mut pm = Pixmap::new(screen.0, screen.1).unwrap();
                    st.draw(&mut pm);
                    let (cx, cy) = ((st.card.x + st.card.w / 2.0) as u32, (st.card.y + st.card.h / 2.0) as u32);
                    assert!(pm.pixel(cx, cy).unwrap().alpha() > 200, "the card is painted: {ctx}");
                    // the panel strip stays clear so the panel shows through
                    let (px, py) = match edge {
                        Edge::Top => (screen.0 / 2, 2),
                        Edge::Bottom => (screen.0 / 2, screen.1 - 2),
                        Edge::Left => (2, screen.1 / 2),
                        Edge::Right => (screen.0 - 2, screen.1 / 2),
                    };
                    assert_eq!(pm.pixel(px, py).unwrap().alpha(), 0, "the panel is covered: {ctx}");
                }
            }
        }
    }
}

#[test]
fn typing_searches_enter_launches_and_escape_closes() {
    for style in STYLES {
        let mut st = styled(style, 30);
        open(&mut st, (1366, 768), 1.0, None);
        for c in "kilo".chars() {
            assert!(st.key(&key(Key::Char, &c.to_string())).redraw, "{style:?}");
        }
        assert_eq!(st.m.query(), "kilo");
        assert_eq!(st.m.zone, "results", "{style:?} focuses the results");
        assert!(!st.m.results.is_empty() && st.m.results.iter().all(|&i| st.m.cat.apps[i].name.starts_with("Kilo")), "{style:?}");
        assert!(st.zones.iter().any(|z| z.id == "results" && !z.items.is_empty()), "{style:?} shows the results");
        let first = st.m.results[0];
        assert_eq!(st.current(), Some(Target::App(first)));
        assert_eq!(st.key(&key(Key::Enter, "")).action, Some(Action::Close), "{style:?} Enter launches and closes");
        for _ in 0..4 {
            st.key(&key(Key::Backspace, ""));
        }
        assert!(!st.m.searching());
        assert_ne!(st.m.zone, "results");
        st.key(&key(Key::Char, "z"));
        st.key(&key(Key::Char, "z"));
        st.key(&key(Key::Char, "z"));
        assert!(st.m.results.is_empty());
        assert_eq!(st.key(&key(Key::Enter, "")).action, None, "{style:?}: nothing to launch");
        let ctrl_u = KeyEvent { ctrl: true, ..key(Key::Char, "u") };
        st.key(&ctrl_u);
        assert_eq!(st.m.query(), "");
        assert_eq!(st.key(&key(Key::Escape, "")).action, Some(Action::Close), "{style:?}");
    }
}

#[test]
fn arrows_and_tab_move_across_items_and_zones() {
    for style in STYLES {
        let mut st = styled(style, 80);
        open(&mut st, (1600, 900), 1.0, None);
        let start = (st.m.zone, st.m.sel(st.m.zone));
        let mut seen = std::collections::HashSet::new();
        for k in [Key::Right, Key::Down, Key::Down, Key::Left, Key::Up] {
            st.key(&key(k, ""));
            let z = st.zones.iter().find(|z| z.id == st.m.zone).expect("focus is in a zone");
            let i = st.m.sel(z.id);
            assert!(z.items[i].focusable(), "{style:?}");
            assert!(z.rect(i, st.m.offset(z.id)).is_some(), "{style:?}: the focused item is scrolled into view");
            seen.insert((st.m.zone, i));
        }
        assert!(seen.len() >= 2, "{style:?}: arrows move ({start:?} -> {seen:?})");
        let mut zones = std::collections::HashSet::new();
        for _ in 0..st.zones.len() + 1 {
            assert!(st.key(&key(Key::Tab, "")).redraw, "{style:?}");
            zones.insert(st.m.zone);
        }
        assert!(zones.len() >= 2, "{style:?}: Tab visits zones {zones:?}");
        let before = st.m.zone;
        st.key(&key(Key::Tab, ""));
        st.key(&KeyEvent { shift: true, ..key(Key::Tab, "") });
        assert_eq!(st.m.zone, before, "{style:?}: Shift+Tab goes back");
        assert!(st.m.keyboard, "keyboard focus draws a ring");
    }
}

#[test]
fn hover_click_right_click_wheel_and_outside_clicks() {
    for style in STYLES {
        let mut st = styled(style, 120);
        open(&mut st, (1280, 800), 1.25, Some(edge_anchor(Edge::Bottom, (1280, 800), 40.0)));
        // the full list, so pinning does not move the item away
        st.m.view = View::All;
        st.relayout();
        let z = st.zones.iter().find(|z| z.id == "apps").unwrap().clone();
        let i = z.visible(0).filter(|&i| z.items[i].focusable()).nth(1).unwrap();
        let r = z.rect(i, 0).unwrap();
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert!(st.motion(x, y).redraw, "{style:?} hover");
        assert!(st.m.focused("apps", i) && !st.m.keyboard);
        let Target::App(a) = z.items[i] else { panic!() };
        let pinned = st.m.cat.is_favorite(a);
        st.button(x, y, Button::Right, true);
        assert_ne!(st.m.cat.is_favorite(a), pinned, "{style:?} right click pins");
        st.button(x, y, Button::Right, true);
        assert_eq!(st.m.cat.is_favorite(a), pinned);
        // wheel over the grid scrolls or pages it
        let z = st.zones.iter().find(|z| z.id == "apps").unwrap().clone();
        if z.max_offset() > 0 {
            let a = z.area();
            assert!(st.scroll(a.x + a.w / 2.0, a.y + a.h / 2.0, 100.0).redraw, "{style:?} wheel");
            assert!(st.m.offset("apps") > 0);
            st.scroll(a.x + a.w / 2.0, a.y + a.h / 2.0, -100.0);
            assert_eq!(st.m.offset("apps"), 0, "{style:?} wheel back");
        }
        // a click inside the card but on nothing keeps it; outside closes
        assert_eq!(st.button(st.card.x + 1.0, st.card.y + 1.0, Button::Left, true).action, None, "{style:?}");
        let outside = if st.card.y > 2.0 { (st.card.x + 2.0, 1.0) } else { (1.0, 799.0) };
        assert_eq!(st.button(outside.0, outside.1, Button::Left, true).action, Some(Action::Close), "{style:?} outside click");
        // a left click on an app launches it (exec `true`) and closes
        let z = st.zones.iter().find(|z| z.id == "apps").unwrap().clone();
        let i = z.visible(st.m.offset("apps")).find(|&i| matches!(z.items[i], Target::App(_))).unwrap();
        let r = z.rect(i, st.m.offset("apps")).unwrap();
        assert_eq!(st.button(r.x + r.w / 2.0, r.y + r.h / 2.0, Button::Left, true).action, Some(Action::Close), "{style:?} click launches");
    }
}

#[test]
fn categories_filter_the_apps() {
    for style in [LauncherStyle::Kde, LauncherStyle::Xfce, LauncherStyle::BlackBerry] {
        let mut st = styled(style, 36);
        open(&mut st, (1366, 768), 1.0, None);
        let cats = st.zones.iter().find(|z| z.id == "cats").expect("a category list").clone();
        let i = cats.items.iter().position(|t| *t == Target::View(View::Category(Category::Graphics))).expect("Graphics is listed");
        let r = cats.rect(i, 0).expect("visible");
        st.button(r.x + 4.0, r.y + 4.0, Button::Left, true);
        assert_eq!(st.m.view, View::Category(Category::Graphics), "{style:?}");
        let apps = st.zones.iter().find(|z| z.id == "apps").unwrap();
        assert_eq!(apps.items.len(), 6, "{style:?}");
        assert!(apps.items.iter().all(|t| matches!(t, Target::App(a) if st.m.cat.category[*a] == Category::Graphics)), "{style:?}");
    }
}

#[test]
fn windows_all_apps_and_back() {
    let mut st = styled(LauncherStyle::Windows, 40);
    open(&mut st, (1920, 1080), 1.0, None);
    let nav = st.zones.iter().find(|z| z.id == "nav").unwrap().clone();
    assert_eq!(nav.items, [Target::View(View::All)]);
    let r = nav.rect(0, 0).unwrap();
    st.button(r.x + 2.0, r.y + 2.0, Button::Left, true);
    assert_eq!(st.m.view, View::All);
    assert_eq!(st.m.zone, "apps", "the list takes the focus");
    let list = st.zones.iter().find(|z| z.id == "apps").unwrap();
    assert_eq!(list.items[0], Target::Letter('A'), "letter headings");
    assert!(matches!(st.current(), Some(Target::App(_))), "headings are skipped by the focus");
    let back = st.zones.iter().find(|z| z.id == "nav").unwrap().items[0];
    assert_eq!(back, Target::View(View::Home));
    // Pin to Start from the search detail pane
    st.key(&key(Key::Char, "e"));
    let detail = st.zones.iter().find(|z| z.id == "detail").expect("best-match pane").clone();
    assert!(matches!(detail.items[1], Target::Pin(_)));
}

#[test]
fn xfce_keeps_whiskers_order_on_any_panel_and_gnome_pages() {
    // Whisker's default layout is title, search, contents wherever the panel is; only the window moves.
    for edge in [Edge::Top, Edge::Bottom] {
        let mut st = styled(LauncherStyle::Xfce, 20);
        open(&mut st, (1366, 768), 1.0, Some(edge_anchor(edge, (1366, 768), 28.0)));
        let y = |id: &str| st.zones.iter().find(|z| z.id == id).unwrap().area().y;
        assert!(y("actions") < y("apps") && y("apps") == y("cats"), "{edge:?}");
    }

    let mut g = styled(LauncherStyle::Gnome, 100);
    open(&mut g, (1024, 600), 1.0, None);
    let pages = g.zones.iter().find(|z| z.id == "pages").expect("page dots").items.len();
    assert!(pages > 1);
    let per = g.zones[0].per_view();
    for _ in 0..per {
        g.key(&key(Key::Right, ""));
    }
    assert_eq!(g.m.offset("apps"), 1, "arrowing past the page turns it");
    let dots = g.zones.iter().find(|z| z.id == "pages").unwrap().clone();
    let r = dots.rect(0, 0).unwrap();
    g.button(r.x + r.w / 2.0, r.y + r.h / 2.0, Button::Left, true);
    assert_eq!(g.m.offset("apps"), 0, "a dot opens its page");

    let mut bb = styled(LauncherStyle::BlackBerry, 20);
    open(&mut bb, (1024, 600), 2.0, None);
    assert!(bb.zones.iter().all(|z| z.id != "cats"), "the hub folds away on a narrow screen");
    open(&mut bb, (1920, 1080), 1.0, None);
    assert!(bb.zones.iter().any(|z| z.id == "cats"));
}

#[test]
fn light_and_dark_themes_both_paint_readable_cards() {
    use crate::theme::{builtin, model::Selection};
    struct RestoreTheme(std::sync::Arc<crate::theme::Theme>);
    impl Drop for RestoreTheme {
        fn drop(&mut self) {
            crate::theme::live::publish_theme((*self.0).clone());
        }
    }
    let _restore = RestoreTheme(crate::theme::th());
    for dark in [true, false] {
        crate::theme::live::publish_theme(builtin::def("macos").unwrap().resolve("macos", dark, &Selection::default()));
        for style in STYLES {
            let mut st = styled(style, 20);
            open(&mut st, (1280, 800), 1.0, None);
            let mut pm = Pixmap::new(1280, 800).unwrap();
            st.draw(&mut pm);
            let p = pm.pixel((st.card.x + st.card.w / 2.0) as u32, (st.card.bottom() - 3.0) as u32).unwrap();
            let lum = (u32::from(p.red()) + u32::from(p.green()) + u32::from(p.blue())) / 3;
            // dark themes paint dark cards, light themes light ones (BB10's dark home only in dark mode)
            assert_eq!(lum < 128, dark, "{style:?} dark={dark} lum={lum}");
        }
    }
}

#[test]
fn opaque_surfaces_get_an_opaque_card() {
    for style in STYLES {
        let mut st = styled(style, 10);
        open(&mut st, (1024, 600), 1.0, None);
        st.alpha = false;
        let mut pm = Pixmap::new(1024, 600).unwrap();
        st.draw(&mut pm);
        let p = pm.pixel((st.card.x + st.card.w / 2.0) as u32, (st.card.y + 4.0) as u32).unwrap();
        assert_eq!(p.alpha(), 255, "{style:?}: no compositor, so no translucency");
    }
}

#[test]
fn windows_best_match_actions_follow_the_current_selection() {
    let detail_app = |st: &Styled| match st.zones.iter().find(|z| z.id == "detail").map(|z| z.items.clone()).as_deref() {
        Some([Target::App(a), Target::Pin(b)]) if a == b => *a,
        other => panic!("detail pane {other:?}"),
    };
    let mut st = styled(LauncherStyle::Windows, 30);
    open(&mut st, (1600, 900), 1.0, None);
    for c in "app".chars() {
        st.key(&key(Key::Char, &c.to_string()));
    }
    assert!(st.m.results.len() >= 2);
    let (first, second) = (st.m.results[0], st.m.results[1]);
    assert_eq!(detail_app(&st), first);
    // keyboard: Down selects the second result; Tab into the pane, Enter opens that one
    st.key(&key(Key::Down, ""));
    assert_eq!(st.current(), Some(Target::App(second)));
    assert_eq!(detail_app(&st), second, "the pane follows the keyboard");
    st.key(&key(Key::Tab, ""));
    assert_eq!(st.m.zone, "detail");
    assert_eq!(st.current(), Some(Target::App(second)), "Tab lands on Open for the selected app");
    // Pin to Start pins exactly the selected app
    st.key(&key(Key::Down, ""));
    assert_eq!(st.current(), Some(Target::Pin(second)));
    let pinned = st.m.cat.is_favorite(second);
    assert_eq!(st.key(&key(Key::Enter, "")).action, None);
    assert_ne!(st.m.cat.is_favorite(second), pinned, "Pin toggles the selected app");
    assert_eq!(st.m.cat.is_favorite(first), st.m.cat.favorites.contains(&first), "and nothing else");
    // pointer: hovering the first result retargets the pane; a click on Open opens it
    let results = st.zones.iter().find(|z| z.id == "results").unwrap().clone();
    let r = results.rect(0, 0).unwrap();
    st.motion(r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(detail_app(&st), first, "the pane follows hover");
    let open_r = st.zones.iter().find(|z| z.id == "detail").unwrap().rect(0, 0).unwrap();
    // even a target built before the selection moved acts on the current selection
    st.m.sel.insert("results", 1);
    assert_eq!(st.activate(Target::App(first), "detail").action, Some(Action::Close));
    assert_eq!(st.m.cat.recent, [second], "the app opened (and recorded) is the selected result, not the stale target");
    let _ = open_r;
}

#[test]
fn search_results_and_their_actions_stay_inside_the_card_everywhere() {
    for style in STYLES {
        for screen in [(800, 480), (1024, 600), (1920, 1080)] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for (edge, t) in [(Edge::Top, 64.0), (Edge::Bottom, 48.0 * scale), (Edge::Left, 32.0 * scale), (Edge::Right, 32.0 * scale)] {
                    let mut st = styled(style, 60);
                    open(&mut st, screen, scale, Some(edge_anchor(edge, screen, t)));
                    for c in "app".chars() {
                        st.key(&key(Key::Char, &c.to_string()));
                    }
                    // walk the focus through every zone: each focused item must be visible inside the card
                    for k in [Key::Down, Key::Down, Key::Tab, Key::Down, Key::Tab, Key::Tab, Key::Right] {
                        st.key(&key(k, ""));
                        let ctx = format!("{style:?} {screen:?} @{scale} {edge:?} after {k:?}");
                        for z in &st.zones {
                            for i in z.visible(st.m.offset(z.id)) {
                                let r = z.rect(i, st.m.offset(z.id)).unwrap();
                                assert!(inside(&r, &st.card) && r.h >= 1.0, "{} {i} {r:?} outside {:?}: {ctx}", z.id, st.card);
                            }
                        }
                        let z = st.zones.iter().find(|z| z.id == st.m.zone).expect("focus in a zone");
                        assert!(z.rect(st.m.sel(z.id), st.m.offset(z.id)).is_some(), "focused item not on screen: {ctx}");
                    }
                }
            }
        }
    }
}
