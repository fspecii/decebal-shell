//! Launchpad: a searchable grid of installed apps. Type to filter, arrows to move, Enter to launch, Esc to close.

use decebal::launcher::placement::{parse_args, Parsed};
use decebal::launcher::Launcher;
use decebal::window_app::run_window;

const HELP: &str = "declaunch - application launcher in the style chosen in Settings > Launcher

Usage: declaunch [toggle|open|close] [--edge top|bottom|left|right] [--anchor X,Y] [--thickness N] [--output NAME]

  toggle (default)  open, or close the running launcher
  open              open unless already open        close   close a running launcher
  --edge            screen edge of the panel whose launcher button was used
  --anchor X,Y      centre of that button (X11 root coordinates; output-local on Wayland)
  --thickness N     the panel's thickness, same units: the launcher never covers it
  --output NAME     the output the panel is on

Without a panel the style opens in its natural place. Unknown arguments are ignored.
Type to search, arrow keys and Tab to move, Enter or click to launch, right click or Ctrl+P to pin,
Esc or a click outside to close. Styles: default, dux, windows, gnome, kde, xfce, blackberry
(decebal.json \"launcher\": {\"style\": ...}; \"theme\" follows the theme).";

fn main() {
    let inv = match parse_args(std::env::args().skip(1)) {
        Parsed::Help => return println!("{HELP}"),
        Parsed::Version => return println!("declaunch {}", env!("CARGO_PKG_VERSION")),
        Parsed::Run(inv) => inv,
    };
    // Install the dedicated close handler before publishing the singleton lock
    // or starting workers (see single::acquire_clean_toggle).
    let Some(_guard) = decebal::single::acquire_clean("declaunch", inv.verb) else { return };
    decebal::compat::init();
    decebal::mem::limit_arenas();
    for arg in &inv.ignored {
        eprintln!("declaunch: ignoring {arg}");
    }
    if let Err(e) = run_window(Launcher::new().with_invocation(inv)) {
        decebal::log_error!("fatal: {e}");
        eprintln!("declaunch: {e}");
        std::process::exit(1);
    }
}
