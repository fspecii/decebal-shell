//! Copy text to the clipboard on Wayland (`wl-copy`) or X11 (a tiny selection owner process, so no
//! `xclip`/`xsel` is needed).

use std::io::Write;
use std::process::{Command, Stdio};

/// Hidden `macspot` argument that runs [`serve_x11`] with the text on stdin.
pub const SERVE_ARG: &str = "--serve-clipboard";

fn in_path(prog: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
}

fn pipe_to(mut cmd: Command, text: &str) -> bool {
    let Ok(mut child) = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    true
}

/// Puts `text` on the clipboard; true when a helper was started.
pub fn copy(text: &str) -> bool {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var("MACSHELL_BACKEND").as_deref() != Ok("x11");
    if wayland && in_path("wl-copy") {
        return pipe_to(Command::new("wl-copy"), text);
    }
    if std::env::var_os("DISPLAY").is_some() {
        if in_path("xclip") {
            let mut c = Command::new("xclip");
            c.args(["-selection", "clipboard"]);
            return pipe_to(c, text);
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut c = Command::new("setsid");
            c.arg(exe).arg(SERVE_ARG);
            if pipe_to(c, text) {
                return true;
            }
        }
    }
    in_path("wl-copy") && pipe_to(Command::new("wl-copy"), text)
}

/// Owns the X11 CLIPBOARD selection with `text` until another client takes it over.
#[cfg(feature = "x11")]
pub fn serve_x11(text: &str) -> Result<(), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::protocol::Event;
    use x11rb::wrapper::ConnectionExt as _;
    let e = |e: &dyn std::fmt::Display| e.to_string();
    let (conn, n) = x11rb::connect(None).map_err(|x| e(&x))?;
    let root = conn.setup().roots[n].root;
    let intern = |name: &str| -> Result<Atom, String> { Ok(conn.intern_atom(false, name.as_bytes()).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?.atom) };
    let (clipboard, targets, utf8) = (intern("CLIPBOARD")?, intern("TARGETS")?, intern("UTF8_STRING")?);
    let win = conn.generate_id().map_err(|x| e(&x))?;
    conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).map_err(|x| e(&x))?;
    conn.set_selection_owner(win, clipboard, x11rb::CURRENT_TIME).map_err(|x| e(&x))?;
    conn.flush().map_err(|x| e(&x))?;
    let owner = conn.get_selection_owner(clipboard).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?.owner;
    if owner != win {
        return Err("could not own the clipboard".into());
    }
    loop {
        match conn.wait_for_event().map_err(|x| e(&x))? {
            Event::SelectionClear(_) => return Ok(()),
            Event::SelectionRequest(r) => {
                let mut property = r.property;
                if property == 0 {
                    property = r.target;
                }
                if r.target == targets {
                    let list = [targets, utf8, u32::from(AtomEnum::STRING)];
                    let _ = conn.change_property32(PropMode::REPLACE, r.requestor, property, AtomEnum::ATOM, &list);
                } else if r.target == utf8 || r.target == u32::from(AtomEnum::STRING) {
                    let _ = conn.change_property8(PropMode::REPLACE, r.requestor, property, r.target, text.as_bytes());
                } else {
                    property = 0;
                }
                let notify = SelectionNotifyEvent { response_type: SELECTION_NOTIFY_EVENT, sequence: 0, time: r.time, requestor: r.requestor, selection: r.selection, target: r.target, property };
                let _ = conn.send_event(false, r.requestor, EventMask::NO_EVENT, notify);
                let _ = conn.flush();
            }
            _ => {}
        }
    }
}

#[cfg(not(feature = "x11"))]
pub fn serve_x11(_: &str) -> Result<(), String> {
    Err("built without x11".into())
}

#[cfg(all(test, feature = "x11"))]
mod tests {
    /// Needs a private X server: `MACSHELL_TEST_X11=1 DISPLAY=:94 cargo test clipboard` (skipped otherwise,
    /// so the suite never touches a real session).
    #[test]
    fn serves_text_to_a_requestor() {
        if std::env::var("MACSHELL_TEST_X11").as_deref() != Ok("1") {
            return;
        }
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::*;
        use x11rb::protocol::Event;
        std::thread::spawn(|| {
            let _ = super::serve_x11("héllo 14");
        });
        let (conn, n) = x11rb::connect(None).unwrap();
        let root = conn.setup().roots[n].root;
        let atom = |s: &str| conn.intern_atom(false, s.as_bytes()).unwrap().reply().unwrap().atom;
        let (clip, utf8, prop) = (atom("CLIPBOARD"), atom("UTF8_STRING"), atom("MACSPOT_TEST"));
        let win = conn.generate_id().unwrap();
        conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).unwrap();
        for _ in 0..50 {
            if conn.get_selection_owner(clip).unwrap().reply().unwrap().owner != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        conn.convert_selection(win, clip, utf8, prop, x11rb::CURRENT_TIME).unwrap();
        conn.flush().unwrap();
        loop {
            if let Event::SelectionNotify(_) = conn.wait_for_event().unwrap() {
                break;
            }
        }
        let r = conn.get_property(false, win, prop, utf8, 0, 100).unwrap().reply().unwrap();
        assert_eq!(String::from_utf8(r.value).unwrap(), "héllo 14");
    }
}
