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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verified {
    pub tree: bool,
    pub archive_sha256: bool,
    pub commit: bool,
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
                index_files: e.pack.index.iter().cloned().zip(e.pack.index_oids.iter().cloned().map(Some)).collect(),
                xcursors: e.pack.xcursor.clone(),
            },
            strip: if archive { e.source.strip } else { 0 },
            link_sources: e.pack.link_sources.clone(),
            pins: Pins { tree: Some(e.pack.tree.clone()), archive_sha256: if archive { e.source.archive_sha256.clone() } else { None }, commit: Some(e.source.commit.clone()) },
            source: SourceRecord { id: e.source.id.clone(), repo: e.source.repo.clone(), revision: e.source.revision.clone(), path: e.pack.path.clone(), ..SourceRecord::default() },
            license: e.license().to_string(),
            input,
            root,
            force: false,
            dry_run: false,
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

/// The private job folder; removed on drop (with whatever the job left in it).
struct Job {
    dir: PathBuf,
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn staging_dir(root: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let s = root.join(STAGING);
    match std::fs::symlink_metadata(&s) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
        Ok(_) => return Err(format!("{} is not a folder", s.display())),
        Err(_) => std::fs::create_dir(&s).map_err(|e| format!("{}: {e}", s.display()))?,
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
            return Ok(Job { dir: d });
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

#[cfg(test)]
fn second_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    if tests::FAIL_SECOND_RENAME.with(|c| c.get()) {
        return Err(std::io::Error::other("injected failure"));
    }
    std::fs::rename(from, to)
}

#[cfg(not(test))]
fn second_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// Puts `new` at `target`. `replace`: `target` is an older decebal pack, which ends up at `old` (inside the job).
fn place(new: &Path, target: &Path, replace: bool, old: &Path) -> Result<(), String> {
    if !replace {
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
    }
    match rename2(new, target, libc::RENAME_EXCHANGE) {
        Ok(()) => std::fs::rename(new, old).map_err(|e| e.to_string()), // `new` holds the old theme now
        Err(Some(e)) => Err(format!("{}: {e}", target.display())),
        Err(None) => {
            std::fs::rename(target, old).map_err(|e| format!("{}: {e}", target.display()))?;
            if let Err(e) = second_rename(new, target) {
                // roll back: the old theme goes back where it was
                return match std::fs::rename(old, target) {
                    Ok(()) => Err(format!("{}: {e} (the previous version was put back)", target.display())),
                    Err(e2) => Err(format!("{}: {e}; restoring the previous version failed too: {e2} (it is in {})", target.display(), old.display())),
                };
            }
            Ok(())
        }
    }
}

fn verify_pins(req: &Request, st: &Staged, archive_sha: Option<&str>) -> Result<Verified, String> {
    let mut v = Verified::default();
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
    let before = existing(&req.root, name);
    match &before {
        Existing::Foreign => return Err(format!("{} exists and was not installed by decebal: it is left alone (remove it yourself, or install under another --name)", target.display())),
        Existing::Ours(p) if p.pack != req.pack && !req.force => {
            return Err(format!("{} holds the pack `{}`; --force replaces it with `{}`", target.display(), p.pack, req.pack));
        }
        _ => {}
    }
    let staging = staging_dir(&req.root)?;
    let _lock = lock(&staging)?;
    let job = new_job(&staging, name)?;
    let (src_dir, out) = (job.dir.join("src"), job.dir.join("out"));
    let files = req.spec.license_files.iter().chain(req.spec.index_files.iter().map(|(p, _)| p)).cloned().collect();
    let filter = Filter { strip: req.strip, path: req.spec.path.clone(), files, link_sources: req.link_sources.clone() };
    let mut rec = req.source.clone();
    let (staged, archive_sha) = match &req.input {
        Input::Dir(d) => {
            rec.fetched_from = "directory".into();
            (source::from_dir(d, src_dir, filter, lim, &assemble::keep)?, None)
        }
        Input::Archive(_) | Input::Download(_) => {
            let file = match &req.input {
                Input::Archive(p) => {
                    rec.fetched_from = "archive".into();
                    p.clone()
                }
                Input::Download(url) => {
                    rec.fetched_from = "download".into();
                    rec.url = Some(url.clone());
                    let f = job.dir.join("download");
                    let (sha, _) = fetch::download(url, &f, lim.max_archive)?;
                    if let Some(want) = &req.pins.archive_sha256 {
                        if *want != sha {
                            return Err(format!("archive sha256 mismatch: pinned {want}, downloaded {sha}"));
                        }
                    }
                    f
                }
                Input::Dir(_) => unreachable!(),
            };
            let sha = fetch::hash_file(&file, lim.max_archive)?;
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
    rec.tree = staged.tree.clone();
    rec.archive_sha256 = archive_sha;
    if rec.commit.is_none() || verified.commit {
        rec.commit = staged.commit.clone().or(req.pins.commit.clone());
    } else {
        rec.commit = req.pins.commit.clone();
    }
    let mut built = assemble::build(&staged, &req.spec, &out, lim)?;
    built.warnings.splice(0..0, staged.warnings.iter().cloned());
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
        files,
        symlinks: links,
        skipped: built.skipped,
        installed_by: concat!("decebal ", env!("CARGO_PKG_VERSION")).into(),
    };
    let report = |action| Report { target: target.clone(), action, built: built.clone(), provenance: prov.clone(), shadows: shadows.clone(), missing_parents: missing_parents.clone() };
    if let Existing::Ours(p) = &before {
        // compare with what is on disk now, not with the record: a modified install is not "unchanged"
        let on_disk = assemble::content_hash(&target).map(|h| h.0).unwrap_or_default();
        if p.pack == req.pack && on_disk == prov.content_hash && !req.force {
            return Ok(report(Action::Unchanged));
        }
        if !req.force && !req.dry_run && p.pack == req.pack && p.content_hash == prov.content_hash {
            return Err(format!("{} was modified since it was installed (decthemectl assets verify): --force restores it", target.display()));
        }
        if !req.force && !req.dry_run {
            return Err(format!(
                "{} holds `{}` from {} ({}); this one is {}: --force replaces it",
                target.display(),
                p.pack,
                p.source.commit.as_deref().map_or("an unknown revision".to_string(), |c| c[..12.min(c.len())].to_string()),
                p.content_hash,
                prov.content_hash
            ));
        }
    }
    if req.dry_run {
        return Ok(report(Action::DryRun));
    }
    let text = serde_json::to_string_pretty(&prov).map_err(|e| e.to_string())? + "\n";
    std::fs::write(out.join(PROVENANCE), text).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    // the target may have changed while we worked: decide again under the lock
    let replace = match existing(&req.root, name) {
        Existing::Absent => false,
        Existing::Ours(_) => true,
        Existing::Foreign => return Err(format!("{} appeared while installing and was not installed by decebal", target.display())),
    };
    place(&out, &target, replace, &job.dir.join("old"))?;
    Ok(report(if replace { Action::Replaced } else { Action::Installed }))
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

/// Removes a decebal pack: it is renamed into the staging folder first, so no half-deleted theme is ever visible.
pub fn remove(root: &Path, name: &str) -> Result<PathBuf, String> {
    if !super::safe_theme_name(name) {
        return Err(format!("`{name}` is not a theme folder name"));
    }
    let target = root.join(name);
    match existing(root, name) {
        Existing::Absent => return Err(format!("{} does not exist", target.display())),
        Existing::Foreign => return Err(format!("{} was not installed by decebal: it is left alone", target.display())),
        Existing::Ours(_) => {}
    }
    let staging = staging_dir(root)?;
    let _lock = lock(&staging)?;
    let job = new_job(&staging, name)?;
    std::fs::rename(&target, job.dir.join("old")).map_err(|e| format!("{}: {e}", target.display()))?;
    drop(job);
    Ok(target)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::testkit::*;
    use super::*;
    use std::cell::Cell;

    thread_local! {
        pub static NO_RENAME2: Cell<bool> = const { Cell::new(false) };
        pub static FAIL_SECOND_RENAME: Cell<bool> = const { Cell::new(false) };
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
                index_files: vec![],
                xcursors: vec![],
            },
            strip: 1,
            link_sources: vec![],
            input: Input::Archive(archive.to_path_buf()),
            pins: Pins { tree: Some(tree_of(entries, "top/bloom")), archive_sha256: Some(fetch::hash_file(archive, 1 << 30).unwrap()), commit: Some(COMMIT.into()) },
            source: SourceRecord { id: "fixture".into(), repo: "https://example.org/fixture".into(), revision: "main".into(), path: "bloom".into(), ..SourceRecord::default() },
            license: "GPL-3.0-or-later".into(),
            root: root.to_path_buf(),
            force: false,
            dry_run: false,
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
        assert_eq!((p.verified, p.source.commit.as_deref(), p.source.fetched_from.as_str()), (Verified { tree: true, archive_sha256: true, commit: true }, Some(COMMIT), "archive"));
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
        remove(&root, "bloom").unwrap();
        assert!(!t.exists() && staging_is_clean(&root));
        assert!(remove(&root, "bloom").is_err());
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
        assert!(remove(&root, "bloom").unwrap_err().contains("left alone"));
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
        FAIL_SECOND_RENAME.with(|c| c.set(true));
        let e = install(&r, &Limits::default()).unwrap_err();
        FAIL_SECOND_RENAME.with(|c| c.set(false));
        assert!(e.contains("previous version was put back"), "{e}");
        assert_eq!(Provenance::read(&root.join("bloom")).unwrap().content_hash, before);
        assert!(staging_is_clean(&root));
        assert_eq!(install(&r, &Limits::default()).unwrap().action, Action::Replaced, "the fallback path works");
        NO_RENAME2.with(|c| c.set(false));
        assert!(root.join("bloom/apps/48/new.svg").exists());
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
            index_files: vec![],
            xcursors: vec![super::super::xcursor::Cursor {
                name: "left_ptr".into(),
                images: super::super::xcursor::parse_config("24 4 0 24x24/left_ptr.png\n32 5 0 32x32/left_ptr.png").unwrap(),
                aliases: vec!["default".into(), "top_left_arrow".into()],
            }],
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
