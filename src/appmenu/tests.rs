//! The real worker against a private `dbus-daemon` and a scriptable exporter/registrar.

use super::testbus::{n, Fake, Mode, PrivateBus, MENU_PATH, N, P};
use super::*;
use crate::dbus::{Value, Writer};
use std::collections::HashMap;

fn wait(m: &AppMenus, what: &str, f: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let end = Instant::now() + Duration::from_secs(6);
    loop {
        let s = m.snapshot();
        if f(&s) {
            return s;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}: {s:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ready(m: &AppMenus) -> Arc<Exported> {
    wait(m, "ready menu", |s| s.state.exported().is_some()).state.exported().unwrap().clone()
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
                    n(13, vec![("label", P::S("Hidden")), ("visible", P::B(false))], vec![]),
                    n(14, vec![("label", P::S("Word Wrap")), ("toggle-type", P::S("checkmark")), ("toggle-state", P::I(1))], vec![]),
                    n(15, vec![("label", P::S("Small")), ("toggle-type", P::S("radio")), ("toggle-state", P::I(0))], vec![]),
                    n(16, vec![("label", P::S("Recent")), ("children-display", P::S("submenu"))], vec![n(160, vec![("label", P::S("notes.txt"))], vec![])]),
                ],
            ),
            n(2, vec![("label", P::S("_Edit")), ("children-display", P::S("submenu"))], vec![n(20, vec![("label", P::S("Undo"))], vec![])]),
        ],
    )
}

fn start(bus: &PrivateBus) -> AppMenus {
    let mut cfg = Config::with_address(bus.address.clone());
    cfg.limits.call_timeout = Duration::from_millis(600);
    AppMenus::start(cfg, Arc::new(|| {})).unwrap()
}

fn owner_of(fake: &Fake, name: &str) -> Option<String> {
    let r = fake.call(bus::call(bus::BUS, bus::BUS_PATH, bus::BUS, "GetNameOwner", "s", bus::strings(&[name])));
    (r.kind == crate::dbus::RETURN).then(|| r.args().unwrap()[0].as_str().unwrap().to_string())
}

#[test]
fn owns_an_absent_registrar_serves_registrations_and_forwards_real_item_ids() {
    let bus = PrivateBus::start();
    let mut menus = start(&bus);
    wait(&menus, "registrar owned", |s| s.registrar == RegistrarRole::Owned);
    let app = Fake::connect(&bus.address, layout());
    assert_eq!(app.register(0x0040_0001, MENU_PATH).kind, crate::dbus::RETURN);
    // The registrar answers other clients like any registrar would.
    let mut w = Writer::default();
    w.u32(0x0040_0001);
    let r = app.call(bus::call(registrar::NAME, registrar::PATH, registrar::IFACE, "GetMenuForWindow", "u", w.buf));
    assert_eq!(r.args().unwrap(), vec![Value::Str(app.unique.clone()), Value::Path(MENU_PATH.into())]);
    let r = app.call(bus::call(registrar::NAME, registrar::PATH, registrar::IFACE, "GetMenus", "", vec![]));
    assert_eq!(r.sig, "a(uso)");

    menus.set_focus(Focus::X11 { window: 0x0040_0001, app_id: "fixture".into() });
    let e = ready(&menus);
    assert_eq!((e.window, e.owner.as_str(), e.path.as_str()), (0x0040_0001, app.unique.as_str(), MENU_PATH));
    let titles = view::titles(&e.tree);
    assert_eq!(titles.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["File", "Edit"]);
    let rows = view::rows(&e.tree, 1).unwrap();
    assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [10, 11, 12, 14, 15, 16]);
    assert_eq!(rows[0].shortcut, "⌃N");

    // Opening a menu runs AboutToShow + "opened" with the real submenu id.
    assert!(menus.opened(e.token, 1));
    app.wait_records("AboutToShow", 1);
    // Plain, enabled items are forwarded with their ids and timestamp; everything else is refused.
    for id in [11, 12, 13, 16, 999, 0, 1] {
        assert!(menus.activate(e.token, id, 5));
    }
    assert!(menus.activate(e.token, 160, 77));
    assert!(menus.activate(e.token, 10, 42));
    let clicks: Vec<(i32, u32)> = app.wait_records("Event", 3).into_iter().filter(|r| r.event == "clicked").map(|r| (r.id, r.timestamp)).collect();
    wait(&menus, "click acknowledged", |s| s.events_delivered == 2);
    assert_eq!(clicks, [(160, 77), (10, 42)]);
    assert!(app.records("Event").iter().any(|r| r.event == "opened" && r.id == 1));
    assert!(app.records("Event").iter().all(|r| r.sender == app.records("Event")[0].sender));

    // The application leaving the bus drops its registration and its menu.
    let unique = app.unique.clone();
    app.quit();
    wait(&menus, "menu gone with its owner", |s| s.state == State::NotExported { window: 0x0040_0001 });
    let probe = Fake::connect(&bus.address, layout());
    let mut w = Writer::default();
    w.u32(0x0040_0001);
    let r = probe.call(bus::call(registrar::NAME, registrar::PATH, registrar::IFACE, "GetMenuForWindow", "u", w.buf));
    assert_eq!(r.args().unwrap()[0].as_str(), Some(""), "{unique} must be forgotten");
    assert!(menus.stop(Duration::from_secs(3)));
    assert_eq!(menus.snapshot().state, State::Stopped);
    // Stopping released the name.
    let end = Instant::now() + Duration::from_secs(3);
    while owner_of(&probe, registrar::NAME).is_some() {
        assert!(Instant::now() < end, "registrar name not released");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn observes_an_existing_registrar_and_never_takes_its_name() {
    let bus = PrivateBus::start();
    let app = Fake::connect(&bus.address, layout());
    let reg = Fake::connect(&bus.address, layout());
    assert_eq!(reg.request_name(registrar::NAME, 0x4), 1);
    let mut table = HashMap::new();
    table.insert(0x77u32, (app.unique.clone(), MENU_PATH.to_string()));
    reg.serve_registrar(table);
    let mut cfg = Config::with_address(bus.address.clone());
    cfg.limits.call_timeout = Duration::from_millis(600);
    let mut menus = AppMenus::start(cfg, Arc::new(|| {})).unwrap();
    wait(&menus, "observing", |s| s.registrar == RegistrarRole::Observing { owner: reg.unique.clone() });
    assert_eq!(owner_of(&app, registrar::NAME).as_deref(), Some(reg.unique.as_str()));
    menus.set_focus(Focus::X11 { window: 0x77, app_id: "a".into() });
    let e = ready(&menus);
    assert_eq!(e.owner, app.unique);
    assert_eq!(reg.wait_records("GetMenuForWindow", 1)[0].id, 0x77);
    // A window the registrar does not know.
    menus.set_focus(Focus::X11 { window: 0x78, app_id: "b".into() });
    wait(&menus, "not exported", |s| s.state == State::NotExported { window: 0x78 });
    // ... until the registrar announces it.
    let mut w = Writer::default();
    w.u32(0x78);
    w.string(&app.unique);
    w.string(MENU_PATH);
    reg.send(crate::dbus::Message::signal(registrar::PATH, registrar::IFACE, "WindowRegistered", "uso", w.buf));
    assert_eq!(ready(&menus).window, 0x78);
    // A spoofed registrar signal from someone else is ignored.
    let mut w = Writer::default();
    w.u32(0x78);
    app.send(crate::dbus::Message::signal(registrar::PATH, registrar::IFACE, "WindowUnregistered", "u", w.buf));
    std::thread::sleep(Duration::from_millis(100));
    assert!(menus.snapshot().state.exported().is_some());
    // When the external registrar leaves, this worker takes over the free name.
    reg.quit();
    wait(&menus, "took over", |s| s.registrar == RegistrarRole::Owned);
    assert!(menus.stop(Duration::from_secs(3)));
}

#[test]
fn a_replacing_registrar_takes_the_name_back_and_is_observed() {
    let bus = PrivateBus::start();
    let menus = start(&bus);
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    let real = Fake::connect(&bus.address, layout());
    // DBUS_NAME_FLAG_REPLACE_EXISTING | DO_NOT_QUEUE: allowed because we asked with ALLOW_REPLACEMENT.
    assert_eq!(real.request_name(registrar::NAME, 0x2 | 0x4), 1);
    wait(&menus, "observing the replacement", |s| s.registrar == RegistrarRole::Observing { owner: real.unique.clone() });
}

#[test]
fn observe_only_without_a_registrar_is_honestly_unavailable() {
    let bus = PrivateBus::start();
    let mut cfg = Config::with_address(bus.address.clone());
    cfg.policy = RegistrarPolicy::ObserveOnly;
    let mut menus = AppMenus::start(cfg, Arc::new(|| {})).unwrap();
    wait(&menus, "absent", |s| s.registrar == RegistrarRole::Absent);
    menus.set_focus(Focus::X11 { window: 5, app_id: String::new() });
    let s = wait(&menus, "unavailable", |s| matches!(s.state, State::Unavailable(Unavailable::NoRegistrar(_))));
    assert!(s.state.reason().unwrap().contains("no menu registrar"));
    let probe = Fake::connect(&bus.address, layout());
    assert!(owner_of(&probe, registrar::NAME).is_none());
}

#[test]
fn layout_and_property_updates_publish_new_immutable_trees() {
    let bus = PrivateBus::start();
    let mut menus = start(&bus);
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    let app = Fake::connect(&bus.address, layout());
    app.register(9, MENU_PATH);
    menus.set_focus(Focus::X11 { window: 9, app_id: String::new() });
    let first = ready(&menus);
    app.props_updated(12, vec![("enabled", P::B(true)), ("label", P::S("Now enabled"))], vec![]);
    let s = wait(&menus, "property update", |s| s.state.exported().is_some_and(|e| e.tree.find(12).is_some_and(|i| i.enabled)));
    let second = s.state.exported().unwrap().clone();
    assert_eq!(second.tree.find(12).unwrap().label, "Now enabled");
    assert!(!first.tree.find(12).unwrap().enabled, "earlier snapshots are not mutated");
    assert_eq!(first.token, second.token, "property updates keep the menu token");
    // A storm of LayoutUpdated signals is coalesced into few fetches.
    let before = app.records("GetLayout").len();
    let mut next = layout();
    next.children.push(n(3, vec![("label", P::S("_View")), ("children-display", P::S("submenu"))], vec![n(30, vec![("label", P::S("Zoom"))], vec![])]));
    app.set_layout(next);
    for r in 0..200 {
        app.layout_updated(r);
    }
    let s = wait(&menus, "new layout", |s| s.state.exported().is_some_and(|e| e.tree.find(30).is_some()));
    std::thread::sleep(Duration::from_millis(150));
    let fetches = app.records("GetLayout").len() - before;
    assert!((1..=12).contains(&fetches), "{fetches} fetches for 200 signals");
    assert!(menus.activate(s.state.exported().unwrap().token, 30, 1));
    assert_eq!(app.wait_records("Event", 1)[0].id, 30);
}

#[test]
fn stale_replies_and_tokens_never_cross_windows() {
    let bus = PrivateBus::start();
    let mut menus = start(&bus);
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    let slow = Fake::connect(&bus.address, layout());
    slow.set_mode(Mode::Slow(Duration::from_millis(300)));
    slow.register(1, MENU_PATH);
    let fast = Fake::connect(&bus.address, n(0, vec![], vec![n(5, vec![("label", P::S("Fast"))], vec![n(50, vec![("label", P::S("Go"))], vec![])])]));
    fast.register(2, MENU_PATH);
    menus.set_focus(Focus::X11 { window: 1, app_id: String::new() });
    slow.wait_records("GetLayout", 1);
    menus.set_focus(Focus::X11 { window: 2, app_id: String::new() });
    let e = ready(&menus);
    assert_eq!(e.window, 2);
    std::thread::sleep(Duration::from_millis(450));
    let s = menus.snapshot();
    assert_eq!(s.state.exported().map(|e| e.window), Some(2), "slow reply for the old window was dropped");
    // A click carrying the old window's token is ignored even though id 50 exists now.
    menus.set_focus(Focus::X11 { window: 1, app_id: String::new() });
    let old = e.token;
    let e1 = wait(&menus, "window 1", |s| s.state.exported().is_some_and(|e| e.window == 1)).state.exported().unwrap().clone();
    assert_ne!(e1.token, old);
    assert!(menus.activate(old, 50, 0));
    assert!(menus.activate(e1.token, 10, 3));
    assert_eq!(slow.wait_records("Event", 1)[0].id, 10);
    std::thread::sleep(Duration::from_millis(100));
    assert!(fast.records("Event").is_empty(), "stale token reached the other app");
}

#[test]
fn malformed_oversized_and_silent_exporters_fail_gracefully() {
    let bus = PrivateBus::start();
    let mut menus = start(&bus);
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    for (window, mode, needle) in [(31u32, Mode::WrongSig, "malformed"), (32, Mode::Huge, "too large"), (33, Mode::Silent, "did not answer")] {
        let app = Fake::connect(&bus.address, layout());
        app.set_mode(mode);
        app.register(window, MENU_PATH);
        menus.set_focus(Focus::X11 { window, app_id: String::new() });
        let s = wait(&menus, needle, |s| matches!(&s.state, State::Failed { window: w, .. } if *w == window));
        let State::Failed { reason, .. } = s.state else { unreachable!() };
        assert!(reason.contains(needle), "{reason}");
    }
    // Wrong-typed registrar calls are refused, not crashes.
    let app = Fake::connect(&bus.address, layout());
    let mut w = Writer::default();
    w.string("not a window id");
    let r = app.call(bus::call(registrar::NAME, registrar::PATH, registrar::IFACE, "RegisterWindow", "s", w.buf));
    assert_eq!(r.error, "org.freedesktop.DBus.Error.InvalidArgs");
    let mut w = Writer::default();
    w.u32(0);
    w.string("/x");
    assert_eq!(app.call(bus::call(registrar::NAME, registrar::PATH, registrar::IFACE, "RegisterWindow", "uo", w.buf)).error, "org.freedesktop.DBus.Error.InvalidArgs");
    // Another client cannot hijack a registered window.
    app.register(40, MENU_PATH);
    let other = Fake::connect(&bus.address, layout());
    assert_eq!(other.register(40, "/Evil").error, "org.freedesktop.DBus.Error.AccessDenied");
    // The worker is still healthy.
    menus.set_focus(Focus::X11 { window: 40, app_id: String::new() });
    assert_eq!(ready(&menus).owner, app.unique);
}

#[test]
fn reentrant_exporter_calls_during_a_click_do_not_deadlock() {
    let bus = PrivateBus::start();
    let mut menus = start(&bus);
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    let app = Fake::connect(&bus.address, layout());
    app.register(60, MENU_PATH);
    menus.set_focus(Focus::X11 { window: 60, app_id: String::new() });
    let e = ready(&menus);
    // While the click is in flight the app calls back into the registrar and changes its layout.
    assert!(menus.activate(e.token, 10, 1));
    app.layout_updated(9);
    let r = app.call(bus::call(registrar::NAME, registrar::PATH, registrar::IFACE, "GetMenus", "", vec![]));
    assert_eq!(r.kind, crate::dbus::RETURN);
    app.wait_records("Event", 1);
    wait(&menus, "click acknowledged", |s| s.events_delivered == 1);
    assert!(menus.stop(Duration::from_secs(3)));
}

#[test]
fn without_a_bus_or_on_wayland_the_state_says_why() {
    let mut cfg = Config::with_address("unix:path=/nonexistent/decebal-appmenu-test/bus");
    cfg.address = None;
    let mut none = AppMenus::start(cfg, Arc::new(|| {})).unwrap();
    wait(&none, "no bus", |s| matches!(s.state, State::Unavailable(Unavailable::NoBus(_))));
    assert!(none.stop(Duration::from_secs(2)));
    let mut bad = AppMenus::start(Config::with_address("unix:path=/nonexistent/decebal-appmenu-test/bus"), Arc::new(|| {})).unwrap();
    let s = wait(&bad, "no bus", |s| matches!(s.state, State::Unavailable(Unavailable::NoBus(_))));
    assert!(s.state.reason().unwrap().starts_with("Application menus are unavailable"));
    assert!(bad.stop(Duration::from_secs(2)), "backoff waits are interruptible");

    let bus = PrivateBus::start();
    let mut menus = start(&bus);
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    menus.set_focus(Focus::Wayland { app_id: "org.gnome.TextEditor".into() });
    let s = wait(&menus, "wayland", |s| s.state == State::Unavailable(Unavailable::Wayland));
    assert!(s.state.reason().unwrap().contains("Wayland"));
    menus.set_focus(Focus::None);
    wait(&menus, "no focus", |s| s.state == State::NoFocus);
}

#[test]
fn the_worker_reconnects_after_the_bus_restarts() {
    let mut bus = PrivateBus::start();
    let address = bus.address.clone();
    let mut cfg = Config::with_address(address.clone());
    cfg.limits.max_backoff = Duration::from_millis(300);
    let menus = AppMenus::start(cfg, Arc::new(|| {})).unwrap();
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    // Kill the daemon but keep the directory (and so the address) for the restart.
    let _ = bus.child_kill_for_restart();
    wait(&menus, "lost", |s| matches!(s.state, State::Unavailable(Unavailable::NoBus(_))));
    bus.restart();
    wait(&menus, "owned again", |s| s.registrar == RegistrarRole::Owned);
}

#[test]
fn focus_from_tracker_windows() {
    let w = crate::wm::Window { id: 0x1a0_0007, app_id: "kate".into(), ..Default::default() };
    assert_eq!(Focus::from_window(Some(&w), Protocol::X11), Focus::X11 { window: 0x1a0_0007, app_id: "kate".into() });
    assert_eq!(Focus::from_window(Some(&w), Protocol::Wayland), Focus::Wayland { app_id: "kate".into() });
    assert_eq!(Focus::from_window(None, Protocol::X11), Focus::None);
    let huge = crate::wm::Window { id: u64::from(u32::MAX) + 1, ..Default::default() };
    assert_eq!(Focus::from_window(Some(&huge), Protocol::X11), Focus::None, "not an X11 id");
}

fn request(app: &Fake, id: i32) {
    let mut w = Writer::default();
    w.u32(id as u32);
    w.u32(0);
    app.send(crate::dbus::Message::signal(MENU_PATH, DBUSMENU, "ItemActivationRequested", "iu", w.buf));
}

#[test]
fn tokens_and_requests_stay_unique_across_a_real_reconnect() {
    let mut bus = PrivateBus::start();
    let mut cfg = Config::with_address(bus.address.clone());
    cfg.limits.call_timeout = Duration::from_millis(600);
    cfg.limits.max_backoff = Duration::from_millis(300);
    let mut menus = AppMenus::start(cfg, Arc::new(|| {})).unwrap();
    wait(&menus, "owned", |s| s.registrar == RegistrarRole::Owned);
    let app = Fake::connect(&bus.address, layout());
    app.register(70, MENU_PATH);
    menus.set_focus(Focus::X11 { window: 70, app_id: String::new() });
    let before = ready(&menus);
    request(&app, 20);
    let r1 = wait(&menus, "first request", |s| s.activation_request.is_some()).activation_request.unwrap();
    assert!(menus.activate(before.token, 10, 1));
    app.wait_records("Event", 1);
    wait(&menus, "first click acknowledged", |s| s.events_delivered == 1);

    app.quit();
    bus.child_kill_for_restart().unwrap();
    wait(&menus, "connection lost", |s| matches!(s.state, State::Unavailable(Unavailable::NoBus(_))));
    bus.restart();
    wait(&menus, "owned again", |s| s.registrar == RegistrarRole::Owned);
    let app = Fake::connect(&bus.address, layout());
    app.register(70, MENU_PATH);
    let after = wait(&menus, "menu after reconnect", |s| s.state.exported().is_some_and(|e| e.window == 70)).state.exported().unwrap().clone();
    assert!(after.token.0 > before.token.0, "epochs keep increasing across connections: {:?} then {:?}", before.token, after.token);
    assert_eq!(menus.snapshot().events_delivered, 1, "the delivered count is not reset by a reconnect");

    // The same request again after the reconnect is a new request, not a replay of the first.
    request(&app, 20);
    let r2 = wait(&menus, "second request", |s| s.activation_request.is_some_and(|r| r.seq > r1.seq)).activation_request.unwrap();
    assert_eq!((r2.token, r2.id), (after.token, 20));

    // A click carrying the pre-reconnect token is stale even though the ids match again.
    assert!(menus.activate(before.token, 10, 2));
    assert!(menus.activate(after.token, 14, 3));
    app.wait_records("Event", 1);
    std::thread::sleep(Duration::from_millis(150));
    let clicks: Vec<(i32, u32)> = app.records("Event").iter().filter(|r| r.event == "clicked").map(|r| (r.id, r.timestamp)).collect();
    assert_eq!(clicks, [(14, 3)], "the stale-token click must not reach the new connection");
    wait(&menus, "second click acknowledged", |s| s.events_delivered == 2);
    assert!(menus.stop(Duration::from_secs(2)));
}

#[test]
fn stop_is_bounded_while_connecting_to_a_stalled_or_saturated_bus() {
    let dir = std::env::temp_dir().join(format!("decebal-appmenu-stall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Accepted by the kernel, never answered: the EXTERNAL exchange stalls.
    let silent = dir.join("silent");
    let _listener = std::os::unix::net::UnixListener::bind(&silent).unwrap();
    // A listener whose backlog is full: connect itself would wait.
    let saturated = dir.join("saturated");
    let _held = super::bus::tests::full_backlog(&saturated);
    for path in [&silent, &saturated] {
        let mut cfg = Config::with_address(format!("unix:path={}", path.display()));
        cfg.limits.connect_timeout = Duration::from_secs(60);
        let mut menus = AppMenus::start(cfg, Arc::new(|| {})).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(menus.snapshot().state, State::Starting, "{}: still connecting", path.display());
        let t = Instant::now();
        assert!(menus.stop(Duration::from_millis(800)), "{}: stop waited on the connect", path.display());
        assert!(t.elapsed() < Duration::from_millis(500), "{}: {:?}", path.display(), t.elapsed());
        assert_eq!(menus.snapshot().state, State::Stopped);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
