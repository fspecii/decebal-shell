//! The lock screen's state machine: what has been typed, whether a check is running, the wrong-password feedback
//! and the back-off after repeated failures. No X11, no PAM, no clock of its own (the caller passes `Instant`s),
//! so every rule is unit-tested.

use super::secret::Secret;
use std::time::{Duration, Instant};

/// Failed attempts that are free; from the next one on the input freezes for a growing delay.
pub const FREE_ATTEMPTS: u32 = 3;
/// Longest freeze.
pub const MAX_DELAY: Duration = Duration::from_secs(8);
/// Length of the "wrong password" shake.
pub const SHAKE: Duration = Duration::from_millis(450);

/// Delay imposed after the `failures`-th consecutive failure: none for the first two, then 1 s, 2 s, 4 s, 8 s, 8 s...
pub fn delay_after(failures: u32) -> Duration {
    if failures < FREE_ATTEMPTS {
        return Duration::ZERO;
    }
    let exp = (failures - FREE_ATTEMPTS).min(3);
    Duration::from_secs(1u64 << exp).min(MAX_DELAY)
}

/// A decoded key press (the X11 layer turns keycodes into these).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Char(char),
    Backspace,
    /// Escape or Ctrl+U: empty the field.
    Clear,
    Submit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for typing.
    Idle,
    /// A password is being checked on the worker thread; input is ignored.
    Checking,
    /// The last attempt failed (shows the message and shakes; typing continues normally).
    Wrong { since: Instant },
    /// Too many failures: input is ignored until `until`.
    Frozen { until: Instant },
    /// Authenticated: the caller exits.
    Unlocked,
}

/// What the caller has to do after feeding an event.
#[derive(Debug)]
pub enum Outcome {
    Nothing,
    Redraw,
    /// Start checking this password (the state is now `Checking`). The secret moves to the worker.
    Check(Secret),
}

/// Why the last attempt failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    WrongPassword,
    /// PAM itself failed (not a verdict on the password).
    AuthUnavailable,
}

pub struct LockState {
    secret: Secret,
    phase: Phase,
    failures: u32,
    fault: Fault,
    failed_at: Option<Instant>,
    /// Dots that were in the field when it was submitted (kept on screen while the check runs).
    submitted: usize,
    pub caps: bool,
}

impl Default for LockState {
    fn default() -> Self {
        Self::new()
    }
}

impl LockState {
    pub fn new() -> LockState {
        LockState { secret: Secret::new(), phase: Phase::Idle, failures: 0, fault: Fault::WrongPassword, failed_at: None, submitted: 0, caps: false }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    pub fn fault(&self) -> Fault {
        self.fault
    }

    /// Characters typed so far (dots in the field).
    pub fn dots(&self) -> usize {
        if self.phase == Phase::Checking {
            self.submitted
        } else {
            self.secret.chars()
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.phase == Phase::Unlocked
    }

    /// Moves time-based phases along; true when the picture changes as a result.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.phase {
            Phase::Frozen { until } if now >= until => {
                self.phase = Phase::Idle;
                true
            }
            _ => false,
        }
    }

    /// Shake progress 0..1 while the shake after a failed attempt runs (also at the start of a freeze).
    pub fn shake(&self, now: Instant) -> Option<f32> {
        let el = now.saturating_duration_since(self.failed_at?);
        (matches!(self.phase, Phase::Wrong { .. } | Phase::Frozen { .. }) && el < SHAKE).then(|| el.as_secs_f32() / SHAKE.as_secs_f32())
    }

    /// Seconds left of a freeze, rounded up (for "Try again in 3 s").
    pub fn frozen_secs(&self, now: Instant) -> Option<u64> {
        match self.phase {
            Phase::Frozen { until } => Some(until.saturating_duration_since(now).as_millis().div_ceil(1000) as u64),
            _ => None,
        }
    }

    /// How long until the picture next changes by itself (animation frame, end of a freeze), if at all.
    pub fn next_change(&self, now: Instant) -> Option<Duration> {
        let frame = self.shake(now).map(|_| Duration::from_millis(16));
        let freeze = match self.phase {
            Phase::Frozen { until } => Some(until.saturating_duration_since(now)),
            _ => None,
        };
        match (frame, freeze) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub fn input(&mut self, input: Input, now: Instant) -> Outcome {
        match self.phase {
            // Nothing is accepted while a check runs or during a freeze, and the keystrokes are dropped, not queued.
            Phase::Checking | Phase::Unlocked => return Outcome::Nothing,
            Phase::Frozen { until } if now < until => return Outcome::Nothing,
            Phase::Frozen { .. } => self.phase = Phase::Idle,
            _ => {}
        }
        match input {
            Input::Char(c) => {
                self.dismiss_wrong();
                if self.secret.push(c) {
                    Outcome::Redraw
                } else {
                    Outcome::Nothing
                }
            }
            Input::Backspace => {
                self.dismiss_wrong();
                if self.secret.pop() {
                    Outcome::Redraw
                } else {
                    Outcome::Nothing
                }
            }
            Input::Clear => {
                self.dismiss_wrong();
                self.secret.clear();
                Outcome::Redraw
            }
            Input::Submit => {
                if self.secret.is_empty() {
                    return Outcome::Nothing;
                }
                self.submitted = self.secret.chars();
                self.phase = Phase::Checking;
                Outcome::Check(std::mem::take(&mut self.secret))
            }
        }
    }

    fn dismiss_wrong(&mut self) {
        if matches!(self.phase, Phase::Wrong { .. }) {
            self.phase = Phase::Idle;
        }
    }

    /// The worker's verdict. `Err(())` = PAM could not decide (counts as a failure, shown differently).
    pub fn verdict(&mut self, result: Result<bool, ()>, now: Instant) {
        if self.phase != Phase::Checking {
            return;
        }
        match result {
            Ok(true) => {
                self.phase = Phase::Unlocked;
                self.failures = 0;
            }
            other => {
                self.failures += 1;
                self.failed_at = Some(now);
                self.fault = if other.is_err() { Fault::AuthUnavailable } else { Fault::WrongPassword };
                let d = delay_after(self.failures);
                self.phase = if d.is_zero() { Phase::Wrong { since: now } } else { Phase::Frozen { until: now + d } };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    fn type_str(st: &mut LockState, s: &str, now: Instant) {
        for c in s.chars() {
            st.input(Input::Char(c), now);
        }
    }

    fn submit(st: &mut LockState, now: Instant) -> Secret {
        match st.input(Input::Submit, now) {
            Outcome::Check(s) => s,
            other => panic!("expected a check, got {other:?}"),
        }
    }

    #[test]
    fn delays_follow_the_back_off_schedule() {
        let secs: Vec<u64> = (0..9).map(|n| delay_after(n).as_secs()).collect();
        assert_eq!(secs, [0, 0, 0, 1, 2, 4, 8, 8, 8], "indexed by the number of failures so far");
    }

    #[test]
    fn typing_builds_dots_and_editing_works() {
        let now = t0();
        let mut st = LockState::new();
        type_str(&mut st, "abc", now);
        assert_eq!(st.dots(), 3);
        st.input(Input::Backspace, now);
        assert_eq!(st.dots(), 2);
        st.input(Input::Clear, now);
        assert_eq!(st.dots(), 0);
        assert!(matches!(st.input(Input::Backspace, now), Outcome::Nothing));
    }

    #[test]
    fn empty_submit_does_nothing() {
        let mut st = LockState::new();
        assert!(matches!(st.input(Input::Submit, t0()), Outcome::Nothing));
        assert_eq!(st.phase(), Phase::Idle);
    }

    #[test]
    fn submit_moves_the_password_out_and_empties_the_field() {
        let now = t0();
        let mut st = LockState::new();
        type_str(&mut st, "hunter2", now);
        let secret = submit(&mut st, now);
        assert_eq!(secret.as_bytes(), b"hunter2");
        assert_eq!((st.phase(), st.dots()), (Phase::Checking, 7), "the dots stay while checking");
        // input during the check is dropped, not queued
        type_str(&mut st, "zzz", now);
        assert_eq!(st.dots(), 7);
        st.verdict(Ok(false), now);
        assert_eq!(st.dots(), 0, "nothing typed during the check was kept");
    }

    #[test]
    fn success_unlocks_and_resets_failures() {
        let now = t0();
        let mut st = LockState::new();
        type_str(&mut st, "a", now);
        submit(&mut st, now);
        st.verdict(Ok(false), now);
        type_str(&mut st, "b", now);
        submit(&mut st, now);
        st.verdict(Ok(true), now);
        assert!(st.is_unlocked());
        assert_eq!(st.failures(), 0);
    }

    #[test]
    fn a_wrong_password_shakes_and_typing_dismisses_it() {
        let now = t0();
        let mut st = LockState::new();
        type_str(&mut st, "x", now);
        submit(&mut st, now);
        st.verdict(Ok(false), now);
        assert!(matches!(st.phase(), Phase::Wrong { .. }));
        assert_eq!(st.fault(), Fault::WrongPassword);
        assert!(st.shake(now).is_some());
        assert!(st.shake(now + SHAKE).is_none());
        assert!(st.next_change(now).is_some());
        st.input(Input::Char('y'), now + Duration::from_millis(100));
        assert_eq!(st.phase(), Phase::Idle);
        assert_eq!(st.dots(), 1);
    }

    #[test]
    fn escape_clears_the_field_and_the_message() {
        let now = t0();
        let mut st = LockState::new();
        type_str(&mut st, "x", now);
        submit(&mut st, now);
        st.verdict(Ok(false), now);
        type_str(&mut st, "partial", now);
        st.input(Input::Clear, now);
        assert_eq!((st.dots(), st.phase()), (0, Phase::Idle));
    }

    #[test]
    fn the_third_failure_freezes_input_with_growing_delays() {
        let mut now = t0();
        let mut st = LockState::new();
        for round in 1..=6u32 {
            type_str(&mut st, "bad", now);
            submit(&mut st, now);
            st.verdict(Ok(false), now);
            let want = delay_after(round);
            if want.is_zero() {
                assert!(matches!(st.phase(), Phase::Wrong { .. }), "attempt {round}");
                continue;
            }
            assert!(matches!(st.phase(), Phase::Frozen { .. }), "attempt {round}");
            // typing and submitting are ignored while frozen
            type_str(&mut st, "ignored", now + want / 2);
            assert_eq!(st.dots(), 0);
            assert!(matches!(st.input(Input::Submit, now + want / 2), Outcome::Nothing));
            assert!(!st.tick(now + want / 2));
            assert_eq!(st.frozen_secs(now), Some(want.as_secs()));
            now += want;
            assert!(st.tick(now));
            assert_eq!(st.phase(), Phase::Idle);
        }
        assert_eq!(st.failures(), 6);
    }

    #[test]
    fn input_after_the_freeze_works_even_without_a_tick() {
        let now = t0();
        let mut st = LockState::new();
        for _ in 0..3 {
            type_str(&mut st, "bad", now);
            submit(&mut st, now);
            st.verdict(Ok(false), now);
            if let Phase::Frozen { until } = st.phase() {
                st.input(Input::Char('q'), until);
            }
        }
        assert_eq!(st.dots(), 1);
    }

    #[test]
    fn pam_failure_counts_and_is_labelled() {
        let now = t0();
        let mut st = LockState::new();
        type_str(&mut st, "x", now);
        submit(&mut st, now);
        st.verdict(Err(()), now);
        assert_eq!((st.failures(), st.fault()), (1, Fault::AuthUnavailable));
    }

    #[test]
    fn a_stray_verdict_is_ignored() {
        let mut st = LockState::new();
        st.verdict(Ok(true), t0());
        assert!(!st.is_unlocked(), "a verdict without a pending check must never unlock");
    }
}
