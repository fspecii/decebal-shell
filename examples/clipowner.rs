//! Test helper for the clipboard e2e script: a window with a given WM_CLASS that owns the CLIPBOARD selection and
//! serves a text, like an application would. Usage: `clipowner CLASS TEXT [--hint]`. With `--hint` the TARGETS list
//! carries `x-kde-passwordManagerHint`, as KeePassXC and Bitwarden do. Exits when another client takes the selection
//! or on SIGTERM.

use decebal::clipboard::{self, SelAtoms};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let class = args.get(1).cloned().unwrap_or_else(|| "clipowner".into());
    let text = args.get(2).cloned().unwrap_or_default();
    let hint = args.iter().any(|a| a == "--hint");
    let (conn, n) = x11rb::connect(None).expect("connect");
    let root = conn.setup().roots[n].root;
    let win = conn.generate_id().unwrap();
    conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).unwrap();
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, format!("{class}\0{class}\0").as_bytes()).unwrap();
    let atom = |s: &str| conn.intern_atom(false, s.as_bytes()).unwrap().reply().unwrap().atom;
    let clip = atom("CLIPBOARD");
    let hint_atom = atom("x-kde-passwordManagerHint");
    let atoms = SelAtoms::intern(&conn).unwrap();
    conn.set_selection_owner(win, clip, x11rb::CURRENT_TIME).unwrap();
    conn.flush().unwrap();
    loop {
        match conn.wait_for_event() {
            Ok(Event::SelectionClear(_)) | Err(_) => return,
            Ok(Event::SelectionRequest(r)) => {
                if hint && r.target == atoms.targets {
                    let list = [atoms.targets, atoms.utf8, atoms.string, hint_atom];
                    let _ = conn.change_property32(PropMode::REPLACE, r.requestor, r.property, AtomEnum::ATOM, &list);
                    let n = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: r.time,
                        requestor: r.requestor,
                        selection: r.selection,
                        target: r.target,
                        property: r.property,
                    };
                    let _ = conn.send_event(false, r.requestor, EventMask::NO_EVENT, n);
                    let _ = conn.flush();
                } else if hint && r.target == hint_atom {
                    let _ = conn.change_property8(PropMode::REPLACE, r.requestor, r.property, hint_atom, b"secret");
                    let n = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: r.time,
                        requestor: r.requestor,
                        selection: r.selection,
                        target: r.target,
                        property: r.property,
                    };
                    let _ = conn.send_event(false, r.requestor, EventMask::NO_EVENT, n);
                    let _ = conn.flush();
                } else {
                    clipboard::respond(&conn, &r, &text, &atoms);
                }
            }
            Ok(_) => {}
        }
    }
}
