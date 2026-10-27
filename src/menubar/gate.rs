//! `bar.enabled` in `macshell.json`: macbar can be switched off (a Windows-only layout) without the
//! session supervisor noticing. The process stays alive while hidden, because the supervisor
//! (`rdwm-macshell-session`) restarts a missing macbar within two seconds; only its surface goes away.
//! [`wait_until_visible`] is what the hidden process does, [`Gate`] is what a shown bar uses to notice it
//! must hide.

use crate::config;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the settings are looked at.
pub const POLL: Duration = Duration::from_secs(2);

/// True when the menu bar should be on screen according to the settings files.
pub fn visible_now() -> bool {
    config::shell_settings().bar_visible(config::dock_settings().style)
}

/// Polled from the bar's tick; raises `closed` when the bar has been switched off.
pub struct Gate {
    closed: Arc<AtomicBool>,
    last: Instant,
    visible: Box<dyn Fn() -> bool>,
}

impl Gate {
    pub fn new(closed: Arc<AtomicBool>) -> Self {
        Gate::with_check(closed, Box::new(visible_now))
    }

    /// Like [`Gate::new`] with another source of truth (tests).
    pub fn with_check(closed: Arc<AtomicBool>, visible: Box<dyn Fn() -> bool>) -> Self {
        Gate { closed, last: Instant::now(), visible }
    }

    pub fn poll(&mut self) {
        if self.last.elapsed() < POLL {
            return;
        }
        self.last = Instant::now();
        if !(self.visible)() {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// Blocks (sleeping, no surface, no threads) until the settings say the bar is wanted again.
pub fn wait_until_visible() {
    while !visible_now() {
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_raises_the_flag_once_the_bar_is_switched_off_and_is_rate_limited() {
        let shown = Arc::new(AtomicBool::new(true));
        let s2 = shown.clone();
        let flag = Arc::new(AtomicBool::new(false));
        let mut g = Gate::with_check(flag.clone(), Box::new(move || s2.load(Ordering::SeqCst)));
        g.last = Instant::now() - POLL;
        g.poll();
        assert!(!g.closed());
        shown.store(false, Ordering::SeqCst);
        g.poll();
        assert!(!g.closed(), "polling is rate limited");
        g.last = Instant::now() - POLL;
        g.poll();
        assert!(g.closed() && flag.load(Ordering::SeqCst));
    }
}
