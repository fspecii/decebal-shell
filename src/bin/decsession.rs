//! decsession: full-screen session menu (lock, log out, suspend, hibernate, restart, shut down).

use decebal::session::{self, Choice, Env, Outcome, Session};
use std::cell::RefCell;
use std::rc::Rc;

const HELP: &str = "decsession - session menu: a dimmed screen with big Lock / Log Out / Suspend / Hibernate / Restart / Shut Down buttons

Usage: decsession [--help | --version]

Keys:  L lock   O log out   S suspend   H hibernate (only when /sys/power/state offers disk)
       R restart   P shut down   (or arrows / Tab and Return)   Escape cancels
Log out, Restart and Shut Down need a second press (or a second click on the same button) to confirm.
A click on the dimmed background cancels. Running decsession again while it is open closes it.

Commands:  rdwm-lock (else loginctl lock-session) | rdwmctl action quit when rdwm runs (else loginctl terminate-user $USER)
           systemctl suspend | hibernate | reboot | poweroff
Bind it in rdwm, e.g.  [[bind]] mods = [\"super\", \"shift\"]  key = \"e\"  action = \"spawn\"  args = [\"decsession\"]";

fn main() {
    decebal::compat::init();
    let started = std::time::Instant::now();
    decebal::mem::limit_arenas();
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => {}
        Some("-h" | "--help") => return println!("{HELP}"),
        Some("-V" | "--version") => return println!("decsession {}", env!("CARGO_PKG_VERSION")),
        Some(a) => {
            eprintln!("decsession: unknown argument {a}\n\n{HELP}");
            std::process::exit(2);
        }
    }
    let Some(_guard) = decebal::single::acquire("decsession") else { return };
    let outcome = Rc::new(RefCell::new(Outcome::Pending));
    let app = Session::new(session::choices(session::hibernate_available()), outcome.clone()).started_at(started);
    if let Err(e) = decebal::window_app::run_window(app) {
        eprintln!("decsession: {e}");
        std::process::exit(1);
    }
    // The overlay (and its keyboard / pointer grabs) is gone now, so a screen locker can take the keyboard.
    let chosen = match *outcome.borrow() {
        Outcome::Run(c) => Some(c),
        _ => None,
    };
    if let Some(c) = chosen {
        run(c);
    }
}

fn run(choice: Choice) {
    let argv = session::command(choice, &Env::detect());
    if !session::spawn_detached(&argv) {
        eprintln!("decsession: could not run {}", argv.join(" "));
        std::process::exit(1);
    }
}
