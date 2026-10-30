//! File edits for the system theme pipeline: managed edits, atomic writes,
//! and the state (manifest + backups) that lets `decthemectl system off` put everything back.
//!
//! * [`Edit::Whole`]: decebal owns the file (colour scheme files, the generated Omarchy theme, ...).
//! * [`Edit::Detached`]: replace a known legacy GTK stylepack symlink with a managed local CSS block.
//!   `extra.detached_links` retains its exact link destination for off; ordinary dotfile links stay linked.
//!   Old whole-file backups repair the shared pack only when its recorded written hash still matches.
//! * [`Edit::Block`]: a marked block inside a file that also holds the user's own content (`gtk.css`, `~/.Xresources`).
//! * [`Edit::Ini`]: a few keys of an INI file (`settings.ini`, `qt6ct.conf`); every other line stays.
//!
//! The first time a file is touched its original bytes (or "did not exist") are saved under the state directory.
//! `off` puts the original back when the file still holds exactly what decebal wrote; when the user edited it in the
//! meantime a block is cut out and anything else is left alone and reported.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Comment syntax of a managed block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comment {
    /// `/* ... */` (CSS)
    Css,
    /// `# ...` (INI, shell, TOML)
    Hash,
    /// `! ...` (Xresources)
    Bang,
}

const MARK: &str = "decebal system theme";

impl Comment {
    fn line(self, s: &str) -> String {
        match self {
            Comment::Css => format!("/* {s} */"),
            Comment::Hash => format!("# {s}"),
            Comment::Bang => format!("! {s}"),
        }
    }
    fn begin(self) -> String {
        self.line(&format!(">>> {MARK} (managed by decthemectl; `decthemectl system off` removes it) >>>"))
    }
    fn end(self) -> String {
        self.line(&format!("<<< {MARK} <<<"))
    }
    /// The markers the pre-rename version wrote ("macshell"); still recognised so an update replaces or removes those blocks.
    fn legacy_begin(self) -> String {
        self.line(">>> macshell system theme (managed by macthemectl; `macthemectl system off` removes it) >>>")
    }
    fn legacy_end(self) -> String {
        self.line("<<< macshell system theme <<<")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Whole {
        path: PathBuf,
        text: String,
    },
    /// Replace a known legacy stylepack link itself; never write through it.
    Detached {
        path: PathBuf,
        text: String,
    },
    Block {
        path: PathBuf,
        comment: Comment,
        text: String,
    },
    Ini {
        path: PathBuf,
        sets: Vec<(String, String, String)>,
    },
}

impl Edit {
    pub fn path(&self) -> &Path {
        match self {
            Edit::Whole { path, .. } | Edit::Detached { path, .. } | Edit::Block { path, .. } | Edit::Ini { path, .. } => path,
        }
    }
    fn kind(&self) -> &'static str {
        match self {
            Edit::Whole { .. } => "whole",
            Edit::Detached { .. } => "whole",
            Edit::Block { .. } => "block",
            Edit::Ini { .. } => "ini",
        }
    }
    /// The file's next content given its current one.
    pub fn render(&self, cur: Option<&str>) -> String {
        match self {
            Edit::Whole { text, .. } => text.clone(),
            Edit::Detached { text, .. } => with_block("", Comment::Css, text),
            Edit::Block { comment, text, .. } => with_block(cur.unwrap_or(""), *comment, text),
            Edit::Ini { sets, .. } => {
                let mut t = cur.unwrap_or("").to_string();
                for (s, k, v) in sets {
                    t = ini_set(&t, s, k, v);
                }
                t
            }
        }
    }
}

// ---------------------------------------------------------------- blocks

/// `text` with the managed block replaced (or appended when there is none).
pub fn with_block(cur: &str, c: Comment, body: &str) -> String {
    let stripped = strip_block(cur, c);
    let mut out = stripped;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&c.begin());
    out.push('\n');
    out.push_str(body.trim_end_matches('\n'));
    out.push('\n');
    out.push_str(&c.end());
    out.push('\n');
    out
}

/// `text` without the managed block (and the blank line that separated it).
pub fn strip_block(cur: &str, c: Comment) -> String {
    let once = strip_block_between(cur, &c.begin(), &c.end());
    strip_block_between(&once, &c.legacy_begin(), &c.legacy_end())
}

fn strip_block_between(cur: &str, b: &str, e: &str) -> String {
    let Some(s) = cur.find(b) else { return cur.to_string() };
    let Some(rel) = cur[s..].find(e) else { return cur.to_string() };
    let mut end = s + rel + e.len();
    if cur[end..].starts_with('\n') {
        end += 1;
    }
    let mut head = cur[..s].to_string();
    if head.ends_with("\n\n") {
        head.pop();
    }
    if head == "\n" {
        head.clear();
    }
    head + &cur[end..]
}

// ---------------------------------------------------------------- ini

fn is_section(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[').and_then(|r| r.strip_suffix(']'))
}

fn key_of(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with('#') || t.starts_with(';') {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

/// Sets `key=value` in `[section]`, adding the section or key when missing. Everything else is kept byte for byte.
pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut in_sec = false;
    let mut sec_end: Option<usize> = None;
    let mut found = false;
    #[allow(clippy::needless_range_loop)] // lines[i] is rewritten in place
    for i in 0..lines.len() {
        if let Some(s) = is_section(&lines[i]) {
            if in_sec {
                break;
            }
            in_sec = s == section;
            if in_sec {
                sec_end = Some(i + 1);
            }
            continue;
        }
        if in_sec {
            if key_of(&lines[i]) == Some(key) {
                lines[i] = format!("{key}={value}");
                found = true;
                break;
            }
            if !lines[i].trim().is_empty() {
                sec_end = Some(i + 1);
            }
        }
    }
    if !found {
        match sec_end {
            Some(at) => lines.insert(at, format!("{key}={value}")),
            None => {
                if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("[{section}]"));
                lines.push(format!("{key}={value}"));
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// `key` of `[section]`, if set.
pub fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_sec = false;
    for l in text.lines() {
        if let Some(s) = is_section(l) {
            in_sec = s == section;
        } else if in_sec && key_of(l) == Some(key) {
            return l.split_once('=').map(|(_, v)| v.trim().to_string());
        }
    }
    None
}

// ---------------------------------------------------------------- hashing and atomic writes

/// FNV-1a: a stable fingerprint of what decebal wrote.
pub fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, x| (h ^ *x as u64).wrapping_mul(0x100000001b3))
}

/// Temp file in the target's directory, then rename. Resolvable symlinks are written through; permissions are kept.
/// Unresolved symlinks are rejected rather than replaced with a regular file.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    write_atomic_mode(path, text, true)
}

fn write_atomic_mode(path: &Path, text: &str, follow: bool) -> Result<(), String> {
    let target = if follow {
        match std::fs::canonicalize(path) {
            Ok(target) => target,
            Err(e) if path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) => {
                return Err(format!("{}: cannot resolve symlink: {e}", path.display()));
            }
            Err(_) => path.to_path_buf(),
        }
    } else {
        path.to_path_buf()
    };
    let dir = target.parent().ok_or("path has no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.decebal-{}.tmp", std::process::id()));
    let res = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        if let Ok(meta) = std::fs::metadata(&target) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        std::fs::rename(&tmp, &target)
    })();
    res.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

// ---------------------------------------------------------------- state

/// What decebal recorded about one file it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Name of the saved original under `backups/`, `None` when the file did not exist.
    pub orig: Option<String>,
    pub written: u64,
    pub kind: String,
    pub comment: Option<Comment>,
    /// The target (`gtk`, `qt`, ...) that wrote it; switching that target off restores only its files.
    pub owner: String,
}

/// The manifest: everything `off` needs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub files: BTreeMap<String, Entry>,
    /// Directories decebal created (removed again when empty).
    pub dirs: Vec<String>,
    /// Extra recorded values: gsettings and xrdb originals, the previous Omarchy theme, ...
    pub extra: serde_json::Map<String, Value>,
}

impl State {
    pub fn load(dir: &Path) -> State {
        let v: Value = std::fs::read_to_string(dir.join("manifest.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
        let mut st = State::default();
        if let Some(o) = v["files"].as_object() {
            for (k, e) in o {
                st.files.insert(
                    k.clone(),
                    Entry {
                        orig: e["orig"].as_str().map(String::from),
                        written: e["written"].as_str().and_then(|s| u64::from_str_radix(s, 16).ok()).unwrap_or(0),
                        kind: e["kind"].as_str().unwrap_or("whole").to_string(),
                        owner: e["owner"].as_str().unwrap_or_default().to_string(),
                        comment: match e["comment"].as_str() {
                            Some("css") => Some(Comment::Css),
                            Some("hash") => Some(Comment::Hash),
                            Some("bang") => Some(Comment::Bang),
                            _ => None,
                        },
                    },
                );
            }
        }
        st.dirs = v["dirs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        st.extra = v["extra"].as_object().cloned().unwrap_or_default();
        st
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let files: serde_json::Map<String, Value> = self
            .files
            .iter()
            .map(|(k, e)| {
                let c = match e.comment {
                    Some(Comment::Css) => json!("css"),
                    Some(Comment::Hash) => json!("hash"),
                    Some(Comment::Bang) => json!("bang"),
                    None => Value::Null,
                };
                (k.clone(), json!({"orig": e.orig, "written": format!("{:016x}", e.written), "kind": e.kind, "comment": c, "owner": e.owner}))
            })
            .collect();
        let v = json!({"version": 1, "files": files, "dirs": self.dirs, "extra": self.extra});
        let mut text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        text.push('\n');
        write_atomic(&dir.join("manifest.json"), &text)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.extra.is_empty()
    }
}

fn backup_name(path: &Path) -> String {
    format!("{:016x}.orig", fnv(path.to_string_lossy().as_bytes()))
}

/// One planned write: what the file holds now and what it will hold.
#[derive(Debug, Clone)]
pub struct Change {
    pub path: PathBuf,
    pub old: Option<String>,
    pub new: String,
    pub edit: Edit,
    /// The target that asked for it.
    pub owner: String,
}

impl Change {
    pub fn is_noop(&self) -> bool {
        self.old.as_deref() == Some(self.new.as_str())
    }
}

/// Computes the content every edit would produce, without touching anything. Edits to the same file compose.
pub fn plan(edits: &[Edit]) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    for e in edits {
        let prior = out.iter().rposition(|c| c.path == e.path());
        let (old, cur) = match prior {
            Some(i) => (out[i].old.clone(), Some(out[i].new.clone())),
            None => {
                let o = std::fs::read_to_string(e.path()).ok();
                (o.clone(), o)
            }
        };
        let new = e.render(cur.as_deref());
        if let Some(i) = prior {
            out[i].new = new;
            if matches!(e, Edit::Whole { .. } | Edit::Detached { .. }) {
                out[i].edit = e.clone();
            }
        } else {
            out.push(Change { path: e.path().to_path_buf(), old, new, edit: e.clone(), owner: String::new() });
        }
    }
    out
}

/// Writes the planned changes. The first time a file is touched its original is saved. If a write fails the files
/// already written by this call are put back and nothing is recorded.
// Snapshot the directory entry as well as its contents, so rollback can restore links.
#[derive(Clone)]
enum Snapshot {
    Missing,
    Link(PathBuf),
    File(String),
}
impl Snapshot {
    fn read(path: &Path) -> Result<Self, String> {
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => std::fs::read_link(path).map(Self::Link).map_err(|e| e.to_string()),
            Ok(_) => std::fs::read_to_string(path).map(Self::File).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(e) => Err(e.to_string()),
        }
    }
    fn restore(&self, path: &Path) -> Result<(), String> {
        match self {
            Self::File(text) => write_atomic_mode(path, text, false),
            Self::Missing => std::fs::remove_file(path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }).map_err(|e| e.to_string()),
            Self::Link(target) => {
                let tmp = path.with_extension(format!("decebal-link-{}", std::process::id()));
                std::os::unix::fs::symlink(target, &tmp).map_err(|e| e.to_string())?;
                let result = std::fs::rename(&tmp, path).map_err(|e| e.to_string());
                if result.is_err() {
                    let _ = std::fs::remove_file(tmp);
                }
                result
            }
        }
    }
}

pub fn commit(changes: &[Change], state_dir: &Path, st: &mut State) -> Result<Vec<PathBuf>, String> {
    let bdir = state_dir.join("backups");
    let mut done: Vec<(PathBuf, Snapshot)> = Vec::new();
    let mut wrote = Vec::new();
    let mut next = st.clone();
    let result = (|| -> Result<(), String> {
        for c in changes {
            let detach = matches!(c.edit, Edit::Detached { .. });
            if c.is_noop() && !detach {
                continue;
            }
            let key = c.path.to_string_lossy().into_owned();
            let snapshot = Snapshot::read(&c.path)?;
            // Older versions saved the shared pack's original under either the link
            // name or its canonical target. Recover it only while its written hash matches.
            if detach {
                let Snapshot::Link(link) = &snapshot else {
                    return Err(format!("{}: legacy link changed during apply", c.path.display()));
                };
                let target = c.path.canonicalize().map_err(|e| e.to_string())?;
                let target_key = target.to_string_lossy().into_owned();
                let old_key = if next.files.contains_key(&key) { key.clone() } else { target_key.clone() };
                if let Some(old) = next.files.get(&old_key).cloned() {
                    if old.owner != c.owner {
                        return Err(format!("{}: legacy backup belongs to another target", c.path.display()));
                    }
                    if let Some(name) = old.orig {
                        let original = std::fs::read_to_string(bdir.join(name)).map_err(|e| e.to_string())?;
                        let current = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
                        if fnv(current.as_bytes()) == old.written {
                            done.push((target.clone(), Snapshot::File(current)));
                            write_atomic(&target, &original)?;
                        }
                    }
                    next.files.remove(&old_key);
                }
                next.extra.entry("detached_links").or_insert_with(|| json!({})).as_object_mut().ok_or("invalid detached_links state")?.insert(key.clone(), json!(link));
            }
            let first = !next.files.contains_key(&key);
            let mut orig = None;
            if first {
                if let Some(old) = &c.old {
                    let n = backup_name(&c.path);
                    std::fs::create_dir_all(&bdir).map_err(|e| e.to_string())?;
                    let backup = bdir.join(&n);
                    done.push((backup.clone(), Snapshot::read(&backup)?));
                    write_atomic(&backup, old)?;
                    orig = Some(n);
                } else {
                    let mut p = c.path.parent();
                    while let Some(d) = p {
                        if d.exists() {
                            break;
                        }
                        let d = d.to_string_lossy().into_owned();
                        if !next.dirs.contains(&d) {
                            next.dirs.push(d);
                        }
                        p = p.and_then(Path::parent);
                    }
                }
            }
            // Ordinary dotfile symlinks retain the existing write-through policy.
            let rollback_path = if !detach { c.path.canonicalize().unwrap_or_else(|_| c.path.clone()) } else { c.path.clone() };
            let rollback_snapshot = if !detach { Snapshot::read(&rollback_path)? } else { snapshot };
            done.push((rollback_path, rollback_snapshot));
            write_atomic_mode(&c.path, &c.new, !detach)?;
            if first {
                next.files.insert(key.clone(), Entry { orig, written: 0, kind: c.edit.kind().into(), comment: None, owner: c.owner.clone() });
            }
            let ent = next.files.get_mut(&key).unwrap();
            ent.written = fnv(c.new.as_bytes());
            ent.kind = c.edit.kind().into();
            ent.comment = match &c.edit {
                Edit::Block { comment, .. } => Some(*comment),
                Edit::Detached { .. } => Some(Comment::Css),
                _ => ent.comment,
            };
            wrote.push(c.path.clone());
        }
        next.save(state_dir)
    })();
    if let Err(e) = result {
        let mut errors = Vec::new();
        for (path, snapshot) in done.into_iter().rev() {
            if let Err(e) = snapshot.restore(&path) {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        return Err(if errors.is_empty() { e } else { format!("{e}; rollback failed: {}", errors.join("; ")) });
    }
    *st = next;
    Ok(wrote)
}

/// Result of restoring one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restored {
    /// Original bytes written back (or the file we created removed).
    Exact,
    /// The file changed since; only our block was cut out.
    BlockRemoved,
    /// The file changed since and holds nothing of ours that can be separated: left alone.
    Kept,
}

/// Puts every recorded file back (only those of `owner` when given). Entries restored are dropped from the manifest.
pub fn restore(state_dir: &Path, st: &mut State, owner: Option<&str>) -> Vec<(String, Restored)> {
    let bdir = state_dir.join("backups");
    let mut report = Vec::new();
    let keys: Vec<String> = st.files.iter().rev().filter(|(_, e)| owner.is_none_or(|o| e.owner == o)).map(|(k, _)| k.clone()).collect();
    for k in keys {
        let ent = st.files[&k].clone();
        let path = PathBuf::from(&k);
        let cur = std::fs::read_to_string(&path).ok();
        let untouched = cur.as_deref().map(|c| fnv(c.as_bytes())) == Some(ent.written);
        let detached = st.extra.get("detached_links").and_then(|v| v.get(&k)).is_some();
        let missing = path.symlink_metadata().is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        // An unreadable or non-UTF-8 local file can contain user edits. Only a
        // genuinely absent detached entry may be restored without a matching hash.
        let outcome = if untouched || (cur.is_none() && (!detached || missing)) {
            if let Some(link) = st.extra.get("detached_links").and_then(|v| v.get(&k)).and_then(Value::as_str) {
                Snapshot::Link(PathBuf::from(link)).restore(&path).map(|_| Restored::Exact)
            } else {
                match &ent.orig {
                    Some(n) => match std::fs::read_to_string(bdir.join(n)) {
                        Ok(o) => write_atomic(&path, &o).map(|_| Restored::Exact),
                        Err(e) => Err(e.to_string()),
                    },
                    None => std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }).map(|_| Restored::Exact).map_err(|e| e.to_string()),
                }
            }
        } else if let (Some(c), Some(cur)) = (ent.comment, cur.as_deref()) {
            let stripped = strip_block(cur, c);
            if ent.orig.is_none() && stripped.trim().is_empty() {
                std::fs::remove_file(&path).map(|_| Restored::BlockRemoved).map_err(|e| e.to_string())
            } else {
                write_atomic(&path, &stripped).map(|_| Restored::BlockRemoved)
            }
        } else {
            Ok(Restored::Kept)
        };
        match outcome {
            Ok(r) => {
                if let Some(n) = &ent.orig {
                    let _ = std::fs::remove_file(bdir.join(n));
                }
                st.files.remove(&k);
                if let Some(links) = st.extra.get_mut("detached_links").and_then(Value::as_object_mut) {
                    links.remove(&k);
                    if links.is_empty() {
                        st.extra.remove("detached_links");
                    }
                }
                report.push((k, r));
            }
            Err(e) => report.push((format!("{k}: {e}"), Restored::Kept)),
        }
    }
    // directories decebal created, deepest first, only if empty
    if owner.is_some() && !st.files.is_empty() {
        return report;
    }
    let mut dirs = std::mem::take(&mut st.dirs);
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    for d in dirs {
        if std::fs::remove_dir(&d).is_err() && Path::new(&d).exists() {
            st.dirs.push(d);
        }
    }
    let _ = std::fs::remove_dir(&bdir);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::theme::system::testutil::tmp;

    #[test]
    fn blocks_append_replace_and_strip_cleanly() {
        let user = "@define-color mine #fff;\n";
        let a = with_block(user, Comment::Css, "@define-color accent_bg_color #123456;\n");
        assert!(a.starts_with(user) && a.contains("accent_bg_color #123456") && a.contains(">>> decebal system theme"));
        let b = with_block(&a, Comment::Css, "@define-color accent_bg_color #654321;\n");
        assert_eq!(b.matches("decebal system theme (managed").count(), 1);
        assert!(b.contains("#654321") && !b.contains("#123456"));
        assert_eq!(strip_block(&b, Comment::Css), user);
        assert_eq!(strip_block(&with_block("", Comment::Hash, "x\n"), Comment::Hash), "");
        assert_eq!(strip_block("no block here\n", Comment::Css), "no block here\n");
    }

    #[test]
    fn blocks_written_before_the_rename_are_replaced_and_stripped() {
        let user = "@define-color mine #fff;\n";
        let old = format!(
            "{user}\n/* >>> macshell system theme (managed by macthemectl; `macthemectl system off` removes it) >>> */\n@define-color accent_bg_color #123456;\n/* <<< macshell system theme <<< */\n"
        );
        assert_eq!(strip_block(&old, Comment::Css), user);
        let new = with_block(&old, Comment::Css, "@define-color accent_bg_color #654321;\n");
        assert!(new.contains("#654321") && !new.contains("#123456") && !new.contains("macshell") && new.starts_with(user), "{new}");
    }

    #[test]
    fn ini_set_keeps_everything_else() {
        let t = "# top\n[A]\nx = 1\n\n[Settings]\ngtk-font-name=Old 10\n# note\n\n[Z]\nq=2\n";
        let r = ini_set(t, "Settings", "gtk-font-name", "Inter 11");
        assert_eq!(r, "# top\n[A]\nx = 1\n\n[Settings]\ngtk-font-name=Inter 11\n# note\n\n[Z]\nq=2\n");
        let r = ini_set(t, "Settings", "gtk-new", "1");
        assert!(r.contains("[Settings]\ngtk-font-name=Old 10\n# note\ngtk-new=1\n\n[Z]"), "{r}");
        assert_eq!(ini_set("", "S", "k", "v"), "[S]\nk=v\n");
        assert_eq!(ini_set("[A]\nx=1\n", "S", "k", "v"), "[A]\nx=1\n\n[S]\nk=v\n");
        assert_eq!(ini_get(&r, "Settings", "gtk-new").as_deref(), Some("1"));
    }

    #[test]
    fn commit_then_restore_is_exact_for_all_three_kinds() {
        let d = tmp("roundtrip");
        let (state, a, b, c, nested) = (d.join("state"), d.join("a.css"), d.join("b.ini"), d.join("c.conf"), d.join("new/dir/x.conf"));
        std::fs::write(&a, "body { }\n").unwrap();
        std::fs::write(&b, "[Settings]\nk=old\n").unwrap();
        let edits = vec![
            Edit::Block { path: a.clone(), comment: Comment::Css, text: "@define-color x #000;\n".into() },
            Edit::Ini { path: b.clone(), sets: vec![("Settings".into(), "k".into(), "new".into()), ("Settings".into(), "j".into(), "1".into())] },
            Edit::Whole { path: c.clone(), text: "own\n".into() },
            Edit::Whole { path: nested.clone(), text: "deep\n".into() },
        ];
        let mut st = State::default();
        let ch = plan(&edits);
        assert_eq!(commit(&ch, &state, &mut st).unwrap().len(), 4);
        assert!(std::fs::read_to_string(&a).unwrap().contains("define-color x"));
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "[Settings]\nk=new\nj=1\n");
        // idempotent: a second plan against the written files is all no-ops and writes nothing
        let again = plan(&edits);
        assert!(again.iter().all(Change::is_noop));
        assert!(commit(&again, &state, &mut st).unwrap().is_empty());
        // no temp files left behind
        assert!(std::fs::read_dir(&d).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")));
        let mut st2 = State::load(&state);
        assert_eq!(st2.files.len(), 4);
        let rep = restore(&state, &mut st2, None);
        assert!(rep.iter().all(|(_, r)| *r == Restored::Exact), "{rep:?}");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "body { }\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "[Settings]\nk=old\n");
        assert!(!c.exists() && !nested.exists() && !d.join("new").exists(), "files and directories decebal created are gone");
        assert!(st2.files.is_empty());
    }

    #[test]
    fn user_edits_after_apply_are_not_clobbered_by_off() {
        let d = tmp("edited");
        let (state, a, c) = (d.join("state"), d.join("gtk.css"), d.join("own.conf"));
        std::fs::write(&a, "a {}\n").unwrap();
        let mut st = State::default();
        commit(&plan(&[Edit::Block { path: a.clone(), comment: Comment::Css, text: "b {}\n".into() }, Edit::Whole { path: c.clone(), text: "ours\n".into() }]), &state, &mut st).unwrap();
        let mut now = std::fs::read_to_string(&a).unwrap();
        now.push_str("/* user */\nc {}\n");
        std::fs::write(&a, &now).unwrap();
        std::fs::write(&c, "edited by user\n").unwrap();
        let rep = restore(&state, &mut st, None);
        assert!(rep.iter().any(|(p, r)| p.ends_with("gtk.css") && *r == Restored::BlockRemoved));
        assert!(rep.iter().any(|(p, r)| p.ends_with("own.conf") && *r == Restored::Kept));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a {}\n/* user */\nc {}\n");
        assert_eq!(std::fs::read_to_string(&c).unwrap(), "edited by user\n");
    }

    #[test]
    fn a_failing_write_rolls_back_earlier_ones() {
        let d = tmp("rollback");
        let (state, a) = (d.join("state"), d.join("a.conf"));
        std::fs::write(&a, "orig\n").unwrap();
        std::fs::write(d.join("blocker"), "i am a file").unwrap();
        let bad = d.join("blocker/sub/x.conf"); // parent is a file: cannot be created
        let ch = plan(&[Edit::Whole { path: a.clone(), text: "new\n".into() }, Edit::Whole { path: bad, text: "x\n".into() }]);
        let mut st = State::default();
        assert!(commit(&ch, &state, &mut st).is_err());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "orig\n");
        assert!(st.files.is_empty());
    }

    fn legacy_fixture(name: &str) -> (crate::theme::system::testutil::TmpDir, PathBuf, PathBuf, PathBuf) {
        let d = tmp(name);
        let target = d.join("pack/gtk.css");
        let link = d.join("gtk/gtk.css");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::fs::write(&target, "pack original\n").unwrap();
        std::os::unix::fs::symlink("../pack/gtk.css", &link).unwrap();
        let state = d.join("state");
        (d, target, link, state)
    }

    fn detached(path: &Path) -> Vec<Change> {
        let mut changes = plan(&[Edit::Detached { path: path.into(), text: "generated {}\n".into() }]);
        changes[0].owner = "gtk".into();
        changes
    }

    #[test]
    fn detached_link_reapply_off_and_user_edits() {
        let (d, target, link, state) = legacy_fixture("detach");
        let mut st = State::default();
        commit(&detached(&link), &state, &mut st).unwrap();
        assert!(!link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "pack original\n");
        let reapply = plan(&[Edit::Block { path: link.clone(), comment: Comment::Css, text: "next {}\n".into() }]);
        commit(&reapply, &state, &mut st).unwrap();
        assert!(!std::fs::read_to_string(&link).unwrap().contains("generated"));
        st = State::load(&state);
        assert_eq!(restore(&state, &mut st, None)[0].1, Restored::Exact);
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("../pack/gtk.css"));
        commit(&detached(&link), &state, &mut st).unwrap();
        let css = std::fs::read_to_string(&link).unwrap() + "user {}\n";
        std::fs::write(&link, css).unwrap();
        assert_eq!(restore(&state, &mut st, None)[0].1, Restored::BlockRemoved);
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "user {}\n");
        assert_eq!(std::fs::read_to_string(target).unwrap(), "pack original\n");
        assert!(st.is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn old_manifest_recovery_preserves_user_modified_packs() {
        for canonical in [false, true] {
            for user_edit in [false, true] {
                let (d, target, link, state) = legacy_fixture("detach-migrate");
                let mut st = State::default();
                let key_path = if canonical { &target } else { &link };
                let mut old = plan(&[Edit::Whole { path: key_path.clone(), text: "old generated\n".into() }]);
                old[0].owner = "gtk".into();
                commit(&old, &state, &mut st).unwrap();
                if user_edit {
                    std::fs::write(&target, "user pack\n").unwrap();
                }
                commit(&detached(&link), &state, &mut st).unwrap();
                assert_eq!(std::fs::read_to_string(&target).unwrap(), if user_edit { "user pack\n" } else { "pack original\n" });
                assert!(!st.files.contains_key(&target.to_string_lossy().into_owned()));
                restore(&state, &mut st, None);
                assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
                assert_eq!(std::fs::read_to_string(&target).unwrap(), if user_edit { "user pack\n" } else { "pack original\n" });
                std::fs::remove_dir_all(d).unwrap();
            }
        }
    }

    #[test]
    fn detached_rollback_restores_link_and_pack_and_reports_failures() {
        for block_rollback in [false, true] {
            let (d, target, link, state) = legacy_fixture("detach-rollback");
            let mut st = State::default();
            let mut old = plan(&[Edit::Whole { path: target.clone(), text: "old generated\n".into() }]);
            old[0].owner = "gtk".into();
            commit(&old, &state, &mut st).unwrap();
            let before = st.clone();
            let mut changes = detached(&link);
            let blocker = d.join("blocker");
            std::fs::write(&blocker, "file").unwrap();
            changes.extend(plan(&[Edit::Whole { path: blocker.join("impossible"), text: "x".into() }]));
            if block_rollback {
                std::fs::write(link.with_extension(format!("decebal-link-{}", std::process::id())), "block").unwrap();
            }
            let error = commit(&changes, &state, &mut st).unwrap_err();
            assert_eq!(st, before);
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "old generated\n");
            if block_rollback {
                assert!(error.contains("rollback failed"));
            } else {
                assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("../pack/gtk.css"));
            }
            std::fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn detached_manifest_save_failure_rolls_back_directory_entry() {
        let (d, target, link, state) = legacy_fixture("detach-save-failure");
        std::fs::create_dir_all(state.join("manifest.json")).unwrap();
        let mut st = State::default();
        assert!(commit(&detached(&link), &state, &mut st).is_err());
        assert!(st.is_empty());
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("../pack/gtk.css"));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "pack original\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn ordinary_symlink_target_bytes_roll_back_after_later_failure() {
        let (d, target, link, state) = legacy_fixture("ordinary-link-rollback");
        let fail = d.join("blocker");
        std::fs::write(&fail, "file").unwrap();
        let mut st = State::default();
        let changes = plan(&[Edit::Whole { path: link.clone(), text: "changed\n".into() }, Edit::Whole { path: fail.join("child"), text: "fail\n".into() }]);
        assert!(commit(&changes, &state, &mut st).is_err());
        assert!(st.is_empty());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "pack original\n");
        assert_eq!(std::fs::read_link(link).unwrap(), PathBuf::from("../pack/gtk.css"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn unresolved_ordinary_symlinks_are_rejected_without_losing_links() {
        let (d, target, link, state) = legacy_fixture("dangling-link");
        std::fs::remove_file(&target).unwrap();
        let mut st = State::default();
        let changes = plan(&[Edit::Whole { path: link.clone(), text: "new\n".into() }]);
        assert!(commit(&changes, &state, &mut st).unwrap_err().contains("cannot resolve symlink"));
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("../pack/gtk.css"));
        assert!(!target.exists());
        assert!(st.is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn off_keeps_non_utf8_user_edits_in_detached_file() {
        let (d, target, link, state) = legacy_fixture("detach-nonutf8");
        let mut st = State::default();
        commit(&detached(&link), &state, &mut st).unwrap();
        std::fs::write(&link, [0xff, 0xfe]).unwrap();
        assert_eq!(restore(&state, &mut st, None)[0].1, Restored::Kept);
        assert_eq!(std::fs::read(link).unwrap(), [0xff, 0xfe]);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "pack original\n");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn symlinked_files_are_written_through() {
        let d = tmp("symlink");
        let real = d.join("dotfiles/gtk.css");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "x {}\n").unwrap();
        let link = d.join("gtk.css");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, "y {}\n").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "y {}\n");
    }
}
