//! Exported application menus ("global menu"): the focused window's real menu, read over
//! `com.canonical.AppMenu.Registrar` + `com.canonical.dbusmenu`, with clicks sent back to the
//! application as `Event(id, "clicked", …)` on the item ids it exported.
//!
//! Nothing here invents menus. When the focused window exported none (most GTK4/GNOME apps,
//! Chrome without its global-menu path, every native Wayland client) the snapshot says so with a
//! [`State`] the bar can show or use to hide app menus; the shell's own system menu is separate.
//!
//! * One worker thread per [`AppMenus`], started only by [`AppMenus::start`]; it owns its bus
//!   connection and exits (closing every fd) on [`AppMenus::stop`] or when the handle is dropped.
//! * The UI never touches the bus: it reads [`AppMenus::snapshot`] (a lock and an `Arc` clone) and
//!   posts focus and commands that never block ([`AppMenus::set_focus`] is a latest-value slot,
//!   commands go through a bounded queue and are refused when it is full).
//! * The registrar name is never taken from an existing owner. With no owner, the worker asks for
//!   it with `DO_NOT_QUEUE | ALLOW_REPLACEMENT` (so a desktop's real registrar can still replace
//!   it) and serves it; with an owner it only observes that owner.
//! * Focus comes from the caller (the bar's existing window tracker); this module starts no
//!   window tracker of its own. Window ids are X11 window ids, which is what the registrar
//!   protocol carries. Wayland has no registrar mapping (see [`Unavailable::Wayland`]).
//!
//! See `docs/appmenu.md` for the protocol notes and the integration contract.

pub mod bus;
pub mod limits;
pub mod registrar;
pub mod tree;
pub mod view;
mod worker;

pub use limits::Limits;
pub use tree::{Disposition, ItemKind, MenuItem, MenuTree, ToggleState, ToggleType};

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const DBUSMENU: &str = "com.canonical.dbusmenu";

/// What the bar knows about the focused window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    None,
    /// An X11 client window (EWMH `_NET_ACTIVE_WINDOW`), the id applications register.
    X11 { window: u32, app_id: String },
    /// A Wayland toplevel: there is no protocol mapping it to an exported menu here.
    Wayland { app_id: String },
}

/// Which protocol the window tracker's ids come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    X11,
    Wayland,
    None,
}

impl Focus {
    /// Builds the focus input from the tracker's focused window and the protocol its ids use.
    pub fn from_window(window: Option<&crate::wm::Window>, protocol: Protocol) -> Focus {
        match (window, protocol) {
            (Some(w), Protocol::X11) => match u32::try_from(w.id) {
                Ok(id) if id != 0 => Focus::X11 { window: id, app_id: w.app_id.clone() },
                _ => Focus::None,
            },
            (Some(w), Protocol::Wayland) => Focus::Wayland { app_id: w.app_id.clone() },
            _ => Focus::None,
        }
    }
}

/// The protocol [`crate::wm::Tracker::start`] picks for this environment (same rules, same order).
/// If its Wayland collector fails to start it falls back to X11 while this still says Wayland:
/// app menus then stay honestly unavailable rather than matching ids of the wrong kind.
pub fn tracker_protocol() -> Protocol {
    let forced = std::env::var("DECEBAL_BACKEND").ok();
    if cfg!(feature = "wayland") && forced.as_deref() != Some("x11") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Protocol::Wayland;
    }
    if cfg!(feature = "x11") && forced.as_deref() != Some("wayland") && std::env::var_os("DISPLAY").is_some() {
        return Protocol::X11;
    }
    Protocol::None
}

/// Why app menus cannot be shown at all right now (as opposed to "this window has none").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// No session bus address, or connecting/authenticating failed.
    NoBus(String),
    /// The session is Wayland: no registrar protocol maps Wayland toplevels to menus here.
    Wayland,
    /// No registrar owns the name and this process may not or could not serve it.
    NoRegistrar(String),
}

impl Unavailable {
    pub fn reason(&self) -> String {
        match self {
            Unavailable::NoBus(e) => format!("Application menus are unavailable: no session bus ({e})."),
            Unavailable::Wayland => "Application menus are unavailable on Wayland: there is no supported menu export protocol for Wayland windows.".into(),
            Unavailable::NoRegistrar(e) => format!("Application menus are unavailable: no menu registrar ({e})."),
        }
    }
}

/// Identifies the focus/menu a UI rendered; commands carrying an older token are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MenuToken(pub(crate) u64);

/// The focused window's exported menu, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Exported {
    pub token: MenuToken,
    pub window: u32,
    /// The name the application registered (often its unique name).
    pub service: String,
    /// The unique name answering for it.
    pub owner: String,
    pub path: String,
    pub tree: Arc<MenuTree>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum State {
    /// The worker has not connected yet.
    #[default]
    Starting,
    Unavailable(Unavailable),
    NoFocus,
    /// The focused window registered no menu.
    NotExported {
        window: u32,
    },
    Loading {
        window: u32,
    },
    Ready(Arc<Exported>),
    /// The application's menu could not be read (error, timeout, malformed or oversized reply).
    Failed {
        window: u32,
        reason: String,
    },
    /// [`AppMenus::stop`] ran or the worker ended.
    Stopped,
}

impl State {
    pub fn exported(&self) -> Option<&Arc<Exported>> {
        match self {
            State::Ready(e) => Some(e),
            _ => None,
        }
    }

    /// A user-facing sentence for states without a menu (`None` while ready or simply unfocused).
    pub fn reason(&self) -> Option<String> {
        Some(match self {
            State::Starting => "Connecting to application menus…".into(),
            State::Unavailable(u) => u.reason(),
            State::NotExported { .. } => "This application does not export a menu.".into(),
            State::Loading { .. } => "Loading the application menu…".into(),
            State::Failed { reason, .. } => reason.clone(),
            State::Stopped => "Application menus are turned off.".into(),
            State::Ready(_) | State::NoFocus => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RegistrarRole {
    #[default]
    Unknown,
    /// Another process owns the registrar name; its table is used.
    Observing { owner: String },
    /// This worker owns the name and serves registrations.
    Owned,
    /// No registrar, and this worker will not or could not become one.
    Absent,
}

/// The application asked (`ItemActivationRequested`) for the menu showing `id` to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationRequest {
    pub token: MenuToken,
    pub id: i32,
    pub timestamp: u32,
    /// Increases with every request so repeated requests for one id are distinguishable.
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    /// Increases on every published change.
    pub generation: u64,
    pub state: State,
    pub registrar: RegistrarRole,
    /// The last non-fatal problem (a rejected click, a refetch timeout).
    pub notice: Option<String>,
    pub activation_request: Option<ActivationRequest>,
    /// Click events the application acknowledged (tests and diagnostics).
    pub events_delivered: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegistrarPolicy {
    /// Use an existing registrar; serve the name only while nobody else owns it.
    #[default]
    ObserveOrOwn,
    /// Never request the name.
    ObserveOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// D-Bus address of the session bus; `None` reports [`Unavailable::NoBus`].
    pub address: Option<String>,
    pub policy: RegistrarPolicy,
    pub limits: Limits,
}

impl Config {
    /// The session bus from `$DBUS_SESSION_BUS_ADDRESS`, else `$XDG_RUNTIME_DIR/bus` when it exists.
    pub fn session() -> Config {
        let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().filter(|a| !a.is_empty()).or_else(|| {
            let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
            let p = std::path::Path::new(&dir).join("bus");
            p.exists().then(|| format!("unix:path={}", p.display()))
        });
        Config { address, policy: RegistrarPolicy::default(), limits: Limits::default() }
    }

    pub fn with_address(address: impl Into<String>) -> Config {
        Config { address: Some(address.into()), policy: RegistrarPolicy::default(), limits: Limits::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Opened { token: MenuToken, id: i32 },
    Closed { token: MenuToken, id: i32 },
    Activate { token: MenuToken, id: i32, timestamp: u32 },
}

pub(crate) struct Shared {
    snap: Mutex<Snapshot>,
    focus: Mutex<Option<Focus>>,
    stop: AtomicBool,
}

impl Shared {
    fn lock_snap(&self) -> std::sync::MutexGuard<'_, Snapshot> {
        self.snap.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// Handle to one app-menu worker. Dropping it stops the worker without waiting for it.
pub struct AppMenus {
    shared: Arc<Shared>,
    tx: SyncSender<Command>,
    wake: UnixStream,
    thread: Option<JoinHandle<()>>,
    last_focus: Focus,
}

impl AppMenus {
    /// Starts the worker thread. `notify` runs on the worker after each published change (it
    /// should only wake the UI loop). Connecting happens on the worker, never on the caller.
    pub fn start(config: Config, notify: Notify) -> io::Result<AppMenus> {
        let (wake, wake_rx) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        wake_rx.set_nonblocking(true)?;
        let (tx, rx) = mpsc::sync_channel(config.limits.command_queue.max(1));
        let shared = Arc::new(Shared { snap: Mutex::new(Snapshot::default()), focus: Mutex::new(None), stop: AtomicBool::new(false) });
        let s = shared.clone();
        let thread = std::thread::Builder::new().name("appmenu".into()).spawn(move || worker::run(config, s, rx, wake_rx, notify))?;
        Ok(AppMenus { shared, tx, wake, thread: Some(thread), last_focus: Focus::None })
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock_snap().clone()
    }

    /// Replaces the focus input (a one-slot channel: only the latest focus matters). Never blocks.
    pub fn set_focus(&mut self, focus: Focus) {
        if focus == self.last_focus {
            return;
        }
        self.last_focus = focus.clone();
        *self.shared.focus.lock().unwrap_or_else(|e| e.into_inner()) = Some(focus);
        self.poke();
    }

    /// The user opened the submenu `id` (`0` for the whole bar): `AboutToShow` + `opened`.
    pub fn opened(&self, token: MenuToken, id: i32) -> bool {
        self.command(Command::Opened { token, id })
    }

    pub fn closed(&self, token: MenuToken, id: i32) -> bool {
        self.command(Command::Closed { token, id })
    }

    /// Sends `Event(id, "clicked")` if `id` is still a reachable, enabled, plain item of the menu
    /// `token` names. `false` only when the queue is full (the click is dropped, not delayed).
    pub fn activate(&self, token: MenuToken, id: i32, timestamp: u32) -> bool {
        self.command(Command::Activate { token, id, timestamp })
    }

    fn command(&self, c: Command) -> bool {
        match self.tx.try_send(c) {
            Ok(()) => {
                self.poke();
                true
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn poke(&self) {
        // A full socket buffer already guarantees a pending wake-up.
        let _ = (&self.wake).write(&[1]);
    }

    /// Stops the worker and waits up to `timeout` for it to exit; true when it has.
    pub fn stop(&mut self, timeout: Duration) -> bool {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.poke();
        let Some(t) = self.thread.take() else { return true };
        let end = Instant::now() + timeout;
        while !t.is_finished() {
            if Instant::now() >= end {
                self.thread = Some(t);
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = t.join();
        true
    }
}

impl Drop for AppMenus {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.poke();
    }
}

#[cfg(test)]
mod testbus;
#[cfg(test)]
mod tests;
