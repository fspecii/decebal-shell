use macshell::config;
use macshell::backend::{self, Anchor, SurfaceSpec};
use macshell::dock::{Dock, DockItem};
use macshell::dock_app::DockApp;
use macshell::wm::Tracker;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut png: Option<String> = None;
    let mut hover: Option<f32> = None;
    let mut running: Vec<String> = vec!["foot".into()];
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--png" => { i += 1; png = args.get(i).cloned(); }
            "--hover" => { i += 1; hover = args.get(i).and_then(|v| v.parse().ok()); }
            "--running" => { i += 1; running = args.get(i).map(|v| v.split(',').map(String::from).collect()).unwrap_or_default(); }
            "-h" | "--help" => {
                println!("macdock [--png FILE] [--hover X] [--running id,id]\nWithout --png it runs as a dock (backends coming).");
                return;
            }
            other => { eprintln!("macdock: unknown argument {other}"); std::process::exit(2); }
        }
        i += 1;
    }

    let items: Vec<DockItem> = config::pinned().ordered().into_iter()
        .map(|id| DockItem { running: running.contains(&id), id }).collect();
    let mut dock = Dock::new(items, config::dock_settings());
    dock.hover_x = hover;
    let (w, h) = dock.canvas_size();
    let Some(mut pm) = tiny_skia::Pixmap::new(w, h) else {
        eprintln!("macdock: cannot allocate {w}x{h} canvas");
        std::process::exit(1);
    };
    dock.draw(&mut pm);
    match png {
        Some(path) => {
            if let Err(e) = pm.save_png(&path) { eprintln!("macdock: {e}"); std::process::exit(1); }
            println!("wrote {path} ({w}x{h})");
        }
        None => {
            let spec = SurfaceSpec { anchor: Anchor::Bottom, margin: 13, reserve: 0, namespace: "macshell-dock" };
            if let Err(e) = backend::run(DockApp::new(dock).with_tracker(Tracker::start(|| {})), spec) {
                eprintln!("macdock: {e}");
                std::process::exit(1);
            }
        }
    }
}
