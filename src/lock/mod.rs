//! `declock`: the themed X11 screen locker.
//!
//! * [`secret`]: the password buffer (fixed size, `mlock`ed, wiped).
//! * [`auth`]: PAM through direct FFI, and the [`auth::Authenticator`] seam (a fake one exists for tests only).
//! * [`state`]: the pure state machine (typing, checking, wrong password, back-off), unit-tested.
//! * [`bg`]: the blurred, dimmed backdrop (screenshot or wallpaper), computed at 1/8 size.
//! * [`view`]: drawing the centre panel with the shared theme.
//! * [`x11`]: the grabbing, fullscreen override-redirect window and the event loop.

pub mod auth;
pub mod bg;
pub mod secret;
pub mod state;
pub mod view;
#[cfg(feature = "x11")]
pub mod x11;

use std::ffi::CStr;
use std::path::PathBuf;

/// The account that started the lock (the only one that can unlock it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInfo {
    pub login: String,
    /// Full name from the passwd GECOS field, else the login name.
    pub display: String,
    /// `$HOME` when set (like the rest of Decebal), else the passwd home directory.
    pub home: PathBuf,
}

/// The passwd entry of the real uid.
pub fn current_user() -> Option<UserInfo> {
    // SAFETY: getpwuid_r with a caller-owned buffer; `pwd` is only read after a success with a non-null result,
    // and the strings it points to live in `buf`, which outlives the reads below.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0u8; 4096];
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        loop {
            let rc = libc::getpwuid_r(libc::getuid(), &mut pwd, buf.as_mut_ptr().cast(), buf.len(), &mut result);
            if rc == libc::ERANGE && buf.len() < (1 << 20) {
                buf.resize(buf.len() * 2, 0);
                continue;
            }
            if rc != 0 || result.is_null() {
                return None;
            }
            break;
        }
        let s = |p: *const libc::c_char| if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() };
        let login = s(pwd.pw_name);
        let gecos = s(pwd.pw_gecos);
        let full = gecos.split(',').next().unwrap_or("").trim().to_string();
        Some(UserInfo {
            display: if full.is_empty() { login.clone() } else { full },
            login,
            home: std::env::var_os("HOME").filter(|h| !h.is_empty()).map_or_else(|| PathBuf::from(s(pwd.pw_dir)), PathBuf::from),
        })
    }
}

/// What the backdrop is made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgMode {
    /// A blurred, dimmed copy of what is on screen now (falls back to the wallpaper, then a flat colour).
    Screenshot,
    /// The wallpaper (rdwm's, else Omarchy's current background), blurred and dimmed.
    Wallpaper,
    /// The theme's background colour.
    Color,
}

impl BgMode {
    pub fn parse(s: &str) -> Option<BgMode> {
        match s.to_ascii_lowercase().as_str() {
            "screenshot" | "screen" => Some(BgMode::Screenshot),
            "wallpaper" => Some(BgMode::Wallpaper),
            "color" | "colour" | "solid" => Some(BgMode::Color),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bg: BgMode,
    /// Blur radius in backdrop pixels (one is 8 screen pixels).
    pub blur: usize,
    /// 0..1: how far the backdrop is darkened.
    pub dim: f32,
    pub hour12: bool,
    pub user: UserInfo,
}

impl Config {
    pub const DEFAULT_BLUR: usize = 3;
    pub const DEFAULT_DIM: f32 = 0.45;
}

/// `clockHour12` of the shared settings (`decebal.json`), so the lock follows the bar's clock.
pub fn hour12_setting() -> bool {
    std::fs::read_to_string(crate::config::config_dir().join("decebal.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["clockHour12"].as_bool())
        .unwrap_or(false)
}

/// The user's picture, if they have one (`~/.face`, as GNOME/KDE/SDDM greeters use).
pub fn face_path(home: &std::path::Path) -> Option<PathBuf> {
    [".face", ".face.icon"].iter().map(|f| home.join(f)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bg_modes_parse() {
        assert_eq!(BgMode::parse("Screenshot"), Some(BgMode::Screenshot));
        assert_eq!(BgMode::parse("wallpaper"), Some(BgMode::Wallpaper));
        assert_eq!(BgMode::parse("colour"), Some(BgMode::Color));
        assert_eq!(BgMode::parse("nope"), None);
    }

    #[test]
    fn the_current_user_is_found() {
        let u = current_user().expect("passwd entry");
        assert!(!u.login.is_empty() && !u.display.is_empty());
        assert!(u.home.is_absolute());
    }
}
