//! Copy text to the clipboard on Wayland (`wl-copy`) or X11 (a tiny selection owner process, so no
//! `xclip`/`xsel` is needed).

use std::io::Write;
use std::process::{Command, Stdio};

/// Hidden `decspot` argument that runs [`serve_x11`] with the text on stdin.
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
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var("DECEBAL_BACKEND").as_deref() != Ok("x11");
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

/// The atoms a selection owner needs to answer requests for plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelAtoms {
    pub targets: u32,
    pub utf8: u32,
    pub string: u32,
    pub text: u32,
    pub mime_utf8: u32,
    pub mime_plain: u32,
}

/// What to answer to one selection request (ICCCM 2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// `TARGETS`: the atoms we can convert to (property type `ATOM`, format 32).
    Targets(Vec<u32>),
    /// The text in `ty` (format 8).
    Bytes { ty: u32, data: Vec<u8> },
    /// Not convertible: the property in the notify event is `None`.
    Refuse,
}

/// `STRING` is ISO 8859-1: characters outside it become `?`.
pub fn latin1(text: &str) -> Vec<u8> {
    text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect()
}

/// The answer to a request for `target`. Pure, so the protocol rules are unit-testable.
pub fn reply_for(text: &str, target: u32, a: &SelAtoms) -> Reply {
    if target == a.targets {
        Reply::Targets(vec![a.targets, a.utf8, a.string, a.text, a.mime_utf8, a.mime_plain])
    } else if target == a.string {
        Reply::Bytes { ty: a.string, data: latin1(text) }
    } else if target == a.utf8 || target == a.text || target == a.mime_utf8 || target == a.mime_plain {
        // TEXT lets the owner choose the encoding; the plain-text MIME types are UTF-8 here. The reply's type names what was sent.
        let ty = if target == a.text { a.utf8 } else { target };
        Reply::Bytes { ty, data: text.as_bytes().to_vec() }
    } else {
        Reply::Refuse
    }
}

#[cfg(feature = "x11")]
impl SelAtoms {
    pub fn intern<C: x11rb::connection::Connection>(conn: &C) -> Result<SelAtoms, String> {
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
        let intern = |name: &str| -> Result<u32, String> { Ok(conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom) };
        Ok(SelAtoms {
            targets: intern("TARGETS")?,
            utf8: intern("UTF8_STRING")?,
            string: u32::from(AtomEnum::STRING),
            text: intern("TEXT")?,
            mime_utf8: intern("text/plain;charset=utf-8")?,
            mime_plain: intern("text/plain")?,
        })
    }
}

/// Answers a `SelectionRequest` for `text`: writes the property on the requestor and sends the `SelectionNotify`.
#[cfg(feature = "x11")]
pub fn respond<C: x11rb::connection::Connection>(conn: &C, r: &x11rb::protocol::xproto::SelectionRequestEvent, text: &str, a: &SelAtoms) {
    use x11rb::protocol::xproto::*;
    use x11rb::wrapper::ConnectionExt as _;
    // A client of the pre-ICCCM era leaves the property unset: use the target as the property.
    let property = if r.property == 0 { r.target } else { r.property };
    let property = match reply_for(text, r.target, a) {
        Reply::Targets(list) => conn.change_property32(PropMode::REPLACE, r.requestor, property, AtomEnum::ATOM, &list).map(|_| property).unwrap_or(0),
        Reply::Bytes { ty, data } => conn.change_property8(PropMode::REPLACE, r.requestor, property, ty, &data).map(|_| property).unwrap_or(0),
        Reply::Refuse => 0,
    };
    let notify = SelectionNotifyEvent { response_type: SELECTION_NOTIFY_EVENT, sequence: 0, time: r.time, requestor: r.requestor, selection: r.selection, target: r.target, property };
    let _ = conn.send_event(false, r.requestor, EventMask::NO_EVENT, notify);
    let _ = conn.flush();
}

/// Owns the X11 CLIPBOARD selection with `text` until another client takes it over.
#[cfg(feature = "x11")]
pub fn serve_x11(text: &str) -> Result<(), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::protocol::Event;
    let e = |e: &dyn std::fmt::Display| e.to_string();
    let (conn, n) = x11rb::connect(None).map_err(|x| e(&x))?;
    let root = conn.setup().roots[n].root;
    let clipboard = conn.intern_atom(false, b"CLIPBOARD").map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?.atom;
    let atoms = SelAtoms::intern(&conn)?;
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
            Event::SelectionRequest(r) => respond(&conn, &r, text, &atoms),
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
    /// Needs a private X server: `DECEBAL_TEST_X11=1 DISPLAY=:94 cargo test clipboard` (skipped otherwise,
    /// so the suite never touches a real session).
    #[test]
    fn serves_text_to_a_requestor() {
        if std::env::var("DECEBAL_TEST_X11").as_deref() != Ok("1") {
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
        let (clip, utf8, prop) = (atom("CLIPBOARD"), atom("UTF8_STRING"), atom("DECSPOT_TEST"));
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

#[cfg(test)]
mod reply_tests {
    use super::*;

    const A: SelAtoms = SelAtoms { targets: 100, utf8: 101, string: 31, text: 102, mime_utf8: 103, mime_plain: 104 };

    #[test]
    fn targets_lists_the_text_formats() {
        let Reply::Targets(t) = reply_for("x", A.targets, &A) else { panic!("not a target list") };
        for want in [A.targets, A.utf8, A.string] {
            assert!(t.contains(&want), "{want} missing from {t:?}");
        }
    }

    #[test]
    fn utf8_is_sent_verbatim_and_text_is_typed_utf8() {
        assert_eq!(reply_for("héllo €", A.utf8, &A), Reply::Bytes { ty: A.utf8, data: "héllo €".as_bytes().to_vec() });
        assert_eq!(reply_for("a", A.text, &A), Reply::Bytes { ty: A.utf8, data: b"a".to_vec() });
        assert_eq!(reply_for("a", A.mime_utf8, &A), Reply::Bytes { ty: A.mime_utf8, data: b"a".to_vec() });
    }

    #[test]
    fn string_is_latin1() {
        assert_eq!(reply_for("é€x", A.string, &A), Reply::Bytes { ty: A.string, data: vec![0xe9, b'?', b'x'] });
    }

    #[test]
    fn unknown_targets_are_refused() {
        assert_eq!(reply_for("x", 9999, &A), Reply::Refuse);
        assert_eq!(reply_for("x", 0, &A), Reply::Refuse);
    }
}
