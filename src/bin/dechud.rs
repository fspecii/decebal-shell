//! On-screen display for volume, brightness and microphone changes (X11).

use decebal::hud::{self, Hud, Kind};

const HELP: &str = "dechud - macOS-style on-screen display

Usage: dechud volume | brightness | mic | workspace | watch | --help | --version

Reads the current value itself and shows it on a small card (icon, label, 16-segment bar) centred horizontally on the
primary monitor, a tenth of its height above the bottom. Bind it after the key's own action, e.g. in rdwm:
  spawn \"sh -c 'wpctl set-volume -l 1.5 @DEFAULT_AUDIO_SINK@ 5%+; dechud volume'\"

  volume       default output: `wpctl get-volume @DEFAULT_AUDIO_SINK@` (pactl as a fallback); muted shows a crossed speaker
  mic          default input: `wpctl get-volume @DEFAULT_AUDIO_SOURCE@`
  brightness   `brightnessctl -m`, else /sys/class/backlight
  workspace    the workspace rdwm shows now: nine dots, the current one lit
  watch        run for the whole session: show the workspace card whenever rdwm switches workspace (autostart this)

If the value cannot be read nothing is shown (exit 0). The card never takes focus, keys or clicks (an override-redirect
window with an empty input region) and disappears after 1.4 s. Running dechud again while it is shown updates it and
restarts the timer (single instance: $XDG_RUNTIME_DIR/dechud.lock). Translucent under a compositor, opaque without one.";

fn main() {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    // A second launch signals us; block the signal before anything else so an early one is held, not fatal.
    decebal::single::block_signals(&[hud::SIG_UPDATE]);
    let arg = std::env::args().nth(1);
    let kind = match arg.as_deref() {
        Some("-h" | "--help") => return println!("{HELP}"),
        Some("-V" | "--version") => return println!("dechud {}", env!("CARGO_PKG_VERSION")),
        Some("watch") => return hud::watch_workspaces(),
        Some(a) => match Kind::parse(a) {
            Some(k) => k,
            None => {
                eprintln!("dechud: unknown argument {a}\n\n{HELP}");
                std::process::exit(2);
            }
        },
        None => {
            eprintln!("{HELP}");
            std::process::exit(2);
        }
    };
    // Nothing readable (no audio daemon, no backlight): show nothing.
    let Some(reading) = hud::probe(kind) else { return };
    // Leave the reading for a running instance first; it is signalled when the lock turns out to be taken.
    hud::post(&reading);
    let Some(_guard) = decebal::single::acquire_signalling(hud::LOCK_NAME, hud::SIG_UPDATE) else {
        return;
    };
    hud::clear_post();
    if let Err(e) = decebal::backend::x11_window::run(Hud::new(reading)) {
        decebal::log_error!("fatal: {e}");
        eprintln!("dechud: {e}");
        std::process::exit(1);
    }
}
