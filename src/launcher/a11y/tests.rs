//! Source tests of the launcher's accessibility adapter. They build the real launcher (Launchpad, Dux and every
//! styled launcher with its real layout), describe it, and send requests the way the window host does: validated
//! against the last published tree, then dispatched to the live model. Nothing here needs a display or a bus.
//!
//! Launching: the Launchpad and Dux start apps through `Launcher::spawn`, replaced here by a recorder, so those tests
//! start nothing. The styled launchers start apps through the catalog's own launch, so their tests use fixture apps
//! whose `Exec` is `true` (exits at once, opens nothing), like the style engine's own tests.

use super::*;
use crate::a11y::action::validate;
use crate::a11y::tree::{stamp, Ids, Node, Tree, WindowInfo};
use crate::a11y::Request;
use crate::app::Button;
use crate::launcher::styled;
use crate::launcher::styled::tests as sty;
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static SPAWNED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn fake_spawn(a: &AppEntry) -> bool {
    SPAWNED.with(|s| s.borrow_mut().push(a.name.clone()));
    true
}

/// The names of the apps "started" since the last call.
fn spawned() -> Vec<String> {
    SPAWNED.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

fn app(name: &str) -> AppEntry {
    AppEntry { name: name.into(), exec: "true".into(), file: format!("/fixture/{}.desktop", name.to_lowercase().replace(' ', "-")), comment: format!("About {name}"), ..Default::default() }
}

fn key(k: Key, text: &str) -> KeyEvent {
    KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift: false, alt: false }
}

fn ctrl_u() -> KeyEvent {
    KeyEvent { key: Key::Char, text: String::new(), ch: Some('u'), ctrl: true, shift: false, alt: false }
}

const SCREEN: (u32, u32) = (1366, 768);

/// The Launchpad or Dux with `n` apps ("App 000" ...), in a window `size` big.
fn classic(style: LauncherStyle, n: usize, size: (u32, u32)) -> Launcher {
    let apps = (0..n).map(|i| app(&format!("App {i:03}"))).collect();
    classic_with(style, apps, size)
}

fn classic_with(style: LauncherStyle, apps: Vec<AppEntry>, size: (u32, u32)) -> Launcher {
    let mut l = Launcher::build(apps, style, None);
    l.fonts = None;
    l.spawn = fake_spawn;
    if style == LauncherStyle::Dux {
        l.set_screen(size.0 + 100, size.1 + 100);
    }
    l.resize(size.0, size.1);
    l
}

fn launchpad(n: usize) -> Launcher {
    classic(LauncherStyle::Default, n, (1000, 680))
}

/// A styled engine opened the way the X11 popup opens it: a full-output surface with the card placed on it.
fn open(style: LauncherStyle, n: usize, screen: (u32, u32)) -> Styled {
    let mut st = sty::styled(style, n);
    st.full = true;
    st.resize(screen.0, screen.1);
    st
}

fn wrap(st: Styled) -> Launcher {
    let style = st.style;
    let mut l = Launcher::build(Vec::new(), style, Some(st));
    l.fonts = None;
    l
}

fn styled_launcher(style: LauncherStyle, n: usize) -> Launcher {
    wrap(open(style, n, SCREEN))
}

fn type_into(st: &mut Styled, text: &str) {
    for c in text.chars() {
        st.key(&key(Key::Char, &c.to_string()));
    }
}

/// A launcher, its published tree and the host's bookkeeping.
struct Rig {
    l: Launcher,
    ids: Ids,
    tree: Tree,
}

impl Rig {
    fn new(l: Launcher) -> Rig {
        let mut r = Rig { l, ids: Ids::default(), tree: Tree::default() };
        r.redraw();
        r
    }

    fn redraw(&mut self) {
        let mut b = TreeBuilder::new(&mut self.ids, "Launcher");
        self.l.a11y_tree(&mut b);
        let mut t = b.finish();
        t.set_window(WindowInfo { origin: Some((0, 0)), active: true });
        let prev = std::mem::take(&mut self.tree);
        stamp(if prev.is_empty() { None } else { Some(&prev) }, &mut t);
        self.tree = t;
    }

    fn find(&self, key: &str) -> Option<&Node> {
        self.tree.by_key(key)
    }

    fn node(&self, key: &str) -> &Node {
        self.find(key).unwrap_or_else(|| panic!("no node {key}; have {:?}", self.tree.nodes().iter().map(|n| n.key.as_str()).collect::<Vec<_>>()))
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.l.key(&key(Key::Char, &c.to_string()));
        }
    }

    /// A request against the last published tree (which may be older than the model), validated and dispatched like
    /// the host does, then the window repaints.
    fn act(&mut self, key: &str, op: Op) -> Result<Response, ActionError> {
        let id = self.node(key).id;
        let req = Request { id: 1, revision: self.tree.revision, node: id, op, expires: None };
        let tree = self.tree.clone();
        let r = validate(&tree, &req).and_then(|t| self.l.a11y_action(&t));
        self.redraw();
        r
    }

    fn selected(&self) -> Vec<&str> {
        self.tree.nodes().iter().filter(|n| n.role == Role::ListItem && n.states.contains(State::Selected)).map(|n| n.key.as_str()).collect()
    }

    fn focus_key(&self) -> Option<&str> {
        self.tree.focus.and_then(|f| self.tree.get(f)).map(|n| n.key.as_str())
    }
}

fn list_items(r: &Rig) -> Vec<&Node> {
    r.tree.nodes().iter().filter(|n| n.role == Role::ListItem).collect()
}

// ------------------------------------------------------------------------------------------------ identity

#[test]
fn launchpad_ids_follow_desktop_ids_across_search_and_scrolling() {
    let mut r = Rig::new(launchpad(200));
    let id = r.node("apps/app:app-005").id;
    assert_eq!(r.node("apps/app:app-005").name, "App 005");
    r.type_text("005");
    r.redraw();
    let n = r.node("apps/app:app-005");
    assert_eq!(n.id, id, "the same app keeps its node when a search moves it");
    let pos = r.l.shown.iter().position(|&i| i == 5).expect("the app matches its own number");
    assert!(n.attributes.contains(&("posinset".to_string(), (pos + 1).to_string())), "{:?}", n.attributes);
    for _ in 0..3 {
        r.l.key(&key(Key::Backspace, ""));
    }
    r.l.sel = 199;
    r.l.reveal_selection();
    r.redraw();
    assert!(r.find("apps/app:app-005").is_none(), "a scrolled-off row has no node");
    r.l.sel = 5;
    r.l.reveal_selection();
    r.redraw();
    assert_eq!(r.node("apps/app:app-005").id, id, "and comes back as the same object");
}

#[test]
fn keys_use_the_desktop_id_and_disambiguate_equal_ids_in_catalog_order() {
    let twin = |dir: &str| AppEntry { name: "Twin".into(), file: format!("/{dir}/twin.desktop"), exec: "true".into(), ..Default::default() };
    let other = AppEntry { name: "Other".into(), file: "/x/other.desktop".into(), exec: "true".into(), ..Default::default() };
    assert_eq!(app_keys(&[twin("a"), other, twin("b")]), ["twin", "other", "twin#2"]);
}

#[test]
fn every_style_keeps_an_apps_node_across_search_and_clearing() {
    for style in sty::STYLES {
        let mut r = Rig::new(styled_launcher(style, 40));
        let first = list_items(&r).into_iter().find(|n| n.key.starts_with("apps/app:")).unwrap_or_else(|| panic!("{style:?}: no app in the main list")).clone();
        r.type_text(&first.name.to_lowercase());
        r.redraw();
        let n = r.node(&first.key);
        assert_eq!(n.id, first.id, "{style:?}: the same app is the same object in the results");
        let parent = r.tree.get(n.parent.unwrap()).unwrap();
        assert_eq!(parent.key, "zone:results", "{style:?}");
        r.l.key(&ctrl_u());
        r.redraw();
        assert_eq!(r.node(&first.key).id, first.id, "{style:?}: and back in the list");
    }
}

// ------------------------------------------------------------------------------------------------ launching

#[test]
fn launchpad_activate_starts_exactly_that_app_and_closes() {
    let mut r = Rig::new(launchpad(40));
    let out = r.act("apps/app:app-007", Op::Action("activate")).unwrap();
    assert_eq!(out.action, Some(Action::Close), "the launcher closes after a launch, as with the keyboard");
    assert_eq!(spawned(), ["App 007"]);
    assert_eq!(r.l.sel, 7);
}

#[test]
fn the_search_field_opens_the_selected_app() {
    let mut r = Rig::new(launchpad(40));
    r.l.key(&key(Key::Down, ""));
    r.redraw();
    let out = r.act("search", Op::Action("activate")).unwrap();
    assert_eq!(out.action, Some(Action::Close));
    assert_eq!(spawned(), ["App 007"], "Down moved one row of seven");
}

#[test]
fn a_launch_that_fails_is_reported_and_does_not_close() {
    let mut r = Rig::new(launchpad(5));
    r.l.spawn = |_| false;
    assert!(matches!(r.act("apps/app:app-002", Op::Action("activate")), Err(ActionError::Refused(_))));
    assert!(spawned().is_empty());
}

#[test]
fn every_style_activate_launches_the_exact_app_and_closes() {
    for style in sty::STYLES {
        let mut r = Rig::new(styled_launcher(style, 40));
        let item = list_items(&r).into_iter().filter(|n| n.key.starts_with("apps/app:")).last().unwrap_or_else(|| panic!("{style:?}: no app")).clone();
        let id = item.key.trim_start_matches("apps/app:").to_string();
        let out = r.act(&item.key, Op::Action("activate")).unwrap_or_else(|e| panic!("{style:?}: {e}"));
        assert_eq!(out.action, Some(Action::Close), "{style:?}");
        let st = r.l.styled.as_ref().unwrap();
        let index = st.m.cat.apps.iter().position(|a| app_id(a) == id).unwrap();
        assert_eq!(st.m.cat.recent, [index], "{style:?}: the launch was recorded for that app only");
    }
}

// ------------------------------------------------------------------------------------------------ refusals

#[test]
fn a_request_that_raced_a_keystroke_is_refused_without_launching() {
    let mut r = Rig::new(launchpad(40));
    let old = r.tree.clone();
    let id = r.node("apps/app:app-005").id;
    // typed after the tree was published: the item is gone from the live list
    r.type_text("zzzz");
    let rq = Request { id: 1, revision: old.revision, node: id, op: Op::Action("activate"), expires: None };
    let t = validate(&old, &rq).expect("the stale tree still offers it");
    assert_eq!(r.l.a11y_action(&t).unwrap_err(), ActionError::UnknownNode);
    assert!(spawned().is_empty());
}

#[test]
fn offpage_items_have_no_node_and_no_action() {
    let mut r = Rig::new(launchpad(200));
    let id = r.node("apps/app:app-005").id;
    r.l.sel = 199;
    r.l.reveal_selection();
    r.redraw();
    assert!(r.find("apps/app:app-005").is_none());
    let rq = Request { id: 1, revision: r.tree.revision, node: id, op: Op::Action("activate"), expires: None };
    assert_eq!(validate(&r.tree, &rq).unwrap_err(), ActionError::UnknownNode);
    let hidden = |n: &&Node| n.key.starts_with("apps/app:") && !n.states.contains(State::Showing);
    assert_eq!(r.tree.nodes().iter().filter(hidden).count(), 0, "every described row is showing");
}

#[test]
fn an_app_that_cannot_start_is_disabled() {
    let broken = AppEntry { try_exec: "definitely-not-installed-xyz".into(), ..app("Broken") };
    let l = classic_with(LauncherStyle::Default, vec![app("Good"), broken], (1000, 680));
    let mut r = Rig::new(l);
    let n = r.node("apps/app:broken");
    assert!(!n.enabled() && n.actions.is_empty());
    assert_eq!(r.act("apps/app:broken", Op::Action("activate")).unwrap_err(), ActionError::Disabled);
    assert!(spawned().is_empty());
    assert!(r.node("apps/app:good").enabled());
}

#[test]
fn a_request_older_than_the_items_last_change_is_stale() {
    let mut r = Rig::new(launchpad(40));
    let (id, before) = (r.node("apps/app:app-000").id, r.tree.revision);
    r.l.key(&key(Key::Down, ""));
    r.redraw();
    let rq = Request { id: 1, revision: before, node: id, op: Op::Action("activate"), expires: None };
    assert!(matches!(validate(&r.tree, &rq), Err(ActionError::Stale { .. })), "the selection moved off it after it was read");
}

// ------------------------------------------------------------------------------------------------ the search field

#[test]
fn the_search_field_is_editable_text_with_caret_and_selection() {
    let mut r = Rig::new(launchpad(40));
    let n = r.node("search");
    assert_eq!(n.role, Role::Entry);
    assert!(n.states.contains(State::Editable) && n.states.contains(State::SingleLine) && n.states.contains(State::Focusable));
    assert!(n.attributes.contains(&("placeholder-text".to_string(), "Search apps".to_string())));
    r.act("search", Op::SetText("app 01".into())).unwrap();
    assert_eq!(r.l.field.text(), "app 01");
    assert_eq!(r.l.shown, crate::apps::filter(&r.l.apps, "app 01"), "the results follow the text like typing");
    let t = r.node("search").text.clone().unwrap();
    assert_eq!((t.visible(), t.caret, t.selection), ("app 01".to_string(), 6, None));
    r.act("search", Op::SetSelection(Some((0, 3)))).unwrap();
    assert_eq!(r.node("search").text.as_ref().unwrap().selection, Some((0, 3)));
    r.act("search", Op::SetCaret(2)).unwrap();
    let t = r.node("search").text.clone().unwrap();
    assert_eq!((t.caret, t.selection), (2, None));
    r.act("search", Op::InsertText { offset: 6, text: "x".into() }).unwrap();
    assert_eq!(r.l.field.text(), "app 01x");
    r.act("search", Op::DeleteText { start: 0, end: 7, cut: false }).unwrap();
    assert_eq!(r.l.field.text(), "");
    assert_eq!(r.l.shown.len(), 40, "emptying the field shows every app again");
    assert!(matches!(r.act("search", Op::SetCaret(3)), Err(ActionError::Invalid(_))), "an offset past the end is refused by the host's validation");
}

#[test]
fn editing_the_search_through_accessibility_equals_typing_in_every_style() {
    for style in sty::STYLES {
        let mut r = Rig::new(styled_launcher(style, 40));
        r.act("search", Op::SetText("kilo".into())).unwrap();
        let mut typed = open(style, 40, SCREEN);
        type_into(&mut typed, "kilo");
        let st = r.l.styled.as_ref().unwrap();
        assert_eq!((st.m.query(), st.m.zone, &st.m.results), (typed.m.query(), typed.m.zone, &typed.m.results), "{style:?}");
        assert_eq!((st.m.sel("results"), st.m.offset("results")), (typed.m.sel("results"), typed.m.offset("results")), "{style:?}");
        assert_eq!(r.node("status").name, format!("{} results", st.m.results.len()), "{style:?}");
        let t = r.node("search").text.clone().unwrap();
        assert_eq!((t.visible(), t.caret), ("kilo".to_string(), 4), "{style:?}");
        r.act("search", Op::SetText(String::new())).unwrap();
        assert!(r.find("status").is_none(), "{style:?}: no status without a search");
    }
}

#[test]
fn an_empty_search_is_announced_once_and_typing_is_not() {
    let mut r = Rig::new(launchpad(40));
    r.type_text("app");
    r.redraw();
    assert_eq!(r.node("status").name, "40 results");
    assert!(r.find("announce:empty").is_none(), "results are counted on the status line, not announced");
    r.type_text("zzzz");
    r.redraw();
    assert_eq!(r.node("status").name, "No matching apps");
    let note = r.node("announce:empty");
    assert_eq!((note.role, note.name.as_str()), (Role::Notification, "No matching apps"));
    for _ in 0..4 {
        r.l.key(&key(Key::Backspace, ""));
    }
    r.redraw();
    assert!(r.find("announce:empty").is_none(), "gone once results are back, so the next empty search announces again");
}

// ------------------------------------------------------------------------------------------------ focus and selection

#[test]
fn the_focus_is_the_search_field_while_typing_and_the_selected_app_while_navigating() {
    let mut r = Rig::new(launchpad(40));
    assert_eq!(r.focus_key(), Some("search"));
    r.l.key(&key(Key::Down, ""));
    r.redraw();
    assert_eq!(r.focus_key(), Some("apps/app:app-007"));
    assert_eq!(r.selected(), ["apps/app:app-007"], "exactly one row is selected");
    r.type_text("app");
    r.redraw();
    assert_eq!(r.focus_key(), Some("search"));
    let first = format!("apps/app:{}", app_keys(&r.l.apps)[r.l.shown[0]]);
    assert_eq!(r.selected(), [first.as_str()], "typing resets the selection to the first result");
}

#[test]
fn accessible_focus_and_choice_move_the_launchers_own_selection_without_launching() {
    let mut r = Rig::new(launchpad(40));
    r.act("apps/app:app-011", Op::Focus).unwrap();
    assert_eq!(r.l.sel, 11);
    assert_eq!(r.focus_key(), Some("apps/app:app-011"));
    let at = r.node("zone:apps").children.iter().position(|c| r.tree.get(*c).unwrap().key == "apps/app:app-013").unwrap();
    let out = r.act("zone:apps", Op::SelectChild(at)).unwrap();
    assert_eq!((r.l.sel, out.action), (13, None));
    assert!(spawned().is_empty());
}

#[test]
fn every_style_reports_the_focused_result_as_selected() {
    for style in sty::STYLES {
        let mut r = Rig::new(styled_launcher(style, 40));
        r.act("search", Op::SetText("kilo".into())).unwrap();
        assert_eq!(r.focus_key(), Some("search"), "{style:?}");
        r.l.key(&key(Key::Down, ""));
        r.redraw();
        let focus = r.tree.focus.and_then(|f| r.tree.get(f)).unwrap_or_else(|| panic!("{style:?}: a focus"));
        assert_ne!(focus.key, "search", "{style:?}: Down moved off the field");
        if focus.role == Role::ListItem {
            assert!(focus.states.contains(State::Selected), "{style:?}: {}", focus.key);
            assert_eq!(r.selected().len(), 1, "{style:?}");
        }
    }
}

#[test]
fn note_key_follows_where_the_keyboard_acts() {
    let mut l = styled_launcher(LauncherStyle::Windows, 40);
    assert_eq!(l.a11y.area, Area::Search);
    l.key(&key(Key::Down, ""));
    assert_eq!(l.a11y.area, Area::List);
    l.key(&key(Key::Char, "k"));
    assert_eq!(l.a11y.area, Area::Search);
    l.key(&key(Key::Tab, ""));
    assert_eq!(l.a11y.area, Area::List);
    l.key(&key(Key::Escape, "")); // closes the launcher; no change of area
    assert_eq!(l.a11y.area, Area::List);
    let mut dux = classic(LauncherStyle::Dux, 30, (620, 620));
    dux.key(&key(Key::Down, ""));
    assert_eq!(dux.a11y.area, Area::List);
    dux.key(&key(Key::Left, ""));
    assert_eq!(dux.a11y.area, Area::Search, "in Dux the sideways arrows move the caret");
}

// ------------------------------------------------------------------------------------------------ real layouts

/// The keys the zones' visible items should have, and those of their offpage items.
fn zone_keys(l: &Launcher) -> (HashSet<String>, HashSet<String>) {
    let st = l.styled.as_ref().unwrap();
    let ids = app_keys(&st.m.cat.apps);
    let (mut visible, mut offpage) = (HashSet::new(), HashSet::new());
    for z in &st.zones {
        let offset = st.m.offset(z.id);
        let shown = z.visible(offset);
        for (pos, &t) in z.items.iter().enumerate() {
            let Some(k) = target_key(z.id, t, &ids, Some(st)) else { continue };
            let on = matches!(z.shape, Shape::Fixed(_)) || shown.contains(&pos);
            if on {
                visible.insert(k);
            } else {
                offpage.insert(k);
            }
        }
    }
    (visible, offpage)
}

#[test]
fn every_style_describes_the_zones_it_really_laid_out() {
    for style in sty::STYLES {
        for query in ["", "kilo"] {
            let mut st = open(style, 60, SCREEN);
            type_into(&mut st, query);
            let r = Rig::new(wrap(st));
            let ctx = format!("{style:?} {query:?}");
            let (visible, offpage) = zone_keys(&r.l);
            let keys: HashSet<String> = r.tree.nodes().iter().map(|n| n.key.clone()).collect();
            for k in &visible {
                assert!(keys.contains(k), "{ctx}: visible item {k} is described");
            }
            for k in offpage.difference(&visible) {
                assert!(!keys.contains(k), "{ctx}: offpage item {k} must not be described");
            }
            // geometry: each item node sits exactly where its zone put it, inside the surface
            let st = r.l.styled.as_ref().unwrap();
            let ids = app_keys(&st.m.cat.apps);
            for z in &st.zones {
                for (pos, &t) in z.items.iter().enumerate() {
                    let (Some(k), Some(rect)) = (target_key(z.id, t, &ids, Some(st)), z.rect(pos, st.m.offset(z.id))) else { continue };
                    let Some(n) = r.find(&k) else { continue };
                    assert_eq!(n.bounds, Some(prect(&rect)), "{ctx}: {k}");
                    let (x, y, w, h) = n.bounds.unwrap();
                    assert!(x >= 0 && y >= 0 && x + w <= SCREEN.0 as i32 && y + h <= SCREEN.1 as i32, "{ctx}: {k} {:?}", n.bounds);
                    assert!(n.states.contains(State::Showing), "{ctx}: {k}");
                }
            }
            assert!(r.find("frame").is_some() && r.find("search").is_some(), "{ctx}");
            // lists and tab strips are described as such, with the roles a screen reader navigates by
            let roles: HashSet<Role> = r.tree.nodes().iter().map(|n| n.role).collect();
            assert!(roles.contains(&Role::List), "{ctx}");
            assert!(r.tree.nodes().iter().all(|n| n.role != Role::PushButton || n.actions.iter().any(|a| a.name == "click")), "{ctx}: every button can be clicked");
        }
    }
}

#[test]
fn styled_tabs_pages_and_buttons_have_roles_names_and_a_current_choice() {
    // Xfce: categories as tabs, Favorites current at the start
    let r = Rig::new(styled_launcher(LauncherStyle::Xfce, 40));
    assert_eq!(r.node("zone:cats").role, Role::PageTabList);
    let fav = r.node("cats/view:favorites");
    assert_eq!((fav.role, fav.name.as_str()), (Role::PageTab, "Favorites"));
    assert!(fav.states.contains(State::Selected) && !r.node("cats/view:all").states.contains(State::Selected));
    let settings = r.node("actions/settings");
    assert_eq!((settings.role, settings.name.as_str()), (Role::PushButton, "Settings"));
    // GNOME: page dots are tabs, the first page is current
    let r = Rig::new(styled_launcher(LauncherStyle::Gnome, 80));
    assert_eq!(r.node("zone:pages").role, Role::PageTabList);
    assert!(r.node("pages/page:0").states.contains(State::Selected) && r.node("pages/page:0").name == "Page 1");
    // Windows: the best match appears with its Open and Pin buttons while searching
    let mut r = Rig::new(styled_launcher(LauncherStyle::Windows, 40));
    r.act("search", Op::SetText("kilo".into())).unwrap();
    let open = r.tree.nodes().iter().find(|n| n.key.starts_with("detail/open:")).expect("the Open button").clone();
    assert_eq!((open.role, open.name.starts_with("Open Kilo")), (Role::PushButton, true));
    assert!(r.tree.nodes().iter().any(|n| n.key.starts_with("detail/pin:") && n.name.contains("favorites")));
}

#[test]
fn views_pages_and_buttons_act_like_a_click_on_them() {
    for style in sty::STYLES {
        for query in ["", "kilo"] {
            let setup = || {
                // (Kickoff's Places data comes from here, never from the disk)
                styled::kde_install_places(Vec::new(), Vec::new());
                let mut st = open(style, 40, SCREEN);
                type_into(&mut st, query);
                st
            };
            let probe = wrap(setup());
            let ids = app_keys(probe.app_list());
            let es = entries(&probe, &ids);
            let mut tried = 0;
            // (apps are covered by the launch tests; they would start a process here)
            for e in es.iter().filter(|e| !matches!(e.target, Tgt::App(_) | Tgt::Letter(_))) {
                let ctx = format!("{style:?} {query:?} {}", e.key);
                // Kickoff's "press again" arming is per thread: the real click goes first and the adapter's launcher is
                // built after it, whose layout ends the arming, so each starts from the same unarmed state
                let mut b = setup();
                let (cx, cy) = (e.rect.0 as f32 + e.rect.2 as f32 / 2.0, e.rect.1 as f32 + e.rect.3 as f32 / 2.0);
                assert_eq!(b.hit(cx, cy).map(|h| (h.0, h.1)), Some((e.zone, e.pos)), "{ctx}: a click at its centre reaches it");
                let real = b.button(cx, cy, Button::Left, true);
                let mut a = wrap(setup());
                let ours = activate_entry(&mut a, e).unwrap_or_else(|err| panic!("{ctx}: {err}"));
                assert_eq!(ours, real, "{ctx}: same response");
                assert_eq!(snapshot(a.styled.as_ref().unwrap()), snapshot(&b), "{ctx}: same state");
                tried += 1;
            }
            assert!(tried > 0, "{style:?} {query:?}: some control was compared");
        }
    }
}

fn snapshot(st: &Styled) -> String {
    let lists = ["apps", "results", "fav", "cats", "pages", "menu", "footer"].map(|id| (st.m.sel(id), st.m.offset(id)));
    let ui = ["kde.tab", "kde.menu", "kde.place"].map(|k| st.m.ui(k));
    format!("{:?} {:?} {} {lists:?} {ui:?} {:?} {} {:?}", st.m.view, st.m.query(), st.m.zone, st.m.cat.favorites, st.m.expanded, st.m.results)
}

#[test]
fn choosing_a_tab_shows_it_and_choosing_a_list_row_only_selects_it() {
    let mut r = Rig::new(styled_launcher(LauncherStyle::Xfce, 40));
    let at = r.l.a11y.lists["zone:cats"].children.iter().position(|k| k == "cats/view:all").unwrap();
    r.act("zone:cats", Op::SelectChild(at)).unwrap();
    assert_eq!(r.l.styled.as_ref().unwrap().m.view, View::All);
    assert!(r.node("cats/view:all").states.contains(State::Selected) && !r.node("cats/view:favorites").states.contains(State::Selected));
    let out = r.act("zone:apps", Op::SelectChild(3)).unwrap();
    assert_eq!(out.action, None, "selecting a row does not launch it");
    assert_eq!(r.l.styled.as_ref().unwrap().m.sel("apps"), 3);
    assert_eq!(r.l.styled.as_ref().unwrap().m.cat.recent.len(), 0);
}

#[test]
fn pinning_and_unpinning_use_the_catalog_and_keep_every_node() {
    let mut r = Rig::new(styled_launcher(LauncherStyle::Gnome, 40));
    let target = list_items(&r).into_iter().find(|n| n.key.starts_with("apps/app:") && n.actions.iter().any(|a| a.name == "pin")).expect("an app that is not pinned").clone();
    let before: Vec<(String, crate::a11y::NodeId)> = list_items(&r).iter().map(|n| (n.key.clone(), n.id)).collect();
    let id = target.key.trim_start_matches("apps/app:").to_string();
    let index = |r: &Rig| r.l.styled.as_ref().unwrap().m.cat.apps.iter().position(|a| app_id(a) == id).unwrap();
    let i = index(&r);
    r.act(&target.key, Op::Action("pin")).unwrap();
    assert!(r.l.styled.as_ref().unwrap().m.cat.is_favorite(i));
    assert!(r.node(&target.key).actions.iter().any(|a| a.name == "unpin") && !r.node(&target.key).actions.iter().any(|a| a.name == "pin"));
    for (k, nid) in &before {
        assert_eq!(r.find(k).map(|n| n.id), Some(*nid), "{k} keeps its node when the favourites change");
    }
    // a request that raced a pin made from the keyboard is refused instead of undoing it
    r.l.styled.as_mut().unwrap().m.cat.toggle_favorite(i);
    r.redraw();
    r.l.styled.as_mut().unwrap().m.cat.toggle_favorite(i);
    let out = r.act(&target.key, Op::Action("pin"));
    assert!(matches!(out, Err(ActionError::Refused(_))), "{out:?}");
    assert!(r.l.styled.as_ref().unwrap().m.cat.is_favorite(i));
    r.act(&target.key, Op::Action("unpin")).unwrap();
    assert!(!r.l.styled.as_ref().unwrap().m.cat.is_favorite(i));
}

#[test]
fn the_launchpad_and_dux_offer_no_pinning() {
    for style in [LauncherStyle::Default, LauncherStyle::Dux] {
        let r = Rig::new(classic(style, 30, if style == LauncherStyle::Dux { (620, 620) } else { (1000, 680) }));
        assert!(list_items(&r).iter().all(|n| n.actions.iter().all(|a| a.name == "activate")), "{style:?}");
    }
}

// ------------------------------------------------------------------------------------------------ Dux

#[test]
fn dux_describes_its_visible_rows_clear_button_and_scrolling() {
    let mut r = Rig::new(classic(LauncherStyle::Dux, 30, (620, 620)));
    let layout = r.l.dux.as_ref().unwrap().layout;
    assert_eq!(list_items(&r).len(), layout.rows, "the rows that fit");
    let (x, y, w, h) = layout.cell(0, 0);
    assert_eq!(r.node("apps/app:app-000").bounds, Some(ints(x, y, w, h)));
    assert_eq!(r.node("apps/app:app-000").description, "About App 000");
    assert!(r.find("search:clear").is_none());
    assert_eq!(r.node("search").bounds, Some(ints(12.0, 12.0, layout.width - 24.0, 48.0)));
    r.type_text("app");
    r.redraw();
    let clear = r.node("search:clear");
    assert_eq!((clear.role, clear.bounds), (Role::PushButton, Some(ints(layout.width - 52.0, 12.0, 40.0, 48.0))));
    r.act("search:clear", Op::Action("click")).unwrap();
    assert_eq!(r.l.field.text(), "");
    assert!(r.find("search:clear").is_none());
    r.l.key(&key(Key::PageDown, ""));
    r.redraw();
    assert!(r.l.scroll > 0 && r.find("apps/app:app-000").is_none(), "a row scrolled away is gone");
    assert_eq!(list_items(&r).len(), layout.rows);
}

#[test]
fn launchpad_bounds_follow_the_overlay_card() {
    let mut l = classic(LauncherStyle::Default, 30, (1000, 680));
    l.set_overlay(true);
    l.resize(3440, 1440);
    let r = Rig::new(l);
    let (ox, oy) = (r.l.origin.0 as i32, r.l.origin.1 as i32);
    let (x, y, w, _) = r.node("search").bounds.unwrap();
    assert_eq!((x, y, w), (ox + PAD as i32, oy + SEARCH_Y as i32, r.l.size.0 as i32 - 2 * PAD as i32));
    let (cx, cy, cw, ch) = r.l.cell_rect(0);
    assert_eq!(r.node("apps/app:app-000").bounds, Some(ints(cx + r.l.origin.0, cy + r.l.origin.1, cw, ch)));
    assert_eq!(r.node("frame").bounds, Some((0, 0, 3440, 1440)));
}

// ------------------------------------------------------------------------------------------------ Kickoff's own controls

fn entry(name: &str, detail: &str, open: &[&str]) -> styled::Entry {
    styled::Entry { name: name.into(), detail: detail.into(), icon: "folder".into(), open: open.iter().map(|s| s.to_string()).collect(), at: 0 }
}

/// Kickoff the size of the original capture, with fixture Places data (nothing is read from the disk).
fn kickoff() -> Rig {
    styled::kde_install_places(vec![entry("Home", "/home/t", &["xdg-open", "/home/t"]), entry("Nothing", "cannot open", &[])], vec![entry("notes.txt", "~/Documents", &["xdg-open", "/n"])]);
    let mut st = sty::styled(LauncherStyle::Kde, 40);
    st.full = false;
    st.scale = 1.0;
    st.resize(655, 516);
    Rig::new(wrap(st))
}

fn kde_state(r: &Rig) -> &Styled {
    r.l.styled.as_ref().unwrap()
}

fn click() -> Op {
    Op::Action("click")
}

#[test]
fn kickoffs_footer_tabs_system_buttons_and_session_button_are_described() {
    let r = kickoff();
    assert_eq!(r.node("zone:footer").role, Role::Panel);
    assert_eq!(r.node("zone:footer/tabs").role, Role::PageTabList);
    let apps = r.node("footer/tab:applications");
    let places = r.node("footer/tab:places");
    assert_eq!((apps.role, apps.name.as_str(), places.name.as_str()), (Role::PageTab, "Applications", "Places"));
    assert!(apps.states.contains(State::Selected) && !places.states.contains(State::Selected), "Applications is the current tab");
    for (key, name) in [("footer/sys:sleep", "Sleep"), ("footer/sys:restart", "Restart"), ("footer/sys:shut-down", "Shut Down")] {
        let n = r.node(key);
        assert_eq!((n.role, n.name.as_str()), (Role::PushButton, name), "{key}");
        assert!(n.enabled() && n.actions.iter().any(|a| a.name == "click") && n.bounds.is_some(), "{key}");
    }
    assert_eq!(r.node("footer/sys:restart").description, "Asks to confirm");
    let session = r.node("footer/session");
    assert!(session.states.contains(State::HasPopup) && session.states.contains(State::Collapsed) && !session.states.contains(State::Expanded));
    // the header: the avatar opens the account settings, the gear the launcher's settings
    assert_eq!(r.node("actions/head:account").name, "Account settings");
    assert_eq!(r.node("actions/head:configure").name, "Configure launcher");
}

#[test]
fn kickoffs_tabs_switch_through_its_own_dispatcher_and_places_rows_open() {
    let mut r = kickoff();
    r.act("footer/tab:places", click()).unwrap();
    assert_eq!(kde_state(&r).m.ui("kde.tab"), 1);
    assert!(r.node("footer/tab:places").states.contains(State::Selected) && !r.node("footer/tab:applications").states.contains(State::Selected));
    // the sidebar now lists the places as tabs, Computer current
    assert_eq!(r.node("zone:cats").role, Role::PageTabList);
    assert!(r.node("cats/place:computer").states.contains(State::Selected));
    assert_eq!(r.node("cats/place:frequently-used").name, "Frequently Used");
    assert_eq!(r.node("zone:apps").name, "Places");
    // Computer: Show KRunner, Home, and a row that cannot open anything
    let rows: Vec<(String, bool)> = list_items(&r).iter().map(|n| (n.name.clone(), n.enabled())).collect();
    assert_eq!(rows, [("Show KRunner".to_string(), true), ("Home".to_string(), true), ("Nothing".to_string(), false)]);
    let nothing = list_items(&r).into_iter().find(|n| n.name == "Nothing").unwrap().key.clone();
    assert_eq!(r.act(&nothing, click()).unwrap_err(), ActionError::Disabled);
    let home = list_items(&r).into_iter().find(|n| n.name == "Home").unwrap().key.clone();
    assert_eq!(r.node(&home).description, "/home/t");
    let out = r.act(&home, click()).unwrap();
    assert_eq!(out.action, Some(Action::Close));
    assert_eq!(styled::kde_effects().0, [vec!["xdg-open".to_string(), "/home/t".to_string()]]);
}

#[test]
fn kickoffs_places_sidebar_selects_through_the_tab_list_and_history_documents_open() {
    let mut r = kickoff();
    r.act("footer/tab:places", click()).unwrap();
    let at = r.l.a11y.lists["zone:cats"].children.iter().position(|k| k == "cats/place:history").unwrap();
    r.act("zone:cats", Op::SelectChild(at)).unwrap();
    assert_eq!(kde_state(&r).m.ui("kde.place"), 1);
    assert!(r.node("cats/place:history").states.contains(State::Selected) && !r.node("cats/place:computer").states.contains(State::Selected));
    let doc = list_items(&r).into_iter().find(|n| n.name == "notes.txt").expect("the history lists the document").key.clone();
    assert!(doc.starts_with("apps/document:notes-txt-"), "{doc}");
    assert_eq!(r.node(&doc).description, "~/Documents");
    assert_eq!(r.act(&doc, click()).unwrap().action, Some(Action::Close));
    assert_eq!(styled::kde_effects().0, [vec!["xdg-open".to_string(), "/n".to_string()]]);
}

#[test]
fn kickoffs_header_buttons_start_the_settings_panes() {
    let mut r = kickoff();
    assert_eq!(r.act("actions/head:account", click()).unwrap().action, Some(Action::Close));
    let mut r2 = kickoff();
    assert_eq!(r2.act("actions/head:configure", click()).unwrap().action, Some(Action::Close));
    let opened = styled::kde_effects().0;
    assert_eq!(opened.iter().map(|c| c.last().cloned().unwrap_or_default()).collect::<Vec<_>>(), ["users", "launcher"]);
}

#[test]
fn kickoffs_leaving_actions_run_through_its_dispatcher_with_the_second_press() {
    let mut r = kickoff();
    // Sleep acts at once
    assert_eq!(r.act("footer/sys:sleep", click()).unwrap().action, Some(Action::Close));
    assert_eq!(styled::kde_effects().1, ["suspend"]);
    // Restart asks first: the caption and the description say so, nothing ran
    let mut r = kickoff();
    let first = r.act("footer/sys:restart", click()).unwrap();
    assert_eq!(first.action, None);
    assert!(styled::kde_effects().1.is_empty());
    let n = r.node("footer/sys:restart");
    assert_eq!((n.name.as_str(), n.description.as_str()), ("Restart?", "Press again to confirm"));
    assert_eq!(r.act("footer/sys:restart", click()).unwrap().action, Some(Action::Close));
    assert_eq!(styled::kde_effects().1, ["reboot"]);
}

#[test]
fn kickoffs_session_menu_is_modal_exposes_its_rows_and_gates_everything_else() {
    let mut r = kickoff();
    r.act("footer/session", click()).unwrap();
    assert!(has_menu(kde_state(&r)), "the Session menu is open");
    let menu = r.node("zone:menu");
    assert!(menu.role == Role::Menu && menu.states.contains(State::Modal) && menu.selection);
    let rows: Vec<(&str, Role)> = menu.children.iter().map(|c| r.tree.get(*c).unwrap()).map(|n| (n.name.as_str(), n.role)).collect();
    assert_eq!(rows, [("Lock", Role::MenuItem), ("Log Out", Role::MenuItem)], "the actions that do not fit the footer");
    let session = r.node("footer/session");
    assert!(session.states.contains(State::Expanded) && !session.states.contains(State::Collapsed));
    assert_eq!(r.focus_key(), Some("menu/menu:lock"), "the first row has the keyboard");
    // everything unrelated is refused while the menu is open
    let app = list_items(&r).into_iter().find(|n| n.key.starts_with("apps/app:")).map(|n| n.key.clone()).expect("an app is showing");
    assert!(matches!(r.act(&app, Op::Action("activate")), Err(ActionError::Disabled)));
    assert!(matches!(r.act(&app, Op::Focus), Err(ActionError::Disabled)));
    assert!(matches!(r.act("search", Op::SetText("x".into())), Err(ActionError::Disabled)));
    assert!(matches!(r.act("footer/tab:places", click()), Err(ActionError::Disabled)));
    assert!(matches!(r.act("footer/sys:sleep", click()), Err(ActionError::Disabled)));
    assert!(matches!(r.act("zone:apps", Op::SelectChild(0)), Err(ActionError::Disabled)));
    assert!(styled::kde_effects().1.is_empty());
    for key in [&app, "search", "footer/tab:places", "footer/sys:sleep", "zone:apps"] {
        let node = r.node(key);
        assert!(!node.states.contains(State::Enabled), "{key} is obscured by the modal menu");
        assert!(node.actions.is_empty(), "{key} must not advertise an action");
    }
    assert!(r.node("footer/session").states.contains(State::Enabled));
    assert!(r.node("menu/menu:lock").states.contains(State::Enabled));
    // its rows work: choosing one by the menu's selection, or clicking it
    let out = r.act("menu/menu:lock", click()).unwrap();
    assert_eq!(out.action, Some(Action::Close));
    assert_eq!(styled::kde_effects().1, ["lock-screen"]);
}

#[test]
fn kickoffs_session_button_closes_its_menu_and_ordinary_actions_return() {
    let mut r = kickoff();
    r.act("footer/session", click()).unwrap();
    r.act("footer/session", click()).unwrap();
    assert!(!has_menu(kde_state(&r)) && r.find("zone:menu").is_none());
    let app = list_items(&r).into_iter().find(|n| n.key.starts_with("apps/app:")).map(|n| n.key.clone()).unwrap();
    assert!(r.act(&app, Op::Focus).is_ok());
}

#[test]
fn kickoffs_menu_rows_confirm_leaving_actions_and_the_menu_selection_runs_them() {
    let mut r = kickoff();
    r.act("footer/session", click()).unwrap();
    let at = r.l.a11y.lists["zone:menu"].children.iter().position(|k| k == "menu/menu:log-out").unwrap();
    // choosing a row of the open menu is a click on it: Log Out asks first
    assert_eq!(r.act("zone:menu", Op::SelectChild(at)).unwrap().action, None);
    assert_eq!(r.node("menu/menu:log-out").name, "Log Out?");
    assert!(styled::kde_effects().1.is_empty());
    assert_eq!(r.act("zone:menu", Op::SelectChild(at)).unwrap().action, Some(Action::Close));
    assert_eq!(styled::kde_effects().1, ["logout"]);
}

#[test]
fn kickoffs_places_loading_is_a_busy_status_and_one_announcement() {
    let (hold, gate) = std::sync::mpsc::channel::<()>();
    let mut st = sty::styled(LauncherStyle::Kde, 40);
    st.full = false;
    st.scale = 1.0;
    st.m.ui.insert("kde.tab", 1);
    styled::kde_hold_places(gate);
    st.resize(655, 516);
    let mut r = Rig::new(wrap(st));
    let status = r.node("status");
    assert_eq!((status.role, status.name.as_str()), (Role::StatusBar, "Reading places..."));
    assert!(status.states.contains(State::Busy));
    assert_eq!(r.node("announce:loading").role, Role::Notification);
    assert!(r.act("status", Op::Focus).is_err(), "a status line takes no action");
    drop(hold);
}

/// Every control a style owns is described, whatever the style, query and tab: none is silently left out.
#[test]
fn no_style_owned_control_goes_undescribed_in_any_style() {
    styled::kde_install_places(vec![entry("Home", "/home/t", &["xdg-open", "/home/t"])], vec![entry("notes.txt", "~/Documents", &["xdg-open", "/n"])]);
    for style in sty::STYLES {
        let mut scenarios: Vec<(String, Styled)> = Vec::new();
        for query in ["", "kilo"] {
            let mut st = open(style, 60, SCREEN);
            type_into(&mut st, query);
            scenarios.push((format!("{style:?} {query:?}"), st));
        }
        if style == LauncherStyle::Kde {
            // the Places tab (each place), and a card narrow enough that every leaving action sits behind Session, menu open
            for place in 0..3 {
                let mut st = open(style, 60, SCREEN);
                st.m.ui.insert("kde.tab", 1);
                st.m.ui.insert("kde.place", place);
                st.relayout();
                scenarios.push((format!("Kde places {place}"), st));
            }
            let mut st = open(style, 60, (500, 600));
            st.m.ui.insert("kde.menu", 1);
            st.relayout();
            scenarios.push(("Kde narrow with the menu".into(), st));
        }
        for (ctx, st) in scenarios {
            let styles_in_zones: Vec<(&str, Tgt)> = st.zones.iter().flat_map(|z| z.items.iter().map(move |t| (z.id, *t))).filter(|(_, t)| matches!(t, Tgt::Style(..))).collect();
            for (zone, t) in &styles_in_zones {
                let sem = st.semantics(*t).unwrap_or_else(|| panic!("{ctx}: {t:?} in {zone} has no description: the adapter is incomplete for this style"));
                assert!(!sem.name.is_empty() && !sem.key.is_empty(), "{ctx}: {t:?}");
            }
            let r = Rig::new(wrap(st));
            let (visible, _) = zone_keys(&r.l);
            let keys: HashSet<String> = r.tree.nodes().iter().map(|n| n.key.clone()).collect();
            for k in &visible {
                assert!(keys.contains(k), "{ctx}: {k} is in the tree");
            }
            if style == LauncherStyle::Kde {
                assert!(!styles_in_zones.is_empty(), "{ctx}: Kickoff has controls of its own");
            }
        }
    }
}

#[test]
fn search_focus_or_activation_never_confirms_an_armed_system_action() {
    for focus_first in [false, true] {
        let mut r = kickoff();
        r.act("footer/sys:restart", click()).unwrap();
        assert_eq!(r.node("footer/sys:restart").name, "Restart?");
        assert!(styled::kde_effects().1.is_empty());
        if focus_first {
            r.act("search", Op::Focus).unwrap();
        }
        assert!(matches!(r.act("search", Op::Action("activate")), Err(ActionError::Refused(_))));
        assert!(styled::kde_effects().1.is_empty(), "search activation must never execute reboot");
        // Returning to Restart asks again; only its explicit second press confirms.
        r.act("footer/sys:restart", click()).unwrap();
        assert!(styled::kde_effects().1.is_empty());
        r.act("footer/sys:restart", click()).unwrap();
        assert_eq!(styled::kde_effects().1, ["reboot"]);
    }
}

#[test]
fn setting_accessible_search_text_uses_single_line_keyboard_filtering() {
    for style in [LauncherStyle::Default, LauncherStyle::Kde, LauncherStyle::Windows] {
        let l = if style == LauncherStyle::Default { launchpad(3) } else { styled_launcher(style, 3) };
        let mut r = Rig::new(l);
        r.act("search", Op::SetText("a\nb\tc\r".into())).unwrap();
        let text = r.l.styled.as_ref().map(|st| st.m.field.text()).unwrap_or_else(|| r.l.field.text());
        assert_eq!(text, "abc");
    }
}
