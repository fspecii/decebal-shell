//! Display backends. Each one runs an [`App`](crate::app::App) until the surface closes.

#[cfg(feature = "x11")]
pub(crate) mod glass;
pub mod scaled;
pub mod session;
#[cfg(feature = "wayland")]
pub mod wayland;
#[cfg(feature = "wayland")]
pub mod wayland_window;
#[cfg(feature = "x11")]
pub mod x11;
#[cfg(feature = "x11")]
pub mod x11_window;

use crate::app::{App, Waker};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Bottom,
    Top,
    /// The left screen edge: a vertical panel (caelestia's bar). `stretch` then means full output height
    /// and an app height of 0 asks for that.
    Left,
    /// The right screen edge: the same vertical panel mirrored (Caelestia's bar with `bar.position = "right"`).
    Right,
}

impl Anchor {
    /// The surface hugs a side edge and runs along the screen height.
    pub fn is_side(self) -> bool {
        matches!(self, Anchor::Left | Anchor::Right)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SurfaceSpec {
    pub anchor: Anchor,
    /// Gap between the screen edge and the surface, in pixels.
    pub margin: i32,
    /// Space other windows must leave free (0 = float above them).
    pub reserve: i32,
    /// Stretch across the whole output (a bar) instead of being centred (a dock); along the edge for
    /// [`Anchor::Left`].
    pub stretch: bool,
    pub namespace: &'static str,
}

/// UI scale factor: `DECEBAL_SCALE`, else `Xft.dpi / 96` on X11, else 1.
pub fn ui_scale() -> f32 {
    if let Some(v) = std::env::var("DECEBAL_SCALE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v > 0.0) {
        return v.clamp(0.75, 4.0);
    }
    #[cfg(feature = "x11")]
    if std::env::var_os("DISPLAY").is_some() && std::env::var("DECEBAL_BACKEND").as_deref() != Ok("wayland") {
        return x11::xft_scale_of_display();
    }
    1.0
}

/// Runs `app` on the server [`session::panel_mode`] picks: Wayland (wlr-layer-shell) in a Wayland session, X11 in an
/// X11 or tty session with a `DISPLAY` even when a `WAYLAND_DISPLAY` leaked into it. `DECEBAL_BACKEND=x11` or `wayland`
/// forces one (and is an error, not a switch, when that server is not there).
pub fn run<A: App + 'static>(app: A, spec: SurfaceSpec) -> Result<(), String> {
    match session::panel_mode() {
        #[cfg(feature = "wayland")]
        Some(session::Mode::Wayland) => wayland::run(app, spec).map_err(|e| e.to_string()),
        #[cfg(feature = "x11")]
        Some(session::Mode::X11) => x11::run(app, spec).map_err(|e| e.to_string()),
        _ => {
            let _ = (app, spec);
            Err("no usable display: need a Wayland compositor with wlr-layer-shell, or an X11 DISPLAY".into())
        }
    }
}

/// Surface pixels per logical pixel of a panel [`run_scaled`] at `scale`: `scale` on X11, 1 under a Wayland compositor
/// (which scales every surface alike). What an unscaled surface beside the panel, like the summoned Control Center,
/// multiplies the panel's logical thickness by.
pub fn surface_scale(scale: f32) -> f32 {
    if session::panel_mode() == Some(session::Mode::Wayland) {
        1.0
    } else {
        scale
    }
}

/// [`run`] for a panel drawn at `scale` (`ui_scale()`): on X11 the surface, strut, input region and drawing are
/// `scale` times the app's logical pixels (see [`scaled::Scaled`]); a Wayland compositor scales surfaces itself, so
/// there the app runs as it is.
pub fn run_scaled<A: App + 'static>(app: A, spec: SurfaceSpec, scale: f32) -> Result<(), String> {
    let scale = surface_scale(scale);
    if (scale - 1.0).abs() < 0.01 {
        return run(app, spec);
    }
    let spec = SurfaceSpec { margin: (spec.margin as f32 * scale).round() as i32, reserve: (spec.reserve as f32 * scale).ceil() as i32, ..spec };
    run(scaled::Scaled::new(app, scale), spec)
}

/// Self-pipe used to wake the event loop: the read end goes into `poll`, the [`Waker`] writes.
pub(crate) struct WakePipe {
    reader: UnixStream,
}

impl WakePipe {
    pub(crate) fn new() -> io::Result<(Self, Waker)> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        writer.set_nonblocking(true)?;
        Ok((Self { reader }, Waker::new(writer)))
    }

    pub(crate) fn fd(&self) -> RawFd {
        self.reader.as_raw_fd()
    }

    pub(crate) fn drain(&self) {
        use std::io::Read;
        let mut buf = [0u8; 64];
        while matches!((&self.reader).read(&mut buf), Ok(n) if n > 0) {}
    }
}

/// poll(2) over `fds` with an optional timeout; returns which entries are readable.
pub(crate) fn poll_readable(fds: &[RawFd], timeout: Option<Duration>) -> io::Result<Vec<bool>> {
    let mut pfds: Vec<libc::pollfd> = fds.iter().map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    let ms = timeout.map(|d| d.as_millis().min(i32::MAX as u128) as i32).unwrap_or(-1);
    loop {
        // SAFETY: `pfds` is a valid, correctly sized array of pollfd for the duration of the call.
        let r = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, ms) };
        if r >= 0 {
            return Ok(pfds.iter().map(|p| p.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0).collect());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Waits until `fd` can accept more data (the compositor socket buffer drained).
pub(crate) fn poll_writable(fd: RawFd, timeout: Duration) -> io::Result<bool> {
    let mut p = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    loop {
        // SAFETY: `p` is a valid pollfd and we pass a length of exactly one.
        let r = unsafe { libc::poll(&mut p, 1, ms) };
        if r >= 0 {
            return Ok(p.revents & libc::POLLOUT != 0);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waker_makes_pipe_readable_and_drains() {
        let (pipe, waker) = WakePipe::new().unwrap();
        assert_eq!(poll_readable(&[pipe.fd()], Some(Duration::from_millis(0))).unwrap(), vec![false]);
        waker.wake();
        assert_eq!(poll_readable(&[pipe.fd()], Some(Duration::from_millis(50))).unwrap(), vec![true]);
        pipe.drain();
        assert_eq!(poll_readable(&[pipe.fd()], Some(Duration::from_millis(0))).unwrap(), vec![false]);
    }

    #[test]
    fn many_wakes_never_block() {
        let (_pipe, waker) = WakePipe::new().unwrap();
        for _ in 0..200_000 {
            waker.wake();
        }
    }
}
