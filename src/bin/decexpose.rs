//! Exposé / Mission Control style window overview for the rdwm X11 session.

use decebal::expose::{self, capture, Expose, Item, Outcome, Shared, State};
use decebal::menubar::rdwm;
use std::cell::RefCell;
use std::rc::Rc;

const HELP: &str = "decexpose - window overview (Mission Control / Exposé) for rdwm

Usage: decexpose [--all] [--help | --version]

Shows the windows of the current workspace (tag view) as a grid of live-ish thumbnails on a dimmed screen: each window
is captured once at start (Composite when available, else GetImage); windows that are not mapped (minimised) show their
app icon on a card. With --all every window is listed, grouped by workspace in rows, under a strip of workspace chips 1-9
(occupied ones highlighted); clicking a chip switches to that workspace.

  Click, Return        switch to the window (its workspace first when it is on another one; restored when minimised)
  Arrows, Tab          move the selection        Home, End   first / last
  type                 filter by title or class (a search pill appears; Backspace edits)    / starts a filter explicitly
  w, Delete, middle click   close the selected window (w types a letter once a filter is started, Ctrl+W always closes)
  Escape, click on empty space   cancel (the previous focus is restored)

Running decexpose again while it is open closes it. Bind it in rdwm, e.g.
  [[bind]] mods = [\"super\"] key = \"Up\" action = \"spawn\" args = [\"decexpose\"]
  [[bind]] mods = [\"super\", \"shift\"] key = \"Up\" action = \"spawn\" args = [\"decexpose\", \"--all\"]";

fn main() {
    decebal::compat::init();
    let started = std::time::Instant::now();
    decebal::mem::limit_arenas();
    // A second launch asks us to close with SIGTERM; hold it from the start so an early one is not fatal.
    decebal::single::block_signals(&[libc::SIGTERM]);
    let mut all = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => return println!("{HELP}"),
            "-V" | "--version" => return println!("decexpose {}", env!("CARGO_PKG_VERSION")),
            "--all" | "-a" => all = true,
            _ => {
                eprintln!("decexpose: unknown option {a}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    let Some(_guard) = decebal::single::acquire("decexpose") else { return };
    let Some(mut state) = expose::fetch_state().filter(|s| !s.items.is_empty()) else {
        eprintln!("decexpose: no windows (is rdwm running?)");
        return;
    };
    // Capture while the windows are still visible: once the overlay covers the screen it is too late.
    if let Some(probe) = capture::probe(&mut state.items) {
        let caps = expose::thumb_caps(&state.items, all, probe.screen.0 as f32, probe.screen.1 as f32, probe.ui);
        capture::capture(&probe, &mut state.items, &caps);
    }
    // What the cancel path needs after the thumbnails moved into the app.
    let plain = State { items: state.items.iter().map(|i| Item { e: i.e.clone(), size: i.size, thumb: None }).collect(), ..state.clone_meta() };
    let shared = Rc::new(RefCell::new(Shared::default()));
    let app = Expose::new(state, all, shared.clone()).with_closer(decebal::backend::x11_window::close_window).started_at(started);
    if let Err(e) = decebal::backend::x11_window::run(app) {
        decebal::log_error!("fatal: {e}");
        eprintln!("decexpose: {e}");
        std::process::exit(1);
    }
    // The overlay (and its keyboard / pointer grabs) is gone now; tell rdwm.
    let (outcome, closed) = {
        let s = shared.borrow();
        (s.outcome.clone(), s.closed.clone())
    };
    match outcome {
        Outcome::Commit(_) | Outcome::View(_) => {
            for line in expose::requests_for(&outcome) {
                let _ = rdwm::request(&line);
            }
            // Focus follows the mouse in rdwm: leave the pointer on the window that was chosen.
            if let Outcome::Commit(e) = &outcome {
                capture::warp_into(e.id);
            }
        }
        // Cancelled: closing the overlay can move the pointer focus onto whatever is under it, so put the focus back.
        Outcome::Cancel | Outcome::Pending => {
            if let Some(line) = expose::restore_request(&plain, &closed) {
                let _ = rdwm::request(&line);
            }
        }
    }
}
