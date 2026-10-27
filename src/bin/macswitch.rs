//! Alt+Tab window switcher for the rdwm X11 session.

use macshell::menubar::rdwm;
use macshell::switcher::{self, HoldMod, Outcome, Switcher};
use std::cell::RefCell;
use std::rc::Rc;

const HELP: &str = "macswitch - Alt+Tab window switcher

Usage: macswitch [--back] [--mod alt|super|ctrl] [--help | --version]

Shows the open windows of rdwm (all tags; the ones not in view are dimmed) as a row of icons on a centred
card. Bind it in rdwm to Alt+Tab (`macswitch`) and Alt+Shift+Tab (`macswitch --back`). The first selection is
the previous window (the last one with --back).

While it is shown it holds the keyboard:
  Tab, Right, Down        next window         Shift+Tab, Left, Up   previous window
  Return, click, or letting go of Alt   switch to the selected window
  Escape, click outside   cancel               q or w   close the selected window

Starting macswitch again while it is open moves its selection (next, or previous with --back).
--mod sets the key that is held with the hotkey (default alt; letting go of it commits).
Windows are listed most recently used first (rdwm's `mru` when it reports one).";

fn main() {
    macshell::mem::limit_arenas();
    // A second launch signals us; block the signals before anything else so an early one is held, not fatal.
    macshell::single::block_signals(&[switcher::SIG_NEXT, switcher::SIG_PREV]);
    let mut back = false;
    let mut hold = HoldMod::Alt;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => return println!("{HELP}"),
            "-V" | "--version" => return println!("macswitch {}", env!("CARGO_PKG_VERSION")),
            "--back" | "-b" => back = true,
            "--mod" => match args.next().as_deref().and_then(HoldMod::parse) {
                Some(m) => hold = m,
                None => {
                    eprintln!("macswitch: --mod takes alt, super or ctrl");
                    std::process::exit(2);
                }
            },
            _ => {
                eprintln!("macswitch: unknown option {a}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    let Some(_guard) = macshell::single::acquire_signalling("macswitch", if back { switcher::SIG_PREV } else { switcher::SIG_NEXT }) else {
        return;
    };
    let entries = switcher::fetch_entries();
    if entries.is_empty() {
        eprintln!("macswitch: no windows (is rdwm running?)");
        return;
    }
    let all = entries.clone();
    let outcome = Rc::new(RefCell::new(Outcome::Pending));
    let app = Switcher::new(entries, back, hold, outcome.clone()).with_closer(macshell::backend::x11_window::close_window);
    if let Err(e) = macshell::backend::x11_window::run(app) {
        eprintln!("macswitch: {e}");
        std::process::exit(1);
    }
    // The popup (and its keyboard grab) is gone now; tell rdwm.
    let chosen = match &*outcome.borrow() {
        Outcome::Commit(e) => Some(e.clone()),
        _ => None,
    };
    if let Some(e) = chosen {
        for line in switcher::commit_requests(&e) {
            let _ = rdwm::request(&line);
        }
        switcher::save_mru(&switcher::mru_after(&all, e.id));
    } else if let Some(first) = all.first().filter(|e| e.on_view && !e.minimized) {
        // Cancelled: closing the overlay can move the pointer-focus (focus-follows-mouse) onto whatever is under
        // the card, so put the focus back where it was.
        let _ = rdwm::request(&rdwm::action_request("focuswindow", &[&first.id.to_string()]));
    }
}
