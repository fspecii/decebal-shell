//! Recursive name search on a bounded worker thread. The UI polls new hits latest-only;
//! a search is cancelled by a newer query, navigation, a tab switch or closing. Never
//! follows symlinks; bounded depth, visited entries, results and live worker threads.
use crate::model::Entry;
use std::{
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

pub const MAX_RESULTS: usize = 2000;
pub const MAX_VISITED: u64 = 200_000;
pub const MAX_DEPTH: usize = 32;
/// Folders queued for a later visit; deeper/wider trees beyond this are reported as truncated.
pub const MAX_PENDING: usize = 10_000;
/// Hits are published to the UI in batches of at most this many.
const BATCH: usize = 128;

#[derive(Clone, Copy, Debug)]
struct Limits {
    results: usize,
    visited: u64,
    pending: usize,
}

const LIMITS: Limits = Limits { results: MAX_RESULTS, visited: MAX_VISITED, pending: MAX_PENDING };
/// Cancelled searches may still be leaving a blocked syscall; refuse to pile up threads.
pub const MAX_LIVE: usize = 4;
static LIVE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub visited: u64,
    /// Largest number of folders that were queued at once (bounded by MAX_PENDING).
    pub peak_pending: usize,
    pub skipped: u64,
    pub done: bool,
    pub truncated: bool,
}

#[derive(Default)]
struct State {
    hits: Vec<Entry>,
    status: Status,
}

struct Shared {
    cancel: AtomicBool,
    state: Mutex<State>,
}

pub struct Search {
    shared: Arc<Shared>,
}

/// Case-insensitive match on the displayed name; non-UTF8 bytes match as U+FFFD.
pub fn matches(name: &std::ffi::OsStr, query: &str) -> bool {
    name.to_string_lossy().to_lowercase().contains(query)
}

impl Search {
    pub fn start(root: PathBuf, query: &str) -> io::Result<Self> {
        Self::spawn(
            root,
            query,
            LIMITS,
            #[cfg(any(test, feature = "fixture-delay"))]
            std::time::Duration::ZERO,
        )
    }

    #[cfg(test)]
    fn start_limited(root: PathBuf, query: &str, limits: Limits) -> io::Result<Self> {
        Self::spawn(root, query, limits, std::time::Duration::ZERO)
    }

    /// Fixture builds only: a capped delay per folder makes cancellation observable.
    #[cfg(any(test, feature = "fixture-delay"))]
    pub fn start_throttled(root: PathBuf, query: &str, delay: std::time::Duration) -> io::Result<Self> {
        Self::spawn(root, query, LIMITS, delay.min(std::time::Duration::from_secs(2)))
    }

    fn spawn(root: PathBuf, query: &str, limits: Limits, #[cfg(any(test, feature = "fixture-delay"))] delay: std::time::Duration) -> io::Result<Self> {
        let query = query.to_lowercase();
        if query.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Enter text to search for."));
        }
        if LIVE.fetch_add(1, Ordering::SeqCst) >= MAX_LIVE {
            LIVE.fetch_sub(1, Ordering::SeqCst);
            return Err(io::Error::other("Earlier searches are still stopping. Try again in a moment."));
        }
        let shared = Arc::new(Shared { cancel: AtomicBool::new(false), state: Mutex::new(State::default()) });
        let worker = shared.clone();
        let spawned = std::thread::Builder::new().name("decfiles-search".into()).spawn(move || {
            walk(
                &worker,
                root,
                &query,
                limits,
                #[cfg(any(test, feature = "fixture-delay"))]
                delay,
            );
            worker.state.lock().unwrap_or_else(|error| error.into_inner()).status.done = true;
            LIVE.fetch_sub(1, Ordering::SeqCst);
        });
        if let Err(error) = spawned {
            LIVE.fetch_sub(1, Ordering::SeqCst);
            return Err(error);
        }
        Ok(Self { shared })
    }

    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::SeqCst);
    }

    /// Hits after the first `from` already delivered, plus the current status.
    pub fn take_new(&self, from: usize) -> (Vec<Entry>, Status) {
        let state = self.shared.state.lock().unwrap_or_else(|error| error.into_inner());
        (state.hits.get(from..).map(<[Entry]>::to_vec).unwrap_or_default(), state.status.clone())
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Budgets are enforced per entry while a folder is being read: hits are flushed in
/// bounded batches, the visited and result limits stop enumeration mid-folder, and the
/// queue of folders still to visit is capped.
fn walk(shared: &Shared, root: PathBuf, query: &str, limits: Limits, #[cfg(any(test, feature = "fixture-delay"))] delay: std::time::Duration) {
    let mut stack = vec![(root, 0usize)];
    let mut visited = 0u64;
    let mut hits = 0usize;
    let mut batch: Vec<Entry> = Vec::with_capacity(BATCH);
    let mut unvisited = 0u64;
    let flush = |batch: &mut Vec<Entry>, visited: u64, skipped: u64, truncated: bool, peak: usize| {
        let mut state = shared.state.lock().unwrap_or_else(|error| error.into_inner());
        state.hits.append(batch);
        state.status.visited = visited;
        state.status.skipped += skipped;
        state.status.truncated |= truncated;
        state.status.peak_pending = state.status.peak_pending.max(peak);
    };
    let mut peak = 1;
    while let Some((folder, depth)) = stack.pop() {
        if shared.cancel.load(Ordering::SeqCst) {
            return;
        }
        #[cfg(any(test, feature = "fixture-delay"))]
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        let Ok(children) = std::fs::read_dir(&folder) else {
            unvisited += 1;
            continue;
        };
        for child in children {
            if shared.cancel.load(Ordering::SeqCst) {
                return;
            }
            let Ok(child) = child else {
                continue;
            };
            if visited >= limits.visited || hits >= limits.results {
                flush(&mut batch, visited, std::mem::take(&mut unvisited), true, peak);
                return;
            }
            visited += 1;
            // DirEntry::file_type does not follow symlinks: links are listed, never entered.
            let directory = child.file_type().is_ok_and(|kind| kind.is_dir());
            let name = child.file_name();
            if matches(&name, query) {
                let content_type = if directory { String::new() } else { gio::content_type_guess(Some(std::path::Path::new(&name)), None::<&[u8]>).0.to_string() };
                batch.push(Entry { path: child.path(), name: name.to_string_lossy().into_owned(), directory, content_type, ..Default::default() });
                hits += 1;
            }
            if directory && depth < MAX_DEPTH {
                if stack.len() < limits.pending {
                    stack.push((child.path(), depth + 1));
                    peak = peak.max(stack.len());
                } else {
                    shared.state.lock().unwrap_or_else(|error| error.into_inner()).status.truncated = true;
                }
            }
            if batch.len() >= BATCH || visited.is_multiple_of(1024) {
                flush(&mut batch, visited, std::mem::take(&mut unvisited), false, peak);
            }
        }
        flush(&mut batch, visited, std::mem::take(&mut unvisited), false, peak);
    }
    flush(&mut batch, visited, unvisited, false, peak);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::ffi::OsStringExt,
        time::{Duration, Instant},
    };

    /// The live-worker bound is process-wide; these tests must not overlap.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn finish(search: &Search) -> (Vec<Entry>, Status) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let (hits, status) = search.take_new(0);
            if status.done {
                return (hits, status);
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn finds_nested_names_without_following_links_and_stays_bounded() {
        let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
        let root = PathBuf::from("/var/tmp").join(format!("decfiles-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Alpha/Beta")).unwrap();
        std::fs::write(root.join("Alpha/Beta/Report.TXT"), b"x").unwrap();
        std::fs::write(root.join("report-top.md"), b"x").unwrap();
        std::fs::write(root.join(std::ffi::OsString::from_vec(b"raw-\xff-report".to_vec())), b"x").unwrap();
        std::fs::write(root.join("unrelated"), b"x").unwrap();
        let outside = root.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("report-outside"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link-to-outside")).unwrap();
        let (hits, status) = finish(&Search::start(root.clone(), "REPORT").unwrap());
        let mut names: Vec<_> = hits.iter().map(|hit| hit.name.clone()).collect();
        names.sort();
        assert_eq!(names, ["Report.TXT", "raw-\u{fffd}-report", "report-top.md"]);
        assert!(hits.iter().any(|hit| hit.path == root.join(std::ffi::OsString::from_vec(b"raw-\xff-report".to_vec()))), "raw path bytes preserved");
        assert_eq!(hits.iter().find(|hit| hit.name == "Report.TXT").unwrap().content_type, "text/plain");
        assert!(status.done && !status.truncated);
        // Bounded results.
        let many = root.join("many");
        std::fs::create_dir(&many).unwrap();
        for index in 0..MAX_RESULTS + 50 {
            std::fs::write(many.join(format!("hit-{index}")), b"").unwrap();
        }
        let (hits, status) = finish(&Search::start(root.clone(), "hit-").unwrap());
        assert_eq!(hits.len(), MAX_RESULTS);
        assert!(status.truncated);
        assert!(Search::start(root.clone(), "").is_err());
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn budgets_are_enforced_inside_enumeration_on_a_real_huge_folder() {
        let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
        let root = PathBuf::from("/var/tmp").join(format!("decfiles-search-huge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let huge = root.join("huge");
        std::fs::create_dir_all(&huge).unwrap();
        for index in 0..20_000 {
            std::fs::write(huge.join(format!("match-{index:05}")), b"").unwrap();
        }
        // The visited budget stops a single 20,000-entry folder mid-enumeration.
        let limits = Limits { results: 100_000, visited: 1_000, pending: 50 };
        let (hits, status) = finish(&Search::start_limited(root.clone(), "match", limits).unwrap());
        assert!(status.done && status.truncated);
        assert_eq!(status.visited, 1_000, "visited stops exactly at the budget");
        assert!(hits.len() < 1_000);
        // The result budget stops it as well, never holding more than the limit.
        let limits = Limits { results: 300, visited: 1_000_000, pending: 50 };
        let (hits, status) = finish(&Search::start_limited(root.clone(), "match", limits).unwrap());
        assert_eq!(hits.len(), 300);
        assert!(status.truncated && status.visited <= 302, "{status:?}");
        // Wide folder trees: the queue of folders to visit stays bounded.
        let wide = root.join("wide");
        for index in 0..400 {
            std::fs::create_dir_all(wide.join(format!("d{index:03}"))).unwrap();
        }
        let limits = Limits { results: 100_000, visited: 1_000_000, pending: 50 };
        let (_, status) = finish(&Search::start_limited(wide.clone(), "zzz", limits).unwrap());
        assert!(status.truncated && status.peak_pending <= 50, "{status:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancellation_stops_the_walk_and_live_workers_are_bounded() {
        let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
        let root = PathBuf::from("/var/tmp").join(format!("decfiles-search-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for index in 0..20 {
            std::fs::create_dir_all(root.join(format!("d{index}"))).unwrap();
            std::fs::write(root.join(format!("d{index}/match-{index}")), b"").unwrap();
        }
        let slow = Search::start_throttled(root.clone(), "match", Duration::from_millis(100)).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        slow.cancel();
        let (hits, status) = finish(&slow);
        assert!(status.done && hits.len() < 20, "cancelled walk stopped early: {}", hits.len());
        // Hold MAX_LIVE slow searches; the next is refused until one finishes.
        let held: Vec<_> = (0..MAX_LIVE).map(|_| Search::start_throttled(root.clone(), "match", Duration::from_millis(400)).unwrap()).collect();
        assert!(Search::start(root.clone(), "match").is_err());
        drop(held);
        let until = Instant::now() + Duration::from_secs(5);
        let again = loop {
            if let Ok(search) = Search::start(root.clone(), "match") {
                break search;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(finish(&again).0.len(), 20);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
