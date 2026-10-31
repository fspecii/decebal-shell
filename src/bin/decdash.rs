//! Dashboard popup under the menu bar: overview, media player and system performance.

use decebal::dash::layout::Tab;
use decebal::dash::Dash;

const HELP: &str = "decdash - dashboard popup (overview, media, performance)

Usage: decdash [TAB] [--x PX] [--top PX] [--output NAME] [--help | --version]

Opens a card under the menu bar (an override-redirect popup on X11 that grabs the keyboard and pointer; a layer-shell card on Wayland, closed by Escape or a click outside).
Running decdash again while it is open closes it, so one hotkey bound to `decdash` toggles it.

TAB is overview (default), media or performance (also 1, 2, 3).
--x PX    centre the card on this x offset from the monitor's left edge (decbar passes its icon's position)
--top PX  distance from the monitor's top edge (default: just under the bar)
--output NAME  open on this monitor (RandR / Wayland output name, e.g. DP-1); default: X11 the panels' monitor,
          Wayland the compositor's focused output

Keys:
  Left/Right, 1 2 3, click   switch tab        Escape, click outside, or running decdash again   close
  Space                      play/pause        Up/Down or wheel (Media tab)   volume
  Tab (Media tab)            next player

Overview: user, uptime, clock, month calendar, weather (the bar's cache), CPU/memory/disk rings, now playing.
Media: any MPRIS player (cover, progress, prev/play/next, volume, player picker).
Performance: CPU and per-core load, memory and swap, root disk, network rates, CPU temperature, NVIDIA GPU
(nvidia-smi; hidden when missing). Sampling is once a second, only while the card is open.";

fn main() {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    let mut tab = Tab::Overview;
    let (mut x, mut top, mut output) = (None, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => return println!("{HELP}"),
            "-V" | "--version" => return println!("decdash {}", env!("CARGO_PKG_VERSION")),
            "--output" => match args.next() {
                Some(name) if !name.is_empty() && !name.starts_with('-') => output = Some(name),
                _ => {
                    eprintln!("decdash: --output takes an output name");
                    std::process::exit(2);
                }
            },
            "--x" | "--top" => {
                let Some(v) = args.next().and_then(|v| v.parse::<i32>().ok()) else {
                    eprintln!("decdash: {a} takes a number of pixels");
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
                    eprintln!("decdash: unknown option {other}\n\n{HELP}");
                    std::process::exit(2);
                }
            },
        }
    }
    let Some(_guard) = decebal::single::acquire("decdash") else {
        return;
    };
    if let Err(e) = decebal::window_app::run_window(Dash::new(tab, x, top).with_output(output)) {
        decebal::log_error!("fatal: {e}");
        eprintln!("decdash: {e}");
        std::process::exit(1);
    }
}
