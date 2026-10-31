//! Durable Settings writes on one owner thread, so the window never waits for the disk.
//!
//! The UI turns every accepted change into an immutable [`Job`] and hands it to the [`Persister`]: it neither runs
//! `rdwmctl check-config` nor writes, fsyncs or renames. One worker thread owns every Settings write and runs the
//! jobs strictly in submission order; each result comes back as an [`io::Ev::Persisted`] on the window's event queue,
//! in the same order. Until then the window shows the change as pending ("Saving…") and never claims it is saved.
//!
//! Jobs are deltas, not file snapshots, so a later job never undoes an earlier one that has not committed yet:
//! * [`Job::Toml`] carries a [`TomlPatch`], the keys one edit changed, computed on the UI from the shown document.
//!   The worker applies it to the file as it is when the job runs (after every earlier job), validates the result
//!   like rdwm does and replaces the file atomically. An external edit since this process last read or wrote the
//!   file fails the job ("changed on disk") instead of being overwritten, exactly like the synchronous store did.
//! * [`Job::Json`] sets or removes one top-level key of a JSON file, keeping every other key.
//! * [`Job::Theme`] plans *and* executes a theme package transaction in the worker, so the plan sees the files as
//!   the earlier jobs left them; the engine's own undo record and rollback keep the multi-file change atomic.
//!
//! Coalescing happens only where it cannot change the result: a queued JSON set of the same file and key with no
//! other job for that file in between is replaced in place by the newer value (setting k=a then k=b leaves the
//! file exactly as setting k=b does). TOML patches and theme transactions are never merged: their validation and
//! planning depend on what ran before them. The queue is bounded ([`CAPACITY`]); when it is full a new change is
//! refused visibly rather than dropped.
//!
//! Dropping the [`Persister`] stops accepting jobs, lets the worker finish everything already queued (the newest
//! edit included) and joins it. That happens when the window is gone, never from an input or paint handler; a
//! disk that stays stuck longer than [`DRAIN_LIMIT`] is logged and the remaining jobs are reported lost.

use super::io::{self, Ev, Paths, Queue, Stamp};
use crate::theme::engine::apply::{self as engine, Applied, WidgetChoice};
use crate::theme::live::Source;
use crate::theme::model::Selection;
use crate::theme::style::Style;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use toml_edit::{DocumentMut, Item, Table};

/// Jobs that may wait for the worker at once.
pub const CAPACITY: usize = 256;
/// How long dropping the persister waits for queued jobs to reach the disk.
pub const DRAIN_LIMIT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------- TOML patches

/// The keys one edit changed in `config.toml`: `Some(item)` sets the key (tables are merged key by key into an
/// existing table), `None` removes it. Arrays of tables (`[[bind]]`, `[[rule]]`) are replaced as a whole.
#[derive(Clone, Default)]
pub struct TomlPatch {
    ops: Vec<(Vec<String>, Option<Item>)>,
}

impl std::fmt::Debug for TomlPatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<String> = self.ops.iter().map(|(p, v)| format!("{}{}", if v.is_some() { "set " } else { "remove " }, p.join("."))).collect();
        f.debug_struct("TomlPatch").field("ops", &keys).finish()
    }
}

fn render(item: &Item) -> String {
    let mut d = DocumentMut::new();
    d.as_table_mut().insert("k", item.clone());
    d.to_string()
}

fn is_plain_table(i: &Item) -> bool {
    matches!(i, Item::Table(_))
}

fn clear_positions(item: &mut Item) {
    if let Item::Table(t) = item {
        t.set_position(None);
        for (_, child) in t.iter_mut() {
            clear_positions(child);
        }
    }
}

impl TomlPatch {
    /// The difference between two renderings of the document (`before` is what the edit started from).
    pub fn between(before: &str, after: &str) -> Result<TomlPatch, String> {
        let a: DocumentMut = before.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
        let b: DocumentMut = after.parse().map_err(|e: toml_edit::TomlError| format!("invalid TOML: {e}"))?;
        let mut ops = Vec::new();
        diff_table(&mut Vec::new(), a.as_table(), b.as_table(), &mut ops);
        Ok(TomlPatch { ops })
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Applies the patch to `text` (the file as it is now) and renders the result.
    pub fn apply_to(&self, text: &str) -> Result<String, String> {
        let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| format!("config.toml is not valid TOML right now: {e}"))?;
        for (path, op) in &self.ops {
            apply_op(doc.as_table_mut(), path, op.as_ref())?;
        }
        Ok(doc.to_string())
    }
}

fn diff_table(path: &mut Vec<String>, a: &Table, b: &Table, out: &mut Vec<(Vec<String>, Option<Item>)>) {
    for (k, bi) in b.iter() {
        match a.get(k) {
            Some(ai) if is_plain_table(ai) && is_plain_table(bi) => {
                path.push(k.to_string());
                diff_table(path, ai.as_table().expect("table"), bi.as_table().expect("table"), out);
                path.pop();
            }
            Some(ai) if render(ai) == render(bi) => {}
            _ => {
                let mut p = path.clone();
                p.push(k.to_string());
                let mut item = bi.clone();
                if a.get(k).is_none() {
                    // A new table renders after its predecessor in whatever document it lands in.
                    clear_positions(&mut item);
                }
                out.push((p, Some(item)));
            }
        }
    }
    for (k, _) in a.iter() {
        if b.get(k).is_none() {
            let mut p = path.clone();
            p.push(k.to_string());
            out.push((p, None));
        }
    }
}

fn apply_op(root: &mut Table, path: &[String], op: Option<&Item>) -> Result<(), String> {
    let (last, parents) = path.split_last().ok_or("empty key path")?;
    let mut t = root;
    for part in parents {
        if !t.contains_key(part) {
            if op.is_none() {
                return Ok(()); // already gone
            }
            let mut n = Table::new();
            n.set_implicit(true);
            t.insert(part, Item::Table(n));
        }
        t = t.get_mut(part).and_then(Item::as_table_mut).ok_or_else(|| format!("`{part}` is not a table in config.toml"))?;
    }
    match op {
        None => {
            t.remove(last);
        }
        Some(Item::Table(new)) if t.get(last).is_some_and(is_plain_table) => {
            // Someone (a theme) created the same table meanwhile: merge, keeping their other keys.
            let into = t.get_mut(last).and_then(Item::as_table_mut).expect("table");
            for (k, v) in new.iter() {
                apply_op(into, &[k.to_string()], Some(v))?;
            }
        }
        Some(item) => match t.get_mut(last) {
            Some(slot) => *slot = item.clone(),
            None => {
                t.insert(last, item.clone());
            }
        },
    }
    Ok(())
}

// ---------------------------------------------------------------- jobs and results

/// What a theme job runs (the engine plans it against the files as they are when the job starts).
#[derive(Debug, Clone)]
pub enum ThemeRequest {
    /// A changed selection of the current or another design (colours preserved): `apply_selection`.
    Selection(Selection),
    /// A complete package application (its own colour scheme): `apply`.
    Package { name: String, base: Selection },
    /// After the widget-layout question was answered.
    Widgets { selection: Selection, reset_colors: bool, choice: WidgetChoice },
}

#[derive(Debug, Clone)]
pub struct ThemeJob {
    pub src: Source,
    pub paths: Paths,
    pub request: ThemeRequest,
    /// Shell style (dock and bar keys) set after the transaction, from the files the transaction wrote.
    pub style: Option<Style>,
}

/// One immutable unit of work. Everything it needs is inside it; the worker never looks at the window's state.
#[derive(Debug, Clone)]
pub enum Job {
    Toml {
        path: PathBuf,
        rdwmctl: PathBuf,
        patch: TomlPatch,
        /// The window's [`io::Store::epoch`] and saved stamp when it computed the patch.
        epoch: u64,
        base: Stamp,
    },
    Json {
        path: PathBuf,
        key: String,
        value: Option<Value>,
        /// Afterwards let the dock follow `dock.visible` and the theme in this `decebal.json`.
        reconcile: Option<PathBuf>,
    },
    Theme(Box<ThemeJob>),
}

impl Job {
    fn touches(&self, p: &Path) -> bool {
        match self {
            Job::Toml { path, .. } | Job::Json { path, .. } => path == p,
            // A package transaction may write any of the files Settings owns.
            Job::Theme(_) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TomlSaved {
    pub text: String,
    pub stamp: Stamp,
    /// False when the patch left the file as it was (nothing was written).
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// The file changed on disk behind this process: reload it, nothing was written.
    pub conflict: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Toml(Result<TomlSaved, Failure>),
    /// The committed JSON document.
    Json(Result<Value, String>),
    Theme(Result<Applied, String>),
}

/// A finished job. `seq` is the ticket [`Persister::submit`] returned (the newest one for coalesced JSON sets).
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub seq: u64,
    pub path: PathBuf,
    pub key: Option<String>,
    pub style: Option<Style>,
    pub outcome: Outcome,
    /// Time the job spent in the worker (validation, writes, fsyncs).
    pub took: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submitted {
    Queued(u64),
    /// Replaced the queued set of the same JSON key `replaced`; its result is this ticket's.
    Merged { seq: u64, replaced: u64 },
}

impl Submitted {
    pub fn seq(&self) -> u64 {
        match self {
            Submitted::Queued(s) | Submitted::Merged { seq: s, .. } => *s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// [`CAPACITY`] jobs are already waiting.
    Full,
    /// The window is closing.
    Closed,
}

// ---------------------------------------------------------------- the worker

#[derive(Default)]
struct FileState {
    epoch: u64,
    /// The file's identity after this process's last read or write; anything else is an external edit.
    known: Option<Stamp>,
    /// An external edit was found at this epoch: later jobs computed before the window reloaded fail too.
    conflicted: bool,
}

/// Runs jobs; owns what it knows about the files. Used by the worker thread (and inline by headless tests).
#[derive(Default)]
struct Runner {
    files: HashMap<PathBuf, FileState>,
}

impl Runner {
    fn run(&mut self, seq: u64, job: Job) -> Done {
        let t0 = Instant::now();
        let (path, key, style) = match &job {
            Job::Toml { path, .. } => (path.clone(), None, None),
            Job::Json { path, key, .. } => (path.clone(), Some(key.clone()), None),
            Job::Theme(t) => (t.paths.shell_json(), None, t.style),
        };
        let outcome = match job {
            Job::Toml { path, rdwmctl, patch, epoch, base } => Outcome::Toml(self.toml(&path, &rdwmctl, &patch, epoch, base)),
            Job::Json { path, key, value, reconcile: rec } => Outcome::Json(json(&path, &key, value, rec.as_deref())),
            Job::Theme(t) => Outcome::Theme(self.theme(*t)),
        };
        let took = t0.elapsed();
        crate::log_debug!("persist #{seq} {} done in {} ms: {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), took.as_millis(), summary(&outcome));
        Done { seq, path, key, style, outcome, took }
    }

    fn toml(&mut self, path: &Path, rdwmctl: &Path, patch: &TomlPatch, epoch: u64, base: Stamp) -> Result<TomlSaved, Failure> {
        let st = self.files.entry(path.to_path_buf()).or_default();
        let conflict = || Failure { conflict: true, message: "config.toml changed on disk; reloaded it, please repeat the change".into() };
        let now = io::stamp(path);
        if epoch > st.epoch {
            // The window (re)read the file. What it read is known, and so is what we wrote ourselves since
            // (its read may have raced with our own queued writes); anything else is still an external edit.
            st.epoch = epoch;
            st.conflicted = false;
            if now == base {
                st.known = Some(now);
            }
        } else if epoch < st.epoch || st.conflicted {
            return Err(conflict());
        }
        if st.known != Some(now) {
            crate::log_warn!("{} changed on disk since it was read; not overwriting", path.display());
            st.conflicted = true;
            return Err(conflict());
        }
        let fail = |message: String| Failure { conflict: false, message };
        let old = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(fail(format!("{}: {e}", path.display()))),
        };
        let text = patch.apply_to(&old).map_err(fail)?;
        if text == old {
            return Ok(TomlSaved { text, stamp: now, changed: false });
        }
        if let Err(e) = io::validate(rdwmctl, path, &text) {
            crate::log_warn!("rdwm config rejected, nothing written: {e}");
            return Err(fail(e));
        }
        io::write_atomic(path, &text).map_err(fail)?;
        crate::log_info!("rdwm config saved: {}", path.display());
        let stamp = io::stamp(path);
        st.known = Some(stamp);
        Ok(TomlSaved { text, stamp, changed: true })
    }

    fn theme(&mut self, t: ThemeJob) -> Result<Applied, String> {
        let ThemeJob { src, paths, request, style } = t;
        let plan = match &request {
            ThemeRequest::Selection(sel) => engine::plan_selection(&src, &paths, sel),
            ThemeRequest::Package { name, base } => engine::plan(&src, &paths, name, base),
            ThemeRequest::Widgets { selection, reset_colors: true, choice } => engine::plan_with_widgets(&src, &paths, &selection.name, selection, *choice),
            ThemeRequest::Widgets { selection, reset_colors: false, choice } => engine::plan_selection_with_widgets(&src, &paths, selection, *choice),
        }
        .inspect_err(|e| crate::log_warn!("theme not applied: {e}"))?;
        let toml_written = plan.writes.iter().any(|w| w.path == paths.rdwm_config);
        let applied = engine::execute(&src, &paths, plan).inspect_err(|e| crate::log_error!("theme apply failed: {e}"))?;
        if toml_written {
            // Our own write: not an external edit for TOML jobs queued behind this one.
            self.files.entry(paths.rdwm_config.clone()).or_default().known = Some(io::stamp(&paths.rdwm_config));
        }
        if let Some(st) = style {
            apply_style(&paths, st).map_err(|e| format!("theme applied, shell style not saved: {e}"))?;
        }
        crate::log_info!("theme `{}` applied: files {:?}, rdwm reloaded {}, undo `{}`", applied.theme_id, applied.files, applied.reloaded, applied.undo_id);
        Ok(applied)
    }
}

fn summary(o: &Outcome) -> String {
    match o {
        Outcome::Toml(Ok(s)) => (if s.changed { "saved" } else { "unchanged" }).into(),
        Outcome::Json(Ok(_)) | Outcome::Theme(Ok(_)) => "saved".into(),
        Outcome::Toml(Err(f)) => format!("failed ({})", if f.conflict { "conflict" } else { "error" }),
        Outcome::Json(Err(_)) | Outcome::Theme(Err(_)) => "failed".into(),
    }
}

fn reconcile(shell: &Path) {
    if let Err(e) = crate::dock_lifecycle::reconcile(shell) {
        crate::log_warn!("dock: {e}");
    }
}

fn json(path: &Path, key: &str, value: Option<Value>, rec: Option<&Path>) -> Result<Value, String> {
    io::json_set(path, key, value)?;
    if let Some(shell) = rec {
        reconcile(shell);
    }
    Ok(io::json_load(path))
}

/// The dock and bar keys of a shell style, computed from the files the theme transaction just wrote.
fn apply_style(paths: &Paths, st: Style) -> Result<(), String> {
    use crate::theme::style;
    for (key, val) in style::dock_edits(st) {
        io::json_set(&paths.dock_settings(), key, val)?;
    }
    let shell = io::json_load(&paths.shell_json());
    io::json_set(&paths.shell_json(), "bar", Some(style::bar_value(st, shell.get("bar"))))?;
    let shell = io::json_load(&paths.shell_json());
    io::json_set(&paths.shell_json(), "dock", Some(style::dock_value(st, shell.get("dock"))))?;
    reconcile(&paths.shell_json());
    Ok(())
}

// ---------------------------------------------------------------- the handle

#[derive(Default)]
struct State {
    jobs: VecDeque<(u64, Job)>,
    next: u64,
    running: bool,
    closing: bool,
    finished: bool,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

enum Mode {
    Thread { shared: Arc<Shared>, handle: Option<std::thread::JoinHandle<()>> },
    /// Headless tests and tools: each job runs on the caller's thread as it is submitted.
    Inline { runner: Runner, next: u64, done: Vec<Done> },
}

/// The window's handle on the persistence worker.
pub struct Persister {
    mode: Mode,
}

impl Persister {
    /// Starts the worker. Results go to `queue` followed by `wake()`.
    pub fn spawn(queue: Queue, wake: Arc<dyn Fn() + Send + Sync>) -> Persister {
        Self::spawn_inner(queue, wake, None)
    }

    #[cfg(test)]
    pub fn spawn_with_sync_hook(queue: Queue, wake: Arc<dyn Fn() + Send + Sync>, hook: Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>) -> Persister {
        Self::spawn_inner(queue, wake, Some(hook))
    }

    #[allow(unused_variables)]
    fn spawn_inner(queue: Queue, wake: Arc<dyn Fn() + Send + Sync>, hook: Option<Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>>) -> Persister {
        let shared = Arc::new(Shared { state: Mutex::new(State { next: 1, ..State::default() }), cv: Condvar::new() });
        let s = shared.clone();
        let handle = std::thread::Builder::new()
            .name("decset-persist".into())
            .spawn(move || {
                #[cfg(test)]
                io::fault::set_thread_hook(hook);
                let mut runner = Runner::default();
                loop {
                    let (seq, job) = {
                        let mut st = s.state.lock().unwrap();
                        loop {
                            if let Some(j) = st.jobs.pop_front() {
                                st.running = true;
                                break j;
                            }
                            if st.closing {
                                st.finished = true;
                                s.cv.notify_all();
                                return;
                            }
                            st = s.cv.wait(st).unwrap();
                        }
                    };
                    let done = runner.run(seq, job);
                    queue.lock().unwrap().push_back(Ev::Persisted(Box::new(done)));
                    wake();
                    s.state.lock().unwrap().running = false;
                    s.cv.notify_all();
                }
            })
            .expect("persistence worker thread");
        Persister { mode: Mode::Thread { shared, handle: Some(handle) } }
    }

    /// Runs every job on the submitting thread; results are collected by [`Persister::take_inline`].
    pub fn inline() -> Persister {
        Persister { mode: Mode::Inline { runner: Runner::default(), next: 1, done: Vec::new() } }
    }

    pub fn is_inline(&self) -> bool {
        matches!(self.mode, Mode::Inline { .. })
    }

    /// Queues `job` behind every job submitted before it.
    pub fn submit(&mut self, job: Job) -> Result<Submitted, Refused> {
        match &mut self.mode {
            Mode::Inline { runner, next, done } => {
                let seq = *next;
                *next += 1;
                done.push(runner.run(seq, job));
                Ok(Submitted::Queued(seq))
            }
            Mode::Thread { shared, .. } => {
                let mut st = shared.state.lock().unwrap();
                if st.closing {
                    return Err(Refused::Closed);
                }
                let seq = st.next;
                if let Job::Json { path, key, value, reconcile } = &job {
                    // Equivalent only when no other job for this file sits between the two sets.
                    let last = st.jobs.iter().rposition(|(_, j)| j.touches(path));
                    if let Some((old_seq, Job::Json { key: k, value: v, reconcile: r, .. })) = last.map(|i| &mut st.jobs[i]) {
                        if k == key {
                            let replaced = *old_seq;
                            *old_seq = seq;
                            *v = value.clone();
                            if r.is_none() {
                                *r = reconcile.clone();
                            }
                            st.next += 1;
                            return Ok(Submitted::Merged { seq, replaced });
                        }
                    }
                }
                if st.jobs.len() >= CAPACITY {
                    return Err(Refused::Full);
                }
                st.next += 1;
                st.jobs.push_back((seq, job));
                shared.cv.notify_all();
                Ok(Submitted::Queued(seq))
            }
        }
    }

    /// Inline mode: the results of the jobs run since the last call.
    pub fn take_inline(&mut self) -> Vec<Done> {
        match &mut self.mode {
            Mode::Inline { done, .. } => std::mem::take(done),
            Mode::Thread { .. } => Vec::new(),
        }
    }

    /// Jobs waiting or running.
    pub fn backlog(&self) -> usize {
        match &self.mode {
            Mode::Inline { .. } => 0,
            Mode::Thread { shared, .. } => {
                let st = shared.state.lock().unwrap();
                st.jobs.len() + usize::from(st.running)
            }
        }
    }

    /// Stops accepting jobs, waits up to `limit` for the queued ones to finish and joins the worker.
    /// Returns false (and leaves the worker to finish on its own) when the disk did not let it finish in time.
    /// Only for teardown and tests: never call this from an input or paint handler.
    pub fn close(&mut self, limit: Duration) -> bool {
        let Mode::Thread { shared, handle } = &mut self.mode else { return true };
        let Some(h) = handle.take() else { return true };
        let deadline = Instant::now() + limit;
        let mut st = shared.state.lock().unwrap();
        st.closing = true;
        shared.cv.notify_all();
        while !st.finished {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                crate::log_error!("settings: {} change(s) were not written within {} s; the disk is not responding", st.jobs.len() + usize::from(st.running), limit.as_secs());
                return false;
            }
            st = shared.cv.wait_timeout(st, left).unwrap().0;
        }
        drop(st);
        let _ = h.join();
        true
    }

    /// Tests: blocks until every job submitted so far finished.
    #[cfg(test)]
    pub fn wait_idle(&self, limit: Duration) -> bool {
        let Mode::Thread { shared, .. } = &self.mode else { return true };
        let deadline = Instant::now() + limit;
        let mut st = shared.state.lock().unwrap();
        while !st.jobs.is_empty() || st.running {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            st = shared.cv.wait_timeout(st, left).unwrap().0;
        }
        true
    }
}

impl Drop for Persister {
    fn drop(&mut self) {
        self.close(DRAIN_LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::tomlcfg::{Cfg, Val};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decsettings-persist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn edited(text: &str, f: impl FnOnce(&mut Cfg)) -> (String, TomlPatch) {
        let mut c = Cfg::parse(text).unwrap();
        f(&mut c);
        let after = c.text();
        let p = TomlPatch::between(text, &after).unwrap();
        (after, p)
    }

    #[test]
    fn a_patch_reproduces_the_edit_byte_for_byte() {
        let base = "# mine\n[general]\nborder_px = 2   # thin\nfoo_future = 1\n\n[[bind]]\nkeys = \"Super+a\"\naction = \"x\"\n\n[decoration]\nstyle = \"mac\"\n";
        type Edit = Box<dyn Fn(&mut Cfg)>;
        let edits: Vec<Edit> = vec![
            Box::new(|c| c.set("general", "border_px", &Val::Int(5)).unwrap()),
            Box::new(|c| c.set("general", "new_key", &Val::Bool(true)).unwrap()),
            Box::new(|c| c.set("snap", "enabled", &Val::Bool(true)).unwrap()),
            Box::new(|c| c.set("decoration.bar", "height", &Val::Int(30)).unwrap()),
            Box::new(|c| {
                c.remove("general", "foo_future");
            }),
            Box::new(|c| {
                c.array_push("bind", &[("keys", Val::Str("Super+b".into())), ("action", Val::Str("y".into()))]).unwrap();
            }),
            Box::new(|c| c.array_remove("bind", 0).unwrap()),
            Box::new(|c| c.set("general", "tags", &Val::List(vec!["a".into(), "b".into()])).unwrap()),
        ];
        for (i, f) in edits.iter().enumerate() {
            let (after, p) = edited(base, f);
            assert!(!p.is_empty(), "edit {i}");
            assert_eq!(p.apply_to(base).unwrap(), after, "edit {i}: {p:?}");
        }
        let (_, none) = edited(base, |_| {});
        assert!(none.is_empty());
    }

    #[test]
    fn a_patch_keeps_keys_that_changed_after_it_was_computed() {
        let base = "[general]\nborder_px = 2\n";
        let (_, p) = edited(base, |c| c.set("general", "gap_inner", &Val::Int(4)).unwrap());
        // a theme committed in between: new colours and a changed border
        let theirs = "[general]\nborder_px = 3\n\n[colors]\nfocused = \"#112233\"\n";
        let out = p.apply_to(theirs).unwrap();
        let c = Cfg::parse(&out).unwrap();
        assert_eq!(c.get("general", "gap_inner"), Some(Val::Int(4)));
        assert_eq!(c.get("general", "border_px"), Some(Val::Int(3)), "an unrelated newer value survives");
        assert_eq!(c.get("colors", "focused"), Some(Val::Str("#112233".into())));
        // both sides create the same new table: merged, not replaced
        let (_, p) = edited(base, |c| c.set("snap", "enabled", &Val::Bool(true)).unwrap());
        let out = p.apply_to("[general]\nborder_px = 2\n[snap]\nthreshold = 9\n").unwrap();
        let c = Cfg::parse(&out).unwrap();
        assert_eq!((c.get("snap", "enabled"), c.get("snap", "threshold")), (Some(Val::Bool(true)), Some(Val::Int(9))));
        // removing what someone else already removed is fine
        let (_, p) = edited("[a]\nx = 1\n", |c| {
            c.remove("a", "x");
        });
        assert_eq!(p.apply_to("").unwrap(), "");
    }

    type Recorded = Arc<Mutex<VecDeque<Ev>>>;

    fn gated(gate: Arc<AtomicBool>, calls: Arc<AtomicUsize>, fail: Option<&'static str>) -> Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync> {
        Arc::new(move |p: &Path| {
            calls.fetch_add(1, Ordering::SeqCst);
            let t0 = Instant::now();
            while gate.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if fail.is_some_and(|f| p.to_string_lossy().contains(f)) {
                return Err(std::io::Error::from_raw_os_error(5));
            }
            Ok(())
        })
    }

    fn done(q: &Recorded) -> Vec<Done> {
        q.lock()
            .unwrap()
            .drain(..)
            .filter_map(|e| match e {
                Ev::Persisted(d) => Some(*d),
                _ => None,
            })
            .collect()
    }

    fn toml_job(path: &Path, from: &str, f: impl FnOnce(&mut Cfg), epoch: u64) -> Job {
        let (_, patch) = edited(from, f);
        Job::Toml { path: path.into(), rdwmctl: "/nonexistent/rdwmctl".into(), patch, epoch, base: io::stamp(path) }
    }

    #[test]
    fn jobs_finish_in_order_while_the_disk_is_blocked_and_submitting_never_waits() {
        let d = tmpdir("order");
        let cfg = d.join("config.toml");
        std::fs::write(&cfg, "[general]\nborder_px = 1\n").unwrap();
        let json = d.join("dock-settings.json");
        let (gate, calls) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let q: Recorded = Default::default();
        let mut p = Persister::spawn_with_sync_hook(q.clone(), Arc::new(|| {}), gated(gate.clone(), calls.clone(), None));
        let shown = "[general]\nborder_px = 1\n";
        let t0 = Instant::now();
        let a = p.submit(toml_job(&cfg, shown, |c| c.set("general", "border_px", &Val::Int(2)).unwrap(), 7)).unwrap();
        // wait until the worker is inside the blocked fsync
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let b = p.submit(Job::Json { path: json.clone(), key: "appSpacing".into(), value: Some(9.into()), reconcile: None }).unwrap();
        let c = p.submit(toml_job(&cfg, "[general]\nborder_px = 2\n", |c| c.set("general", "gap_inner", &Val::Int(5)).unwrap(), 7)).unwrap();
        assert!(t0.elapsed() < Duration::from_millis(500), "submitting must not wait for the disk");
        assert_eq!(p.backlog(), 3);
        assert!(done(&q).is_empty(), "nothing claims success while the write is blocked");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "[general]\nborder_px = 1\n");
        gate.store(false, Ordering::SeqCst);
        assert!(p.wait_idle(Duration::from_secs(10)));
        let all = done(&q);
        assert_eq!(all.iter().map(|d| d.seq).collect::<Vec<_>>(), vec![a.seq(), b.seq(), c.seq()]);
        assert!(all.iter().all(|d| !matches!(&d.outcome, Outcome::Toml(Err(_)) | Outcome::Json(Err(_)))), "{all:?}");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "[general]\nborder_px = 2\ngap_inner = 5\n");
        assert_eq!(io::json_load(&json)["appSpacing"], 9);
        assert!(p.close(Duration::from_secs(5)));
    }

    #[test]
    fn same_key_json_sets_coalesce_only_when_nothing_else_touches_the_file() {
        let d = tmpdir("coalesce");
        let shell = d.join("decebal.json");
        let other = d.join("dock-settings.json");
        std::fs::write(&shell, "{\"keep\": 1}").unwrap();
        let (gate, calls) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let q: Recorded = Default::default();
        let mut p = Persister::spawn_with_sync_hook(q.clone(), Arc::new(|| {}), gated(gate.clone(), calls.clone(), None));
        let set = |k: &str, v: i64, path: &Path| Job::Json { path: path.into(), key: k.into(), value: Some(v.into()), reconcile: None };
        p.submit(set("blocker", 0, &other)).unwrap();
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let first = p.submit(set("x", 1, &shell)).unwrap();
        assert!(matches!(first, Submitted::Queued(_)));
        let mut last = first.clone();
        for v in 2..=50 {
            last = p.submit(set("x", v, &shell)).unwrap();
            assert!(matches!(last, Submitted::Merged { .. }), "{v}");
        }
        assert_eq!(p.backlog(), 2, "fifty sets of one key are one job");
        // a set of another key in the same file is a barrier for later coalescing
        p.submit(set("y", 1, &shell)).unwrap();
        assert!(matches!(p.submit(set("x", 99, &shell)).unwrap(), Submitted::Queued(_)), "x=99 must stay after y");
        // another file in between does not matter
        assert!(matches!(p.submit(set("x", 100, &shell)).unwrap(), Submitted::Merged { .. }));
        gate.store(false, Ordering::SeqCst);
        assert!(p.wait_idle(Duration::from_secs(10)));
        let all = done(&q);
        assert_eq!(all.len(), 4);
        assert_eq!(all[1].seq, last.seq(), "the merged job reports under the newest ticket");
        let v = io::json_load(&shell);
        assert_eq!((v["keep"].clone(), v["x"].clone(), v["y"].clone()), (1.into(), 100.into(), 1.into()));
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["keep", "x", "y"]);
        drop(p);
    }

    #[test]
    fn a_full_queue_refuses_and_a_closed_one_reports_closed() {
        let d = tmpdir("full");
        let json = d.join("a.json");
        let (gate, calls) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let q: Recorded = Default::default();
        let mut p = Persister::spawn_with_sync_hook(q.clone(), Arc::new(|| {}), gated(gate.clone(), calls.clone(), None));
        p.submit(Job::Json { path: d.join("first.json"), key: "k".into(), value: Some(0.into()), reconcile: None }).unwrap();
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        for i in 0..CAPACITY {
            p.submit(Job::Json { path: json.clone(), key: format!("k{}", i % 2), value: Some((i as i64).into()), reconcile: None }).unwrap();
        }
        assert_eq!(p.submit(Job::Json { path: d.join("b.json"), key: "z".into(), value: None, reconcile: None }), Err(Refused::Full));
        gate.store(false, Ordering::SeqCst);
        assert!(p.close(Duration::from_secs(30)), "close drains every queued job");
        assert_eq!(done(&q).len(), CAPACITY + 1);
        assert_eq!(io::json_load(&json)["k1"], (CAPACITY as i64 - 1));
        assert_eq!(p.submit(Job::Json { path: json, key: "z".into(), value: None, reconcile: None }), Err(Refused::Closed));
    }

    #[test]
    fn dropping_the_handle_drains_the_newest_edit_and_ends_the_thread() {
        let d = tmpdir("drain");
        let cfg = d.join("config.toml");
        std::fs::write(&cfg, "").unwrap();
        let (gate, calls) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let q: Recorded = Default::default();
        let mut p = Persister::spawn_with_sync_hook(q.clone(), Arc::new(|| {}), gated(gate.clone(), calls.clone(), None));
        let mut shown = String::new();
        for v in 1..=5 {
            let (after, patch) = edited(&shown, |c| c.set("general", "border_px", &Val::Int(v)).unwrap());
            p.submit(Job::Toml { path: cfg.clone(), rdwmctl: "/nonexistent".into(), patch, epoch: 3, base: io::stamp(&cfg) }).unwrap();
            shown = after;
        }
        let g = gate.clone();
        let opener = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            g.store(false, Ordering::SeqCst);
        });
        assert!(p.close(Duration::from_secs(10)), "teardown waits for the queued edits and joins the worker");
        opener.join().unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "[general]\nborder_px = 5\n");
        assert_eq!(done(&q).len(), 5);
        assert_eq!(p.submit(Job::Json { path: d.join("x.json"), key: "k".into(), value: None, reconcile: None }), Err(Refused::Closed));
        // a disk that never answers: teardown gives up after its limit instead of hanging forever
        let (gate, calls) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let mut p = Persister::spawn_with_sync_hook(q.clone(), Arc::new(|| {}), gated(gate.clone(), calls.clone(), None));
        p.submit(Job::Json { path: d.join("stuck.json"), key: "k".into(), value: Some(1.into()), reconcile: None }).unwrap();
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let t0 = Instant::now();
        assert!(!p.close(Duration::from_millis(200)));
        assert!(t0.elapsed() < Duration::from_secs(2));
        gate.store(false, Ordering::SeqCst);
    }

    #[test]
    fn external_edits_are_never_overwritten_until_the_window_reloads() {
        let d = tmpdir("conflict");
        let cfg = d.join("config.toml");
        std::fs::write(&cfg, "[general]\nborder_px = 1\n").unwrap();
        let mut r = Runner::default();
        let shown = "[general]\nborder_px = 1\n";
        let ok = r.run(1, toml_job(&cfg, shown, |c| c.set("general", "border_px", &Val::Int(2)).unwrap(), 10));
        assert!(matches!(ok.outcome, Outcome::Toml(Ok(TomlSaved { changed: true, .. }))));
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&cfg, "[general]\nborder_px = 9\n").unwrap();
        let (_, patch) = edited(shown, |c| c.set("general", "gap_inner", &Val::Int(3)).unwrap());
        let job = |epoch, base| Job::Toml { path: cfg.clone(), rdwmctl: "/nonexistent".into(), patch: patch.clone(), epoch, base };
        let base = io::stamp(&cfg);
        assert!(matches!(r.run(2, job(10, base)).outcome, Outcome::Toml(Err(Failure { conflict: true, .. }))));
        assert!(matches!(r.run(3, job(10, base)).outcome, Outcome::Toml(Err(Failure { conflict: true, .. }))), "queued before the reload: still a conflict");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "[general]\nborder_px = 9\n");
        assert!(matches!(r.run(4, job(9, base)).outcome, Outcome::Toml(Err(Failure { conflict: true, .. }))), "an older epoch never writes");
        // the window reloaded (new epoch, the stamp it read)
        let ok = r.run(5, job(11, base));
        assert!(matches!(ok.outcome, Outcome::Toml(Ok(_))), "{ok:?}");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "[general]\nborder_px = 9\ngap_inner = 3\n");
    }

    #[test]
    fn a_failed_fsync_or_rejected_config_writes_nothing_and_says_why() {
        let d = tmpdir("fail");
        let cfg = d.join("config.toml");
        let orig = "# keep me\n[general]\nborder_px = 1\n";
        std::fs::write(&cfg, orig).unwrap();
        let (gate, calls) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
        let q: Recorded = Default::default();
        let mut p = Persister::spawn_with_sync_hook(q.clone(), Arc::new(|| {}), gated(gate, calls, Some("config.toml")));
        p.submit(toml_job(&cfg, orig, |c| c.set("general", "border_px", &Val::Int(4)).unwrap(), 20)).unwrap();
        let ctl = d.join("rdwmctl");
        std::fs::write(&ctl, "#!/bin/sh\necho 'config error: border_px out of range' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&ctl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let (_, patch) = edited(orig, |c| c.set("general", "border_px", &Val::Int(400)).unwrap());
        p.submit(Job::Toml { path: cfg.clone(), rdwmctl: ctl, patch, epoch: 20, base: io::stamp(&cfg) }).unwrap();
        assert!(p.wait_idle(Duration::from_secs(10)));
        let all = done(&q);
        match &all[0].outcome {
            Outcome::Toml(Err(f)) => assert!(!f.conflict && f.message.contains("Input/output error"), "{f:?}"),
            o => panic!("{o:?}"),
        }
        match &all[1].outcome {
            Outcome::Toml(Err(f)) => assert_eq!(f.message, "border_px out of range"),
            o => panic!("{o:?}"),
        }
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), orig);
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".tmp") || n.contains("decsettings-check")).collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_theme_transaction_that_fails_midway_restores_every_byte() {
        let (d, src, mut paths) = crate::theme::engine::apply::tests::sandbox("persist-rollback");
        let rdwm = "# my config\n[general]\nborder_px = 2\nunknown_future = \"keep\"\n";
        std::fs::write(&paths.rdwm_config, rdwm).unwrap();
        let shell = "{\"theme\": {\"name\": \"nord\"}, \"future\": [1, 2]}";
        std::fs::write(paths.shell_json(), shell).unwrap();
        std::fs::write(paths.dock_settings(), "{\"appSpacing\": 3}").unwrap();
        paths.rdwmctl = "/nonexistent/rdwmctl".into();
        let q: Recorded = Default::default();
        let (gate, calls) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
        // fail the rdwm file, written after decebal.json: the files already written must be put back
        let mut p = Persister::spawn_with_sync_hook(q.clone(), Arc::new(|| {}), gated(gate, calls.clone(), Some("config.toml")));
        let sel = Selection { name: "dracula".into(), ..Selection::default() };
        p.submit(Job::Theme(Box::new(ThemeJob { src, paths: paths.clone(), request: ThemeRequest::Package { name: "dracula".into(), base: sel }, style: None }))).unwrap();
        assert!(p.wait_idle(Duration::from_secs(20)));
        let all = done(&q);
        assert!(matches!(&all[0].outcome, Outcome::Theme(Err(e)) if e.contains("nothing was changed")), "{:?}", all[0].outcome);
        assert!(calls.load(Ordering::SeqCst) >= 3, "decebal.json written, config.toml failed, decebal.json restored");
        assert_eq!(std::fs::read_to_string(&paths.rdwm_config).unwrap(), rdwm);
        assert_eq!(std::fs::read_to_string(paths.shell_json()).unwrap(), shell);
        assert_eq!(std::fs::read_to_string(paths.dock_settings()).unwrap(), "{\"appSpacing\": 3}");
        drop(p);
        let _ = std::fs::remove_dir_all(d);
    }
}

