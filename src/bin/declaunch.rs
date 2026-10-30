//! Launchpad: a searchable grid of installed apps. Type to filter, arrows to move, Enter to launch, Esc to close.

use decebal::launcher::Launcher;
use decebal::window_app::run_window;

const HELP: &str = "declaunch - searchable application launcher

Usage: declaunch

Type to filter, arrow keys to move, Enter or click to launch, Esc to close.
It opens as a normal window; give it a floating, centred window rule (see contrib/).";

fn main() {
    // Install the dedicated close handler before publishing the singleton lock
    // or starting workers. Argument/helper modes retain ordinary signal handling.
    let _guard = if std::env::args_os().len() == 1 {
        let Some(guard) = decebal::single::acquire_clean_toggle("declaunch") else { return };
        Some(guard)
    } else {
        None
    };
    decebal::compat::init();
    decebal::mem::limit_arenas();
    if let Some(a) = std::env::args().nth(1) {
        match a.as_str() {
            "-h" | "--help" => println!("{HELP}"),
            "-V" | "--version" => println!("declaunch {}", env!("CARGO_PKG_VERSION")),
            _ => {
                eprintln!("declaunch: unknown option {a}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        return;
    }
    if let Err(e) = run_window(Launcher::new()) {
        decebal::log_error!("fatal: {e}");
        eprintln!("declaunch: {e}");
        std::process::exit(1);
    }
}
