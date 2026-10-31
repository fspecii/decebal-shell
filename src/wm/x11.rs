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
/// structure events; root structure events also refresh the desktop-size fallback.
pub fn event_refreshes(event: &Event, watched: &[Atom]) -> bool {
    match event {
        Event::PropertyNotify(e) => watched.contains(&e.atom),
        Event::ConfigureNotify(_) | Event::MapNotify(_) | Event::UnmapNotify(_) => true,
        _ => false,
    }
}

/// Size of the whole desktop: `_NET_DESKTOP_GEOMETRY` when the window manager sets it, else the root window's own size (`root`, asked
/// only when needed). The root window is selected for structure events, so a resize of it refreshes the snapshot.
pub fn desktop_extent(property: &[u32], root: impl FnOnce() -> Option<(u32, u32)>) -> Option<(u32, u32)> {
    desktop_size(property).or_else(root)
}

/// Most events one pass reads before it hands the thread back to its stop check and a rebuild.
pub const BURST_EVENTS: usize = 64;
/// Longest one pass reads for.
pub const BURST_TIME: std::time::Duration = std::time::Duration::from_millis(25);

/// What a pass over the queued events decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// Something relevant was seen: rebuild the snapshot.
    Refresh,
    /// The queue is empty and nothing relevant was seen: wait on the sockets again.
    Idle,
    /// The budget ran out: rescan the internal queue before waiting on sockets.
    More,
    /// The last owner went away: end the thread.
    Stop,
    /// The connection failed: end the thread.
    Closed,
}

/// Reads the events already queued without blocking. It checks `stopped` before every event and gives up after `max` events or
/// `budget`, so a producer that never lets the queue run dry (a window drag, a client rewriting a property in a loop) can neither
/// delay a rebuild nor keep the thread alive after its last owner left.
pub fn scan_events<E>(next: &mut impl FnMut() -> Result<Option<Event>, E>, stopped: &mut impl FnMut() -> bool, relevant: &impl Fn(&Event) -> bool, max: usize, budget: std::time::Duration) -> Pass {
    let start = std::time::Instant::now();
    let (mut seen, mut refresh) = (0usize, false);
    loop {
        if stopped() {
            return Pass::Stop;
        }
        if seen >= max || start.elapsed() >= budget {
            return if refresh { Pass::Refresh } else { Pass::More };
        }
        match next() {
            Err(_) => return Pass::Closed,
            Ok(None) => return if refresh { Pass::Refresh } else { Pass::Idle },
            Ok(Some(event)) => {
                seen += 1;
                refresh |= relevant(&event);
            }
        }
    }
}

/// The window's client origin and size in root coordinates (two pipelined requests; `None` when either fails, e.g. the window vanished). The
/// map state is not checked: a minimised window is skipped by the caller, any other gets its configured rectangle.
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
    if conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY)).is_err() {
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
        // the last public owner closes `stop`
        let mut stopped = || !matches!(crate::backend::poll_readable(&[stop.as_raw_fd()], Some(std::time::Duration::ZERO)), Ok(ready) if !ready[0]);
        loop {
            if stopped() {
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
                let property = desktop_geometry.map(|a| prop32(conn, root, a, AtomEnum::CARDINAL)).unwrap_or_default();
                let screen = desktop_extent(&property, || {
                    let g = conn.get_geometry(root).ok()?.reply().ok()?;
                    Some((g.width as u32, g.height as u32))
                });
                (windows, workspaces, screen)
            };
            // the build above takes round trips: never publish for an owner that has already gone
            if stopped() {
                return;
            }
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
                let mut next = || conn.poll_for_event();
                let relevant = |event: &Event| event_refreshes(event, &watched);
                match scan_events(&mut next, &mut stopped, &relevant, BURST_EVENTS, BURST_TIME) {
                    Pass::Refresh => break,
                    Pass::Stop | Pass::Closed => return,
                    Pass::Idle => {}
                    Pass::More => continue,
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

    fn motion() -> Event {
        use x11rb::protocol::xproto::ConfigureNotifyEvent;
        Event::ConfigureNotify(ConfigureNotifyEvent {
            response_type: 22,
            sequence: 0,
            event: 1,
            window: 1,
            above_sibling: 0,
            x: 3,
            y: 4,
            width: 5,
            height: 6,
            border_width: 0,
            override_redirect: false,
        })
    }

    fn noise() -> Event {
        Event::Unknown(vec![0u8; 32])
    }

    #[test]
    fn a_producer_that_never_runs_dry_cannot_hold_the_scan_open() {
        use std::time::Duration;
        let always = |e: fn() -> Event| {
            let mut counted = 0usize;
            let verdict = scan_events::<()>(
                &mut || {
                    counted += 1;
                    Ok(Some(e()))
                },
                &mut || false,
                &|ev| event_refreshes(ev, &[]),
                BURST_EVENTS,
                Duration::from_secs(60),
            );
            (verdict, counted)
        };
        // relevant events forever: a refresh after exactly the cap, not an endless drain
        assert_eq!(always(motion), (Pass::Refresh, BURST_EVENTS));
        // Irrelevant events forever: yield to the stop check, then rescan the buffered queue.
        assert_eq!(always(noise), (Pass::More, BURST_EVENTS));
    }

    #[test]
    fn a_relevant_event_behind_a_buffered_noise_burst_is_not_stranded() {
        let mut queue: std::collections::VecDeque<_> = (0..BURST_EVENTS).map(|_| noise()).chain(std::iter::once(motion())).collect();
        let mut next = || Ok::<_, ()>(queue.pop_front());
        let relevant = |ev: &Event| event_refreshes(ev, &[]);
        let mut running = || false;
        assert_eq!(scan_events(&mut next, &mut running, &relevant, BURST_EVENTS, BURST_TIME), Pass::More);
        assert_eq!(scan_events(&mut next, &mut running, &relevant, BURST_EVENTS, BURST_TIME), Pass::Refresh);
        assert_eq!(scan_events(&mut next, &mut running, &relevant, BURST_EVENTS, BURST_TIME), Pass::Idle);
    }

    #[test]
    fn the_scan_ends_on_time_even_when_every_event_is_slow() {
        use std::time::Duration;
        let mut counted = 0usize;
        let verdict = scan_events::<()>(
            &mut || {
                counted += 1;
                std::thread::sleep(Duration::from_millis(2));
                Ok(Some(motion()))
            },
            &mut || false,
            &|ev| event_refreshes(ev, &[]),
            usize::MAX,
            Duration::from_millis(20),
        );
        assert_eq!(verdict, Pass::Refresh);
        assert!((1..200).contains(&counted), "a 20 ms budget read {counted} events");
    }

    #[test]
    fn the_scan_notices_the_last_owner_leaving_between_events() {
        use std::cell::Cell;
        use std::time::Duration;
        let pulls = Cell::new(0usize);
        let verdict = scan_events::<()>(
            &mut || {
                pulls.set(pulls.get() + 1);
                Ok(Some(motion()))
            },
            // the owner leaves after the fifth event
            &mut || pulls.get() >= 5,
            &|ev| event_refreshes(ev, &[]),
            usize::MAX,
            Duration::from_secs(60),
        );
        assert_eq!((verdict, pulls.get()), (Pass::Stop, 5), "no event is read once the owner is gone");
        // a stop that is already set reads nothing at all
        let read = Cell::new(0usize);
        let verdict = scan_events::<()>(
            &mut || {
                read.set(read.get() + 1);
                Ok(None)
            },
            &mut || true,
            &|_| true,
            BURST_EVENTS,
            BURST_TIME,
        );
        assert_eq!((verdict, read.get()), (Pass::Stop, 0));
    }

    #[test]
    fn a_quiet_queue_and_a_broken_connection_are_told_apart() {
        let mut queue = vec![Ok(Some(noise())), Ok(None)].into_iter();
        let verdict = scan_events::<()>(&mut || queue.next().unwrap(), &mut || false, &|ev| event_refreshes(ev, &[]), BURST_EVENTS, BURST_TIME);
        assert_eq!(verdict, Pass::Idle);
        let mut queue = vec![Ok(Some(motion())), Err(())].into_iter();
        let verdict = scan_events(&mut || queue.next().unwrap(), &mut || false, &|ev| event_refreshes(ev, &[]), BURST_EVENTS, BURST_TIME);
        assert_eq!(verdict, Pass::Closed);
        let mut queue = vec![Ok(Some(motion())), Ok(None)].into_iter();
        let verdict = scan_events::<()>(&mut || queue.next().unwrap(), &mut || false, &|ev| event_refreshes(ev, &[]), BURST_EVENTS, BURST_TIME);
        assert_eq!(verdict, Pass::Refresh, "a short burst is read to its end");
    }

    #[test]
    fn the_desktop_size_falls_back_to_the_root_window_and_follows_its_resizes() {
        use std::cell::Cell;
        let asked = Cell::new(0);
        let root = Cell::new(Some((1600u32, 900u32)));
        let ask = || {
            asked.set(asked.get() + 1);
            root.get()
        };
        // a window manager that publishes the size is believed and the root window is not even asked
        assert_eq!(desktop_extent(&[2560, 1440], ask), Some((2560, 1440)));
        assert_eq!(asked.get(), 0);
        // one that does not (rdwm sets no _NET_DESKTOP_GEOMETRY): the root window's size, and a resize of the root changes it
        assert_eq!(desktop_extent(&[], ask), Some((1600, 900)));
        root.set(Some((1280, 720)));
        assert_eq!(desktop_extent(&[], ask), Some((1280, 720)));
        // an empty property is no answer, and a root that cannot be read leaves the size unknown
        assert_eq!(desktop_extent(&[0, 0], ask), Some((1280, 720)));
        root.set(None);
        assert_eq!(desktop_extent(&[], ask), None);
    }

    #[test]
    fn a_structure_event_of_the_root_window_refreshes_like_any_other() {
        use x11rb::protocol::xproto::ConfigureNotifyEvent;
        // a resize of the root arrives as a ConfigureNotify whose window is the root itself
        let resized = Event::ConfigureNotify(ConfigureNotifyEvent {
            response_type: 22,
            sequence: 0,
            event: 0x2a,
            window: 0x2a,
            above_sibling: 0,
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
            border_width: 0,
            override_redirect: false,
        });
        assert!(event_refreshes(&resized, &[]));
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
