//! Test helper for the X11 end-to-end scripts: a plain window with a given WM_CLASS and title that
//! closes on WM_DELETE_WINDOW. Usage: `fakewin CLASS TITLE [RRGGBB]`. With `FAKEWIN_LOG=file` it logs the clicks it receives.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let class = args.get(1).cloned().unwrap_or_else(|| "fakewin".into());
    let title = args.get(2).cloned().unwrap_or_else(|| class.clone());
    let rgb = args.get(3).and_then(|c| u32::from_str_radix(c, 16).ok()).unwrap_or(0x3366cc);
    let (conn, n) = x11rb::connect(None).expect("connect");
    let screen = &conn.setup().roots[n];
    let win = conn.generate_id().unwrap();
    let aux = CreateWindowAux::new().background_pixel(rgb).event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY | EventMask::BUTTON_PRESS);
    conn.create_window(0, win, screen.root, 0, 0, 480, 300, 0, WindowClass::INPUT_OUTPUT, 0, &aux).unwrap();
    let wm_class = format!("{class}\0{class}\0");
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, wm_class.as_bytes()).unwrap();
    conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes()).unwrap();
    let utf8 = conn.intern_atom(false, b"UTF8_STRING").unwrap().reply().unwrap().atom;
    let net_name = conn.intern_atom(false, b"_NET_WM_NAME").unwrap().reply().unwrap().atom;
    conn.change_property8(PropMode::REPLACE, win, net_name, utf8, title.as_bytes()).unwrap();
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS").unwrap().reply().unwrap().atom;
    let delete = conn.intern_atom(false, b"WM_DELETE_WINDOW").unwrap().reply().unwrap().atom;
    conn.change_property32(PropMode::REPLACE, win, protocols, AtomEnum::ATOM, &[delete]).unwrap();
    conn.map_window(win).unwrap();
    conn.flush().unwrap();
    loop {
        match conn.wait_for_event() {
            Ok(Event::ButtonPress(b)) => {
                // FAKEWIN_LOG: append "<title> button <n>" so scripts can see which window got a click.
                if let Some(path) = std::env::var_os("FAKEWIN_LOG") {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                        let _ = writeln!(f, "{title} button {}", b.detail);
                    }
                }
            }
            Ok(Event::ClientMessage(m)) if m.type_ == protocols && m.data.as_data32()[0] == delete => return,
            Ok(_) => {}
            Err(_) => return,
        }
    }
}
