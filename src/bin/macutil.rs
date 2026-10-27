//! macutil: screen recording, idle inhibit, game mode and do-not-disturb from the command line.

use macshell::util::{self, Cmd, Ctx, RealSys};

const HELP: &str = "macutil - utilities behind the Control Center's quick toggles

Usage:
  macutil record start|stop|toggle|status [--region]
                     screen recording to ~/Videos/Recordings/recording-DATE.mp4 (ffmpeg x11grab on X11, wf-recorder on
                     Wayland); --region asks for an area with slop (slurp on Wayland); a notification says where it was saved
  macutil idle-inhibit on|off|toggle|status
                     keep the screen from locking or blanking (xset s off -dpms; the old values come back on `off`)
  macutil gamemode on|off|toggle|status
                     stop picom and use the performance power profile (powerprofilesctl); restored on `off`
  macutil dnd on|off|toggle|status
                     do-not-disturb of macnotify (dunst / mako when one of them is the daemon)
  macutil status     all four on one line:  record=off idle-inhibit=on gamemode=off dnd=off

`status` prints `on` or `off`. Every command is idempotent (`on` twice does nothing). -q / --quiet (or MACUTIL_QUIET=1)
suppresses the toasts. State lives in $XDG_RUNTIME_DIR/macutil; recordings go to $MACUTIL_RECORD_DIR when set.";

fn main() {
    macshell::mem::limit_arenas();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match util::parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("macutil: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    match cmd {
        Cmd::Help => println!("{HELP}"),
        Cmd::Version => println!("macutil {}", env!("CARGO_PKG_VERSION")),
        _ => match util::execute(&cmd, &Ctx::from_env(), &RealSys) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("macutil: {e}");
                std::process::exit(1);
            }
        },
    }
}
