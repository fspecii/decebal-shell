//! decutil: screen recording, idle inhibit, game mode and do-not-disturb from the command line.

use decebal::util::{self, Cmd, Ctx, RealSys};

const HELP: &str = "decutil - utilities behind the Control Center's quick toggles

Usage:
  decutil record start|stop|toggle|status [--region]
                     screen recording to ~/Videos/Recordings/recording-DATE.mp4 (ffmpeg x11grab on X11, wf-recorder on
                     Wayland); --region asks for an area with slop (slurp on Wayland); a notification says where it was saved
  decutil idle-inhibit on|off|toggle|status
                     keep the screen from locking or blanking (xset s off -dpms; the old values come back on `off`)
  decutil gamemode on|off|toggle|status
                     stop picom and use the performance power profile (powerprofilesctl); restored on `off`
  decutil dnd on|off|toggle|status
                     do-not-disturb of decnotify (dunst / mako when one of them is the daemon)
  decutil control-center [toggle|open|close] [--corner auto|top-right|top-left|bottom-right|bottom-left]
                     the Control Center (quick settings) panel, in every theme and with or without a bar: under the menu bar,
                     above the Windows taskbar, beside the Caelestia bar, or in a screen corner; `toggle` closes it when open.
                     Bind it to a key (rdwm: action spawn decutil control-center toggle) or a hot corner (control-center)
  decutil status     all four on one line:  record=off idle-inhibit=on gamemode=off dnd=off

`status` prints `on` or `off`. Every command is idempotent (`on` twice does nothing). -q / --quiet (or DECUTIL_QUIET=1)
suppresses the toasts. State lives in $XDG_RUNTIME_DIR/decutil; recordings go to $DECUTIL_RECORD_DIR when set.";

fn main() {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(parsed) = util::control_center_args(&args) {
        match parsed {
            Ok(argv) => {
                let status = std::process::Command::new(decebal::launch::sibling_or_path("decbar")).args(&argv).status();
                std::process::exit(match status {
                    Ok(s) => s.code().unwrap_or(1),
                    Err(e) => {
                        eprintln!("decutil: cannot run decbar: {e}");
                        1
                    }
                });
            }
            Err(e) => {
                eprintln!("decutil: {e}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    let cmd = match util::parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("decutil: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    match cmd {
        Cmd::Help => println!("{HELP}"),
        Cmd::Version => println!("decutil {}", env!("CARGO_PKG_VERSION")),
        _ => match util::execute(&cmd, &Ctx::from_env(), &RealSys) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("decutil: {e}");
                std::process::exit(1);
            }
        },
    }
}
