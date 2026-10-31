//! Event-driven X11 shell-frame colour publication.
//!
//! A single worker owns at most one running and one latest pending request. X11
//! connection/property I/O never runs in a paint/input callback; short-lived
//! theme commands wait briefly for acknowledgement before exiting. An unavailable
//! server is best-effort and cannot block UI or grow a thread/request backlog.
use super::{live::Source, Rgb, Theme};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;

pub const ATOM: &str = "_DECEBAL_DESKTOP_FRAME_THEME";
const ACK_BUDGET: Duration = Duration::from_millis(300);
type Reply = mpsc::SyncSender<bool>;
struct Job {
    colour: Rgb,
    reply: Option<Reply>,
}
struct Publisher {
    latest: Arc<Mutex<Option<Job>>>,
    wake: mpsc::SyncSender<()>,
}
impl Publisher {
    fn start(sink: impl Fn(Rgb) -> bool + Send + 'static) -> std::io::Result<Self> {
        let latest = Arc::new(Mutex::new(None::<Job>));
        let work = latest.clone();
        let (wake, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("frame-theme".into()).spawn(move || {
            while rx.recv().is_ok() {
                loop {
                    let job = work.lock().unwrap().take();
                    let Some(job) = job else { break };
                    let success = sink(job.colour);
                    if let Some(reply) = job.reply {
                        let _ = reply.try_send(success);
                    }
                }
            }
        })?;
        Ok(Self { latest, wake })
    }
    fn submit(&self, colour: Rgb, wait: bool) -> bool {
        let (tx, rx) = mpsc::sync_channel(1);
        // Replacing the slot drops a superseded acknowledgement immediately.
        *self.latest.lock().unwrap() = Some(Job { colour, reply: wait.then_some(tx) });
        if self.wake.try_send(()).is_err_and(|e| matches!(e, mpsc::TrySendError::Disconnected(_))) {
            return false;
        }
        !wait || rx.recv_timeout(ACK_BUDGET).unwrap_or(false)
    }
}

fn actual_x11() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty()) && !std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

fn submit(colour: Rgb, wait: bool) -> bool {
    if cfg!(test) || !cfg!(feature = "x11") || !actual_x11() {
        return false;
    }
    static PUBLISHER: OnceLock<Option<Publisher>> = OnceLock::new();
    PUBLISHER.get_or_init(|| Publisher::start(write_x11).ok()).as_ref().is_some_and(|p| p.submit(colour, wait))
}

/// Bar startup/generation hook. The outer shell strip uses `background`, while
/// `surface` is a raised card and is intentionally not the frame default.
pub fn publish(theme: &Theme) {
    let _ = submit(theme.background, false);
}

/// Real process startup, after offscreen branches. Wait briefly because an
/// inactive bottom-role process can exit without entering its event loop.
pub fn publish_startup(theme: &Theme) {
    let _ = submit(theme.background, true);
}

/// Successful real apply/undo hook, also covering a session without a bar.
/// A private/preview Source must never publish into an unrelated live display.
pub fn publish_committed(src: &Source, dark_hint: bool) {
    if cfg!(test) || !cfg!(feature = "x11") || !actual_x11() {
        return;
    }
    let default = Source::discover();
    if src.shell_json != default.shell_json || src.themes_dir != default.themes_dir {
        return;
    }
    let (theme, problem) = src.build(&src.selection(), dark_hint);
    if problem.is_some() {
        return;
    }
    let theme = crate::accessibility::Preferences::from_env().apply(theme);
    let _ = submit(theme.background, true);
}

#[cfg(any(feature = "x11", test))]
fn needs_update(format: u8, cardinal: bool, remaining: u32, current: Option<Vec<u32>>, desired: [u32; 2]) -> bool {
    format != 32 || !cardinal || remaining != 0 || current.is_none_or(|words| words != desired)
}

#[cfg(feature = "x11")]
fn write_x11(colour: Rgb) -> bool {
    use x11rb::{
        connection::Connection,
        protocol::xproto::{AtomEnum, ConnectionExt, PropMode},
        wrapper::ConnectionExt as _,
    };
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let (conn, screen) = crate::x11_connection::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let atom = conn.intern_atom(false, ATOM.as_bytes())?.reply()?.atom;
        let words = [1, (u32::from(colour.0) << 16) | (u32::from(colour.1) << 8) | u32::from(colour.2)];
        let current = conn.get_property(false, root, atom, AtomEnum::CARDINAL, 0, 3)?.reply()?;
        if needs_update(current.format, current.type_ == u32::from(AtomEnum::CARDINAL), current.bytes_after, current.value32().map(Iterator::collect), words) {
            conn.change_property32(PropMode::REPLACE, root, atom, AtomEnum::CARDINAL, &words)?.check()?;
            conn.flush()?;
        }
        Ok(())
    })();
    result.is_ok()
}
#[cfg(not(feature = "x11"))]
fn write_x11(_: Rgb) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_requests_coalesce_and_short_lived_callers_get_acknowledged() {
        let (entered, rx) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let (completed, done) = mpsc::channel();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copies = seen.clone();
        let publisher = Publisher::start(move |rgb| {
            copies.lock().unwrap().push(rgb);
            if rgb == Rgb(1, 0, 0) {
                entered.send(()).unwrap();
                gate.recv().unwrap();
            }
            completed.send(rgb).unwrap();
            true
        })
        .unwrap();
        assert!(publisher.submit(Rgb(1, 0, 0), false));
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(publisher.submit(Rgb(2, 0, 0), false));
        assert!(publisher.submit(Rgb(3, 0, 0), false));
        assert_eq!(publisher.latest.lock().unwrap().as_ref().unwrap().colour, Rgb(3, 0, 0));
        release.send(()).unwrap();
        assert_eq!(done.recv_timeout(Duration::from_secs(1)).unwrap(), Rgb(1, 0, 0));
        assert_eq!(done.recv_timeout(Duration::from_secs(1)).unwrap(), Rgb(3, 0, 0));
        assert!(publisher.submit(Rgb(4, 0, 0), true));
        assert_eq!(*seen.lock().unwrap(), vec![Rgb(1, 0, 0), Rgb(3, 0, 0), Rgb(4, 0, 0)]);
    }
    #[test]
    fn blocked_sink_has_one_pending_slot_and_wait_is_bounded() {
        let (release, gate) = mpsc::sync_channel(1);
        let (entered, rx) = mpsc::sync_channel(1);
        let publisher = Publisher::start(move |_| {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            false
        })
        .unwrap();
        publisher.submit(Rgb(1, 2, 3), false);
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let start = std::time::Instant::now();
        assert!(!publisher.submit(Rgb(4, 5, 6), true));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(publisher.latest.lock().unwrap().is_some());
        // Drop the pending slot so releasing the running request lets the worker
        // exit rather than waiting for a second synthetic gate.
        publisher.latest.lock().unwrap().take();
        release.send(()).unwrap();
    }
    #[test]
    fn current_property_is_a_noop_only_for_exact_protocol_and_colour() {
        let desired = [1, 0x123456];
        assert!(!needs_update(32, true, 0, Some(desired.to_vec()), desired));
        assert!(needs_update(8, true, 0, Some(desired.to_vec()), desired));
        assert!(needs_update(32, false, 0, Some(desired.to_vec()), desired));
        assert!(needs_update(32, true, 4, Some(desired.to_vec()), desired));
        assert!(needs_update(32, true, 0, Some(vec![2, 0x123456]), desired));
        assert!(needs_update(32, true, 0, Some(vec![1, 0x654321]), desired));
        assert!(needs_update(32, true, 0, None, desired));
    }
    #[test]
    fn unsupported_publication_is_disabled_in_unit_tests() {
        assert!(!submit(Rgb(10, 20, 30), true));
    }
}
