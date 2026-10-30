//! Persistent countdown. Wall-clock deadlines include suspend and time spent closed;
//! manual clock corrections consequently change remaining time. Delivery is at most
//! once: a durable claim precedes the notifier (a crash between them may lose delivery).
use crate::{
    app::Button,
    text::{Fonts, Weight},
    window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tiny_skia::Pixmap;
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Ready,
    Running,
    Paused,
    Complete,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub status: Status,
    pub duration: u64,
    pub remaining: u64,
    pub title: String,
    deadline: Option<u64>,
    claimed: bool,
    #[serde(default)]
    pub notification_error: Option<String>,
    version: u8,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self { status: Status::Ready, duration: 300, remaining: 300, title: "Timer".into(), deadline: None, claimed: false, notification_error: None, version: 1 }
    }
}
impl Snapshot {
    fn at(mut self, now: u64) -> Self {
        if self.status == Status::Running {
            self.remaining = self.deadline.unwrap_or(now).saturating_sub(now).min(self.duration);
            if self.remaining == 0 {
                self.status = Status::Complete
            }
        }
        self
    }
    fn validate(&self) -> Result<()> {
        if self.version != 1
            || !(1..=86400).contains(&self.duration)
            || self.remaining > self.duration
            || self.title.len() > 120
            || self.title.chars().any(char::is_control)
            || (self.status == Status::Running) != self.deadline.is_some()
        {
            Err("Invalid timer state; move timer.json aside to recover".into())
        } else {
            Ok(())
        }
    }
}
pub trait Clock {
    fn now(&self) -> u64;
}
pub struct WallClock;
impl Clock for WallClock {
    fn now(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
    }
}
pub trait Notifier {
    fn notify(&self, title: &str) -> Result<()>;
}
pub struct DesktopNotifier;
impl Notifier for DesktopNotifier {
    fn notify(&self, title: &str) -> Result<()> {
        let mut child = Command::new("notify-send").args(["--app-name=Decebal Timer", "--", title, "Countdown finished"]).spawn().map_err(|e| format!("Notification unavailable: {e}"))?;
        let until = std::time::Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(s) = child.try_wait().map_err(err)? {
                break s;
            }
            if std::time::Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Notification timed out".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if status.success() {
            Ok(())
        } else {
            Err(format!("Notification failed: {status}"))
        }
    }
}
#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
impl Store {
    pub fn discover() -> Result<Self> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .ok_or("Neither XDG_STATE_HOME nor HOME is set")?;
        if !base.is_absolute() {
            return Err("State directory must be absolute".into());
        }
        Ok(Self::in_dir(base.join("decebal/timer")))
    }
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
    fn check(&self) -> Result<()> {
        let m = fs::symlink_metadata(&self.dir).map_err(err)?;
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.permissions().mode() & 0o077 != 0 {
            return Err("Timer directory must be owned by you and private (0700)".into());
        }
        Ok(())
    }
    fn read(&self) -> Result<Snapshot> {
        if !self.dir.exists() {
            return Ok(Snapshot::default());
        }
        self.check()?;
        let path = self.dir.join("timer.json");
        let mut f = match OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Snapshot::default()),
            Err(e) => return Err(err(e)),
        };
        let m = f.metadata().map_err(err)?;
        if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.permissions().mode() & 0o077 != 0 || m.len() > 8192 {
            return Err("Unsafe timer state permissions or size".into());
        }
        let mut data = String::new();
        f.read_to_string(&mut data).map_err(err)?;
        let s: Snapshot = serde_json::from_str(&data).map_err(|e| format!("Corrupt timer state: {e}"))?;
        s.validate()?;
        Ok(s)
    }
    /// Pure read: does not create a directory, lock, or state file.
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.snapshot_at(&WallClock)
    }
    pub fn snapshot_at(&self, c: &impl Clock) -> Result<Snapshot> {
        Ok(self.read()?.at(c.now()))
    }
    fn lock(&self) -> Result<File> {
        if !self.dir.exists() {
            {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = fs::DirBuilder::new();
                builder.recursive(true).mode(0o700).create(&self.dir).map_err(err)?;
            }
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700)).map_err(err)?;
        }
        self.check()?;
        let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(self.dir.join("lock")).map_err(err)?;
        let m = f.metadata().map_err(err)?;
        if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.permissions().mode() & 0o077 != 0 {
            return Err("Unsafe timer lock".into());
        }
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(err(std::io::Error::last_os_error()));
        }
        Ok(f)
    }
    fn write(&self, s: &Snapshot) -> Result<()> {
        let p = self.dir.join(format!(".timer-{}.tmp", std::process::id()));
        let result = (|| {
            let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&p).map_err(err)?;
            f.write_all(&serde_json::to_vec(s).map_err(err)?).map_err(err)?;
            f.sync_all().map_err(err)?;
            fs::rename(&p, self.dir.join("timer.json")).map_err(err)?;
            File::open(&self.dir).and_then(|d| d.sync_all()).map_err(err)
        })();
        if result.is_err() {
            let _ = fs::remove_file(p);
        }
        result
    }
    pub fn apply(&self, action: TimerAction) -> Result<Snapshot> {
        self.apply_at(action, &WallClock)
    }
    pub fn apply_at(&self, action: TimerAction, c: &impl Clock) -> Result<Snapshot> {
        let _guard = self.lock()?;
        let now = c.now();
        let mut s = self.read()?.at(now);
        match action {
            TimerAction::Start { seconds, title } => {
                if !(1..=86400).contains(&seconds) || title.len() > 120 || title.chars().any(char::is_control) {
                    return Err("Use 1–86400 seconds and a title of at most 120 bytes".into());
                }
                s = Snapshot {
                    status: Status::Running,
                    duration: seconds,
                    remaining: seconds,
                    title,
                    deadline: Some(now.checked_add(seconds).ok_or("Clock overflow")?),
                    claimed: false,
                    notification_error: None,
                    version: 1,
                };
            }
            TimerAction::Pause => {
                if s.status != Status::Running {
                    return Err("Timer is not running".into());
                }
                s.status = Status::Paused;
                s.deadline = None;
            }
            TimerAction::Resume => {
                if s.status != Status::Paused {
                    return Err("Timer is not paused".into());
                }
                s.status = Status::Running;
                s.deadline = Some(now.checked_add(s.remaining).ok_or("Clock overflow")?);
            }
            TimerAction::Reset => {
                s.status = Status::Ready;
                s.remaining = s.duration;
                s.deadline = None;
                s.claimed = false;
                s.notification_error = None;
            }
        }
        self.write(&s)?;
        Ok(s)
    }
    /// Claims under the same lock as edits. Never calls the notifier twice for a run.
    pub fn notify_due(&self, c: &impl Clock, n: &impl Notifier) -> Result<bool> {
        let _guard = self.lock()?;
        let mut s = self.read()?.at(c.now());
        if s.status != Status::Complete || s.claimed {
            return Ok(false);
        }
        s.claimed = true;
        s.deadline = None;
        self.write(&s)?;
        if let Err(e) = n.notify(&s.title) {
            s.notification_error = Some(e.clone());
            self.write(&s)?;
            return Err(e);
        }
        Ok(true)
    }
    /// Login recovery stays headless and read-only when there is no work. The
    /// existing worker lock arbitrates recovery, normal UI and duplicate logins.
    pub fn recover(&self) -> Result<()> {
        if self.recovery_needed(&WallClock)? {
            self.worker()?;
        }
        Ok(())
    }

    fn recovery_needed(&self, clock: &impl Clock) -> Result<bool> {
        let state = self.snapshot_at(clock)?;
        Ok(state.status == Status::Running || (state.status == Status::Complete && !state.claimed))
    }

    pub fn worker(&self) -> Result<()> {
        let guard = self.lock()?;
        let worker = OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(self.dir.join("worker.lock")).map_err(err)?;
        let meta = worker.metadata().map_err(err)?;
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.permissions().mode() & 0o077 != 0 {
            return Err("Unsafe worker lock".into());
        }
        if unsafe { libc::flock(worker.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(());
            }
            return Err(err(e));
        }
        drop(guard);
        loop {
            let edit_guard = self.lock()?;
            let s = self.read()?.at(WallClock.now());
            if s.status == Status::Running {
                drop(edit_guard);
                std::thread::sleep(Duration::from_secs(1));
            } else if s.status == Status::Complete && !s.claimed {
                drop(edit_guard);
                // Persist delivery failures, then recheck under the edit lock before exit.
                let _ = self.notify_due(&WallClock, &DesktopNotifier);
            } else {
                // Release worker ownership before edits can start/resume another run.
                drop(worker);
                drop(edit_guard);
                return Ok(());
            }
        }
    }

    /// Detached helper exits as soon as the timer pauses, resets, or completes.
    pub fn spawn_worker(&self) -> Result<()> {
        if !self.recovery_needed(&WallClock)? {
            return Ok(());
        }
        Command::new(std::env::current_exe().map_err(err)?).arg("--timer-worker").arg(&self.dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(err)?;
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum TimerAction {
    Start { seconds: u64, title: String },
    Pause,
    Resume,
    Reset,
}
pub fn parse_duration(s: &str) -> Result<u64> {
    let parts = s.split(':').map(str::parse::<u64>).collect::<std::result::Result<Vec<_>, _>>().map_err(|_| "Use seconds or MM:SS".to_string())?;
    let seconds = match parts.as_slice() {
        [s] => *s,
        [m, s] if *s < 60 => m.checked_mul(60).and_then(|v| v.checked_add(*s)).ok_or("Duration overflow")?,
        _ => return Err("Use seconds or MM:SS".into()),
    };
    if !(1..=86400).contains(&seconds) {
        return Err("Duration must be 1–86400 seconds".into());
    }
    Ok(seconds)
}
pub fn format_remaining(s: u64) -> String {
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

pub struct TimerApp {
    store: Store,
    pub input: String,
    pub error: Option<String>,
    fonts: Option<Fonts>,
    poll_token: std::sync::Arc<()>,
    snapshot: Snapshot,
    replace_input: bool,
    size: (u32, u32),
}
impl TimerApp {
    pub fn new(store: Store) -> Self {
        let result = store.snapshot();
        let error = result.as_ref().err().cloned().or_else(|| result.as_ref().ok().and_then(|s| s.notification_error.clone()));
        let snapshot = result.unwrap_or_default();
        let input = Self::duration_input(snapshot.duration);
        Self { store, input, error, fonts: None, poll_token: std::sync::Arc::new(()), snapshot, replace_input: false, size: (560, 460) }
    }
    fn duration_input(seconds: u64) -> String {
        // The editable field accepts MM:SS, unlike the HH:MM:SS countdown.
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
    fn sync_snapshot(&mut self, snapshot: Snapshot, reset_input: bool) {
        if reset_input || snapshot.duration != self.snapshot.duration {
            self.input = Self::duration_input(snapshot.duration);
            self.replace_input = false;
        }
        self.snapshot = snapshot;
    }
    fn perform(&mut self, a: TimerAction) {
        let reset_input = matches!(a, TimerAction::Reset);
        match self.store.apply(a) {
            Ok(s) => {
                self.sync_snapshot(s, reset_input);
                self.error = None;
                if self.snapshot.status == Status::Running {
                    if let Err(e) = self.store.spawn_worker() {
                        self.error = Some(format!("Timer saved, completion worker unavailable: {e}"));
                    }
                }
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn primary(&mut self) {
        match self.snapshot.status {
            Status::Running => self.perform(TimerAction::Pause),
            Status::Paused => self.perform(TimerAction::Resume),
            _ => match parse_duration(&self.input) {
                Ok(seconds) => self.perform(TimerAction::Start { seconds, title: "Timer".into() }),
                Err(e) => self.error = Some(e),
            },
        }
    }
}
impl WindowApp for TimerApp {
    fn title(&self) -> String {
        "Decebal Timer".into()
    }
    fn app_id(&self) -> &'static str {
        "dectimer"
    }
    fn initial_size(&self) -> (u32, u32) {
        self.size
    }
    fn set_waker(&mut self, w: Waker) {
        let token = std::sync::Arc::downgrade(&self.poll_token);
        let tick = w.clone();
        std::thread::spawn(move || {
            while token.strong_count() > 0 {
                std::thread::sleep(Duration::from_secs(1));
                if token.strong_count() > 0 {
                    tick();
                }
            }
        });
        crate::theme::live::start_watcher(move || w());
    }
    fn set_screen(&mut self, _: u32, _: u32) {}
    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w, h)
    }
    fn set_rounded(&mut self, _: bool) {}
    fn draw(&mut self, pm: &mut Pixmap) {
        use crate::gfx::fill_rounded;
        let t = crate::theme::th();
        pm.fill(t.background.color(1.));
        if self.fonts.is_none() {
            self.fonts = Fonts::load();
        }
        let w = self.size.0 as f32;
        let h = self.size.1 as f32;
        let cx = w / 2.;
        let top = ((h - 420.) / 2.).max(12.);
        let running = self.snapshot.status == Status::Running;
        let label = match self.snapshot.status {
            Status::Running => "Pause",
            Status::Paused => "Resume",
            Status::Complete => "Start again",
            Status::Ready => "Start timer",
        };
        fill_rounded(pm, cx - 160., top + 225., 320., 44., 10., t.surface.color(1.));
        for i in 0..3 {
            fill_rounded(pm, cx - 160. + i as f32 * 110., top + 282., 100., 34., 9., t.surface.color(1.));
        }
        fill_rounded(pm, cx - 160., top + 330., 208., 44., 11., t.accent.color(1.));
        fill_rounded(pm, cx + 60., top + 330., 100., 44., 11., t.surface.color(1.));
        let total = 320.;
        fill_rounded(pm, cx - 160., top + 190., total, 4., 2., t.surface.color(1.));
        let fraction = (1. - self.snapshot.remaining as f32 / self.snapshot.duration.max(1) as f32).clamp(0., 1.);
        if fraction > 0. {
            fill_rounded(pm, cx - 160., top + 190., total * fraction, 4., 2., t.accent.color(1.));
        }
        if let Some(f) = &mut self.fonts {
            let mut center = |text: &str, y: f32, size: f32, weight: Weight, color| {
                let x = cx - f.measure(text, size, weight) / 2.;
                f.draw(pm, text, x, top + y, size, weight, color);
            };
            center("Timer", 8., 24., Weight::Medium, t.text.color(1.));
            center(
                match self.snapshot.status {
                    Status::Ready => "Make time for your next task",
                    Status::Running => "Counting down · continues when closed",
                    Status::Paused => "Take a breath · resume when ready",
                    Status::Complete => "Time’s up",
                },
                49.,
                13.,
                Weight::Regular,
                t.text_secondary.color(1.),
            );
            center(
                &format_remaining(if self.snapshot.status == Status::Ready { parse_duration(&self.input).unwrap_or(self.snapshot.remaining) } else { self.snapshot.remaining }),
                104.,
                (w / 9.).min(56.),
                Weight::Medium,
                t.text.color(1.),
            );
            center("Duration · seconds or MM:SS", 207., 11., Weight::Regular, t.text_secondary.color(1.));
            center(&self.input, 236., 20., Weight::Medium, if running { t.text_secondary.color(1.) } else { t.text.color(1.) });
            for (i, text) in ["5 min", "15 min", "25 min"].iter().enumerate() {
                let x = cx - 110. + i as f32 * 110. - f.measure(text, 13., Weight::Medium) / 2.;
                f.draw(pm, text, x, top + 291., 13., Weight::Medium, t.text.color(1.));
            }
            let x = cx - 56. - f.measure(label, 14., Weight::Medium) / 2.;
            f.draw(pm, label, x, top + 344., 14., Weight::Medium, t.accent.readable_on().color(1.));
            f.draw(pm, "Reset", cx + 110. - f.measure("Reset", 14., Weight::Medium) / 2., top + 344., 14., Weight::Medium, t.text.color(1.));
            let message = self.error.as_deref().unwrap_or("Enter to start/pause · R to reset");
            let text: String = message.chars().take(((w - 40.) / 7.).max(1.) as usize).collect();
            f.draw(pm, &text, (cx - f.measure(&text, 11., Weight::Regular) / 2.).max(8.), top + 393., 11., Weight::Regular, t.text_secondary.color(1.));
        }
    }
    fn pointer_motion(&mut self, _: f32, _: f32) -> Response {
        Response::NONE
    }
    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }
    fn pointer_button(&mut self, x: f32, y: f32, b: Button, p: bool) -> Response {
        if b != Button::Left || !p {
            return Response::NONE;
        }
        let cx = self.size.0 as f32 / 2.;
        let y = y - ((self.size.1 as f32 - 420.) / 2.).max(12.);
        if (225. ..269.).contains(&y) && (cx - 160. ..cx + 160.).contains(&x) {
            self.replace_input = true;
        } else if (282. ..316.).contains(&y) && (cx - 160. ..cx + 160.).contains(&x) {
            let i = ((x - (cx - 160.)) / 110.) as usize;
            if i < 3 {
                self.input = ["05:00", "15:00", "25:00"][i].into();
                self.replace_input = true;
            }
        } else if (330. ..374.).contains(&y) {
            if (cx - 160. ..cx + 48.).contains(&x) {
                self.primary();
            } else if (cx + 60. ..cx + 160.).contains(&x) {
                self.perform(TimerAction::Reset);
            }
        }
        Response::REDRAW
    }
    fn scroll(&mut self, _: f32, _: f32, _: f32) -> Response {
        Response::NONE
    }
    fn key(&mut self, e: &KeyEvent) -> Response {
        if e.ctrl && e.ch == Some('a') {
            self.replace_input = true;
            return Response::REDRAW;
        }
        match e.key {
            Key::Escape => return Response::act(Action::Close),
            Key::Enter => self.primary(),
            Key::Backspace => {
                if self.replace_input {
                    self.input.clear();
                    self.replace_input = false;
                } else {
                    self.input.pop();
                }
            }
            Key::Char => match e.text.as_str() {
                " " => self.primary(),
                "r" | "R" => self.perform(TimerAction::Reset),
                _ => {
                    if e.ctrl || e.alt {
                        return Response::NONE;
                    }
                    if e.text.chars().all(|c| c.is_ascii_digit() || c == ':') && self.input.len() + e.text.len() <= 12 {
                        if self.replace_input {
                            self.input.clear();
                            self.replace_input = false;
                        }
                        self.input.push_str(&e.text);
                    }
                }
            },
            _ => return Response::NONE,
        }
        Response::REDRAW
    }
    fn unmapped_key(&mut self, sym: u32, _: bool, _: bool, _: bool) -> Response {
        match sym {
            0xffbe => self.input = "05:00".into(),
            0xffbf => self.input = "15:00".into(),
            0xffc0 => self.input = "25:00".into(),
            _ => return Response::NONE,
        }
        Response::REDRAW
    }
    fn tick(&mut self) -> Response {
        match self.store.snapshot() {
            Ok(s) => {
                self.error = s.notification_error.clone();
                self.sync_snapshot(s, false);
            }
            Err(e) => self.error = Some(e),
        }
        Response::REDRAW
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    struct FakeClock(u64);
    impl Clock for FakeClock {
        fn now(&self) -> u64 {
            self.0
        }
    }
    struct FakeNotify(AtomicUsize);
    impl Notifier for FakeNotify {
        fn notify(&self, _: &str) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    static ID: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("timer-test-{}-{}", std::process::id(), ID.fetch_add(1, Ordering::SeqCst))))
        }
        fn store(&self) -> Store {
            Store::in_dir(&self.0)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn start(s: &Store) {
        s.apply_at(TimerAction::Start { seconds: 60, title: "Tea".into() }, &FakeClock(100)).unwrap();
    }
    #[test]
    fn readonly_does_not_create() {
        let t = Temp::new();
        assert_eq!(t.store().snapshot().unwrap().status, Status::Ready);
        assert!(!t.0.exists());
    }
    #[test]
    fn recovery_is_readonly_for_missing_idle_paused_and_claimed_state() {
        let t = Temp::new();
        let store = t.store();
        assert!(!store.recovery_needed(&FakeClock(100)).unwrap());
        store.recover().unwrap();
        store.spawn_worker().unwrap();
        assert!(!t.0.exists(), "idle login/UI must not create state or worker locks");
        start(&store);
        assert!(store.recovery_needed(&FakeClock(120)).unwrap());
        assert!(store.recovery_needed(&FakeClock(200)).unwrap(), "elapsed unclaimed timer needs delivery");
        store.apply_at(TimerAction::Pause, &FakeClock(120)).unwrap();
        let before = fs::read(t.0.join("timer.json")).unwrap();
        store.recover().unwrap();
        assert_eq!(fs::read(t.0.join("timer.json")).unwrap(), before);
        assert!(!t.0.join("worker.lock").exists());
        store.apply_at(TimerAction::Reset, &FakeClock(120)).unwrap();
        store.recover().unwrap();
        assert!(!t.0.join("worker.lock").exists());
        start(&store);
        store.notify_due(&FakeClock(200), &FakeNotify(AtomicUsize::new(0))).unwrap();
        store.recover().unwrap();
        assert!(!t.0.join("worker.lock").exists(), "already delivered state needs no resident helper");
        fs::write(t.0.join("timer.json"), b"broken state").unwrap();
        assert!(store.recover().unwrap_err().contains("Corrupt"));
        assert_eq!(fs::read(t.0.join("timer.json")).unwrap(), b"broken state");
    }

    #[test]
    fn lifecycle_restart_suspend() {
        let t = Temp::new();
        let s = t.store();
        start(&s);
        assert_eq!(t.store().snapshot_at(&FakeClock(120)).unwrap().remaining, 40);
        s.apply_at(TimerAction::Pause, &FakeClock(120)).unwrap();
        assert_eq!(s.snapshot_at(&FakeClock(900)).unwrap().remaining, 40);
        s.apply_at(TimerAction::Resume, &FakeClock(900)).unwrap();
        assert_eq!(s.snapshot_at(&FakeClock(940)).unwrap().status, Status::Complete);
        s.apply_at(TimerAction::Reset, &FakeClock(950)).unwrap();
        assert_eq!(s.snapshot().unwrap().remaining, 60);
    }
    #[test]
    fn one_completion_claim_under_conflict() {
        let t = Temp::new();
        let s = t.store();
        start(&s);
        let n = std::sync::Arc::new(FakeNotify(AtomicUsize::new(0)));
        let mut workers = vec![];
        for _ in 0..8 {
            let s = s.clone();
            let n = n.clone();
            workers.push(std::thread::spawn(move || s.notify_due(&FakeClock(200), n.as_ref()).unwrap()));
        }
        for w in workers {
            w.join().unwrap();
        }
        assert_eq!(n.0.load(Ordering::SeqCst), 1);
        assert!(!s.notify_due(&FakeClock(300), n.as_ref()).unwrap());
    }
    #[test]
    fn corruption_and_permissions_are_errors() {
        let t = Temp::new();
        let s = t.store();
        start(&s);
        fs::write(t.0.join("timer.json"), b"oops").unwrap();
        assert!(s.snapshot().unwrap_err().contains("Corrupt"));
        fs::set_permissions(&t.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(s.snapshot().unwrap_err().contains("private"));
    }
    #[test]
    fn symlink_state_is_rejected() {
        let t = Temp::new();
        let s = t.store();
        start(&s);
        fs::remove_file(t.0.join("timer.json")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", t.0.join("timer.json")).unwrap();
        assert!(s.snapshot().is_err());
    }
    #[test]
    fn malformed_duration() {
        for bad in ["0", "86401", "-3", "1:60", "1:2:3", "999999999999999999:00"] {
            assert!(parse_duration(bad).is_err());
        }
        assert_eq!(parse_duration("25:00").unwrap(), 1500);
    }
    #[test]
    fn invalid_transition_and_clock_rollback() {
        let t = Temp::new();
        let s = t.store();
        assert!(s.apply_at(TimerAction::Pause, &FakeClock(1)).is_err());
        start(&s);
        assert_eq!(s.snapshot_at(&FakeClock(1)).unwrap().remaining, 60);
    }
    #[test]
    fn duration_input_follows_shared_state_without_clobbering_drafts() {
        let t = Temp::new();
        let store = t.store();
        store.apply_at(TimerAction::Start { seconds: 1500, title: "Focus".into() }, &FakeClock(100)).unwrap();
        let mut app = TimerApp::new(store.clone());
        assert_eq!(app.input, "25:00", "reopening keeps the persisted duration");
        app.input = "12:34".into();
        app.replace_input = true;
        app.tick();
        assert_eq!(app.input, "12:34", "routine polling preserves the user's draft");
        assert!(app.replace_input);
        store.apply_at(TimerAction::Start { seconds: 3661, title: "Shared".into() }, &FakeClock(100)).unwrap();
        app.tick();
        assert_eq!(app.input, "61:01", "another app or widget can change the duration");
        assert_eq!(parse_duration(&app.input).unwrap(), 3661);
        assert!(!app.replace_input);
        app.input = "5".into();
        app.perform(TimerAction::Reset);
        assert_eq!(app.input, "61:01", "reset restores the shared duration");
        assert_eq!(app.snapshot.status, Status::Ready);
        assert_eq!(parse_duration(&TimerApp::duration_input(86400)).unwrap(), 86400);
    }
    #[test]
    fn native_controls_and_preview_are_visible() {
        let t = Temp::new();
        let mut app = TimerApp::new(t.store());
        app.resize(560, 460);
        app.pointer_button(390., 319., Button::Left, true);
        assert_eq!(app.input, "25:00");
        app.pointer_button(280., 265., Button::Left, true);
        app.key(&KeyEvent { key: Key::Char, text: "8".into(), ch: Some('8'), ctrl: false, shift: false, alt: false });
        assert_eq!(app.input, "8");
        let mut pm = Pixmap::new(560, 460).unwrap();
        app.draw(&mut pm);
        assert_ne!(pm.pixel(120, 361), pm.pixel(0, 0));
        if let Some(dir) = std::env::var_os("ECOSYSTEM_EVIDENCE_DIR") {
            fs::create_dir_all(&dir).unwrap();
            pm.save_png(PathBuf::from(dir).join("timer.png")).unwrap();
        }
    }
    #[test]
    fn ui_input_and_render_without_session() {
        let t = Temp::new();
        let mut app = TimerApp::new(t.store());
        app.fonts = None;
        let event = KeyEvent { key: Key::Backspace, text: String::new(), ch: None, ctrl: false, shift: false, alt: false };
        app.key(&event);
        assert_eq!(app.input, "05:0");
        app.unmapped_key(0xffc0, false, false, false);
        assert_eq!(app.input, "25:00");
        assert!(!t.0.exists());
        app.input = "0".into();
        app.primary();
        assert!(app.error.is_some());
        assert!(!t.0.exists());
    }
}
