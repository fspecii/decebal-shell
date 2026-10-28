//! Themed X11 screen locker: blurred backdrop, clock, avatar, password field; PAM authentication.

use decebal::lock::auth::{Authenticator, PamAuth};
use decebal::lock::{self, BgMode, Config};
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;

const HELP: &str = "declock - themed screen locker for X11

Usage: declock [--bg screenshot|wallpaper|color] [--blur N] [--dim 0..1] [--help | --version]

Covers every monitor with a blurred, dimmed backdrop (a screenshot of the desktop at the moment of locking, or the
wallpaper), a big clock and date, your picture (~/.face, else your initial) and name, and a rounded password field
that shows a dot per character. Colours, corner radius and fonts come from the Decebal theme, so it matches the bar
and the dock. The keyboard and pointer are grabbed for as long as it runs.

Keys:
  type, Backspace    edit the password        Enter    unlock        Escape or Ctrl+U    clear the field

A wrong password shakes the field and says so; from the third failure in a row input is ignored for 1, 2, 4, 8 s
(then 8 s each time), with a countdown. \"Caps Lock is on\" is shown while it is.

Options (environment in brackets):
  --bg MODE    screenshot (default; falls back to the wallpaper, then the theme colour), wallpaper, or color  [DECLOCK_BG]
  --blur N     blur radius, in units of 8 screen pixels (default 3)                                            [DECLOCK_BLUR]
  --dim F      how far the backdrop is darkened, 0 to 1 (default 0.45)                                          [DECLOCK_DIM]
  DECLOCK_PAM_SERVICE   PAM service to authenticate with (default login)

Stays in the foreground until unlocked, then exits 0 (what `xss-lock --transfer-sleep-lock` expects; it also closes
XSS_SLEEP_LOCK_FD once the screen is locked). Exits 1 if the screen could not be locked (the keyboard or pointer
could not be grabbed), 2 on bad usage, 3 if PAM is unavailable: nothing is left on screen then, so a wrapper such
as rdwm-lock can fall back to another locker. A second declock while one runs exits 0 at once.

Files: ~/.face (picture), the shared theme (see docs/lockscreen.md), clockHour12 in decebal.json for a 12-hour clock.";

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("declock: {msg}\nTry `declock --help`.");
    ExitCode::from(2)
}

/// The real PAM authenticator; with a `test-auth` build (never the shipped one) a fake one when asked for.
fn authenticator(user: &str) -> Result<Arc<dyn Authenticator>, String> {
    #[cfg(feature = "test-auth")]
    if std::env::var("DECLOCK_FAKE_AUTH").as_deref() == Ok("1") {
        let pw = std::env::var("DECLOCK_TEST_PASSWORD").map_err(|_| "DECLOCK_FAKE_AUTH needs DECLOCK_TEST_PASSWORD".to_string())?;
        let mut fake = decebal::lock::auth::FakeAuth::new(&pw);
        fake.delay = std::time::Duration::from_millis(std::env::var("DECLOCK_FAKE_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
        eprintln!("declock: TEST BUILD with a fake authenticator; do not use this binary to protect a screen");
        return Ok(Arc::new(fake));
    }
    let service = std::env::var("DECLOCK_PAM_SERVICE").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "login".into());
    PamAuth::new(&service, user).map(|p| Arc::new(p) as Arc<dyn Authenticator>).map_err(|e| format!("PAM ({service}) is not usable: {e}"))
}

/// No core dumps and no ptrace/proc-mem access by other processes of the same user: the password is in memory.
fn harden() {
    // SAFETY: prctl/setrlimit with constant arguments and a valid rlimit struct.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        libc::setrlimit(libc::RLIMIT_CORE, &zero);
    }
}

fn main() -> ExitCode {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    let env_or = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let mut bg = env_or("DECLOCK_BG");
    let mut blur = env_or("DECLOCK_BLUR");
    let mut dim = env_or("DECLOCK_DIM");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        match name.as_str() {
            "-h" | "--help" => {
                // `declock --help | head` closes the pipe early: that is not an error
                let _ = writeln!(std::io::stdout(), "{HELP}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                let _ = writeln!(std::io::stdout(), "declock {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--bg" | "--blur" | "--dim" => {
                let Some(v) = inline.or_else(|| args.next()) else { return usage_error(&format!("{name} needs a value")) };
                match name.as_str() {
                    "--bg" => bg = Some(v),
                    "--blur" => blur = Some(v),
                    _ => dim = Some(v),
                }
            }
            _ => return usage_error(&format!("unknown argument `{a}`")),
        }
    }
    let bg = match bg.as_deref().map(BgMode::parse) {
        None => BgMode::Screenshot,
        Some(Some(m)) => m,
        Some(None) => return usage_error("--bg is screenshot, wallpaper or color"),
    };
    let blur = match blur.as_deref().map(str::parse::<usize>) {
        None => Config::DEFAULT_BLUR,
        Some(Ok(n)) if n <= 30 => n,
        Some(_) => return usage_error("--blur is a whole number from 0 to 30"),
    };
    let dim = match dim.as_deref().map(str::parse::<f32>) {
        None => Config::DEFAULT_DIM,
        Some(Ok(d)) if (0.0..=1.0).contains(&d) => d,
        Some(_) => return usage_error("--dim is a number from 0 to 1"),
    };

    harden();
    let Some(user) = lock::current_user() else {
        eprintln!("declock: cannot find the passwd entry of uid {}", unsafe { libc::getuid() });
        return ExitCode::from(3);
    };
    let auth = match authenticator(&user.login) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("declock: {e}");
            return ExitCode::from(3);
        }
    };
    // One lock at a time; the guard (an flock) goes away with the process, so a crash cannot leave a stale one.
    let Some(_guard) = decebal::single::hold("declock") else {
        eprintln!("declock: already running");
        return ExitCode::SUCCESS;
    };
    let cfg = Config { bg, blur, dim, hour12: lock::hour12_setting(), user };
    match lock::x11::run(&cfg, auth) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("declock: could not lock the screen: {e}");
            ExitCode::from(1)
        }
    }
}
