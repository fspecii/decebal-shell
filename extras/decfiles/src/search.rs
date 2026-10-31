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
/// Cancelled searches may still be leaving a blocked syscall; refuse to pile up threads.
pub const MAX_LIVE: usize = 4;
static LIVE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub visited: u64,
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
            #[cfg(any(test, feature = "fixture-delay"))]
            std::time::Duration::ZERO,
        )
    }

    /// Fixture builds only: a capped delay per folder makes cancellation observable.
    #[cfg(any(test, feature = "fixture-delay"))]
    pub fn start_throttled(root: PathBuf, query: &str, delay: std::time::Duration) -> io::Result<Self> {
        Self::spawn(root, query, delay.min(std::time::Duration::from_secs(2)))
    }

    fn spawn(root: PathBuf, query: &str, #[cfg(any(test, feature = "fixture-delay"))] delay: std::time::Duration) -> io::Result<Self> {
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

fn walk(shared: &Shared, root: PathBuf, query: &str, #[cfg(any(test, feature = "fixture-delay"))] delay: std::time::Duration) {
    let mut stack = vec![(root, 0usize)];
    while let Some((folder, depth)) = stack.pop() {
        if shared.cancel.load(Ordering::SeqCst) {
            return;
        }
        #[cfg(any(test, feature = "fixture-delay"))]
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        let Ok(children) = std::fs::read_dir(&folder) else {
            shared.state.lock().unwrap_or_else(|error| error.into_inner()).status.skipped += 1;
            continue;
        };
        let mut found = Vec::new();
        let mut visited = 0;
        for child in children.flatten() {
            if shared.cancel.load(Ordering::SeqCst) {
                return;
            }
            visited += 1;
            // DirEntry::file_type does not follow symlinks: links are listed, never entered.
            let directory = child.file_type().is_ok_and(|kind| kind.is_dir());
            let name = child.file_name();
            if matches(&name, query) {
                let content_type = if directory { String::new() } else { gio::content_type_guess(Some(std::path::Path::new(&name)), None::<&[u8]>).0.to_string() };
                found.push(Entry { path: child.path(), name: name.to_string_lossy().into_owned(), directory, content_type, ..Default::default() });
            }
            if directory && depth < MAX_DEPTH {
                stack.push((child.path(), depth + 1));
            }
        }
        let mut state = shared.state.lock().unwrap_or_else(|error| error.into_inner());
        state.status.visited += visited;
        let room = MAX_RESULTS - state.hits.len();
        if found.len() > room {
            found.truncate(room);
            state.status.truncated = true;
        }
        state.hits.extend(found);
        if state.status.truncated || state.status.visited >= MAX_VISITED {
            state.status.truncated = true;
            return;
        }
    }
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
