#![cfg(feature = "x11")]
//! This test owns its Xvfb. It never connects to the inherited desktop.
use decebal::wm::Tracker;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::{connection::Connection, protocol::xproto::*, wrapper::ConnectionExt};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(mut test: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !test() {
        assert!(Instant::now() < until, "private tracker operation timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn count(dir: &str) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

#[test]
fn idle_collectors_close_and_clones_keep_real_x11_updates_and_control() {
    let private = std::env::temp_dir().join(format!("tracker-native-{}", std::process::id()));
    std::fs::create_dir(&private).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(private.clone());
    for (key, dir) in [
        ("HOME", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_RUNTIME_DIR", "run"),
        ("XDG_CONFIG_DIRS", "empty-config"),
        ("XDG_DATA_DIRS", "empty-data"),
    ] {
        let path = private.join(dir);
        std::fs::create_dir(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::env::set_var(key, path);
    }
    for key in ["DBUS_SESSION_BUS_ADDRESS", "DBUS_SYSTEM_BUS_ADDRESS"] {
        std::env::set_var(key, format!("unix:path={}/no-bus", private.display()));
    }
    for key in ["WAYLAND_DISPLAY", "SESSION_MANAGER", "AT_SPI_BUS_ADDRESS", "HYPRLAND_INSTANCE_SIGNATURE"] {
        std::env::remove_var(key);
    }
    let n = (600..700).find(|n| !PathBuf::from(format!("/tmp/.X11-unix/X{n}")).exists() && !PathBuf::from(format!("/tmp/.X{n}-lock")).exists()).unwrap();
    let display = format!(":{n}");
    let mut server = Server(Command::new("Xvfb").args([&display, "-screen", "0", "640x480x24", "-nolisten", "tcp"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    wait(|| {
        assert!(server.0.try_wait().unwrap().is_none());
        std::fs::read_to_string(format!("/tmp/.X{n}-lock")).is_ok_and(|s| s.trim() == server.0.id().to_string())
    });
    std::env::set_var("DISPLAY", &display);
    std::env::set_var("DECEBAL_BACKEND", "x11");
    std::env::remove_var("XAUTHORITY");
    let (conn, screen) = x11rb::connect(Some(&display)).unwrap();
    let root = conn.setup().roots[screen].root;
    let atom = |name: &[u8]| conn.intern_atom(false, name).unwrap().reply().unwrap().atom;
    let clients = atom(b"_NET_CLIENT_LIST");
    let active = atom(b"_NET_ACTIVE_WINDOW");
    let title = atom(b"_NET_WM_NAME");
    let utf8 = atom(b"UTF8_STRING");
    let wid = conn.generate_id().unwrap();
    conn.create_window(0, wid, root, 0, 0, 100, 100, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new()).unwrap().check().unwrap();
    conn.change_property32(PropMode::REPLACE, root, clients, AtomEnum::WINDOW, &[wid]).unwrap();
    conn.change_property32(PropMode::REPLACE, root, active, AtomEnum::WINDOW, &[wid]).unwrap();
    conn.change_property8(PropMode::REPLACE, wid, AtomEnum::WM_CLASS, AtomEnum::STRING, b"instance\0TrackerApp\0").unwrap();
    conn.change_property8(PropMode::REPLACE, wid, title, utf8, b"first").unwrap();
    conn.flush().unwrap();
    let baseline_threads = count("/proc/self/task");
    let baseline_fds = count("/proc/self/fd");
    for round in 0..8 {
        let updates = Arc::new(AtomicUsize::new(0));
        let u = updates.clone();
        let tracker = Tracker::start(move || {
            u.fetch_add(1, Ordering::Relaxed);
        });
        wait(|| tracker.snapshot().windows.iter().any(|w| w.id == u64::from(wid)));
        let clone = tracker.clone();
        drop(tracker);
        let next_title = format!("changed-{round}");
        conn.change_property8(PropMode::REPLACE, wid, title, utf8, next_title.as_bytes()).unwrap();
        conn.flush().unwrap();
        wait(|| clone.snapshot().windows[0].title == next_title);
        assert!(updates.load(Ordering::Relaxed) >= 2);
        conn.delete_property(wid, title).unwrap();
        let legacy_title = format!("legacy-{round}");
        conn.change_property8(PropMode::REPLACE, wid, AtomEnum::WM_NAME, AtomEnum::STRING, legacy_title.as_bytes()).unwrap();
        conn.flush().unwrap();
        wait(|| clone.snapshot().windows[0].title == legacy_title);
        // The same shared controller must stay usable after another public clone disappears.
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY)).unwrap().check().unwrap();
        clone.activate(u64::from(wid));
        wait(|| {
            while let Some(event) = conn.poll_for_event().unwrap() {
                if let x11rb::protocol::Event::ClientMessage(e) = event {
                    if e.type_ == active && e.window == wid {
                        return true;
                    }
                }
            }
            false
        });
        let snapshot_count = updates.load(Ordering::Relaxed);
        drop(clone);
        wait(|| count("/proc/self/task") == baseline_threads && count("/proc/self/fd") <= baseline_fds);
        let current = updates.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(updates.load(Ordering::Relaxed), current.max(snapshot_count), "closed collectors must not wake dead UIs");
    }
    println!("8 idle create/drop cycles: threads={baseline_threads}, fds={baseline_fds}; clone title/control updates passed");
    #[cfg(feature = "wayland")]
    if Command::new("sway").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) {
        let config = private.join("sway.conf");
        std::fs::write(&config, "output * mode 640x480\nseat * hide_cursor 1000\n").unwrap();
        let mut compositor = Server(
            Command::new("sway")
                .args(["--unsupported-gpu", "-c"])
                .arg(config)
                .env("WLR_BACKENDS", "headless")
                .env("WLR_RENDERER", "pixman")
                .env("WLR_HEADLESS_OUTPUTS", "1")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let runtime = private.join("run");
        let mut socket = None;
        wait(|| {
            assert!(compositor.0.try_wait().unwrap().is_none(), "private headless compositor exited");
            socket = std::fs::read_dir(&runtime)
                .unwrap()
                .filter_map(Result::ok)
                .find(|e| e.file_name().to_string_lossy().starts_with("wayland-") && !e.file_name().to_string_lossy().ends_with(".lock"))
                .map(|e| e.file_name());
            socket.is_some()
        });
        std::env::set_var("WAYLAND_DISPLAY", socket.unwrap());
        std::env::set_var("DECEBAL_BACKEND", "wayland");
        let base_threads = count("/proc/self/task");
        let base_fds = count("/proc/self/fd");
        for _ in 0..4 {
            let tracker = Tracker::start(|| {});
            wait(|| count("/proc/self/task") > base_threads);
            let clone = tracker.clone();
            drop(tracker);
            assert!(count("/proc/self/task") > base_threads, "a public Wayland clone keeps tracking alive");
            drop(clone);
            wait(|| count("/proc/self/task") == base_threads && count("/proc/self/fd") <= base_fds);
        }
        println!("4 headless Wayland idle create/drop cycles: threads={base_threads}, fds={base_fds}; final public-owner shutdown passed");
    }
}
