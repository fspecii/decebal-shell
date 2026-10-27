//! Hot Corners for the X11 session: a daemon plus a tiny CLI to read and change the settings.

use macshell::hot::{self, Action, Corner, HotConfig, ACTION_NAMES};

const HELP: &str = "machot - macOS-style hot corners for X11

Usage: machot [daemon | get | set CORNER ACTION | set dwell_ms MS | actions | --help | --version]

  machot daemon           run for the whole session (autostart: contrib/macshell-hot.desktop; X11 only)
  machot get              print the four corners and the dwell time
  machot set CORNER ACTION
                          CORNER is top-left, top-right, bottom-left or bottom-right; the running daemon
                          picks the change up at once. `machot set dwell_ms 200` changes the dwell time.
  machot actions          list the actions

Settings live in macshell.json (the omarchy config folder, or $MACSHELL_CONFIG):
  \"hot_corners\": { \"top_left\": \"none\", \"top_right\": \"dashboard\", \"bottom_left\": \"lock\",
                   \"bottom_right\": \"custom:notify-send hello\", \"dwell_ms\": 150 }

The daemon puts a 2x2 px InputOnly window in each configured corner of every monitor (RandR; the corner where two
monitors touch is not a corner) and runs the corner's action once the pointer has stayed there for dwell_ms. It then
waits for the pointer to leave (plus a short cooldown). Nothing fires while a mouse button is held, so dragging a
window to a corner is safe. The daemon grabs nothing, takes no focus and sleeps in poll(2) while idle.";

fn die(msg: &str) -> ! {
    eprintln!("machot: {msg}");
    std::process::exit(1);
}

fn get() {
    let cfg = HotConfig::load();
    for c in Corner::ALL {
        println!("{:<13}{}", c.key().replace('_', "-"), cfg.action(c).name());
    }
    println!("{:<13}{}", "dwell_ms", cfg.dwell_ms);
}

fn actions() {
    for a in ACTION_NAMES {
        let note = match Action::parse(a) {
            Some(Action::ShowDesktop) => "  (accepted, does nothing: rdwm has no such action)",
            _ => "",
        };
        println!("{a}{note}");
    }
    println!("custom:<shell command>");
}

fn main() {
    macshell::mem::limit_arenas();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["daemon"] => {
            if std::env::var_os("DISPLAY").is_none() {
                die("needs an X11 session (DISPLAY is not set)");
            }
            if let Err(e) = hot::daemon::run() {
                die(&e);
            }
        }
        [] | ["get"] => get(),
        ["set", corner, rest @ ..] if !rest.is_empty() => {
            // Everything after the corner is the action, so `custom:` commands need no quoting.
            match hot::set_in(&hot::config_path(), corner, &rest.join(" ")) {
                Ok(stored) => println!("{} = {stored}", corner.replace('_', "-")),
                Err(e) => {
                    eprintln!("machot: {e}");
                    std::process::exit(2);
                }
            }
        }
        ["actions"] => actions(),
        ["-h" | "--help" | "help"] => println!("{HELP}"),
        ["-V" | "--version"] => println!("machot {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!("machot: bad arguments\n\n{HELP}");
            std::process::exit(2);
        }
    }
}
