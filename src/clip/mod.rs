//! `macclip`: clipboard history for the X11 session.
//!
//! * [`history`]: the stored entries (newest first, deduplicated, capped), their file and the list helpers the picker uses.
//! * [`policy`]: what is never stored (blank or huge text, password managers).
//! * `daemon` (X11): watches the CLIPBOARD selection with XFixes, records text, keeps the clipboard alive.
//! * [`picker`]: the Spotlight-style popup.
//! * `paste` (X11): puts the focus back and sends Ctrl+V with XTest.

pub mod history;
pub mod picker;
pub mod policy;

#[cfg(feature = "x11")]
pub mod daemon;
#[cfg(feature = "x11")]
pub mod paste;
