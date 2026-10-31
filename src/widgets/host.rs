//! The `decwidgets` daemon: one small surface per desktop widget, kept below every application window.
//!
//! It runs natively on both display servers and picks one from the session (see [`choose_mode`]): a wlr-layer-shell surface per
//! card on Wayland ([`super::host_wayland`]), an override-redirect window per card on X11 ([`super::host_x11`]). The layout,
//! painting, live data and click actions are shared; only the surfaces and their input differ.

use super::config::{self, Instance, WidgetsConfig};
use super::data::Data;
use super::dsl::WidgetDef;
use super::layout::{self, Ctx, Item};
use super::paint::FontMetrics;
use crate::text::Fonts;
use std::path::PathBuf;
use std::time::Duration;

pub struct Paths {
    pub shell_json: PathBuf,
    pub themes_dir: PathBuf,
}

/// Which display server the cards are drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Wayland,
    X11,
}

/// Picks the display server. `forced` is `DECEBAL_BACKEND`, `session` is `XDG_SESSION_TYPE`; `layer_shell` says the Wayland
/// compositor offers wlr-layer-shell (without it cards cannot be placed). A native Wayland session also has an XWayland
/// `DISPLAY`, so Wayland wins there unless the session says it is X11; an X11 session that merely inherited a stale
/// `WAYLAND_DISPLAY` stays on X11.
pub fn choose_mode(forced: Option<&str>, session: Option<&str>, display: bool, wayland: bool, layer_shell: bool) -> Option<Mode> {
    let wl = wayland && layer_shell;
    match forced {
        Some("x11") => display.then_some(Mode::X11),
        Some("wayland") => wl.then_some(Mode::Wayland),
        _ if wl && session != Some("x11") => Some(Mode::Wayland),
        _ if display => Some(Mode::X11),
        _ if wl => Some(Mode::Wayland),
        _ => None,
    }
}

/// The display server of the current session, honouring the features this binary was built with.
pub fn mode() -> Option<Mode> {
    let forced = std::env::var("DECEBAL_BACKEND").ok();
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let (display, wayland) = (cfg!(feature = "x11") && set("DISPLAY"), cfg!(feature = "wayland") && set("WAYLAND_DISPLAY"));
    #[cfg(feature = "wayland")]
    let layer_shell = wayland && crate::backend::wayland::supported();
    #[cfg(not(feature = "wayland"))]
    let layer_shell = false;
    choose_mode(forced.as_deref(), session.as_deref(), display, wayland, layer_shell)
}

/// Where the Wayland host says it is up: a file holding its pid (X11 uses a selection owner instead).
pub(super) fn ready_file() -> PathBuf {
    crate::single::runtime_file("decwidgets.ready")
}

fn pid_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

/// True once the host of this session has finished its first render.
pub fn is_running() -> bool {
    match mode() {
        #[cfg(feature = "x11")]
        Some(Mode::X11) => super::host_x11::ready(),
        Some(Mode::Wayland) => std::fs::read_to_string(ready_file()).ok().and_then(|s| s.trim().parse::<i32>().ok()).is_some_and(pid_alive),
        _ => false,
    }
}

/// Start the single shared host when a user enables desktop widgets.
pub fn ensure_running() -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    if mode().is_none() {
        return Err("No desktop display is available for widgets (a Wayland compositor with layer-shell or an X11 session is needed)".into());
    }
    let running = crate::single::signal("decwidgets", 0);
    if !running && !crate::launch::spawn_argv(&[crate::launch::sibling_or_path("decwidgets"), "daemon".into()]) {
        return Err("Could not start desktop widgets".into());
    }
    for _ in 0..100 {
        if is_running() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("Desktop widgets did not start; see the decwidgets log".into())
}

/// Runs the host for this session until it is stopped.
pub fn run(paths: Paths) -> Result<(), String> {
    match mode() {
        #[cfg(feature = "wayland")]
        Some(Mode::Wayland) => super::host_wayland::run(paths),
        #[cfg(feature = "x11")]
        Some(Mode::X11) => super::host_x11::run(paths),
        _ => Err("No desktop display is available for widgets (a Wayland compositor with layer-shell or an X11 session is needed)".into()),
    }
}

// ---------------------------------------------------------------- shared by both backends

/// Size of the card for the current data and fonts.
pub(super) fn lay(def: &WidgetDef, data: &Data, fonts: &Fonts) -> Item {
    let th = crate::theme::th();
    layout::layout(def, &Ctx { data, metrics: &FontMetrics(fonts), theme_radius: th.radius }, None)
}

pub(super) fn data_with_options(base: &Data, inst: &Instance) -> Data {
    let mut d = base.clone();
    for (k, v) in &inst.options {
        let val = match v {
            serde_json::Value::String(s) => super::data::Val::Text(s.clone()),
            serde_json::Value::Number(n) => super::data::Val::Num(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::Bool(b) => super::data::Val::Bool(*b),
            _ => continue,
        };
        d.set(&format!("opt.{k}"), val);
    }
    d
}

/// Mtimes of the widget definition files in use, so editing a widget redraws it without a restart.
pub(super) fn theme_stamp(paths: &Paths) -> u64 {
    let cfg = WidgetsConfig::load(&paths.shell_json);
    let mut sum = 0u64;
    for i in &cfg.items {
        let file = match &i.source {
            Some(s) => config::package_dir(&paths.themes_dir, s).map(|d| d.join("widgets").join(format!("{}.toml", i.use_))),
            None => Some(config::user_dir().join(format!("{}.toml", i.use_))),
        };
        if let Some(m) = file.and_then(|f| std::fs::metadata(f).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
            sum = sum.wrapping_add(m.as_nanos() as u64);
        }
    }
    sum
}

pub(super) fn dots_under(item: &Item, x: f32, y: f32) -> Option<u32> {
    if !item.rect.contains(x, y) {
        return None;
    }
    item.children.iter().find_map(|c| dots_under(c, x, y)).or_else(|| layout::dot_at(item, x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_native_wayland_session_uses_wayland_even_though_xwayland_sets_display() {
        assert_eq!(choose_mode(None, Some("wayland"), true, true, true), Some(Mode::Wayland));
        assert_eq!(choose_mode(None, None, true, true, true), Some(Mode::Wayland));
        assert_eq!(choose_mode(None, None, false, true, true), Some(Mode::Wayland));
    }

    #[test]
    fn an_x11_session_stays_on_x11_even_with_a_stale_wayland_display() {
        assert_eq!(choose_mode(None, Some("x11"), true, true, true), Some(Mode::X11));
        assert_eq!(choose_mode(None, None, true, false, false), Some(Mode::X11));
    }

    #[test]
    fn without_layer_shell_wayland_falls_back_to_xwayland_and_then_to_nothing() {
        assert_eq!(choose_mode(None, Some("wayland"), true, true, false), Some(Mode::X11));
        assert_eq!(choose_mode(None, Some("wayland"), false, true, false), None);
        assert_eq!(choose_mode(None, None, false, false, false), None);
    }

    #[test]
    fn the_backend_variable_forces_a_server_but_never_an_unavailable_one() {
        assert_eq!(choose_mode(Some("x11"), Some("wayland"), true, true, true), Some(Mode::X11));
        assert_eq!(choose_mode(Some("wayland"), Some("x11"), true, true, true), Some(Mode::Wayland));
        assert_eq!(choose_mode(Some("x11"), None, false, true, true), None, "forcing X11 without a DISPLAY is an error, not a silent switch");
        assert_eq!(choose_mode(Some("wayland"), None, true, false, false), None);
    }

    #[test]
    fn an_exited_host_is_not_alive() {
        assert!(pid_alive(std::process::id() as i32));
        assert!(!pid_alive(0) && !pid_alive(-1));
    }
}
