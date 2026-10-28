//! Client-side rate limit for wallhaven's API (45 requests a minute): a sliding window that stays a
//! little under that, plus a cool-down after the server said 429. Time is passed in, so it is testable.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Requests per window. Wallhaven allows 45 a minute; this leaves room for other tools on the same key / IP.
pub const LIMIT: usize = 40;
pub const WINDOW: Duration = Duration::from_secs(60);
/// Cool-down after a 429 that came without a `Retry-After`.
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(60);
/// A request that would have to wait longer than this is refused instead of blocking a worker.
pub const MAX_WAIT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Send now (the request has been counted).
    Go,
    /// The window is full: the oldest request leaves it after this long.
    Wait(Duration),
    /// The server told us to back off; nothing is sent until then.
    Blocked(Duration),
}

#[derive(Debug)]
pub struct Throttle {
    limit: usize,
    window: Duration,
    stamps: VecDeque<Instant>,
    blocked_until: Option<Instant>,
}

impl Default for Throttle {
    fn default() -> Self {
        Throttle::new(LIMIT, WINDOW)
    }
}

impl Throttle {
    pub fn new(limit: usize, window: Duration) -> Throttle {
        Throttle { limit: limit.max(1), window, stamps: VecDeque::new(), blocked_until: None }
    }

    fn expire(&mut self, now: Instant) {
        while self.stamps.front().is_some_and(|t| now.duration_since(*t) >= self.window) {
            self.stamps.pop_front();
        }
        if self.blocked_until.is_some_and(|b| now >= b) {
            self.blocked_until = None;
        }
    }

    /// Asks to send one request at `now`.
    pub fn check(&mut self, now: Instant) -> Verdict {
        self.expire(now);
        if let Some(b) = self.blocked_until {
            return Verdict::Blocked(b.duration_since(now));
        }
        if self.stamps.len() >= self.limit {
            let oldest = self.stamps.front().copied().unwrap_or(now);
            return Verdict::Wait((oldest + self.window).saturating_duration_since(now));
        }
        self.stamps.push_back(now);
        Verdict::Go
    }

    /// The server answered 429: stop for `d` (or the default).
    pub fn penalise(&mut self, now: Instant, d: Option<Duration>) {
        let d = d.unwrap_or(DEFAULT_COOLDOWN).clamp(Duration::from_secs(1), Duration::from_secs(600));
        self.blocked_until = Some(now + d);
    }

    /// Seconds left of a cool-down, if any.
    pub fn blocked_for(&mut self, now: Instant) -> Option<Duration> {
        self.expire(now);
        self.blocked_until.map(|b| b.duration_since(now))
    }

    pub fn used(&self) -> usize {
        self.stamps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_the_limit_then_waits_for_the_oldest_to_leave() {
        let t0 = Instant::now();
        let mut t = Throttle::new(3, Duration::from_secs(60));
        for i in 0..3 {
            assert_eq!(t.check(t0 + Duration::from_secs(i)), Verdict::Go);
        }
        assert_eq!(t.check(t0 + Duration::from_secs(10)), Verdict::Wait(Duration::from_secs(50)));
        assert_eq!(t.used(), 3, "a refused request is not counted");
        assert_eq!(t.check(t0 + Duration::from_secs(60)), Verdict::Go, "the first slot is free again after the window");
        assert_eq!(t.check(t0 + Duration::from_secs(60)), Verdict::Wait(Duration::from_secs(1)));
    }

    #[test]
    fn never_more_than_the_limit_in_any_window() {
        let t0 = Instant::now();
        let mut t = Throttle::default();
        let mut sent = Vec::new();
        // hammer it every 100 ms for three minutes
        for i in 0..1800u64 {
            let now = t0 + Duration::from_millis(i * 100);
            if t.check(now) == Verdict::Go {
                sent.push(now);
            }
        }
        for (i, s) in sent.iter().enumerate() {
            let in_window = sent[i..].iter().take_while(|x| x.duration_since(*s) < WINDOW).count();
            assert!(in_window <= LIMIT, "{in_window} requests inside one minute");
        }
        const { assert!(LIMIT < 45, "stays under wallhaven's 45 a minute") };
        assert!(sent.len() >= 100, "but still makes progress: {}", sent.len());
    }

    #[test]
    fn a_429_blocks_everything_until_the_cooldown_ends() {
        let t0 = Instant::now();
        let mut t = Throttle::default();
        assert_eq!(t.check(t0), Verdict::Go);
        t.penalise(t0, Some(Duration::from_secs(30)));
        assert_eq!(t.check(t0 + Duration::from_secs(5)), Verdict::Blocked(Duration::from_secs(25)));
        assert_eq!(t.blocked_for(t0 + Duration::from_secs(5)), Some(Duration::from_secs(25)));
        assert_eq!(t.check(t0 + Duration::from_secs(31)), Verdict::Go);
        assert_eq!(t.blocked_for(t0 + Duration::from_secs(31)), None);
    }

    #[test]
    fn cooldown_defaults_and_is_clamped() {
        let t0 = Instant::now();
        let mut t = Throttle::default();
        t.penalise(t0, None);
        assert_eq!(t.blocked_for(t0), Some(DEFAULT_COOLDOWN));
        t.penalise(t0, Some(Duration::from_secs(86_400)));
        assert_eq!(t.blocked_for(t0), Some(Duration::from_secs(600)), "a hostile Retry-After cannot lock the pane for a day");
        t.penalise(t0, Some(Duration::ZERO));
        assert_eq!(t.blocked_for(t0), Some(Duration::from_secs(1)));
    }
}
