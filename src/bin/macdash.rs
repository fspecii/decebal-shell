//! Dashboard popup under the menu bar: overview, media player and system performance.

use macshell::dash::layout::Tab;
use macshell::dash::Dash;

const HELP: &str = "macdash - dashboard popup (overview, media, performance)

Usage: macdash [TAB] [--x PX] [--top PX] [--help | --version]

Opens a card under the menu bar (an override-redirect popup on X11 that grabs the keyboard and pointer).
Running macdash again while it is open closes it, so one hotkey bound to `macdash` toggles it.

TAB is overview (default), media or performance (also 1, 2, 3).
--x PX    centre the card on this x offset from the monitor's left edge (macbar passes its icon's position)
--top PX  distance from the monitor's top edge (default: just under the bar)

Keys:
  Left/Right, 1 2 3, click   switch tab        Escape, click outside, or running macdash again   close
  Space                      play/pause        Up/Down or wheel (Media tab)   volume
  Tab (Media tab)            next player

Overview: user, uptime, clock, month calendar, weather (the bar's cache), CPU/memory/disk rings, now playing.
Media: any MPRIS player (cover, progress, prev/play/next, volume, player picker).
Performance: CPU and per-core load, memory and swap, root disk, network rates, CPU temperature, NVIDIA GPU
(nvidia-smi; hidden when missing). Sampling is once a second, only while the card is open.";

fn main() {
    macshell::mem::limit_arenas();
    let mut tab = Tab::Overview;
    let (mut x, mut top) = (None, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => return println!("{HELP}"),
            "-V" | "--version" => return println!("macdash {}", env!("CARGO_PKG_VERSION")),
            "--x" | "--top" => {
                let Some(v) = args.next().and_then(|v| v.parse::<i32>().ok()) else {
                    eprintln!("macdash: {a} takes a number of pixels");
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
                    eprintln!("macdash: unknown option {other}\n\n{HELP}");
                    std::process::exit(2);
                }
            },
        }
    }
    let Some(_guard) = macshell::single::acquire("macdash") else {
        return;
    };
    if let Err(e) = macshell::backend::x11_window::run(Dash::new(tab, x, top)) {
        eprintln!("macdash: {e}");
        std::process::exit(1);
    }
}
