//! One bounded latest-request worker per Dashboard. No font work on its event
//! thread; closed/stale requests cannot publish or keep waking an old surface.
use super::*;
use crate::window_app::Waker;
use std::sync::{Condvar, Mutex};

struct Job {
    revision: u64,
    generation: u64,
    input: Input,
    plan: FontPlan,
}
struct Done {
    revision: u64,
    pixels: Option<ClockFace>,
    #[cfg(test)]
    thread: std::thread::ThreadId,
}
#[derive(Default)]
struct Queue {
    pending: Option<Job>,
    done: Option<Done>,
    waker: Option<Waker>,
    stopped: bool,
}
#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
}
struct Worker {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    fn start(waker: Waker) -> Self {
        let shared = Arc::new(Shared::default());
        shared.queue.lock().unwrap().waker = Some(waker);
        let state = shared.clone();
        let thread = std::thread::Builder::new()
            .name("dux-clock".into())
            .spawn(move || {
                let mut clock = ClockFace::default();
                let mut fonts = None;
                loop {
                    let job = {
                        let mut q = state.queue.lock().unwrap_or_else(|p| p.into_inner());
                        while !q.stopped && q.pending.is_none() {
                            q = state.changed.wait(q).unwrap_or_else(|p| p.into_inner());
                        }
                        if q.stopped {
                            break;
                        }
                        q.pending.take().unwrap()
                    };
                    let current = job.generation == crate::theme::generation();
                    let pixels = if current {
                        clock.sync(&job.plan);
                        let fonts = fonts.get_or_insert_with(Fonts::load);
                        if let Some(fonts) = fonts {
                            clock.prepare(job.input, fonts);
                        }
                        (job.generation == crate::theme::generation()).then(|| clock.pixels())
                    } else {
                        None
                    };
                    let mut q = state.queue.lock().unwrap_or_else(|p| p.into_inner());
                    if q.stopped {
                        break;
                    }
                    // Skip obsolete output before a newer queued job, not merely
                    // when the UI next polls it. One active + one pending maximum.
                    if q.pending.as_ref().is_some_and(|next| next.revision > job.revision) {
                        continue;
                    }
                    q.done = Some(Done {
                        revision: job.revision,
                        pixels,
                        #[cfg(test)]
                        thread: std::thread::current().id(),
                    });
                    // Drop sets stopped under this same mutex, preventing a late
                    // wake from escaping after the Dashboard releases its worker.
                    if let Some(waker) = &q.waker {
                        waker();
                    }
                }
            })
            .expect("clock worker");
        Self { shared, thread: Some(thread) }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let mut q = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        q.stopped = true;
        q.pending = None;
        q.done = None;
        q.waker = None;
        self.shared.changed.notify_one();
        // Detach only the already bounded CPU task; it sees stopped and exits
        // after its current preparation. Never block the event thread on join.
        self.thread.take();
    }
}

#[derive(Default)]
pub struct AsyncClock {
    plan: FontPlan,
    desired: Option<Input>,
    requested_scale: f32,
    revision: u64,
    published_revision: u64,
    worker: Option<Worker>,
    pixels: ClockFace,
    #[cfg(test)]
    prepared_on: Option<std::thread::ThreadId>,
}
impl AsyncClock {
    pub fn load(plan: &FontPlan) -> Self {
        Self { plan: plan.clone(), ..Default::default() }
    }
    /// A valid registered variable face, or the canonical fifth asset reference,
    /// opts into this capability. Existing four-font profiles stay unchanged.
    pub fn enabled(&self) -> bool {
        self.plan.files.iter().any(|path| path.file_name().is_some_and(|name| name == "GoogleSansFlex-Variable.ttf"))
            || self.plan.package_faces.iter().any(|face| face.variable && face.names.iter().any(|name| name == "Google Sans Flex"))
    }
    pub fn sync(&mut self, plan: &FontPlan) -> bool {
        if self.plan == *plan {
            return false;
        }
        self.plan = plan.clone();
        if !self.enabled() {
            self.worker = None;
        }
        self.desired = None;
        self.pixels = Default::default();
        // Invalidate an active old-font job even before the next request.
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn set_waker(&mut self, waker: Waker) {
        if !self.enabled() {
            return;
        }
        if let Some(worker) = &self.worker {
            worker.shared.queue.lock().unwrap().waker = Some(waker);
            return;
        }
        self.worker = Some(Worker::start(waker));
        if let Some(input) = self.desired.take() {
            self.request(input);
        }
    }
    pub fn request(&mut self, input: Input) -> bool {
        let scale = crate::theme::live::font_scale();
        if self.desired.as_ref() == Some(&input) && self.requested_scale == scale {
            return false;
        }
        if self.desired.as_ref().is_none_or(|old| old.width != input.width || old.height != input.height || old.scale != input.scale) {
            self.pixels = Default::default();
        }
        self.desired = Some(input.clone());
        self.requested_scale = scale;
        self.revision = self.revision.wrapping_add(1);
        if let Some(worker) = &self.worker {
            let mut q = worker.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
            q.done = None;
            q.pending = Some(Job { revision: self.revision, generation: crate::theme::generation(), input, plan: self.plan.clone() });
            worker.shared.changed.notify_one();
        }
        false // pending work is not a completed frame
    }
    pub fn poll(&mut self) -> bool {
        let Some(worker) = &self.worker else {
            return false;
        };
        let done = worker.shared.queue.lock().unwrap_or_else(|p| p.into_inner()).done.take();
        let Some(done) = done else {
            return false;
        };
        if done.revision != self.revision {
            return false;
        }
        if let Some(pixels) = done.pixels {
            self.pixels = pixels;
            self.published_revision = done.revision;
            #[cfg(test)]
            {
                self.prepared_on = Some(done.thread);
            }
            true
        } else {
            self.desired = None;
            false
        } // theme changed mid-job: retry latest
    }
    pub fn paint(&self, pm: &mut Pixmap, origin: (f32, f32), ink: Color, support: Color) -> bool {
        self.pixels.paint(pm, origin, ink, support)
    }
    pub fn available(&self) -> bool {
        self.pixels.available()
    }
    pub fn metrics(&self) -> Option<&Metrics> {
        self.pixels.metrics()
    }
}

#[cfg(test)]
impl AsyncClock {
    pub(crate) fn wait_prepared(&mut self) {
        if self.worker.is_none() {
            self.set_waker(Arc::new(|| {}));
        }
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while self.published_revision != self.revision {
            self.poll();
            assert!(std::time::Instant::now() < end, "clock worker did not publish");
            assert!(self.desired.is_some(), "test theme changed while preparing");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
pub(super) fn assert_async_pipeline(plan: &FontPlan, input: Input) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    let mut clock = AsyncClock::load(plan);
    clock.request(input.clone());
    assert!(clock.worker.is_none(), "requests before mapping only retain the latest geometry");
    clock.set_waker(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    clock.wait_prepared();
    assert!(clock.available());
    let thread = clock.prepared_on.unwrap();
    assert_ne!(thread, std::thread::current().id(), "font parsing/rasterization must be off the event thread");
    let first = clock.pixels.frame.as_ref().unwrap().clone();
    let mut next = input.clone();
    next.minutes = "09".into();
    clock.request(next);
    clock.wait_prepared();
    assert!(Arc::ptr_eq(&first, clock.pixels.frame.as_ref().unwrap()), "minute-only results share coverage, not copies");
    assert_eq!(clock.prepared_on, Some(thread));
    let mut latest = input.clone();
    for i in 0..1000 {
        latest.width = 96.0 + (i % 64) as f32;
        clock.request(latest.clone());
    }
    clock.wait_prepared();
    assert_eq!(clock.pixels.frame.as_ref().unwrap().input.width, latest.width, "only latest geometry may publish");
    assert!(clock.pixels.preparations < 1000, "resize storms must coalesce");
    assert_eq!(clock.prepared_on, Some(thread), "resizes reuse one bounded worker");
    // A version not registered by Source cannot reuse an old byte snapshot or
    // reopen a file. The old frame must not replace the missing-state result.
    let mut unregistered = plan.clone();
    for version in &mut unregistered.file_versions {
        version.1 = version.1.wrapping_add(1);
    }
    clock.sync(&unregistered);
    clock.request(input.clone());
    clock.wait_prepared();
    assert!(!clock.available());
    assert!(clock.pixels.fallback.is_some());
    clock.sync(plan);
    clock.request(input);
    clock.wait_prepared();
    assert!(clock.available());
    let mut pending = latest;
    pending.height = 512.0;
    clock.request(pending);
    let mut worker = clock.worker.take().unwrap();
    let handle = worker.thread.take().unwrap();
    drop(worker);
    let after = wakes.load(Ordering::SeqCst);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !handle.is_finished() {
        assert!(std::time::Instant::now() < end, "closed clock worker failed to stop");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    handle.join().unwrap();
    assert_eq!(after, wakes.load(Ordering::SeqCst), "closed surfaces must never receive late wakes");
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn an_unregistered_asset_does_not_enable_the_future_clock() {
        let mut plan = FontPlan {
            files: ["GoogleSansFlex-400.ttf", "GoogleSansFlex-500.ttf", "GoogleSansFlex-700.ttf", "MaterialSymbolsRounded.ttf"].iter().map(std::path::PathBuf::from).collect(),
            ..Default::default()
        };
        let mut clock = AsyncClock::load(&plan);
        assert!(!clock.enabled());
        clock.set_waker(Arc::new(|| panic!("no clock work for a four-font profile")));
        assert!(clock.worker.is_none());
        plan.files.push("GoogleSansFlex-Variable.ttf".into());
        clock.sync(&plan);
        assert!(clock.enabled(), "registration is the explicit capability boundary");
    }
}
