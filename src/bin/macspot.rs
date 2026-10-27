//! Spotlight: search apps, files, commands, packages, and calculate or convert.

use macshell::spotlight::Spotlight;
use macshell::window_app::run_window;

const HELP: &str = "macspot - Spotlight search

Usage: macspot [--help | --version]

Opens a search box centred on the primary monitor (a layer-shell overlay on Wayland, a grabbed
override-redirect popup on X11). Running macspot again while it is open closes it, so one hotkey
bound to `macspot` toggles it.

Type to search:
  apps          fuzzy-ranked, with the ones you open most/recently first
  files         in your home directory (plocate, else fd), 3+ characters
  2+3*4         calculator; Enter copies the result
  5 km to mi    unit conversion (length, mass, volume, data, time, speed, temperature)
  > cmd         run a shell command in a terminal (Ctrl+Enter: in the background)
  install foo   search Arch packages (also: remove foo)
  lock, sleep, restart, shut down, log out, screenshot, ...   system commands
  anything else falls back to a web search (MACSPOT_SEARCH_URL, `%s` = query)

Keys:
  Enter            open the selected result
  Ctrl/Alt+Enter   alternative: apps in a terminal, files reveal the folder, calculator copies `expr = result`
  Up/Down, Tab, Ctrl+N/P   choose        Esc, click outside, or the hotkey again   close
  Ctrl+U clear line, Ctrl+A/E start/end

Files: ~/.cache/macshell/apps.tsv (app index, refreshed in the background),
       ~/.local/state/macshell/spotlight-history (usage counts for ranking).";

fn main() {
    macshell::mem::limit_arenas();
    if let Some(a) = std::env::args().nth(1) {
        match a.as_str() {
            "-h" | "--help" => println!("{HELP}"),
            "-V" | "--version" => println!("macspot {}", env!("CARGO_PKG_VERSION")),
            // Internal: keep the X11 clipboard selection alive with the text on stdin (no xclip needed).
            macshell::clipboard::SERVE_ARG => {
                let mut text = String::new();
                let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
                if let Err(e) = macshell::clipboard::serve_x11(&text) {
                    eprintln!("macspot: clipboard: {e}");
                    std::process::exit(1);
                }
            }
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
