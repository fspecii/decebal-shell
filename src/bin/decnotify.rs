//! decnotify: notification daemon with macOS-style banners, and its small control CLI.

use decebal::dbus::Writer;
use decebal::notify::{self, Config, Start};

const HELP: &str = "decnotify - freedesktop notification daemon with macOS-style banners (X11)

Usage:
  decnotify                 run the daemon (exits quietly if mako, dunst, ... already own the name)
  decnotify history [show|retry|status [--json]]
                            toggle/show history, retry durable storage, or report storage status
  decnotify retry           retry pending history changes after repairing storage
  decnotify status [--json] report history persistence (exit 1 when unavailable)
  decnotify clear           clear history; reports failure if the clear cannot be saved
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
        Some("history") => match arg(1) {
            None => history_call("ToggleHistory", "open history"),
            Some("show") => history_call("ShowHistory", "show history"),
            Some("retry") => history_call("RetryHistory", "retry history storage"),
            Some("clear") => history_call("ClearHistory", "save cleared history"),
            Some("status") => history_status(arg(2)),
            Some(other) => die(&format!("history: expected show, retry, clear or status, not {other}")),
        },
        Some("retry") => history_call("RetryHistory", "retry history storage"),
        Some("status") => history_status(arg(1)),
        Some("clear") => history_call("ClearHistory", "save cleared history"),
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

fn history_call(method: &str, action: &str) {
    notify::ext_call(method, "", Vec::new()).unwrap_or_else(|e| die(&format!("cannot {action}: {e}")));
}
fn decode_history_status(reply: &decebal::dbus::Message) -> Result<(String, bool, bool), String> {
    let args = reply.args().map_err(|e| e.to_string())?;
    if reply.sig != "sbb" || args.len() != 3 {
        return Err("invalid history status reply".into());
    }
    let error = args[0].as_str().ok_or("invalid history error field")?.to_string();
    let pending = args[1].as_bool().ok_or("invalid pending field")?;
    let clear = args[2].as_bool().ok_or("invalid pending-clear field")?;
    Ok((error, pending, clear))
}
fn history_status(format: Option<&str>) {
    if format.is_some_and(|s| s != "--json") {
        die("status accepts only --json");
    }
    let reply = notify::ext_call("GetHistoryStatus", "", Vec::new()).unwrap_or_else(|e| die(&format!("cannot query history storage: {e}")));
    let (error, pending, clear_pending) = decode_history_status(&reply).unwrap_or_else(|e| die(&e));
    if format == Some("--json") {
        println!("{}", serde_json::json!({"error":if error.is_empty(){None}else{Some(error.as_str())},"pending":pending,"clear_pending":clear_pending}));
    } else if error.is_empty() {
        println!("History is saved");
    } else {
        println!("History unavailable: {error}\nPending changes: {}{}", if pending { "yes" } else { "no" }, if clear_pending { " (clear)" } else { "" });
    }
    if !error.is_empty() {
        std::process::exit(1);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_status_decodes_typed_persistence_state() {
        let call = decebal::dbus::Message::call("org.test", "/Test", "org.test", "Test", "", vec![]);
        let mut w = Writer::default();
        w.string("Cannot save history");
        w.u32(1);
        w.u32(0);
        let reply = decebal::dbus::Message::reply_to(&call, "sbb", w.buf);
        assert_eq!(decode_history_status(&reply).unwrap(), ("Cannot save history".into(), true, false));
        assert!(decode_history_status(&decebal::dbus::Message::reply_to(&call, "", vec![])).is_err());
    }
}
