use decebal::backend::{self, Anchor, SurfaceSpec};
use decebal::config;
use decebal::dock::{Dock, DockItem};
use decebal::dock_app::DockApp;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn dock_items(running: &[String]) -> Vec<DockItem> {
    config::pinned().or_default(&|id| decebal::launch::find_exec(id).is_some()).into_iter().map(|id| DockItem::pinned(&id, running.contains(&id))).collect()
}

fn main() {
    decebal::compat::init();
    let args: Vec<String> = std::env::args().collect();
    let mut png: Option<String> = None;
    let mut hover: Option<f32> = None;
    let mut running: Vec<String> = vec!["foot".into()];
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--reconcile" => {
                if let Err(e) = decebal::dock_lifecycle::reconcile(&config::config_dir().join("decebal.json")) {
                    eprintln!("decdock: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "--wanted" => {
                std::process::exit(if decebal::menubar::gate::dock_visible_now() { 0 } else { 1 });
            }
            "--focus" => {
                // keyboard mode of the running taskbar (bind it to a key, like decbar --focus for the bar)
                if !decebal::single::signal("decdock", libc::SIGUSR2) {
                    eprintln!("decdock: no dock is running");
                    std::process::exit(1);
                }
                return;
            }
            "--png" => {
                i += 1;
                png = args.get(i).cloned();
            }
            "--hover" => {
                i += 1;
                hover = args.get(i).and_then(|v| v.parse().ok());
            }
            "--running" => {
                i += 1;
                running = args.get(i).map(|v| v.split(',').map(String::from).collect()).unwrap_or_default();
            }
            "-h" | "--help" => {
                println!("decdock [--png FILE] [--hover X] [--running id,id] [--wanted | --reconcile | --focus]\nWithout --png it runs the active theme dock.");
                return;
            }
            other => {
                eprintln!("decdock: unknown argument {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let _instance = if png.is_none() {
        if !decebal::menubar::gate::dock_visible_now() {
            return;
        }
        match decebal::single::hold("decdock") {
            Some(g) => {
                // before any other thread starts: `decdock --focus` arrives as a signal this thread mask holds for its collector
                decebal::menubar::DecBar::init_focus_signal();
                Some(g)
            }
            None => return,
        }
    } else {
        None
    };
    let mut dock = Dock::new(dock_items(&running), config::dock_settings());
    dock.hover_x = hover;
    let (w, h) = dock.canvas_size();
    let Some(mut pm) = tiny_skia::Pixmap::new(w, h) else {
        eprintln!("decdock: cannot allocate {w}x{h} canvas");
        std::process::exit(1);
    };
    dock.draw(&mut pm);
    match png {
        Some(path) => {
            if let Err(e) = pm.save_png(&path) {
                decebal::log_error!("fatal: {e}");
                eprintln!("decdock: {e}");
                std::process::exit(1);
            }
            println!("wrote {path} ({w}x{h})");
        }
        None => {
            let spec = SurfaceSpec { anchor: Anchor::Bottom, margin: 13, reserve: 71, stretch: false, namespace: "decebal-dock" };
            // The style can change at runtime (dock-settings.json, the dock's menu); the app then re-specifies its surface.
            let scale = backend::ui_scale();
            // A disabled theme releases the whole process. Theme application starts it again.
            let closed = Arc::new(AtomicBool::new(false));
            let mut dock = Dock::new(dock_items(&running), config::dock_settings());
            dock.hover_x = hover;
            let app = DockApp::new(dock).with_scale(scale).track_windows().watch_config().with_gate(closed).with_focus_signal();
            if let Err(e) = backend::run(app, spec) {
                decebal::log_error!("fatal: {e}");
                eprintln!("decdock: {e}");
                std::process::exit(1);
            }
        }
    }
}
