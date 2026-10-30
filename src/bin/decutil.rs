//! decutil: screen recording, idle inhibit, game mode and do-not-disturb from the command line.

use decebal::util::{self, Cmd, Ctx, RealSys};

const HELP: &str = "decutil - utilities behind the Control Center's quick toggles

Usage:
  decutil reveal -- PATH\n                     select a file or folder in its file manager; falls back to opening its parent\n  decutil screenshot [--fullscreen]\n                     select a region, save to Pictures, copy it and open the annotation editor (X11/Wayland)\n  decutil record start|stop|toggle|status [--region]
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
    let raw: Vec<_> = std::env::args_os().skip(1).collect();
    if raw.first().is_some_and(|a| a == "reveal") {
        if raw.len() != 3 || raw[1] != "--" {
            eprintln!("decutil reveal -- PATH");
            std::process::exit(2);
        }
        match decebal::reveal::reveal(std::path::Path::new(&raw[2])) {
            Ok(decebal::reveal::Outcome::Selected) => println!("selected"),
            Ok(decebal::reveal::Outcome::OpenedParent) => println!("folder"),
            Err(e) => {
                eprintln!("decutil reveal: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if raw.first().is_some_and(|a| a == decebal::reveal::WORKER_ARG) {
        let result = if raw.len() == 2 { raw[1].to_str().ok_or_else(|| "Invalid URI".to_string()).and_then(decebal::reveal::show_items) } else { Err("Expected one file URI".into()) };
        if let Err(e) = result {
            eprintln!("decutil reveal: {e}");
            std::process::exit(1);
        }
        return;
    }
    let args: Vec<String> = raw
        .into_iter()
        .map(|a| {
            a.into_string().unwrap_or_else(|_| {
                eprintln!("decutil: arguments must be UTF-8");
                std::process::exit(2)
            })
        })
        .collect();
    if args.first().is_some_and(|a| a == "screenshot") {
        if args.iter().skip(1).any(|a| a != "--fullscreen") {
            eprintln!("decutil screenshot [--fullscreen]");
            std::process::exit(2);
        }
        match decebal::capture::capture(args.iter().any(|a| a == "--fullscreen")) {
            Ok(Some(path)) => println!("{}", path.display()),
            Ok(None) => {}
            Err(e) => {
                eprintln!("Screenshot: {e}");
                let _ = std::process::Command::new("notify-send").args(["-a", "Decebal", "Screenshot failed", &e]).status();
                std::process::exit(1);
            }
        }
        return;
    }
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
                decebal::log_error!("fatal: {e}");
                eprintln!("decutil: {e}");
                std::process::exit(1);
            }
        },
    }
}
