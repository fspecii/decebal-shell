//! File edits for the system theme pipeline: the three ways a target may change a file, atomic writes,
//! and the state (manifest + backups) that lets `macthemectl system off` put everything back.
//!
//! * [`Edit::Whole`]: macshell owns the file (colour scheme files, the generated Omarchy theme, ...).
//! * [`Edit::Block`]: a marked block inside a file that also holds the user's own content (`gtk.css`, `~/.Xresources`).
//! * [`Edit::Ini`]: a few keys of an INI file (`settings.ini`, `qt6ct.conf`); every other line stays.
//!
//! The first time a file is touched its original bytes (or "did not exist") are saved under the state directory.
//! `off` puts the original back when the file still holds exactly what macshell wrote; when the user edited it in the
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

const MARK: &str = "macshell system theme";

impl Comment {
    fn line(self, s: &str) -> String {
        match self {
            Comment::Css => format!("/* {s} */"),
            Comment::Hash => format!("# {s}"),
            Comment::Bang => format!("! {s}"),
        }
    }
    fn begin(self) -> String {
        self.line(&format!(">>> {MARK} (managed by macthemectl; `macthemectl system off` removes it) >>>"))
    }
    fn end(self) -> String {
        self.line(&format!("<<< {MARK} <<<"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Whole { path: PathBuf, text: String },
    Block { path: PathBuf, comment: Comment, text: String },
    Ini { path: PathBuf, sets: Vec<(String, String, String)> },
}

impl Edit {
    pub fn path(&self) -> &Path {
        match self {
            Edit::Whole { path, .. } | Edit::Block { path, .. } | Edit::Ini { path, .. } => path,
        }
    }
    fn kind(&self) -> &'static str {
        match self {
            Edit::Whole { .. } => "whole",
            Edit::Block { .. } => "block",
            Edit::Ini { .. } => "ini",
        }
    }
    /// The file's next content given its current one.
    pub fn render(&self, cur: Option<&str>) -> String {
        match self {
            Edit::Whole { text, .. } => text.clone(),
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
    let (b, e) = (c.begin(), c.end());
    let Some(s) = cur.find(&b) else { return cur.to_string() };
    let Some(rel) = cur[s..].find(&e) else { return cur.to_string() };
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

/// FNV-1a: a stable fingerprint of what macshell wrote.
pub fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, x| (h ^ *x as u64).wrapping_mul(0x100000001b3))
}

/// Temp file in the target's directory, then rename. Symlinks are written through; permissions are kept.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = target.parent().ok_or("path has no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.macshell-{}.tmp", std::process::id()));
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

/// What macshell recorded about one file it changed.
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
    /// Directories macshell created (removed again when empty).
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
            if let Edit::Whole { .. } = e {
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
pub fn commit(changes: &[Change], state_dir: &Path, st: &mut State) -> Result<Vec<PathBuf>, String> {
    let bdir = state_dir.join("backups");
    let mut done: Vec<(&Change, bool)> = Vec::new();
    let mut wrote = Vec::new();
    let mut new_state = st.clone();
    for c in changes {
        if c.is_noop() {
            continue;
        }
        let key = c.path.to_string_lossy().into_owned();
        let first = !new_state.files.contains_key(&key);
        let mut res: Result<(), String> = Ok(());
        let mut orig = None;
        if first {
            if let Some(old) = &c.old {
                let n = backup_name(&c.path);
                res = std::fs::create_dir_all(&bdir).map_err(|e| format!("{}: {e}", bdir.display())).and_then(|_| write_atomic(&bdir.join(&n), old));
                orig = Some(n);
            } else {
                let mut p = c.path.parent();
                let mut missing = Vec::new();
                while let Some(d) = p {
                    if d.exists() {
                        break;
                    }
                    missing.push(d.to_string_lossy().into_owned());
                    p = d.parent();
                }
                for d in missing.into_iter().rev() {
                    if !new_state.dirs.contains(&d) {
                        new_state.dirs.push(d);
                    }
                }
            }
        }
        if res.is_ok() {
            res = write_atomic(&c.path, &c.new);
        }
        if let Err(e) = res {
            for (d, existed) in done.into_iter().rev() {
                match (&d.old, existed) {
                    (Some(o), _) => {
                        let _ = write_atomic(&d.path, o);
                    }
                    (None, _) => {
                        let _ = std::fs::remove_file(&d.path);
                    }
                }
            }
            return Err(e);
        }
        done.push((c, first));
        if first {
            new_state.files.insert(key.clone(), Entry { orig, written: 0, kind: c.edit.kind().into(), comment: None, owner: c.owner.clone() });
        }
        let ent = new_state.files.get_mut(&key).unwrap();
        ent.written = fnv(c.new.as_bytes());
        ent.kind = c.edit.kind().into();
        ent.comment = if let Edit::Block { comment, .. } = &c.edit { Some(*comment) } else { ent.comment };
        wrote.push(c.path.clone());
    }
    *st = new_state;
    st.save(state_dir)?;
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
        let outcome = if untouched || cur.is_none() {
            match &ent.orig {
                Some(n) => match std::fs::read_to_string(bdir.join(n)) {
                    Ok(o) => write_atomic(&path, &o).map(|_| Restored::Exact),
                    Err(e) => Err(e.to_string()),
                },
                None => std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }).map(|_| Restored::Exact).map_err(|e| e.to_string()),
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
                report.push((k, r));
            }
            Err(e) => report.push((format!("{k}: {e}"), Restored::Kept)),
        }
    }
    // directories macshell created, deepest first, only if empty
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
        assert!(a.starts_with(user) && a.contains("accent_bg_color #123456") && a.contains(">>> macshell system theme"));
        let b = with_block(&a, Comment::Css, "@define-color accent_bg_color #654321;\n");
        assert_eq!(b.matches("macshell system theme (managed").count(), 1);
        assert!(b.contains("#654321") && !b.contains("#123456"));
        assert_eq!(strip_block(&b, Comment::Css), user);
        assert_eq!(strip_block(&with_block("", Comment::Hash, "x\n"), Comment::Hash), "");
        assert_eq!(strip_block("no block here\n", Comment::Css), "no block here\n");
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
        assert!(!c.exists() && !nested.exists() && !d.join("new").exists(), "files and directories macshell created are gone");
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
