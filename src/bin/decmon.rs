//! decmon: Activity Monitor / Task Manager. A resizable window, or (`--popup`) the card under the bar that the agents icon opens.

use decebal::procmon::{Mode, Monitor, Tab};
use decebal::window_app::run_window;
#[cfg(feature = "x11")]
use decebal::window_app::PopupAnchor;

const HELP: &str = "decmon - Activity Monitor: processes and system performance

Usage: decmon [TAB] [--popup [--x PX] [--top PX]] [--help | --version]

TAB is processes (default) or performance (also 1, 2).
Without --popup it is a normal resizable window (also what the \"Activity Monitor\" entry of the launcher starts).
--popup  open as a card under the menu bar (an X11 popup that grabs the keyboard; this is what decbar's agents icon runs)
--x PX   centre the popup on this x offset from the monitor's left edge
--top PX distance of the popup from the monitor's top edge (default: just under the bar)

Running decmon again while its window is open closes it, so one hotkey bound to `decmon` toggles it.

Processes: apps with their child processes folded in, background processes; click a column to sort (CPU, memory, disk, PID, name);
just type to search. End task sends SIGTERM, Force quit SIGKILL (asks first). Only your own processes (all as root); PID 1 and
decmon itself are never touched, desktop-session programs and kernel threads need an extra confirmation.
Performance: CPU and per-core load, memory and swap, disk, network per interface, GPU, temperatures, battery power.
Sampling is once a second and only while the window is open.

Keys: Tab switch tab, Up/Down select, Left/Right fold, Delete end task, Shift+Delete force quit, Esc clear search / close.";

fn main() {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    let mut tab = Tab::Processes;
    let (mut popup, mut x, mut top) = (false, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => return println!("{HELP}"),
            "-V" | "--version" => return println!("decmon {}", env!("CARGO_PKG_VERSION")),
            "--popup" => popup = true,
            "--x" | "--top" => {
                let Some(v) = args.next().and_then(|v| v.parse::<i32>().ok()) else {
                    eprintln!("decmon: {a} takes a number of pixels");
                    std::process::exit(2);
                };
                if a == "--x" {
                    x = Some(v);
                } else {
                    top = Some(v);
                }
            }
            other => match Tab::parse(other) {
                Some(t) => tab = t,
                None => {
                    eprintln!("decmon: unknown option {other}\n\n{HELP}");
                    std::process::exit(2);
                }
            },
        }
    }
    let lock = if popup { "decmon-popup" } else { "decmon" };
    let Some(_guard) = decebal::single::acquire(lock) else {
        return;
    };
    // The popup is an X11 override-redirect window; without X11 (or a DISPLAY) it opens as a normal window instead.
    #[cfg(feature = "x11")]
    let result = if popup && std::env::var_os("DISPLAY").is_some() {
        let anchor = PopupAnchor { top: top.unwrap_or_else(decebal::procmon::default_top), center_x: x };
        decebal::backend::x11_window::run(Monitor::new(Mode::Popup(anchor), tab)).map_err(|e| e.to_string())
    } else {
        run_window(Monitor::new(Mode::Window, tab))
    };
    #[cfg(not(feature = "x11"))]
    let result = {
        let _ = (x, top, popup);
        run_window(Monitor::new(Mode::Window, tab))
    };
    if let Err(e) = result {
        eprintln!("decmon: {e}");
        std::process::exit(1);
    }
}
