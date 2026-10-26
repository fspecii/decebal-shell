//! Debug tool: prints the window list whenever it changes.
use macshell::wm::{ActiveWindow, Tracker};
use std::time::Duration;

fn main() {
    let tracker = Tracker::start(|| {});
    let mut last = String::new();
    loop {
        let snap = tracker.snapshot();
        let line = format!(
            "active={:?} running={:?}",
            tracker.active_app_name(),
            snap.windows.iter().map(|w| format!("{}{}", w.app_id, if w.active { "*" } else { "" })).collect::<Vec<_>>()
        );
        if line != last {
            println!("{line}");
            last = line;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
