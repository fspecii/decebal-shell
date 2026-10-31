//! The staged source of a pack: what an archive or a folder held under the pack's `path` (plus the licence files and
//! `link_sources`), read once as a stream.
//!
//! * Regular files are written below a private staging folder with `O_CREAT | O_EXCL | O_NOFOLLOW`; files whose name
//!   the theme cannot use are hashed but never written.
//! * Symlinks and hard links are **recorded, never created**: nothing on disk can redirect a later write. A link target
//!   must be relative and stay inside the source when resolved; anything else refuses the source.
//! * An entry below a recorded file or link, a second entry with the same path and a file where a folder already
//!   has entries are refused.
//! * The git tree id of `path` is computed from exactly the bytes and modes read.

use super::gitoid::{self, Mode, TreeBuilder};
use super::tar::{self, Kind};
use super::Limits;
use crate::theme::engine::sha256::Sha256;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    File {
        exec: bool,
        size: u64,
        /// Written below the staging folder (a name the theme can use, a licence or a `link_sources` file).
        staged: bool,
        oid: [u8; 20],
        sha256: [u8; 32],
    },
    Link(String),
    Dir,
}

/// Which entries of the source matter.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Leading parts of archive paths to drop (GitHub / Gitee archives wrap everything in one folder).
    pub strip: usize,
    /// The theme folder, source-relative; `.` for the root.
    pub path: String,
    /// Single files to stage as well (licences).
    pub files: Vec<String>,
    /// Folders to stage as well, for links of the theme that point into them.
    pub link_sources: Vec<String>,
    /// Folders of regular files read as alias targets (`[[pack.text_links]]`): staged, with their own tree id.
    pub text_links: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Want {
    /// Under `path`: hashed for the tree id; staged when `keep` says the name is useful.
    Theme(String),
    Extra,
    No,
}

fn under<'a>(p: &'a str, dir: &str) -> Option<&'a str> {
    if dir == "." || dir.is_empty() {
        return Some(p);
    }
    p.strip_prefix(dir).and_then(|r| r.strip_prefix('/'))
}

impl Filter {
    fn want(&self, p: &str) -> Want {
        if let Some(rel) = under(p, &self.path) {
            return Want::Theme(rel.to_string());
        }
        if self.files.iter().any(|f| f == p) || self.link_sources.iter().chain(&self.text_links).any(|d| under(p, d).is_some()) {
            return Want::Extra;
        }
        Want::No
    }
}

/// What a path of the source resolves to after following links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    File(String),
    Dir(String),
    Missing(String),
    /// Leaves the source (absolute target, too many `..`).
    Escapes,
    /// More than [`super::MAX_HOPS`] links.
    Loop,
}

#[derive(Debug)]
pub struct Staged {
    /// Where staged files live: `<dir>/<source-relative path>`.
    pub dir: PathBuf,
    pub filter: Filter,
    /// Every wanted entry, source-relative.
    pub nodes: BTreeMap<String, Node>,
    /// `git rev-parse <commit>:<path>` of what was read; `None` when `path` held no file.
    pub tree: Option<String>,
    /// The git tree id of every `link_sources` folder, read from the same stream (`None`: it held no file).
    pub link_trees: BTreeMap<String, Option<String>>,
    /// The pax global comment of a git archive (its commit id).
    pub commit: Option<String>,
    /// Entries looked at, and bytes read from files.
    pub entries: usize,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

/// Decides, from a name relative to the theme folder, whether the installed theme can use the file (see
/// `assemble::keep`). Passed in so the reader does not write what will be thrown away.
pub type Keep<'k> = &'k dyn Fn(&str) -> bool;

/// Streams a file's content: called once with the sink that hashes and writes it.
type Fill<'f> = &'f mut dyn FnMut(&mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<(), String>;

struct Builder<'a> {
    st: Staged,
    lim: &'a Limits,
    keep: Keep<'a>,
    tree: TreeBuilder,
    link_trees: BTreeMap<String, TreeBuilder>,
    /// Folder entries read but not processed yet, over the whole recursion (bounded with `entries`).
    pending: usize,
}

/// `parent(p)` joined with a relative link target, lexically; `None` when it climbs out of the source.
pub fn join_link(p: &str, target: &str) -> Option<String> {
    if target.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = p.split('/').collect();
    parts.pop();
    for t in target.split('/') {
        match t {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            t => parts.push(t),
        }
    }
    Some(parts.join("/"))
}

impl<'a> Builder<'a> {
    fn new(dir: PathBuf, filter: Filter, lim: &'a Limits, keep: Keep<'a>) -> Builder<'a> {
        let link_trees = filter.link_sources.iter().chain(&filter.text_links).map(|d| (d.clone(), TreeBuilder::default())).collect();
        Builder {
            st: Staged { dir, filter, nodes: BTreeMap::new(), tree: None, link_trees: BTreeMap::new(), commit: None, entries: 0, bytes: 0, warnings: Vec::new() },
            lim,
            keep,
            tree: TreeBuilder::default(),
            link_trees,
            pending: 0,
        }
    }

    /// Refuses `p` when an ancestor is a file or link, or when `p` exists already.
    fn check_place(&self, p: &str, is_dir: bool) -> Result<(), String> {
        let mut acc = String::new();
        let parts: Vec<&str> = p.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            match self.st.nodes.get(&acc) {
                Some(Node::Link(_)) => return Err(format!("`{p}` lies below the symlink `{acc}`")),
                Some(Node::File { .. }) => return Err(format!("`{p}` lies below the file `{acc}`")),
                _ => {}
            }
        }
        match self.st.nodes.get(p) {
            Some(Node::Dir) if is_dir => Ok(()),
            Some(_) => Err(format!("`{p}` appears twice")),
            None if !is_dir && self.st.nodes.range(format!("{p}/")..).next().is_some_and(|(k, _)| k.starts_with(&format!("{p}/"))) => Err(format!("`{p}` is both a folder and a file")),
            None => Ok(()),
        }
    }

    /// Adds an entry to the tree of the `link_sources` folder it lies in (if any).
    fn link_tree_add(&mut self, p: &str, mode: Mode, oid: [u8; 20]) {
        for (d, t) in self.link_trees.iter_mut() {
            if let Some(rel) = under(p, d).filter(|r| !r.is_empty()) {
                t.add(rel, mode, oid);
            }
        }
    }

    fn mode_of(exec: bool) -> Mode {
        if exec {
            Mode::Exec
        } else {
            Mode::File
        }
    }

    fn dir(&mut self, p: &str) -> Result<(), String> {
        if self.st.filter.want(p) == Want::No {
            return Ok(());
        }
        self.check_place(p, true)?;
        self.st.nodes.insert(p.to_string(), Node::Dir);
        Ok(())
    }

    fn link(&mut self, p: &str, target: &str) -> Result<(), String> {
        let want = self.st.filter.want(p);
        if want == Want::No {
            return Ok(());
        }
        if target.is_empty() || target.len() > 1024 || target.chars().any(char::is_control) || target.contains('\\') {
            return Err(format!("symlink `{p}` has an invalid target"));
        }
        if target.starts_with('/') {
            return Err(format!("symlink `{p}` -> `{target}` is absolute (links must stay inside the pack)"));
        }
        if join_link(p, target).is_none() {
            return Err(format!("symlink `{p}` -> `{target}` points outside the source"));
        }
        self.check_place(p, false)?;
        if let Want::Theme(rel) = &want {
            self.tree.add(rel, Mode::Link, gitoid::blob_oid(target.as_bytes()));
        } else {
            self.link_tree_add(p, Mode::Link, gitoid::blob_oid(target.as_bytes()));
        }
        self.st.nodes.insert(p.to_string(), Node::Link(target.to_string()));
        Ok(())
    }

    fn hard(&mut self, p: &str, target: &str) -> Result<(), String> {
        let want = self.st.filter.want(p);
        if want == Want::No {
            return Ok(());
        }
        self.check_place(p, false)?;
        let Some(Node::File { exec, size, staged, oid, sha256 }) = self.st.nodes.get(target).cloned() else {
            self.st.warnings.push(format!("hard link `{p}` -> `{target}`: the target is not a file of the pack, left out"));
            return Ok(());
        };
        let rel = if let Want::Theme(rel) = &want { Some(rel.clone()) } else { None };
        let stage = match &rel {
            Some(r) => (self.keep)(r),
            None => true,
        };
        if stage {
            if !staged {
                self.st.warnings.push(format!("hard link `{p}` -> `{target}`: the target was not kept, left out"));
                return Ok(());
            }
            let dst = self.st.dir.join(p);
            std::fs::create_dir_all(dst.parent().expect("inside")).map_err(|e| e.to_string())?;
            let mut out = create_new(&dst)?;
            let mut inp = std::fs::File::open(self.st.dir.join(target)).map_err(|e| e.to_string())?;
            std::io::copy(&mut inp, &mut out).map_err(|e| e.to_string())?;
        }
        if let Some(r) = rel {
            self.tree.add(&r, Self::mode_of(exec), oid);
        } else {
            self.link_tree_add(p, Self::mode_of(exec), oid);
        }
        self.st.nodes.insert(p.to_string(), Node::File { exec, size, staged: stage, oid, sha256 });
        Ok(())
    }

    /// A regular file of `size` bytes; `fill` streams its content into the sink it is given.
    fn file(&mut self, p: &str, size: u64, exec: bool, fill: Fill) -> Result<(), String> {
        let want = self.st.filter.want(p);
        if want == Want::No {
            return Ok(());
        }
        self.check_place(p, false)?;
        let (rel, stage, cap) = match &want {
            // a licence or index file inside the theme folder is staged even when the theme does not install it
            Want::Theme(rel) => (Some(rel.clone()), (self.keep)(rel) || self.st.filter.files.iter().any(|f| f == p), self.lim.max_file),
            _ if self.st.filter.files.iter().any(|f| f == p) => (None, true, self.lim.max_text),
            _ => (None, true, self.lim.max_file),
        };
        if size > cap {
            return Err(format!("`{p}` is {} KiB, the limit is {} KiB", size >> 10, cap >> 10));
        }
        self.st.bytes += size;
        if self.st.bytes > self.lim.max_total {
            return Err(format!("the pack is larger than {} MiB", self.lim.max_total >> 20));
        }
        let mut out = if stage {
            let dst = self.st.dir.join(p);
            std::fs::create_dir_all(dst.parent().expect("inside")).map_err(|e| format!("{}: {e}", dst.display()))?;
            Some(create_new(&dst)?)
        } else {
            None
        };
        let mut blob = gitoid::blob_hasher(size);
        let mut sha = Sha256::default();
        let mut got = 0u64;
        fill(&mut |b: &[u8]| {
            got += b.len() as u64;
            if got > size {
                return Err(format!("`{p}` is larger than it said"));
            }
            blob.update(b);
            sha.update(b);
            if let Some(o) = out.as_mut() {
                o.write_all(b).map_err(|e| e.to_string())?;
            }
            Ok(())
        })?;
        if got != size {
            return Err(format!("`{p}` is shorter than it said (changed while it was read?)"));
        }
        let oid = blob.finish();
        if let Some(r) = rel {
            self.tree.add(&r, Self::mode_of(exec), oid);
        } else {
            self.link_tree_add(p, Self::mode_of(exec), oid);
        }
        self.st.nodes.insert(p.to_string(), Node::File { exec, size, staged: stage, oid, sha256: sha.finish() });
        Ok(())
    }

    fn done(mut self) -> Staged {
        self.st.tree = self.tree.oid();
        self.st.link_trees = self.link_trees.iter().map(|(d, t)| (d.clone(), t.oid())).collect();
        self.st
    }
}

fn create_new(p: &Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(p).map_err(|e| format!("{}: {e}", p.display()))
}

/// Strips `n` leading parts; `None` when nothing is left (the wrapping folder itself).
fn strip(p: &str, n: usize) -> Option<String> {
    let mut it = p.splitn(n + 1, '/');
    for _ in 0..n {
        it.next()?;
    }
    it.next().filter(|r| !r.is_empty()).map(String::from)
}

/// Reads a `.tar` / `.tar.gz` stream into `dir` (an empty private folder).
pub fn from_tar(r: impl Read, fmt: tar::Format, dir: PathBuf, filter: Filter, lim: &Limits, keep: Keep) -> Result<Staged, String> {
    let mut rd = tar::Reader::new(r, fmt, lim.max_stream, lim.max_entries);
    let n = filter.strip;
    let mut b = Builder::new(dir, filter, lim, keep);
    while let Some(h) = rd.next_entry()? {
        b.st.entries += 1;
        let Some(p) = strip(&h.path, n) else {
            if h.kind == Kind::Dir {
                continue;
            }
            return Err(format!("`{}` is not inside the archive's folder (strip_components = {n})", h.path));
        };
        let res = match &h.kind {
            Kind::Dir => b.dir(&p),
            Kind::Symlink(t) => b.link(&p, t),
            Kind::Hardlink(t) => match strip(t, n) {
                Some(t) => b.hard(&p, &t),
                None => Err(format!("hard link `{p}` points outside the archive's folder")),
            },
            Kind::File => {
                let exec = h.mode & 0o100 != 0;
                b.file(&p, h.size, exec, &mut |sink| rd.content(sink))
            }
        };
        res.map_err(|e| format!("archive entry rejected: {e}"))?;
    }
    b.st.commit = rd.global_comment.take();
    Ok(b.done())
}

/// Reads a folder (a clone or an unpacked archive) into `dir`. Links are read, never followed; `.git` is skipped.
pub fn from_dir(root: &Path, dir: PathBuf, filter: Filter, lim: &Limits, keep: Keep) -> Result<Staged, String> {
    // the folder the user named may itself be reached through a link; nothing below it is followed
    let root = &std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let meta = std::fs::symlink_metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    let mut b = Builder::new(dir, filter, lim, keep);
    walk(root, "", 0, &mut b)?;
    Ok(b.done())
}

fn walk(root: &Path, rel: &str, depth: usize, b: &mut Builder) -> Result<(), String> {
    if depth > super::MAX_DEPTH {
        return Err(format!("`{rel}` is nested deeper than {} folders", super::MAX_DEPTH));
    }
    let here = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let mut names: Vec<String> = Vec::new();
    for e in std::fs::read_dir(&here).map_err(|e| format!("{}: {e}", here.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        // counted while the folder is read: a huge folder is refused before its names pile up in memory
        if b.st.entries + b.pending + names.len() >= b.lim.max_entries {
            return Err(format!("more than {} entries", b.lim.max_entries));
        }
        let n = e.file_name().into_string().map_err(|n| format!("`{}` in {} is not UTF-8", n.to_string_lossy(), here.display()))?;
        names.push(n);
    }
    names.sort();
    b.pending += names.len();
    for n in names {
        b.pending -= 1;
        if n == ".git" {
            continue;
        }
        let p = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
        super::safe_rel(&p)?;
        b.st.entries += 1;
        if b.st.entries > b.lim.max_entries {
            return Err(format!("more than {} entries", b.lim.max_entries));
        }
        let full = root.join(&p);
        let m = std::fs::symlink_metadata(&full).map_err(|e| format!("{}: {e}", full.display()))?;
        let ft = m.file_type();
        if ft.is_symlink() {
            let t = std::fs::read_link(&full).map_err(|e| e.to_string())?;
            let t = t.to_str().ok_or_else(|| format!("symlink `{p}` has a target that is not UTF-8"))?.to_string();
            b.link(&p, &t)?;
        } else if ft.is_dir() {
            // a folder nobody wants is still walked only when something below it could be wanted
            if b.st.filter.want(&p) != Want::No || wanted_below(&b.st.filter, &p) {
                b.dir(&p)?;
                walk(root, &p, depth + 1, b)?;
            }
        } else if ft.is_file() {
            if b.st.filter.want(&p) == Want::No {
                continue;
            }
            use std::os::unix::fs::PermissionsExt;
            let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&full).map_err(|e| format!("{}: {e}", full.display()))?;
            let fm = f.metadata().map_err(|e| e.to_string())?;
            if !fm.is_file() {
                return Err(format!("`{p}` changed while it was read"));
            }
            let exec = fm.permissions().mode() & 0o100 != 0;
            let mut f = f.take(fm.len() + 1);
            b.file(&p, fm.len(), exec, &mut |sink| {
                let mut buf = vec![0u8; 64 << 10];
                loop {
                    let k = f.read(&mut buf).map_err(|e| e.to_string())?;
                    if k == 0 {
                        return Ok(());
                    }
                    sink(&buf[..k])?;
                }
            })?;
        } else {
            return Err(format!("`{p}` is a device, socket or FIFO"));
        }
    }
    Ok(())
}

/// Stages pinned git objects of a verified pack (`gitpack`): each of `roots` (source-relative path, pinned id) is a tree,
/// expanded with everything below it, or a single blob. Roots inside an earlier tree root are covered by it. Names,
/// links and caps go through the same checks as an archive; the tree ids are computed again from what is staged.
pub fn from_git(pack: &super::gitpack::Pack, trees: &[(String, String)], blobs: &[(String, String)], dir: PathBuf, filter: Filter, lim: &Limits, keep: Keep) -> Result<Staged, String> {
    use super::gitpack::{self, OBJ_BLOB};
    let mut b = Builder::new(dir, filter, lim, keep);
    let mut done: Vec<String> = Vec::new();
    let covered = |p: &str, done: &[String]| done.iter().any(|d| d == "." || d == p || under(p, d).is_some());
    for (p, oid) in trees {
        if covered(p, &done) {
            continue;
        }
        let o = gitpack::parse_oid(oid).ok_or_else(|| format!("`{p}`: the pinned tree id is not 40 hex digits"))?;
        git_tree(pack, &o, if p == "." { "" } else { p }, 0, &mut b)?;
        done.push(p.clone());
    }
    for (p, oid) in blobs {
        if covered(p, &done) {
            continue;
        }
        super::safe_rel(p)?;
        let o = gitpack::parse_oid(oid).ok_or_else(|| format!("`{p}`: the pinned blob id is not 40 hex digits"))?;
        let (k, data) = pack.get(&o)?.ok_or_else(|| format!("the server did not send `{p}` ({oid})"))?;
        if k != OBJ_BLOB {
            return Err(format!("`{p}` ({oid}) is not a blob"));
        }
        b.st.entries += 1;
        b.file(p, data.len() as u64, false, &mut |sink| sink(&data))?;
    }
    Ok(b.done())
}

fn git_tree(pack: &super::gitpack::Pack, oid: &[u8; 20], rel: &str, depth: usize, b: &mut Builder) -> Result<(), String> {
    use super::gitpack::{parse_tree, Mode, OBJ_BLOB, OBJ_TREE};
    if depth > super::MAX_DEPTH {
        return Err(format!("`{rel}` is nested deeper than {} folders", super::MAX_DEPTH));
    }
    let here = if rel.is_empty() { "." } else { rel };
    let (k, data) = pack.get(oid)?.ok_or_else(|| format!("the server did not send the tree of `{here}`"))?;
    if k != OBJ_TREE {
        return Err(format!("`{here}` is not a tree object"));
    }
    let entries = parse_tree(&data)?;
    if b.st.entries + b.pending + entries.len() > b.lim.max_entries {
        return Err(format!("more than {} entries", b.lim.max_entries));
    }
    b.pending += entries.len();
    for e in entries {
        b.pending -= 1;
        let p = if rel.is_empty() { e.name.clone() } else { format!("{rel}/{}", e.name) };
        super::safe_rel(&p)?;
        b.st.entries += 1;
        match e.mode {
            Mode::Dir => {
                b.dir(&p)?;
                git_tree(pack, &e.oid, &p, depth + 1, b)?;
            }
            Mode::Gitlink => return Err(format!("`{p}` is a submodule (another repository); it cannot be fetched with this pack")),
            Mode::File | Mode::Exec | Mode::Link => {
                let (k, data) = pack.get(&e.oid)?.ok_or_else(|| format!("the server did not send `{p}`"))?;
                if k != OBJ_BLOB {
                    return Err(format!("`{p}` is not a blob"));
                }
                if e.mode == Mode::Link {
                    let t = std::str::from_utf8(&data).map_err(|_| format!("symlink `{p}` has a target that is not UTF-8"))?;
                    b.link(&p, t)?;
                } else {
                    b.file(&p, data.len() as u64, e.mode == Mode::Exec, &mut |sink| sink(&data))?;
                }
            }
        }
    }
    Ok(())
}

fn wanted_below(f: &Filter, dir: &str) -> bool {
    let pre = format!("{dir}/");
    let starts = |x: &str| x.starts_with(&pre);
    f.path == "." || f.path == dir || starts(&f.path) || f.files.iter().any(|x| starts(x)) || f.link_sources.iter().chain(&f.text_links).any(|x| x == dir || starts(x))
}

impl Staged {
    /// Follows links component by component (every hop must stay inside the source).
    pub fn resolve(&self, start: &str) -> Resolved {
        let mut pending: std::collections::VecDeque<String> = start.split('/').map(String::from).collect();
        let mut cur: Vec<String> = Vec::new();
        let mut hops = 0;
        while let Some(c) = pending.pop_front() {
            match c.as_str() {
                "" | "." => {}
                ".." => {
                    if cur.pop().is_none() {
                        return Resolved::Escapes;
                    }
                }
                _ => {
                    cur.push(c);
                    let p = cur.join("/");
                    if let Some(Node::Link(t)) = self.nodes.get(&p) {
                        hops += 1;
                        if hops > super::MAX_HOPS {
                            return Resolved::Loop;
                        }
                        if t.starts_with('/') {
                            return Resolved::Escapes;
                        }
                        cur.pop();
                        for part in t.split('/').rev() {
                            pending.push_front(part.to_string());
                        }
                    }
                }
            }
        }
        let p = cur.join("/");
        match self.nodes.get(&p) {
            Some(Node::File { .. }) => Resolved::File(p),
            Some(Node::Dir) => Resolved::Dir(p),
            _ if p.is_empty() || self.nodes.range(format!("{p}/")..).next().is_some_and(|(k, _)| k.starts_with(&format!("{p}/"))) => Resolved::Dir(p),
            _ => Resolved::Missing(p),
        }
    }

    /// The staged copy of a source file.
    pub fn file_path(&self, p: &str) -> PathBuf {
        self.dir.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;

    fn keep_all(_: &str) -> bool {
        true
    }

    fn stage(entries: &[E], path: &str, files: &[&str]) -> Result<(Staged, Scratch), String> {
        let s = Scratch::new("src");
        let tgz = targz(entries, Some("5d11ba4e4013a4d1e1c601265d55c66b95184922"));
        let f = Filter { strip: 1, path: path.into(), files: files.iter().map(|x| x.to_string()).collect(), link_sources: vec![], ..Filter::default() };
        let st = from_tar(&tgz[..], tar::Format::TarGz, s.path().to_path_buf(), f, &Limits::default(), &keep_all)?;
        Ok((st, s))
    }

    #[test]
    fn only_the_pack_path_and_licences_are_staged_and_the_tree_id_matches() {
        let src = icon_source("bloom", vec![E::file("top/other/huge.svg", SVG)]);
        let (st, s) = stage(&src, "bloom", &["COPYING"]).unwrap();
        assert_eq!(st.tree.as_deref(), Some(tree_of(&src, "top/bloom").as_str()));
        assert_eq!(st.commit.as_deref(), Some("5d11ba4e4013a4d1e1c601265d55c66b95184922"));
        assert!(s.path().join("COPYING").is_file() && s.path().join("bloom/index.theme").is_file());
        assert!(!s.path().join("install.sh").exists() && !s.path().join("other").exists(), "outside the filter: never written");
        assert!(matches!(st.nodes.get("bloom/apps/48/terminal.svg"), Some(Node::Link(t)) if t == "utilities-terminal.svg"));
        assert!(std::fs::symlink_metadata(s.path().join("bloom/apps/48/terminal.svg")).is_err(), "links are recorded, not created");
        assert_eq!(st.resolve("bloom/cursors/arrow"), Resolved::File("bloom/cursors/left_ptr".into()), "two hops");
        assert_eq!(st.resolve("bloom/places/scalable/user-home.svg"), Resolved::File("bloom/places/scalable/folder.svg".into()));
        assert_eq!(st.resolve("bloom/places"), Resolved::Dir("bloom/places".into()));
    }

    #[test]
    fn a_folder_source_gives_the_same_tree_id_as_its_archive() {
        let src = icon_source("ocean", vec![]);
        let d = Scratch::new("clone");
        write_tree(&src, d.path());
        std::fs::create_dir_all(d.path().join(".git/objects")).unwrap();
        std::fs::write(d.path().join(".git/config"), "x").unwrap();
        let s = Scratch::new("src");
        let f = Filter { strip: 0, path: "ocean".into(), files: vec!["COPYING".into()], link_sources: vec![], ..Filter::default() };
        let st = from_dir(d.path(), s.path().to_path_buf(), f, &Limits::default(), &keep_all).unwrap();
        assert_eq!(st.tree.as_deref(), Some(tree_of(&src, "top/ocean").as_str()), "exec bits and links read from disk");
        assert!(!st.nodes.contains_key(".git"));
    }

    #[test]
    fn traversal_links_and_conflicts_are_refused() {
        let base = |extra: Vec<E>| icon_source("t", extra);
        for (what, extra) in [
            ("absolute link", vec![E::link("top/t/apps/48/x.svg", "/etc/passwd")]),
            ("escaping link", vec![E::link("top/t/apps/48/x.svg", "../../../../etc/passwd")]),
            ("write through a link", vec![E::link("top/t/evil", ".."), E::file("top/t/evil/x.svg", SVG)]),
            ("duplicate", vec![E::file("top/t/apps/48/utilities-terminal.svg", SVG)]),
            ("file then folder", vec![E::file("top/t/apps/48/utilities-terminal.svg/x.svg", SVG)]),
            ("link with a control char", vec![E::link("top/t/apps/48/y.svg", "a\u{7}b")]),
        ] {
            let r = stage(&base(extra), "t", &["COPYING"]);
            assert!(r.is_err(), "{what} accepted");
        }
        // a link inside the source but outside the pack is fine at this stage (assemble decides)
        let (st, _s) = stage(&base(vec![E::link("top/t/apps/48/c.svg", "../../../COPYING")]), "t", &["COPYING"]).unwrap();
        assert_eq!(st.resolve("t/apps/48/c.svg"), Resolved::File("COPYING".into()));
        let (st, _s) = stage(&base(vec![E::link("top/t/a", "b"), E::link("top/t/b", "a")]), "t", &[]).unwrap();
        assert_eq!(st.resolve("t/a"), Resolved::Loop);
    }

    #[test]
    fn hard_links_are_copies_and_caps_hold() {
        let src = icon_source("t", vec![E::hard("top/t/apps/48/copy.svg", "top/t/apps/48/utilities-terminal.svg")]);
        let (st, s) = stage(&src, "t", &[]).unwrap();
        assert_eq!(std::fs::read(s.path().join("t/apps/48/copy.svg")).unwrap(), SVG);
        assert_eq!(st.tree.as_deref(), Some(tree_of(&src, "top/t").as_str()));
        let s2 = Scratch::new("cap");
        let tgz = targz(&icon_source("t", vec![E::file("top/t/apps/48/big.png", &vec![1u8; 5000])]), None);
        let lim = Limits { max_file: 4000, ..Limits::default() };
        let f = Filter { strip: 1, path: "t".into(), ..Filter::default() };
        assert!(from_tar(&tgz[..], tar::Format::TarGz, s2.path().to_path_buf(), f.clone(), &lim, &keep_all).unwrap_err().contains("limit"));
        let s3 = Scratch::new("cap2");
        let lim = Limits { max_total: 300, ..Limits::default() };
        assert!(from_tar(&tgz[..], tar::Format::TarGz, s3.path().to_path_buf(), f, &lim, &keep_all).unwrap_err().contains("larger"));
    }

    #[test]
    fn folder_reads_stop_at_the_entry_limit_while_enumerating() {
        let d = Scratch::new("many");
        for i in 0..10 {
            d.write(&format!("t/f{i}.svg"), SVG);
        }
        let s = Scratch::new("many-src");
        let f = Filter { strip: 0, path: "t".into(), ..Filter::default() };
        let lim = Limits { max_entries: 5, ..Limits::default() };
        assert!(from_dir(d.path(), s.path().to_path_buf(), f.clone(), &lim, &keep_all).unwrap_err().contains("more than 5 entries"));
        // names waiting in an enclosing folder count too: a/ (3 files) and b/ (3 files) are 8 entries in all
        let d2 = Scratch::new("many2");
        for x in ["a", "b"] {
            for i in 0..3 {
                d2.write(&format!("t/{x}/f{i}.svg"), SVG);
            }
        }
        let s2 = Scratch::new("many2-src");
        let lim = Limits { max_entries: 7, ..Limits::default() };
        assert!(from_dir(d2.path(), s2.path().to_path_buf(), f.clone(), &lim, &keep_all).is_err());
        let s3 = Scratch::new("many3-src");
        let lim = Limits { max_entries: 9, ..Limits::default() };
        assert!(from_dir(d2.path(), s3.path().to_path_buf(), f, &lim, &keep_all).is_ok(), "t, a, b and six files fit in 9");
    }

    #[test]
    fn link_joins_stay_lexical() {
        assert_eq!(join_link("a/b/c", "../d").as_deref(), Some("a/d"));
        assert_eq!(join_link("a/b", "./x/../y").as_deref(), Some("a/y"));
        assert_eq!(join_link("a", "../../x"), None);
        assert_eq!(join_link("a", "/x"), None);
        assert_eq!(strip("top/a/b", 1).as_deref(), Some("a/b"));
        assert_eq!(strip("top", 1), None);
        assert_eq!(strip("a", 0).as_deref(), Some("a"));
    }
}
