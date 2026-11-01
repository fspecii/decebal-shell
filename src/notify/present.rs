//! Bounded, coalesced frame presentation for the Wayland front end, as a state machine with no protocol types so
//! it is unit tested in every feature combination.
//!
//! One [`Presenter`] belongs to each banner or panel surface:
//!
//! * only the **latest** painted frame waits to be shown; a newer paint replaces it (nothing queues up);
//! * at most [`MAX_BUFFERS`] shm buffers exist per surface, **counting the ones kept alive because the compositor has
//!   not released them yet, including buffers of an older size or scale**. A buffer is reused only once released;
//!   a size change never allocates past the bound, it waits for a release instead;
//! * a commit that attaches a frame asks for a frame callback and the next frame waits for it, with a bounded
//!   fallback ([`FRAME_STALL`]) so a compositor that never answers cannot stall the surface, and at most
//!   [`MAX_CALLBACKS`] callbacks are ever outstanding;
//! * when every buffer is busy the next attempt is scheduled ([`BUSY_RETRY`], then [`BUSY_BACKOFF`]) instead of spinning;
//! * a single buffer, one surface and the whole daemon have byte budgets; an allocation past one is refused (and
//!   reported by the caller), never silently grown.

use std::time::{Duration, Instant};

/// Shm buffers one surface may hold, including released-late buffers of an older size.
pub const MAX_BUFFERS: usize = 3;
/// Frame callbacks one surface may have outstanding.
pub const MAX_CALLBACKS: u32 = 2;
/// A frame callback that has not arrived after this long stops holding the next frame back.
pub const FRAME_STALL: Duration = Duration::from_secs(1);
/// All buffers busy: look again this soon (releases normally wake the loop sooner).
pub const BUSY_RETRY: Duration = Duration::from_millis(16);
/// Busy for longer than [`BUSY_BACKOFF_AFTER`]: look this rarely.
pub const BUSY_BACKOFF: Duration = Duration::from_millis(250);
pub const BUSY_BACKOFF_AFTER: Duration = Duration::from_secs(1);
/// Largest single buffer (a 4096 x 1024 card at 4 bytes a pixel).
pub const MAX_BUFFER_BYTES: usize = 16 << 20;
/// All buffers of one surface.
pub const MAX_SURFACE_BYTES: usize = 32 << 20;
/// All buffers of the daemon, including those of destroyed surfaces the compositor still holds.
pub const MAX_TOTAL_BYTES: usize = 96 << 20;

/// Whether the compositor has let go of a buffer (it may be drawn into and attached again, or destroyed).
pub trait Releasable {
    fn released(&self) -> bool;
}

#[derive(Debug)]
pub struct Entry<B> {
    pub buf: B,
    /// Device pixels.
    pub size: (u32, u32),
}

impl<B> Entry<B> {
    pub fn bytes(&self) -> usize {
        self.size.0 as usize * self.size.1 as usize * 4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// One buffer would be larger than [`MAX_BUFFER_BYTES`].
    Oversize,
    /// The surface or the daemon would pass its byte budget.
    Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Reuse(usize),
    /// Create a buffer and [`BufferSet::insert`] it.
    New,
    /// Every slot holds a buffer the compositor has not released.
    Busy,
    Refused(Refusal),
}

#[derive(Debug)]
pub struct BufferSet<B> {
    entries: Vec<Entry<B>>,
}

impl<B> Default for BufferSet<B> {
    fn default() -> Self {
        BufferSet { entries: Vec::new() }
    }
}

impl<B: Releasable> BufferSet<B> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.entries.iter().map(Entry::bytes).sum()
    }

    pub fn get(&self, i: usize) -> &B {
        &self.entries[i].buf
    }

    pub fn insert(&mut self, buf: B, size: (u32, u32)) -> usize {
        self.entries.push(Entry { buf, size });
        self.entries.len() - 1
    }

    /// Whether a buffer of `size` is held (released or not).
    pub fn has_size(&self, size: (u32, u32)) -> bool {
        self.entries.iter().any(|e| e.size == size)
    }

    /// Drops the released buffers that are not of `size` (an older size or scale): destroying them gives their
    /// memory back at once. Returns how many went.
    pub fn reap(&mut self, size: Option<(u32, u32)>) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| Some(e.size) == size || !e.buf.released());
        before - self.entries.len()
    }

    /// Hands every buffer over (a destroyed surface's buffers wait elsewhere until the compositor releases them).
    pub fn take_all(&mut self) -> Vec<Entry<B>> {
        std::mem::take(&mut self.entries)
    }

    /// Chooses where a frame of `size` goes. `budget_left` is what the daemon's byte budget still allows.
    pub fn pick(&mut self, size: (u32, u32), budget_left: usize) -> Pick {
        let bytes = size.0 as usize * size.1 as usize * 4;
        if bytes > MAX_BUFFER_BYTES {
            return Pick::Refused(Refusal::Oversize);
        }
        self.reap(Some(size));
        if let Some(i) = self.entries.iter().position(|e| e.size == size && e.buf.released()) {
            return Pick::Reuse(i);
        }
        if self.entries.len() >= MAX_BUFFERS {
            return Pick::Busy;
        }
        if self.bytes() + bytes > MAX_SURFACE_BYTES || bytes > budget_left {
            return Pick::Refused(Refusal::Budget);
        }
        Pick::New
    }
}

/// Frame-callback pacing and the busy back-off of one surface.
#[derive(Debug, Default)]
pub struct Pacer {
    pending_since: Option<Instant>,
    outstanding: u32,
    retry_at: Option<Instant>,
    busy_since: Option<Instant>,
}

impl Pacer {
    /// The next frame may go out: the last frame's callback arrived (or [`FRAME_STALL`] passed) and no busy retry is
    /// scheduled for later.
    pub fn can_commit(&self, now: Instant) -> bool {
        self.pending_since.is_none_or(|t| now.saturating_duration_since(t) >= FRAME_STALL) && self.retry_at.is_none_or(|t| now >= t)
    }

    /// Whether a commit now should ask for a frame callback (never more than [`MAX_CALLBACKS`] outstanding).
    pub fn should_request(&self) -> bool {
        self.outstanding < MAX_CALLBACKS
    }

    pub fn outstanding(&self) -> u32 {
        self.outstanding
    }

    pub fn committed(&mut self, now: Instant, requested: bool) {
        self.pending_since = Some(now);
        if requested {
            self.outstanding += 1;
        }
        self.retry_at = None;
        self.busy_since = None;
    }

    /// A frame callback arrived.
    pub fn done(&mut self) {
        self.outstanding = self.outstanding.saturating_sub(1);
        self.pending_since = None;
    }

    /// Every buffer was busy: try again later, less often the longer it lasts.
    pub fn busy(&mut self, now: Instant) {
        let since = *self.busy_since.get_or_insert(now);
        let wait = if now.saturating_duration_since(since) >= BUSY_BACKOFF_AFTER { BUSY_BACKOFF } else { BUSY_RETRY };
        self.retry_at = Some(now + wait);
    }

    /// How long the event loop may sleep for this surface: until the nearest of the callback fallback and the busy
    /// retry that is still in the future. `None` when neither is.
    pub fn wake_in(&self, now: Instant) -> Option<Duration> {
        let stall = self.pending_since.map(|t| t + FRAME_STALL);
        [stall, self.retry_at].into_iter().flatten().filter(|t| *t > now).map(|t| t - now).min()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub created: u64,
    pub reused: u64,
    pub commits: u64,
    /// A newer paint replaced a frame that was never shown.
    pub coalesced: u64,
    pub busy: u64,
    pub stale: u64,
    pub refused: u64,
    pub callbacks_requested: u64,
    pub callbacks_done: u64,
    pub max_buffers: usize,
    pub max_callbacks: u32,
}

impl Stats {
    /// Adds another surface's counters (a destroyed surface's totals are kept this way); maxima stay maxima.
    pub fn merge(&mut self, o: &Stats) {
        self.created += o.created;
        self.reused += o.reused;
        self.commits += o.commits;
        self.coalesced += o.coalesced;
        self.busy += o.busy;
        self.stale += o.stale;
        self.refused += o.refused;
        self.callbacks_requested += o.callbacks_requested;
        self.callbacks_done += o.callbacks_done;
        self.max_buffers = self.max_buffers.max(o.max_buffers);
        self.max_callbacks = self.max_callbacks.max(o.max_callbacks);
    }
}

impl Stats {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "created": self.created, "reused": self.reused, "commits": self.commits, "coalesced": self.coalesced, "busy": self.busy, "stale": self.stale,
            "refused": self.refused, "callbacks_requested": self.callbacks_requested, "callbacks_done": self.callbacks_done,
            "max_buffers": self.max_buffers, "max_callbacks": self.max_callbacks,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// No frame is waiting.
    Nothing,
    /// A frame waits for a callback, a release or the next scheduled attempt.
    Deferred,
    /// The waiting frame was not of the surface's size any more and was dropped (a repaint follows a layout change).
    Stale,
    /// The frame was dropped: its buffer would pass a byte budget.
    Refused(Refusal),
    Reuse(usize),
    New,
}

/// The latest painted frame of one surface, its bounded buffers and its pacing.
#[derive(Debug)]
pub struct Presenter<B, F> {
    pub frame: Option<F>,
    pub buffers: BufferSet<B>,
    pub pacer: Pacer,
    pub stats: Stats,
}

impl<B, F> Default for Presenter<B, F> {
    fn default() -> Self {
        Presenter { frame: None, buffers: BufferSet::default(), pacer: Pacer::default(), stats: Stats::default() }
    }
}

impl<B: Releasable, F> Presenter<B, F> {
    /// A new paint. Only the latest one is kept.
    pub fn submit(&mut self, frame: F) {
        if self.frame.replace(frame).is_some() {
            self.stats.coalesced += 1;
        }
    }

    /// Decides what to do with the waiting frame for a surface of `size` device pixels.
    pub fn plan(&mut self, now: Instant, size: (u32, u32), budget_left: usize, size_of: impl Fn(&F) -> (u32, u32)) -> Plan {
        let Some(frame) = &self.frame else { return Plan::Nothing };
        if size_of(frame) != size {
            self.frame = None;
            self.stats.stale += 1;
            return Plan::Stale;
        }
        if !self.pacer.can_commit(now) {
            return Plan::Deferred;
        }
        match self.buffers.pick(size, budget_left) {
            Pick::Reuse(i) => Plan::Reuse(i),
            Pick::New => Plan::New,
            Pick::Busy => {
                self.pacer.busy(now);
                self.stats.busy += 1;
                Plan::Deferred
            }
            Pick::Refused(why) => {
                self.frame = None;
                self.stats.refused += 1;
                Plan::Refused(why)
            }
        }
    }

    /// Takes the waiting frame to draw it into the buffer [`Self::plan`] chose.
    pub fn take(&mut self) -> Option<F> {
        self.frame.take()
    }

    /// Records a buffer created for [`Plan::New`].
    pub fn insert(&mut self, buf: B, size: (u32, u32)) -> usize {
        self.stats.created += 1;
        let i = self.buffers.insert(buf, size);
        self.stats.max_buffers = self.stats.max_buffers.max(self.buffers.len());
        i
    }

    /// The frame was attached and committed (`requested`: with a frame callback; `reused`: into a released buffer).
    pub fn committed(&mut self, now: Instant, requested: bool, reused: bool) {
        self.pacer.committed(now, requested);
        self.stats.commits += 1;
        if reused {
            self.stats.reused += 1;
        }
        if requested {
            self.stats.callbacks_requested += 1;
        }
        self.stats.max_callbacks = self.stats.max_callbacks.max(self.pacer.outstanding());
    }

    pub fn callback_done(&mut self) {
        self.pacer.done();
        self.stats.callbacks_done += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Clone)]
    struct Fake(Rc<Cell<bool>>);

    impl Releasable for Fake {
        fn released(&self) -> bool {
            self.0.get()
        }
    }

    type P = Presenter<Fake, (u32, u32, u32)>;

    fn size_of(f: &(u32, u32, u32)) -> (u32, u32) {
        (f.0, f.1)
    }

    /// Runs one present step the way the Wayland host does; returns the plan and, for a commit, the buffer's release flag.
    fn step(p: &mut P, now: Instant, size: (u32, u32), flags: &mut Vec<Rc<Cell<bool>>>) -> Plan {
        let plan = p.plan(now, size, MAX_TOTAL_BYTES, size_of);
        match plan {
            Plan::New => {
                p.take().unwrap();
                let flag = Rc::new(Cell::new(false)); // attached below: active
                p.insert(Fake(flag.clone()), size);
                flags.push(flag);
                let request = p.pacer.should_request();
                p.committed(now, request, false);
            }
            Plan::Reuse(i) => {
                p.take().unwrap();
                let fake = p.buffers.get(i).clone();
                fake.0.set(false);
                let request = p.pacer.should_request();
                p.committed(now, request, true);
            }
            _ => {}
        }
        plan
    }

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn only_the_latest_pending_frame_is_kept() {
        let mut p = P::default();
        for n in 0..6 {
            p.submit((100, 50, n));
        }
        assert_eq!(p.stats.coalesced, 5);
        assert_eq!(p.frame, Some((100, 50, 5)), "the newest paint wins");
        let mut flags = Vec::new();
        assert_eq!(step(&mut p, t0(), (100, 50), &mut flags), Plan::New);
        assert_eq!(p.stats.commits, 1);
        assert_eq!(step(&mut p, t0(), (100, 50), &mut flags), Plan::Nothing);
    }

    #[test]
    fn a_frame_waits_for_the_callback_then_goes_out() {
        let mut p = P::default();
        let mut flags = Vec::new();
        let now = t0();
        p.submit((100, 50, 0));
        assert_eq!(step(&mut p, now, (100, 50), &mut flags), Plan::New);
        assert_eq!(p.pacer.outstanding(), 1);
        // an input burst: many paints, nothing is committed while the callback is outstanding
        for n in 1..=50 {
            p.submit((100, 50, n));
            assert_eq!(step(&mut p, now + Duration::from_millis(n as u64 % 10), (100, 50), &mut flags), Plan::Deferred);
        }
        assert_eq!(p.stats.commits, 1);
        assert_eq!(p.stats.coalesced, 49);
        assert_eq!(p.buffers.len(), 1, "a burst allocates nothing");
        // the callback arrives; the compositor released the first buffer: the one pending frame reuses it
        p.callback_done();
        flags[0].set(true);
        assert_eq!(step(&mut p, now + Duration::from_millis(16), (100, 50), &mut flags), Plan::Reuse(0));
        assert_eq!(p.frame, None);
        assert_eq!(p.stats.created, 1);
    }

    #[test]
    fn a_lost_callback_falls_back_after_the_stall_and_never_exceeds_the_callback_bound() {
        let mut p = P::default();
        let mut flags = Vec::new();
        let now = t0();
        p.submit((100, 50, 0));
        step(&mut p, now, (100, 50), &mut flags);
        p.submit((100, 50, 1));
        assert_eq!(p.pacer.wake_in(now), Some(FRAME_STALL), "the loop is told when the fallback opens");
        assert_eq!(step(&mut p, now + FRAME_STALL - Duration::from_millis(1), (100, 50), &mut flags), Plan::Deferred);
        // the fallback commit reuses nothing yet: the first buffer is still unreleased, so a second one is made
        let later = now + FRAME_STALL;
        assert_eq!(step(&mut p, later, (100, 50), &mut flags), Plan::New);
        assert_eq!(p.pacer.outstanding(), 2);
        // a third stalled frame commits without asking for a third callback
        p.submit((100, 50, 2));
        let third = later + FRAME_STALL;
        assert_eq!(step(&mut p, third, (100, 50), &mut flags), Plan::New);
        assert_eq!(p.pacer.outstanding(), MAX_CALLBACKS, "never more than the bound outstanding");
        assert_eq!(p.stats.max_callbacks, MAX_CALLBACKS);
        assert_eq!(p.stats.callbacks_requested, 2);
        // a late callback releases one slot of the bound
        p.callback_done();
        assert_eq!(p.pacer.outstanding(), 1);
    }

    #[test]
    fn delayed_releases_never_grow_past_three_buffers() {
        let mut p = P::default();
        let mut flags = Vec::new();
        let mut now = t0();
        // the compositor holds every buffer: frames keep being painted and callbacks keep arriving
        for n in 0..40 {
            p.submit((100, 50, n));
            step(&mut p, now, (100, 50), &mut flags);
            p.callback_done();
            now += Duration::from_millis(20);
            assert!(p.buffers.len() <= MAX_BUFFERS, "{} buffers after frame {n}", p.buffers.len());
        }
        assert_eq!(p.buffers.len(), MAX_BUFFERS);
        assert_eq!(p.stats.created, MAX_BUFFERS as u64, "a busy compositor makes the surface wait, not allocate");
        assert!(p.stats.busy > 0);
        assert!(p.frame.is_some(), "the newest frame is still waiting for a release");
        // releases arrive late, one at a time: each lets exactly the pending frame out
        flags[1].set(true);
        now += BUSY_BACKOFF;
        assert_eq!(step(&mut p, now, (100, 50), &mut flags), Plan::Reuse(1));
        assert_eq!(p.stats.created, 3);
    }

    #[test]
    fn busy_retries_are_scheduled_and_back_off_so_the_loop_never_spins() {
        let mut p = P::default();
        let mut flags = Vec::new();
        let start = t0();
        for n in 0..3 {
            p.submit((100, 50, n));
            step(&mut p, start, (100, 50), &mut flags);
            p.callback_done();
        }
        p.submit((100, 50, 9));
        assert_eq!(step(&mut p, start, (100, 50), &mut flags), Plan::Deferred);
        assert_eq!(p.pacer.wake_in(start), Some(BUSY_RETRY));
        assert_eq!(step(&mut p, start + Duration::from_millis(5), (100, 50), &mut flags), Plan::Deferred, "not before the retry time");
        assert!(!p.pacer.can_commit(start + Duration::from_millis(5)));
        let mut now = start;
        let mut wakes = Vec::new();
        for _ in 0..200 {
            now += p.pacer.wake_in(now).expect("a waiting surface always has a deadline");
            assert_eq!(step(&mut p, now, (100, 50), &mut flags), Plan::Deferred);
            wakes.push(p.pacer.wake_in(now).unwrap());
        }
        assert!(wakes.iter().all(|w| *w >= BUSY_RETRY), "no zero-length sleeps");
        assert_eq!(*wakes.last().unwrap(), BUSY_BACKOFF, "after a second of busy buffers the retries slow down");
        assert!(wakes.iter().filter(|w| **w == BUSY_RETRY).count() < 100, "the quick retries are bounded");
        assert_eq!(p.buffers.len(), 3);
        // one release ends it, and the back-off state resets
        flags[0].set(true);
        now += BUSY_BACKOFF;
        assert_eq!(step(&mut p, now, (100, 50), &mut flags), Plan::Reuse(0));
        assert_eq!(p.pacer.wake_in(now), Some(FRAME_STALL));
    }

    #[test]
    fn nothing_waiting_means_no_deadline() {
        let p = P::default();
        assert_eq!(p.pacer.wake_in(t0()), None);
        let mut p = P::default();
        let mut flags = Vec::new();
        p.submit((10, 10, 0));
        step(&mut p, t0(), (10, 10), &mut flags);
        p.callback_done();
        assert_eq!(p.pacer.wake_in(t0()), None, "a delivered callback leaves nothing to wait for");
    }

    #[test]
    fn a_size_change_waits_for_a_release_instead_of_allocating_past_the_bound() {
        let mut p = P::default();
        let mut flags = Vec::new();
        let mut now = t0();
        for n in 0..3 {
            p.submit((100, 50, n));
            step(&mut p, now, (100, 50), &mut flags);
            p.callback_done();
            now += Duration::from_millis(20);
        }
        assert_eq!(p.buffers.len(), 3);
        // the surface grows; all three old-size buffers are still held by the compositor
        p.submit((120, 60, 7));
        assert_eq!(step(&mut p, now, (120, 60), &mut flags), Plan::Deferred);
        assert_eq!(p.buffers.len(), 3, "old-size active buffers count toward the bound");
        assert_eq!(p.stats.created, 3);
        // one old buffer is released: it is destroyed (not reused for the wrong size) and the slot is used for the new size
        flags[2].set(true);
        now += BUSY_RETRY;
        assert_eq!(step(&mut p, now, (120, 60), &mut flags), Plan::New);
        assert_eq!(p.buffers.len(), 3);
        assert_eq!(p.stats.created, 4);
        assert_eq!(p.buffers.bytes(), 2 * 100 * 50 * 4 + 120 * 60 * 4);
        // the other old buffers go as soon as they are released, even with nothing to draw
        flags[0].set(true);
        flags[1].set(true);
        assert_eq!(p.buffers.reap(Some((120, 60))), 2);
        assert_eq!(p.buffers.len(), 1);
    }

    #[test]
    fn resize_and_scale_churn_stays_bounded_whatever_the_release_order() {
        let mut p = P::default();
        let mut flags = Vec::new();
        let mut now = t0();
        // sizes as a scale change 1 -> 1.5 -> 2 -> 1 and card growth would produce them
        let sizes = [(344, 80), (516, 120), (688, 160), (344, 80), (344, 120), (360, 120), (516, 180)];
        let mut seed = 0x2545_f491u32;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for round in 0..400u32 {
            let size = sizes[(round as usize / 7) % sizes.len()];
            p.submit((size.0, size.1, round));
            step(&mut p, now, size, &mut flags);
            if rand() % 3 == 0 {
                p.callback_done();
            }
            // the compositor releases a random buffer now and then
            if !flags.is_empty() && rand() % 4 == 0 {
                flags[(rand() as usize) % flags.len()].set(true);
            }
            now += Duration::from_millis(5 + u64::from(rand() % 40));
            assert!(p.buffers.len() <= MAX_BUFFERS, "round {round}: {} buffers", p.buffers.len());
            assert!(p.pacer.outstanding() <= MAX_CALLBACKS);
            assert!(p.buffers.bytes() <= MAX_SURFACE_BYTES);
        }
        assert!(p.stats.max_buffers <= MAX_BUFFERS);
        assert!(p.stats.max_callbacks <= MAX_CALLBACKS);
        assert!(p.stats.commits > 5, "the churn still made progress");
        assert!(p.stats.created + p.stats.reused >= p.stats.commits);
    }

    #[test]
    fn a_frame_of_the_wrong_size_is_dropped_not_presented() {
        let mut p = P::default();
        p.submit((100, 50, 0));
        let mut flags = Vec::new();
        assert_eq!(step(&mut p, t0(), (120, 60), &mut flags), Plan::Stale);
        assert_eq!(p.frame, None);
        assert_eq!(p.stats.stale, 1);
        assert_eq!(p.buffers.len(), 0);
    }

    #[test]
    fn oversize_and_budget_refusals_drop_the_frame_and_are_counted() {
        let mut p = P::default();
        p.submit((5000, 1000, 0));
        assert_eq!(p.plan(t0(), (5000, 1000), MAX_TOTAL_BYTES, size_of), Plan::Refused(Refusal::Oversize));
        assert_eq!((p.frame, p.stats.refused), (None, 1));
        // the daemon-wide budget refuses even a normal card when nothing is left
        p.submit((344, 100, 0));
        assert_eq!(p.plan(t0(), (344, 100), 1000, size_of), Plan::Refused(Refusal::Budget));
        assert_eq!(p.stats.refused, 2);
        // the surface budget: three 4 MiB buffers fit, a fourth size would not
        let mut set: BufferSet<Fake> = BufferSet::default();
        let big = (2048, 512); // 4 MiB
        for _ in 0..MAX_BUFFERS {
            assert_eq!(set.pick(big, MAX_TOTAL_BYTES), Pick::New);
            set.insert(Fake(Rc::new(Cell::new(false))), big);
        }
        assert_eq!(set.pick(big, MAX_TOTAL_BYTES), Pick::Busy);
        let mut set: BufferSet<Fake> = BufferSet::default();
        let huge = (4000, 1000); // 16 MB: allowed alone, but not twice within 32 MiB
        assert_eq!(set.pick(huge, MAX_TOTAL_BYTES), Pick::New);
        set.insert(Fake(Rc::new(Cell::new(false))), huge);
        assert_eq!(set.pick(huge, MAX_TOTAL_BYTES), Pick::New);
        set.insert(Fake(Rc::new(Cell::new(false))), huge);
        assert_eq!(set.pick(huge, MAX_TOTAL_BYTES), Pick::Refused(Refusal::Budget));
    }

    #[test]
    fn a_released_buffer_of_the_right_size_is_reused_before_a_new_one_is_made() {
        let mut set: BufferSet<Fake> = BufferSet::default();
        let flag = Rc::new(Cell::new(false));
        set.insert(Fake(flag.clone()), (10, 10));
        assert_eq!(set.pick((10, 10), MAX_TOTAL_BYTES), Pick::New, "the only buffer is still held");
        flag.set(true);
        assert_eq!(set.pick((10, 10), MAX_TOTAL_BYTES), Pick::Reuse(0));
        assert_eq!(set.pick((12, 10), MAX_TOTAL_BYTES), Pick::New, "a released buffer of another size is dropped, not reused");
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn totals_of_destroyed_surfaces_merge_with_maxima_kept() {
        let mut a = Stats { created: 3, commits: 5, max_buffers: 3, max_callbacks: 1, ..Stats::default() };
        let b = Stats { created: 1, commits: 2, coalesced: 4, max_buffers: 2, max_callbacks: 2, ..Stats::default() };
        a.merge(&b);
        assert_eq!((a.created, a.commits, a.coalesced, a.max_buffers, a.max_callbacks), (4, 7, 4, 3, 2));
    }

    #[test]
    fn a_destroyed_surfaces_buffers_can_be_handed_over() {
        let mut set: BufferSet<Fake> = BufferSet::default();
        set.insert(Fake(Rc::new(Cell::new(false))), (10, 10));
        set.insert(Fake(Rc::new(Cell::new(true))), (10, 10));
        let taken = set.take_all();
        assert_eq!(taken.len(), 2);
        assert_eq!(taken.iter().map(Entry::bytes).sum::<usize>(), 800);
        assert!(set.is_empty());
    }
}
