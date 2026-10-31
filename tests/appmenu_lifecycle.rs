//! The app-menu worker leaves no thread, socket or bus name behind once it is stopped or dropped
//! (the bar turning the applet off). Its own test binary, so fd and thread counts are not shared
//! with other tests. Runs a private `dbus-daemon` with no service directories.

use decebal::appmenu::{AppMenus, Config, RegistrarRole, State};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fds() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

fn appmenu_threads() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().filter_map(|t| std::fs::read_to_string(t.ok()?.path().join("comm")).ok()).filter(|c| c.trim() == "appmenu").count()
}

fn wait(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn stopping_or_dropping_the_worker_releases_threads_fds_and_the_registrar_name() {
    let dir = std::env::temp_dir().join(format!("decebal-appmenu-life-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("bus");
    std::fs::write(
        dir.join("bus.conf"),
        format!(
            "<busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth><policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
            sock.display()
        ),
    )
    .unwrap();
    let mut daemon = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", dir.join("bus.conf").display()))
        .args(["--nofork", "--nopidfile"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbus-daemon is required");
    wait("bus socket", || sock.exists());
    let address = format!("unix:path={}", sock.display());

    let base_fds = fds();
    assert_eq!(appmenu_threads(), 0);
    for round in 0..3 {
        let mut menus = AppMenus::start(Config::with_address(address.clone()), Arc::new(|| {})).unwrap();
        wait("registrar owned", || menus.snapshot().registrar == RegistrarRole::Owned);
        assert_eq!(appmenu_threads(), 1);
        assert!(fds() > base_fds, "the worker holds its bus socket and wake pipe");
        if round == 1 {
            // Dropping without stop(): the worker notices the closed wake pipe and exits by itself.
            drop(menus);
        } else {
            assert!(menus.stop(Duration::from_secs(3)), "worker joined");
            assert_eq!(menus.snapshot().state, State::Stopped);
            drop(menus);
        }
        wait("worker thread gone", || appmenu_threads() == 0);
        wait("fds released", || fds() == base_fds);
    }
    // The bus saw every connection go: a new worker can own the name again immediately.
    let mut again = AppMenus::start(Config::with_address(address), Arc::new(|| {})).unwrap();
    wait("owned again", || again.snapshot().registrar == RegistrarRole::Owned);
    assert!(again.stop(Duration::from_secs(3)));

    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
