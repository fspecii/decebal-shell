//! Spotlight: search apps, commands, packages to install or remove, and calculate.

use macshell::spotlight::Spotlight;
use macshell::window_app::run_window;

const HELP: &str = "macspot - Spotlight search

Usage: macspot

Type to search apps and commands. Prefix with `install` or `remove` to find packages,
or type a sum to calculate. Enter runs, Up/Down choose, Esc closes.";

fn main() {
    macshell::mem::limit_arenas();
    if let Some(a) = std::env::args().nth(1) {
        match a.as_str() {
            "-h" | "--help" => println!("{HELP}"),
            "-V" | "--version" => println!("macspot {}", env!("CARGO_PKG_VERSION")),
            _ => {
                eprintln!("macspot: unknown option {a}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        return;
    }
    let Some(_guard) = macshell::single::acquire("macspot") else {
        return;
    };
    if let Err(e) = run_window(Spotlight::new()) {
        eprintln!("macspot: {e}");
        std::process::exit(1);
    }
}
