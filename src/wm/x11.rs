//! EWMH tracker (_NET_CLIENT_LIST / _NET_ACTIVE_WINDOW), used on dwm, i3, openbox, bspwm, ...

use super::{Notify, Tracker, Window};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;

/// Splits a `WM_CLASS` property (`instance\0class\0`) and returns the class, falling back to the instance.
pub fn parse_wm_class(raw: &[u8]) -> String {
    let mut parts = raw.split(|b| *b == 0).filter(|p| !p.is_empty());
    let instance = parts.next();
    let class = parts.next().or(instance);
    class.map(|c| String::from_utf8_lossy(c).into_owned()).unwrap_or_default()
}

fn atom(conn: &impl Connection, name: &str) -> Option<Atom> {
    conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom)
}

fn prop32(conn: &impl Connection, win: u32, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
    conn.get_property(false, win, prop, ty, 0, 4096)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|v| v.collect()))
        .unwrap_or_default()
}

fn text_prop(conn: &impl Connection, win: u32, prop: Atom, ty: impl Into<Atom>) -> Vec<u8> {
    conn.get_property(false, win, prop, ty, 0, 1024).ok().and_then(|c| c.reply().ok()).map(|r| r.value).unwrap_or_default()
}

pub fn spawn(tracker: Tracker, notify: Notify) -> bool {
    let Ok((conn, screen_num)) = x11rb::connect(None) else { return false };
    let root = conn.setup().roots[screen_num].root;
    let (Some(client_list), Some(active), Some(net_name), Some(utf8)) = (
        atom(&conn, "_NET_CLIENT_LIST"),
        atom(&conn, "_NET_ACTIVE_WINDOW"),
        atom(&conn, "_NET_WM_NAME"),
        atom(&conn, "UTF8_STRING"),
    ) else {
        return false;
    };
    if conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE)).is_err() {
        return false;
    }
    let _ = conn.flush();

    std::thread::spawn(move || loop {
        let windows = {
            let active_win = prop32(&conn, root, active, AtomEnum::WINDOW).first().copied().unwrap_or(0);
            prop32(&conn, root, client_list, AtomEnum::WINDOW)
                .into_iter()
                .map(|w| Window {
                    app_id: parse_wm_class(&text_prop(&conn, w, AtomEnum::WM_CLASS.into(), AtomEnum::STRING)),
                    title: String::from_utf8_lossy(&text_prop(&conn, w, net_name, utf8)).into_owned(),
                    active: w == active_win,
                })
                .collect::<Vec<_>>()
        };
        if tracker.replace(windows) {
            notify();
        }
        // Block until one of the two watched root properties changes.
        loop {
            match conn.wait_for_event() {
                Ok(Event::PropertyNotify(e)) if e.atom == client_list || e.atom == active => break,
                Ok(_) => {}
                Err(_) => return,
            }
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_prefers_class_over_instance() {
        assert_eq!(parse_wm_class(b"st\0St\0"), "St");
        assert_eq!(parse_wm_class(b"foot\0"), "foot");
        assert_eq!(parse_wm_class(b""), "");
    }
}
