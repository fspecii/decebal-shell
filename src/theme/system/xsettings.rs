//! A minimal XSETTINGS manager for X11 sessions without one, so running GTK 3 / GTK 4 apps follow a theme change live.
//!
//! GTK on X11 reads `gtk-theme-name` and friends from `settings.ini` only at startup; afterwards only an XSETTINGS manager
//! (freedesktop XSETTINGS 0.5, GDK `gdk/x11/gdksettings.c`) can change them. The GTK target alternates its generated theme between
//! the `Decebal` / `Decebal-Alt` slots on every colour change, so publishing the new `Net/ThemeName` makes running plain GTK 3 / GTK 4
//! apps reload the whole generated theme, dark or light included (GDK has no XSETTINGS name for the dark preference; the generated
//! slot itself imports the dark or light toolkit sheet). libadwaita apps are not helped: they ignore GTK themes and read the user
//! `gtk.css` palette only at startup (dark/light still follows `color-scheme`).
//!
//! * Values come from the generated `gtk-3.0/settings.ini` (the file the GTK target writes), mapped by [`MAPPING`]; only keys present
//!   are published. The file's directory is watched (inotify), so the atomic replace the system transaction does is seen.
//! * Ownership: the `_XSETTINGS_S<screen>` selection is the singleton. An existing owner (another daemon, or Decebal's own) is never
//!   replaced. Decebal's manager window carries `_DECEBAL_XSETTINGS_OWNER` = "pid start-time"; losing the selection (SelectionClear)
//!   ends the manager quietly.
//! * Stop: [`stop`] sends `_DECEBAL_XSETTINGS_QUIT` with that pid and start time to the owner window; the manager checks they are its
//!   own, releases the selection and exits. No process is ever signalled.

use super::files::ini_get;
use std::collections::BTreeMap;

/// `settings.ini` key → XSETTINGS name, integer valued or not (GDK's table, the entries the GTK target writes).
pub const MAPPING: &[(&str, &str, bool)] = &[
    ("gtk-theme-name", "Net/ThemeName", false),
    ("gtk-icon-theme-name", "Net/IconThemeName", false),
    ("gtk-cursor-theme-name", "Gtk/CursorThemeName", false),
    ("gtk-cursor-theme-size", "Gtk/CursorThemeSize", true),
    ("gtk-font-name", "Gtk/FontName", false),
    ("gtk-decoration-layout", "Gtk/DecorationLayout", false),
];

pub const OWNER_PROPERTY: &str = "_DECEBAL_XSETTINGS_OWNER";
pub const QUIT_MESSAGE: &str = "_DECEBAL_XSETTINGS_QUIT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i32),
    Str(String),
}

/// The XSETTINGS values of a `settings.ini` text: the mapped keys present in `[Settings]` (an unparsable integer is skipped).
pub fn from_ini(text: &str) -> BTreeMap<String, Value> {
    MAPPING
        .iter()
        .filter_map(|(key, name, int)| {
            let v = ini_get(text, "Settings", key)?;
            let value = if *int { Value::Int(v.trim().parse().ok()?) } else { Value::Str(v) };
            Some((name.to_string(), value))
        })
        .collect()
}

/// The published table: a serial and, per setting, its value and the serial it last changed in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub serial: u32,
    pub entries: BTreeMap<String, (Value, u32)>,
}

impl Table {
    /// Takes the new values; returns whether anything changed. A change bumps the serial once, and only the settings that
    /// changed (or appeared) carry the new serial, so clients compare per setting (spec: last-change-serial > stored serial).
    pub fn update(&mut self, values: BTreeMap<String, Value>) -> bool {
        let same = values.len() == self.entries.len() && values.iter().all(|(k, v)| self.entries.get(k).is_some_and(|(old, _)| old == v));
        if same {
            return false;
        }
        let serial = self.serial.wrapping_add(1);
        self.entries = values
            .into_iter()
            .map(|(k, v)| {
                let changed = self.entries.get(&k).is_none_or(|(old, _)| *old != v);
                let at = if changed { serial } else { self.entries[&k].1 };
                (k, (v, at))
            })
            .collect();
        self.serial = serial;
        true
    }

    /// The `_XSETTINGS_SETTINGS` property bytes, in this machine's byte order (byte 0: X11 LSBFirst 0 / MSBFirst 1), names in
    /// sorted order, every name and string padded to 4 bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(if cfg!(target_endian = "little") { 0 } else { 1 });
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&self.serial.to_ne_bytes());
        b.extend_from_slice(&(self.entries.len() as u32).to_ne_bytes());
        let pad = |b: &mut Vec<u8>| {
            while b.len() % 4 != 0 {
                b.push(0);
            }
        };
        for (name, (value, at)) in &self.entries {
            b.push(match value {
                Value::Int(_) => 0,
                Value::Str(_) => 1,
            });
            b.push(0);
            b.extend_from_slice(&(name.len() as u16).to_ne_bytes());
            b.extend_from_slice(name.as_bytes());
            pad(&mut b);
            b.extend_from_slice(&at.to_ne_bytes());
            match value {
                Value::Int(i) => b.extend_from_slice(&i.to_ne_bytes()),
                Value::Str(s) => {
                    b.extend_from_slice(&(s.len() as u32).to_ne_bytes());
                    b.extend_from_slice(s.as_bytes());
                    pad(&mut b);
                }
            }
        }
        b
    }
}

/// Who owns the XSETTINGS selection of a display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    None,
    /// Decebal's manager: its window, pid and process start time (from its marker property).
    Ours {
        window: u32,
        pid: u32,
        start: u64,
    },
    /// Another manager (or a Decebal window whose process is gone or was replaced): never touched.
    Foreign {
        window: u32,
    },
}

/// The marker value a manager window carries.
pub fn marker(pid: u32, start: u64) -> String {
    format!("{pid} {start}")
}

/// Classifies a selection owner from its marker property: ours only while that exact process (pid and start time) is alive.
pub fn classify(window: u32, marker: Option<&str>, alive: impl Fn(u32, u64) -> bool) -> Owner {
    if window == 0 {
        return Owner::None;
    }
    let parsed = marker.and_then(|m| {
        let (p, s) = m.trim().split_once(' ')?;
        Some((p.parse::<u32>().ok()?, s.parse::<u64>().ok()?))
    });
    match parsed {
        Some((pid, start)) if alive(pid, start) => Owner::Ours { window, pid, start },
        _ => Owner::Foreign { window },
    }
}

/// The X requests a selection claim needs, so the claim's order and its cleanup on every path are testable without a server.
pub trait ClaimOps {
    fn grab(&mut self) -> Result<(), String>;
    fn ungrab(&mut self);
    fn owner(&mut self) -> Result<u32, String>;
    fn set_owner(&mut self, window: u32) -> Result<(), String>;
}

/// The outcome of [`claim`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Won,
    /// The selection belongs to this window; it was not taken.
    Held(u32),
}

/// Takes the selection for `window` only if nobody owns it, atomically: the owner check and the claim happen under a server grab,
/// so no other client can claim in between (a check-then-set without the grab would silently steal a manager that started in
/// the gap). The grab covers only these requests, no file I/O or waiting, and is released on every path (also on errors).
pub fn claim(ops: &mut impl ClaimOps, window: u32) -> Result<Claim, String> {
    struct Ungrab<'a, O: ClaimOps>(&'a mut O);
    impl<O: ClaimOps> Drop for Ungrab<'_, O> {
        fn drop(&mut self) {
            self.0.ungrab();
        }
    }
    ops.grab()?;
    let g = Ungrab(ops);
    let current = g.0.owner()?;
    if current != 0 {
        return Ok(Claim::Held(current));
    }
    g.0.set_owner(window)?;
    let now = g.0.owner()?;
    Ok(if now == window { Claim::Won } else { Claim::Held(now) })
}

/// What [`ensure`] found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Decebal's manager already runs and watches the settings file.
    Running { pid: u32 },
    /// A manager was started and observed owning the selection.
    Started { pid: u32 },
    /// Another XSETTINGS manager owns the display; it is left alone (running apps follow it, not Decebal).
    Foreign,
}

/// Should the GTK target run a manager here? X11 session with a display, not Wayland, not a unit test, not suppressed
/// (`DECEBAL_XSETTINGS=0`), and the `gtk.xsettings` option on.
pub fn wanted(display: Option<&str>, wayland: bool, suppressed: bool, option_on: bool) -> bool {
    !cfg!(test) && display.is_some_and(|d| !d.is_empty()) && !wayland && !suppressed && option_on
}

#[cfg(feature = "x11")]
mod x {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass};
    use x11rb::protocol::Event;
    use x11rb::rust_connection::{DefaultStream, RustConnection};
    use x11rb::wrapper::ConnectionExt as _;

    type Conn = RustConnection<DefaultStream>;
    type R<T> = Result<T, String>;

    fn e<E: std::fmt::Display>(err: E) -> String {
        err.to_string()
    }

    fn atom(c: &Conn, name: &str) -> R<u32> {
        Ok(c.intern_atom(false, name.as_bytes()).map_err(e)?.reply().map_err(e)?.atom)
    }

    fn connect(display: &str) -> R<(Conn, usize)> {
        crate::x11_connection::connect(Some(display)).map_err(|err| format!("{display}: {err}"))
    }

    fn owner_of(c: &Conn, screen: usize) -> R<Owner> {
        let sel = atom(c, &format!("_XSETTINGS_S{screen}"))?;
        let window = c.get_selection_owner(sel).map_err(e)?.reply().map_err(e)?.owner;
        if window == 0 {
            return Ok(Owner::None);
        }
        let mark = atom(c, OWNER_PROPERTY)?;
        // A window that vanished between the two requests reads as foreign: never acted on.
        let value = c.get_property(false, window, mark, AtomEnum::ANY, 0, 64).ok().and_then(|ck| ck.reply().ok()).map(|r| String::from_utf8_lossy(&r.value).into_owned());
        Ok(classify(window, value.as_deref(), |pid, start| super::super::proc_start(pid) == Some(start)))
    }

    /// The selection owner of `display`'s default screen.
    pub fn probe(display: &str) -> R<Owner> {
        let (c, screen) = connect(display)?;
        owner_of(&c, screen)
    }

    /// A real server timestamp (ICCCM: never CurrentTime for selection ownership): a zero-length property append on our window.
    fn timestamp(c: &Conn, win: u32) -> R<u32> {
        let a = atom(c, OWNER_PROPERTY)?;
        c.change_property8(PropMode::APPEND, win, a, AtomEnum::STRING, &[]).map_err(e)?;
        c.flush().map_err(e)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(Event::PropertyNotify(ev)) = c.poll_for_event().map_err(e)? {
                if ev.window == win {
                    return Ok(ev.time);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("no PropertyNotify from the X server".into())
    }

    /// An inotify descriptor watching `dir`; closed when dropped, on every exit path.
    fn inotify_watch(dir: &Path) -> R<std::os::fd::OwnedFd> {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).map_err(e)?;
        let mask = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM;
        if unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(fd)
    }

    /// The X side of [`claim`] for one selection, with a fixed server timestamp.
    struct XClaim<'a> {
        c: &'a Conn,
        sel: u32,
        time: u32,
    }

    impl ClaimOps for XClaim<'_> {
        fn grab(&mut self) -> R<()> {
            self.c.grab_server().map_err(e)?.check().map_err(e)
        }
        fn ungrab(&mut self) {
            let _ = self.c.ungrab_server();
            let _ = self.c.flush();
        }
        fn owner(&mut self) -> R<u32> {
            Ok(self.c.get_selection_owner(self.sel).map_err(e)?.reply().map_err(e)?.owner)
        }
        fn set_owner(&mut self, window: u32) -> R<()> {
            self.c.set_selection_owner(window, self.sel, self.time).map_err(e)?.check().map_err(e)
        }
    }

    fn drain(fd: std::os::fd::RawFd) {
        let mut buf = [0u8; 4096];
        while unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    }

    /// The manager itself (`decthemectl system xsettings serve`). Returns why it ended; never replaces an existing owner.
    pub fn serve(display: &str, ini: &Path) -> R<String> {
        use std::os::fd::AsRawFd;
        let (c, screen) = connect(display)?;
        match owner_of(&c, screen)? {
            Owner::None => {}
            Owner::Ours { pid, .. } => return Ok(format!("Decebal's XSETTINGS manager already runs (pid {pid})")),
            Owner::Foreign { window } => return Err(format!("another XSETTINGS manager owns {display} (window {window:#x}); it is left alone")),
        }
        let root = c.setup().roots[screen].root;
        let sel = atom(&c, &format!("_XSETTINGS_S{screen}"))?;
        let (settings_atom, manager, mark, quit) = (atom(&c, "_XSETTINGS_SETTINGS")?, atom(&c, "MANAGER")?, atom(&c, OWNER_PROPERTY)?, atom(&c, QUIT_MESSAGE)?);
        let win = c.generate_id().map_err(e)?;
        c.create_window(0, win, root, -1, -1, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new().override_redirect(1).event_mask(EventMask::PROPERTY_CHANGE)).map_err(e)?;
        let me = super::super::Worker::current();
        c.change_property8(PropMode::REPLACE, win, mark, AtomEnum::STRING, marker(me.pid, me.start).as_bytes()).map_err(e)?;
        let mut table = Table::default();
        table.update(from_ini(&std::fs::read_to_string(ini).unwrap_or_default()));
        c.change_property8(PropMode::REPLACE, win, settings_atom, settings_atom, &table.serialize()).map_err(e)?;
        let time = timestamp(&c, win)?;
        // Everything slow (settings file, timestamp round trip) happened before; the grab covers only check-and-claim.
        if let Claim::Held(owner) = claim(&mut XClaim { c: &c, sel, time }, win)? {
            let _ = c.destroy_window(win);
            let _ = c.flush();
            return Err(format!("another XSETTINGS manager (window {owner:#x}) owns the display; it is left alone"));
        }
        let announce = ClientMessageEvent::new(32, root, manager, [time, sel, win, 0, 0]);
        c.send_event(false, root, EventMask::STRUCTURE_NOTIFY, announce).map_err(e)?;
        c.flush().map_err(e)?;
        let dir = ini.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let watch = inotify_watch(&dir)?;
        let xfd = c.stream().as_raw_fd();
        let wfd = watch.as_raw_fd();
        let release = |c: &Conn| {
            if c.get_selection_owner(sel).ok().and_then(|ck| ck.reply().ok()).is_some_and(|r| r.owner == win) {
                let _ = c.set_selection_owner(0u32, sel, time);
            }
            let _ = c.destroy_window(win);
            let _ = c.flush();
        };
        loop {
            while let Some(ev) = c.poll_for_event().map_err(e)? {
                match ev {
                    Event::SelectionClear(ev) if ev.selection == sel => {
                        let _ = c.destroy_window(win);
                        let _ = c.flush();
                        return Ok("another XSETTINGS manager took over; Decebal's ended".into());
                    }
                    Event::ClientMessage(ev) if ev.window == win && ev.type_ == quit => {
                        let d = ev.data.as_data32();
                        if d[0] == me.pid && (u64::from(d[2]) << 32 | u64::from(d[1])) == me.start {
                            release(&c);
                            return Ok("stopped on request".into());
                        }
                    }
                    _ => {}
                }
            }
            let mut fds = [libc::pollfd { fd: xfd, events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: wfd, events: libc::POLLIN, revents: 0 }];
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                release(&c);
                return Err(std::io::Error::last_os_error().to_string());
            }
            if fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                return Ok("the X server went away".into());
            }
            if fds[1].revents & libc::POLLIN != 0 {
                drain(wfd);
                // A missing file (between unlink and rename) publishes nothing new; the rename that follows triggers again.
                if let Ok(text) = std::fs::read_to_string(ini) {
                    if table.update(from_ini(&text)) {
                        c.change_property8(PropMode::REPLACE, win, settings_atom, settings_atom, &table.serialize()).map_err(e)?;
                        c.flush().map_err(e)?;
                    }
                }
            }
        }
    }

    /// Makes sure Decebal's manager publishes `ini` on `display`, unless another manager owns it. Starting is observed: the new
    /// process must be seen owning the selection (marker = its pid and start time) within two seconds, else it is an error.
    pub fn ensure(display: &str, ini: &Path) -> R<Status> {
        match probe(display)? {
            Owner::Ours { pid, .. } => return Ok(Status::Running { pid }),
            Owner::Foreign { .. } => return Ok(Status::Foreign),
            Owner::None => {}
        }
        use std::os::unix::process::CommandExt;
        let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("decthemectl"))).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("decthemectl"));
        let child = std::process::Command::new(exe)
            .args(["system", "xsettings", "serve", "--display", display, "--ini"])
            .arg(ini)
            .env("DISPLAY", display)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|err| format!("could not start the XSETTINGS manager: {err}"))?;
        // Every exit below either hands the serving child to a reaper or ends it through `Launched`'s drop.
        let mut launched = Launched(Some(child));
        let pid = launched.0.as_ref().map(|c| c.id()).unwrap_or_default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(status) = launched.0.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
                launched.0 = None; // reaped
                return Err(format!("the XSETTINGS manager exited at once ({status})"));
            }
            match probe(display)? {
                Owner::Ours { pid: owner, .. } if owner == pid => {
                    launched.serve_on();
                    return Ok(Status::Started { pid });
                }
                // a concurrent start won (or another manager appeared): ours sees that and ends; `Launched` reaps it
                Owner::Ours { pid: owner, .. } => return Ok(Status::Running { pid: owner }),
                Owner::Foreign { .. } => return Ok(Status::Foreign),
                Owner::None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        Err("the XSETTINGS manager did not take the selection within 2 s; it was stopped".into())
    }

    /// The manager child [`ensure`] started. Dropped while still held, it gets two seconds to end by itself (it does when it finds
    /// another owner) and is then killed. Only this exact, not yet reaped child is signalled: its pid cannot have been reused.
    struct Launched(Option<std::process::Child>);

    impl Launched {
        /// It serves: a thread waits for it, so it never lingers as a zombie of a long-lived caller.
        fn serve_on(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = std::thread::Builder::new().name("decebal-xsettings-reaper".into()).spawn(move || {
                    let _ = child.wait();
                });
            }
        }
    }

    impl Drop for Launched {
        fn drop(&mut self) {
            let Some(mut child) = self.0.take() else { return };
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Asks Decebal's manager (proved by its marker) to release the selection and exit; another manager is never touched.
    /// Returns whether a Decebal manager was stopped.
    pub fn stop(display: &str) -> R<bool> {
        let (c, screen) = connect(display)?;
        let Owner::Ours { window, pid, start } = owner_of(&c, screen)? else { return Ok(false) };
        let quit = atom(&c, QUIT_MESSAGE)?;
        let msg = ClientMessageEvent::new(32, window, quit, [pid, start as u32, (start >> 32) as u32, 0, 0]);
        c.send_event(false, window, EventMask::NO_EVENT, msg).map_err(e)?.check().map_err(e)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !matches!(owner_of(&c, screen)?, Owner::Ours { window: w, .. } if w == window) {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!("Decebal's XSETTINGS manager (pid {pid}) did not release {display} within 2 s"))
    }
}

#[cfg(feature = "x11")]
pub use x::{ensure, probe, serve, stop};

#[cfg(test)]
mod tests {
    use super::*;

    fn le32(b: &[u8], at: usize) -> u32 {
        u32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn values_come_from_the_generated_settings_ini_keys_gdk_maps() {
        let ini = "[Settings]\ngtk-theme-name=Decebal-Alt\ngtk-icon-theme-name=Papirus\ngtk-font-name=Inter 11\ngtk-cursor-theme-size=bogus\ngtk-application-prefer-dark-theme=1\n[Other]\ngtk-cursor-theme-name=Nope\n";
        let v = from_ini(ini);
        assert_eq!(v.get("Net/ThemeName"), Some(&Value::Str("Decebal-Alt".into())));
        assert_eq!(v.get("Net/IconThemeName"), Some(&Value::Str("Papirus".into())));
        assert_eq!(v.get("Gtk/FontName"), Some(&Value::Str("Inter 11".into())));
        assert!(!v.contains_key("Gtk/CursorThemeSize"), "an unparsable integer is not published");
        assert!(!v.contains_key("Gtk/CursorThemeName"), "only [Settings]");
        assert_eq!(v.len(), 3, "GDK has no XSETTINGS name for the dark preference: {v:?}");
        assert_eq!(from_ini("[Settings]\ngtk-cursor-theme-size=32\n").get("Gtk/CursorThemeSize"), Some(&Value::Int(32)));
    }

    #[test]
    fn serialization_follows_the_xsettings_layout() {
        let mut t = Table::default();
        t.update([("Net/ThemeName".to_string(), Value::Str("Decebal".into())), ("Gtk/CursorThemeSize".to_string(), Value::Int(24))].into());
        let b = t.serialize();
        assert_eq!(b[0], if cfg!(target_endian = "little") { 0 } else { 1 });
        assert_eq!(&b[1..4], &[0, 0, 0]);
        assert_eq!((le32(&b, 4), le32(&b, 8)), (1, 2), "serial and count");
        // first, in name order: Gtk/CursorThemeSize (19 bytes, padded to 20), integer
        assert_eq!((b[12], b[13]), (0, 0));
        assert_eq!(u16::from_ne_bytes([b[14], b[15]]), 19);
        assert_eq!(&b[16..35], b"Gtk/CursorThemeSize");
        assert_eq!(b[35], 0, "pad to 4");
        assert_eq!((le32(&b, 36), le32(&b, 40) as i32), (1, 24));
        // then Net/ThemeName (13 bytes, padded to 16), string "Decebal" (7, padded to 8)
        assert_eq!(b[44], 1);
        assert_eq!(u16::from_ne_bytes([b[46], b[47]]), 13);
        assert_eq!(&b[48..61], b"Net/ThemeName");
        assert_eq!((le32(&b, 64), le32(&b, 68)), (1, 7));
        assert_eq!(&b[72..79], b"Decebal");
        assert_eq!(b.len(), 80);
        assert_eq!(b.len() % 4, 0);
    }

    #[test]
    fn serials_advance_once_per_change_and_only_changed_settings_carry_them() {
        let mut t = Table::default();
        let v = |theme: &str, font: &str| -> BTreeMap<String, Value> { [("Net/ThemeName".to_string(), Value::Str(theme.into())), ("Gtk/FontName".to_string(), Value::Str(font.into()))].into() };
        assert!(t.update(v("Decebal", "Inter 11")));
        assert!(!t.update(v("Decebal", "Inter 11")), "identical values: no new serial, no property write");
        assert_eq!(t.serial, 1);
        assert!(t.update(v("Decebal-Alt", "Inter 11")));
        assert_eq!(t.serial, 2);
        assert_eq!(t.entries["Net/ThemeName"].1, 2, "the theme changed in serial 2");
        assert_eq!(t.entries["Gtk/FontName"].1, 1, "the font did not");
        assert!(t.update([("Gtk/FontName".to_string(), Value::Str("Inter 11".into()))].into()), "a removed setting is a change");
        assert!(!t.entries.contains_key("Net/ThemeName"));
    }

    #[test]
    fn only_a_live_marked_decebal_window_is_ours() {
        let alive = |pid: u32, start: u64| pid == 42 && start == 777;
        assert_eq!(classify(0, None, alive), Owner::None);
        assert_eq!(classify(9, Some("42 777"), alive), Owner::Ours { window: 9, pid: 42, start: 777 });
        assert_eq!(classify(9, Some("42 778"), alive), Owner::Foreign { window: 9 }, "same pid, other start time: a reused pid");
        assert_eq!(classify(9, None, alive), Owner::Foreign { window: 9 }, "xsettingsd, gsd-xsettings, ...: never touched");
        assert_eq!(classify(9, Some("garbage"), alive), Owner::Foreign { window: 9 });
        assert_eq!(marker(42, 777), "42 777");
    }

    /// A scripted X server for [`claim`]: owners returned in order, optional failures, and the request log.
    struct Fake {
        owners: Vec<u32>,
        fail: Option<&'static str>,
        log: Vec<String>,
    }

    impl ClaimOps for Fake {
        fn grab(&mut self) -> Result<(), String> {
            self.log.push("grab".into());
            if self.fail == Some("grab") {
                return Err("grab failed".into());
            }
            Ok(())
        }
        fn ungrab(&mut self) {
            self.log.push("ungrab".into());
        }
        fn owner(&mut self) -> Result<u32, String> {
            self.log.push("owner".into());
            if self.fail == Some("owner") {
                return Err("owner failed".into());
            }
            Ok(if self.owners.is_empty() { 0 } else { self.owners.remove(0) })
        }
        fn set_owner(&mut self, w: u32) -> Result<(), String> {
            self.log.push(format!("set {w}"));
            if self.fail == Some("set") {
                return Err("set failed".into());
            }
            Ok(())
        }
    }

    fn fake(owners: &[u32], fail: Option<&'static str>) -> Fake {
        Fake { owners: owners.to_vec(), fail, log: Vec::new() }
    }

    #[test]
    fn a_claim_checks_and_sets_under_one_grab_and_never_takes_an_owned_selection() {
        let mut f = fake(&[0, 7], None);
        assert_eq!(claim(&mut f, 7), Ok(Claim::Won));
        assert_eq!(f.log, ["grab", "owner", "set 7", "owner", "ungrab"]);
        let mut f = fake(&[99], None);
        assert_eq!(claim(&mut f, 7), Ok(Claim::Held(99)), "a manager that started before the grab is never replaced");
        assert_eq!(f.log, ["grab", "owner", "ungrab"], "no SetSelectionOwner at all");
        let mut f = fake(&[0, 99], None);
        assert_eq!(claim(&mut f, 7), Ok(Claim::Held(99)), "a claim that did not stick is reported, not assumed");
    }

    #[test]
    fn every_claim_error_releases_the_grab() {
        for step in ["owner", "set"] {
            let mut f = fake(&[0, 7], Some(step));
            assert!(claim(&mut f, 7).is_err(), "{step}");
            assert_eq!(f.log.last().map(String::as_str), Some("ungrab"), "{step}: {:?}", f.log);
            assert_eq!(f.log.iter().filter(|l| *l == "ungrab").count(), 1);
        }
        let mut f = fake(&[0], Some("grab"));
        assert!(claim(&mut f, 7).is_err());
        assert_eq!(f.log, ["grab"], "nothing was grabbed, nothing is released or claimed");
    }

    #[test]
    fn the_manager_is_wanted_only_on_x11_when_not_suppressed_or_opted_out() {
        // unit tests never start one, whatever the inputs
        assert!(!wanted(Some(":0"), false, false, true));
    }
}
