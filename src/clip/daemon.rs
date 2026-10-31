//! The decclip daemon: records the CLIPBOARD selection and keeps it alive after its source exits.
//!
//! No polling: the server tells us (XFixes `SelectionNotify`) whenever the owner of CLIPBOARD changes. We then ask
//! the new owner for `TARGETS` and the text, apply the privacy rules and store it. PRIMARY is never looked at.
//! When the owner disappears and the clipboard is empty we take it over and serve the newest text.
//!
//! Other decclip processes talk to the daemon through its window: its id is the `_DECCLIP_DAEMON` property of the
//! root window, commands are `_DECCLIP_CMD` client messages (see [`Cmd`]), a text to put on the clipboard travels in
//! the `_DECCLIP_TEXT` property of the daemon's window.

use super::history::{self, History, MAX_BYTES};
use super::policy::{self, Verdict};
use crate::clipboard::{self, SelAtoms};
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

const ROOT_PROP: &str = "_DECCLIP_DAEMON";
const CMD_MSG: &str = "_DECCLIP_CMD";
const TEXT_PROP: &str = "_DECCLIP_TEXT";
const DONE_PROP: &str = "_DECCLIP_DONE";
/// How long an owner gets to answer a conversion before it is given up on.
const FETCH_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a client waits for the daemon to act on a command.
const ACK_TIMEOUT: Duration = Duration::from_millis(1500);
/// Name of the daemon's single-instance lock (`$XDG_RUNTIME_DIR/decclip-daemon.lock`).
pub const LOCK_NAME: &str = "decclip-daemon";

/// A command for the daemon (`data32[0]` of the client message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    /// Own the clipboard with the text in `_DECCLIP_TEXT` (and make it the newest entry).
    Set = 1,
    /// Forget everything and empty the clipboard.
    Clear = 2,
    /// Read the history file again (the picker deleted an entry).
    Reload = 3,
}

impl Cmd {
    fn from_u32(n: u32) -> Option<Cmd> {
        match n {
            1 => Some(Cmd::Set),
            2 => Some(Cmd::Clear),
            3 => Some(Cmd::Reload),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- pure decisions

/// What to do with an owner after its `TARGETS` answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Fetch this text target.
    Text(Atom),
    /// The password-manager hint is there.
    Secret,
    /// No text on offer (an image, files, ...).
    NoText,
}

/// Picks the text target from an owner's `TARGETS` list: UTF8_STRING, else STRING; the hint atom wins over everything.
pub fn choose(targets: &[Atom], a: &SelAtoms, hint: Atom) -> Choice {
    if hint != 0 && targets.contains(&hint) {
        Choice::Secret
    } else if targets.contains(&a.utf8) {
        Choice::Text(a.utf8)
    } else if targets.contains(&a.string) {
        Choice::Text(a.string)
    } else {
        Choice::NoText
    }
}

/// Text from a converted property: UTF8_STRING is UTF-8 (anything else is refused), STRING is Latin-1.
pub fn decode(utf8_target: bool, bytes: Vec<u8>) -> Option<String> {
    if utf8_target {
        String::from_utf8(bytes).ok()
    } else {
        Some(bytes.into_iter().map(char::from).collect())
    }
}

// ---------------------------------------------------------------- daemon state

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Targets,
    Text(Atom),
}

struct Fetch {
    owner: Window,
    stage: Stage,
    deadline: Instant,
    prop: Atom,
    /// STRING can be tried when UTF8_STRING fails (the owner listed it, or its list is unknown).
    string_ok: bool,
}

struct Daemon {
    conn: RustConnection,
    root: Window,
    win: Window,
    atoms: SelAtoms,
    clipboard: Atom,
    cmd: Atom,
    text_prop: Atom,
    done: Atom,
    hint: Atom,
    wm_class: Atom,
    /// One property per fetch generation, so a late answer of an abandoned fetch cannot be taken for the current one.
    props: [Atom; 4],
    generation: usize,
    done_count: u32,
    history: History,
    path: Option<PathBuf>,
    fetch: Option<Fetch>,
    /// The text we serve while we own the clipboard.
    serving: Option<String>,
    /// The last owner's text went into the history: only then is it worth restoring when that owner goes away.
    last_was_text: bool,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn intern(c: &RustConnection, name: &str) -> Result<Atom, String> {
    Ok(c.intern_atom(false, name.as_bytes()).map_err(err)?.reply().map_err(err)?.atom)
}

/// `WM_CLASS` names (instance and class) of `win` and of its ancestors up to the root.
fn class_chain(conn: &RustConnection, wm_class: Atom, mut win: Window, root: Window) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..8 {
        if win <= 1 || win == root {
            break;
        }
        if let Some(r) = conn.get_property(false, win, wm_class, AtomEnum::STRING, 0, 64).ok().and_then(|c| c.reply().ok()) {
            out.extend(r.value.split(|&b| b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()));
        }
        match conn.query_tree(win).ok().and_then(|c| c.reply().ok()) {
            Some(t) => win = t.parent,
            None => break,
        }
    }
    out
}

fn wait_readable(fd: RawFd, timeout: Option<Duration>) {
    let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let ms = timeout.map_or(-1, |d| i32::try_from(d.as_millis() + 1).unwrap_or(i32::MAX));
    // SAFETY: poll(2) on one valid pollfd that lives on the stack; it only reads `fd` and writes `revents`.
    unsafe { libc::poll(&mut p, 1, ms) };
}

impl Daemon {
    fn new() -> Result<Daemon, String> {
        let (conn, n) = crate::x11_connection::connect(None).map_err(err)?;
        let root = conn.setup().roots[n].root;
        conn.xfixes_query_version(5, 0).map_err(err)?.reply().map_err(|_| "the XFIXES extension is missing")?;
        let atoms = SelAtoms::intern(&conn)?;
        let clipboard = intern(&conn, "CLIPBOARD")?;
        let win = conn.generate_id().map_err(err)?;
        conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE)).map_err(err)?;
        conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"decclip").map_err(err)?;
        conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, b"decclip\0Decclip\0").map_err(err)?;
        let mask = xfixes::SelectionEventMask::SET_SELECTION_OWNER | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE;
        conn.xfixes_select_selection_input(win, clipboard, mask).map_err(err)?;
        let root_prop = intern(&conn, ROOT_PROP)?;
        conn.change_property32(PropMode::REPLACE, root, root_prop, AtomEnum::WINDOW, &[win]).map_err(err)?;
        let props = [intern(&conn, "_DECCLIP_DATA0")?, intern(&conn, "_DECCLIP_DATA1")?, intern(&conn, "_DECCLIP_DATA2")?, intern(&conn, "_DECCLIP_DATA3")?];
        let d = Daemon {
            cmd: intern(&conn, CMD_MSG)?,
            text_prop: intern(&conn, TEXT_PROP)?,
            done: intern(&conn, DONE_PROP)?,
            hint: intern(&conn, policy::HINT_TARGET)?,
            wm_class: u32::from(AtomEnum::WM_CLASS),
            props,
            generation: 0,
            done_count: 0,
            history: History::default(),
            path: history::state_path(),
            fetch: None,
            serving: None,
            last_was_text: false,
            conn,
            root,
            win,
            atoms,
            clipboard,
        };
        d.conn.flush().map_err(err)?;
        Ok(d)
    }

    fn now() -> u64 {
        crate::fuzzy::now()
    }

    fn save(&self) {
        if let Some(p) = &self.path {
            if let Err(e) = self.history.save(p) {
                eprintln!("decclip: cannot write {}: {e}", p.display());
            }
        }
    }

    fn reload(&mut self) {
        self.history = self.path.as_deref().map(History::load).unwrap_or_default();
    }

    fn owner(&self) -> Window {
        self.conn.get_selection_owner(self.clipboard).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.owner)
    }

    /// Takes the clipboard and serves `text` from now on.
    fn own(&mut self, text: &str) -> bool {
        let _ = self.conn.set_selection_owner(self.win, self.clipboard, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
        let ours = self.owner() == self.win;
        self.serving = ours.then(|| text.to_string());
        ours
    }

    /// Empties the clipboard whoever holds it: take it over, then let go (the old owner gets `SelectionClear`).
    fn drop_clipboard(&mut self) {
        self.serving = None;
        if self.owner() != x11rb::NONE {
            let _ = self.conn.set_selection_owner(self.win, self.clipboard, x11rb::CURRENT_TIME);
            let _ = self.conn.set_selection_owner(x11rb::NONE, self.clipboard, x11rb::CURRENT_TIME);
            let _ = self.conn.flush();
        }
    }

    // ---- owner changes

    fn secret_window(&self, owner: Window) -> bool {
        let mut classes = class_chain(&self.conn, self.wm_class, owner, self.root);
        // The owner is often a hidden helper window without a class: the focused window (where the copy was made) counts too.
        if let Some(focus) = self.conn.get_input_focus().ok().and_then(|c| c.reply().ok()).map(|r| r.focus) {
            classes.extend(class_chain(&self.conn, self.wm_class, focus, self.root));
        }
        policy::class_is_secret(&classes)
    }

    fn on_owner_change(&mut self, owner: Window) {
        self.fetch = None;
        if owner == self.win {
            return;
        }
        if self.serving.is_some() {
            self.serving = None;
        }
        if owner == x11rb::NONE {
            // The owner went away and nobody took over: keep what it had copied available.
            if self.last_was_text {
                if let Some(text) = self.history.latest().map(|e| e.text.clone()) {
                    self.own(&text);
                }
            }
            return;
        }
        if self.secret_window(owner) {
            self.last_was_text = false;
            return;
        }
        self.start_fetch(owner);
    }

    fn start_fetch(&mut self, owner: Window) {
        self.generation = self.generation.wrapping_add(1);
        let prop = self.props[self.generation % self.props.len()];
        self.fetch = Some(Fetch { owner, stage: Stage::Targets, deadline: Instant::now() + FETCH_TIMEOUT, prop, string_ok: true });
        let _ = self.conn.convert_selection(self.win, self.clipboard, self.atoms.targets, prop, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
    }

    fn convert(&mut self, target: Atom) {
        let Some(f) = &mut self.fetch else { return };
        f.stage = Stage::Text(target);
        f.deadline = Instant::now() + FETCH_TIMEOUT;
        let prop = f.prop;
        let _ = self.conn.convert_selection(self.win, self.clipboard, target, prop, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
    }

    fn give_up(&mut self) {
        self.fetch = None;
        self.last_was_text = false;
    }

    fn on_selection_notify(&mut self, e: SelectionNotifyEvent) {
        let Some(f) = &self.fetch else { return };
        if e.requestor != self.win || e.selection != self.clipboard || e.property != 0 && e.property != f.prop {
            return;
        }
        match f.stage {
            Stage::Targets if e.target == self.atoms.targets => self.on_targets(e.property),
            Stage::Text(t) if e.target == t => self.on_text(t, e.property),
            _ => {}
        }
    }

    fn on_targets(&mut self, property: Atom) {
        let Some(prop) = self.fetch.as_ref().map(|f| f.prop) else { return };
        if property == 0 {
            // The owner does not answer TARGETS: just try for UTF8_STRING.
            self.convert(self.atoms.utf8);
            return;
        }
        let list: Vec<Atom> =
            self.conn.get_property(true, self.win, prop, AtomEnum::ATOM, 0, 512).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(Iterator::collect)).unwrap_or_default();
        match choose(&list, &self.atoms, self.hint) {
            Choice::Text(t) => {
                if let Some(f) = &mut self.fetch {
                    f.string_ok = list.contains(&self.atoms.string);
                }
                self.convert(t);
            }
            Choice::Secret | Choice::NoText => self.give_up(),
        }
    }

    fn on_text(&mut self, target: Atom, property: Atom) {
        let Some((prop, owner, string_ok)) = self.fetch.as_ref().map(|f| (f.prop, f.owner, f.string_ok)) else { return };
        if property == 0 {
            if target == self.atoms.utf8 && string_ok {
                let string = self.atoms.string;
                self.convert(string);
            } else {
                self.give_up();
            }
            return;
        }
        let words = (MAX_BYTES / 4 + 2) as u32;
        let reply = self.conn.get_property(true, self.win, prop, AtomEnum::ANY, 0, words).ok().and_then(|c| c.reply().ok());
        self.fetch = None;
        let Some(r) = reply else { return self.last_was_text = false };
        let incr = intern(&self.conn, "INCR").unwrap_or(0);
        if r.type_ == incr || r.bytes_after > 0 || r.format != 8 {
            // Larger than we keep (INCR transfers start at ~100 KB or more): not stored.
            let _ = self.conn.delete_property(self.win, prop);
            self.last_was_text = false;
            return;
        }
        // The owner changed again while we were reading: that change has its own event.
        if self.owner() != owner {
            return;
        }
        let Some(text) = decode(target == self.atoms.utf8, r.value) else {
            self.last_was_text = false;
            return;
        };
        self.last_was_text = match policy::judge::<&str, &str>(&text, &[], &[]) {
            Verdict::Store => {
                self.history.push(&text, Self::now());
                self.save();
                true
            }
            _ => false,
        };
    }

    // ---- requests and commands

    fn on_selection_request(&mut self, r: SelectionRequestEvent) {
        if r.selection != self.clipboard {
            return;
        }
        match &self.serving {
            Some(text) => clipboard::respond(&self.conn, &r, text, &self.atoms),
            None => {
                let n = SelectionNotifyEvent { response_type: SELECTION_NOTIFY_EVENT, sequence: 0, time: r.time, requestor: r.requestor, selection: r.selection, target: r.target, property: 0 };
                let _ = self.conn.send_event(false, r.requestor, EventMask::NO_EVENT, n);
                let _ = self.conn.flush();
            }
        }
    }

    fn on_command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Set => {
                let words = (MAX_BYTES / 4 + 2) as u32;
                let text = self
                    .conn
                    .get_property(true, self.win, self.text_prop, self.atoms.utf8, 0, words)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .filter(|r| r.bytes_after == 0)
                    .and_then(|r| String::from_utf8(r.value).ok());
                if let Some(text) = text.filter(|t| History::acceptable(t)) {
                    self.fetch = None;
                    self.reload();
                    self.history.push(&text, Self::now());
                    self.save();
                    self.own(&text);
                    self.last_was_text = true;
                }
            }
            Cmd::Clear => {
                self.fetch = None;
                self.last_was_text = false;
                self.history.clear();
                self.save();
                self.drop_clipboard();
            }
            Cmd::Reload => self.reload(),
        }
        // Tell the sender (it waits for this counter to change).
        self.done_count = self.done_count.wrapping_add(1);
        let _ = self.conn.change_property32(PropMode::REPLACE, self.win, self.done, AtomEnum::CARDINAL, &[self.done_count]);
        let _ = self.conn.flush();
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::XfixesSelectionNotify(e) if e.selection == self.clipboard => self.on_owner_change(e.owner),
            Event::SelectionNotify(e) => self.on_selection_notify(e),
            Event::SelectionRequest(r) => self.on_selection_request(r),
            Event::SelectionClear(c) if c.selection == self.clipboard => self.serving = None,
            Event::ClientMessage(m) if m.type_ == self.cmd => {
                if let Some(cmd) = Cmd::from_u32(m.data.as_data32()[0]) {
                    self.on_command(cmd);
                }
            }
            _ => {}
        }
    }

    fn run(&mut self) -> Result<(), String> {
        self.reload();
        // Whatever is on the clipboard right now counts as copied.
        let owner = self.owner();
        if owner != x11rb::NONE && owner != self.win {
            self.on_owner_change(owner);
        }
        let fd = self.conn.stream().as_raw_fd();
        loop {
            while let Some(ev) = self.conn.poll_for_event().map_err(err)? {
                self.handle(ev);
            }
            if let Some(f) = &self.fetch {
                if Instant::now() >= f.deadline {
                    self.give_up();
                }
            }
            let wait = self.fetch.as_ref().map(|f| f.deadline.saturating_duration_since(Instant::now()));
            wait_readable(fd, wait);
        }
    }
}

/// Runs the daemon until the X connection ends. Only one runs per session: a second one returns at once.
pub fn run() -> Result<(), String> {
    let Some(_lock) = crate::single::acquire_signalling(LOCK_NAME, 0) else {
        eprintln!("decclip: the daemon is already running");
        return Ok(());
    };
    let mut d = Daemon::new()?;
    d.run()
}

// ---------------------------------------------------------------- talking to a running daemon

/// A short-lived connection to the daemon.
pub struct Link {
    conn: RustConnection,
    daemon: Window,
    cmd: Atom,
    text_prop: Atom,
    done: Atom,
    utf8: Atom,
    clipboard: Atom,
}

impl Link {
    /// Connects to the running daemon of this X display (`None` when there is none).
    pub fn connect() -> Option<Link> {
        let (conn, n) = crate::x11_connection::connect(None).ok()?;
        let root = conn.setup().roots[n].root;
        let root_prop = conn.intern_atom(true, ROOT_PROP.as_bytes()).ok()?.reply().ok()?.atom;
        if root_prop == 0 {
            return None;
        }
        let daemon = conn.get_property(false, root, root_prop, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?.value32()?.next()?;
        // A stale property from a crashed daemon names a window that no longer exists.
        conn.get_window_attributes(daemon).ok()?.reply().ok()?;
        let intern = |s: &str| -> Option<Atom> { Some(conn.intern_atom(false, s.as_bytes()).ok()?.reply().ok()?.atom) };
        Some(Link { daemon, cmd: intern(CMD_MSG)?, text_prop: intern(TEXT_PROP)?, done: intern(DONE_PROP)?, utf8: intern("UTF8_STRING")?, clipboard: intern("CLIPBOARD")?, conn })
    }

    fn counter(&self) -> u32 {
        self.conn.get_property(false, self.daemon, self.done, AtomEnum::CARDINAL, 0, 1).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().and_then(|mut v| v.next())).unwrap_or(0)
    }

    /// Sends the command and waits until the daemon has acted on it.
    pub fn send(&self, cmd: Cmd) -> bool {
        let before = self.counter();
        let ev = ClientMessageEvent::new(32, self.daemon, self.cmd, [cmd as u32, 0, 0, 0, 0]);
        if self.conn.send_event(false, self.daemon, EventMask::NO_EVENT, ev).is_err() || self.conn.flush().is_err() {
            return false;
        }
        let start = Instant::now();
        while start.elapsed() < ACK_TIMEOUT {
            if self.counter() != before {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    /// Makes `text` the clipboard (and the newest history entry); true once the daemon owns it.
    pub fn set_clipboard(&self, text: &str) -> bool {
        if self.conn.change_property8(PropMode::REPLACE, self.daemon, self.text_prop, self.utf8, text.as_bytes()).is_err() {
            return false;
        }
        self.send(Cmd::Set) && self.conn.get_selection_owner(self.clipboard).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.owner == self.daemon)
    }
}

/// Puts `text` on the clipboard without a daemon: a detached helper process (`decclip --serve-clipboard`) owns it
/// until someone else copies. True once the helper is the owner.
pub fn serve_detached(text: &str) -> bool {
    use std::io::Write;
    let Ok((conn, _)) = crate::x11_connection::connect(None) else { return false };
    let Some(clipboard) = conn.intern_atom(false, b"CLIPBOARD").ok().and_then(|c| c.reply().ok()).map(|r| r.atom) else { return false };
    let owner = |c: &RustConnection| c.get_selection_owner(clipboard).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.owner);
    let before = owner(&conn);
    let Ok(exe) = std::env::current_exe() else { return false };
    let child =
        std::process::Command::new("setsid").arg(exe).arg(clipboard::SERVE_ARG).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    let Ok(mut child) = child else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let start = Instant::now();
    while start.elapsed() < ACK_TIMEOUT {
        let now = owner(&conn);
        if now != 0 && now != before {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Asks a running daemon to do `cmd`; false when there is none.
pub fn tell(cmd: Cmd) -> bool {
    Link::connect().is_some_and(|l| l.send(cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: SelAtoms = SelAtoms { targets: 100, utf8: 101, string: 31, text: 102, mime_utf8: 103, mime_plain: 104 };
    const HINT: Atom = 500;

    #[test]
    fn utf8_is_preferred_then_string() {
        assert_eq!(choose(&[100, 101, 31], &A, HINT), Choice::Text(101));
        assert_eq!(choose(&[100, 31], &A, HINT), Choice::Text(31));
    }

    #[test]
    fn the_password_hint_wins_over_text() {
        assert_eq!(choose(&[100, 101, HINT], &A, HINT), Choice::Secret);
    }

    #[test]
    fn owners_without_text_are_skipped() {
        assert_eq!(choose(&[100, 700, 701], &A, HINT), Choice::NoText);
        assert_eq!(choose(&[], &A, HINT), Choice::NoText);
    }

    #[test]
    fn decoding_utf8_and_latin1() {
        assert_eq!(decode(true, "héllo €".as_bytes().to_vec()).as_deref(), Some("héllo €"));
        assert_eq!(decode(true, vec![0xff, 0xfe]), None, "invalid UTF-8 is refused");
        assert_eq!(decode(false, vec![b'a', 0xe9]).as_deref(), Some("aé"));
    }

    #[test]
    fn commands_round_trip() {
        for c in [Cmd::Set, Cmd::Clear, Cmd::Reload] {
            assert_eq!(Cmd::from_u32(c as u32), Some(c));
        }
        assert_eq!(Cmd::from_u32(0), None);
        assert_eq!(Cmd::from_u32(9), None);
    }
}
