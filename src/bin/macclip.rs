//! Clipboard history for the X11 session: a daemon that records the CLIPBOARD selection and a Spotlight-style picker.

use macshell::clip::daemon::{self, Cmd};
use macshell::clip::history::{self, History};
use macshell::clip::paste;
use macshell::clip::picker::{Outcome, Picker};
use std::cell::RefCell;
use std::rc::Rc;

const HELP: &str = "macclip - clipboard history

Usage: macclip [pick | daemon | clear | --help | --version]

  macclip, macclip pick   open the picker (running it again while it is open closes it, so one hotkey toggles it)
  macclip daemon          watch the clipboard and record text (start it with your session; X11 only)
  macclip clear           forget the whole history and empty the clipboard

The daemon waits for XFixes selection-owner events (no polling) on CLIPBOARD, never looks at PRIMARY, stores up to
100 text entries (newest first, a re-copy moves the entry to the top, nothing blank or over 100 KB) in
$XDG_STATE_HOME/macshell/clipboard-history (mode 0600), skips password managers (window class 1password, bitwarden,
keepassxc, keepass, or the x-kde-passwordManagerHint target), and keeps the clipboard alive after the app that
copied it exits.

Picker keys:
  type                 filter (case-insensitive)
  Up/Down, Tab         choose              Return    paste: set the clipboard, close, press Ctrl+V in the previous window
  Shift+Return         copy only           Alt+1..9  paste that entry
  Delete               remove the entry    Esc, click outside   close";

fn die(msg: &str) -> ! {
    eprintln!("macclip: {msg}");
    std::process::exit(1);
}

fn need_display() {
    if std::env::var_os("DISPLAY").is_none() {
        die("needs an X11 session (DISPLAY is not set)");
    }
}

/// Makes `text` the clipboard: through the daemon when it runs (it then also serves it), else a detached helper.
fn set_clipboard(text: &str) -> bool {
    if let Some(link) = daemon::Link::connect() {
        if link.set_clipboard(text) {
            return true;
        }
    }
    // No daemon: keep the history ourselves so the entry still moves to the top.
    if let Some(path) = history::state_path() {
        let mut h = History::load(&path);
        if h.push(text, macshell::fuzzy::now()) {
            let _ = h.save(&path);
        }
    }
    daemon::serve_detached(text)
}

fn pick() {
    need_display();
    let Some(_guard) = macshell::single::acquire("macclip") else { return };
    // The popup takes the focus: remember who had it, to paste there afterwards.
    let focus = paste::current_focus();
    let path = history::state_path();
    let hist = path.as_deref().map(History::load).unwrap_or_default();
    let outcome = Rc::new(RefCell::new(Outcome::Pending));
    let app = Picker::new(
        hist,
        path,
        Box::new(|| {
            daemon::tell(Cmd::Reload);
        }),
        outcome.clone(),
    );
    if let Err(e) = macshell::backend::x11_window::run(app) {
        die(&e.to_string());
    }
    // The popup and its grabs are gone now.
    let chosen = std::mem::replace(&mut *outcome.borrow_mut(), Outcome::Pending);
    match chosen {
        Outcome::Pending => {}
        Outcome::Copy(text) => {
            if !set_clipboard(&text) {
                die("could not take the clipboard");
            }
        }
        Outcome::Paste(text) => {
            if !set_clipboard(&text) {
                die("could not take the clipboard");
            }
            if let Err(e) = paste::paste(focus) {
                die(&format!("paste: {e}"));
            }
        }
    }
}

fn clear() {
    if let Some(path) = history::state_path() {
        if let Err(e) = History::default().save(&path) {
            die(&format!("cannot write {}: {e}", path.display()));
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        daemon::tell(Cmd::Clear);
    }
    println!("clipboard history cleared");
}

fn main() {
    macshell::mem::limit_arenas();
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        None | Some("pick") => pick(),
        Some("daemon") => {
            need_display();
            if let Err(e) = daemon::run() {
                die(&e);
            }
        }
        Some("clear") => clear(),
        Some("-h" | "--help" | "help") => println!("{HELP}"),
        Some("-V" | "--version") => println!("macclip {}", env!("CARGO_PKG_VERSION")),
        // Internal: own the clipboard with the text on stdin until another client copies (used when no daemon runs).
        Some(macshell::clipboard::SERVE_ARG) => {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
            if let Err(e) = macshell::clipboard::serve_x11(&text) {
                die(&e);
            }
        }
        Some(a) => {
            eprintln!("macclip: unknown argument {a}\n\n{HELP}");
            std::process::exit(2);
        }
    }
}
