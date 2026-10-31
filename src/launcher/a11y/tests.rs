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

#[test]
fn an_open_session_menu_gates_every_other_action() {
    // narrow enough that every system action sits behind the Session button
    let mut st = open(LauncherStyle::Kde, 40, (500, 600));
    st.m.ui.insert("kde.menu", 1);
    st.relayout();
    assert!(has_menu(&st), "the Session menu is open");
    let mut r = Rig::new(wrap(st));
    let menu = r.node("menu");
    assert!(menu.states.contains(State::Modal) && menu.role == Role::Menu);
    assert_eq!(r.focus_key(), Some("menu"));
    let item = list_items(&r).into_iter().next().map(|n| n.key.clone()).expect("an app is showing");
    assert!(matches!(r.act(&item, Op::Action("activate")), Err(ActionError::Refused(_))));
    assert!(matches!(r.act("search", Op::SetText("x".into())), Err(ActionError::Refused(_))));
    let st = r.l.styled.as_mut().unwrap();
    st.m.ui.insert("kde.menu", 0);
    st.relayout();
    r.redraw();
    assert!(r.find("menu").is_none());
    assert!(r.act(&item, Op::Focus).is_ok(), "ordinary actions work again once it closes");
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
            let Some(k) = target_key(z.id, t, &ids) else { continue };
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
                    let (Some(k), Some(rect)) = (target_key(z.id, t, &ids), z.rect(pos, st.m.offset(z.id))) else { continue };
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
                let mut a = wrap(setup());
                let mut b = setup();
                let (cx, cy) = (e.rect.0 as f32 + e.rect.2 as f32 / 2.0, e.rect.1 as f32 + e.rect.3 as f32 / 2.0);
                assert_eq!(b.hit(cx, cy).map(|h| (h.0, h.1)), Some((e.zone, e.pos)), "{ctx}: a click at its centre reaches it");
                let real = b.button(cx, cy, Button::Left, true);
                let ours = activate_entry(&mut a, e).unwrap_or_else(|err| panic!("{ctx}: {err}"));
                assert_eq!(ours, real, "{ctx}: same response");
                assert_eq!(snapshot(a.styled.as_ref().unwrap()), snapshot(&b), "{ctx}: same state");
                tried += 1;
            }
            assert!(tried > 0 || style == LauncherStyle::Kde, "{style:?} {query:?}: some control was compared");
        }
    }
}

fn snapshot(st: &Styled) -> String {
    let lists = ["apps", "results", "fav", "cats", "pages"].map(|id| (st.m.sel(id), st.m.offset(id)));
    format!("{:?} {:?} {} {lists:?} {:?} {} {:?}", st.m.view, st.m.query(), st.m.zone, st.m.cat.favorites, st.m.expanded, st.m.results)
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
