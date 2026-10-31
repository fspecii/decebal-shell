//! Recursive copy/move on one bounded worker thread per transfer. The UI thread only
//! polls a latest-state snapshot and answers conflict prompts. All filesystem work is
//! relative to owned directory FDs; leaf symlinks are never followed. There is no
//! overwrite, no shell command and no Trash/permanent-delete fallback.
use serde::Serialize;
use std::{
    ffi::{CStr, CString, OsStr},
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};

/// Bounds for one request. Copying more requires a later streaming design.
pub const MAX_SOURCES: usize = 4096;
pub const MAX_ITEMS: u64 = 100_000;
pub const MAX_DEPTH: usize = 64;
const MAX_FAILURES: usize = 50;
const CHUNK: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Mode {
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Choice {
    Skip,
    KeepBoth,
    Stop,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub mode: Mode,
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
    /// Pre-answered conflict policy. `None` asks the UI for each conflict.
    pub policy: Option<Choice>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Phase {
    #[default]
    Preparing,
    Transferring,
    WaitingForChoice,
    Finished,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Progress {
    pub phase: Phase,
    pub items_done: u64,
    pub items_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current: Option<String>,
}

impl Progress {
    pub fn fraction(&self) -> Option<f64> {
        if self.bytes_total > 0 {
            Some((self.bytes_done as f64 / self.bytes_total as f64).min(1.0))
        } else if self.items_total > 0 {
            Some((self.items_done as f64 / self.items_total as f64).min(1.0))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Conflict {
    pub id: u64,
    #[serde(serialize_with = "path_text")]
    pub source: PathBuf,
    #[serde(serialize_with = "path_text")]
    pub destination: PathBuf,
    pub directory: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemFailure {
    #[serde(serialize_with = "path_text")]
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub mode: Mode,
    /// Top-level results created in the destination, in request order.
    #[serde(serialize_with = "paths_text")]
    pub created: Vec<PathBuf>,
    pub completed: usize,
    pub skipped: usize,
    pub failures: Vec<ItemFailure>,
    pub failure_count: usize,
    pub cancelled: bool,
    /// True when anything in the source or destination changed.
    pub changed: bool,
    pub notes: Vec<String>,
    pub message: String,
}

fn path_text<S: serde::Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}
fn paths_text<S: serde::Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(paths.iter().map(|path| path.to_string_lossy()))
}

#[derive(Default)]
struct State {
    progress: Progress,
    conflict: Option<Conflict>,
    answer: Option<(u64, Choice, bool)>,
    report: Option<Report>,
}

#[cfg(test)]
type Hook = Arc<dyn Fn(&str, &Path) + Send + Sync>;

/// Fixture and test controls. Normal builds compile none of them.
#[derive(Default)]
struct Options {
    #[cfg(any(test, feature = "fixture-delay"))]
    chunk_delay: Duration,
    /// Deterministic race points: before-publish, published, before-claim, read-link, staged-folder.
    #[cfg(test)]
    hook: Option<Hook>,
    /// Force the private `O_EXCL` staging file instead of `O_TMPFILE`.
    #[cfg(test)]
    named_files: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    cancel: AtomicBool,
    #[cfg_attr(not(any(test, feature = "fixture-delay")), allow(dead_code))]
    options: Options,
}

/// Latest-only snapshot. `report` is delivered exactly once.
pub struct Snapshot {
    pub progress: Progress,
    pub conflict: Option<Conflict>,
    pub report: Option<Report>,
}

pub struct Transfer {
    shared: Arc<Shared>,
}

impl Transfer {
    pub fn start(request: Request) -> io::Result<Self> {
        Self::spawn(request, Options::default())
    }

    /// Controlled per-chunk delay for cancellation fixtures. Never compiled into normal builds.
    #[cfg(any(test, feature = "fixture-delay"))]
    #[allow(clippy::needless_update)] // test builds add fields to Options
    pub fn start_throttled(request: Request, chunk_delay: Duration) -> io::Result<Self> {
        Self::spawn(request, Options { chunk_delay: chunk_delay.min(Duration::from_secs(5)), ..Options::default() })
    }

    #[cfg(test)]
    fn start_with(request: Request, options: Options) -> io::Result<Self> {
        Self::spawn(request, options)
    }

    fn spawn(request: Request, options: Options) -> io::Result<Self> {
        let shared = Arc::new(Shared { state: Mutex::new(State::default()), wake: Condvar::new(), cancel: AtomicBool::new(false), options });
        let worker = shared.clone();
        std::thread::Builder::new().name("decfiles-transfer".into()).spawn(move || {
            let report = Worker::new(&worker, request).run();
            let mut state = worker.lock();
            state.progress.phase = Phase::Finished;
            state.progress.current = None;
            state.conflict = None;
            state.report = Some(report);
        })?;
        Ok(Self { shared })
    }

    /// Request cancellation. Work already committed stays; the report says so.
    pub fn cancel(&self) {
        self.shared.request_cancel();
    }

    /// Thread-safe cancellation hook, e.g. for a GCancellable `cancelled` handler.
    pub fn canceller(&self) -> impl FnOnce() + Send + 'static {
        let shared = self.shared.clone();
        move || shared.request_cancel()
    }

    pub fn is_cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::SeqCst)
    }

    pub fn answer(&self, id: u64, choice: Choice, apply_to_all: bool) {
        let mut state = self.shared.lock();
        if state.conflict.as_ref().is_some_and(|conflict| conflict.id == id) {
            state.answer = Some((id, choice, apply_to_all));
            self.shared.wake.notify_all();
        }
    }

    pub fn poll(&self) -> Snapshot {
        let mut state = self.shared.lock();
        Snapshot { progress: state.progress.clone(), conflict: state.conflict.clone(), report: state.report.take() }
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        // Dropping the handle (window closed) must never leave a worker waiting for a choice.
        self.cancel();
    }
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _guard = self.lock();
        self.wake.notify_all();
    }
}

#[derive(Debug)]
enum ItemError {
    Exists,
    Cancelled,
    Failed(String),
}

fn describe(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => "permission denied".into(),
        Some(libc::EROFS) => "the location is read-only".into(),
        Some(libc::ENOSPC | libc::EDQUOT) => "not enough free space".into(),
        Some(libc::ENOENT) => "the item no longer exists".into(),
        Some(libc::ENAMETOOLONG) => "the name is too long".into(),
        Some(libc::ENOTEMPTY) => "the folder is not empty".into(),
        _ => error.to_string(),
    }
}

impl From<io::Error> for ItemError {
    fn from(error: io::Error) -> Self {
        if error.raw_os_error() == Some(libc::EEXIST) {
            Self::Exists
        } else {
            Self::Failed(describe(&error))
        }
    }
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

fn retry<T: Copy + PartialEq + From<i8>>(mut call: impl FnMut() -> T) -> T {
    loop {
        let value = call();
        if value != T::from(-1) || io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return value;
        }
    }
}

fn open_path_dir(path: &Path) -> io::Result<OwnedFd> {
    let path = c_name(path.as_os_str())?;
    // SAFETY: NUL-terminated path; the returned descriptor is owned exactly once.
    let fd = retry(|| unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) });
    if fd < 0 {
        return Err(last_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Open a child directory without following a leaf symlink.
fn open_child_dir(dir: RawFd, name: &CStr) -> io::Result<OwnedFd> {
    // SAFETY: valid directory FD and NUL-terminated leaf; result owned once.
    let fd = retry(|| unsafe { libc::openat(dir, name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) });
    if fd < 0 {
        return Err(last_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn stat_at(dir: RawFd, name: &CStr) -> io::Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the kernel fully initializes stat on success.
    if unsafe { libc::fstatat(dir, name.as_ptr(), stat.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0 {
        return Err(last_error());
    }
    Ok(unsafe { stat.assume_init() })
}

fn stat_fd(fd: RawFd) -> io::Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: as above.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(last_error());
    }
    Ok(unsafe { stat.assume_init() })
}

fn same_inode(a: &libc::stat, b: &libc::stat) -> bool {
    a.st_dev == b.st_dev && a.st_ino == b.st_ino
}

fn unchanged(before: &libc::stat, after: &libc::stat) -> bool {
    same_inode(before, after) && before.st_size == after.st_size && before.st_mtime == after.st_mtime && before.st_mtime_nsec == after.st_mtime_nsec
}

fn times(stat: &libc::stat) -> [libc::timespec; 2] {
    [libc::timespec { tv_sec: stat.st_atime, tv_nsec: stat.st_atime_nsec }, libc::timespec { tv_sec: stat.st_mtime, tv_nsec: stat.st_mtime_nsec }]
}

/// Permission bits only; set-user/group-ID bits are dropped for copied items.
fn copied_mode(stat: &libc::stat) -> libc::mode_t {
    stat.st_mode & 0o1777
}

fn kind(stat: &libc::stat) -> libc::mode_t {
    stat.st_mode & libc::S_IFMT
}

/// Names in one directory, excluding `.` and `..`. Bounded by the caller's item limit.
fn list_dir(fd: RawFd, limit: u64) -> io::Result<Vec<CString>> {
    // SAFETY: fdopendir takes ownership of a CLOEXEC duplicate; closedir releases it.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(last_error());
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        let error = last_error();
        unsafe { libc::close(duplicate) };
        return Err(error);
    }
    // Duplicates share one directory offset; an earlier listing must not hide entries.
    unsafe { libc::rewinddir(stream) };
    let mut names = Vec::new();
    let result = loop {
        unsafe { *libc::__errno_location() = 0 };
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let error = last_error();
            break if error.raw_os_error().unwrap_or(0) == 0 { Ok(()) } else { Err(error) };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if names.len() as u64 >= limit {
            break Err(io::Error::other(format!("a folder has more than {MAX_ITEMS} items")));
        }
        names.push(name.to_owned());
    };
    unsafe { libc::closedir(stream) };
    result.map(|()| names)
}

/// `name (copy).ext`, then `name (copy 2).ext`. Leading-dot names keep their dot.
pub fn keep_both_name(name: &OsStr, attempt: u32) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    let bytes = name.as_bytes();
    let split = bytes.iter().rposition(|byte| *byte == b'.').filter(|index| *index > 0 && *index + 1 < bytes.len());
    let (stem, extension) = split.map_or((bytes, &b""[..]), |index| (&bytes[..index], &bytes[index..]));
    let suffix = if attempt <= 1 { " (copy)".to_string() } else { format!(" (copy {attempt})") };
    let mut output = stem.to_vec();
    output.extend_from_slice(suffix.as_bytes());
    output.extend_from_slice(extension);
    std::ffi::OsString::from_vec(output)
}

fn valid_absolute(path: &Path) -> bool {
    path.is_absolute() && !path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir))
}

pub fn validate(request: &Request) -> Result<(), String> {
    if request.sources.is_empty() {
        return Err("There is nothing to paste.".into());
    }
    if request.sources.len() > MAX_SOURCES {
        return Err(format!("This preview transfers at most {MAX_SOURCES} selected items at once."));
    }
    if !valid_absolute(&request.destination) {
        return Err("The destination must be an absolute, resolved folder.".into());
    }
    for source in &request.sources {
        if !valid_absolute(source) || source.file_name().is_none() || source.parent().is_none() {
            return Err(format!("“{}” is not a local item inside a folder.", source.display()));
        }
    }
    Ok(())
}

/// A copy that exists only under a private name (or none) until it is published.
enum Staged {
    /// `O_TMPFILE`: invisible until `linkat` publishes it.
    Anonymous(OwnedFd),
    /// A private hidden file or folder in the destination, held open by FD.
    Named { name: CString, fd: OwnedFd, directory: bool },
    /// Symbolic link contents; `symlinkat` publishes it atomically.
    Link { target: CString, times: [libc::timespec; 2] },
}

/// What was copied, recorded so a cross-device original is removed only after its copy
/// is published and verified. Children are recorded only when copied completely.
struct Copied {
    name: CString,
    source: libc::stat,
    dest: (libc::dev_t, libc::ino_t),
    children: Option<Vec<Copied>>,
    complete: bool,
}

fn identity(stat: &libc::stat) -> (libc::dev_t, libc::ino_t) {
    (stat.st_dev, stat.st_ino)
}

/// Unpredictable hidden name for staging or claiming. Other processes of the same user
/// could still interfere deliberately; that is outside the stated contract.
fn private_name(purpose: &str) -> CString {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut random = [0u8; 8];
    // SAFETY: getrandom writes at most the buffer length.
    unsafe { libc::getrandom(random.as_mut_ptr().cast(), random.len(), 0) };
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    CString::new(format!(".decfiles-{purpose}-{}-{counter}-{:016x}", std::process::id(), u64::from_ne_bytes(random))).unwrap()
}

fn rename_noreplace(from_dir: RawFd, from: &CStr, to_dir: RawFd, to: &CStr) -> io::Result<()> {
    // SAFETY: valid directory FDs and NUL-terminated leaves; never replaces, never follows leaves.
    if unsafe { libc::renameat2(from_dir, from.as_ptr(), to_dir, to.as_ptr(), libc::RENAME_NOREPLACE) } != 0 {
        return Err(last_error());
    }
    Ok(())
}

/// Empty a private folder we created, through FDs only, never following symlinks.
fn clear_owned_dir(fd: RawFd, depth: usize) -> io::Result<()> {
    if depth > MAX_DEPTH + 1 {
        return Err(io::Error::other("staging folder is nested too deeply"));
    }
    // Copied read-only folders must become writable again to be emptied.
    unsafe { libc::fchmod(fd, 0o700) };
    for name in list_dir(fd, MAX_ITEMS)? {
        let stat = stat_at(fd, &name)?;
        let flags = if kind(&stat) == libc::S_IFDIR {
            let child = open_child_dir(fd, &name)?;
            clear_owned_dir(child.as_raw_fd(), depth + 1)?;
            libc::AT_REMOVEDIR
        } else {
            0
        };
        // SAFETY: owned FD and NUL-terminated leaf inside our private folder.
        if unsafe { libc::unlinkat(fd, name.as_ptr(), flags) } != 0 {
            return Err(last_error());
        }
    }
    Ok(())
}

struct Worker<'a> {
    shared: &'a Shared,
    mode: Mode,
    policy: Option<Choice>,
    failures: Vec<ItemFailure>,
    failure_count: usize,
    notes: Vec<String>,
    changed: bool,
    kept_originals: usize,
    next_conflict: u64,
    items_done: u64,
    bytes_done: u64,
    request: Request,
}

enum ScanError {
    Limit(String),
    Cancelled,
    Unreadable,
}

impl<'a> Worker<'a> {
    fn new(shared: &'a Shared, request: Request) -> Self {
        Self {
            shared,
            mode: request.mode,
            policy: request.policy,
            failures: Vec::new(),
            failure_count: 0,
            notes: Vec::new(),
            changed: false,
            kept_originals: 0,
            next_conflict: 0,
            items_done: 0,
            bytes_done: 0,
            request,
        }
    }

    fn fail(&mut self, path: &Path, message: impl Into<String>) {
        self.failure_count += 1;
        if self.failures.len() < MAX_FAILURES {
            self.failures.push(ItemFailure { path: path.to_path_buf(), message: message.into() });
        }
    }

    fn note(&mut self, note: String) {
        if self.notes.len() < MAX_FAILURES {
            self.notes.push(note);
        }
    }

    fn progress(&self, update: impl FnOnce(&mut Progress)) {
        let mut state = self.shared.lock();
        update(&mut state.progress);
    }

    fn advance(&mut self, items: u64, bytes: u64) {
        self.items_done += items;
        self.bytes_done += bytes;
        let (items_done, bytes_done) = (self.items_done, self.bytes_done);
        self.progress(|progress| {
            progress.items_done = items_done;
            progress.bytes_done = bytes_done;
        });
    }

    fn check_cancel(&self) -> Result<(), ItemError> {
        if self.shared.cancelled() {
            Err(ItemError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Returns (items, bytes). Unreadable entries are reported precisely when transferred.
    fn scan(&self, path: &Path, depth: usize, budget: &mut u64) -> Result<(u64, u64), ScanError> {
        if self.shared.cancelled() {
            return Err(ScanError::Cancelled);
        }
        if depth > MAX_DEPTH {
            return Err(ScanError::Limit(format!("folders nested deeper than {MAX_DEPTH} levels are not supported")));
        }
        if *budget == 0 {
            return Err(ScanError::Limit(format!("this preview transfers at most {MAX_ITEMS} items at once")));
        }
        *budget -= 1;
        let metadata = std::fs::symlink_metadata(path).map_err(|_| ScanError::Unreadable)?;
        let mut totals = (1, if metadata.is_file() { metadata.len() } else { 0 });
        if metadata.is_dir() {
            if let Ok(children) = std::fs::read_dir(path) {
                for child in children.flatten() {
                    let (items, bytes) = match self.scan(&child.path(), depth + 1, budget) {
                        Err(ScanError::Unreadable) => (1, 0),
                        other => other?,
                    };
                    totals.0 += items;
                    totals.1 += bytes;
                }
            }
        }
        Ok(totals)
    }

    fn ask(&mut self, source: &Path, destination: &Path, directory: bool) -> Choice {
        if let Some(choice) = self.policy {
            return choice;
        }
        self.next_conflict += 1;
        let id = self.next_conflict;
        let mut state = self.shared.lock();
        state.answer = None;
        state.conflict = Some(Conflict { id, source: source.to_path_buf(), destination: destination.to_path_buf(), directory });
        state.progress.phase = Phase::WaitingForChoice;
        let choice = loop {
            if self.shared.cancelled() {
                break (Choice::Stop, false);
            }
            if let Some((answered, choice, all)) = state.answer.take() {
                if answered == id {
                    break (choice, all);
                }
            }
            state = self.shared.wake.wait_timeout(state, Duration::from_millis(250)).unwrap_or_else(|error| error.into_inner()).0;
        };
        state.conflict = None;
        state.progress.phase = Phase::Transferring;
        drop(state);
        if choice.1 && choice.0 != Choice::Stop {
            self.policy = Some(choice.0);
        }
        choice.0
    }

    fn run(mut self) -> Report {
        let mut report = Report {
            mode: self.mode,
            created: Vec::new(),
            completed: 0,
            skipped: 0,
            failures: Vec::new(),
            failure_count: 0,
            cancelled: false,
            changed: false,
            notes: Vec::new(),
            message: String::new(),
        };
        if let Err(message) = validate(&self.request) {
            report.failure_count = 1;
            report.message = message;
            return report;
        }
        let destination = self.request.destination.clone();
        let destination_fd = match open_path_dir(&destination) {
            Ok(fd) => fd,
            Err(error) => {
                report.failure_count = 1;
                report.message = format!("Could not open the destination folder: {}.", describe(&error));
                return report;
            }
        };
        let destination_real = std::fs::canonicalize(&destination).unwrap_or_else(|_| destination.clone());
        let sources = std::mem::take(&mut self.request.sources);
        // Copies need totals up front. Moves are usually one atomic rename; a
        // cross-device fallback counts its own subtree when it happens.
        let mut totals = (sources.len() as u64, 0);
        if self.mode == Mode::Copy {
            let mut budget = MAX_ITEMS;
            totals = (0, 0);
            for source in &sources {
                match self.scan(source, 0, &mut budget) {
                    Ok((items, bytes)) => {
                        totals.0 += items;
                        totals.1 += bytes;
                    }
                    Err(ScanError::Cancelled) => break,
                    Err(ScanError::Limit(message)) => {
                        report.failure_count = 1;
                        report.message = format!("Nothing was copied: {message}.");
                        return report;
                    }
                    Err(ScanError::Unreadable) => totals.0 += 1,
                }
            }
        }
        self.progress(|progress| {
            progress.items_total = totals.0;
            progress.bytes_total = totals.1;
            progress.phase = Phase::Transferring;
        });
        let mut cancelled = self.shared.cancelled();
        for source in &sources {
            if cancelled {
                break;
            }
            match self.top_level(source, &destination, &destination_fd, &destination_real) {
                Ok(Some(created)) => {
                    report.completed += 1;
                    report.created.push(created);
                }
                Ok(None) => report.skipped += 1,
                Err(ItemError::Cancelled) => cancelled = true,
                Err(ItemError::Exists) => self.fail(source, "an item with that name appeared in the destination"),
                Err(ItemError::Failed(message)) => self.fail(source, message),
            }
        }
        report.cancelled = cancelled;
        if self.kept_originals > 1 {
            self.notes.push(format!("{} originals were kept in total.", self.kept_originals));
        }
        report.failures = std::mem::take(&mut self.failures);
        report.failure_count = self.failure_count;
        report.notes = std::mem::take(&mut self.notes);
        report.changed = self.changed;
        report.message = summary(&report, &destination);
        report
    }

    /// Transfer one selected item. Returns the published destination path, or None if skipped.
    fn top_level(&mut self, source: &Path, destination: &Path, destination_fd: &OwnedFd, destination_real: &Path) -> Result<Option<PathBuf>, ItemError> {
        self.check_cancel()?;
        let parent = source.parent().unwrap();
        let name = source.file_name().unwrap();
        let name_c = c_name(name)?;
        let parent_fd = open_path_dir(parent).map_err(|error| ItemError::Failed(format!("could not open its folder: {}", describe(&error))))?;
        let stat = stat_at(parent_fd.as_raw_fd(), &name_c)?;
        let directory = kind(&stat) == libc::S_IFDIR;
        let same_folder = same_inode(&stat_fd(parent_fd.as_raw_fd())?, &stat_fd(destination_fd.as_raw_fd())?);
        if same_folder && self.mode == Mode::Move {
            self.note(format!("“{}” is already in this folder.", name.to_string_lossy()));
            return Ok(None);
        }
        if directory {
            // A folder cannot be copied or moved into itself or one of its own folders.
            if let Ok(real) = std::fs::canonicalize(source) {
                if destination_real.starts_with(&real) {
                    return Err(ItemError::Failed("a folder cannot be placed inside itself".into()));
                }
            }
        }
        self.progress(|progress| progress.current = Some(name.to_string_lossy().into_owned()));
        let mut staged: Option<(Staged, Copied)> = None;
        let mut target = name.to_os_string();
        let mut attempt = 0;
        // Copying into the source folder is a familiar duplicate: always keep both.
        let mut keep_both = same_folder;
        let mut asked = false;
        let result = loop {
            if keep_both {
                attempt += 1;
                if attempt > 1000 {
                    break Err(ItemError::Failed("no free “copy” name was found".into()));
                }
                target = keep_both_name(name, attempt);
            }
            let target_c = c_name(&target)?;
            // Advisory early prompt so a large copy is not wasted. The atomic publication
            // below remains the actual no-overwrite guarantee.
            let outcome = if stat_at(destination_fd.as_raw_fd(), &target_c).is_ok() {
                Err(ItemError::Exists)
            } else {
                self.place(&mut staged, &parent_fd, &name_c, source, destination_fd, &target_c, &destination.join(&target))
            };
            match outcome {
                Ok(()) => break Ok(Some(destination.join(&target))),
                Err(ItemError::Exists) if keep_both => continue,
                Err(ItemError::Exists) if !asked => {
                    asked = true;
                    match self.ask(source, &destination.join(&target), directory) {
                        Choice::Skip => break Ok(None),
                        Choice::KeepBoth => keep_both = true,
                        Choice::Stop => break Err(ItemError::Cancelled),
                    }
                }
                Err(error) => break Err(error),
            }
        };
        // Anything still staged was never published: remove only our own private copy.
        if let Some((staged, _)) = staged {
            self.discard(staged, destination_fd);
        }
        result
    }

    /// Move by atomic rename, or stage once (reused across conflict retries) and publish.
    #[allow(clippy::too_many_arguments)]
    fn place(&mut self, staged: &mut Option<(Staged, Copied)>, parent: &OwnedFd, name: &CStr, source: &Path, destination_dir: &OwnedFd, target: &CStr, destination: &Path) -> Result<(), ItemError> {
        if self.mode == Mode::Move && staged.is_none() {
            match rename_noreplace(parent.as_raw_fd(), name, destination_dir.as_raw_fd(), target) {
                Ok(()) => {
                    self.changed = true;
                    self.advance(1, 0);
                    return Ok(());
                }
                Err(error) => match error.raw_os_error() {
                    Some(libc::EXDEV) => {}
                    Some(libc::EINVAL) => return Err(ItemError::Failed("a folder cannot be placed inside itself, or safe moving is unsupported here".into())),
                    Some(libc::ENOSYS | libc::EOPNOTSUPP) => return Err(ItemError::Failed("safe no-overwrite moving is not supported here".into())),
                    _ => return Err(error.into()),
                },
            }
            // Different filesystem: verified copy, then remove only claimed, unchanged originals.
            let mut budget = MAX_ITEMS;
            match self.scan(source, 0, &mut budget) {
                Ok((items, bytes)) => self.progress(|progress| {
                    progress.items_total += items.saturating_sub(1);
                    progress.bytes_total += bytes;
                }),
                Err(ScanError::Cancelled) => return Err(ItemError::Cancelled),
                Err(ScanError::Limit(message)) => return Err(ItemError::Failed(format!("not moved across filesystems: {message}"))),
                Err(ScanError::Unreadable) => {}
            }
        }
        if staged.is_none() {
            *staged = Some(self.stage(parent, name, source, destination_dir, 0)?);
        }
        self.hook("before-publish", destination);
        let published = self.publish(&staged.as_ref().unwrap().0, destination_dir, target)?;
        let (_, mut copied) = staged.take().unwrap();
        copied.dest = published;
        self.changed = true;
        self.hook("published", destination);
        if self.mode == Mode::Move {
            self.remove_original(parent, destination_dir, target, &copied, source);
        }
        Ok(())
    }

    /// Copy into a private, unpublished form. Cancellation removes our own staging.
    fn stage(&mut self, parent: &OwnedFd, name: &CStr, source: &Path, destination_dir: &OwnedFd, depth: usize) -> Result<(Staged, Copied), ItemError> {
        self.check_cancel()?;
        if depth > MAX_DEPTH {
            return Err(ItemError::Failed(format!("nested deeper than {MAX_DEPTH} levels")));
        }
        let stat = stat_at(parent.as_raw_fd(), name)?;
        let copied = |stat: libc::stat| Copied { name: name.to_owned(), source: stat, dest: (0, 0), children: None, complete: true };
        match kind(&stat) {
            libc::S_IFLNK => {
                let (target, stat) = self.read_link(parent, name, source)?;
                self.advance(1, 0);
                Ok((Staged::Link { target, times: times(&stat) }, copied(stat)))
            }
            libc::S_IFREG => Ok((self.stage_file(parent, name, &stat, destination_dir)?, copied(stat))),
            libc::S_IFDIR => self.stage_dir(parent, name, &stat, source, destination_dir, depth),
            _ => Err(ItemError::Failed("special files such as devices, sockets and pipes are not copied".into())),
        }
    }

    /// Bounded identity check: the link must be the same inode, of the same length,
    /// before and after reading it.
    fn read_link(&mut self, parent: &OwnedFd, name: &CStr, source: &Path) -> Result<(CString, libc::stat), ItemError> {
        for _ in 0..3 {
            let before = stat_at(parent.as_raw_fd(), name)?;
            if kind(&before) != libc::S_IFLNK {
                return Err(ItemError::Failed("the item changed while it was being read".into()));
            }
            let mut buffer = vec![0u8; libc::PATH_MAX as usize + 1];
            // SAFETY: buffer bounds are passed; readlinkat does not NUL-terminate.
            let length = unsafe { libc::readlinkat(parent.as_raw_fd(), name.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len()) };
            if length < 0 {
                return Err(last_error().into());
            }
            buffer.truncate(length as usize);
            self.hook("read-link", source);
            let after = stat_at(parent.as_raw_fd(), name)?;
            if same_inode(&before, &after) && kind(&after) == libc::S_IFLNK && after.st_size as usize == buffer.len() && buffer.len() <= libc::PATH_MAX as usize {
                let target = CString::new(buffer).map_err(|_| ItemError::Failed("invalid symbolic link".into()))?;
                return Ok((target, after));
            }
        }
        Err(ItemError::Failed("the link kept changing while it was being read".into()))
    }

    fn stage_file(&mut self, parent: &OwnedFd, name: &CStr, before: &libc::stat, destination_dir: &OwnedFd) -> Result<Staged, ItemError> {
        // O_NONBLOCK and O_NOFOLLOW: a leaf swapped for a FIFO or symlink cannot block or redirect us.
        // SAFETY: owned FDs; results owned exactly once.
        let raw = retry(|| unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC) });
        if raw < 0 {
            return Err(last_error().into());
        }
        let input = unsafe { OwnedFd::from_raw_fd(raw) };
        let opened = stat_fd(input.as_raw_fd())?;
        if kind(&opened) != libc::S_IFREG || !same_inode(&opened, before) {
            return Err(ItemError::Failed("the item changed while it was being opened".into()));
        }
        let staged = self.create_staging_file(destination_dir)?;
        let output = match &staged {
            Staged::Anonymous(fd) | Staged::Named { fd, .. } => fd.as_raw_fd(),
            Staged::Link { .. } => unreachable!(),
        };
        let result = self.copy_bytes(&input, output, before).and_then(|copied| {
            let after = stat_fd(input.as_raw_fd())?;
            if copied != after.st_size as u64 || !unchanged(before, &after) {
                return Err(ItemError::Failed("the file changed while it was being copied; no copy was kept".into()));
            }
            let times = times(before);
            // SAFETY: owned output FD.
            if unsafe { libc::fchmod(output, copied_mode(before)) } != 0 || unsafe { libc::futimens(output, times.as_ptr()) } != 0 {
                return Err(last_error().into());
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                self.advance(1, 0);
                Ok(staged)
            }
            Err(error) => {
                self.discard(staged, destination_dir);
                Err(error)
            }
        }
    }

    /// An anonymous file when the filesystem supports it, else a private `O_EXCL` file.
    fn create_staging_file(&mut self, destination_dir: &OwnedFd) -> Result<Staged, ItemError> {
        #[cfg(test)]
        let anonymous_allowed = !self.shared.options.named_files;
        #[cfg(not(test))]
        let anonymous_allowed = true;
        if anonymous_allowed {
            // SAFETY: owned FD; result owned exactly once.
            let fd = retry(|| unsafe { libc::openat(destination_dir.as_raw_fd(), c".".as_ptr(), libc::O_TMPFILE | libc::O_WRONLY | libc::O_CLOEXEC, 0o600) });
            if fd >= 0 {
                return Ok(Staged::Anonymous(unsafe { OwnedFd::from_raw_fd(fd) }));
            }
            if !matches!(last_error().raw_os_error(), Some(libc::EOPNOTSUPP | libc::EISDIR | libc::EINVAL)) {
                return Err(last_error().into());
            }
        }
        let name = private_name("copy");
        // SAFETY: O_EXCL|O_NOFOLLOW creates a new file we own or fails.
        let fd = retry(|| unsafe { libc::openat(destination_dir.as_raw_fd(), name.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) });
        if fd < 0 {
            return Err(last_error().into());
        }
        Ok(Staged::Named { name, fd: unsafe { OwnedFd::from_raw_fd(fd) }, directory: false })
    }

    /// Populate a private folder through its FD; children are published inside it.
    fn stage_dir(&mut self, parent: &OwnedFd, name: &CStr, stat: &libc::stat, source: &Path, destination_dir: &OwnedFd, depth: usize) -> Result<(Staged, Copied), ItemError> {
        let source_dir = open_child_dir(parent.as_raw_fd(), name)?;
        if !same_inode(&stat_fd(source_dir.as_raw_fd())?, stat) {
            return Err(ItemError::Failed("the folder changed while it was being opened".into()));
        }
        let children = list_dir(source_dir.as_raw_fd(), MAX_ITEMS).map_err(|error| ItemError::Failed(describe(&error)))?;
        let private = private_name("folder");
        // SAFETY: NUL-terminated leaf; mkdirat fails with EEXIST and never merges.
        if unsafe { libc::mkdirat(destination_dir.as_raw_fd(), private.as_ptr(), 0o700) } != 0 {
            return Err(last_error().into());
        }
        let created = open_child_dir(destination_dir.as_raw_fd(), &private)?;
        let work = created.try_clone()?;
        let owned = stat_fd(created.as_raw_fd())?;
        // SAFETY: geteuid has no preconditions.
        let ours = owned.st_uid == unsafe { libc::geteuid() } && owned.st_mode & 0o077 == 0 && list_dir(created.as_raw_fd(), 1).is_ok_and(|names| names.is_empty());
        let staged = Staged::Named { name: private, fd: created, directory: true };
        if !ours {
            // Not demonstrably our fresh private folder: leave it alone, publish nothing.
            drop(staged);
            return Err(ItemError::Failed("the private staging folder could not be verified".into()));
        }
        #[cfg(test)]
        self.hook("staged-folder", &destination_dir_path(destination_dir, &staged));
        self.advance(1, 0);
        let created = work;
        let mut copied_children = Vec::new();
        let mut complete = true;
        for child in children {
            let child_source = source.join(OsStr::from_bytes(child.to_bytes()));
            let outcome = self.stage(&source_dir, &child, &child_source, &created, depth + 1).and_then(|(child_staged, mut record)| match self.publish(&child_staged, &created, &child) {
                Ok(published) => {
                    record.dest = published;
                    Ok(record)
                }
                Err(error) => {
                    self.discard(child_staged, &created);
                    Err(error)
                }
            });
            match outcome {
                Ok(record) => copied_children.push(record),
                Err(ItemError::Cancelled) => {
                    self.discard(staged, destination_dir);
                    return Err(ItemError::Cancelled);
                }
                Err(ItemError::Exists) => {
                    complete = false;
                    self.fail(&child_source, "an item with that name appeared in the private copy");
                }
                Err(ItemError::Failed(message)) => {
                    complete = false;
                    self.fail(&child_source, message);
                }
            }
        }
        // Apply restrictive modes last so read-only folders can be populated.
        let times = times(stat);
        unsafe {
            libc::fchmod(created.as_raw_fd(), copied_mode(stat));
            libc::futimens(created.as_raw_fd(), times.as_ptr());
        }
        Ok((staged, Copied { name: name.to_owned(), source: *stat, dest: (0, 0), children: Some(copied_children), complete }))
    }

    /// Atomically publish under `target`; never replaces. Exists keeps the staging for a retry.
    /// The published name must then resolve to the staged inode.
    fn publish(&mut self, staged: &Staged, destination_dir: &OwnedFd, target: &CStr) -> Result<(libc::dev_t, libc::ino_t), ItemError> {
        let dir = destination_dir.as_raw_fd();
        let expected = match staged {
            Staged::Anonymous(fd) => {
                let proc_path = CString::new(format!("/proc/self/fd/{}", fd.as_raw_fd())).unwrap();
                // SAFETY: linkat with EEXIST is the atomic no-overwrite commit.
                if unsafe { libc::linkat(libc::AT_FDCWD, proc_path.as_ptr(), dir, target.as_ptr(), libc::AT_SYMLINK_FOLLOW) } != 0 {
                    return Err(last_error().into());
                }
                identity(&stat_fd(fd.as_raw_fd())?)
            }
            Staged::Named { name, fd, .. } => {
                rename_noreplace(dir, name, dir, target)?;
                identity(&stat_fd(fd.as_raw_fd())?)
            }
            Staged::Link { target: link, times } => {
                // SAFETY: NUL-terminated strings; symlinkat fails with EEXIST and never replaces.
                if unsafe { libc::symlinkat(link.as_ptr(), dir, target.as_ptr()) } != 0 {
                    return Err(last_error().into());
                }
                // Best effort: some filesystems do not store symlink timestamps.
                unsafe { libc::utimensat(dir, target.as_ptr(), times.as_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
                let created = stat_at(dir, target)?;
                if kind(&created) != libc::S_IFLNK {
                    return Err(ItemError::Failed("the new link was replaced immediately after creation".into()));
                }
                identity(&created)
            }
        };
        match stat_at(dir, target) {
            Ok(now) if identity(&now) == expected => Ok(expected),
            _ => {
                if let Staged::Named { name, .. } = staged {
                    // Our private name was swapped before publication. Put what we published
                    // back under the private name; never claim success.
                    let _ = rename_noreplace(dir, target, dir, name);
                }
                Err(ItemError::Failed("the private copy was replaced before it could be published; nothing was published".into()))
            }
        }
    }

    /// Remove only our own unpublished staging, identified by FD.
    fn discard(&mut self, staged: Staged, destination_dir: &OwnedFd) {
        let Staged::Named { name, fd, directory } = staged else {
            return; // anonymous files vanish with their FD; links were never created
        };
        if directory && clear_owned_dir(fd.as_raw_fd(), 0).is_err() {
            self.note(format!("An incomplete private copy could not be fully removed: “{}”.", OsStr::from_bytes(name.to_bytes()).to_string_lossy()));
            return;
        }
        let ours = matches!((stat_at(destination_dir.as_raw_fd(), &name), stat_fd(fd.as_raw_fd())), (Ok(now), Ok(own)) if same_inode(&now, &own));
        let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: the private name still resolves to our own staged inode.
        if !ours || unsafe { libc::unlinkat(destination_dir.as_raw_fd(), name.as_ptr(), flags) } != 0 {
            self.note(format!("An incomplete private copy could not be removed: “{}”.", OsStr::from_bytes(name.to_bytes()).to_string_lossy()));
        }
    }

    fn keep(&mut self, source: &Path, reason: &str) -> bool {
        self.kept_originals += 1;
        if self.kept_originals == 1 {
            self.notes.push(format!("The copy is complete, but the original “{}” was kept: {reason}.", source.display()));
        }
        false
    }

    /// Cross-device move only. Remove an original after its copy is published and still
    /// verified at the destination: claim it atomically under a private name, check it is
    /// the same unchanged inode that was copied, then remove the claimed item. Folders are
    /// removed only when every child was removed and they are empty. Returns true if removed.
    fn remove_original(&mut self, parent: &OwnedFd, destination_dir: &OwnedFd, destination_name: &CStr, item: &Copied, source: &Path) -> bool {
        match stat_at(destination_dir.as_raw_fd(), destination_name) {
            Ok(now) if identity(&now) == item.dest => {}
            _ => return self.keep(source, "its copy at the destination changed or disappeared"),
        }
        let directory = item.children.is_some();
        if let Some(children) = &item.children {
            let (Ok(source_dir), Ok(destination_child)) = (open_child_dir(parent.as_raw_fd(), &item.name), open_child_dir(destination_dir.as_raw_fd(), destination_name)) else {
                return self.keep(source, "it could not be opened again");
            };
            if !stat_fd(source_dir.as_raw_fd()).is_ok_and(|now| same_inode(&now, &item.source)) || !stat_fd(destination_child.as_raw_fd()).is_ok_and(|now| identity(&now) == item.dest) {
                return self.keep(source, "it changed during the move");
            }
            let mut all = true;
            for child in children {
                let child_source = source.join(OsStr::from_bytes(child.name.to_bytes()));
                all &= self.remove_original(&source_dir, &destination_child, &child.name, child, &child_source);
            }
            if !item.complete {
                // Children that were copied have been moved; the folder keeps the rest.
                self.note(format!("The original folder “{}” was kept because not every item in it was moved.", source.display()));
                return false;
            }
            if !all {
                return false; // the first kept child was already reported
            }
        }
        self.hook("before-claim", source);
        let claim = private_name("claim");
        if let Err(error) = rename_noreplace(parent.as_raw_fd(), &item.name, parent.as_raw_fd(), &claim) {
            return self.keep(source, &describe(&error));
        }
        let claimed = stat_at(parent.as_raw_fd(), &claim);
        let same = claimed.as_ref().is_ok_and(|now| if directory { same_inode(&item.source, now) } else { unchanged(&item.source, now) });
        let removed = same && {
            let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
            // SAFETY: the private claim name holds the verified original.
            unsafe { libc::unlinkat(parent.as_raw_fd(), claim.as_ptr(), flags) == 0 }
        };
        if removed {
            return true;
        }
        let reason = if same { describe(&last_error()) } else { "it changed during the move".to_string() };
        // Restore whatever we claimed to its own name, never replacing anything.
        if rename_noreplace(parent.as_raw_fd(), &claim, parent.as_raw_fd(), &item.name).is_err() {
            self.note(format!("“{}” could not be restored and was left as “{}” in the same folder.", source.display(), OsStr::from_bytes(claim.to_bytes()).to_string_lossy()));
        }
        self.keep(source, &reason)
    }

    fn copy_bytes(&mut self, input: &OwnedFd, output: RawFd, before: &libc::stat) -> Result<u64, ItemError> {
        let mut buffer = vec![0u8; CHUNK];
        let mut copied = 0u64;
        loop {
            self.check_cancel()?;
            #[cfg(any(test, feature = "fixture-delay"))]
            if !self.shared.options.chunk_delay.is_zero() {
                std::thread::sleep(self.shared.options.chunk_delay);
                self.check_cancel()?;
            }
            // SAFETY: buffer bounds are passed to read/write.
            let read = retry(|| unsafe { libc::read(input.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) });
            if read < 0 {
                return Err(last_error().into());
            }
            if read == 0 {
                return Ok(copied);
            }
            let mut written = 0;
            while written < read as usize {
                let count = retry(|| unsafe { libc::write(output, buffer[written..].as_ptr().cast(), read as usize - written) });
                if count < 0 {
                    return Err(last_error().into());
                }
                written += count as usize;
            }
            copied += read as u64;
            if copied > before.st_size as u64 {
                return Err(ItemError::Failed("the file grew while it was being copied; no copy was kept".into()));
            }
            self.advance(0, read as u64);
        }
    }

    #[cfg(test)]
    fn hook(&self, point: &str, path: &Path) {
        if let Some(hook) = &self.shared.options.hook {
            hook(point, path);
        }
    }

    #[cfg(not(test))]
    fn hook(&self, _: &str, _: &Path) {}
}

/// Test hooks see the private staging folder's current path.
#[cfg(test)]
fn destination_dir_path(destination_dir: &OwnedFd, staged: &Staged) -> PathBuf {
    let Staged::Named { name, .. } = staged else {
        return PathBuf::new();
    };
    std::fs::read_link(format!("/proc/self/fd/{}", destination_dir.as_raw_fd())).unwrap_or_default().join(OsStr::from_bytes(name.to_bytes()))
}

fn summary(report: &Report, destination: &Path) -> String {
    let (verb, past) = match report.mode {
        Mode::Copy => ("copy", "Copied"),
        Mode::Move => ("move", "Moved"),
    };
    let plural = |count: usize| if count == 1 { "item" } else { "items" };
    let folder = destination.file_name().map_or_else(|| destination.display().to_string(), |name| name.to_string_lossy().into_owned());
    let mut message = if !report.cancelled {
        format!("{past} {} {} to “{folder}”.", report.completed, plural(report.completed))
    } else if report.completed > 0 {
        format!("Cancelled. {past} {} {} to “{folder}” before cancelling; they remain there.", report.completed, plural(report.completed))
    } else if report.changed {
        format!("Cancelled. Some changes were already made in “{folder}”; check it before retrying.")
    } else {
        "Cancelled. Nothing was changed.".to_string()
    };
    if report.skipped > 0 {
        message.push_str(&format!(" Skipped {}.", report.skipped));
    }
    if report.failure_count > 0 {
        let first = &report.failures[0];
        let name = first.path.file_name().map_or_else(|| first.path.display().to_string(), |name| name.to_string_lossy().into_owned());
        message.push_str(&format!(" Could not {verb} {} {}: “{name}”: {}.", report.failure_count, plural(report.failure_count), first.message));
    }
    for note in &report.notes {
        message.push(' ');
        message.push_str(note);
    }
    message
}

/// Clipboard formats shared with GNOME Files: `x-special/gnome-copied-files`
/// ("copy"/"cut" then one file URI per line) and RFC 2483 `text/uri-list`.
pub mod clipboard {
    use super::Mode;
    use std::path::PathBuf;

    pub const GNOME: &str = "x-special/gnome-copied-files";
    pub const URI_LIST: &str = "text/uri-list";
    /// Untrusted clipboard payloads larger than this are ignored.
    pub const MAX_BYTES: usize = 1024 * 1024;

    pub fn uris(paths: &[PathBuf]) -> Vec<String> {
        paths.iter().filter_map(|path| glib::filename_to_uri(path, None).ok().map(|uri| uri.to_string())).collect()
    }

    pub fn encode_gnome(mode: Mode, paths: &[PathBuf]) -> String {
        let mut text = String::from(if mode == Mode::Move { "cut" } else { "copy" });
        for uri in uris(paths) {
            text.push('\n');
            text.push_str(&uri);
        }
        text
    }

    pub fn encode_uri_list(paths: &[PathBuf]) -> String {
        uris(paths).into_iter().map(|uri| uri + "\r\n").collect()
    }

    fn local(uri: &str) -> Option<PathBuf> {
        let (path, host) = glib::filename_from_uri(uri).ok()?;
        if host.as_deref().is_some_and(|host| !host.is_empty() && host != "localhost") {
            return None;
        }
        Some(path).filter(|path| path.is_absolute())
    }

    /// Returns None unless every listed item is a local file URI.
    pub fn decode_gnome(text: &str) -> Option<(Mode, Vec<PathBuf>)> {
        let mut lines = text.split('\n').map(|line| line.trim_end_matches('\r')).filter(|line| !line.is_empty());
        let mode = match lines.next()? {
            "copy" => Mode::Copy,
            "cut" => Mode::Move,
            _ => return None,
        };
        let paths: Option<Vec<_>> = lines.take(super::MAX_SOURCES + 1).map(local).collect();
        paths.filter(|paths| !paths.is_empty() && paths.len() <= super::MAX_SOURCES).map(|paths| (mode, paths))
    }

    /// GDK on X11 can report a stale GNOME type for an owner serving a plain URI list.
    /// Without a "copy"/"cut" header the payload is treated as a URI list, which only copies.
    pub fn decode(mime: &str, text: &str) -> Option<(Mode, Vec<PathBuf>)> {
        let gnome = if mime == GNOME { decode_gnome(text) } else { None };
        gnome.or_else(|| decode_uri_list(text).map(|paths| (Mode::Copy, paths)))
    }

    pub fn decode_uri_list(text: &str) -> Option<Vec<PathBuf>> {
        let paths: Option<Vec<_>> = text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).take(super::MAX_SOURCES + 1).map(local).collect();
        paths.filter(|paths| !paths.is_empty() && paths.len() <= super::MAX_SOURCES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ffi::OsString,
        os::unix::{
            ffi::OsStringExt,
            fs::{symlink, MetadataExt, PermissionsExt},
        },
        sync::Arc,
        time::{Instant, SystemTime},
    };

    fn fixture(name: &str) -> PathBuf {
        let root = PathBuf::from("/var/tmp").join(format!("decfiles-transfer-{name}-{}", std::process::id()));
        if root.exists() {
            restore_modes(&root);
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn restore_modes(root: &Path) {
        if let Ok(metadata) = std::fs::symlink_metadata(root) {
            if metadata.is_dir() {
                let _ = std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700));
                for child in std::fs::read_dir(root).into_iter().flatten().flatten() {
                    restore_modes(&child.path());
                }
            }
        }
    }

    fn cleanup(root: &Path) {
        restore_modes(root);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn request(mode: Mode, sources: &[PathBuf], destination: &Path, policy: Option<Choice>) -> Request {
        Request { mode, sources: sources.to_vec(), destination: destination.to_path_buf(), policy }
    }

    /// Poll like the UI does, answering conflicts through the public interface.
    fn wait(transfer: &Transfer, mut answer: impl FnMut(&Transfer, &Conflict, &Progress)) -> Report {
        let until = Instant::now() + Duration::from_secs(30);
        let mut answered = 0;
        loop {
            let snapshot = transfer.poll();
            if let Some(report) = snapshot.report {
                return report;
            }
            if let Some(conflict) = snapshot.conflict {
                if conflict.id != answered {
                    answered = conflict.id;
                    answer(transfer, &conflict, &snapshot.progress);
                }
            }
            assert!(Instant::now() < until, "transfer did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn perform(request: Request) -> Report {
        let transfer = Transfer::start(request).unwrap();
        wait(&transfer, |_, conflict, _| panic!("unexpected conflict {conflict:?}"))
    }

    fn mtime(path: &Path) -> (i64, i64) {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        (metadata.mtime(), metadata.mtime_nsec())
    }

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().mode() & 0o7777
    }

    fn write(path: &Path, bytes: &[u8], mode: u32) {
        std::fs::write(path, bytes).unwrap();
        let modified = SystemTime::UNIX_EPOCH + Duration::new(1_600_000_000, 123_456_789);
        std::fs::File::options().write(true).open(path).unwrap().set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn pattern(length: usize) -> Vec<u8> {
        (0..length).map(|index| (index * 31 % 251) as u8).collect()
    }

    /// Recursive source tree used by copy and cross-device move checks.
    fn tree(root: &Path, outside: &Path) -> PathBuf {
        let tree = root.join("tree # %");
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        write(&tree.join("nested/deep.bin"), &pattern(CHUNK * 2 + 17), 0o640);
        write(&tree.join("readonly.txt"), b"read only bytes", 0o444);
        symlink("readonly.txt", tree.join("relative-link")).unwrap();
        symlink(outside, tree.join("outside-link")).unwrap();
        symlink(root.join("missing-target"), tree.join("dangling")).unwrap();
        write(&tree.join(OsString::from_vec(b"raw-\xff".to_vec())), b"raw name bytes", 0o600);
        std::fs::create_dir(tree.join("locked")).unwrap();
        write(&tree.join("locked/inner.txt"), b"inside locked", 0o644);
        std::fs::set_permissions(tree.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(&tree, std::fs::Permissions::from_mode(0o750)).unwrap();
        tree
    }

    fn assert_tree(copy: &Path, outside: &Path) {
        assert_eq!(std::fs::read(copy.join("nested/deep.bin")).unwrap(), pattern(CHUNK * 2 + 17));
        assert_eq!(mode(&copy.join("nested/deep.bin")), 0o640);
        assert_eq!(mtime(&copy.join("nested/deep.bin")), (1_600_000_000, 123_456_789));
        assert_eq!(std::fs::read(copy.join("readonly.txt")).unwrap(), b"read only bytes");
        assert_eq!(mode(&copy.join("readonly.txt")), 0o444);
        assert_eq!(std::fs::read_link(copy.join("relative-link")).unwrap(), Path::new("readonly.txt"));
        assert_eq!(std::fs::read_link(copy.join("outside-link")).unwrap(), outside);
        assert!(std::fs::symlink_metadata(copy.join("dangling")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(copy.join(OsString::from_vec(b"raw-\xff".to_vec()))).unwrap(), b"raw name bytes");
        assert_eq!(std::fs::read(copy.join("locked/inner.txt")).unwrap(), b"inside locked");
        assert_eq!(mode(&copy.join("locked")), 0o555);
        assert_eq!(mode(copy), 0o750);
    }

    #[test]
    fn keep_both_names_preserve_extensions_and_dotfiles() {
        assert_eq!(keep_both_name(OsStr::new("report.txt"), 1), "report (copy).txt");
        assert_eq!(keep_both_name(OsStr::new("report.txt"), 3), "report (copy 3).txt");
        assert_eq!(keep_both_name(OsStr::new(".bashrc"), 1), ".bashrc (copy)");
        assert_eq!(keep_both_name(OsStr::new("Folder"), 2), "Folder (copy 2)");
        assert_eq!(keep_both_name(OsStr::new("trailing."), 1), "trailing. (copy)");
        assert_eq!(keep_both_name(OsStr::from_bytes(b"raw-\xff.bin"), 1).as_bytes(), b"raw-\xff (copy).bin");
    }

    #[test]
    fn copy_preserves_bytes_metadata_symlinks_raw_names_and_never_overwrites() {
        let root = fixture("copy");
        let outside = root.join("outside-target");
        write(&outside, b"never touch the target", 0o600);
        let source = root.join("source");
        std::fs::create_dir(&source).unwrap();
        let tree = tree(&source, &outside);
        let file = source.join("file.txt");
        write(&file, b"file bytes", 0o640);
        let selected_link = source.join("selected-link");
        symlink(&outside, &selected_link).unwrap();
        let destination = root.join("destination");
        std::fs::create_dir(&destination).unwrap();
        let sources = [file.clone(), tree.clone(), selected_link.clone()];
        let report = perform(request(Mode::Copy, &sources, &destination, None));
        assert_eq!((report.completed, report.skipped, report.failure_count, report.cancelled), (3, 0, 0, false), "{report:?}");
        assert_eq!(report.created, [destination.join("file.txt"), destination.join("tree # %"), destination.join("selected-link")]);
        assert_eq!(report.message, "Copied 3 items to “destination”.");
        assert_tree(&destination.join("tree # %"), &outside);
        assert_tree(&tree, &outside);
        assert_eq!(std::fs::read(destination.join("file.txt")).unwrap(), b"file bytes");
        assert_eq!(mtime(&destination.join("file.txt")), (1_600_000_000, 123_456_789));
        assert_eq!(std::fs::read_link(destination.join("selected-link")).unwrap(), outside);
        assert_eq!(std::fs::read(&outside).unwrap(), b"never touch the target");

        // Pasting into the source folder duplicates with familiar names, never asks.
        let report = perform(request(Mode::Copy, std::slice::from_ref(&file), &source, None));
        assert_eq!(report.created, [source.join("file (copy).txt")]);
        let report = perform(request(Mode::Copy, std::slice::from_ref(&file), &source, None));
        assert_eq!(report.created, [source.join("file (copy 2).txt")]);
        assert_eq!(std::fs::read(source.join("file (copy 2).txt")).unwrap(), b"file bytes");

        // Existing destinations, including a dangling symlink, are never replaced.
        std::fs::write(destination.join("file.txt"), b"newer destination bytes").unwrap();
        std::fs::remove_file(destination.join("selected-link")).unwrap();
        symlink(root.join("nowhere"), destination.join("selected-link")).unwrap();
        let report = perform(request(Mode::Copy, &sources, &destination, Some(Choice::Skip)));
        assert_eq!((report.completed, report.skipped, report.changed), (0, 3, false));
        assert_eq!(std::fs::read(destination.join("file.txt")).unwrap(), b"newer destination bytes");
        assert_eq!(std::fs::read_link(destination.join("selected-link")).unwrap(), root.join("nowhere"));
        let report = perform(request(Mode::Copy, &sources, &destination, Some(Choice::KeepBoth)));
        assert_eq!(report.created, [destination.join("file (copy).txt"), destination.join("tree # % (copy)"), destination.join("selected-link (copy)")]);
        assert_tree(&destination.join("tree # % (copy)"), &outside);
        assert_eq!(std::fs::read(destination.join("file.txt")).unwrap(), b"newer destination bytes");

        // Interactive: one prompt answered with apply-to-all covers the rest.
        let mut prompts = Vec::new();
        let transfer = Transfer::start(request(Mode::Copy, &sources, &destination, None)).unwrap();
        let report = wait(&transfer, |transfer, conflict, progress| {
            assert_eq!(progress.phase, Phase::WaitingForChoice);
            prompts.push(conflict.destination.clone());
            transfer.answer(conflict.id, Choice::Skip, true);
        });
        assert_eq!(prompts, [destination.join("file.txt")]);
        assert_eq!((report.skipped, report.changed), (3, false));
        // Stop at the first prompt cancels the remaining items and changes nothing.
        let transfer = Transfer::start(request(Mode::Copy, &sources, &destination, None)).unwrap();
        let report = wait(&transfer, |transfer, conflict, _| transfer.answer(conflict.id, Choice::Stop, false));
        assert!(report.cancelled && !report.changed);
        assert_eq!(report.message, "Cancelled. Nothing was changed.");
        // Cancelling while a prompt is waiting releases the worker.
        let transfer = Transfer::start(request(Mode::Copy, &sources, &destination, None)).unwrap();
        let report = wait(&transfer, |transfer, _, _| transfer.cancel());
        assert!(report.cancelled && !report.changed);

        // A folder cannot be copied into itself.
        let report = perform(request(Mode::Copy, std::slice::from_ref(&tree), &tree.join("nested"), None));
        assert_eq!(report.failure_count, 1);
        assert!(report.failures[0].message.contains("inside itself"));
        assert_eq!(std::fs::read_dir(tree.join("nested")).unwrap().count(), 1);
        cleanup(&root);
    }

    #[test]
    fn move_is_atomic_on_one_filesystem_and_verified_across_filesystems() {
        let root = fixture("move");
        let outside = root.join("outside-target");
        write(&outside, b"never touch the target", 0o600);
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let file = source.join("move me.txt");
        write(&file, b"moved bytes", 0o600);
        let inode = std::fs::metadata(&file).unwrap().ino();
        let report = perform(request(Mode::Move, std::slice::from_ref(&file), &destination, None));
        assert_eq!(report.created, [destination.join("move me.txt")]);
        assert_eq!(report.message, "Moved 1 item to “destination”.");
        assert!(!file.exists());
        assert_eq!(std::fs::metadata(destination.join("move me.txt")).unwrap().ino(), inode);

        // Conflicts never overwrite; Skip leaves the source in place.
        write(&file, b"second source", 0o600);
        let report = perform(request(Mode::Move, std::slice::from_ref(&file), &destination, Some(Choice::Skip)));
        assert_eq!(report.skipped, 1);
        assert_eq!(std::fs::read(&file).unwrap(), b"second source");
        assert_eq!(std::fs::read(destination.join("move me.txt")).unwrap(), b"moved bytes");
        let report = perform(request(Mode::Move, std::slice::from_ref(&file), &destination, Some(Choice::KeepBoth)));
        assert_eq!(report.created, [destination.join("move me (copy).txt")]);
        assert!(!file.exists());

        // Same folder and into-itself moves change nothing.
        let tree = tree(&source, &outside);
        let report = perform(request(Mode::Move, std::slice::from_ref(&tree), &source, None));
        assert_eq!((report.skipped, report.changed), (1, false));
        let report = perform(request(Mode::Move, std::slice::from_ref(&tree), &tree.join("nested"), None));
        assert_eq!((report.failure_count, report.changed), (1, false));
        assert_tree(&tree, &outside);

        // A selected symlink moves as a symlink; its target is untouched.
        let link = source.join("link");
        symlink(&outside, &link).unwrap();
        perform(request(Mode::Move, std::slice::from_ref(&link), &destination, None));
        assert_eq!(std::fs::read_link(destination.join("link")).unwrap(), outside);
        assert_eq!(std::fs::read(&outside).unwrap(), b"never touch the target");

        // Different filesystem: /var/tmp (fixture) to a private /dev/shm folder.
        let other = PathBuf::from("/dev/shm").join(format!("decfiles-transfer-move-{}", std::process::id()));
        if other.exists() {
            cleanup(&other);
        }
        std::fs::create_dir(&other).unwrap();
        if std::fs::metadata(&other).unwrap().dev() == std::fs::metadata(&root).unwrap().dev() {
            eprintln!("skipping cross-device move: /dev/shm shares the fixture filesystem");
        } else {
            let report = perform(request(Mode::Move, std::slice::from_ref(&tree), &other, None));
            assert_eq!((report.completed, report.failure_count), (1, 0), "{report:?}");
            assert_tree(&other.join("tree # %"), &outside);
            // Verified originals are removed, except where the read-only source folder
            // forbids it; that original and its enclosing folders are kept and reported.
            assert!(report.message.contains("locked/inner.txt” was kept: permission denied."), "{}", report.message);
            assert_eq!(std::fs::read(tree.join("locked/inner.txt")).unwrap(), b"inside locked");
            assert_eq!(std::fs::read_dir(&tree).unwrap().count(), 1);
            assert!(!tree.join("nested").exists());
            // Partial failure: an unreadable file stays with its original folder.
            let partial = source.join("partial");
            std::fs::create_dir(&partial).unwrap();
            write(&partial.join("readable"), b"readable", 0o600);
            write(&partial.join("unreadable"), b"secret", 0o000);
            let report = perform(request(Mode::Move, std::slice::from_ref(&partial), &other, None));
            assert_eq!(report.failure_count, 1, "{report:?}");
            assert!(report.failures[0].path.ends_with("partial/unreadable"));
            assert!(report.message.contains("was kept"), "{}", report.message);
            assert!(!partial.join("readable").exists());
            assert_eq!(std::fs::read(other.join("partial/readable")).unwrap(), b"readable");
            std::fs::set_permissions(partial.join("unreadable"), std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(std::fs::read(partial.join("unreadable")).unwrap(), b"secret");
            assert!(!other.join("partial/unreadable").exists());
        }
        cleanup(&other);
        cleanup(&root);
    }

    #[test]
    fn denied_special_cancelled_and_externally_changed_items_report_without_false_success() {
        let root = fixture("races");
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let readable = source.join("readable");
        let unreadable = source.join("unreadable");
        write(&readable, b"readable", 0o600);
        write(&unreadable, b"secret", 0o000);
        let fifo = source.join("pipe");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        let report = perform(request(Mode::Copy, &[readable.clone(), unreadable.clone(), fifo.clone()], &destination, None));
        assert_eq!((report.completed, report.failure_count), (1, 2), "{report:?}");
        assert!(report.failures.iter().any(|failure| failure.path == unreadable && failure.message == "permission denied"));
        assert!(report.failures.iter().any(|failure| failure.path == fifo && failure.message.contains("special files")));
        assert!(!destination.join("unreadable").exists() && !destination.join("pipe").exists());
        assert!(report.message.starts_with("Copied 1 item to “destination”. Could not copy 2 items"), "{}", report.message);

        // Read-only destination: nothing is created and the error is explicit.
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let report = perform(request(Mode::Copy, std::slice::from_ref(&readable), &locked, None));
        assert_eq!((report.completed, report.failure_count, report.changed), (0, 1, false));
        assert_eq!(report.failures[0].message, "permission denied");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 0);

        // Cancellation during a file copy leaves no partial destination.
        let big = source.join("big.bin");
        write(&big, &pattern(CHUNK * 12), 0o600);
        let throttle = Duration::from_millis(40);
        let transfer = Transfer::start_throttled(request(Mode::Copy, &[readable.clone(), big.clone()], &destination, Some(Choice::KeepBoth)), throttle).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let progress = transfer.poll().progress;
            if progress.items_done >= 1 && progress.bytes_done > 8 {
                break;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        transfer.cancel();
        let report = wait(&transfer, |_, _, _| {});
        assert!(report.cancelled && report.changed);
        assert_eq!(report.created, [destination.join("readable (copy)")]);
        assert!(report.message.starts_with("Cancelled. Copied 1 item"), "{}", report.message);
        assert!(!destination.join("big.bin").exists());
        assert_eq!(std::fs::read(&big).unwrap(), pattern(CHUNK * 12));
        let names: Vec<_> = std::fs::read_dir(&destination).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "{names:?}");

        // A source modified during copying is not published as a complete copy.
        let transfer = Transfer::start_throttled(request(Mode::Copy, std::slice::from_ref(&big), &destination, None), throttle).unwrap();
        while transfer.poll().progress.bytes_done == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
        std::fs::OpenOptions::new().append(true).open(&big).unwrap().write_all_at_end(b"appended");
        let report = wait(&transfer, |_, _, _| {});
        assert_eq!((report.completed, report.failure_count), (0, 1), "{report:?}");
        assert!(report.failures[0].message.contains("changed") || report.failures[0].message.contains("grew"));
        assert!(!destination.join("big.bin").exists());

        // A destination created by another process mid-copy is never overwritten.
        write(&big, &pattern(CHUNK * 12), 0o600);
        let transfer = Transfer::start_throttled(request(Mode::Copy, std::slice::from_ref(&big), &destination, Some(Choice::Skip)), throttle).unwrap();
        while transfer.poll().progress.bytes_done == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
        std::fs::write(destination.join("big.bin"), b"external contender").unwrap();
        let report = wait(&transfer, |_, _, _| {});
        assert_eq!((report.completed, report.skipped), (0, 1), "{report:?}");
        assert_eq!(std::fs::read(destination.join("big.bin")).unwrap(), b"external contender");

        // Cancelling inside a folder keeps what was copied and says so.
        let folder = source.join("folder");
        std::fs::create_dir(&folder).unwrap();
        write(&folder.join("a"), &pattern(CHUNK * 4), 0o600);
        write(&folder.join("b"), &pattern(CHUNK * 4), 0o600);
        let transfer = Transfer::start_throttled(request(Mode::Copy, std::slice::from_ref(&folder), &destination, None), throttle).unwrap();
        while transfer.poll().progress.bytes_done < CHUNK as u64 * 5 {
            std::thread::sleep(Duration::from_millis(2));
        }
        transfer.cancel();
        let report = wait(&transfer, |_, _, _| {});
        // The folder was staged privately: cancelling publishes nothing and removes the staging.
        assert!(report.cancelled && !report.changed && report.completed == 0, "{report:?}");
        assert_eq!(report.message, "Cancelled. Nothing was changed.");
        assert!(!destination.join("folder").exists());
        let leftovers: Vec<_> = std::fs::read_dir(&destination).unwrap().map(|entry| entry.unwrap().file_name()).filter(|name| name.to_string_lossy().starts_with(".decfiles-")).collect();
        assert!(leftovers.is_empty(), "{leftovers:?} {:?}", std::fs::read_dir(destination.join(&leftovers[0])).map(|d| d.map(|e| e.unwrap().file_name()).collect::<Vec<_>>()));
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
        cleanup(&root);
    }

    fn hooked(request: Request, named_files: bool, hook: impl Fn(&str, &Path) + Send + Sync + 'static) -> Report {
        let transfer = Transfer::start_with(request, Options { hook: Some(Arc::new(hook)), named_files, ..Options::default() }).unwrap();
        wait(&transfer, |_, conflict, _| panic!("unexpected conflict {conflict:?}"))
    }

    fn private_leftovers(folder: &Path) -> Vec<OsString> {
        std::fs::read_dir(folder).unwrap().map(|entry| entry.unwrap().file_name()).filter(|name| name.to_string_lossy().starts_with(".decfiles-")).collect()
    }

    #[test]
    fn deterministic_races_never_remove_or_publish_the_wrong_item() {
        use std::sync::atomic::AtomicUsize;
        let root = fixture("hooks");
        let other = PathBuf::from("/dev/shm").join(format!("decfiles-transfer-hooks-{}", std::process::id()));
        if other.exists() {
            cleanup(&other);
        }
        std::fs::create_dir(&other).unwrap();
        let cross_device = std::fs::metadata(&other).unwrap().dev() != std::fs::metadata(&root).unwrap().dev();
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();

        if cross_device {
            // An original replaced after copying but before its claim is never removed.
            let original = source.join("claimed.txt");
            write(&original, b"original bytes", 0o600);
            let (watched, moved_away) = (original.clone(), source.join("moved-away.txt"));
            let report = hooked(request(Mode::Move, std::slice::from_ref(&original), &other, None), false, move |point, path| {
                if point == "before-claim" && path == watched {
                    std::fs::rename(&watched, &moved_away).unwrap();
                    std::fs::write(&watched, b"replacement").unwrap();
                }
            });
            assert_eq!(report.completed, 1, "{report:?}");
            assert!(report.message.contains("claimed.txt” was kept: it changed during the move"), "{}", report.message);
            assert_eq!(std::fs::read(&original).unwrap(), b"replacement");
            assert_eq!(std::fs::read(source.join("moved-away.txt")).unwrap(), b"original bytes");
            assert_eq!(std::fs::read(other.join("claimed.txt")).unwrap(), b"original bytes");
            assert!(private_leftovers(&source).is_empty());

            // A published copy replaced or deleted before the original is removed keeps the original.
            for (index, replace) in [true, false].into_iter().enumerate() {
                let original = source.join(format!("published-{index}.txt"));
                write(&original, b"keep me", 0o600);
                let target = other.join(format!("published-{index}.txt"));
                let watched = target.clone();
                let report = hooked(request(Mode::Move, std::slice::from_ref(&original), &other, None), false, move |point, path| {
                    if point == "published" && path == watched {
                        std::fs::remove_file(&watched).unwrap();
                        if replace {
                            std::fs::write(&watched, b"someone else").unwrap();
                        }
                    }
                });
                assert!(report.message.contains("was kept: its copy at the destination changed or disappeared"), "{}", report.message);
                assert_eq!(std::fs::read(&original).unwrap(), b"keep me");
                assert_eq!(std::fs::read(&target).ok(), replace.then(|| b"someone else".to_vec()));
            }
        } else {
            eprintln!("skipping cross-device claim races: /dev/shm shares the fixture filesystem");
        }

        // A link replaced while it is read is re-read; one that keeps changing is refused.
        let link = source.join("racing-link");
        symlink("first", &link).unwrap();
        let (watched, swaps) = (link.clone(), Arc::new(AtomicUsize::new(0)));
        let counter = swaps.clone();
        let report = hooked(request(Mode::Copy, std::slice::from_ref(&link), &destination, None), false, move |point, path| {
            if point == "read-link" && path == watched && counter.fetch_add(1, Ordering::SeqCst) == 0 {
                std::fs::remove_file(&watched).unwrap();
                symlink("second-longer", &watched).unwrap();
            }
        });
        assert_eq!(report.completed, 1, "{report:?}");
        assert_eq!(std::fs::read_link(destination.join("racing-link")).unwrap(), Path::new("second-longer"));
        assert_eq!(swaps.load(Ordering::SeqCst), 2);
        let watched = link.clone();
        let report = hooked(request(Mode::Copy, std::slice::from_ref(&link), &destination, Some(Choice::KeepBoth)), false, move |point, path| {
            if point == "read-link" && path == watched {
                let next = if std::fs::read_link(&watched).unwrap() == Path::new("a") { "bb" } else { "a" };
                std::fs::remove_file(&watched).unwrap();
                symlink(next, &watched).unwrap();
            }
        });
        assert_eq!((report.completed, report.failure_count), (0, 1));
        assert!(report.failures[0].message.contains("kept changing"));
        assert!(!destination.join("racing-link (copy)").exists());

        // A private staging folder swapped before publication is never published as ours.
        let tree = source.join("tree");
        std::fs::create_dir(&tree).unwrap();
        write(&tree.join("inside.txt"), b"inside", 0o600);
        let stolen = root.join("stolen");
        let thief = stolen.clone();
        let report = hooked(request(Mode::Copy, std::slice::from_ref(&tree), &destination, None), false, move |point, path| {
            if point == "staged-folder" && path.parent().is_some_and(|parent| parent.ends_with("destination")) {
                std::fs::rename(path, &thief).unwrap();
                std::fs::create_dir(path).unwrap();
            }
        });
        assert_eq!((report.completed, report.failure_count), (0, 1), "{report:?}");
        assert!(report.failures[0].message.contains("replaced before it could be published"));
        assert!(!destination.join("tree").exists(), "the impostor folder must not be published");
        let impostors = private_leftovers(&destination);
        assert_eq!(impostors.len(), 1, "the impostor is left alone under the private name");
        assert_eq!(std::fs::read_dir(destination.join(&impostors[0])).unwrap().count(), 0);
        std::fs::remove_dir(destination.join(&impostors[0])).unwrap();
        assert_eq!(std::fs::read(tree.join("inside.txt")).unwrap(), b"inside");
        std::fs::remove_dir_all(&stolen).unwrap();

        // A conflict at publication reuses the complete staging for Keep Both; Skip discards it.
        for choice in [Choice::KeepBoth, Choice::Skip] {
            let (watched, stages, created) = (destination.join("tree"), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
            let (stage_count, create_count) = (stages.clone(), created.clone());
            let report = hooked(request(Mode::Copy, std::slice::from_ref(&tree), &destination, Some(choice)), false, move |point, path| {
                if point == "staged-folder" {
                    stage_count.fetch_add(1, Ordering::SeqCst);
                }
                if point == "before-publish" && path == watched && create_count.fetch_add(1, Ordering::SeqCst) == 0 {
                    std::fs::create_dir(&watched).unwrap();
                    std::fs::write(watched.join("marker"), b"external").unwrap();
                }
            });
            assert_eq!(stages.load(Ordering::SeqCst), 1, "staged once for {choice:?}");
            assert_eq!(std::fs::read(destination.join("tree/marker")).unwrap(), b"external");
            match choice {
                Choice::KeepBoth => {
                    assert_eq!(report.created, [destination.join("tree (copy)")]);
                    assert_eq!(std::fs::read(destination.join("tree (copy)/inside.txt")).unwrap(), b"inside");
                }
                _ => assert_eq!((report.completed, report.skipped), (0, 1)),
            }
            assert!(private_leftovers(&destination).is_empty());
            std::fs::remove_dir_all(destination.join("tree")).unwrap();
        }

        // The private O_EXCL fallback publishes atomically and leaves nothing when it loses.
        let file = source.join("named.txt");
        write(&file, &pattern(CHUNK * 3), 0o640);
        let report = hooked(request(Mode::Copy, std::slice::from_ref(&file), &destination, None), true, |_, _| {});
        assert_eq!(report.completed, 1);
        assert_eq!(std::fs::read(destination.join("named.txt")).unwrap(), pattern(CHUNK * 3));
        assert_eq!(mode(&destination.join("named.txt")), 0o640);
        let watched = destination.join("named (copy).txt");
        let report = hooked(request(Mode::Copy, std::slice::from_ref(&file), &destination, Some(Choice::KeepBoth)), true, move |point, path| {
            if point == "before-publish" && path == watched && !watched.exists() {
                std::fs::write(&watched, b"contender").unwrap();
            }
        });
        assert_eq!(report.created, [destination.join("named (copy 2).txt")]);
        assert_eq!(std::fs::read(destination.join("named (copy).txt")).unwrap(), b"contender");
        let transfer =
            Transfer::start_with(request(Mode::Copy, std::slice::from_ref(&file), &other, None), Options { named_files: true, chunk_delay: Duration::from_millis(40), ..Options::default() }).unwrap();
        while transfer.poll().progress.bytes_done == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
        transfer.cancel();
        let report = wait(&transfer, |_, _, _| {});
        assert!(report.cancelled && !report.changed, "{report:?}");
        assert!(private_leftovers(&destination).is_empty() && private_leftovers(&other).is_empty() && !other.join("named.txt").exists());
        cleanup(&other);
        cleanup(&root);
    }

    trait AppendAll {
        fn write_all_at_end(self, bytes: &[u8]);
    }
    impl AppendAll for std::fs::File {
        fn write_all_at_end(mut self, bytes: &[u8]) {
            use std::io::Write;
            self.write_all(bytes).unwrap();
        }
    }

    #[test]
    fn clipboard_formats_round_trip_local_names_and_reject_remote_items() {
        let raw = PathBuf::from(OsString::from_vec(b"/home/user/raw-\xff # %.txt".to_vec()));
        let plain = PathBuf::from("/home/user/A folder/ü");
        let gnome = clipboard::encode_gnome(Mode::Move, &[raw.clone(), plain.clone()]);
        assert!(gnome.starts_with("cut\nfile:///home/user/raw-%FF%20%23%20%25.txt\n"), "{gnome}");
        assert_eq!(clipboard::decode_gnome(&gnome), Some((Mode::Move, vec![raw.clone(), plain.clone()])));
        assert_eq!(clipboard::decode_gnome("copy\r\nfile:///tmp/a\r\n"), Some((Mode::Copy, vec![PathBuf::from("/tmp/a")])));
        let list = clipboard::encode_uri_list(std::slice::from_ref(&raw));
        assert!(list.ends_with("\r\n"));
        assert_eq!(clipboard::decode_uri_list(&format!("# comment\r\n{list}")), Some(vec![raw]));
        assert_eq!(clipboard::decode_gnome("copy\nhttps://example.com/file"), None);
        assert_eq!(clipboard::decode_gnome("copy\nfile://server/share/file"), None);
        assert_eq!(clipboard::decode_gnome("move\nfile:///tmp/a"), None);
        assert_eq!(clipboard::decode_gnome("copy\n"), None);
        assert_eq!(clipboard::decode_uri_list("file:///tmp/a\nsmb://server/b"), None);
        // A mislabelled plain list never becomes a move; a text/uri-list "cut" header is not a file.
        assert_eq!(clipboard::decode(clipboard::GNOME, "file:///tmp/a\r\n"), Some((Mode::Copy, vec![PathBuf::from("/tmp/a")])));
        assert_eq!(clipboard::decode(clipboard::URI_LIST, "cut\nfile:///tmp/a"), None);
        assert_eq!(clipboard::decode(clipboard::GNOME, "cut\nfile:///tmp/a"), Some((Mode::Move, vec![PathBuf::from("/tmp/a")])));
    }
}
