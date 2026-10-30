//! Opt-in integration against tests/e2e/sni-fixture.py on an explicitly private bus.
use super::*;
use std::{path::PathBuf, thread};
fn wait(host: &Host, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        let s = host.snapshot();
        if predicate(&s) {
            return s;
        }
        assert!(Instant::now() < end, "backend wait timed out: {} items, generation {}, connected {}", s.items.len(), s.generation, s.connected);
        thread::sleep(Duration::from_millis(20));
    }
}
fn control(conn: &mut Conn, method: &str, sig: &str, body: Vec<u8>) {
    let m = conn.call_blocking(Message::call("org.decebal.TestSNI", "/TestControl", "org.decebal.TestSNI", method, sig, body)).unwrap();
    assert_eq!(m.kind, dbus::RETURN, "control {method}: {}", m.error);
}
fn update(conn: &mut Conn, index: u32, field: &str, value: &str) {
    let mut w = Writer::default();
    w.u32(index);
    w.string(field);
    w.string(value);
    control(conn, "Update", "iss", w.buf)
}
fn owner(conn: &mut Conn) -> String {
    let m = conn.call_blocking(Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner", "s", strings(&[WATCH]))).unwrap();
    m.args().unwrap()[0].as_str().unwrap().into()
}
fn events(root: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(root.join("events.jsonl")).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}
#[test]
#[ignore = "run tests/e2e/sni-backend.sh; never use a live session bus"]
fn private_bus_backend() {
    let root = PathBuf::from(std::env::var("DECEBAL_SNI_TEST_ROOT").expect("private fixture root"));
    assert!(root.to_string_lossy().starts_with("/tmp/decebal-sni-backend."));
    assert_eq!(std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(), format!("unix:path={}/bus", root.display()));
    let mut conn = Conn::session().unwrap();
    let initial_owner = (std::env::var("DECEBAL_SNI_WATCHER_MODE").as_deref() == Ok("external")).then(|| owner(&mut conn));
    let host = Host::start(Arc::new(|| {}));
    let s = wait(&host, |s| s.items.len() == 2);
    let watcher_owner = owner(&mut conn);
    if let Some(initial_owner) = initial_owner {
        assert_eq!(watcher_owner, initial_owner, "host stole external watcher");
    }
    let red = s.items.iter().find(|i| i.title == "Fixture red").unwrap().clone();
    let cyan = s.items.iter().find(|i| i.title == "Fixture cyan").unwrap();
    assert_eq!(&red.icon.as_ref().unwrap().data()[..4], &[240, 60, 90, 255]);
    assert_eq!(&cyan.icon.as_ref().unwrap().data()[..4], &[24, 199, 232, 255]);
    assert!(cyan.item_is_menu);
    assert!(!red.item_is_menu);
    let second = Host::start(Arc::new(|| {}));
    wait(&second, |s| s.items.len() == 2);
    assert_eq!(owner(&mut conn), watcher_owner, "second host stole watcher");
    update(&mut conn, 1, "title", "Backend changed");
    update(&mut conn, 1, "status", "NeedsAttention");
    update(&mut conn, 1, "color", "4fe187");
    wait(&host, |s| {
        s.items.iter().any(|i| i.key == red.key && i.title == "Backend changed" && i.status == Status::NeedsAttention && i.icon.as_ref().is_some_and(|p| p.data()[..4] == [79, 225, 135, 255]))
    });
    update(&mut conn, 1, "status", "Passive");
    wait(&host, |s| s.items.iter().any(|i| i.key == red.key && i.status == Status::Passive));
    for mode in ["truncated", "oversize"] {
        let before = host.snapshot().generation;
        update(&mut conn, 1, "bad", mode);
        wait(&host, |s| s.generation > before);
        assert!(host.snapshot().items.iter().find(|i| i.key == red.key).unwrap().icon.as_ref().is_none_or(|p| p.width() <= 512 && p.height() <= 512));
    }
    update(&mut conn, 1, "bad", "");
    assert!(host.send(Action::Activate { key: red.key, x: 12, y: 34 }));
    assert!(host.send(Action::Secondary { key: red.key, x: 12, y: 34 }));
    assert!(host.send(Action::Scroll { key: red.key, delta: -120, horizontal: false }));
    assert!(host.send(Action::OpenMenu { key: red.key, x: 12, y: 34 }));
    let s = wait(&host, |s| s.items.iter().any(|i| i.key == red.key && i.menu.is_some() && !i.menu_loading));
    let menu = s.items.iter().find(|i| i.key == red.key).unwrap().menu.as_ref().unwrap();
    assert_eq!(menu.len(), 5);
    assert_eq!(menu[1].toggle, Some(true));
    assert!(!menu[2].enabled);
    assert!(menu[3].separator);
    assert_eq!(menu[4].children[0].id, 6);
    host.send(Action::MenuEvent { key: red.key, id: 3, timestamp: 100 });
    host.send(Action::MenuEvent { key: red.key, id: 2, timestamp: 101 });
    host.send(Action::MenuEvent { key: red.key, id: 6, timestamp: 102 });
    wait(&host, |s| s.items.iter().any(|i| i.key == red.key && i.menu.as_ref().is_some_and(|m| m[1].toggle == Some(false))));
    control(&mut conn, "Drop", "i", dbus::body_u32(1));
    wait(&host, |s| s.items.len() == 1);
    let count = events(&root).iter().filter(|r| r["method"] == "Activate").count();
    host.send(Action::Activate { key: red.key, x: 99, y: 99 });
    control(&mut conn, "Spawn", "i", dbus::body_u32(1));
    let s = wait(&host, |s| s.items.len() == 2);
    assert!(s.items.iter().all(|i| i.key != red.key));
    thread::sleep(Duration::from_millis(150));
    assert_eq!(count, events(&root).iter().filter(|r| r["method"] == "Activate").count(), "stale key targeted reowned service");
    assert_eq!(owner(&mut conn), watcher_owner);
    let records = events(&root);
    for name in ["Activate", "SecondaryActivate", "Scroll", "AboutToShow", "GetLayout", "Event"] {
        assert!(records.iter().any(|r| r["method"] == name), "missing {name}");
    }
    assert!(!records.iter().any(|r| r["method"] == "Event" && r["id"] == 3));
    assert!(records.iter().any(|r| r["method"] == "Event" && r["id"] == 6));
    drop(second);
    drop(host);
}
