//! Launchpad: a searchable grid of installed apps. Type to filter, arrows to move, Enter to launch, Esc to close.

use macshell::launcher::Launcher;
use macshell::window_app::run_window;

const HELP: &str = "maclaunch - searchable application launcher

Usage: maclaunch

Type to filter, arrow keys to move, Enter or click to launch, Esc to close.
It opens as a normal window; give it a floating, centred window rule (see contrib/).";

fn main() {
    macshell::mem::limit_arenas();
    if let Some(a) = std::env::args().nth(1) {
        match a.as_str() {
            "-h" | "--help" => println!("{HELP}"),
            "-V" | "--version" => println!("maclaunch {}", env!("CARGO_PKG_VERSION")),
            _ => {
                eprintln!("maclaunch: unknown option {a}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        return;
    }
    if let Err(e) = run_window(Launcher::new()) {
        eprintln!("maclaunch: {e}");
        std::process::exit(1);
    }
}
