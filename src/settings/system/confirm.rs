//! "Keep these display settings?" state machine. Pure: the caller passes the clock in.
//!
//! `begin` starts a countdown with the command that undoes the change. If another change arrives while
//! one is pending the *original* revert command is kept (reverting must go back to what the user last
//! confirmed, not to an unconfirmed intermediate state).

use super::run::Cmd;
use std::time::{Duration, Instant};

pub const CONFIRM_SECS: u64 = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub deadline: Instant,
    /// Commands that restore the previous layout.
    pub revert: Vec<Cmd>,
    /// What to persist when the user keeps the change (`xrandr` arguments).
    pub persist: Vec<String>,
    /// Safety flag file the out-of-process watchdog looks at.
    pub flag: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Idle,
    /// Seconds left (rounded up).
    Counting(u32),
    /// The countdown ran out: revert now.
    Expired(Pending),
}

#[derive(Debug, Default)]
pub struct Confirm {
    pending: Option<Pending>,
}

impl Confirm {
    pub fn active(&self) -> bool {
        self.pending.is_some()
    }

    pub fn begin(&mut self, now: Instant, secs: u64, revert: Vec<Cmd>, persist: Vec<String>, flag: Option<std::path::PathBuf>) {
        let deadline = now + Duration::from_secs(secs);
        match &mut self.pending {
            Some(p) => {
                p.deadline = deadline;
                p.persist = persist;
            }
            None => self.pending = Some(Pending { deadline, revert, persist, flag }),
        }
    }

    pub fn poll(&mut self, now: Instant) -> Step {
        match &self.pending {
            None => Step::Idle,
            Some(p) if now >= p.deadline => Step::Expired(self.pending.take().expect("pending")),
            Some(p) => Step::Counting(left(p.deadline, now)),
        }
    }

    pub fn seconds_left(&self, now: Instant) -> Option<u32> {
        self.pending.as_ref().map(|p| if now >= p.deadline { 0 } else { left(p.deadline, now) })
    }

    /// The user pressed "Keep changes".
    pub fn keep(&mut self) -> Option<Pending> {
        self.pending.take()
    }

    /// The user pressed "Revert" (same result as expiry).
    pub fn revert(&mut self) -> Option<Pending> {
        self.pending.take()
    }
}

fn left(deadline: Instant, now: Instant) -> u32 {
    let d = deadline.saturating_duration_since(now);
    (d.as_millis() as u32).div_ceil(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rv(s: &str) -> Vec<Cmd> {
        vec![Cmd::new("xrandr", &[s])]
    }

    #[test]
    fn counts_down_then_expires_with_the_revert() {
        let t0 = Instant::now();
        let mut c = Confirm::default();
        assert_eq!(c.poll(t0), Step::Idle);
        c.begin(t0, 15, rv("old"), vec!["new".into()], None);
        assert!(c.active());
        assert_eq!(c.poll(t0), Step::Counting(15));
        assert_eq!(c.poll(t0 + Duration::from_millis(1500)), Step::Counting(14));
        assert_eq!(c.poll(t0 + Duration::from_millis(14_001)), Step::Counting(1));
        match c.poll(t0 + Duration::from_secs(15)) {
            Step::Expired(p) => assert_eq!(p.revert, rv("old")),
            s => panic!("{s:?}"),
        }
        assert!(!c.active());
        assert_eq!(c.poll(t0 + Duration::from_secs(16)), Step::Idle);
    }

    #[test]
    fn keep_cancels_the_revert() {
        let t0 = Instant::now();
        let mut c = Confirm::default();
        c.begin(t0, 15, rv("old"), vec!["new".into()], None);
        let p = c.keep().unwrap();
        assert_eq!(p.persist, ["new"]);
        assert_eq!(c.poll(t0 + Duration::from_secs(60)), Step::Idle);
        assert!(c.keep().is_none());
    }

    #[test]
    fn second_change_keeps_the_original_revert_and_restarts_the_clock() {
        let t0 = Instant::now();
        let mut c = Confirm::default();
        c.begin(t0, 15, rv("confirmed"), vec!["a".into()], None);
        c.begin(t0 + Duration::from_secs(10), 15, rv("unconfirmed-intermediate"), vec!["b".into()], None);
        assert_eq!(c.poll(t0 + Duration::from_secs(16)), Step::Counting(9));
        let p = c.revert().unwrap();
        assert_eq!(p.revert, rv("confirmed"));
        assert_eq!(p.persist, ["b"]);
    }

    #[test]
    fn seconds_left_for_the_banner() {
        let t0 = Instant::now();
        let mut c = Confirm::default();
        assert_eq!(c.seconds_left(t0), None);
        c.begin(t0, 15, vec![], vec![], None);
        assert_eq!(c.seconds_left(t0 + Duration::from_secs(5)), Some(10));
        assert_eq!(c.seconds_left(t0 + Duration::from_secs(99)), Some(0));
    }
}
