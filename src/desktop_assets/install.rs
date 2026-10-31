//! Installing, verifying and removing packs in an icon root (`~/.local/share/icons` by default).
//!
//! One install is one transaction in a private staging folder on the same file system as the target
//! (`<root>/.decebal-staging/<name>.<pid>.<n>`, mode 0700, a lock file serialising installs): read and verify the
//! source, build the theme, write its provenance, then put it in place with one `rename` (`RENAME_NOREPLACE` for a new
//! folder, `RENAME_EXCHANGE` to replace an older decebal pack; where the kernel lacks them, two renames with
//! rollback). The staging folder is removed whatever happens. Only folders carrying the provenance file are ever
//! replaced or removed.

use super::assemble::{self, Built, Spec, PROVENANCE};
use super::source::{self, Filter, Staged};
use super::{catalog, fetch, tar, Limits, Provides};
use serde::{Deserialize, Serialize};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const STAGING: &str = ".decebal-staging";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: String,
    pub repo: String,
    pub revision: String,
    /// The pinned commit (catalogue) or the one a git archive named.
    pub commit: Option<String>,
    pub path: String,
    /// The tree id of `path` as read.
    pub tree: Option<String>,
    /// SHA-256 of the archive file (not for folders).
    pub archive_sha256: Option<String>,
    /// `download`, `archive` or `directory`.
    pub fetched_from: String,
    pub url: Option<String>,
}

/// What was checked. `tree` covers only the pack's own folder; everything the pack took from outside it (files and
/// folders of `link_sources`, `index` files, licence texts) is listed in `outside`, and `complete` is true only when
/// every installed byte came from verified input. `commit` compares a git archive's pax comment with the pin: a
/// consistency check, not authentication.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verified {
    pub tree: bool,
    pub archive_sha256: bool,
    pub commit: bool,
    #[serde(default)]
    pub outside: Vec<OutsideCheck>,
    #[serde(default)]
    pub complete: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutsideCheck {
    /// `link_sources`, `index` or `licences`.
    pub what: String,
    pub verified: bool,
    /// `tree`, `blob`, `archive_sha256` or `none`.
    pub by: String,
}

/// `.decebal-asset.json` of an installed pack.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub format: u32,
    pub pack: String,
    pub name: String,
    pub provides: Vec<String>,
    pub install_name: String,
    #[serde(default)]
    pub family: String,
    /// `catalog` or `local`.
    pub origin: String,
    pub source: SourceRecord,
    pub verified: Verified,
    pub license: String,
    pub license_files: Vec<String>,
    pub content_hash: String,
    /// The tree ids read for each `link_sources` folder.
    #[serde(default)]
    pub link_sources: Vec<(String, String)>,
    /// Files, bytes and folder links that came from `link_sources`.
    #[serde(default)]
    pub from_link_sources: (usize, u64, usize),
    /// Icon directories of index.theme present and listed, and whether that is fewer than half.
    #[serde(default)]
    pub icon_dirs: Option<(usize, usize)>,
    #[serde(default)]
    pub degraded: bool,
    /// Declared aliases applied: text links, pack files they replaced, folder aliases.
    #[serde(default)]
    pub aliases: (usize, usize, usize),
    pub files: usize,
    pub symlinks: usize,
    pub skipped: usize,
    pub installed_by: String,
}

impl Provenance {
    pub fn provides(&self, k: Provides) -> bool {
        self.provides.iter().any(|p| p == k.id())
    }

    pub fn read(dir: &Path) -> Option<Provenance> {
        let p = dir.join(PROVENANCE);
        let m = std::fs::symlink_metadata(&p).ok()?;
        if !m.is_file() || m.len() > 256 << 10 {
            return None;
        }
        serde_json::from_str::<Provenance>(&std::fs::read_to_string(p).ok()?).ok().filter(|p| p.format == 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Fetch the catalogue's `archive_url`.
    Download(String),
    Archive(PathBuf),
    Dir(PathBuf),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pins {
    pub tree: Option<String>,
    pub archive_sha256: Option<String>,
    pub commit: Option<String>,
    /// `link_sources` folder -> pinned tree id.
    pub link_trees: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub pack: String,
    pub name: String,
    pub family: String,
    /// `catalog` or `local`.
    pub origin: String,
    pub spec: Spec,
    pub strip: usize,
    pub link_sources: Vec<String>,
    pub input: Input,
    pub pins: Pins,
    pub source: SourceRecord,
    pub license: String,
    pub root: PathBuf,
    pub force: bool,
    pub dry_run: bool,
    /// Install even when fewer than a quarter of the listed icon directories exist.
    pub allow_degraded: bool,
}

impl Request {
    /// A catalogue pack read from `input` (a download needs the source's `archive_url`).
    pub fn from_catalog(e: catalog::Entry, input: Option<Input>, root: PathBuf) -> Result<Request, String> {
        let input = match input {
            Some(i) => i,
            None => Input::Download(
                e.source
                    .archive_url
                    .clone()
                    .ok_or_else(|| format!("`{}` has no archive_url: pass --archive FILE (a download of {} at {}) or --source-dir DIR (a clone)", e.pack.id, e.source.repo, e.source.commit))?,
            ),
        };
        let archive = matches!(input, Input::Download(_) | Input::Archive(_));
        Ok(Request {
            pack: e.pack.id.clone(),
            name: e.pack.name.clone(),
            family: e.catalog.family.clone(),
            origin: "catalog".into(),
            spec: Spec {
                provides: e.pack.provides.clone(),
                path: e.pack.path.clone(),
                install_name: e.pack.install_name.clone(),
                display_name: e.pack.name.clone(),
                license_files: e.license_files(),
                license_oids: e.license_oids(),
                index_files: e.pack.index.iter().cloned().zip(e.pack.index_oids.iter().cloned().map(Some)).collect(),
                xcursors: e.pack.xcursor.clone(),
                text_links: e.pack.text_links.clone(),
                dir_aliases: e.pack.dir_aliases.clone(),
            },
            strip: if archive { e.source.strip } else { 0 },
            link_sources: e.pack.link_sources.clone(),
            pins: Pins {
                tree: Some(e.pack.tree.clone()),
                archive_sha256: if archive { e.source.archive_sha256.clone() } else { None },
                commit: Some(e.source.commit.clone()),
                link_trees: e.pack.link_sources.iter().cloned().zip(e.pack.link_source_trees.iter().cloned()).chain(e.pack.text_links.iter().map(|t| (t.from.clone(), t.tree.clone()))).collect(),
            },
            source: SourceRecord { id: e.source.id.clone(), repo: e.source.repo.clone(), revision: e.source.revision.clone(), path: e.pack.path.clone(), ..SourceRecord::default() },
            license: e.license().to_string(),
            input,
            root,
            force: false,
            dry_run: false,
            allow_degraded: false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Installed,
    Replaced,
    Unchanged,
    DryRun,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub target: PathBuf,
    pub action: Action,
    pub built: Built,
    pub provenance: Provenance,
    /// Themes of the same name in other icon roots (the new one shadows them).
    pub shadows: Vec<PathBuf>,
    /// `Inherits=` parents that are not installed.
    pub missing_parents: Vec<String>,
}

/// What occupies `<root>/<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Existing {
    Absent,
    /// Not installed by decebal (or a link, or a file): never touched.
    Foreign,
    Ours(Box<Provenance>),
}

pub fn existing(root: &Path, name: &str) -> Existing {
    let t = root.join(name);
    match std::fs::symlink_metadata(&t) {
        Err(_) => Existing::Absent,
        Ok(m) if !m.is_dir() => Existing::Foreign,
        Ok(_) => Provenance::read(&t).map_or(Existing::Foreign, |p| Existing::Ours(Box::new(p))),
    }
}

#[derive(Debug)]
struct Lock(#[allow(dead_code)] std::fs::File);

/// How long an install waits for another one in the same root.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

fn lock(staging: &Path) -> Result<Lock, String> {
    lock_within(staging, LOCK_WAIT)
}

/// Polls instead of blocking: a lock briefly kept alive by a forked child (between `fork` and `exec` it holds a copy
/// of every descriptor) must not fail the next install, and a stuck one must not hang it forever.
fn lock_within(staging: &Path, wait: std::time::Duration) -> Result<Lock, String> {
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(staging.join("lock")).map_err(|e| format!("{}: {e}", staging.display()))?;
    use std::os::fd::AsRawFd;
    let start = std::time::Instant::now();
    // SAFETY: flock on a file descriptor we own; released when the file is closed.
    while unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        if start.elapsed() >= wait {
            return Err("another asset pack install is running in this folder (try again when it has finished)".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Ok(Lock(f))
}

/// The private job folder; removed on drop (with whatever the job left in it) unless `retain` was set because it holds
/// the only copy of a previous version.
struct Job {
    dir: PathBuf,
    retain: bool,
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.retain {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Where a previous version goes when putting it back failed: never cleaned up automatically.
pub const RECOVERY: &str = ".decebal-recovery";

fn staging_dir(root: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let s = root.join(STAGING);
    // two installs may create it at once: whoever loses the race just uses it
    if let Err(e) = std::fs::create_dir(&s) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(format!("{}: {e}", s.display()));
        }
    }
    match std::fs::symlink_metadata(&s) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        _ => return Err(format!("{} is not a folder", s.display())),
    }
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    Ok(s)
}

fn new_job(staging: &Path, name: &str) -> Result<Job, String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    for _ in 0..100 {
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let d = staging.join(format!("{name}.{}.{n}.{nanos}", std::process::id()));
        if std::fs::create_dir(&d).is_ok() {
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
            for sub in ["src", "out"] {
                std::fs::create_dir(d.join(sub)).map_err(|e| e.to_string())?;
            }
            return Ok(Job { dir: d, retain: false });
        }
    }
    Err("could not create a staging folder".into())
}

fn cstring(p: &Path) -> std::ffi::CString {
    std::ffi::CString::new(p.as_os_str().as_bytes()).expect("paths have no NUL")
}

/// `renameat2(from, to, flags)`; `Err(None)` when the kernel or file system does not support the flag.
fn rename2(from: &Path, to: &Path, flags: libc::c_uint) -> Result<(), Option<std::io::Error>> {
    #[cfg(test)]
    if tests::NO_RENAME2.with(|c| c.get()) {
        return Err(None);
    }
    #[cfg(test)]
    if flags == libc::RENAME_EXCHANGE && tests::FAIL.with(|c| c.borrow().contains(&Step::Exchange)) {
        return Err(Some(std::io::Error::other("injected exchange failure")));
    }
    let (a, b) = (cstring(from), cstring(to));
    // SAFETY: two valid NUL-terminated paths; the syscall does not keep them.
    let r = unsafe { libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), flags) };
    if r == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ENOSYS | libc::EINVAL) => Err(None),
        _ => Err(Some(e)),
    }
}

/// One step of a placement, so tests can make any of them fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Moving the installed version aside (two-rename fallback).
    Aside,
    /// Moving the new version into place (two-rename fallback).
    Into,
    /// Putting the previous version back after `Into` failed.
    Restore,
    /// Moving the previous version to the recovery folder after `Restore` failed.
    Recover,
    /// The atomic `RENAME_EXCHANGE` (tests only make it fail).
    #[cfg_attr(not(test), allow(dead_code))]
    Exchange,
}

fn step_rename(step: Step, from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::FAIL.with(|c| c.borrow().contains(&step)) {
        return Err(std::io::Error::other(format!("injected failure at {step:?}")));
    }
    let _ = step;
    std::fs::rename(from, to)
}

/// Is `dir` the decebal pack we decided to replace (same pack and content hash as observed under the lock)?
fn is_expected(dir: &Path, expect: &Provenance) -> bool {
    Provenance::read(dir).is_some_and(|p| p.pack == expect.pack && p.content_hash == expect.content_hash)
}

/// Puts `new` at `target`. `replace`: `target` holds `expect` (observed under the install lock); it ends up at `old`
/// inside the job. If putting the previous version back fails, it is moved to `<root>/.decebal-recovery/` (or, failing
/// that, the job folder is kept) and the error names where it is: a previous version is never deleted by an error path.
fn place(job: &mut Job, root: &Path, new: &Path, target: &Path, replace: Option<&Provenance>) -> Result<(), String> {
    let old = job.dir.join("old");
    let Some(expect) = replace else {
        return match rename2(new, target, libc::RENAME_NOREPLACE) {
            Ok(()) => Ok(()),
            Err(Some(e)) => Err(format!("{}: {e}", target.display())),
            Err(None) => {
                if std::fs::symlink_metadata(target).is_ok() {
                    return Err(format!("{} appeared while installing", target.display()));
                }
                std::fs::rename(new, target).map_err(|e| format!("{}: {e}", target.display()))
            }
        };
    };
    match rename2(new, target, libc::RENAME_EXCHANGE) {
        Ok(()) => {
            // `new` now holds what was at the target: it must be what we decided to replace
            if !is_expected(new, expect) {
                return match rename2(new, target, libc::RENAME_EXCHANGE) {
                    Ok(()) => Err(format!("{} changed while installing; nothing was replaced", target.display())),
                    Err(e) => {
                        job.retain = true;
                        Err(format!(
                            "{} changed while installing and swapping back failed ({}): the other version is kept in {}",
                            target.display(),
                            e.map_or("unsupported".to_string(), |e| e.to_string()),
                            new.display()
                        ))
                    }
                };
            }
            // the new pack is live; the old one goes with the job folder
            let _ = std::fs::rename(new, &old);
            Ok(())
        }
        Err(Some(e)) => Err(format!("{}: {e} (nothing was replaced)", target.display())),
        Err(None) => {
            step_rename(Step::Aside, target, &old).map_err(|e| format!("{}: {e} (nothing was replaced)", target.display()))?;
            if !is_expected(&old, expect) {
                return match step_rename(Step::Restore, &old, target) {
                    Ok(()) => Err(format!("{} changed while installing; nothing was replaced", target.display())),
                    Err(e2) => Err(keep_previous(job, root, target, &old, &e2.to_string())),
                };
            }
            if let Err(e) = step_rename(Step::Into, new, target) {
                return match step_rename(Step::Restore, &old, target) {
                    Ok(()) => Err(format!("{}: {e} (the previous version was put back)", target.display())),
                    Err(e2) => Err(format!("{}: {e}; {}", target.display(), keep_previous(job, root, target, &old, &e2.to_string()))),
                };
            }
            Ok(())
        }
    }
}

/// The previous version could not be put back: move it somewhere no cleanup touches.
fn keep_previous(job: &mut Job, root: &Path, target: &Path, old: &Path, why: &str) -> String {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let rec_dir = root.join(RECOVERY);
    let dest = rec_dir.join(format!("{name}.{}.{nanos}", std::process::id()));
    let moved = std::fs::create_dir_all(&rec_dir).and_then(|_| std::fs::set_permissions(&rec_dir, std::fs::Permissions::from_mode(0o700))).and_then(|_| step_rename(Step::Recover, old, &dest));
    match moved {
        Ok(()) => format!("putting the previous version back failed ({why}): it is kept in {}", dest.display()),
        Err(e) => {
            job.retain = true;
            format!("putting the previous version back failed ({why}) and so did moving it to {} ({e}): it is kept in {}", rec_dir.display(), old.display())
        }
    }
}

fn verify_pins(req: &Request, st: &Staged, archive_sha: Option<&str>) -> Result<Verified, String> {
    let mut v = Verified { tree: false, archive_sha256: false, commit: false, outside: Vec::new(), complete: false };
    let tree = st.tree.as_deref().ok_or_else(|| format!("`{}` holds no files in the source (wrong path or strip_components?)", req.spec.path))?;
    if let Some(want) = &req.pins.tree {
        if want != tree {
            return Err(format!("tree mismatch for `{}`: pinned {want}, read {tree} (a different revision, or a modified source)", req.spec.path));
        }
        v.tree = true;
    }
    if let Some(want) = &req.pins.archive_sha256 {
        let got = archive_sha.unwrap_or("");
        if want != got {
            return Err(format!("archive sha256 mismatch: pinned {want}, got {got}"));
        }
        v.archive_sha256 = true;
    }
    // a folder read as alias targets is pinned by its own tree id, always: no text is read as a link without it
    for t in &req.spec.text_links {
        if !req.pins.link_trees.iter().any(|(d, _)| *d == t.from) {
            return Err(format!("text_links folder `{}` is not pinned (tree = git rev-parse <commit>:{})", t.from, t.from));
        }
    }
    // link_sources of a catalogue pack must be pinned before anything is built from them (each folder's tree, or the
    // whole archive's sha256)
    if req.origin == "catalog" && !req.link_sources.is_empty() && !v.archive_sha256 {
        let unpinned: Vec<&String> = req.link_sources.iter().filter(|d| !req.pins.link_trees.iter().any(|(p, _)| p == *d)).collect();
        if !unpinned.is_empty() {
            return Err(format!(
                "link source{} {} not pinned: add link_source_trees (git rev-parse <commit>:<folder>) or install from an archive with a pinned archive_sha256",
                if unpinned.len() == 1 { "" } else { "s" },
                unpinned.iter().map(|d| format!("`{d}`")).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    for (d, want) in &req.pins.link_trees {
        match st.link_trees.get(d) {
            Some(Some(got)) if got == want => {}
            Some(Some(got)) => return Err(format!("tree mismatch for link source `{d}`: pinned {want}, read {got}")),
            _ => return Err(format!("link source `{d}` holds no files in the source")),
        }
    }
    if let (Some(want), Some(got)) = (&req.pins.commit, &st.commit) {
        if want != got {
            return Err(format!("the archive is of commit {got}, the pin is {want}"));
        }
        v.commit = true;
    }
    if req.origin == "catalog" && !(v.tree || v.archive_sha256) {
        return Err("nothing verified the source".into());
    }
    Ok(v)
}

/// Runs one install (or a dry run). Errors leave the target untouched.
pub fn install(req: &Request, lim: &Limits) -> Result<Report, String> {
    let name = &req.spec.install_name;
    if !super::safe_theme_name(name) {
        return Err(format!("`{name}` is not a usable theme folder name"));
    }
    let target = req.root.join(name);
    // the lock is held for the whole transaction: every observation of the target below is made under it
    let staging = staging_dir(&req.root)?;
    let _lock = lock(&staging)?;
    // refuse early (foreign folder, another pack without --force) before any work
    decide(&existing(&req.root, name), req, &target, None)?;
    let mut job = new_job(&staging, name)?;
    let (src_dir, out) = (job.dir.join("src"), job.dir.join("out"));
    let files = req.spec.license_files.iter().chain(req.spec.index_files.iter().map(|(p, _)| p)).cloned().collect();
    let filter = Filter { strip: req.strip, path: req.spec.path.clone(), files, link_sources: req.link_sources.clone(), text_links: req.spec.text_links.iter().map(|t| t.from.clone()).collect() };
    let mut rec = req.source.clone();
    let (mut staged, archive_sha) = match &req.input {
        Input::Dir(d) => {
            rec.fetched_from = "directory".into();
            (source::from_dir(d, src_dir, filter, lim, &assemble::keep)?, None)
        }
        Input::Archive(_) | Input::Download(_) => {
            // the archive that is hashed is the archive that is read: an external file is copied into this private job
            // while it is hashed (bytes capped as they stream), and only that owned snapshot is sniffed and staged
            let (file, sha) = match &req.input {
                Input::Archive(p) => {
                    rec.fetched_from = "archive".into();
                    let m = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
                    if !m.is_file() {
                        return Err(format!("{} is not a file", p.display()));
                    }
                    let snap = job.dir.join("archive");
                    let f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
                    let (sha, _) = fetch::copy_hashed(f, &snap, lim.max_archive)?;
                    #[cfg(test)]
                    tests::AFTER_SNAPSHOT.with(|h| {
                        if let Some(f) = h.borrow().as_ref() {
                            f(p)
                        }
                    });
                    (snap, sha)
                }
                Input::Download(url) => {
                    rec.fetched_from = "download".into();
                    rec.url = Some(url.clone());
                    let f = job.dir.join("download");
                    let (sha, _) = fetch::download(url, &f, lim.max_archive)?;
                    (f, sha)
                }
                Input::Dir(_) => unreachable!(),
            };
            if let Some(want) = &req.pins.archive_sha256 {
                if *want != sha {
                    return Err(format!("archive sha256 mismatch: pinned {want}, file has {sha}"));
                }
            }
            let mut head = [0u8; 512];
            let n = {
                use std::io::Read;
                let mut f = std::fs::File::open(&file).map_err(|e| e.to_string())?;
                f.read(&mut head).map_err(|e| e.to_string())?
            };
            let fmt = tar::sniff(&head[..n])?;
            let f = std::fs::File::open(&file).map_err(|e| e.to_string())?;
            (source::from_tar(std::io::BufReader::new(f), fmt, src_dir, filter, lim, &assemble::keep)?, Some(sha))
        }
    };
    let verified = verify_pins(req, &staged, archive_sha.as_deref())?;
    // declared aliases become link nodes only now, after their folders' tree ids were verified
    let aliases = super::aliases::apply(&mut staged, &req.spec.path, &req.spec.text_links, &req.spec.dir_aliases)?;
    rec.tree = staged.tree.clone();
    rec.archive_sha256 = archive_sha;
    if rec.commit.is_none() || verified.commit {
        rec.commit = staged.commit.clone().or(req.pins.commit.clone());
    } else {
        rec.commit = req.pins.commit.clone();
    }
    let mut built = assemble::build(&staged, &req.spec, &out, lim)?;
    built.warnings.splice(0..0, staged.warnings.iter().cloned());
    let mut verified = verified;
    outside_checks(req, &built, &mut verified)?;
    let degraded = built.icon_dirs_declared > 0 && built.icon_dirs * 2 < built.icon_dirs_declared;
    if degraded {
        let msg = format!(
            "only {} of the {} icon directories listed in index.theme exist (missing e.g. {}){}",
            built.icon_dirs,
            built.icon_dirs_declared,
            built.missing_dirs_sample.join(", "),
            if built.dropped_links > 0 { format!("; {} links were dropped (folders of another theme? declare it in link_sources)", built.dropped_links) } else { String::new() }
        );
        if built.icon_dirs * 4 < built.icon_dirs_declared && !req.allow_degraded {
            return Err(format!("degraded pack refused: {msg}; --allow-degraded installs it anyway"));
        }
        built.warnings.insert(0, format!("DEGRADED: {msg}"));
    }
    let roots = super::roots_with(&req.root);
    let installed = crate::icons::resolve::list_themes(&roots);
    let missing_parents: Vec<String> =
        built.inherits.iter().filter(|i| *i != "hicolor" && !installed.iter().any(|t| &t.id == *i) && !roots.icon_dirs.iter().any(|r| r.join(i).is_dir())).cloned().collect();
    let shadows: Vec<PathBuf> = roots.icon_dirs.iter().skip(1).map(|r| r.join(name)).filter(|p| p.join("index.theme").exists() || p.join("cursors").is_dir()).collect();
    let (_, files, links) = assemble::content_hash(&out)?;
    let prov = Provenance {
        format: 1,
        pack: req.pack.clone(),
        name: req.name.clone(),
        provides: req.spec.provides.iter().map(|p| p.id().to_string()).collect(),
        install_name: name.clone(),
        family: req.family.clone(),
        origin: req.origin.clone(),
        source: rec,
        verified,
        license: req.license.clone(),
        license_files: built.licenses.clone(),
        content_hash: built.content_hash.clone(),
        link_sources: staged.link_trees.iter().filter_map(|(d, t)| t.clone().map(|t| (d.clone(), t))).collect(),
        from_link_sources: (built.expanded_files + built.copies, built.expanded_bytes, built.expanded_links + built.expanded_dirs),
        icon_dirs: (built.icon_dirs_declared > 0).then_some((built.icon_dirs, built.icon_dirs_declared)),
        degraded,
        aliases: (aliases.text_links, aliases.replaced, aliases.dir_aliases),
        files,
        symlinks: links,
        skipped: built.skipped,
        installed_by: concat!("decebal ", env!("CARGO_PKG_VERSION")).into(),
    };
    let report = |action| Report { target: target.clone(), action, built: built.clone(), provenance: prov.clone(), shadows: shadows.clone(), missing_parents: missing_parents.clone() };
    // decide from a fresh observation (still under the lock), with the full policy, right before committing
    let now = existing(&req.root, name);
    let plan = decide(&now, req, &target, Some(&prov.content_hash))?;
    if plan == Plan::Unchanged {
        return Ok(report(Action::Unchanged));
    }
    if req.dry_run {
        return Ok(report(Action::DryRun));
    }
    let text = serde_json::to_string_pretty(&prov).map_err(|e| e.to_string())? + "\n";
    std::fs::write(out.join(PROVENANCE), text).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    let expect = match &now {
        Existing::Ours(p) => Some(p.as_ref()),
        _ => None,
    };
    place(&mut job, &req.root, &out, &target, expect)?;
    Ok(report(if expect.is_some() { Action::Replaced } else { Action::Installed }))
}

/// Records what the pack took from outside its folder and whether each part was verified; refuses unverified
/// `link_sources` content for catalogue packs (it becomes icons of the pack).
fn outside_checks(req: &Request, built: &Built, v: &mut Verified) -> Result<(), String> {
    let by_archive = v.archive_sha256;
    let mut add = |what: &str, verified: bool, by: &str| v.outside.push(OutsideCheck { what: what.into(), verified, by: by.into() });
    let used_links = built.copies + built.expanded_files + built.expanded_dirs + built.cursor_outside_inputs > 0;
    if used_links {
        let all_pinned = !req.link_sources.is_empty() && req.link_sources.iter().all(|d| req.pins.link_trees.iter().any(|(p, _)| p == d));
        let ok = all_pinned || by_archive;
        if !ok && req.origin == "catalog" {
            return Err("files from link_sources are not pinned: add link_source_trees (git rev-parse <commit>:<folder>) or an archive_sha256".into());
        }
        add(
            "link_sources",
            ok,
            if all_pinned {
                "tree"
            } else if by_archive {
                "archive_sha256"
            } else {
                "none"
            },
        );
    }
    if !req.spec.text_links.is_empty() {
        add("text_links", true, "tree");
    }
    if !req.spec.index_files.is_empty() {
        let pinned = req.spec.index_files.iter().all(|(_, o)| o.is_some());
        add(
            "index",
            pinned || by_archive,
            if pinned {
                "blob"
            } else if by_archive {
                "archive_sha256"
            } else {
                "none"
            },
        );
    }
    if built.licences_outside > 0 {
        let pinned = built.licences_pinned == built.licences_outside;
        add(
            "licences",
            pinned || by_archive,
            if pinned {
                "blob"
            } else if by_archive {
                "archive_sha256"
            } else {
                "none"
            },
        );
    }
    v.complete = (v.tree || by_archive) && v.outside.iter().all(|o| o.verified);
    Ok(())
}

/// Icon directories of an installed theme: (present, listed), or `None` without an index or directories.
pub fn health(dir: &Path) -> Option<(usize, usize)> {
    let ix = crate::icons::index::parse(&std::fs::read_to_string(dir.join("index.theme")).ok()?);
    if ix.dirs.is_empty() {
        return None;
    }
    let present = ix.dirs.iter().filter(|d| super::safe_rel(&d.path).is_ok() && dir.join(&d.path).is_dir()).count();
    Some((present, ix.dirs.len()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    New,
    Replace,
    Unchanged,
}

/// The overwrite policy for what is at the target now. `new_hash`: the content hash of the built pack (`None` for the
/// early check, before it is built).
fn decide(now: &Existing, req: &Request, target: &Path, new_hash: Option<&str>) -> Result<Plan, String> {
    match now {
        Existing::Absent => Ok(Plan::New),
        Existing::Foreign => Err(format!("{} exists and was not installed by decebal: it is left alone (remove it yourself, or install under another --name)", target.display())),
        Existing::Ours(p) if p.pack != req.pack && !req.force => Err(format!("{} holds the pack `{}`; --force replaces it with `{}`", target.display(), p.pack, req.pack)),
        Existing::Ours(p) => {
            let Some(new_hash) = new_hash else { return Ok(Plan::Replace) };
            // compare with what is on disk now, not with the record: a modified install is not "unchanged"
            let on_disk = assemble::content_hash(target).map(|h| h.0).unwrap_or_default();
            if req.force || req.dry_run {
                return Ok(if p.pack == req.pack && on_disk == new_hash && !req.force { Plan::Unchanged } else { Plan::Replace });
            }
            if p.pack == req.pack && on_disk == new_hash {
                return Ok(Plan::Unchanged);
            }
            if p.pack == req.pack && p.content_hash == new_hash {
                return Err(format!("{} was modified since it was installed (decthemectl assets verify): --force restores it", target.display()));
            }
            Err(format!(
                "{} holds `{}` from {} ({}); this one is {new_hash}: --force replaces it",
                target.display(),
                p.pack,
                p.source.commit.as_deref().map_or("an unknown revision".to_string(), |c| c[..12.min(c.len())].to_string()),
                p.content_hash
            ))
        }
    }
}

/// Every decebal pack installed in `root`.
pub fn installed(root: &Path) -> Vec<(PathBuf, Provenance)> {
    let mut v: Vec<(PathBuf, Provenance)> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Provenance::read(&e.path()).map(|p| (e.path(), p)))
        .collect();
    v.sort_by_key(|a| a.1.install_name.to_lowercase());
    v
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrity {
    Ok,
    Modified { recorded: String, now: String },
}

pub fn verify(dir: &Path) -> Result<(Provenance, Integrity), String> {
    let p = Provenance::read(dir).ok_or_else(|| format!("{} was not installed by decebal", dir.display()))?;
    let (now, _, _) = assemble::content_hash(dir)?;
    Ok(if now == p.content_hash { (p, Integrity::Ok) } else { (p.clone(), Integrity::Modified { recorded: p.content_hash, now }) })
}

/// Removes a decebal pack. Everything is decided under the install lock: what is there, and (unless `force`) whether
/// `in_use` says the theme is selected. The folder is renamed into the job first and checked to be the pack that was
/// observed, so no half-deleted theme is visible and nothing that appeared meanwhile is deleted.
pub fn remove(root: &Path, name: &str, force: bool, in_use: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    if !super::safe_theme_name(name) {
        return Err(format!("`{name}` is not a theme folder name"));
    }
    let target = root.join(name);
    let staging = staging_dir(root)?;
    let _lock = lock(&staging)?;
    let expect = match existing(root, name) {
        Existing::Absent => return Err(format!("{} does not exist", target.display())),
        Existing::Foreign => return Err(format!("{} was not installed by decebal: it is left alone", target.display())),
        Existing::Ours(p) => p,
    };
    if !force {
        if let Some(what) = in_use(name) {
            return Err(format!("`{name}` is the {what} theme in use: choose another first, or --force"));
        }
    }
    let mut job = new_job(&staging, name)?;
    let old = job.dir.join("old");
    step_rename(Step::Aside, &target, &old).map_err(|e| format!("{}: {e} (nothing was removed)", target.display()))?;
    if !is_expected(&old, &expect) {
        return match step_rename(Step::Restore, &old, &target) {
            Ok(()) => Err(format!("{} changed while removing; nothing was removed", target.display())),
            Err(e) => Err(keep_previous(&mut job, root, &target, &old, &e.to_string())),
        };
    }
    drop(job);
    Ok(target)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::testkit::*;
    use super::*;
    use std::cell::Cell;

    /// A test hook run with the original archive's path.
    pub type PathHook = Option<Box<dyn Fn(&Path)>>;

    thread_local! {
        pub static NO_RENAME2: Cell<bool> = const { Cell::new(false) };
        pub static FAIL: std::cell::RefCell<Vec<Step>> = const { std::cell::RefCell::new(Vec::new()) };
        /// Runs right after an external archive was copied and hashed (to mutate the original in tests).
        pub static AFTER_SNAPSHOT: std::cell::RefCell<PathHook> = const { std::cell::RefCell::new(None) };
    }

    pub const COMMIT: &str = "5d11ba4e4013a4d1e1c601265d55c66b95184922";

    pub fn request(root: &Path, archive: &Path, entries: &[E]) -> Request {
        Request {
            pack: "fixture-bloom".into(),
            name: "Bloom".into(),
            family: "fixture".into(),
            origin: "catalog".into(),
            spec: Spec {
                provides: vec![Provides::Icons, Provides::Cursors],
                path: "bloom".into(),
                install_name: "bloom".into(),
                display_name: "Bloom".into(),
                license_files: vec!["COPYING".into()],
                license_oids: Default::default(),
                index_files: vec![],
                xcursors: vec![],
                text_links: vec![],
                dir_aliases: vec![],
            },
            strip: 1,
            link_sources: vec![],
            input: Input::Archive(archive.to_path_buf()),
            pins: Pins { tree: Some(tree_of(entries, "top/bloom")), archive_sha256: Some(fetch::hash_file(archive, 1 << 30).unwrap()), commit: Some(COMMIT.into()), link_trees: vec![] },
            source: SourceRecord { id: "fixture".into(), repo: "https://example.org/fixture".into(), revision: "main".into(), path: "bloom".into(), ..SourceRecord::default() },
            license: "GPL-3.0-or-later".into(),
            root: root.to_path_buf(),
            force: false,
            dry_run: false,
            allow_degraded: false,
        }
    }

    pub fn fixture(s: &Scratch, extra: Vec<E>, name: &str) -> (PathBuf, Vec<E>) {
        let entries = icon_source("bloom", extra);
        let p = s.write(name, &targz(&entries, Some(COMMIT)));
        (p, entries)
    }

    fn staging_is_clean(root: &Path) -> bool {
        std::fs::read_dir(root.join(STAGING)).map(|r| r.flatten().all(|e| e.file_name() == "lock")).unwrap_or(true)
    }

    #[test]
    fn install_verify_reinstall_and_remove() {
        let s = Scratch::new("inst");
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        let r = install(&request(&root, &a, &entries), &Limits::default()).unwrap();
        assert_eq!(r.action, Action::Installed);
        let t = root.join("bloom");
        assert!(t.join("index.theme").is_file() && t.join("LICENSES/COPYING").is_file() && t.join(PROVENANCE).is_file());
        let p = Provenance::read(&t).unwrap();
        assert_eq!((p.verified.tree, p.verified.archive_sha256, p.verified.commit, p.verified.complete), (true, true, true, true));
        assert_eq!((p.source.commit.as_deref(), p.source.fetched_from.as_str()), (Some(COMMIT), "archive"));
        assert_eq!((p.icon_dirs, p.degraded), (Some((2, 2)), false));
        assert_eq!(p.verified.outside, [OutsideCheck { what: "licences".into(), verified: true, by: "archive_sha256".into() }], "COPYING lies outside the pack folder; the pinned archive covers it");
        assert_eq!(p.provides, ["icons", "cursors"]);
        assert!(staging_is_clean(&root));
        assert_eq!(verify(&t).unwrap().1, Integrity::Ok);
        assert_eq!(install(&request(&root, &a, &entries), &Limits::default()).unwrap().action, Action::Unchanged);
        std::fs::write(t.join("apps/48/utilities-terminal.svg"), "<svg>tampered</svg>").unwrap();
        assert!(matches!(verify(&t).unwrap().1, Integrity::Modified { .. }));
        let e = install(&request(&root, &a, &entries), &Limits::default()).unwrap_err();
        assert!(e.contains("was modified since it was installed") && e.contains("--force restores it"), "{e}");
        let mut req = request(&root, &a, &entries);
        req.force = true;
        assert_eq!(install(&req, &Limits::default()).unwrap().action, Action::Replaced);
        assert_eq!(verify(&t).unwrap().1, Integrity::Ok, "repaired");
        assert_eq!(installed(&root).len(), 1);
        remove(&root, "bloom", false, &|_| None).unwrap();
        assert!(!t.exists() && staging_is_clean(&root));
        assert!(remove(&root, "bloom", false, &|_| None).is_err());
    }

    #[test]
    fn foreign_themes_and_conflicts_are_never_overwritten() {
        let s = Scratch::new("conflict");
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        s.write("icons/bloom/index.theme", b"[Icon Theme]\nName=mine\n");
        let mut req = request(&root, &a, &entries);
        req.force = true;
        let e = install(&req, &Limits::default()).unwrap_err();
        assert!(e.contains("not installed by decebal"), "{e}");
        assert_eq!(std::fs::read_to_string(root.join("bloom/index.theme")).unwrap(), "[Icon Theme]\nName=mine\n", "untouched, even with --force");
        assert!(remove(&root, "bloom", false, &|_| None).unwrap_err().contains("left alone"));
        std::fs::remove_dir_all(root.join("bloom")).unwrap();
        std::os::unix::fs::symlink("/usr/share/icons/Adwaita", root.join("bloom")).unwrap();
        assert!(install(&req, &Limits::default()).unwrap_err().contains("not installed by decebal"), "a link is foreign too");
        std::fs::remove_file(root.join("bloom")).unwrap();
        install(&request(&root, &a, &entries), &Limits::default()).unwrap();
        let mut other = request(&root, &a, &entries);
        other.pack = "another-pack".into();
        assert!(install(&other, &Limits::default()).unwrap_err().contains("holds the pack `fixture-bloom`"));
    }

    #[test]
    fn pins_hash_mismatches_and_bad_archives_leave_nothing_behind() {
        let s = Scratch::new("pins");
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        let mut r = request(&root, &a, &entries);
        r.pins.archive_sha256 = Some("0".repeat(64));
        let e = install(&r, &Limits::default()).unwrap_err();
        assert!(e.contains("sha256 mismatch"), "{e}");
        let mut r = request(&root, &a, &entries);
        r.pins.tree = Some("1".repeat(40));
        let e = install(&r, &Limits::default()).unwrap_err();
        assert!(e.contains("tree mismatch"), "{e}");
        let mut r = request(&root, &a, &entries);
        r.pins.commit = Some("2".repeat(40));
        let e = install(&r, &Limits::default()).unwrap_err();
        assert!(e.contains("commit"), "{e}");
        let mut r = request(&root, &a, &entries);
        r.spec.license_files = vec!["LICENSE".into()];
        let e = install(&r, &Limits::default()).unwrap_err();
        assert!(e.contains("missing"), "{e}");
        for (name, extra) in [
            ("trav.tgz", vec![E::file("top/../../escape.svg", SVG)]),
            ("abs.tgz", vec![E::link("top/bloom/apps/48/x.svg", "/etc/passwd")]),
            ("esc.tgz", vec![E::link("top/bloom/apps/48/x.svg", "../../../../../home/x.svg")]),
            ("through.tgz", vec![E::link("top/bloom/l", ".."), E::file("top/bloom/l/x.svg", SVG)]),
        ] {
            let (p, ents) = fixture(&s, extra, name);
            let mut r = request(&root, &p, &ents);
            r.origin = "local".into();
            r.pins = Pins::default();
            let e = install(&r, &Limits::default()).unwrap_err();
            assert!(e.contains("rejected") || e.contains("outside") || e.contains("absolute") || e.contains("below"), "{name}: {e}");
        }
        // the same unpinned local request without hostile entries installs: the refusals above are the content's
        let mut r = request(&root, &a, &entries);
        r.origin = "local".into();
        r.pins = Pins::default();
        r.dry_run = true;
        assert_eq!(install(&r, &Limits::default()).unwrap().action, Action::DryRun);
        assert!(!root.join("bloom").exists() && staging_is_clean(&root));
        assert!(!s.path().join("escape.svg").exists() && !root.join("escape.svg").exists());
    }

    #[test]
    fn folder_sources_install_and_dry_runs_write_nothing() {
        let s = Scratch::new("dir");
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        let clone = s.path().join("clone");
        write_tree(&entries, &clone);
        let mut r = request(&root, &a, &entries);
        r.input = Input::Dir(clone.clone());
        r.strip = 0;
        r.pins.archive_sha256 = None;
        r.dry_run = true;
        let rep = install(&r, &Limits::default()).unwrap();
        assert_eq!(rep.action, Action::DryRun);
        assert!(!root.join("bloom").exists());
        r.dry_run = false;
        let rep = install(&r, &Limits::default()).unwrap();
        assert_eq!((rep.action, rep.provenance.source.fetched_from.as_str(), rep.provenance.verified.tree), (Action::Installed, "directory", true));
        assert!(!root.join("bloom/install.sh").exists());
        assert!(!clone.join("bloom/index.theme").symlink_metadata().unwrap().file_type().is_symlink());
        assert!(clone.join("install.sh").exists(), "the source folder is only read");
    }

    #[test]
    fn replacing_without_renameat2_rolls_back_on_failure() {
        let s = Scratch::new("rollback");
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        install(&request(&root, &a, &entries), &Limits::default()).unwrap();
        let before = Provenance::read(&root.join("bloom")).unwrap().content_hash;
        let (b, entries2) = fixture(&s, vec![E::file("top/bloom/apps/48/new.svg", SVG)], "b.tgz");
        let mut r = request(&root, &b, &entries2);
        r.force = true;
        NO_RENAME2.with(|c| c.set(true));
        fail(&[Step::Into]);
        let e = install(&r, &Limits::default()).unwrap_err();
        fail(&[]);
        assert!(e.contains("previous version was put back"), "{e}");
        assert_eq!(Provenance::read(&root.join("bloom")).unwrap().content_hash, before);
        assert!(staging_is_clean(&root));
        assert_eq!(install(&r, &Limits::default()).unwrap().action, Action::Replaced, "the fallback path works");
        NO_RENAME2.with(|c| c.set(false));
        assert!(root.join("bloom/apps/48/new.svg").exists());
    }

    fn fail(steps: &[Step]) {
        FAIL.with(|c| *c.borrow_mut() = steps.to_vec());
    }

    /// An installed `bloom` and a forced request for a different build of it.
    fn replace_setup(tag: &str) -> (Scratch, PathBuf, String, Request) {
        let s = Scratch::new(tag);
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        install(&request(&root, &a, &entries), &Limits::default()).unwrap();
        let before = Provenance::read(&root.join("bloom")).unwrap().content_hash;
        let (b, entries2) = fixture(&s, vec![E::file("top/bloom/apps/48/new.svg", SVG)], "b.tgz");
        let mut r = request(&root, &b, &entries2);
        r.force = true;
        (s, root, before, r)
    }

    #[test]
    fn a_failed_exchange_changes_nothing() {
        let (_s, root, before, r) = replace_setup("xchg");
        fail(&[Step::Exchange]);
        let e = install(&r, &Limits::default()).unwrap_err();
        fail(&[]);
        assert!(e.contains("nothing was replaced"), "{e}");
        assert_eq!(Provenance::read(&root.join("bloom")).unwrap().content_hash, before);
        assert!(staging_is_clean(&root));
    }

    #[test]
    fn a_previous_version_that_cannot_be_put_back_is_kept_for_recovery() {
        let (_s, root, before, r) = replace_setup("recover");
        NO_RENAME2.with(|c| c.set(true));
        fail(&[Step::Into, Step::Restore]);
        let e = install(&r, &Limits::default()).unwrap_err();
        assert!(e.contains("it is kept in") && e.contains(RECOVERY), "{e}");
        let kept: Vec<PathBuf> = std::fs::read_dir(root.join(RECOVERY)).unwrap().flatten().map(|e| e.path()).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(Provenance::read(&kept[0]).unwrap().content_hash, before, "the previous version survives intact");
        assert_eq!(verify(&kept[0]).unwrap().1, Integrity::Ok);
        assert!(staging_is_clean(&root), "the job itself is cleaned up: the copy is outside it");
        // and when even the recovery folder cannot take it, the job folder is kept instead of deleted
        std::fs::rename(&kept[0], root.join("bloom")).unwrap();
        std::fs::remove_dir_all(root.join(RECOVERY)).unwrap();
        fail(&[Step::Into, Step::Restore, Step::Recover]);
        let e = install(&r, &Limits::default()).unwrap_err();
        fail(&[]);
        NO_RENAME2.with(|c| c.set(false));
        let at = e.rsplit("it is kept in ").next().unwrap().trim();
        assert!(Path::new(at).is_dir() && at.contains(STAGING), "{e}");
        assert_eq!(Provenance::read(Path::new(at)).unwrap().content_hash, before, "retained, not deleted with the job");
    }

    #[test]
    fn a_foreign_folder_swapped_in_before_the_commit_is_put_back_untouched() {
        let (_s, root, _before, r) = replace_setup("swap");
        // decide() observed our pack; simulate a foreign folder appearing in its place just before place()
        let staging = staging_dir(&root).unwrap();
        let mut job = new_job(&staging, "bloom").unwrap();
        let expect = Provenance::read(&root.join("bloom")).unwrap();
        std::fs::rename(root.join("bloom"), job.dir.join("theirs-moved")).unwrap();
        std::fs::create_dir(root.join("bloom")).unwrap();
        std::fs::write(root.join("bloom/index.theme"), "[Icon Theme]\nName=Theirs\n").unwrap();
        let newdir = job.dir.join("out");
        std::fs::write(newdir.join("marker"), "new").unwrap();
        for no_rename2 in [false, true] {
            NO_RENAME2.with(|c| c.set(no_rename2));
            let e = place(&mut job, &root, &newdir, &root.join("bloom"), Some(&expect)).unwrap_err();
            assert!(e.contains("changed while installing"), "{e}");
            assert_eq!(std::fs::read_to_string(root.join("bloom/index.theme")).unwrap(), "[Icon Theme]\nName=Theirs\n", "the foreign folder is back in place");
        }
        NO_RENAME2.with(|c| c.set(false));
        let _ = r;
    }

    #[test]
    fn concurrent_installs_of_two_packs_never_overwrite_without_force() {
        for round in 0..4 {
            let s = Scratch::new("race");
            let root = s.path().join("icons");
            let (a, entries) = fixture(&s, vec![], "a.tgz");
            let (b, entries2) = fixture(&s, vec![E::file("top/bloom/apps/48/other.svg", SVG)], "b.tgz");
            let r1 = request(&root, &a, &entries);
            let mut r2 = request(&root, &b, &entries2);
            r2.pack = "another-bloom".into();
            let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
            let (g1, g2) = (gate.clone(), gate);
            let t1 = std::thread::spawn(move || {
                g1.wait();
                install(&r1, &Limits::default())
            });
            let t2 = std::thread::spawn(move || {
                g2.wait();
                install(&r2, &Limits::default())
            });
            let (o1, o2) = (t1.join().unwrap(), t2.join().unwrap());
            assert!(o1.is_ok() != o2.is_ok(), "round {round}: exactly one wins: {:?} / {:?}", o1.as_ref().err(), o2.as_ref().err());
            let loser = o1.as_ref().err().or(o2.as_ref().err()).unwrap();
            assert!(loser.contains("holds the pack"), "round {round}: {loser}");
            let winner = if o1.is_ok() { "fixture-bloom" } else { "another-bloom" };
            let p = Provenance::read(&root.join("bloom")).unwrap();
            assert_eq!(p.pack, winner);
            assert_eq!(verify(&root.join("bloom")).unwrap().1, Integrity::Ok);
            assert!(staging_is_clean(&root));
        }
    }

    #[test]
    fn concurrent_install_and_remove_leave_a_consistent_target() {
        for round in 0..4 {
            let (s, root, _before, mut r) = replace_setup("race-rm");
            r.force = false;
            r.pack = "fixture-bloom".into();
            let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
            let (g1, g2) = (gate.clone(), gate);
            let root2 = root.clone();
            let t1 = std::thread::spawn(move || {
                g1.wait();
                install(&r, &Limits::default())
            });
            let t2 = std::thread::spawn(move || {
                g2.wait();
                remove(&root2, "bloom", false, &|_| None)
            });
            let (o1, o2) = (t1.join().unwrap(), t2.join().unwrap());
            match existing(&root, "bloom") {
                Existing::Absent => assert!(o2.is_ok(), "round {round}: gone only through remove"),
                Existing::Ours(p) => {
                    assert_eq!(verify(&root.join("bloom")).unwrap().1, Integrity::Ok, "round {round}");
                    assert!(o1.is_ok() && p.pack == "fixture-bloom", "round {round}: present only through a successful install");
                }
                Existing::Foreign => panic!("round {round}: a half-written folder"),
            }
            assert!(o1.is_ok() || o1.as_ref().unwrap_err().contains("--force"), "round {round}: {o1:?}");
            assert!(staging_is_clean(&root));
            drop(s);
        }
    }

    #[test]
    fn remove_decides_in_use_under_the_lock() {
        let s = Scratch::new("rm-use");
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        install(&request(&root, &a, &entries), &Limits::default()).unwrap();
        let e = remove(&root, "bloom", false, &|n| (n == "bloom").then(|| "icon".to_string())).unwrap_err();
        assert!(e.contains("in use"), "{e}");
        assert!(root.join("bloom/index.theme").exists());
        remove(&root, "bloom", true, &|_| Some("icon".into())).unwrap();
        assert!(!root.join("bloom").exists());
    }

    #[test]
    fn cursors_are_built_from_pngs_and_declared_hotspots() {
        let png = |w: u32| {
            let mut out = Vec::new();
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, w, image::Rgba([20, 20, 20, 255]))).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
            out
        };
        let entries = vec![
            E::dir("top/"),
            E::file("top/debian/copyright", b"License: GPL-3\n"),
            E::exec("top/sense/cursors.sh", b"#!/bin/bash\nmkdir cursors\n"),
            E::file("top/sense/index.theme", b"[Icon Theme]\nName=Sense\n"),
            E::file("top/sense/24x24/left_ptr.png", &png(24)),
            E::file("top/sense/32x32/left_ptr.png", &png(32)),
            E::link("top/sense/24x24/default.png", "left_ptr.png"),
            E::file("top/sense/24x24/unused.png", &png(24)),
            E::file("top/sense/cursors_scalable/left_ptr/left_ptr.svg", crate::desktop_assets::testkit::SVG),
        ];
        let s = Scratch::new("xcur-install");
        let a = s.write("sense.tgz", &targz(&entries, Some(COMMIT)));
        let dummy = icon_source("bloom", vec![]); // `request` derives a bloom pin; replaced below
        let mut r = request(&s.path().join("icons"), &a, &dummy);
        r.pins.tree = Some(tree_of(&entries, "top/sense"));
        r.spec = Spec {
            provides: vec![Provides::Cursors],
            path: "sense".into(),
            install_name: "sense".into(),
            display_name: "Sense".into(),
            license_files: vec!["debian/copyright".into()],
            license_oids: Default::default(),
            index_files: vec![],
            xcursors: vec![super::super::xcursor::Cursor {
                name: "left_ptr".into(),
                images: super::super::xcursor::parse_config("24 4 0 24x24/left_ptr.png\n32 5 0 32x32/left_ptr.png").unwrap(),
                aliases: vec!["default".into(), "top_left_arrow".into()],
            }],
            text_links: vec![],
            dir_aliases: vec![],
        };
        let rep = install(&r, &Limits::default()).unwrap();
        let t = s.path().join("icons/sense");
        assert_eq!(rep.built.cursors_built, 1);
        let im = crate::desktop_assets::cursor::images(&std::fs::read(t.join("cursors/left_ptr")).unwrap()).unwrap();
        assert_eq!(im.iter().map(|i| (i.nominal, i.xhot)).collect::<Vec<_>>(), [(24, 4), (32, 5)]);
        assert_eq!(std::fs::read_link(t.join("cursors/default")).unwrap().to_str(), Some("left_ptr"));
        assert!(!t.join("24x24").exists() && !t.join("cursors.sh").exists(), "sources, unused images and the script are not installed");
        assert!(t.join("cursors_scalable/left_ptr/left_ptr.svg").is_file(), "Plasma's scalable cursors stay");
        assert!(t.join("LICENSES/copyright").is_file());
        assert_eq!(verify(&t).unwrap().1, Integrity::Ok);
        // a cursor shipped in cursors/ and built as well is a conflict, not an overwrite
        let mut clash = entries.clone();
        clash.push(E::file("top/sense/cursors/left_ptr", &xcursor(24)));
        let b = s.write("clash.tgz", &targz(&clash, Some(COMMIT)));
        let mut r2 = request(&s.path().join("icons2"), &b, &dummy);
        r2.spec = r.spec.clone();
        r2.pins.tree = Some(tree_of(&clash, "top/sense"));
        assert!(install(&r2, &Limits::default()).unwrap_err().contains("both shipped"));
        let mut alias_clash = entries.clone();
        alias_clash.push(E::file("top/sense/cursors/top_left_arrow", &xcursor(24)));
        let c = s.write("alias-clash.tgz", &targz(&alias_clash, Some(COMMIT)));
        let mut r3 = request(&s.path().join("icons3"), &c, &dummy);
        r3.spec = r.spec.clone();
        r3.pins.tree = Some(tree_of(&alias_clash, "top/sense"));
        assert!(install(&r3, &Limits::default()).unwrap_err().contains("`top_left_arrow` is both shipped"), "an alias is not silently replaced by a shipped file");
    }

    /// `dark` with folder links into `lib` (see assemble's tests), as an archive.
    fn sibling(s: &Scratch, name: &str) -> (PathBuf, Vec<E>) {
        let mut v = vec![
            E::dir("top/"),
            E::file("top/COPYING", b"GPL\n"),
            E::file("top/lib/48/apps/a.svg", SVG),
            E::file("top/lib/48/places/f.svg", SVG),
            E::file("top/dark/index.theme", b"[Icon Theme]\nName=Dark\nDirectories=48/apps,48/places,own\n[48/apps]\nSize=48\n[48/places]\nSize=48\n[own]\nSize=16\n"),
            E::file("top/dark/own/x.svg", SVG),
        ];
        v.push(E::link("top/dark/48", "../lib/48"));
        (s.write(name, &targz(&v, Some(COMMIT))), v)
    }

    fn dark_request(root: &Path, a: &Path, v: &[E]) -> Request {
        let dummy = icon_source("bloom", vec![]);
        let mut r = request(root, a, &dummy);
        r.spec.path = "dark".into();
        r.spec.install_name = "dark".into();
        r.spec.provides = vec![Provides::Icons];
        r.pins.tree = Some(tree_of(v, "top/dark"));
        r
    }

    #[test]
    fn link_sources_are_pinned_by_their_own_tree_or_the_archive() {
        let s = Scratch::new("linktrees");
        let (a, v) = sibling(&s, "d.tgz");
        // pinned by tree: complete
        let mut r = dark_request(&s.path().join("i1"), &a, &v);
        r.link_sources = vec!["lib".into()];
        r.pins.archive_sha256 = None;
        r.pins.link_trees = vec![("lib".into(), tree_of(&v, "top/lib"))];
        r.spec.license_oids.insert("COPYING".into(), crate::desktop_assets::gitoid::hex(&crate::desktop_assets::gitoid::blob_oid(b"GPL\n")));
        let rep = install(&r, &Limits::default()).unwrap();
        let p = &rep.provenance;
        assert!(p.verified.complete && p.verified.outside.iter().any(|o| o.what == "link_sources" && o.verified && o.by == "tree"), "{:?}", p.verified);
        assert!(p.verified.outside.iter().any(|o| o.what == "licences" && o.by == "blob"));
        // the same install without the licence pin is not called complete
        let mut r0 = r.clone();
        r0.root = s.path().join("i0");
        r0.spec.license_oids.clear();
        let p0 = install(&r0, &Limits::default()).unwrap().provenance;
        assert!(!p0.verified.complete && p0.verified.outside.iter().any(|o| o.what == "licences" && !o.verified));
        assert_eq!(p.link_sources, [("lib".to_string(), tree_of(&v, "top/lib"))]);
        assert_eq!((p.icon_dirs, p.degraded), (Some((3, 3)), false));
        assert!(p.from_link_sources.0 == 2, "{:?}", p.from_link_sources);
        // a wrong link tree pin
        let mut r2 = r.clone();
        r2.root = s.path().join("i2");
        r2.pins.link_trees = vec![("lib".into(), "2".repeat(40))];
        assert!(install(&r2, &Limits::default()).unwrap_err().contains("tree mismatch for link source `lib`"));
        // neither a link tree pin nor an archive sha256: a catalogue pack refuses the unverified files
        let mut r3 = r.clone();
        r3.root = s.path().join("i3");
        r3.pins.link_trees = vec![];
        assert!(install(&r3, &Limits::default()).unwrap_err().contains("not pinned"));
        // a pinned archive covers them, and says so
        let mut r4 = r3.clone();
        r4.root = s.path().join("i4");
        r4.pins.archive_sha256 = Some(fetch::hash_file(&a, 1 << 30).unwrap());
        let p = install(&r4, &Limits::default()).unwrap().provenance;
        assert!(p.verified.complete && p.verified.outside.iter().any(|o| o.by == "archive_sha256"));
        // a local import without pins installs, and says it is not complete
        let mut r5 = r3.clone();
        r5.root = s.path().join("i5");
        r5.origin = "local".into();
        let p = install(&r5, &Limits::default()).unwrap().provenance;
        assert!(!p.verified.complete && p.verified.outside.iter().any(|o| o.what == "link_sources" && !o.verified));
    }

    #[test]
    fn a_degraded_pack_is_refused_or_marked_and_verify_reports_it() {
        let s = Scratch::new("degraded");
        let (a, v) = sibling(&s, "d.tgz");
        // no link_sources: 1 of 3 directories -> fewer than half: marked; add more listed dirs for "fewer than a quarter"
        let mut r = dark_request(&s.path().join("i1"), &a, &v);
        let rep = install(&r, &Limits::default()).unwrap();
        assert!(rep.provenance.degraded && rep.built.warnings[0].starts_with("DEGRADED: only 1 of the 3"), "{:?}", rep.built.warnings);
        assert_eq!(health(&s.path().join("i1/dark")), Some((1, 3)));
        let mut many = v.clone();
        many.retain(|e| !e.name.ends_with("index.theme"));
        many.push(E::file(
            "top/dark/index.theme",
            b"[Icon Theme]\nName=Dark\nDirectories=48/apps,48/places,a,b,c,own\n[48/apps]\nSize=48\n[48/places]\nSize=48\n[a]\nSize=1\n[b]\nSize=1\n[c]\nSize=1\n[own]\nSize=16\n",
        ));
        let b = s.write("many.tgz", &targz(&many, Some(COMMIT)));
        r = dark_request(&s.path().join("i2"), &b, &many);
        r.pins.archive_sha256 = Some(fetch::hash_file(&b, 1 << 30).unwrap());
        let e = install(&r, &Limits::default()).unwrap_err();
        assert!(e.contains("degraded pack refused: only 1 of the 6") && e.contains("link_sources") && e.contains("--allow-degraded"), "{e}");
        assert!(!s.path().join("i2/dark").exists());
        r.allow_degraded = true;
        assert!(install(&r, &Limits::default()).unwrap().provenance.degraded);
    }

    fn png(w: u32, rgb: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, w, image::Rgba(rgb))).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    /// A clone-like folder: the cursor pack `sense` builds `left_ptr` from a PNG that is a link into the link source `lib`.
    fn cursor_link_source(lib_png: &[u8]) -> Vec<E> {
        vec![
            E::dir("top/"),
            E::file("top/COPYING", b"GPL\n"),
            E::file("top/sense/index.theme", b"[Icon Theme]\nName=Sense\n"),
            E::link("top/sense/24x24/left_ptr.png", "../../lib/left_ptr.png"),
            E::file("top/lib/left_ptr.png", lib_png),
        ]
    }

    fn cursor_request(root: &Path, dir: &Path, v: &[E]) -> Request {
        let dummy = icon_source("bloom", vec![]);
        let s = Scratch::new("dummy-arch");
        let a = s.write("d.tgz", &targz(&dummy, None));
        let mut r = request(root, &a, &dummy);
        r.input = Input::Dir(dir.to_path_buf());
        r.strip = 0;
        r.pins = Pins { tree: Some(tree_of(v, "top/sense")), archive_sha256: None, commit: None, link_trees: vec![] };
        r.link_sources = vec!["lib".into()];
        r.spec = Spec {
            provides: vec![Provides::Cursors],
            path: "sense".into(),
            install_name: "sense".into(),
            display_name: "Sense".into(),
            license_files: vec!["COPYING".into()],
            license_oids: [("COPYING".to_string(), crate::desktop_assets::gitoid::hex(&crate::desktop_assets::gitoid::blob_oid(b"GPL\n")))].into(),
            index_files: vec![],
            xcursors: vec![super::super::xcursor::Cursor { name: "left_ptr".into(), images: super::super::xcursor::parse_config("24 1 1 24x24/left_ptr.png").unwrap(), aliases: vec![] }],
            text_links: vec![],
            dir_aliases: vec![],
        };
        r
    }

    #[test]
    fn cursor_pngs_read_from_link_sources_are_pinned_and_recorded() {
        let s = Scratch::new("cur-outside");
        let v = cursor_link_source(&png(24, [0, 0, 0, 255]));
        let clone = s.path().join("clone");
        write_tree(&v, &clone);
        // no pin for `lib`: refused before anything is built
        let r = cursor_request(&s.path().join("i1"), &clone, &v);
        assert!(install(&r, &Limits::default()).unwrap_err().contains("link source `lib` not pinned"));
        assert!(!s.path().join("i1/sense").exists());
        // pinned: installed, the outside PNG is counted and the record is complete
        let mut r = cursor_request(&s.path().join("i2"), &clone, &v);
        r.pins.link_trees = vec![("lib".into(), tree_of(&v, "top/lib"))];
        let rep = install(&r, &Limits::default()).unwrap();
        assert_eq!(rep.built.cursor_outside_inputs, 1);
        assert!(rep.provenance.verified.complete && rep.provenance.verified.outside.iter().any(|o| o.what == "link_sources" && o.by == "tree"), "{:?}", rep.provenance.verified);
        // the external PNG changes, the pack's own tree does not: the link source pin catches it
        let v2 = cursor_link_source(&png(24, [255, 0, 0, 255]));
        assert_eq!(tree_of(&v2, "top/sense"), tree_of(&v, "top/sense"), "the pack folder itself is unchanged");
        std::fs::write(clone.join("lib/left_ptr.png"), png(24, [255, 0, 0, 255])).unwrap();
        let mut r3 = r.clone();
        r3.root = s.path().join("i3");
        assert!(install(&r3, &Limits::default()).unwrap_err().contains("tree mismatch for link source `lib`"));
        // a local import without pins installs but does not claim to be complete
        let mut r4 = r.clone();
        r4.root = s.path().join("i4");
        r4.origin = "local".into();
        r4.pins.link_trees = vec![];
        let p = install(&r4, &Limits::default()).unwrap().provenance;
        assert!(!p.verified.complete && p.verified.outside.iter().any(|o| o.what == "link_sources" && !o.verified && o.by == "none"), "{:?}", p.verified);
        // and a cursor PNG that resolves outside the pack and its link_sources is refused
        let mut r5 = r.clone();
        r5.root = s.path().join("i5");
        r5.link_sources = vec![];
        r5.pins.link_trees = vec![];
        r5.origin = "local".into();
        assert!(install(&r5, &Limits::default()).unwrap_err().contains("not in the pack"), "lib is not staged at all");
        // staged, but only as a licence text, not as a link source: still refused for a cursor
        let mut r6 = r5.clone();
        r6.root = s.path().join("i6");
        r6.spec.license_files.push("lib/left_ptr.png".into());
        assert!(install(&r6, &Limits::default()).unwrap_err().contains("outside the pack and its link_sources"));
        assert!(!s.path().join("i5/sense").exists() && !s.path().join("i6/sense").exists());
    }

    #[test]
    fn the_archive_that_is_verified_is_the_archive_that_is_staged() {
        let s = Scratch::new("snapshot");
        let root = s.path().join("icons");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        let other = targz(&icon_source("bloom", vec![E::file("top/bloom/apps/48/evil.svg", SVG)]), Some(COMMIT));
        let r = request(&root, &a, &entries);
        let sha_a = r.pins.archive_sha256.clone().unwrap();
        // the original is replaced right after it was copied and hashed: staging must not see the new bytes
        AFTER_SNAPSHOT.with(|h| *h.borrow_mut() = Some(Box::new(move |p: &Path| std::fs::write(p, &other).unwrap())));
        let rep = install(&r, &Limits::default());
        AFTER_SNAPSHOT.with(|h| *h.borrow_mut() = None);
        let rep = rep.unwrap();
        assert_eq!(rep.provenance.source.archive_sha256.as_deref(), Some(sha_a.as_str()));
        assert!(!root.join("bloom/apps/48/evil.svg").exists(), "only the verified snapshot was staged");
        assert_ne!(fetch::hash_file(&a, 1 << 30).unwrap(), sha_a, "the original did change");
        assert!(staging_is_clean(&root), "the snapshot went with the job");
    }

    #[test]
    fn text_link_folders_are_pinned_by_their_own_tree() {
        let v = vec![
            E::dir("top/"),
            E::file("top/COPYING", b"GPL\n"),
            E::file("top/src/index.theme", b"[Icon Theme]\nName=T\nDirectories=apps/48,apps@2x/48\n[apps/48]\nSize=48\n[apps@2x/48]\nSize=48\nScale=2\n"),
            E::file("top/src/apps/48/real.svg", SVG),
            E::file("top/links/apps/48/alias.svg", b"real.svg"),
            E::file("top/links/apps/48/gone.svg", b"nothing.svg"),
        ];
        let s = Scratch::new("textlinks");
        let a = s.write("t.tgz", &targz(&v, Some(COMMIT)));
        let dummy = icon_source("bloom", vec![]);
        let mut r = request(&s.path().join("i1"), &a, &dummy);
        r.pins = Pins { tree: Some(tree_of(&v, "top/src")), archive_sha256: None, commit: Some(COMMIT.into()), link_trees: vec![("links".into(), tree_of(&v, "top/links"))] };
        r.spec.path = "src".into();
        r.spec.install_name = "t".into();
        r.spec.provides = vec![Provides::Icons];
        r.spec.text_links = vec![crate::desktop_assets::aliases::TextLinks { from: "links".into(), tree: tree_of(&v, "top/links"), onto: ".".into(), replace: false }];
        r.spec.dir_aliases = vec![crate::desktop_assets::aliases::DirAlias { name: "apps@2x".into(), target: "apps".into() }];
        r.spec.license_oids.insert("COPYING".into(), crate::desktop_assets::gitoid::hex(&crate::desktop_assets::gitoid::blob_oid(b"GPL\n")));
        let rep = install(&r, &Limits::default()).unwrap();
        let p = &rep.provenance;
        assert_eq!((p.aliases, p.icon_dirs, p.degraded), ((2, 0, 1), Some((2, 2)), false));
        assert!(p.verified.complete && p.verified.outside.iter().any(|o| o.what == "text_links" && o.verified && o.by == "tree"), "{:?}", p.verified);
        assert!(p.link_sources.iter().any(|(d, t)| d == "links" && *t == tree_of(&v, "top/links")), "the receipt names the alias folder's tree");
        assert_eq!(std::fs::read_link(s.path().join("i1/t/apps/48/alias.svg")).unwrap().display().to_string(), "real.svg");
        assert_eq!(verify(&s.path().join("i1/t")).unwrap().1, Integrity::Ok);
        // a changed alias text under an unchanged pack folder: its own pin catches it
        let mut v2 = v.clone();
        v2[4] = E::file("top/links/apps/48/alias.svg", b"other.svg");
        assert_eq!(tree_of(&v2, "top/src"), tree_of(&v, "top/src"));
        let b = s.write("t2.tgz", &targz(&v2, Some(COMMIT)));
        let mut r2 = r.clone();
        r2.input = Input::Archive(b);
        r2.root = s.path().join("i2");
        assert!(install(&r2, &Limits::default()).unwrap_err().contains("tree mismatch for link source `links`"));
        // and without a pin no text is read as a link, whatever the origin
        let mut r3 = r.clone();
        r3.root = s.path().join("i3");
        r3.origin = "local".into();
        r3.pins.link_trees.clear();
        assert!(install(&r3, &Limits::default()).unwrap_err().contains("text_links folder `links` is not pinned"));
        assert!(!s.path().join("i2/t").exists() && !s.path().join("i3/t").exists());
    }

    #[test]
    fn a_second_install_waits_for_the_lock() {
        let s = Scratch::new("lock");
        let staging = staging_dir(&s.path().join("icons")).unwrap();
        let held = lock(&staging).unwrap();
        let t = std::time::Instant::now();
        assert!(lock_within(&staging, std::time::Duration::from_millis(100)).unwrap_err().contains("another"));
        assert!(t.elapsed() >= std::time::Duration::from_millis(100), "it waited");
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            drop(held);
        });
        assert!(lock_within(&staging, std::time::Duration::from_secs(5)).is_ok(), "taken once the other one lets go");
        release.join().unwrap();
    }
}
