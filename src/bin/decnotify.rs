//! decnotify: notification daemon with macOS-style banners, and its small control CLI.

use decebal::dbus::Writer;
use decebal::notify::{self, Config, Start};

const HELP: &str = "decnotify - freedesktop notification daemon with macOS-style banners (X11)

Usage:
  decnotify                 run the daemon (exits quietly if mako, dunst, ... already own the name)
  decnotify history         toggle the notification history panel of the running daemon
  decnotify clear           clear the notification history
  decnotify dnd [on|off|toggle|status]
                            do-not-disturb; `status` prints on/off and fails when decnotify is not the daemon
  decnotify test [TITLE [BODY]]   send a sample notification
  decnotify -h | -V

Settings: ~/.config/decebal/notifications.toml
  dnd = false            # do not disturb: banners are hidden, history still records (critical still shows)
  banner_seconds = 5     # how long a banner stays
  muted_apps = []        # app names / desktop-entry ids that never show";

fn die(msg: &str) -> ! {
    eprintln!("decnotify: {msg}");
    std::process::exit(1);
}

fn main() {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    match arg(0) {
        None | Some("daemon") => daemon(),
        Some("-h" | "--help") => println!("{HELP}"),
        Some("-V" | "--version") => println!("decnotify {}", env!("CARGO_PKG_VERSION")),
        Some("history") => {
            notify::ext_call("ToggleHistory", "", Vec::new()).unwrap_or_else(|e| die(&format!("no decnotify daemon running ({e})")));
        }
        Some("clear") => {
            notify::ext_call("ClearHistory", "", Vec::new()).unwrap_or_else(|e| die(&format!("no decnotify daemon running ({e})")));
        }
        Some("test") => {
            let id = notify::send_test(arg(1).unwrap_or("decnotify"), arg(2).unwrap_or("Notifications are working.")).unwrap_or_else(|e| die(&e));
            println!("{id}");
        }
        Some("dnd") => dnd(arg(1).unwrap_or("status")),
        Some(other) => {
            eprintln!("decnotify: unknown command {other}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}

fn dnd(what: &str) {
    let running = notify::running_dnd();
    let current = running.unwrap_or_else(|| Config::load().dnd);
    let want = match what {
        "status" => match running {
            Some(on) => {
                println!("{}", if on { "on" } else { "off" });
                return;
            }
            None => std::process::exit(1),
        },
        "on" => true,
        "off" => false,
        "toggle" => !current,
        other => die(&format!("dnd: expected on, off, toggle or status, not {other}")),
    };
    if running.is_some() {
        let mut w = Writer::default();
        w.u32(u32::from(want));
        notify::ext_call("SetDnd", "b", w.buf).unwrap_or_else(|e| die(&e));
    } else {
        Config::save_dnd(want).unwrap_or_else(|e| die(&e.to_string()));
    }
}

fn daemon() {
    // Only on X11: a Wayland compositor has its own daemon (mako) and layer-shell notifications.
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("decnotify: no X11 DISPLAY; nothing to do");
        return;
    }
    if std::env::var_os("DECNOTIFY_FORCE").is_none() && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") {
        eprintln!("decnotify: Wayland session; leaving notifications to the compositor's daemon (DECNOTIFY_FORCE=1 overrides)");
        return;
    }
    match notify::run_daemon() {
        Ok(Start::Ran) => {}
        Ok(Start::NameTaken) => eprintln!("decnotify: org.freedesktop.Notifications is already owned by another daemon; exiting"),
        Err(e) => die(&e),
    }
}
