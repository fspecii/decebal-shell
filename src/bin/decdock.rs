use decebal::backend::{self, Anchor, SurfaceSpec};
use decebal::config;
use decebal::dock::{Dock, DockItem};
use decebal::dock_app::DockApp;
use std::sync::atomic::{AtomicBool, Ordering};
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
                println!("decdock [--png FILE] [--hover X] [--running id,id]\nWithout --png it runs as a dock (backends coming).");
                return;
            }
            other => {
                eprintln!("decdock: unknown argument {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

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
                eprintln!("decdock: {e}");
                std::process::exit(1);
            }
            println!("wrote {path} ({w}x{h})");
        }
        None => {
            let spec = SurfaceSpec { anchor: Anchor::Bottom, margin: 13, reserve: 71, stretch: false, namespace: "decebal-dock" };
            // The style can change at runtime (dock-settings.json, the dock's menu); the app then re-specifies its surface.
            let scale = backend::ui_scale();
            // `dock.enabled` (decebal.json) can switch the dock off, and `"auto"` does so while the theme has a vertical bar
            // (Caelestia has no dock). A hidden dock keeps its process, because the session supervisor restarts a missing one;
            // only its surface goes away, and it comes back by itself when the setting or the theme changes.
            loop {
                decebal::menubar::gate::wait_until_dock_visible();
                let closed = Arc::new(AtomicBool::new(false));
                let mut dock = Dock::new(dock_items(&running), config::dock_settings());
                dock.hover_x = hover;
                let app = DockApp::new(dock).with_scale(scale).track_windows().watch_config().with_gate(closed.clone());
                if let Err(e) = backend::run(app, spec) {
                    eprintln!("decdock: {e}");
                    std::process::exit(1);
                }
                if !closed.load(Ordering::SeqCst) {
                    break;
                }
                decebal::mem::trim();
            }
        }
    }
}
