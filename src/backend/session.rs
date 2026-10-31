//! Which display server a surface goes on: the one policy every Decebal program follows (panels through
//! [`super::run`], the desktop widgets host, popup windows).
//!
//! The session decides, not whichever socket happens to be in the environment. A native Wayland session also has an
//! XWayland `DISPLAY`; an X11 session (or a program started from a tty) can inherit a `WAYLAND_DISPLAY` that leaked from
//! a nested compositor. So:
//!
//! * `DECEBAL_BACKEND=x11|wayland` wins, but never selects a server that is not there;
//! * `XDG_SESSION_TYPE=wayland` prefers Wayland (XWayland stays the fallback when the compositor lacks the protocol);
//! * any other session (`x11`, `tty`, unset, unknown) with a `DISPLAY` stays on X11, even when a `WAYLAND_DISPLAY` is set;
//! * without a `DISPLAY`, a reachable Wayland compositor is used.
//!
//! The protocol probe (a registry round trip) is the only costly part; [`detect`] runs it at most once and only when the
//! answer can depend on it, never when X11 is forced or already decided.

/// The display server a surface is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    X11,
    Wayland,
}

/// `DECEBAL_BACKEND`, normalised: `Some(mode)` only for a recognised value (`x11`, `wayland`, any case, trimmed).
fn forced_mode(forced: Option<&str>) -> Option<Mode> {
    let v = forced?.trim();
    if v.eq_ignore_ascii_case("x11") {
        Some(Mode::X11)
    } else if v.eq_ignore_ascii_case("wayland") {
        Some(Mode::Wayland)
    } else {
        None
    }
}

fn wayland_session(session: Option<&str>) -> bool {
    session.is_some_and(|s| s.trim().eq_ignore_ascii_case("wayland"))
}

/// Whether Wayland should be tried first. `forced` is `DECEBAL_BACKEND`, `session` is `XDG_SESSION_TYPE`, `display` and
/// `wayland` say `DISPLAY` / `WAYLAND_DISPLAY` are set (and supported by this build). No protocol probe is involved.
pub fn prefer_wayland(forced: Option<&str>, session: Option<&str>, display: bool, wayland: bool) -> bool {
    match forced_mode(forced) {
        Some(Mode::X11) => false,
        Some(Mode::Wayland) => true,
        None if !wayland => false,
        // A Wayland session: its XWayland DISPLAY does not make it an X11 session.
        None if wayland_session(session) => true,
        // x11, tty, unset or unknown: a DISPLAY is the session's own server; a WAYLAND_DISPLAY beside it may have leaked
        // from a nested compositor. Without a DISPLAY there is nothing else to use.
        None => !display,
    }
}

/// The server to use, or `None` when none is usable. `wayland_supported` says the compositor offers what the caller
/// needs (wlr-layer-shell for panels and widgets); it is ignored whenever the policy does not reach Wayland.
pub fn choose_mode(forced: Option<&str>, session: Option<&str>, display: bool, wayland: bool, wayland_supported: bool) -> Option<Mode> {
    decide(forced, session, display, wayland, || wayland_supported)
}

/// [`choose_mode`] with a lazy probe: `probe` runs at most once, and only when the answer depends on it.
pub fn decide(forced: Option<&str>, session: Option<&str>, display: bool, wayland: bool, probe: impl FnOnce() -> bool) -> Option<Mode> {
    match forced_mode(forced) {
        // An explicit choice is never silently switched to the other server.
        Some(Mode::X11) => display.then_some(Mode::X11),
        Some(Mode::Wayland) => (wayland && probe()).then_some(Mode::Wayland),
        None if prefer_wayland(forced, session, display, wayland) => {
            if probe() {
                Some(Mode::Wayland)
            } else {
                // a compositor without the protocol: XWayland, when there is one
                display.then_some(Mode::X11)
            }
        }
        None if display => Some(Mode::X11),
        None => None,
    }
}

/// The session as this process sees it: the environment variables of the policy, with `DISPLAY` / `WAYLAND_DISPLAY`
/// counted only when this binary was built with that backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub forced: Option<String>,
    pub session: Option<String>,
    pub display: bool,
    pub wayland: bool,
}

impl Env {
    pub fn current() -> Env {
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        Env {
            forced: std::env::var("DECEBAL_BACKEND").ok(),
            session: std::env::var("XDG_SESSION_TYPE").ok(),
            display: cfg!(feature = "x11") && set("DISPLAY"),
            wayland: cfg!(feature = "wayland") && set("WAYLAND_DISPLAY"),
        }
    }

    pub fn prefer_wayland(&self) -> bool {
        prefer_wayland(self.forced.as_deref(), self.session.as_deref(), self.display, self.wayland)
    }

    pub fn decide(&self, probe: impl FnOnce() -> bool) -> Option<Mode> {
        decide(self.forced.as_deref(), self.session.as_deref(), self.display, self.wayland, probe)
    }
}

/// The server for this session, probing the compositor with `probe` only when the policy needs it.
pub fn detect(probe: impl FnOnce() -> bool) -> Option<Mode> {
    Env::current().decide(probe)
}

/// The server [`super::run`] puts a layer-shell panel on, decided once per process (one registry round trip at most).
pub fn panel_mode() -> Option<Mode> {
    static MODE: std::sync::OnceLock<Option<Mode>> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| detect(layer_shell_supported))
}

/// The compositor offers wlr-layer-shell and wl_shm (one registry round trip).
#[cfg(feature = "wayland")]
pub fn layer_shell_supported() -> bool {
    super::wayland::supported()
}

/// Built without Wayland: never.
#[cfg(not(feature = "wayland"))]
pub fn layer_shell_supported() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use Mode::{Wayland, X11};

    #[test]
    fn a_wayland_session_prefers_wayland_over_its_xwayland_display() {
        assert!(prefer_wayland(None, Some("wayland"), true, true));
        assert_eq!(choose_mode(None, Some("wayland"), true, true, true), Some(Wayland));
        assert_eq!(choose_mode(None, Some("Wayland "), true, true, true), Some(Wayland), "the session value is trimmed and case-blind");
        assert_eq!(choose_mode(None, Some("wayland"), false, true, true), Some(Wayland));
    }

    #[test]
    fn x11_tty_and_unset_sessions_with_a_display_stay_on_x11_despite_a_leaked_wayland_display() {
        for session in [Some("x11"), Some("tty"), None, Some("unspecified"), Some("")] {
            assert!(!prefer_wayland(None, session, true, true), "{session:?}");
            assert_eq!(choose_mode(None, session, true, true, true), Some(X11), "{session:?}");
        }
    }

    #[test]
    fn without_a_display_any_session_can_use_a_reachable_compositor() {
        for session in [Some("x11"), Some("tty"), None] {
            assert!(prefer_wayland(None, session, false, true));
            assert_eq!(choose_mode(None, session, false, true, true), Some(Wayland), "{session:?}");
            assert_eq!(choose_mode(None, session, false, true, false), None, "no layer shell and no DISPLAY: nothing usable");
        }
        assert_eq!(choose_mode(None, None, false, false, false), None);
    }

    #[test]
    fn a_wayland_session_without_the_protocol_falls_back_to_xwayland() {
        assert_eq!(choose_mode(None, Some("wayland"), true, true, false), Some(X11));
        assert_eq!(choose_mode(None, Some("wayland"), false, true, false), None);
        assert_eq!(choose_mode(None, Some("wayland"), true, false, true), Some(X11), "no WAYLAND_DISPLAY at all");
    }

    #[test]
    fn an_explicit_backend_wins_but_never_picks_a_missing_server() {
        assert_eq!(choose_mode(Some("x11"), Some("wayland"), true, true, true), Some(X11));
        assert_eq!(choose_mode(Some(" X11"), Some("wayland"), true, true, true), Some(X11));
        assert_eq!(choose_mode(Some("wayland"), Some("x11"), true, true, true), Some(Wayland));
        assert_eq!(choose_mode(Some("x11"), None, false, true, true), None, "forcing X11 without a DISPLAY is an error, not a silent switch");
        assert_eq!(choose_mode(Some("wayland"), None, true, false, true), None);
        assert_eq!(choose_mode(Some("wayland"), None, true, true, false), None, "forced Wayland without the protocol does not fall back");
        assert!(prefer_wayland(Some("wayland"), Some("x11"), true, true) && !prefer_wayland(Some("x11"), Some("wayland"), true, true));
        // unknown values are ignored, not treated as a choice
        assert_eq!(choose_mode(Some("auto"), Some("x11"), true, true, true), Some(X11));
    }

    #[test]
    fn the_protocol_probe_runs_only_when_the_answer_depends_on_it() {
        let never = || -> bool { panic!("probed although the answer was already decided") };
        assert_eq!(decide(Some("x11"), Some("wayland"), true, true, never), Some(X11));
        assert_eq!(decide(None, Some("x11"), true, true, never), Some(X11));
        assert_eq!(decide(None, None, true, true, never), Some(X11));
        assert_eq!(decide(None, Some("wayland"), true, false, never), Some(X11));
        assert_eq!(decide(Some("wayland"), None, true, false, never), None);
        let mut calls = 0;
        assert_eq!(
            decide(None, Some("wayland"), true, true, || {
                calls += 1;
                true
            }),
            Some(Wayland)
        );
        assert_eq!(calls, 1);
    }
}
