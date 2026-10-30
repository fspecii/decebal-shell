use decebal::{
    notes::{Notes, Store},
    window_app::run_window,
};
fn main() {
    let mut blank = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            decebal::clipboard::SERVE_ARG => {
                use std::io::Read;
                let mut text = String::new();
                if std::io::stdin().take(4 * 1024 * 1024).read_to_string(&mut text).is_ok() {
                    #[cfg(feature = "x11")]
                    let _ = decebal::clipboard::serve_x11(&text);
                }
                return;
            }
            "--version" | "-V" => {
                println!("decnotes {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--new" => blank = true,
            "--help" | "-h" => {
                println!("decnotes [--new]\nCtrl+N new · Ctrl+S save · Ctrl+Shift+S save copy · Tab title/body");
                return;
            }
            _ => {
                eprintln!("decnotes: unknown option");
                std::process::exit(2)
            }
        }
    }
    let result = Store::discover().map_err(|e| e.to_string()).and_then(|s| run_window(Notes::new(s, blank)));
    if let Err(e) = result {
        eprintln!("decnotes: {e}");
        std::process::exit(1)
    }
}
