//! Caps Lock / Num Lock for the vertical bar's lock indicator (caelestia shows a key while it is locked on).
//!
//! X11 only: one thread with its own connection selects XKB state notifications and sleeps in `wait_for_event`
//! between them, so it costs nothing while no lock key is pressed. Without the extension (or on Wayland) the
//! indicator simply never appears.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

pub const CAPS: u8 = 1;
pub const NUM: u8 = 2;

/// X11 modifier masks: Lock is bit 1, Num Lock sits on Mod2 on virtually every layout.
const MOD_LOCK: u16 = 1 << 1;
const MOD_NUM: u16 = 1 << 4;

/// The lock bits ([`CAPS`], [`NUM`]) for a locked-modifier mask.
pub fn from_mods(locked: u16) -> u8 {
    (if locked & MOD_LOCK != 0 { CAPS } else { 0 }) | (if locked & MOD_NUM != 0 { NUM } else { 0 })
}

/// Shared lock state: what the watcher thread stores and the bar reads while drawing.
#[derive(Clone, Default)]
pub struct Locks(Arc<AtomicU8>);

impl Locks {
    pub fn caps(&self) -> bool {
        self.0.load(Ordering::Relaxed) & CAPS != 0
    }
    pub fn num(&self) -> bool {
        self.0.load(Ordering::Relaxed) & NUM != 0
    }
    pub fn set(&self, bits: u8) -> bool {
        self.0.swap(bits, Ordering::Relaxed) != bits
    }
}

/// A running lock-key watcher; dropping it ends the thread and closes its X connection (the lock-keys applet was
/// turned off).
pub struct Watch {
    #[cfg(feature = "x11")]
    stop: Option<(Arc<std::sync::atomic::AtomicBool>, Arc<x11rb::rust_connection::RustConnection>, u32)>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        #[cfg(feature = "x11")]
        if let Some((stop, conn, win)) = self.stop.take() {
            use x11rb::connection::Connection;
            use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt as _, EventMask};
            stop.store(true, Ordering::SeqCst);
            // wake the thread blocked in wait_for_event: a message to its own window
            let ev = ClientMessageEvent::new(32, win, x11rb::NONE, [0u32; 5]);
            let _ = conn.send_event(false, win, EventMask::NO_EVENT, ev);
            let _ = conn.flush();
        }
    }
}

/// Starts the watcher; `wake` runs after every change. `None` when there is no X server to ask.
#[cfg(feature = "x11")]
pub fn spawn_watcher(locks: Locks, wake: impl Fn() + Send + 'static) -> Option<Watch> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xkb::{self, ConnectionExt as _};
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::protocol::Event;
    if std::env::var_os("DISPLAY").is_none() || std::env::var("DECEBAL_BACKEND").as_deref() == Ok("wayland") {
        return None;
    }
    let (conn, screen) = crate::x11_connection::connect(None).ok()?;
    let core_kbd: xkb::DeviceSpec = xkb::ID::USE_CORE_KBD.into();
    let ok = conn.xkb_use_extension(1, 0).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.supported);
    if !ok {
        return None;
    }
    let none = xkb::MapPart::from(0u16);
    let selected = conn.xkb_select_events(core_kbd, xkb::EventType::from(0u16), xkb::EventType::STATE_NOTIFY, none, none, &xkb::SelectEventsAux::new()).map(|c| c.ignore_error());
    // an unmapped input-only window of our own, the address of the stop message
    let win = conn.generate_id().ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let created = conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).map(|c| c.ignore_error());
    if selected.is_err() || created.is_err() || conn.flush().is_err() {
        return None;
    }
    let read = move |conn: &x11rb::rust_connection::RustConnection| conn.xkb_get_state(core_kbd).ok().and_then(|c| c.reply().ok()).map(|r| from_mods(u16::from(r.locked_mods)));
    if let Some(bits) = read(&conn) {
        locks.set(bits);
    }
    let conn = Arc::new(conn);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (thread_conn, thread_stop) = (Arc::clone(&conn), Arc::clone(&stop));
    std::thread::Builder::new()
        .name("lock-keys".into())
        .spawn(move || loop {
            if thread_stop.load(Ordering::SeqCst) {
                return;
            }
            match thread_conn.wait_for_event() {
                Ok(Event::XkbStateNotify(e)) => {
                    if locks.set(from_mods(u16::from(e.locked_mods))) {
                        wake();
                    }
                }
                Ok(_) => {}
                Err(_) => return,
            }
        })
        .ok()?;
    Some(Watch { stop: Some((stop, conn, win)) })
}

#[cfg(not(feature = "x11"))]
pub fn spawn_watcher(_locks: Locks, _wake: impl Fn() + Send + 'static) -> Option<Watch> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_modifier_masks_map_to_caps_and_num() {
        assert_eq!(from_mods(0), 0);
        assert_eq!(from_mods(MOD_LOCK), CAPS);
        assert_eq!(from_mods(MOD_NUM), NUM);
        assert_eq!(from_mods(MOD_LOCK | MOD_NUM | 1), CAPS | NUM, "shift (bit 0) is not a lock");
        assert_eq!(from_mods(1 << 2), 0, "control is not a lock");
    }

    #[test]
    fn shared_state_reports_changes_once() {
        let l = Locks::default();
        assert!(!l.caps() && !l.num());
        assert!(l.set(CAPS));
        assert!(!l.set(CAPS), "unchanged");
        assert!(l.caps() && !l.num());
        assert!(l.set(CAPS | NUM) && l.num());
    }
}
