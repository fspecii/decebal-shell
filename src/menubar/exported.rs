//! The bar side of exported application menus ([`crate::appmenu`]): turns the focused window's
//! real menu into bar titles after the shell's own menus, and sends clicks back with the
//! exporter's item ids. Without an exported menu the app menu carries one honest, inert row
//! saying why; no File/Edit/View entries are ever made up.

use super::menus::{self, Item, Menu};
use crate::appmenu::{self, view, AppMenus, Focus, MenuToken};
use std::sync::Arc;
use std::time::Duration;

/// Key of bar menus that came from an application.
pub const KEY: &str = "exported";

/// One bar menu backed by an exported submenu.
#[derive(Debug, Clone, PartialEq)]
struct Slot {
    token: MenuToken,
    title: i32,
    /// The submenu currently listed (the title, or a nested submenu after drilling down).
    parent: i32,
    /// Rows as listed; a drilled-down list starts with a "back" row (`None`).
    rows: Vec<Option<view::Row>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// The row was sent to the application; close the menu.
    Sent,
    /// The dropdown now lists another submenu; keep it open.
    Moved,
    /// Inert row (disabled, separator, information).
    Ignored,
}

#[derive(Default)]
pub struct Exported {
    handle: Option<AppMenus>,
    snap: appmenu::Snapshot,
    /// Index-aligned with the bar's menus.
    slots: Vec<Option<Slot>>,
    /// Drill-down path inside the open exported menu: (bar menu index, submenu ids below the title).
    path: Option<(usize, Vec<i32>)>,
    open: Option<usize>,
    /// Last `ItemActivationRequested` acted on.
    request_seq: u64,
}

impl Exported {
    /// Starts or stops the worker. Disabling joins it briefly and releases its bus connection.
    pub fn set_enabled(&mut self, enabled: bool, config: impl FnOnce() -> appmenu::Config, notify: impl FnOnce() -> appmenu::Notify) {
        match (enabled, self.handle.is_some()) {
            (true, false) => match AppMenus::start(config(), notify()) {
                Ok(h) => self.handle = Some(h),
                Err(e) => eprintln!("decbar: application menus unavailable: {e}"),
            },
            (false, true) => {
                if let Some(mut h) = self.handle.take() {
                    h.stop(Duration::from_millis(200));
                }
            }
            _ => {}
        }
    }

    pub fn enabled(&self) -> bool {
        self.handle.is_some()
    }

    pub fn set_focus(&mut self, focus: Focus) {
        if let Some(h) = self.handle.as_mut() {
            h.set_focus(focus);
        }
    }

    /// Picks up a new worker snapshot; true when the bar's menus must be rebuilt.
    pub fn poll(&mut self) -> bool {
        let snap = match &self.handle {
            Some(h) => h.snapshot(),
            None => appmenu::Snapshot::default(),
        };
        if snap.generation == self.snap.generation && self.handle.is_some() {
            return false;
        }
        let changed = snap.state != self.snap.state;
        if changed && self.snap.state.exported().map(|e| e.token) != snap.state.exported().map(|e| e.token) {
            self.path = None;
        }
        self.snap = snap;
        changed
    }

    /// The shell's menus with the exported ones (or the reason there are none) spliced in.
    pub fn compose(&mut self, app: &str, light: bool) -> Vec<Menu> {
        let mut out = menus::build(app, light);
        self.slots = vec![None; out.len()];
        if !self.enabled() {
            return out;
        }
        let Some(e) = self.snap.state.exported().cloned() else {
            if let (Some(why), Some(app_menu)) = (self.snap.state.reason(), out.iter_mut().find(|m| m.key == "app")) {
                app_menu.items.splice(0..0, [info(&why), sep()]);
            }
            return out;
        };
        let at = out.iter().position(|m| m.key == "app").map_or(out.len(), |i| i + 1);
        let mut menus = Vec::new();
        let mut slots = Vec::new();
        for (n, t) in view::titles(&e.tree).into_iter().enumerate() {
            let index = at + n;
            let below: Vec<i32> = self.path.as_ref().filter(|(i, _)| *i == index).map(|(_, p)| p.clone()).unwrap_or_default();
            let parent = below.last().copied().unwrap_or(t.id);
            let mut rows: Vec<Option<view::Row>> = Vec::new();
            if !below.is_empty() {
                rows.push(None);
            }
            if t.submenu {
                rows.extend(view::rows(&e.tree, parent).unwrap_or_default().into_iter().map(Some));
            } else {
                // A top-level command: its dropdown is the command itself.
                rows.extend(view::rows(&e.tree, e.tree.root.id).unwrap_or_default().into_iter().filter(|r| r.id == t.id).map(Some));
            }
            let back = below.len().checked_sub(2).and_then(|i| e.tree.find(below[i])).map(|i| i.display_label()).unwrap_or_else(|| t.label.clone());
            let items = rows
                .iter()
                .map(|r| match r {
                    None => Item { label: format!("‹ {back}"), action: "", shortcut: "", separator: false },
                    Some(r) if r.separator => Item { label: String::new(), action: "", shortcut: "", separator: true },
                    Some(r) => Item { label: if r.mark().is_empty() { r.label.clone() } else { format!("{} {}", r.mark(), r.label) }, action: "", shortcut: "", separator: false },
                })
                .collect();
            menus.push(Menu { key: KEY, title: t.label.clone(), icon: false, bold: false, items });
            slots.push(Some(Slot { token: e.token, title: t.id, parent, rows }));
        }
        out.splice(at..at, menus);
        self.slots.splice(at..at, slots);
        out
    }

    fn row(&self, menu: usize, item: usize) -> Option<&Option<view::Row>> {
        self.slots.get(menu)?.as_ref()?.rows.get(item)
    }

    pub fn is_exported(&self, menu: usize) -> bool {
        self.slots.get(menu).is_some_and(Option::is_some)
    }

    /// Whether an exported row reacts to clicks (`None`: not an exported menu).
    pub fn available(&self, menu: usize, item: usize) -> Option<bool> {
        self.is_exported(menu).then(|| matches!(self.row(menu, item), Some(None)) || self.row(menu, item).and_then(Option::as_ref).is_some_and(|r| r.enabled && !r.separator))
    }

    /// Right-hand text of an exported row: its shortcut, or `▸` for a submenu.
    pub fn shortcut(&self, menu: usize, item: usize) -> Option<String> {
        let r = self.row(menu, item)?.as_ref()?;
        Some(if r.submenu { "▸".into() } else { r.shortcut.clone() })
    }

    /// The bar opened menu `index` (possibly by sliding from another one).
    pub fn opened(&mut self, index: usize) {
        if self.open == Some(index) {
            return;
        }
        self.closed();
        self.open = Some(index);
        if let (Some(h), Some(Some(s))) = (&self.handle, self.slots.get(index)) {
            h.opened(s.token, s.parent);
        }
    }

    /// The open menu closed; drill-down state resets.
    pub fn closed(&mut self) {
        if let Some(i) = self.open.take() {
            if let (Some(h), Some(Some(s))) = (&self.handle, self.slots.get(i)) {
                h.closed(s.token, s.parent);
                if s.parent != s.title {
                    h.closed(s.token, s.title);
                }
            }
        }
        self.path = None;
    }

    /// A click on row `item` of bar menu `menu` (`None` if that menu is not exported).
    pub fn click(&mut self, menu: usize, item: usize, timestamp: u32) -> Option<Click> {
        let slot = self.slots.get(menu)?.clone()?;
        let Some(h) = &self.handle else { return Some(Click::Ignored) };
        Some(match slot.rows.get(item) {
            Some(None) => {
                if let Some((_, p)) = self.path.as_mut() {
                    p.pop();
                }
                h.closed(slot.token, slot.parent);
                Click::Moved
            }
            Some(Some(r)) if r.enabled && r.submenu => {
                let (_, p) = self.path.get_or_insert_with(|| (menu, Vec::new()));
                if p.len() < 16 {
                    p.push(r.id);
                }
                h.opened(slot.token, r.id);
                Click::Moved
            }
            Some(Some(r)) if r.activatable() => {
                h.activate(slot.token, r.id, timestamp);
                Click::Sent
            }
            _ => Click::Ignored,
        })
    }

    /// The bar menu to open because the application asked (`ItemActivationRequested`, e.g. Alt+F
    /// inside a Qt window), once per request.
    pub fn take_request(&mut self) -> Option<usize> {
        let r = self.snap.activation_request?;
        if r.seq <= self.request_seq {
            return None;
        }
        self.request_seq = r.seq;
        let e = self.snap.state.exported()?;
        if e.token != r.token {
            return None;
        }
        let title = e.tree.top_level_of(r.id)?;
        self.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.token == r.token && s.title == title))
    }

    /// Identity of bar menu `menu` when it is exported: the same app menu keeps it across rebuilds.
    pub fn identity(&self, menu: usize) -> Option<(MenuToken, i32)> {
        self.slots.get(menu)?.as_ref().map(|s| (s.token, s.title))
    }

    #[cfg(test)]
    pub(super) fn handle(&self) -> Option<&AppMenus> {
        self.handle.as_ref()
    }
}

fn info(text: &str) -> Item {
    Item { label: text.trim_end_matches('.').to_string(), action: "", shortcut: "", separator: false }
}

fn sep() -> Item {
    Item { label: String::new(), action: "", shortcut: "", separator: true }
}

/// The notifier the worker calls: wake the bar loop.
pub fn notifier(waker: Option<crate::app::Waker>) -> appmenu::Notify {
    Arc::new(move || {
        if let Some(w) = &waker {
            w.wake();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::active::ActiveWindow;
    use super::super::{DecBar, Open};
    use super::KEY;
    use crate::app::{App, Button};
    use crate::appmenu::testbus::{n, Fake, PrivateBus, MENU_PATH, N, P};
    use crate::appmenu::{Config, Focus, RegistrarRole};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const WINDOW: u32 = 0x0051_0001;

    struct Focused;
    impl ActiveWindow for Focused {
        fn active_app_name(&self) -> Option<String> {
            Some("fixture".into())
        }
        fn active_focus(&self) -> Focus {
            Focus::X11 { window: WINDOW, app_id: "fixture".into() }
        }
    }

    fn layout() -> N {
        n(
            0,
            vec![],
            vec![
                n(
                    1,
                    vec![("label", P::S("_File")), ("children-display", P::S("submenu"))],
                    vec![
                        n(10, vec![("label", P::S("_New Window")), ("shortcut", P::Keys(vec![vec!["Control", "n"]]))], vec![]),
                        n(11, vec![("type", P::S("separator"))], vec![]),
                        n(12, vec![("label", P::S("Disabled")), ("enabled", P::B(false))], vec![]),
                        n(14, vec![("label", P::S("Word Wrap")), ("toggle-type", P::S("checkmark")), ("toggle-state", P::I(1))], vec![]),
                        n(16, vec![("label", P::S("Recent")), ("children-display", P::S("submenu"))], vec![n(160, vec![("label", P::S("notes.txt"))], vec![])]),
                    ],
                ),
                n(2, vec![("label", P::S("_Edit")), ("children-display", P::S("submenu"))], vec![n(20, vec![("label", P::S("Undo"))], vec![])]),
            ],
        )
    }

    fn settle(bar: &mut DecBar, what: &str, f: impl Fn(&DecBar) -> bool) {
        let end = Instant::now() + Duration::from_secs(6);
        loop {
            bar.refresh_app_name();
            if f(bar) {
                return;
            }
            assert!(Instant::now() < end, "timed out: {what}; menus {:?}", bar.menus.iter().map(|m| (m.key, m.title.clone())).collect::<Vec<_>>());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn keys(bar: &DecBar) -> Vec<&'static str> {
        bar.menus.iter().map(|m| m.key).collect()
    }

    fn click_row(bar: &mut DecBar, menu: usize, label: &str) -> bool {
        let d = bar.dropdown(menu).unwrap();
        let i = bar.menus[menu].items.iter().position(|it| it.label == label).unwrap_or_else(|| panic!("no row {label}"));
        let y = d.rows[i].y + 5.0;
        bar.pointer_button(d.x + 20.0, y, Button::Left)
    }

    #[test]
    fn bar_clicks_reach_the_exporter_with_real_item_ids() {
        let bus = PrivateBus::start();
        let mut bar = DecBar::new(Box::new(Focused));
        bar.width = 1920;
        let address = bus.address.clone();
        bar.app_menu.set_enabled(true, || Config::with_address(address), || Arc::new(|| {}));
        settle(&mut bar, "registrar", |b| b.app_menu.handle().is_some_and(|h| h.snapshot().registrar == RegistrarRole::Owned));
        // Nothing exported yet: no invented menus, and the app menu says why.
        settle(&mut bar, "honest reason", |b| b.menus.iter().find(|m| m.key == "app").is_some_and(|m| m.items[0].label == "This application does not export a menu"));
        assert_eq!(keys(&bar), ["apple", "app", "window", "help"]);

        let app = Fake::connect(&bus.address, layout());
        app.register(WINDOW, MENU_PATH);
        settle(&mut bar, "exported titles", |b| keys(b) == ["apple", "app", KEY, KEY, "window", "help"]);
        assert_eq!((bar.menus[2].title.as_str(), bar.menus[3].title.as_str()), ("File", "Edit"));
        assert_eq!(bar.menus[2].items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["New Window", "", "Disabled", "✓ Word Wrap", "Recent"]);
        assert_eq!(bar.app_menu.shortcut(2, 0).as_deref(), Some("⌃N"));
        assert_eq!(bar.app_menu.shortcut(2, 4).as_deref(), Some("▸"));

        // Clicking the title opens it and tells the application (AboutToShow on the File id).
        let file = bar.spans()[2];
        assert!(bar.pointer_button(file.x + 3.0, 10.0, Button::Left));
        assert_eq!(bar.open, Open::Menu(2));
        assert_eq!(app.wait_records("AboutToShow", 1)[0].id, 1);
        // A disabled row is inert and keeps the menu open.
        assert!(!click_row(&mut bar, 2, "Disabled"));
        assert_eq!(bar.open, Open::Menu(2));
        // A real row is sent with its exported id and closes the menu.
        assert!(click_row(&mut bar, 2, "New Window"));
        assert_eq!(bar.open, Open::None);
        let clicks = || app.records("Event").into_iter().filter(|r| r.event == "clicked").map(|r| r.id).collect::<Vec<_>>();
        let end = Instant::now() + Duration::from_secs(5);
        while clicks().is_empty() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(clicks(), [10]);

        // Submenus drill down inside the dropdown; "‹ File" goes back.
        bar.pointer_button(file.x + 3.0, 10.0, Button::Left);
        assert!(click_row(&mut bar, 2, "Recent"));
        assert_eq!(bar.open, Open::Menu(2));
        assert_eq!(bar.menus[2].items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["‹ File", "notes.txt"]);
        assert!(click_row(&mut bar, 2, "‹ File"));
        assert_eq!(bar.menus[2].items[0].label, "New Window");
        assert!(click_row(&mut bar, 2, "Recent"));
        assert!(click_row(&mut bar, 2, "notes.txt"));
        let end = Instant::now() + Duration::from_secs(5);
        while clicks().len() < 2 {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(clicks(), [10, 160]);
        assert!(app.records("Event").iter().any(|r| r.event == "opened" && r.id == 16), "nested submenu was announced");

        // Sliding to Edit while open switches menus and closes File on the application side.
        bar.pointer_button(file.x + 3.0, 10.0, Button::Left);
        let edit = bar.spans()[3];
        bar.pointer_motion(edit.x + 3.0, 10.0);
        assert_eq!(bar.open, Open::Menu(3));
        let end = Instant::now() + Duration::from_secs(5);
        while !app.records("Event").iter().any(|r| r.event == "closed" && r.id == 1) {
            assert!(Instant::now() < end, "File was not closed: {:?}", app.records("Event"));
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut pm = tiny_skia::Pixmap::new(1920, 400).unwrap();
        bar.draw_dropdown(&mut pm, 3);

        // The application asks for its Edit menu (as Qt does for Alt+E): the bar opens it.
        bar.close();
        let mut w = crate::dbus::Writer::default();
        w.u32(20);
        w.u32(0);
        app.send(crate::dbus::Message::signal(MENU_PATH, crate::appmenu::DBUSMENU, "ItemActivationRequested", "iu", w.buf));
        settle(&mut bar, "requested menu opened", |b| b.open == Open::Menu(3));
        bar.close();
        bar.refresh_app_name();
        assert_eq!(bar.open, Open::None, "a request opens the menu once");

        // The application leaving takes its menus with it.
        app.quit();
        settle(&mut bar, "menus removed", |b| keys(b) == ["apple", "app", "window", "help"]);
        assert_eq!(bar.open, Open::None, "the open exported menu closed with its app");
        bar.app_menu.set_enabled(false, || unreachable!(), || unreachable!());
        assert!(bar.app_menu.handle().is_none());
        bar.refresh_app_name();
        assert_eq!(bar.menus.iter().find(|m| m.key == "app").unwrap().items[0].label, format!("About {}", bar.app_name), "disabled: no reason row");
    }

    #[test]
    fn the_folded_app_menu_popup_reaches_the_exporter_too() {
        use super::super::applets::tests::{fixture, reset};
        use super::super::popup::Act;
        use super::super::Tray;
        use crate::panel::{AppletId, AppletSet, Edge, Renderer};
        let bus = PrivateBus::start();
        let mut bar = fixture(Renderer::MenuBar, Edge::Left, AppletSet::of(&[AppletId::AppMenu]), 900);
        bar.active = Box::new(Focused);
        let address = bus.address.clone();
        bar.app_menu.set_enabled(true, || Config::with_address(address), || Arc::new(|| {}));
        settle(&mut bar, "registrar", |b| b.app_menu.handle().is_some_and(|h| h.snapshot().registrar == RegistrarRole::Owned));
        settle(&mut bar, "honest reason", |b| b.menus.iter().find(|m| m.key == "app").is_some_and(|m| m.items[0].label == "This application does not export a menu"));
        // The app menu's page shows the reason as a disabled row.
        bar.toggle_popup(Tray::AppMenu);
        let app_index = bar.menus.iter().position(|m| m.key == "app").unwrap();
        bar.run_act(Act::MenuOpen(app_index));
        let rows = bar.popup.clone().unwrap().rows;
        let why = rows.iter().find(|r| r.label == "This application does not export a menu").expect("reason row");
        assert!(!why.enabled);
        bar.close();

        let app = Fake::connect(&bus.address, layout());
        app.register(WINDOW, MENU_PATH);
        settle(&mut bar, "exported titles", |b| keys(b) == ["apple", "app", KEY, KEY, "window", "help"]);
        bar.toggle_popup(Tray::AppMenu);
        let rows = bar.popup.clone().unwrap().rows;
        assert!(rows.iter().any(|r| r.label == "File" && r.act == Act::MenuOpen(2)), "{:?}", rows.iter().map(|r| &r.label).collect::<Vec<_>>());
        bar.run_act(Act::MenuOpen(2));
        assert_eq!(app.wait_records("AboutToShow", 1)[0].id, 1);
        let rows = bar.popup.clone().unwrap().rows;
        let new = rows.iter().find(|r| r.label == "New Window").expect("exported row");
        assert!(new.enabled && new.detail == "⌃N", "{new:?}");
        assert!(!rows.iter().find(|r| r.label == "Disabled").unwrap().enabled);
        let recent = rows.iter().find(|r| r.label == "Recent").unwrap();
        assert_eq!(recent.detail, "▸");
        // Drilling into a nested submenu keeps the popup on the page; its row is then sent with its id.
        bar.run_act(recent.act.clone());
        let rows = bar.popup.clone().expect("popup stays open").rows;
        let notes = rows.iter().find(|r| r.label == "notes.txt").expect("nested row").act.clone();
        bar.run_act(notes);
        let end = Instant::now() + Duration::from_secs(5);
        while !app.records("Event").iter().any(|r| r.event == "clicked" && r.id == 160) {
            assert!(Instant::now() < end, "{:?}", app.records("Event"));
            std::thread::sleep(Duration::from_millis(5));
        }
        // Going back from a page closes that menu on the application side.
        bar.toggle_popup(Tray::AppMenu);
        bar.run_act(Act::MenuOpen(3));
        bar.run_act(Act::MenuBack);
        let end = Instant::now() + Duration::from_secs(5);
        while !app.records("Event").iter().any(|r| r.event == "closed" && r.id == 2) {
            assert!(Instant::now() < end, "{:?}", app.records("Event"));
            std::thread::sleep(Duration::from_millis(5));
        }
        reset();
    }
}
