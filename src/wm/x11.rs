//! EWMH tracker (_NET_CLIENT_LIST / _NET_ACTIVE_WINDOW), used on dwm, i3, openbox, bspwm, ...

use super::{mark_occupied, mark_urgent, Control, Geometry, Notify, Tracker, Window, Workspace};
use std::os::fd::AsRawFd;
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

/// `WM_HINTS` flag word bit for the ICCCM urgency hint (`UrgencyHint`, 1 << 8).
const URGENCY_HINT: u32 = 1 << 8;

/// ICCCM urgency: the first word of `WM_HINTS` carries the flags.
pub fn hints_urgent(wm_hints: &[u32]) -> bool {
    wm_hints.first().is_some_and(|flags| flags & URGENCY_HINT != 0)
}

/// (minimised, demands attention) from a `_NET_WM_STATE` atom list; either atom may be unknown to the server.
pub fn net_state_flags(states: &[u32], hidden: Option<Atom>, demands: Option<Atom>) -> (bool, bool) {
    (hidden.is_some_and(|a| states.contains(&a)), demands.is_some_and(|a| states.contains(&a)))
}

/// A window's rectangle in root coordinates from the root-relative position of its origin and its own size.
pub fn root_geometry(origin: (i16, i16), size: (u16, u16)) -> Geometry {
    Geometry { x: origin.0 as i32, y: origin.1 as i32, w: size.0 as u32, h: size.1 as u32 }
}

/// Size of the whole desktop from `_NET_DESKTOP_GEOMETRY` (`width height`); empty or zero sizes are no answer.
pub fn desktop_size(raw: &[u32]) -> Option<(u32, u32)> {
    match raw {
        [w, h, ..] if *w > 0 && *h > 0 => Some((*w, *h)),
        _ => None,
    }
}

/// Whether an event means the window list or something a task or the pager shows may have changed. Only managed clients get
/// structure events (the root selects properties only), so any configure / map / unmap is one of them.
pub fn event_refreshes(event: &Event, watched: &[Atom]) -> bool {
    match event {
        Event::PropertyNotify(e) => watched.contains(&e.atom),
        Event::ConfigureNotify(_) | Event::MapNotify(_) | Event::UnmapNotify(_) => true,
        _ => false,
    }
}

/// The window's origin and size in root coordinates (two pipelined requests; None for an unmapped or vanished window).
fn window_geometry(conn: &impl Connection, root: u32, win: u32) -> Option<Geometry> {
    let size = conn.get_geometry(win).ok()?;
    let origin = conn.translate_coordinates(win, root, 0, 0).ok()?;
    let (size, origin) = (size.reply().ok()?, origin.reply().ok()?);
    origin.same_screen.then(|| root_geometry((origin.dst_x, origin.dst_y), (size.width, size.height)))
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

/// Client message asking the window manager to close `win` (`_NET_CLOSE_WINDOW`).
pub fn close_message(win: u32, close_atom: Atom) -> ClientMessageEvent {
    ClientMessageEvent::new(32, win, close_atom, [x11rb::CURRENT_TIME, SOURCE_PAGER, 0, 0, 0])
}

/// ICCCM `WM_CHANGE_STATE` to IconicState: asks the window manager to minimise `win`.
pub fn iconify_message(win: u32, change_state_atom: Atom) -> ClientMessageEvent {
    ClientMessageEvent::new(32, win, change_state_atom, [ICONIC_STATE, 0, 0, 0, 0])
}

const ICONIC_STATE: u32 = 3;

struct Ewmh {
    conn: RustConnection,
    root: u32,
    active_atom: Atom,
    desktop_atom: Atom,
    close_atom: Option<Atom>,
    change_state_atom: Option<Atom>,
}

impl Ewmh {
    fn to_root(&self, ev: ClientMessageEvent) {
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        let _ = self.conn.send_event(false, self.root, mask, ev);
        let _ = self.conn.flush();
    }
}

impl Control for Ewmh {
    fn activate(&self, id: u64) {
        let ev = activate_message(id as u32, self.active_atom);
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        let _ = self.conn.send_event(false, self.root, mask, ev);
        let _ = self.conn.flush();
    }

    fn minimize(&self, id: u64) {
        if let Some(a) = self.change_state_atom {
            self.to_root(iconify_message(id as u32, a));
        }
    }

    fn close(&self, id: u64) {
        if let Some(a) = self.close_atom {
            self.to_root(close_message(id as u32, a));
        }
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
    let (conn, n) = crate::x11_connection::connect(None).ok()?;
    let root = conn.setup().roots[n].root;
    let check = prop32(&conn, root, atom(&conn, "_NET_SUPPORTING_WM_CHECK")?, AtomEnum::WINDOW).first().copied()?;
    let name = text_prop(&conn, check, atom(&conn, "_NET_WM_NAME")?, atom(&conn, "UTF8_STRING")?);
    (!name.is_empty()).then(|| String::from_utf8_lossy(&name).into_owned())
}

pub fn spawn(tracker: Tracker, notify: Notify) -> bool {
    let Ok((conn, screen_num)) = crate::x11_connection::connect(None) else {
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
    let conn = Arc::new(Ewmh { close_atom: atom(&conn, "_NET_CLOSE_WINDOW"), change_state_atom: atom(&conn, "WM_CHANGE_STATE"), conn, root, active_atom: active, desktop_atom: current });
    tracker.set_control(conn.clone());
    let (wm_state, demands, hidden) = (atom(&conn.conn, "_NET_WM_STATE"), atom(&conn.conn, "_NET_WM_STATE_DEMANDS_ATTENTION"), atom(&conn.conn, "_NET_WM_STATE_HIDDEN"));
    let desktop_geometry = atom(&conn.conn, "_NET_DESKTOP_GEOMETRY");

    let Ok((tracker, stop)) = tracker.worker() else { return false };
    std::thread::spawn(move || {
        let conn = &conn.conn;
        loop {
            if !matches!(crate::backend::poll_readable(&[stop.as_raw_fd()], Some(std::time::Duration::ZERO)), Ok(ready) if !ready[0]) {
                return;
            }
            let (windows, workspaces, screen) = {
                let active_win = prop32(conn, root, active, AtomEnum::WINDOW).first().copied().unwrap_or(0);
                let windows = prop32(conn, root, client_list, AtomEnum::WINDOW)
                    .into_iter()
                    .map(|w| {
                        // Tag moves, state and urgency arrive as property changes on the window itself, moves and resizes as
                        // structure events.
                        let _ = conn.change_window_attributes(w, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY));
                        let desktop = prop32(conn, w, wm_desktop, AtomEnum::CARDINAL).first().copied().filter(|d| *d != u32::MAX);
                        let states = wm_state.map(|ws| prop32(conn, w, ws, AtomEnum::ATOM)).unwrap_or_default();
                        let (minimized, demanded) = net_state_flags(&states, hidden, demands);
                        let attention = demanded || hints_urgent(&prop32(conn, w, AtomEnum::WM_HINTS.into(), AtomEnum::WM_HINTS));
                        let mut title = text_prop(conn, w, net_name, utf8);
                        if title.is_empty() {
                            title = text_prop(conn, w, AtomEnum::WM_NAME.into(), AtomEnum::STRING);
                        }
                        Window {
                            id: w as u64,
                            app_id: parse_wm_class(&text_prop(conn, w, AtomEnum::WM_CLASS.into(), AtomEnum::STRING)),
                            title: String::from_utf8_lossy(&title).into_owned(),
                            active: w == active_win,
                            desktop,
                            minimized,
                            attention,
                            geometry: if minimized { None } else { window_geometry(conn, root, w) },
                        }
                    })
                    .collect::<Vec<_>>();
                let count = prop32(conn, root, number, AtomEnum::CARDINAL).first().copied().unwrap_or(0) as usize;
                let cur = prop32(conn, root, current, AtomEnum::CARDINAL).first().copied();
                let mut workspaces: Vec<Workspace> = if count > 1 {
                    desktop_names(&text_prop(conn, root, names, utf8), count).into_iter().map(|name| Workspace { name, ..Default::default() }).collect()
                } else {
                    Vec::new() // a single desktop is not worth an indicator
                };
                if let Some(ws) = cur.and_then(|c| workspaces.get_mut(c as usize)) {
                    ws.active = true;
                }
                mark_occupied(&mut workspaces, &windows);
                mark_urgent(&mut workspaces, &windows);
                let screen = desktop_geometry.and_then(|a| desktop_size(&prop32(conn, root, a, AtomEnum::CARDINAL))).or_else(|| {
                    let g = conn.get_geometry(root).ok()?.reply().ok()?;
                    Some((g.width as u32, g.height as u32))
                });
                (windows, workspaces, screen)
            };
            if tracker.replace_all(windows, workspaces, screen) {
                notify();
            }
            // Drain x11rb's queued events before polling its socket. Last-owner drop
            // closes `stop`, waking this wait even when the X server has no events.
            let watched: Vec<Atom> = [client_list, active, current, names, number, wm_desktop, net_name, AtomEnum::WM_CLASS.into(), AtomEnum::WM_NAME.into(), AtomEnum::WM_HINTS.into()]
                .into_iter()
                .chain(wm_state)
                .chain(desktop_geometry)
                .collect();
            loop {
                match conn.poll_for_event() {
                    Ok(Some(event)) if event_refreshes(&event, &watched) => {
                        // one rebuild covers a whole burst (a window drag sends a configure per motion)
                        loop {
                            match conn.poll_for_event() {
                                Ok(Some(_)) => {}
                                Ok(None) => break,
                                Err(_) => return,
                            }
                        }
                        break;
                    }
                    Ok(Some(_)) => continue,
                    Err(_) => return,
                    Ok(None) => {}
                }
                match crate::backend::poll_readable(&[conn.stream().as_raw_fd(), stop.as_raw_fd()], None) {
                    Ok(ready) if !ready[1] => {}
                    _ => return,
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
    fn net_wm_state_and_urgency_hint_become_flags() {
        assert_eq!(net_state_flags(&[7, 9], Some(7), Some(8)), (true, false));
        assert_eq!(net_state_flags(&[8], Some(7), Some(8)), (false, true));
        assert_eq!(net_state_flags(&[], Some(7), Some(8)), (false, false));
        // an atom the server does not know can never match
        assert_eq!(net_state_flags(&[7, 8], None, None), (false, false));
        assert!(hints_urgent(&[URGENCY_HINT | 1, 0]));
        assert!(!hints_urgent(&[1 | (1 << 6), 0]));
        assert!(!hints_urgent(&[]));
    }

    #[test]
    fn geometry_is_the_root_origin_plus_the_window_size() {
        assert_eq!(root_geometry((-12, 40), (300, 200)), Geometry { x: -12, y: 40, w: 300, h: 200 });
        assert_eq!(desktop_size(&[1920, 1080]), Some((1920, 1080)));
        assert_eq!(desktop_size(&[1920, 1080, 5]), Some((1920, 1080)));
        assert_eq!(desktop_size(&[1920]), None);
        assert_eq!(desktop_size(&[0, 1080]), None);
        assert_eq!(desktop_size(&[]), None);
    }

    #[test]
    fn only_relevant_events_refresh_the_snapshot() {
        use x11rb::protocol::xproto::{ConfigureNotifyEvent, MapNotifyEvent, PropertyNotifyEvent};
        let prop = |atom| Event::PropertyNotify(PropertyNotifyEvent { response_type: 28, sequence: 0, window: 1, atom, time: 0, state: Property::NEW_VALUE });
        assert!(event_refreshes(&prop(40), &[10, 40]));
        assert!(!event_refreshes(&prop(41), &[10, 40]));
        let configure = Event::ConfigureNotify(ConfigureNotifyEvent { response_type: 22, sequence: 0, event: 1, window: 1, above_sibling: 0, x: 3, y: 4, width: 5, height: 6, border_width: 0, override_redirect: false });
        assert!(event_refreshes(&configure, &[]));
        let map = Event::MapNotify(MapNotifyEvent { response_type: 19, sequence: 0, event: 1, window: 1, override_redirect: false });
        assert!(event_refreshes(&map, &[]));
        assert!(!event_refreshes(&Event::Unknown(vec![0u8; 32]), &[]));
    }

    #[test]
    fn ewmh_messages_carry_the_request() {
        let m = activate_message(77, 5);
        assert_eq!((m.window, m.type_, m.format), (77, 5, 32));
        assert_eq!(m.data.as_data32()[0], SOURCE_PAGER);
        let d = desktop_message(1, 4, 9);
        assert_eq!((d.window, d.type_, d.data.as_data32()[0]), (1, 9, 4));
        let c = close_message(5, 11);
        assert_eq!((c.window, c.type_, c.format), (5, 11, 32));
        let i = iconify_message(5, 12);
        assert_eq!((i.window, i.type_, i.data.as_data32()[0]), (5, 12, ICONIC_STATE));
    }
}
