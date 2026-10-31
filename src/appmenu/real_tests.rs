//! Opt-in: the worker against real exporters — a Qt 6 `QMainWindow` (Qt's own `QDBusMenuBar`)
//! on a private Xvfb, focused through the real X11 `wm::Tracker`, and libdbusmenu-glib.
//! Run through `tests/e2e/appmenu-real.sh`, which owns the bus, the display and every path.

use super::*;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is set by tests/e2e/appmenu-real.sh"))
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn ready_for(m: &AppMenus, window: u32, what: &str, f: impl Fn(&Exported) -> bool) -> Arc<Exported> {
    wait_for(what, || m.snapshot().state.exported().filter(|e| e.window == window && f(e)).cloned())
}

fn lines(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p).unwrap_or_default().lines().map(str::to_string).collect()
}

fn id_of(tree: &MenuTree, parent: i32, label: &str) -> i32 {
    view::rows(tree, parent).unwrap().into_iter().find(|r| r.label == label).unwrap_or_else(|| panic!("no row {label:?} under {parent}")).id
}

fn title_id(tree: &MenuTree, label: &str) -> i32 {
    view::titles(tree).into_iter().find(|t| t.label == label).unwrap_or_else(|| panic!("no title {label:?}")).id
}

#[test]
#[ignore = "run tests/e2e/appmenu-real.sh; needs its private bus, Xvfb and fixtures"]
fn real_qt_and_libdbusmenu_exporters() {
    let root = PathBuf::from(env("DECEBAL_APPMENU_REAL_ROOT"));
    assert!(root.file_name().unwrap().to_string_lossy().starts_with("decebal-appmenu-real."), "not the script's private root");
    let address = env("DECEBAL_APPMENU_BUS");
    assert!(address.starts_with(&format!("unix:path={}", root.display())), "bus must be the private one");
    assert_eq!(std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref(), Some(address.as_str()));
    assert_eq!(env("DISPLAY"), env("DECEBAL_APPMENU_DISPLAY"));
    let number: u32 = env("DISPLAY").trim_start_matches(':').parse().expect("plain :N display");
    assert!(number >= 700, "private fixtures use reserved high display numbers only");
    assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
    assert!(env("XAUTHORITY").starts_with(&*root.to_string_lossy()), "the private display needs its own cookie");

    let mut menus = AppMenus::start(Config::with_address(address.clone()), Arc::new(|| {})).unwrap();
    wait_for("registrar", || (menus.snapshot().registrar == RegistrarRole::Owned).then_some(()));

    // ------------------------------------------------------------------ libdbusmenu-glib
    let (glib_record, glib_ready) = (root.join("dbusmenu.log"), root.join("dbusmenu.ready"));
    let glib_window = 0x05a0_0001u32;
    let _glib = Kill(
        Command::new("/usr/bin/python3")
            .arg("-I")
            .arg(env("DECEBAL_APPMENU_DBUSMENU_FIXTURE"))
            .args(["--window", &glib_window.to_string()])
            .arg("--record")
            .arg(&glib_record)
            .arg("--ready")
            .arg(&glib_ready)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let glib_unique = wait_for("libdbusmenu fixture", || std::fs::read_to_string(&glib_ready).ok().filter(|s| s.starts_with(':')));
    menus.set_focus(Focus::X11 { window: glib_window, app_id: "dbusmenu-fixture".into() });
    let e = ready_for(&menus, glib_window, "libdbusmenu menu", |e| !e.tree.is_empty());
    assert_eq!(e.owner, glib_unique);
    assert_eq!(view::titles(&e.tree).iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["File", "Help"]);
    let file = title_id(&e.tree, "File");
    let rows = view::rows(&e.tree, file).unwrap();
    assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["New", "", "Show Toolbar", "Disabled", "Recent"]);
    assert!(rows[1].separator && rows[2].toggle == ToggleType::Check && rows[2].state == ToggleState::On);
    assert!(!rows[3].enabled && rows[4].submenu);
    assert!(menus.opened(e.token, file));
    assert!(menus.activate(e.token, rows[3].id, 11), "queued, then refused as disabled");
    assert!(menus.activate(e.token, id_of(&e.tree, file, "New"), 12));
    let recent = id_of(&e.tree, file, "Recent");
    assert!(menus.activate(e.token, id_of(&e.tree, recent, "doc.txt"), 13));
    let got = wait_for("libdbusmenu activations", || Some(lines(&glib_record)).filter(|l| l.len() >= 2));
    assert_eq!(got.iter().map(|l| l.split(' ').nth(1).unwrap()).collect::<Vec<_>>(), ["new", "doc"], "{got:?}");
    assert_eq!(got[0], "activated new 12", "the click's timestamp reaches the app");

    // ------------------------------------------------------------------ Qt 6 on Xvfb
    let tracker = crate::wm::Tracker::start(|| {});
    assert_eq!(tracker_protocol(), Protocol::X11);
    let (qt_record, qt_ready) = (root.join("qt.log"), root.join("qt.ready"));
    let mut qt =
        Kill(Command::new(env("DECEBAL_APPMENU_QT_FIXTURE")).env("QT_QPA_PLATFORM", "xcb").env("APPMENU_RECORD", &qt_record).env("APPMENU_READY", &qt_ready).stdin(Stdio::null()).spawn().unwrap());
    let win: u32 = wait_for("Qt window", || std::fs::read_to_string(&qt_ready).ok().and_then(|s| s.trim().parse().ok()));
    // No window manager on the private display: publish the EWMH focus the way one would.
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    let (xc, screen) = x11rb::connect(None).unwrap();
    let root_win = xc.setup().roots[screen].root;
    let set = |prop: &str| {
        let atom = xc.intern_atom(false, prop.as_bytes()).unwrap().reply().unwrap().atom;
        xc.change_property32(PropMode::REPLACE, root_win, atom, AtomEnum::WINDOW, &[win]).unwrap();
        xc.flush().unwrap();
    };
    set("_NET_CLIENT_LIST");
    set("_NET_ACTIVE_WINDOW");
    // The connection stays open for the rest of the test (the real tracker reads these properties).
    let end = Instant::now() + Duration::from_secs(10);
    let active = loop {
        let snap = tracker.snapshot();
        if let Some(w) = snap.active().filter(|w| w.id == u64::from(win)) {
            break w.clone();
        }
        assert!(Instant::now() < end, "tracker never saw window 0x{win:x} focused: {:?}", snap.windows);
        std::thread::sleep(Duration::from_millis(20));
    };
    menus.set_focus(Focus::from_window(Some(&active), tracker_protocol()));
    let e = ready_for(&menus, win, "Qt menu", |e| view::titles(&e.tree).len() >= 2);
    assert_eq!(e.window, win, "Qt registered its real X11 window id");
    assert_eq!(view::titles(&e.tree).iter().map(|t| (t.label.as_str(), t.mnemonic)).collect::<Vec<_>>(), [("File", Some('f')), ("Edit", Some('e'))]);
    let file = title_id(&e.tree, "File");
    let rows = view::rows(&e.tree, file).unwrap();
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["Open…", "", "Word Wrap", "Small", "Large", "Recent"], "Hidden is not shown");
    assert_eq!(rows[0].shortcut, "⌃O");
    assert_eq!((rows[2].toggle, rows[2].state), (ToggleType::Check, ToggleState::On));
    assert_eq!((rows[3].toggle, rows[3].state, rows[4].state), (ToggleType::Radio, ToggleState::Off, ToggleState::On));
    assert!(rows[5].submenu);

    assert!(menus.opened(e.token, file));
    assert!(menus.activate(e.token, rows[0].id, 0));
    assert!(menus.activate(e.token, rows[2].id, 0));
    wait_for("Qt triggers", || Some(lines(&qt_record)).filter(|l| l.iter().filter(|x| x.starts_with("triggered")).count() >= 2));
    let trig: Vec<String> = lines(&qt_record).into_iter().filter(|l| l.starts_with("triggered")).collect();
    assert_eq!(trig, ["triggered open 0", "triggered wrap 0"]);
    // Qt pushes the toggled state back as a property update.
    ready_for(&menus, win, "Word Wrap unchecked", |e| e.tree.find(rows[2].id).is_some_and(|i| i.toggle_state == ToggleState::Off));

    // Opening Edit runs Qt's aboutToShow, which adds an item: the layout update brings it in.
    let edit = title_id(&e.tree, "Edit");
    let undo = id_of(&e.tree, edit, "Undo");
    assert!(menus.opened(e.token, edit));
    wait_for("aboutToShow", || lines(&qt_record).contains(&"aboutToShow edit".to_string()).then_some(()));
    let e2 = ready_for(&menus, win, "late item", |e| view::rows(&e.tree, edit).is_some_and(|r| r.iter().any(|r| r.label == "Late Item")));
    assert!(menus.activate(e2.token, undo, 0), "disabled Undo is refused by the worker");
    assert!(menus.activate(e2.token, id_of(&e2.tree, edit, "Late Item"), 0));
    assert!(menus.closed(e2.token, edit));
    wait_for("late trigger", || lines(&qt_record).contains(&"triggered late 0".to_string()).then_some(()));
    assert!(!lines(&qt_record).iter().any(|l| l.contains("undo")), "disabled item must not trigger");

    // Alt+F inside the Qt window (XTEST on the verified private display): Qt asks the global menu to
    // open File (`ItemActivationRequested`), the same request the bar turns into an open dropdown.
    let before = menus.snapshot().activation_request.map_or(0, |r| r.seq);
    let st = Command::new("xdotool").args(["windowfocus", "--sync", &win.to_string(), "key", "--clearmodifiers", "alt+f"]).status().unwrap();
    assert!(st.success());
    let req = wait_for("ItemActivationRequested", || menus.snapshot().activation_request.filter(|r| r.seq > before));
    let e3 = menus.snapshot().state.exported().unwrap().clone();
    assert_eq!(req.token, e3.token);
    assert_eq!(e3.tree.top_level_of(req.id), Some(file), "Qt asked for File (id {})", req.id);

    // The app exits: its registration and menu go with it.
    let _ = qt.0.kill();
    let _ = qt.0.wait();
    wait_for("Qt menu gone", || (menus.snapshot().state == State::NotExported { window: win }).then_some(()));
    assert!(menus.stop(Duration::from_secs(3)));
}
