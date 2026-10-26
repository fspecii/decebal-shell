//! EWMH tracker (_NET_CLIENT_LIST / _NET_ACTIVE_WINDOW), used on dwm, i3, openbox, bspwm, ...

use super::{mark_occupied, Control, Notify, Tracker, Window, Workspace};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

/// Splits a `WM_CLASS` property (`instance\0class\0`) and returns the class, falling back to the instance.
pub fn parse_wm_class(raw: &[u8]) -> String {
    let mut parts = raw.split(|b| *b == 0).filter(|p| !p.is_empty());
    let instance = parts.next();
    let class = parts.next().or(instance);
    let name = class.map(|c| String::from_utf8_lossy(c).into_owned()).unwrap_or_default();
    // Terminals such as st report their TERM as the class ("st-256color").
    name.strip_suffix("-256color").map(str::to_owned).unwrap_or(name)
}

fn atom(conn: &impl Connection, name: &str) -> Option<Atom> {
    conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom)
}

fn prop32(conn: &impl Connection, win: u32, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
    conn.get_property(false, win, prop, ty, 0, 4096).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|v| v.collect())).unwrap_or_default()
}

fn text_prop(conn: &impl Connection, win: u32, prop: Atom, ty: impl Into<Atom>) -> Vec<u8> {
    conn.get_property(false, win, prop, ty, 0, 1024).ok().and_then(|c| c.reply().ok()).map(|r| r.value).unwrap_or_default()
}

/// `_NET_DESKTOP_NAMES` (NUL separated UTF-8) padded or trimmed to `count` entries; desktops
/// without a name are called "1", "2", ...
pub fn desktop_names(raw: &[u8], count: usize) -> Vec<String> {
    let mut names: Vec<String> = raw.split(|b| *b == 0).map(|n| String::from_utf8_lossy(n).into_owned()).collect();
    if raw.last() == Some(&0) {
        names.pop(); // the terminator after the last name, not an empty entry
    }
    let count = if count == 0 { names.len() } else { count };
    names.resize(count, String::new());
    for (i, n) in names.iter_mut().enumerate() {
        if n.is_empty() {
            *n = (i + 1).to_string();
        }
    }
    names
}

const SOURCE_PAGER: u32 = 2;

/// Client message asking the window manager to focus `win` (`_NET_ACTIVE_WINDOW`).
pub fn activate_message(win: u32, active_atom: Atom) -> ClientMessageEvent {
    ClientMessageEvent::new(32, win, active_atom, [SOURCE_PAGER, x11rb::CURRENT_TIME, 0, 0, 0])
}

/// Client message asking the window manager to view desktop `index` (`_NET_CURRENT_DESKTOP`).
pub fn desktop_message(root: u32, index: u32, desktop_atom: Atom) -> ClientMessageEvent {
    ClientMessageEvent::new(32, root, desktop_atom, [index, x11rb::CURRENT_TIME, 0, 0, 0])
}

struct Ewmh {
    conn: RustConnection,
    root: u32,
    active_atom: Atom,
    desktop_atom: Atom,
}

impl Control for Ewmh {
    fn activate(&self, id: u64) {
        let ev = activate_message(id as u32, self.active_atom);
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        let _ = self.conn.send_event(false, self.root, mask, ev);
        let _ = self.conn.flush();
    }

    fn switch_workspace(&self, index: usize) {
        let ev = desktop_message(self.root, index as u32, self.desktop_atom);
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        let _ = self.conn.send_event(false, self.root, mask, ev);
        let _ = self.conn.flush();
    }
}

/// Name of the running EWMH window manager (`_NET_SUPPORTING_WM_CHECK` -> `_NET_WM_NAME`).
pub fn wm_name() -> Option<String> {
    let (conn, n) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[n].root;
    let check = prop32(&conn, root, atom(&conn, "_NET_SUPPORTING_WM_CHECK")?, AtomEnum::WINDOW).first().copied()?;
    let name = text_prop(&conn, check, atom(&conn, "_NET_WM_NAME")?, atom(&conn, "UTF8_STRING")?);
    (!name.is_empty()).then(|| String::from_utf8_lossy(&name).into_owned())
}

pub fn spawn(tracker: Tracker, notify: Notify) -> bool {
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return false;
    };
    let root = conn.setup().roots[screen_num].root;
    let (Some(client_list), Some(active), Some(net_name), Some(utf8)) = (atom(&conn, "_NET_CLIENT_LIST"), atom(&conn, "_NET_ACTIVE_WINDOW"), atom(&conn, "_NET_WM_NAME"), atom(&conn, "UTF8_STRING"))
    else {
        return false;
    };
    // The workspace atoms are optional: window managers without desktops just never change them.
    let (Some(current), Some(names), Some(number), Some(wm_desktop)) =
        (atom(&conn, "_NET_CURRENT_DESKTOP"), atom(&conn, "_NET_DESKTOP_NAMES"), atom(&conn, "_NET_NUMBER_OF_DESKTOPS"), atom(&conn, "_NET_WM_DESKTOP"))
    else {
        return false;
    };
    if conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE)).is_err() {
        return false;
    }
    let _ = conn.flush();
    // One connection for both directions: this thread reads, the UI thread sends client messages.
    let conn = Arc::new(Ewmh { conn, root, active_atom: active, desktop_atom: current });
    tracker.set_control(conn.clone());

    std::thread::spawn(move || {
        let conn = &conn.conn;
        loop {
            let (windows, workspaces) = {
                let active_win = prop32(conn, root, active, AtomEnum::WINDOW).first().copied().unwrap_or(0);
                let windows = prop32(conn, root, client_list, AtomEnum::WINDOW)
                    .into_iter()
                    .map(|w| {
                        // Tag moves of a window arrive as property changes on the window itself.
                        let _ = conn.change_window_attributes(w, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE));
                        let desktop = prop32(conn, w, wm_desktop, AtomEnum::CARDINAL).first().copied().filter(|d| *d != u32::MAX);
                        Window {
                            id: w as u64,
                            app_id: parse_wm_class(&text_prop(conn, w, AtomEnum::WM_CLASS.into(), AtomEnum::STRING)),
                            title: String::from_utf8_lossy(&text_prop(conn, w, net_name, utf8)).into_owned(),
                            active: w == active_win,
                            desktop,
                        }
                    })
                    .collect::<Vec<_>>();
                let count = prop32(conn, root, number, AtomEnum::CARDINAL).first().copied().unwrap_or(0) as usize;
                let cur = prop32(conn, root, current, AtomEnum::CARDINAL).first().copied();
                let mut workspaces: Vec<Workspace> = if count > 1 {
                    desktop_names(&text_prop(conn, root, names, utf8), count).into_iter().map(|name| Workspace { name, active: false, occupied: false }).collect()
                } else {
                    Vec::new() // a single desktop is not worth an indicator
                };
                if let Some(ws) = cur.and_then(|c| workspaces.get_mut(c as usize)) {
                    ws.active = true;
                }
                mark_occupied(&mut workspaces, &windows);
                (windows, workspaces)
            };
            if tracker.replace(windows, workspaces) {
                notify();
            }
            // Block until something we display changes.
            loop {
                match conn.wait_for_event() {
                    Ok(Event::PropertyNotify(e)) if [client_list, active, current, names, number, wm_desktop].contains(&e.atom) => break,
                    Ok(_) => {}
                    Err(_) => return,
                }
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
        assert_eq!(parse_wm_class(b"st-256color\0st-256color\0"), "st");
    }

    #[test]
    fn desktop_names_split_and_fill_gaps() {
        assert_eq!(desktop_names(b"web\0code\0", 2), vec!["web", "code"]);
        assert_eq!(desktop_names(b"web\0", 3), vec!["web", "2", "3"]);
        assert_eq!(desktop_names(b"", 2), vec!["1", "2"]);
        assert_eq!(desktop_names(b"a\0\0c\0", 3), vec!["a", "2", "c"]);
        assert_eq!(desktop_names(b"a\0b\0c\0", 2), vec!["a", "b"]);
    }

    #[test]
    fn ewmh_messages_carry_the_request() {
        let m = activate_message(77, 5);
        assert_eq!((m.window, m.type_, m.format), (77, 5, 32));
        assert_eq!(m.data.as_data32()[0], SOURCE_PAGER);
        let d = desktop_message(1, 4, 9);
        assert_eq!((d.window, d.type_, d.data.as_data32()[0]), (1, 9, 4));
    }
}
