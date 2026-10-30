//! System Settings > Wallpaper > Wallhaven: browse and search wallhaven.cc and set a wallpaper from it.
//!
//! * [`query`], [`parse`], [`throttle`], [`store`], [`client`] hold everything that is not drawing (and are unit tested);
//! * [`Wh`] is the pane's state; it lives in the settings `Model`, starts its worker threads only when the
//!   Wallhaven tab is first opened, and talks to the UI through a channel drained by [`pump`] on every tick;
//! * the UI thread never touches the network or the disk for images: a single API worker (rate limited),
//!   a few thumbnail workers and one thread per download do that;
//! * "Set as wallpaper" downloads into the Wallpaper pane's own backgrounds folder and then goes through the
//!   same [`panes::set_wallpaper`](super::panes::set_wallpaper) as picking a picture there, so it is saved to
//!   `config.toml`, applied by rdwm and followed by "Colours from wallpaper".
//!
//! See `docs/wallpapers.md`.

pub mod cli;
pub mod client;
pub mod curated;
pub mod parse;
pub mod query;
pub mod store;
pub mod throttle;

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Opt, Row, Swatch, Thumb};
use crate::wallhaven::cache::Lru;
use client::{Client, NetError, Progress};
use parse::{human_size, short_count, Detail, Page, Wall};
use query::{purity_allowed, purity_bits, Prefs, Query, Sort};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use store::{Dirs, Secret};
use throttle::Throttle;
use tiny_skia::Pixmap;

/// Decoded thumbnails kept in memory (small ones are ~110 KB, large ones ~240 KB).
pub const MAX_THUMBS: usize = 120;
/// Longest edge a thumbnail is decoded at.
const THUMB_W: u32 = 320;
const THUMB_WORKERS: usize = 3;
/// A thumbnail that failed to load is not asked for again for this long.
const RETRY_THUMB: Duration = Duration::from_secs(20);
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(400);
const DETAIL_DEBOUNCE: Duration = Duration::from_millis(450);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Backgrounds,
    Wallhaven,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Browse,
    Favourites,
    Curated,
}

// ---------------------------------------------------------------- worker plumbing

enum Job {
    Search { gen: u64, page: u32, query: Query },
    Detail { id: String },
    Pick { query: Query },
}

struct ThumbJob {
    key: String,
    url: String,
    id: String,
    large: bool,
}

enum ThumbRes {
    Ok(Pixmap),
    Failed,
    Skipped,
}

enum Msg {
    Page { gen: u64, page: u32, result: Result<Page, NetError> },
    Detail { id: String, result: Result<(Wall, Detail), NetError> },
    Picked(Result<Wall, NetError>),
    Thumb { key: String, res: ThumbRes },
    Downloaded { id: String, apply: bool, result: Result<PathBuf, NetError> },
}

struct Shared {
    client: RwLock<Arc<Client>>,
    dirs: Dirs,
    wanted: Mutex<HashSet<String>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    throttle: Arc<Mutex<Throttle>>,
    writes: AtomicUsize,
}

impl Shared {
    fn client(&self) -> Arc<Client> {
        self.client.read().map(|c| c.clone()).unwrap_or_else(|e| e.into_inner().clone())
    }
}

struct Engine {
    api: Sender<Job>,
    thumbs: Sender<ThumbJob>,
    out: Sender<Msg>,
    rx: Receiver<Msg>,
    shared: Arc<Shared>,
}

fn pick_index(len: usize, entropy: u64) -> usize {
    if len == 0 {
        0
    } else {
        (entropy % len as u64) as usize
    }
}

fn entropy() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    t ^ (u64::from(std::process::id()) << 32)
}

impl Engine {
    fn start(dirs: Dirs, key: Option<Secret>, wake: Arc<dyn Fn() + Send + Sync>) -> Engine {
        let throttle: Arc<Mutex<Throttle>> = Arc::default();
        let client = Arc::new(Client::new(client::api_base(), key, throttle.clone()));
        let shared = Arc::new(Shared { client: RwLock::new(client), dirs, wanted: Mutex::new(HashSet::new()), wake, throttle, writes: AtomicUsize::new(0) });
        let (out, rx) = channel::<Msg>();
        let (api, api_rx) = channel::<Job>();
        let (thumbs, thumb_rx) = channel::<ThumbJob>();
        {
            let (shared, out) = (shared.clone(), out.clone());
            std::thread::Builder::new()
                .name("wallhaven-api".into())
                .spawn(move || {
                    while let Ok(job) = api_rx.recv() {
                        let c = shared.client();
                        let msg = match job {
                            Job::Search { gen, page, query } => Msg::Page { gen, page, result: c.search(&query, page) },
                            Job::Detail { id } => Msg::Detail { result: c.detail(&id), id },
                            Job::Pick { query } => Msg::Picked(c.search(&query, 1).and_then(|p| {
                                let n = p.walls.len();
                                p.walls.into_iter().nth(pick_index(n, entropy())).ok_or_else(|| NetError::Bad("no wallpaper matched; loosen the filters".into()))
                            })),
                        };
                        if out.send(msg).is_err() {
                            return;
                        }
                        (shared.wake)();
                    }
                })
                .ok();
        }
        let thumb_rx = Arc::new(Mutex::new(thumb_rx));
        for n in 0..THUMB_WORKERS {
            let (shared, out, rx) = (shared.clone(), out.clone(), thumb_rx.clone());
            std::thread::Builder::new()
                .name(format!("wallhaven-thumb{n}"))
                .stack_size(1 << 20)
                .spawn(move || loop {
                    let job = match rx.lock() {
                        Ok(g) => g.recv(),
                        Err(_) => return,
                    };
                    let Ok(job) = job else { return };
                    let res = run_thumb(&shared, &job);
                    crate::mem::trim();
                    if out.send(Msg::Thumb { key: job.key, res }).is_err() {
                        return;
                    }
                    (shared.wake)();
                })
                .ok();
        }
        let dir = shared.dirs.thumb_dir();
        std::thread::spawn(move || {
            store::prune_thumbs(&dir, store::MAX_THUMB_FILES, store::MAX_THUMB_BYTES);
        });
        Engine { api, thumbs, out, rx, shared }
    }
}

fn run_thumb(shared: &Shared, job: &ThumbJob) -> ThumbRes {
    if !shared.wanted.lock().map(|w| w.contains(&job.key)).unwrap_or(false) {
        return ThumbRes::Skipped; // scrolled away while it waited in the queue
    }
    let bytes = match store::read_thumb(&shared.dirs, &job.id, job.large) {
        Some(b) => b,
        None => match shared.client().thumb(&job.url) {
            Ok(b) => {
                store::write_thumb(&shared.dirs, &job.id, job.large, &b);
                if shared.writes.fetch_add(1, Ordering::Relaxed) % 100 == 99 {
                    store::prune_thumbs(&shared.dirs.thumb_dir(), store::MAX_THUMB_FILES, store::MAX_THUMB_BYTES);
                }
                b
            }
            Err(_) => return ThumbRes::Failed,
        },
    };
    match crate::wallhaven::net::decode_thumb(&bytes, THUMB_W) {
        Ok(p) => ThumbRes::Ok(p),
        Err(_) => ThumbRes::Failed,
    }
}

// ---------------------------------------------------------------- state

#[derive(Default)]
struct Results {
    walls: Vec<Wall>,
    page: u32,
    last_page: u32,
    total: u64,
    gen: u64,
    loading: bool,
    seed: Option<String>,
    /// The search the pages belong to (so page 2 uses the filters of page 1).
    query: Option<Query>,
    error: Option<NetError>,
    /// Showing the saved results of an earlier session because the network failed.
    stale: bool,
}

impl Results {
    fn new_search(&mut self) -> u64 {
        self.gen += 1;
        self.walls.clear();
        self.page = 0;
        self.last_page = 1;
        self.total = 0;
        self.loading = false;
        self.seed = None;
        self.error = None;
        self.stale = false;
        self.gen
    }

    fn next_page(&self) -> Option<u32> {
        (!self.loading && self.query.is_some() && self.page < self.last_page && self.error.is_none() && !self.stale).then_some(self.page + 1)
    }

    /// Applies a reply; false when it belongs to an older search.
    fn apply(&mut self, gen: u64, page: u32, result: Result<Page, NetError>, cache: &dyn Fn() -> Vec<Wall>) -> bool {
        if gen != self.gen {
            return false;
        }
        self.loading = false;
        match result {
            Ok(p) => {
                self.error = None;
                self.stale = false;
                for w in p.walls {
                    if !self.walls.iter().any(|x| x.id == w.id) {
                        self.walls.push(w);
                    }
                }
                self.page = p.current.max(page);
                self.last_page = p.last_page;
                self.total = p.total;
                if p.seed.is_some() {
                    self.seed = p.seed;
                }
            }
            Err(e) => {
                if self.walls.is_empty() {
                    let old = cache();
                    if !old.is_empty() {
                        self.walls = old;
                        self.stale = true;
                    }
                }
                self.error = Some(e);
            }
        }
        true
    }
}

struct Dl {
    wall: Wall,
    apply: bool,
    progress: Arc<Progress>,
    finished: Arc<AtomicBool>,
}

pub struct Wh {
    pub tab: Tab,
    pub view: View,
    pub prefs: Prefs,
    key: Option<Secret>,
    pub filters_open: bool,
    /// Size of the screen (from the window system, else rdwm), 0x0 = unknown.
    pub screen: (u32, u32),
    dirs: Dirs,
    loaded: bool,
    engine: Option<Engine>,
    res: Results,
    sel: Option<Wall>,
    details: HashMap<String, Detail>,
    detail_due: Option<(Instant, String)>,
    detail_asked: HashSet<String>,
    favs: Vec<Wall>,
    curated_theme: String,
    curated_label: String,
    curated: Vec<curated::Choice>,
    curated_problem: Option<String>,
    curated_checks: HashMap<String, Result<(), String>>,
    dl: Option<Dl>,
    busy: Option<String>,
    search_due: Option<Instant>,
    thumbs: RefCell<Lru<String, Rc<Pixmap>>>,
    inflight: HashSet<String>,
    failed: HashMap<String, Instant>,
    scroll_top: bool,
}

impl Default for Wh {
    fn default() -> Self {
        Wh::with_dirs(Dirs::discover())
    }
}

impl Wh {
    pub fn with_dirs(dirs: Dirs) -> Wh {
        Wh {
            tab: Tab::Backgrounds,
            view: View::Browse,
            prefs: Prefs::default(),
            key: None,
            filters_open: false,
            screen: (0, 0),
            dirs,
            loaded: false,
            engine: None,
            res: Results::default(),
            sel: None,
            details: HashMap::new(),
            detail_due: None,
            detail_asked: HashSet::new(),
            favs: Vec::new(),
            curated_theme: String::new(),
            curated_label: String::new(),
            curated: vec![],
            curated_problem: None,
            curated_checks: HashMap::new(),
            dl: None,
            busy: None,
            search_due: None,
            thumbs: RefCell::new(Lru::new(MAX_THUMBS)),
            inflight: HashSet::new(),
            failed: HashMap::new(),
            scroll_top: false,
        }
    }

    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// Sketchy / NSFW switches are only live with the master switch **and** a key.
    pub fn purity(&self) -> [bool; 3] {
        purity_bits(&self.prefs, self.has_key())
    }

    fn load(&mut self) {
        if !self.loaded {
            self.prefs = store::load_prefs(&self.dirs);
            self.key = store::load_key(&self.dirs);
            self.favs = store::load_favourites(&self.dirs);
            self.loaded = true;
        }
    }

    fn query(&self) -> Query {
        Query::from_prefs(&self.prefs, self.has_key(), self.screen)
    }

    fn save_prefs(&self) -> Result<(), String> {
        store::save_prefs(&self.dirs, &self.prefs)
    }

    /// The wallpaper with `id` among everything the pane knows.
    fn find_wall(&self, id: &str) -> Option<&Wall> {
        self.res.walls.iter().chain(self.favs.iter()).chain(self.curated.iter().map(|c| &c.wall)).chain(self.sel.iter()).find(|w| w.id == id)
    }

    /// Items currently listed (browse results or favourites), purity-checked once more.
    fn listed(&self) -> Vec<&Wall> {
        let bits = self.purity();
        let src: Vec<&Wall> = match self.view {
            View::Favourites => self.favs.iter().collect(),
            View::Browse => self.res.walls.iter().collect(),
            View::Curated => self.curated.iter().map(|c| &c.wall).collect(),
        };
        src.into_iter().filter(|w| purity_allowed(&w.purity, bits)).collect()
    }

    pub fn is_favourite(&self, id: &str) -> bool {
        self.favs.iter().any(|w| w.id == id)
    }

    pub fn take_scroll_top(&mut self) -> bool {
        std::mem::take(&mut self.scroll_top)
    }

    // ------------------------------------------------------------ thumbnails

    /// Decoded thumbnail for a grid / preview id (`wh:<id>`, `wh:L:<id>`).
    pub fn thumb(&self, id: &str) -> Option<Rc<Pixmap>> {
        self.thumbs.borrow_mut().get(&id.to_string()).cloned()
    }

    pub fn cached_thumbs(&self) -> usize {
        self.thumbs.borrow().len()
    }

    fn split_thumb_id(id: &str) -> Option<(bool, &str)> {
        if let Some(rest) = id.strip_prefix("wh:L:") {
            Some((true, rest))
        } else {
            id.strip_prefix("wh:").map(|r| (false, r))
        }
    }

    /// The ids visible right now: fetch what is missing, forget what scrolled away.
    pub fn set_visible(&mut self, ids: &[String]) {
        let Some(e) = &self.engine else { return };
        if let Ok(mut w) = e.shared.wanted.lock() {
            *w = ids.iter().cloned().collect();
        }
        let now = Instant::now();
        self.failed.retain(|_, t| now.duration_since(*t) < RETRY_THUMB);
        for key in ids {
            if self.inflight.contains(key) || self.failed.contains_key(key) || self.thumbs.borrow().contains(key) {
                continue;
            }
            let Some((large, id)) = Self::split_thumb_id(key) else { continue };
            let Some(w) = self.find_wall(id) else { continue };
            let url = if large && !w.thumb_large.is_empty() { w.thumb_large.clone() } else { w.thumb_small.clone() };
            let job = ThumbJob { key: key.clone(), url, id: id.to_string(), large };
            if e.thumbs.send(job).is_ok() {
                self.inflight.insert(key.clone());
            }
        }
    }

    // ------------------------------------------------------------ searching

    fn submit(&self, job: Job) {
        if let Some(e) = &self.engine {
            let _ = e.api.send(job);
        }
    }

    fn start_search(&mut self) {
        self.search_due = None;
        let gen = self.res.new_search();
        let q = self.query();
        self.res.query = Some(q.clone());
        self.res.loading = true;
        self.submit(Job::Search { gen, page: 1, query: q });
    }

    fn fetch_next(&mut self) {
        if let Some(page) = self.res.next_page() {
            let mut q = self.res.query.clone().unwrap_or_else(|| self.query());
            if q.seed.is_none() {
                q.seed = self.res.seed.clone();
            }
            self.res.loading = true;
            let gen = self.res.gen;
            self.submit(Job::Search { gen, page, query: q });
        }
    }

    fn select(&mut self, id: &str) {
        let Some(w) = self.find_wall(id).cloned() else { return };
        if self.sel.as_ref().is_some_and(|s| s.id == id) {
            return;
        }
        if !self.details.contains_key(id) && !self.detail_asked.contains(id) {
            self.detail_due = Some((Instant::now() + DETAIL_DEBOUNCE, id.to_string()));
            self.wake_later(DETAIL_DEBOUNCE);
        }
        self.sel = Some(w);
        self.scroll_top = true;
    }

    fn wake_later(&self, d: Duration) {
        if let Some(e) = &self.engine {
            let wake = e.shared.wake.clone();
            std::thread::spawn(move || {
                std::thread::sleep(d + Duration::from_millis(30));
                wake();
            });
        }
    }

    fn schedule_search(&mut self, d: Duration) {
        self.search_due = Some(Instant::now() + d);
        self.wake_later(d);
    }
}

/// Called before the Wallpaper pane is painted with the Wallhaven tab shown.
pub fn prepare(m: &mut Model) {
    if m.wh.tab != Tab::Wallhaven {
        return;
    }
    m.wh.load();
    let source = super::themepane::source(m);
    let selection = source.selection();
    let id = source.find(&selection.name).map(|v| v.0).unwrap_or(selection.name);
    if m.wh.curated_theme != id {
        m.wh.curated_label = source.load_def(&id).map(|d| d.name).ok().filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
        m.wh.curated_theme = id.clone();
        match curated::choices(&id) {
            Ok(picks) => {
                m.wh.curated = picks;
                m.wh.curated_problem = None;
            }
            Err(error) => {
                m.wh.curated.clear();
                m.wh.curated_problem = Some(error);
            }
        }
        if m.wh.view == View::Curated && m.wh.sel.as_ref().is_some_and(|w| !m.wh.curated.iter().any(|c| c.wall.id == w.id)) {
            m.wh.sel = None;
            m.wh.detail_due = None;
        }
    }
    if m.wh.engine.is_none() {
        let wake = m.wake.clone();
        m.wh.engine = Some(Engine::start(m.wh.dirs.clone(), m.wh.key.clone(), wake));
    }
    if m.wh.screen.0 == 0 {
        if let Some(wa) = m.rt.state["monitors"][0]["workarea"].as_object() {
            let g = |k: &str| wa.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            m.wh.screen = (g("w"), g("h"));
        }
    }
    let wh = &mut m.wh;
    if wh.view == View::Browse && wh.res.query.is_none() {
        wh.start_search();
    }
}

/// Paint hook: asks for the next page when the end of the grid is close.
pub fn on_paint(m: &mut Model, total: f32, scroll: f32, viewport: f32) {
    if m.wh.tab == Tab::Wallhaven && m.wh.view == View::Browse && total - scroll - viewport < viewport * 0.3 {
        m.wh.fetch_next();
    }
}

/// Handles finished background work and due timers. True when the screen needs repainting.
pub fn pump(m: &mut Model) -> bool {
    let mut changed = false;
    let msgs: Vec<Msg> = match &m.wh.engine {
        Some(e) => e.rx.try_iter().collect(),
        None => return false,
    };
    for msg in msgs {
        changed = true;
        match msg {
            Msg::Page { gen, page, result } => {
                let dirs = m.wh.dirs.clone();
                let ok = result.is_ok();
                if m.wh.res.apply(gen, page, result, &|| store::load_last_search(&dirs)) && ok && page == 1 {
                    store::save_last_search(&m.wh.dirs, &m.wh.res.walls);
                }
            }
            Msg::Detail { id, result } => {
                if curated::known(&id) {
                    let checked = match &result {
                        Ok((wall, detail)) if wall.id == id => curated::validate_live(wall, detail),
                        Ok(_) => Err("Photo metadata did not match the selected image".into()),
                        Err(e) => Err(format!("Could not verify this recommendation: {}. Saved pictures remain available under Backgrounds.", e.friendly())),
                    };
                    m.wh.curated_checks.insert(id.clone(), checked);
                }
                if let Ok((wall, d)) = result {
                    if m.wh.sel.as_ref().is_some_and(|s| s.id == id) && wall.id == id {
                        m.wh.sel = Some(wall);
                    }
                    if m.wh.details.len() > 64 {
                        m.wh.details.clear();
                    }
                    m.wh.details.insert(id, d);
                }
            }
            Msg::Picked(result) => {
                m.wh.busy = None;
                match result {
                    Ok(w) => {
                        m.wh.sel = Some(w);
                        m.wh.scroll_top = true;
                        start_download(m, true);
                    }
                    Err(e) => m.say(e.friendly(), true),
                }
            }
            Msg::Thumb { key, res } => {
                m.wh.inflight.remove(&key);
                match res {
                    ThumbRes::Ok(px) => {
                        m.wh.thumbs.borrow_mut().insert(key, Rc::new(px));
                    }
                    ThumbRes::Failed => {
                        m.wh.failed.insert(key, Instant::now());
                    }
                    ThumbRes::Skipped => {}
                }
            }
            Msg::Downloaded { id, apply, result } => {
                if let Some(dl) = m.wh.dl.take() {
                    dl.finished.store(true, Ordering::Relaxed);
                }
                match result {
                    Ok(path) if apply => {
                        let p = path.to_string_lossy().into_owned();
                        super::panes::set_wallpaper(m, &p);
                    }
                    Ok(path) => m.say(format!("Saved {}", path.display()), false),
                    Err(e) => m.say(format!("{id}: {}", e.friendly()), true),
                }
            }
        }
    }
    let now = Instant::now();
    if m.wh.search_due.is_some_and(|d| now >= d) && m.wh.tab == Tab::Wallhaven && m.wh.view == View::Browse {
        m.wh.start_search();
        changed = true;
    }
    if let Some((due, id)) = m.wh.detail_due.clone() {
        if now >= due {
            m.wh.detail_due = None;
            if m.wh.sel.as_ref().is_some_and(|s| s.id == id) && m.wh.detail_asked.insert(id.clone()) {
                m.wh.submit(Job::Detail { id });
            }
        }
    }
    if m.wh.dl.is_some() {
        changed = true; // progress bar
    }
    changed
}

/// Starts downloading the selected wallpaper (and applying it when `apply`).
fn start_download(m: &mut Model, apply: bool) {
    let Some(wall) = m.wh.sel.clone() else { return };
    if m.wh.dl.is_some() {
        m.say("A download is already running", true);
        return;
    }
    let Some(e) = &m.wh.engine else { return };
    let (out, shared) = (e.out.clone(), e.shared.clone());
    let dir = m.paths.backgrounds();
    let bits = m.wh.purity();
    let progress = Arc::new(Progress::default());
    let finished = Arc::new(AtomicBool::new(false));
    m.wh.dl = Some(Dl { wall: wall.clone(), apply, progress: progress.clone(), finished: finished.clone() });
    {
        let (wake, finished) = (shared.wake.clone(), finished.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(150));
                wake();
            }
        });
    }
    std::thread::Builder::new()
        .name("wallhaven-download".into())
        .spawn(move || {
            let result = (|| {
                let current = if curated::known(&wall.id) {
                    let (live, detail) = shared.client().detail(&wall.id)?;
                    if live.id != wall.id {
                        return Err(NetError::Refused("Photo metadata did not match the selection".into()));
                    }
                    curated::validate_live(&live, &detail).map_err(NetError::Refused)?;
                    live
                } else {
                    wall.clone()
                };
                shared.client().download(&current, &dir, bits, &progress)
            })();
            let _ = out.send(Msg::Downloaded { id: wall.id.clone(), apply, result });
            (shared.wake)();
        })
        .ok();
}

pub fn follow_theme_notice(m: &Model) -> Option<Item> {
    if !crate::theme::wallpaper_follow::enabled(&m.paths) {
        return None;
    }
    let status = crate::theme::wallpaper_follow::status(&m.paths);
    (status.pending || status.error).then_some(Item::Note { text: status.message, warn: status.error })
}
pub fn follow_theme_items(m: &Model) -> Vec<Item> {
    let on = crate::theme::wallpaper_follow::enabled(&m.paths);
    let mut rows = vec![Row::new("Follow theme wallpaper")
        .detail("Use the matching curated Wallhaven photo when applying a theme or changing light/dark mode. Off keeps your own wallpaper. Applies static photo backgrounds; turn off live wallpapers first. Photos download in the background; theme previews never download them.")
        .ctl(Ctl::new("wh:follow-theme", Kind::Toggle(on)))];
    let status = crate::theme::wallpaper_follow::status(&m.paths);
    if on && status.error {
        rows.push(Row::new("Matching wallpaper needs attention").detail(&status.message).ctl(button("wh:follow-retry", "Retry", true)));
    } else if on && !status.message.is_empty() {
        rows.push(Row::new(if status.pending { "Preparing wallpaper…" } else { "Theme wallpaper" }).detail(&status.message));
    }
    vec![Item::Card(rows)]
}

// ---------------------------------------------------------------- controls

fn opt(l: &str, v: &str) -> Opt {
    (l.to_string(), v.to_string())
}

fn seg(id: &str, opts: &[(&str, &str)], cur: &str) -> Ctl {
    let o: Vec<Opt> = opts.iter().map(|(l, v)| opt(l, v)).collect();
    let sel = o.iter().position(|(_, v)| v == cur);
    Ctl::new(id, Kind::Seg(o, sel))
}

fn popup(id: &str, opts: &[(&str, &str)], cur: &str) -> Ctl {
    let o: Vec<Opt> = opts.iter().map(|(l, v)| opt(l, v)).collect();
    let sel = o.iter().position(|(_, v)| v == cur);
    Ctl::new(id, Kind::Popup(o, sel))
}

fn button(id: &str, label: &str, enabled: bool) -> Ctl {
    let mut c = Ctl::new(id, Kind::Button { label: label.into(), danger: false });
    c.enabled = enabled;
    c
}

fn chip(id: &str, label: &str, on: bool, enabled: bool) -> Ctl {
    let mut c = Ctl::new(id, Kind::Chip { label: label.into(), on });
    c.enabled = enabled;
    c
}

/// The "My backgrounds | Wallhaven" switch at the top of the pane.
pub fn tab_card(wh: &Wh) -> Item {
    let cur = match wh.tab {
        Tab::Wallhaven => "wallhaven",
        Tab::Live => "live",
        Tab::Backgrounds => "backgrounds",
    };
    Item::Card(vec![Row::new("Pictures from").ctl(seg("wh:tab", &[("My backgrounds", "backgrounds"), ("Wallhaven", "wallhaven"), ("Live wallpapers", "live")], cur))])
}

fn tidy_path(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

fn kind_label(t: &str) -> &'static str {
    match t {
        "image/png" => "PNG",
        "image/jpeg" => "JPEG",
        "image/webp" => "WebP",
        _ => "image",
    }
}

/// (headline, statistics) of the selected wallpaper.
fn sel_info(w: &Wall, d: Option<&Detail>) -> (String, String) {
    let favs = d.and_then(|d| d.favorites).unwrap_or(w.favorites);
    let mut stats = format!("{} favourites", short_count(favs));
    if let Some(v) = d.and_then(|d| d.views).or((w.views > 0).then_some(w.views)) {
        stats.push_str(&format!(" · {} views", short_count(v)));
    }
    (format!("{} · {} {}", w.resolution.replace('x', "×"), human_size(w.file_size), kind_label(&w.file_type)), stats)
}

/// Everything the Wallhaven tab shows.
pub fn items(m: &Model) -> Vec<Item> {
    build(&m.wh, &m.paths.backgrounds())
}

pub fn build(wh: &Wh, backgrounds: &std::path::Path) -> Vec<Item> {
    let p = &wh.prefs;
    let mut out = vec![tab_card(wh), Item::Space(6.0)];
    let faves = wh.favs.len();

    // search and sorting
    let mut rows = vec![
        Row::new("Search").ctl(Ctl::new("wh:q", Kind::Text { val: p.query.clone(), hint: "tags, e.g. mountains lake".into() })).ctl(button(
            "wh:filters",
            if wh.filters_open && wh.view != View::Curated { "Hide filters" } else { "Filters" },
            true,
        )),
        Row::new("Show").ctl(seg(
            "wh:view",
            &[("Browse", "browse"), (&format!("Favourites ({faves})"), "favourites"), ("For this theme", "curated")],
            match wh.view {
                View::Favourites => "favourites",
                View::Curated => "curated",
                View::Browse => "browse",
            },
        )),
    ];
    if wh.view == View::Curated {
        rows.remove(0);
    }
    if wh.view == View::Browse {
        let sorts: Vec<(&str, &str)> = Sort::ALL.iter().map(|s| (s.label(), s.api())).collect();
        rows.push(Row::new("Sort by").ctl(seg("wh:sort", &sorts, p.sort.api())));
        if p.sort == Sort::Toplist {
            rows.push(Row::new("Top of the last").ctl(seg("wh:range", &query::TOP_RANGES, &p.top_range)));
        }
    }
    if wh.view != View::Curated {
        rows.push(Row::new("Random wallpaper").detail("Pick one at random from the current search and set it").ctl(button("wh:random", "Set a random one", wh.dl.is_none() && wh.busy.is_none())));
    }
    out.push(Item::Card(rows));

    if wh.view == View::Curated {
        out.push(Item::Header(format!("Photographs for {}", wh.curated_label)));
        out.push(Item::Note {text:"Two individually chosen Wallhaven photographs, one for each colour mode. Select a picture to preview and download it. Changing a theme keeps your wallpaper; nothing is downloaded until you browse or choose it.".into(),warn:false});
        if let Some(error) = &wh.curated_problem {
            out.push(Item::Note { text: error.clone(), warn: true });
        }
    }
    // status
    if let Some(b) = &wh.busy {
        out.push(Item::Note { text: b.clone(), warn: false });
    }
    if let Some(e) = &wh.res.error {
        if wh.view == View::Browse {
            out.push(Item::Note { text: e.friendly(), warn: true });
            out.push(Item::Card(vec![Row::new("Search again").ctl(button("wh:retry", "Try again", true))]));
        }
    }
    if wh.res.stale && wh.view == View::Browse {
        out.push(Item::Note { text: "These are the results saved from your last search.".into(), warn: false });
    }

    // filters
    if wh.filters_open && wh.view != View::Curated {
        out.push(Item::Header("Filters".into()));
        let mut cat = Row::new("Categories");
        for (i, (_, label)) in query::CATEGORIES.iter().enumerate() {
            cat = cat.ctl(chip(&format!("wh:cat:{i}"), label, p.categories[i], true));
        }
        let mature_live = p.allow_mature && wh.has_key();
        let bits = wh.purity();
        let pur = Row::new("Content")
            .detail(if wh.has_key() { "Sketchy and NSFW also need the switch under Account" } else { "Safe for work only. Sketchy and NSFW need an API key (Account, below)" })
            .ctl(chip("wh:pur:0", "SFW", true, false))
            .ctl(chip("wh:pur:1", "Sketchy", bits[1], mature_live))
            .ctl(chip("wh:pur:2", "NSFW", bits[2], mature_live));
        let scr = if wh.screen.0 > 0 { format!("{}×{}", wh.screen.0, wh.screen.1) } else { "unknown".into() };
        out.push(Item::Card(vec![
            cat,
            pur,
            Row::new("Resolution").detail(format!("This screen: {scr}")).ctl(popup("wh:res", &query::RESOLUTIONS, &p.resolution)),
            Row::new("Shape").ctl(popup("wh:ratio", &query::RATIOS, &p.ratio)),
            Row::new("Colour").detail("Wallpapers dominated by one colour").ctl(Ctl::new("wh:lbl:color", Kind::Label(p.color.as_ref().map_or("any".to_string(), |c| format!("#{c}"))))),
        ]));
        let mut sw = vec![Swatch { id: "wh:color:none".into(), name: "Any colour".into(), color: (128, 128, 128), selected: p.color.is_none(), unset: true }];
        for c in query::COLORS {
            let rgb = parse::hex_rgb(&format!("#{c}")).unwrap_or((0, 0, 0));
            sw.push(Swatch { id: format!("wh:color:{c}"), name: format!("#{c}"), color: rgb, selected: p.color.as_deref() == Some(c), unset: false });
        }
        out.push(Item::Swatches(sw));
        out.push(Item::Header("Account".into()));
        let mut key_row = Row::new("API key")
            .detail(format!("Optional. Saved privately in {}", tidy_path(&wh.dirs.key_file())))
            .ctl(Ctl::new("wh:key:secret", Kind::Text { val: if wh.has_key() { "•".repeat(12) } else { String::new() }, hint: "paste your key, press Enter".into() }));
        if wh.has_key() {
            key_row = key_row.ctl(button("wh:keyrm", "Remove", true));
        }
        let mut allow = Ctl::new("wh:mature", Kind::Toggle(p.allow_mature && wh.has_key()));
        allow.enabled = wh.has_key();
        out.push(Item::Card(vec![key_row, Row::new("Allow sketchy and NSFW results").detail(if wh.has_key() { "Off by default" } else { "Needs an API key" }).ctl(allow)]));
        out.push(Item::Note {
            text: "Get a key at wallhaven.cc > Account > API. The key stays on this computer and is only sent to wallhaven.cc. Wallhaven allows 45 requests a minute; this pane keeps below that."
                .into(),
            warn: false,
        });
    }

    // the selected wallpaper
    if let Some(w) = &wh.sel {
        let d = wh.details.get(&w.id);
        out.push(Item::Header(format!("Selected · {}", w.id)));
        let allowed = purity_allowed(&w.purity, wh.purity());
        let checked = wh.curated_checks.get(&w.id);
        let verified = !curated::known(&w.id) || checked.is_some_and(Result::is_ok);
        let idle = wh.dl.is_none() && allowed && verified;
        if let Some(pick) = wh.curated.iter().find(|c| c.wall.id == w.id) {
            out.push(Item::Note { text: format!("{} choice · {}. {}\n{}", pick.mode, pick.subject, pick.rationale, pick.provenance), warn: false });
        }
        if !verified {
            out.push(Item::Note { text: checked.and_then(|r| r.as_ref().err()).cloned().unwrap_or_else(|| "Checking the current photo details before download…".into()), warn: checked.is_some() });
            if checked.is_some() {
                out.push(Item::Card(vec![Row::new("Check recommendation again").ctl(button("wh:verify", "Retry", wh.dl.is_none()))]));
            }
        }
        let is_fav = wh.is_favourite(&w.id);
        let (info, stats) = sel_info(w, d);
        let act = vec![
            Row::new(info)
                .detail(match &wh.dl {
                    Some(dl) if dl.wall.id == w.id => match dl.progress.fraction() {
                        Some(f) => format!("{} {:.0} %", if dl.apply { "Downloading to set it…" } else { "Downloading…" }, f * 100.0),
                        None => "Downloading…".into(),
                    },
                    _ if !allowed => "Not allowed by the content settings".to_string(),
                    _ => stats,
                })
                .ctl(button("wh:set", "Set as wallpaper", idle))
                .ctl(button("wh:dl", "Download only", idle))
                .ctl(button("wh:fav", if is_fav { "Remove favourite" } else { "Add to favourites" }, true)),
            Row::new("wallhaven.cc").detail(w.url.trim_start_matches("https://")).ctl(button("wh:open", "Open in browser", true)).ctl(button("wh:close", "Close", true)),
        ];
        let tags = match d {
            Some(d) if !d.tags.is_empty() => d.tags.join(", "),
            Some(_) => "no tags".into(),
            None => "loading…".into(),
        };
        out.push(Item::Card(act));
        out.push(Item::Preview { id: format!("wh:L:{}", w.id), caption: format!("{} · {}", w.resolution.replace('x', "×"), w.category) });
        let pal: Vec<Swatch> =
            w.colors.iter().enumerate().filter_map(|(i, c)| Some(Swatch { id: format!("wh:pal:{i}"), name: c.clone(), color: parse::hex_rgb(c)?, selected: false, unset: false })).collect();
        if !pal.is_empty() {
            out.push(Item::Swatches(pal));
        }
        out.push(Item::Note { text: format!("Tags: {tags}. Saved pictures go to {}.", tidy_path(backgrounds)), warn: false });
    }

    // the grid
    let listed = wh.listed();
    let head = match wh.view {
        View::Curated => format!("Dark and light choices ({})", listed.len()),
        View::Favourites => format!("Favourites ({})", listed.len()),
        View::Browse if wh.res.total > 0 && !wh.res.stale => format!("Results ({} of {})", listed.len(), wh.res.total),
        View::Browse => format!("Results ({})", listed.len()),
    };
    out.push(Item::Header(head));
    if listed.is_empty() {
        let msg = match wh.view {
            View::Curated => "No curated photographs for this custom or legacy theme. Use Browse to choose your own; your wallpaper stays unchanged.",
            View::Favourites => "No favourites yet. Select a wallpaper and choose Add to favourites.",
            View::Browse if wh.res.loading => "Loading…",
            View::Browse if wh.res.error.is_some() => "Nothing to show.",
            View::Browse => "No wallpapers match. Try fewer filters or other words.",
        };
        out.push(Item::Note { text: msg.into(), warn: false });
    } else {
        let thumbs: Vec<Thumb> = listed
            .iter()
            .map(|w| Thumb {
                id: format!("wh:{}", w.id),
                caption: wh
                    .curated
                    .iter()
                    .find(|c| c.wall.id == w.id)
                    .filter(|_| wh.view == View::Curated)
                    .map(|c| format!("{} · {}", if c.mode == "dark" { "Dark" } else { "Light" }, c.subject))
                    .unwrap_or_else(|| w.resolution.replace('x', "×")),
                selected: wh.sel.as_ref().is_some_and(|s| s.id == w.id),
            })
            .collect();
        out.push(Item::Thumbs(thumbs));
        if wh.view == View::Browse {
            if wh.res.loading {
                out.push(Item::Note { text: "Loading more…".into(), warn: false });
            } else if wh.res.page < wh.res.last_page && (wh.res.stale || wh.res.error.is_some()) {
                out.push(Item::Card(vec![Row::new("More wallpapers").ctl(button("wh:more", "Load more", wh.res.error.is_none()))]));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- applying control changes

fn flip(bits: &mut [bool; 3], i: usize) {
    bits[i] = !bits[i];
}

/// Handles every `wh:*` control and the grid thumbnails (`w:wh:<id>`).
pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    m.wh.load();
    m.dirty = true;
    // a click on a grid thumbnail arrives as ["w", "wh", id]
    if let ["w", "wh", id] = parts {
        m.wh.select(id);
        return;
    }
    let mut changed_filters = false;
    let mut delay = SEARCH_DEBOUNCE;
    match (parts, v) {
        (["wh", "follow-theme"], CtlVal::Bool(on)) => {
            match crate::theme::wallpaper_follow::set_enabled_async(&m.paths, on) {
                Ok(()) => m.say(if on { "Wallpaper follows theme; matching photo is being prepared" } else { "Wallpaper stays independent" }, false),
                Err(error) => m.say(error, true),
            }
            return;
        }
        (["wh", "follow-retry"], CtlVal::Click) => {
            match crate::theme::wallpaper_follow::set_enabled_async(&m.paths, true) {
                Ok(()) => m.say("Retrying matching wallpaper", false),
                Err(error) => m.say(error, true),
            }
            return;
        }
        (["wh", "curated"], CtlVal::Click) => {
            m.wh.tab = Tab::Wallhaven;
            m.wh.view = View::Curated;
            m.wh.search_due = None;
            m.sel.reset_scroll = true;
            return;
        }
        (["wh", "verify"], CtlVal::Click) => {
            if let Some(w) = &m.wh.sel {
                let id = w.id.clone();
                m.wh.detail_asked.remove(&id);
                m.wh.curated_checks.remove(&id);
                m.wh.detail_due = Some((Instant::now(), id));
                m.wh.wake_later(Duration::ZERO);
            }
            return;
        }
        (["wh", "tab"], CtlVal::Str(s)) => {
            m.wh.tab = match s.as_str() {
                "wallhaven" => Tab::Wallhaven,
                "live" => Tab::Live,
                _ => Tab::Backgrounds,
            };
            m.sel.reset_scroll = true;
            return;
        }
        (["wh", "view"], CtlVal::Str(s)) => {
            m.wh.view = match s.as_str() {
                "favourites" => View::Favourites,
                "curated" => View::Curated,
                _ => View::Browse,
            };
            if m.wh.view == View::Curated {
                m.wh.search_due = None;
            }
            if m.wh.view == View::Browse && m.wh.res.query.is_none() {
                m.wh.start_search();
            }
            return;
        }
        (["wh", "filters"], _) => {
            m.wh.filters_open = !m.wh.filters_open;
            return;
        }
        (["wh", "q"], CtlVal::Str(s)) => {
            m.wh.prefs.query = query::clean_query(&s);
            if m.wh.view != View::Browse {
                m.wh.view = View::Browse;
            }
            changed_filters = true;
            delay = Duration::ZERO;
        }
        (["wh", "sort"], CtlVal::Str(s)) => {
            if let Some(sort) = Sort::parse(&s) {
                m.wh.prefs.sort = sort;
                changed_filters = true;
            }
        }
        (["wh", "range"], CtlVal::Str(s)) => {
            m.wh.prefs.top_range = s;
            changed_filters = true;
        }
        (["wh", "cat", i], CtlVal::Click) => {
            if let Ok(i) = i.parse::<usize>() {
                if i < 3 {
                    let mut c = m.wh.prefs.categories;
                    flip(&mut c, i);
                    if c.iter().any(|x| *x) {
                        m.wh.prefs.categories = c;
                        changed_filters = true;
                    } else {
                        m.say("Keep at least one category on", true);
                    }
                }
            }
        }
        (["wh", "pur", i], CtlVal::Click) => {
            if !(m.wh.prefs.allow_mature && m.wh.has_key()) {
                m.say("Sketchy and NSFW need an API key and the switch under Account", true);
            } else {
                match *i {
                    "1" => m.wh.prefs.sketchy = !m.wh.prefs.sketchy,
                    "2" => m.wh.prefs.nsfw = !m.wh.prefs.nsfw,
                    _ => {}
                }
                changed_filters = true;
            }
        }
        (["wh", "mature"], CtlVal::Bool(b)) => {
            if b && !m.wh.has_key() {
                m.say("Add an API key first", true);
            } else {
                m.wh.prefs.allow_mature = b;
                if b && !m.wh.prefs.sketchy && !m.wh.prefs.nsfw {
                    m.wh.prefs.sketchy = true;
                }
                changed_filters = true;
            }
        }
        (["wh", "res"], CtlVal::Str(s)) => {
            m.wh.prefs.resolution = s;
            changed_filters = true;
        }
        (["wh", "ratio"], CtlVal::Str(s)) => {
            m.wh.prefs.ratio = s;
            changed_filters = true;
        }
        (["wh", "color", c], CtlVal::Click) => {
            m.wh.prefs.color = if *c == "none" || !query::valid_color(c) { None } else { Some((*c).to_string()) };
            changed_filters = true;
        }
        (["wh", "key", "secret"], CtlVal::Str(s)) => {
            if s.trim().is_empty() {
                return;
            }
            match store::save_key(&m.wh.dirs, &s) {
                Ok(()) => {
                    m.wh.key = store::load_key(&m.wh.dirs);
                    rebuild_client(m);
                    m.say("API key saved", false);
                    changed_filters = true;
                    delay = Duration::ZERO;
                }
                Err(e) => m.say(e, true),
            }
        }
        (["wh", "keyrm"], _) => {
            store::remove_key(&m.wh.dirs);
            m.wh.key = None;
            m.wh.prefs.allow_mature = false;
            rebuild_client(m);
            m.say("API key removed", false);
            changed_filters = true;
        }
        (["wh", "retry"], _) | (["wh", "more"], _) => {
            if m.wh.res.stale || m.wh.res.error.is_some() || m.wh.res.query.is_none() {
                if m.wh.res.page == 0 || m.wh.res.stale {
                    m.wh.start_search();
                } else {
                    m.wh.res.error = None;
                    m.wh.fetch_next();
                }
            } else {
                m.wh.fetch_next();
            }
            return;
        }
        (["wh", "random"], _) => {
            let mut q = m.wh.query();
            q.sort = Sort::Random;
            q.seed = None;
            m.wh.busy = Some("Picking a random wallpaper…".into());
            m.wh.submit(Job::Pick { query: q });
            return;
        }
        (["wh", "close"], _) => {
            m.wh.sel = None;
            return;
        }
        (["wh", "set"], _) => return start_download(m, true),
        (["wh", "dl"], _) => return start_download(m, false),
        (["wh", "fav"], _) => {
            if let Some(w) = m.wh.sel.clone() {
                let on = store::toggle_favourite(&mut m.wh.favs, &w);
                match store::save_favourites(&m.wh.dirs, &m.wh.favs) {
                    Ok(()) => m.say(if on { "Added to favourites" } else { "Removed from favourites" }, false),
                    Err(e) => m.say(format!("Favourites not saved: {e}"), true),
                }
            }
            return;
        }
        (["wh", "open"], _) => {
            if let Some(w) = &m.wh.sel {
                let url = if w.url.starts_with("https://wallhaven.cc/") { w.url.clone() } else { format!("https://wallhaven.cc/w/{}", w.id) };
                if !super::io::spawn_detached("xdg-open", &[&url]) {
                    m.say("Could not start xdg-open", true);
                }
            }
            return;
        }
        _ => return,
    }
    if changed_filters {
        if let Err(e) = m.wh.save_prefs() {
            m.say(format!("Filters not saved: {e}"), true);
        }
        if m.wh.view == View::Browse {
            m.wh.res.loading = false;
            m.wh.schedule_search(delay);
        }
    }
}

fn rebuild_client(m: &mut Model) {
    if let Some(e) = &m.wh.engine {
        let c = Arc::new(Client::new(client::api_base(), m.wh.key.clone(), e.shared.throttle.clone()));
        if let Ok(mut w) = e.shared.client.write() {
            *w = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse::tests::SEARCH;
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wh-pane-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn wh(name: &str) -> (Wh, PathBuf) {
        let root = tmp(name);
        let mut w = Wh::with_dirs(Dirs::under(&root));
        w.tab = Tab::Wallhaven;
        w.load();
        (w, root)
    }

    fn walls() -> Vec<Wall> {
        parse::parse_search(SEARCH, 1).unwrap().walls
    }

    fn page(n: usize, cur: u32, last: u32) -> Page {
        Page { walls: walls().into_iter().take(n).collect(), current: cur, last_page: last, total: 100, seed: None }
    }

    fn ctls(items: &[Item]) -> Vec<Ctl> {
        items
            .iter()
            .flat_map(|i| match i {
                Item::Card(rows) => rows.iter().flat_map(|r| r.ctls.clone()).collect::<Vec<_>>(),
                _ => vec![],
            })
            .collect()
    }

    fn ctl(items: &[Item], id: &str) -> Option<Ctl> {
        ctls(items).into_iter().find(|c| c.id == id)
    }

    #[test]
    fn paging_dedupes_and_ignores_stale_replies() {
        let mut r = Results::default();
        let gen = r.new_search();
        r.query = Some(Query::from_prefs(&Prefs::default(), false, (0, 0)));
        r.loading = true;
        assert!(r.apply(gen, 1, Ok(page(4, 1, 3)), &Vec::new));
        assert_eq!((r.walls.len(), r.page, r.last_page, r.next_page()), (4, 1, 3, Some(2)));
        r.loading = true;
        assert_eq!(r.next_page(), None, "no second request while one is in flight");
        assert!(r.apply(gen, 2, Ok(page(6, 2, 3)), &Vec::new));
        assert_eq!(r.walls.len(), 6, "the 4 repeated ids are not listed twice");
        // an answer to an older search is dropped
        let gen2 = r.new_search();
        assert!(!r.apply(gen, 3, Ok(page(6, 3, 3)), &Vec::new));
        assert!(r.walls.is_empty() && gen2 != gen);
        // the last page ends paging
        r.query = Some(Query::from_prefs(&Prefs::default(), false, (0, 0)));
        r.apply(gen2, 1, Ok(page(2, 1, 1)), &Vec::new);
        assert_eq!(r.next_page(), None);
    }

    #[test]
    fn offline_falls_back_to_the_saved_results_and_stops_paging() {
        let mut r = Results::default();
        let gen = r.new_search();
        r.query = Some(Query::from_prefs(&Prefs::default(), false, (0, 0)));
        r.loading = true;
        assert!(r.apply(gen, 1, Err(NetError::Offline("down".into())), &|| walls()));
        assert!(r.stale && r.walls.len() == 6 && r.error.is_some());
        assert_eq!(r.next_page(), None, "no paging against a dead network");
        // a later good answer replaces them
        let gen = r.new_search();
        assert!(!r.stale && r.walls.is_empty());
        r.apply(gen, 1, Ok(page(3, 1, 1)), &Vec::new);
        assert!(!r.stale && r.error.is_none() && r.walls.len() == 3);
        // failing after some results are showing keeps them, no cache swap
        let mut r2 = Results { gen: 5, ..Results::default() };
        r2.query = Some(Query::from_prefs(&Prefs::default(), false, (0, 0)));
        r2.apply(5, 1, Ok(page(2, 1, 4)), &Vec::new);
        r2.apply(5, 2, Err(NetError::RateLimited(30)), &|| walls());
        assert_eq!((r2.walls.len(), r2.stale), (2, false));
        assert_eq!(r2.next_page(), None, "stays quiet after a 429 until the user retries");
    }

    #[test]
    fn rows_without_a_key_cannot_enable_mature_content() {
        let (mut w, root) = wh("gate");
        w.filters_open = true;
        w.prefs.allow_mature = true;
        w.prefs.sketchy = true;
        w.prefs.nsfw = true;
        let items = build(&w, std::path::Path::new("/bg"));
        for id in ["wh:pur:1", "wh:pur:2", "wh:mature"] {
            assert!(!ctl(&items, id).unwrap().enabled, "{id} needs a key");
        }
        assert!(matches!(ctl(&items, "wh:pur:2").unwrap().kind, Kind::Chip { on: false, .. }), "ticked in the file but not live without a key");
        assert_eq!(w.purity(), [true, false, false]);
        assert!(!ctl(&items, "wh:pur:0").unwrap().enabled, "SFW is not a choice");
        // with a key they come alive
        store::save_key(&w.dirs, "AbCdEfGh12345678AbCdEfGh12345678").unwrap();
        w.key = store::load_key(&w.dirs);
        let items = build(&w, std::path::Path::new("/bg"));
        assert!(ctl(&items, "wh:mature").unwrap().enabled && ctl(&items, "wh:pur:2").unwrap().enabled);
        assert_eq!(w.purity(), [true, true, true]);
        // the key is never part of what is drawn
        assert!(!format!("{items:?}").contains("AbCdEfGh"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_selected_block_has_the_actions_and_the_grid_marks_the_selection() {
        let (mut w, root) = wh("sel");
        w.res.walls = walls();
        w.res.total = 514;
        w.select("lygg6r");
        assert!(w.take_scroll_top() && !w.take_scroll_top());
        let items = build(&w, std::path::Path::new("/bg"));
        for id in ["wh:set", "wh:dl", "wh:fav", "wh:open"] {
            assert!(ctl(&items, id).is_some(), "{id}");
        }
        assert!(items.iter().any(|i| matches!(i, Item::Preview { id, .. } if id == "wh:L:lygg6r")));
        assert!(items.iter().any(|i| matches!(i, Item::Swatches(s) if s.len() == 5 || !s.is_empty())), "palette swatches");
        let grid = items.iter().find_map(|i| if let Item::Thumbs(t) = i { Some(t) } else { None }).unwrap();
        assert_eq!(grid.len(), 6);
        assert_eq!(grid.iter().filter(|t| t.selected).map(|t| t.id.as_str()).collect::<Vec<_>>(), ["wh:lygg6r"]);
        assert!(items.iter().any(|i| matches!(i, Item::Header(h) if h.starts_with("Results (6 of 514)"))));
        // downloading disables the buttons
        w.dl = Some(Dl { wall: w.sel.clone().unwrap(), apply: true, progress: Arc::new(Progress::default()), finished: Arc::default() });
        let items = build(&w, std::path::Path::new("/bg"));
        assert!(!ctl(&items, "wh:set").unwrap().enabled && !ctl(&items, "wh:dl").unwrap().enabled);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn favourites_view_lists_the_local_list() {
        let (mut w, root) = wh("favs");
        w.view = View::Favourites;
        let items = build(&w, std::path::Path::new("/bg"));
        assert!(items.iter().any(|i| matches!(i, Item::Note { text, .. } if text.contains("No favourites yet"))));
        w.favs = walls().into_iter().take(2).collect();
        let items = build(&w, std::path::Path::new("/bg"));
        let grid = items.iter().find_map(|i| if let Item::Thumbs(t) = i { Some(t.len()) } else { None });
        assert_eq!(grid, Some(2));
        assert!(w.is_favourite("d8vvlo"));
        // a favourite that is sketchy is hidden while sketchy is off
        w.favs[0].purity = "sketchy".into();
        assert_eq!(w.listed().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn errors_show_a_friendly_note_and_a_retry() {
        let (mut w, root) = wh("err");
        w.res.error = Some(NetError::RateLimited(42));
        let items = build(&w, std::path::Path::new("/bg"));
        assert!(items.iter().any(|i| matches!(i, Item::Note { text, warn: true } if text.contains("42 s"))));
        assert!(ctl(&items, "wh:retry").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thumbnail_memory_is_bounded() {
        let (w, root) = wh("lru");
        for i in 0..(MAX_THUMBS * 3) {
            w.thumbs.borrow_mut().insert(format!("wh:{i}"), Rc::new(Pixmap::new(220, 124).unwrap()));
        }
        assert_eq!(w.cached_thumbs(), MAX_THUMBS);
        assert!(w.thumb("wh:0").is_none() && w.thumb(&format!("wh:{}", MAX_THUMBS * 3 - 1)).is_some());
        const { assert!(MAX_THUMBS * 240 * 1024 < 40 * 1024 * 1024, "worst case stays well under 40 MB") };
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thumb_ids_split_into_size_and_wallpaper_id() {
        assert_eq!(Wh::split_thumb_id("wh:abc123"), Some((false, "abc123")));
        assert_eq!(Wh::split_thumb_id("wh:L:abc123"), Some((true, "abc123")));
        assert_eq!(Wh::split_thumb_id("/home/x/a.png"), None);
    }

    #[test]
    fn random_pick_is_always_in_range() {
        for e in [0u64, 1, 7, u64::MAX, 123456789] {
            assert!(pick_index(5, e) < 5);
        }
        assert_eq!(pick_index(0, 99), 0);
    }

    #[test]
    fn control_changes_update_and_persist_prefs() {
        let root = tmp("ctl");
        let (mut m, _) = super::super::model::tests_support::model_with("");
        m.wh = Wh::with_dirs(Dirs::under(&root));
        m.wh.tab = Tab::Wallhaven;
        apply(&mut m, &["wh", "sort"], CtlVal::Str("random".into()));
        apply(&mut m, &["wh", "cat", "0"], CtlVal::Click);
        apply(&mut m, &["wh", "color", "0066cc"], CtlVal::Click);
        apply(&mut m, &["wh", "q"], CtlVal::Str("  big   mountain\n".into()));
        let p = store::load_prefs(&m.wh.dirs);
        assert_eq!((p.sort, p.categories, p.color.as_deref(), p.query.as_str()), (Sort::Random, [false, true, true], Some("0066cc"), "big mountain"));
        // the last category cannot be switched off
        apply(&mut m, &["wh", "cat", "1"], CtlVal::Click);
        apply(&mut m, &["wh", "cat", "2"], CtlVal::Click);
        assert_eq!(m.wh.prefs.categories, [false, false, true]);
        assert_eq!(m.notice.as_ref().map(|n| n.error), Some(true));
        // mature content: refused without a key
        apply(&mut m, &["wh", "mature"], CtlVal::Bool(true));
        assert!(!m.wh.prefs.allow_mature);
        apply(&mut m, &["wh", "pur", "2"], CtlVal::Click);
        assert!(!m.wh.prefs.nsfw);
        // saving a key turns the switch available, and removing it turns everything off again
        apply(&mut m, &["wh", "key", "secret"], CtlVal::Str("AbCdEfGh12345678AbCdEfGh12345678".into()));
        assert!(m.wh.has_key());
        apply(&mut m, &["wh", "mature"], CtlVal::Bool(true));
        assert!(m.wh.prefs.allow_mature && m.wh.purity()[1]);
        apply(&mut m, &["wh", "keyrm"], CtlVal::Click);
        assert!(!m.wh.has_key() && !m.wh.prefs.allow_mature && m.wh.purity() == [true, false, false]);
        assert!(!std::fs::read_to_string(m.wh.dirs.prefs_file()).unwrap().contains("AbCd"));
        // a junk key is refused
        apply(&mut m, &["wh", "key", "secret"], CtlVal::Str("not a key!".into()));
        assert!(!m.wh.has_key());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn favouriting_through_the_pane_writes_the_state_file() {
        let root = tmp("fav-ctl"); // not "ctl": two tests sharing a directory delete each other's files
        let (mut m, _) = super::super::model::tests_support::model_with("");
        m.wh = Wh::with_dirs(Dirs::under(&root));
        m.wh.tab = Tab::Wallhaven;
        m.wh.res.walls = walls();
        apply(&mut m, &["w", "wh", "w53xlp"], CtlVal::Click);
        assert_eq!(m.wh.sel.as_ref().map(|w| w.id.as_str()), Some("w53xlp"));
        apply(&mut m, &["wh", "fav"], CtlVal::Click);
        assert_eq!(store::load_favourites(&m.wh.dirs).iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["w53xlp"]);
        apply(&mut m, &["wh", "fav"], CtlVal::Click);
        assert!(store::load_favourites(&m.wh.dirs).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn setting_a_downloaded_wallpaper_uses_the_wallpaper_panes_path() {
        let (mut m, cfg) = super::super::model::tests_support::model_with("[wallpaper]\nmode = \"fit\"\n");
        m.walls = Some(vec![]);
        super::super::panes::set_wallpaper(&mut m, "/home/x/backgrounds/wallhaven-d8vvlo.png");
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("image = \"/home/x/backgrounds/wallhaven-d8vvlo.png\""), "{text}");
        assert!(text.contains("mode = \"fit\""), "the rest of the section is untouched");
        assert!(m.walls.is_none(), "the Backgrounds list is rebuilt so the new file shows up");
        // the Wallpaper pane's own click goes through the same function
        super::super::panes::apply_special(&mut m, "w:/a/b.png", CtlVal::Click);
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("/a/b.png"));
    }

    #[test]
    fn tabs_switch() {
        let root = tmp("tabs");
        let (mut m, _) = super::super::model::tests_support::model_with("");
        m.wh = Wh::with_dirs(Dirs::under(&root));
        apply(&mut m, &["wh", "tab"], CtlVal::Str("wallhaven".into()));
        assert_eq!(m.wh.tab, Tab::Wallhaven);
        apply(&mut m, &["wh", "tab"], CtlVal::Str("backgrounds".into()));
        assert_eq!(m.wh.tab, Tab::Backgrounds);
        let _ = std::fs::remove_dir_all(root);
    }
}
