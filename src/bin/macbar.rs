//! macbar: the macOS-style menu bar and Control Center, or the Caelestia pill bar under the Caelestia theme.

use macshell::app::App;
use macshell::backend::{self, Anchor, SurfaceSpec};
use macshell::config::BarPosition;
use macshell::menubar::active::{ActiveWindow, StaticApp, WmActive};
use macshell::menubar::vbar::{self, Process};
use macshell::menubar::MacBar;
use macshell::theme::BAR_HEIGHT;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const USAGE: &str = "macbar [--dry-run] [--app ID] [--role main|bottom] [--position POS] [--png FILE [--width N] [--height N] [--open KEY|control]]\n\
Runs the menu bar. Under the Caelestia theme (bar_style = \"vertical\") it is the pill bar instead, on the edge macshell.json says:\n\
\"bar\": {\"position\": \"left\" | \"right\" | \"top\" | \"bottom\" | \"split\"} (default left; macthemectl layout sets it). Changes are followed live.\n\
split is two bars: this process draws the top one (OS icon, workspaces, window title) and starts `macbar --role bottom` for the\n\
bottom one (tray, status icons, clock, power). Each role holds a lock of its own, so a second copy of either exits at once.\n\
--role bottom runs only that second bar (it leaves by itself when the layout is not split); --position draws the pill bar at POS\n\
whatever the setting says (for --png).\n\
With --png it renders one frame to a file (for testing) instead; --height is the screen height of a side bar, --width the screen\n\
width of a top or bottom one.\n\
KEY is one of apple, app, file, edit, view, window, help, calendar; for the pill bar one of audio, network, bluetooth, battery, window, layout.";

fn main() {
    macshell::mem::limit_arenas();
    let args: Vec<String> = std::env::args().collect();
    let mut png: Option<String> = None;
    let mut width: u32 = 1920;
    let mut height: u32 = 900;
    let mut open: Option<String> = None;
    let mut app_id: Option<String> = None;
    let mut role = Process::Main;
    let mut position: Option<BarPosition> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--png" => {
                i += 1;
                png = args.get(i).cloned();
            }
            "--width" => {
                i += 1;
                width = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(width);
            }
            "--height" => {
                i += 1;
                height = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(height);
            }
            "--app" => {
                i += 1;
                app_id = args.get(i).cloned();
            }
            "--role" => {
                i += 1;
                role = args.get(i).and_then(|v| Process::parse(v)).unwrap_or_else(|| usage_error("--role is main or bottom"));
            }
            "--position" => {
                i += 1;
                position = Some(args.get(i).and_then(|v| BarPosition::parse(v)).unwrap_or_else(|| usage_error("--position is left, right, top, bottom or split")));
            }
            "--dry-run" => macshell::controlcenter::sys::set_dry_run(true),
            "--open" => {
                i += 1;
                open = args.get(i).cloned();
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => usage_error(&format!("unknown argument {other}")),
        }
        i += 1;
    }

    let show_workspaces = macshell::config::shell_settings().workspaces.resolve(macshell::wm::is_rdwm());
    if let Some(path) = png {
        let mut bar = MacBar::new(make_active(app_id.clone())).with_workspaces(show_workspaces).with_height(height).with_process(role).with_position(position);
        if let Err(e) = bar.render_png(&path, width, open.as_deref()) {
            eprintln!("macbar: {e}");
            std::process::exit(1);
        }
        println!("wrote {path}");
        return;
    }

    // `--app` runs are for tests and ignore the settings and the lock. A real bar holds the lock of its role (a supervisor may
    // start it again; the second copy just leaves).
    let gated = app_id.is_none();
    let _lock = if gated {
        match macshell::single::hold(role.lock_name()) {
            Some(l) => Some(l),
            None => return,
        }
    } else {
        None
    };

    // `bar.enabled` (macshell.json) can switch the bar off, e.g. for the Windows-only layout. A hidden bar
    // keeps its process (the session supervisor would restart a missing one) but has no surface; it comes back
    // by itself when the setting changes. `--png` and `--app` runs ignore the setting. The bottom half of a split
    // layout is not supervised (the main bar starts it), so it simply ends when it has nothing to draw.
    loop {
        if gated {
            if role == Process::Bottom {
                if !macshell::menubar::gate::visible_now() || vbar::place_now(role, position).is_none() {
                    return;
                }
            } else {
                macshell::menubar::gate::wait_until_visible();
            }
        }
        let closed = Arc::new(AtomicBool::new(false));
        let mut bar =
            MacBar::new(make_active(app_id.clone())).with_workspaces(macshell::config::shell_settings().workspaces.resolve(macshell::wm::is_rdwm())).with_process(role).with_position(position);
        if gated {
            bar = bar.with_gate(closed.clone());
            if role == Process::Main {
                bar = bar.with_companion();
            }
        }
        let fallback = SurfaceSpec { anchor: Anchor::Top, margin: 0, reserve: BAR_HEIGHT as i32, stretch: true, namespace: "macshell-bar" };
        let spec = App::surface_spec(&bar).unwrap_or(fallback);
        if let Err(e) = backend::run(bar, spec) {
            eprintln!("macbar: {e}");
            std::process::exit(1);
        }
        if !closed.load(Ordering::SeqCst) || role == Process::Bottom {
            break;
        }
        macshell::mem::trim();
    }
}

fn usage_error(msg: &str) -> ! {
    eprintln!("macbar: {msg}\n{USAGE}");
    std::process::exit(2);
}

fn make_active(app_id: Option<String>) -> Box<dyn ActiveWindow> {
    match app_id {
        Some(id) => Box::new(StaticApp(id)),
        None => Box::new(WmActive::default()),
    }
}
