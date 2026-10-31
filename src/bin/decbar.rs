//! decbar: the macOS-style menu bar and Control Center, or the Caelestia pill bar under the Caelestia theme.

use decebal::app::App;
use decebal::backend::{self, Anchor, SurfaceSpec};
use decebal::config::BarPosition;
use decebal::menubar::active::{ActiveWindow, StaticApp, WmActive};
use decebal::menubar::vbar::{self, Process};
use decebal::menubar::DecBar;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const USAGE: &str = "decbar --focus [--role main|bottom]\n\
Keyboard mode of the running bar: arrows move between applets, Enter or Space opens one, arrows walk its popup or menu,\n\
Escape leaves.\n\
decbar --control-center [toggle|open|close] [--corner auto|top-right|top-left|bottom-right|bottom-left]\n\
Opens the Control Center on its own (any theme, with or without a bar; a second `toggle` closes it): under the menu bar, above the\n\
Windows taskbar, beside the pill bar or in a screen corner, as the theme's `control_center` settings say. Bind it to a key or a hot corner.\n\
decbar [--dry-run] [--app ID] [--role main|bottom] [--position POS] [--png FILE [--width N] [--height N] [--open KEY|control]]\n\
Runs the menu bar. Under the Caelestia theme (bar_style = \"vertical\") it is the pill bar instead, on the edge decebal.json says:\n\
\"bar\": {\"position\": \"left\" | \"right\" | \"top\" | \"bottom\" | \"split\"} (default left; decthemectl layout sets it). Changes are followed live.\n\
split is two bars: this process draws the top one (OS icon, workspaces, window title) and starts `decbar --role bottom` for the\n\
bottom one (tray, status icons, clock, power). Each role holds a lock of its own, so a second copy of either exits at once.\n\
--role bottom runs only that second bar (it leaves by itself when the layout is not split); --position draws the pill bar at POS\n\
whatever the setting says (for --png).\n\
With --png it renders one frame to a file (for testing) instead; --height is the screen height of a side bar, --width the screen\n\
width of a top or bottom one.\n\
KEY is one of apple, app, file, edit, view, window, help, calendar; for the pill bar one of audio, network, bluetooth, battery, window, layout.";

fn main() {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    let args: Vec<String> = std::env::args().collect();
    let mut png: Option<String> = None;
    let mut width: u32 = 1920;
    let mut height: u32 = 900;
    let mut open: Option<String> = None;
    let mut app_id: Option<String> = None;
    let mut role = Process::Main;
    let mut position: Option<BarPosition> = None;
    let mut panel: Option<String> = None;
    let mut corner: Option<decebal::controlcenter::settings::Corner> = None;
    let mut focus = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--control-center" | "--cc" => {
                panel = Some(match args.get(i + 1).map(String::as_str) {
                    Some(v @ ("toggle" | "open" | "close")) => {
                        i += 1;
                        v.to_string()
                    }
                    _ => "toggle".to_string(),
                });
            }
            "--corner" => {
                i += 1;
                corner = Some(
                    args.get(i).and_then(|v| decebal::controlcenter::settings::Corner::parse(v)).unwrap_or_else(|| usage_error("--corner is auto, top-right, top-left, bottom-right or bottom-left")),
                );
            }
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
            "--dry-run" => decebal::controlcenter::sys::set_dry_run(true),
            "--focus" => focus = true,
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

    if let Some(verb) = panel {
        control_center(&verb, corner);
        return;
    }
    if focus {
        // keyboard mode of the running bar (bind it to a key, like KDE's "focus the panel" shortcut)
        if !decebal::single::signal(role.lock_name(), libc::SIGUSR2) {
            eprintln!("decbar: no bar is running");
            std::process::exit(1);
        }
        return;
    }
    // the workspaces applet (and the old `workspaces` switch, which it reads) decides whether they show
    let show_workspaces = true;
    if let Some(path) = png {
        let mut bar = DecBar::new(make_active(app_id.clone())).with_workspaces(show_workspaces).with_height(height).with_process(role).with_position(position);
        if let Err(e) = bar.render_png(&path, width, open.as_deref()) {
            decebal::log_error!("fatal: {e}");
            eprintln!("decbar: {e}");
            std::process::exit(1);
        }
        println!("wrote {path}");
        return;
    }

    // `--app` runs are for tests and ignore the settings and the lock. A real bar holds the lock of its role (a supervisor may
    // start it again; the second copy just leaves).
    let gated = app_id.is_none();
    let _lock = if gated {
        match decebal::single::hold(role.lock_name()) {
            Some(l) => Some(l),
            None => return,
        }
    } else {
        None
    };
    // before any other thread starts: `decbar --focus` arrives as a signal this thread mask holds for its collector
    DecBar::init_focus_signal();

    // Runs after offscreen/Control Center exits, including when a hidden bar
    // waits without creating a surface. Bounded acknowledgement also covers a
    // bottom-role invocation that immediately has nothing to display.
    decebal::theme::frame::publish_startup(&decebal::theme::th());

    if gated && role == Process::Main {
        let cfg = decebal::widgets::config::WidgetsConfig::load(&decebal::theme::live::Source::discover().shell_json);
        if cfg.enabled && cfg.items.iter().any(|i| i.enabled && i.place == decebal::widgets::config::Place::Desktop) {
            if let Err(e) = decebal::widgets::host::start() {
                decebal::log_warn!("widgets: {e}");
            }
        }
    }

    // `bar.enabled` (decebal.json) can switch the bar off, e.g. for the Windows-only layout. A hidden bar
    // keeps its process (the session supervisor would restart a missing one) but has no surface; it comes back
    // by itself when the setting changes. `--png` and `--app` runs ignore the setting. The bottom half of a split
    // layout is not supervised (the main bar starts it), so it simply ends when it has nothing to draw.
    loop {
        if gated {
            if role == Process::Bottom {
                if !decebal::menubar::gate::visible_now() || vbar::place_now(role, position).is_none() {
                    return;
                }
            } else {
                decebal::menubar::gate::wait_until_visible();
            }
        }
        let closed = Arc::new(AtomicBool::new(false));
        let mut bar = DecBar::new(make_active(app_id.clone())).with_focus_signal().with_frame_publisher().with_workspaces(show_workspaces).with_process(role).with_position(position);
        if gated {
            bar = bar.with_gate(closed.clone());
            if role == Process::Main {
                bar = bar.with_companion();
            }
        }
        let fallback = SurfaceSpec { anchor: Anchor::Top, margin: 0, reserve: decebal::theme::th().flat_bar_height().ceil() as i32, stretch: true, namespace: "decebal-bar" };
        let spec = App::surface_spec(&bar).unwrap_or(fallback);
        if let Err(e) = backend::run_scaled(bar, spec, backend::ui_scale()) {
            decebal::log_error!("fatal: {e}");
            eprintln!("decbar: {e}");
            std::process::exit(1);
        }
        if !closed.load(Ordering::SeqCst) || role == Process::Bottom {
            break;
        }
        decebal::mem::trim();
    }
}

/// `decbar --control-center`: the panel as a program of its own. Its lock makes a second `toggle` close the first.
fn control_center(verb: &str, corner: Option<decebal::controlcenter::settings::Corner>) {
    use decebal::controlcenter::settings::{place, Host};
    use decebal::menubar::overlay::Overlay;
    const LOCK: &str = "decbar-cc";
    if verb == "close" {
        decebal::single::signal(LOCK, libc::SIGTERM);
        return;
    }
    let _lock = if verb == "open" {
        match decebal::single::hold(LOCK) {
            Some(l) => l,
            None => return,
        }
    } else {
        match decebal::single::acquire(LOCK) {
            Some(l) => l,
            None => return,
        }
    };
    decebal::theme::live::reload();
    let cc = decebal::theme::th().control_center;
    if !cc.enabled {
        eprintln!("decbar: the Control Center is turned off (control_center.enabled in decebal.json)");
        return;
    }
    let scale = backend::ui_scale();
    let host = Host::detect(&decebal::config::shell_settings(), &decebal::config::dock_settings(), decebal::theme::th().pill_bar(), scale);
    // the host's clearances are surface pixels; the overlay draws at the display scale like the bars, in logical ones
    let k = backend::surface_scale(scale);
    let p = place(&host, corner.unwrap_or(cc.position));
    let placement = decebal::controlcenter::settings::Placement { inset_x: p.inset_x / k, inset_y: p.inset_y / k, ..p };
    let overlay = Overlay::new(placement, host.is_taskbar());
    let spec = Overlay::spec(&placement);
    if let Err(e) = backend::run_scaled(overlay, spec, scale) {
        eprintln!("decbar: {e}");
        std::process::exit(1);
    }
}

fn usage_error(msg: &str) -> ! {
    eprintln!("decbar: {msg}\n{USAGE}");
    std::process::exit(2);
}

fn make_active(app_id: Option<String>) -> Box<dyn ActiveWindow> {
    match app_id {
        Some(id) => Box::new(StaticApp(id)),
        None => Box::new(WmActive::default()),
    }
}
