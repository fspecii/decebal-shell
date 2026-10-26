//! macbar: the macOS-style menu bar and Control Center.

use macshell::app::App;
use macshell::backend::{self, Anchor, SurfaceSpec};
use macshell::menubar::active::{ActiveWindow, StaticApp, WmActive};
use macshell::menubar::MacBar;
use macshell::theme::BAR_HEIGHT;

const USAGE: &str = "macbar [--dry-run] [--app ID] [--png FILE [--width N] [--open KEY|control]]\n\
Runs the menu bar. With --png it renders one frame to a file (for testing) instead.\n\
KEY is one of apple, app, file, edit, view, window, help.";

fn main() {
    macshell::mem::limit_arenas();
    let args: Vec<String> = std::env::args().collect();
    let mut png: Option<String> = None;
    let mut width: u32 = 1920;
    let mut open: Option<String> = None;
    let mut app_id: Option<String> = None;
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
            "--app" => {
                i += 1;
                app_id = args.get(i).cloned();
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
            other => {
                eprintln!("macbar: unknown argument {other}\n{USAGE}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let active: Box<dyn ActiveWindow> = match app_id {
        Some(id) => Box::new(StaticApp(id)),
        None => Box::new(WmActive::default()),
    };
    let mut bar = MacBar::new(active);
    if let Some(path) = png {
        if let Err(e) = bar.render_png(&path, width, open.as_deref()) {
            eprintln!("macbar: {e}");
            std::process::exit(1);
        }
        println!("wrote {path}");
        return;
    }

    let spec = SurfaceSpec { anchor: Anchor::Top, margin: 0, reserve: BAR_HEIGHT as i32, stretch: true, namespace: "macshell-bar" };
    let _: &dyn App = &bar;
    if let Err(e) = backend::run(bar, spec) {
        eprintln!("macbar: {e}");
        std::process::exit(1);
    }
}
