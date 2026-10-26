//! Display backends. Each one runs an [`App`](crate::app::App) until the surface closes.

#[cfg(feature = "wayland")]
pub mod wayland;
#[cfg(feature = "x11")]
pub mod x11;

use crate::app::{App, Waker};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Bottom,
    Top,
}

#[derive(Debug, Clone, Copy)]
pub struct SurfaceSpec {
    pub anchor: Anchor,
    /// Gap between the screen edge and the surface, in pixels.
    pub margin: i32,
    /// Space other windows must leave free (0 = float above them).
    pub reserve: i32,
    /// Stretch across the whole output (a bar) instead of being centred (a dock).
    pub stretch: bool,
    pub namespace: &'static str,
}

/// Picks Wayland when a compositor with layer-shell is reachable, otherwise X11.
/// Set `MACSHELL_BACKEND=x11` or `wayland` to force one.
pub fn run<A: App + 'static>(app: A, spec: SurfaceSpec) -> Result<(), String> {
    let forced = std::env::var("MACSHELL_BACKEND").ok();
    #[cfg(feature = "wayland")]
    if forced.as_deref() != Some("x11") && std::env::var_os("WAYLAND_DISPLAY").is_some() && wayland::supported() {
        return wayland::run(app, spec).map_err(|e| e.to_string());
    }
    #[cfg(feature = "x11")]
    if forced.as_deref() != Some("wayland") && std::env::var_os("DISPLAY").is_some() {
        return x11::run(app, spec).map_err(|e| e.to_string());
    }
    let _ = (app, spec, forced);
    Err("no usable display: need a Wayland compositor with wlr-layer-shell, or an X11 DISPLAY".into())
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
