//! Pasting into the window that had the focus before the picker opened: remember the focus, give it back,
//! then press Ctrl+V with the XTest extension (no xdotool needed).

use std::time::{Duration, Instant};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::*;
use x11rb::protocol::xtest::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

/// `Control_L` and `v`.
const SYM_CONTROL: u32 = 0xffe3;
const SYM_V: u32 = 0x76;
/// Modifiers whose being held would turn Ctrl+V into something else (shift, control, alt, super).
const HELD_MASK: u16 = 0x1 | 0x4 | 0x8 | 0x40;

/// The window that has the keyboard focus right now (`None` for no focus or the pointer-root pseudo window).
pub fn current_focus() -> Option<Window> {
    let (conn, _) = x11rb::connect(None).ok()?;
    let f = conn.get_input_focus().ok()?.reply().ok()?.focus;
    (f > 1).then_some(f)
}

/// First keycode that produces `sym` in any column of the keyboard mapping.
fn keycode_of(conn: &RustConnection, sym: u32) -> Option<u8> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let m = conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
    let per = usize::from(m.keysyms_per_keycode).max(1);
    // Prefer the first column (the first layout's unshifted symbol), then anything else.
    let found = |col: Option<usize>| {
        m.keysyms.chunks(per).position(|c| match col {
            Some(i) => c.get(i) == Some(&sym),
            None => c.contains(&sym),
        })
    };
    found(Some(0)).or_else(|| found(None)).map(|i| min + i as u8)
}

/// Gives the focus back to `focus` and presses Ctrl+V. `Ok(false)` when the XTest extension is missing
/// (the clipboard is set; the user pastes by hand).
pub fn paste(focus: Option<Window>) -> Result<bool, String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    let (conn, n) = x11rb::connect(None).map_err(|x| e(&x))?;
    let root = conn.setup().roots[n].root;
    // The popup's connection is closing in the server right now; give it a moment, so its focus revert cannot undo ours.
    std::thread::sleep(Duration::from_millis(15));
    if let Some(w) = focus {
        let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, w, x11rb::CURRENT_TIME);
    }
    if conn.extension_information(xtest::X11_EXTENSION_NAME).map_err(|x| e(&x))?.is_none() {
        return Ok(false);
    }
    // Alt+1 and friends: wait for the fingers to let go, or the application sees Ctrl+Alt+V.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        let mask = conn.query_pointer(root).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?.mask;
        if u16::from(mask) & HELD_MASK == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (Some(ctrl), Some(v)) = (keycode_of(&conn, SYM_CONTROL), keycode_of(&conn, SYM_V)) else { return Ok(false) };
    for (ty, key) in [(KEY_PRESS_EVENT, ctrl), (KEY_PRESS_EVENT, v), (KEY_RELEASE_EVENT, v), (KEY_RELEASE_EVENT, ctrl)] {
        conn.xtest_fake_input(ty, key, 0, root, 0, 0, 0).map_err(|x| e(&x))?;
    }
    conn.flush().map_err(|x| e(&x))?;
    // A round trip: the server has handled the key events before we exit.
    let _ = conn.get_input_focus().map_err(|x| e(&x))?.reply();
    Ok(true)
}
