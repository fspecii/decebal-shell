//! Builds the theme folder of a pack from its staged source and checks it is a usable icon / cursor theme.
//!
//! Kept: icon files whose content matches their extension (`.svg`, `.png`, `.xpm`), `index.theme` / `cursor.theme`,
//! valid Xcursor files in `cursors/`, and plain text (`README*`, `COPYING*`, `LICENSE*`, `AUTHORS*`, `*.md`, `*.txt`).
//! Everything else is left out and counted. Files are `0644`, folders `0755`. Links are rewritten to the direct
//! relative path of what they finally point at; links into `link_sources` become copies; links to nothing, to a
//! left-out file or to an enclosing folder are dropped with a warning.

use super::source::{Node, Resolved, Staged};
use super::{cursor, Limits, Provides};
use crate::theme::engine::sha256::{self, Sha256};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// The provenance file inside an installed pack (not part of its content hash).
pub const PROVENANCE: &str = ".decebal-asset.json";

#[derive(Debug, Clone)]
pub struct Spec {
    pub provides: Vec<Provides>,
    /// The theme folder in the source (`.` = root).
    pub path: String,
    pub install_name: String,
    pub display_name: String,
    /// Source-relative licence files (at least one).
    pub license_files: Vec<String>,
    /// Source files concatenated into `index.theme` when the theme folder has none, with their pinned blob ids.
    pub index_files: Vec<(String, Option<String>)>,
    /// Cursors to build from PNGs of the pack (see [`super::xcursor`]).
    pub xcursors: Vec<super::xcursor::Cursor>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Built {
    pub files: usize,
    pub links: usize,
    /// Links into `link_sources` written as copies.
    pub copies: usize,
    pub skipped: usize,
    /// The first few left-out names (for the report).
    pub skipped_sample: Vec<String>,
    pub dropped_links: usize,
    pub warnings: Vec<String>,
    /// `LICENSES/...` inside the theme.
    pub licenses: Vec<String>,
    pub icon_dirs: usize,
    pub cursors: usize,
    /// Cursors built from PNGs.
    pub cursors_built: usize,
    pub inherits: Vec<String>,
    pub content_hash: String,
}

fn hidden(rel: &str) -> bool {
    rel.split('/').any(|p| p.starts_with('.'))
}

fn is_text_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    ["readme", "copying", "license", "licence", "authors", "credits", "copyright"].iter().any(|p| l.starts_with(p)) || l.ends_with(".md") || l.ends_with(".txt")
}

fn is_image(rel: &str) -> bool {
    let l = rel.to_ascii_lowercase();
    [".svg", ".png", ".xpm"].iter().any(|e| l.ends_with(e))
}

/// Can the theme use a file of this name (relative to the theme folder)? Content is checked later.
pub fn keep(rel: &str) -> bool {
    if hidden(rel) {
        return false;
    }
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if rel == "index.theme" || rel == "cursor.theme" {
        return true;
    }
    if rel.starts_with("cursors/") && rel.matches('/').count() == 1 {
        return true;
    }
    let l = name.to_ascii_lowercase();
    if l == "icon-theme.cache" {
        return false;
    }
    if [".svg", ".png", ".xpm"].iter().any(|e| l.ends_with(e)) {
        return true;
    }
    is_text_name(name)
}

/// Does the content match what the name says? `Err` explains why not.
fn content_ok(rel: &str, data_head: &[u8], full: &Path, size: u64, lim: &Limits) -> Result<(), String> {
    let l = rel.to_ascii_lowercase();
    if rel.starts_with("cursors/") {
        return cursor::check_file(full);
    }
    if l.ends_with(".png") {
        return if data_head.starts_with(b"\x89PNG\r\n\x1a\n") { Ok(()) } else { Err("not a PNG".into()) };
    }
    if l.ends_with(".xpm") {
        return if data_head.starts_with(b"/* XPM */") { Ok(()) } else { Err("not an XPM".into()) };
    }
    if l.ends_with(".svg") {
        return if data_head.windows(4).any(|w| w == b"<svg") { Ok(()) } else { Err("not an SVG (no <svg> element)".into()) };
    }
    // index.theme, cursor.theme, text
    if size > lim.max_text {
        return Err(format!("text file larger than {} KiB", lim.max_text >> 10));
    }
    if data_head.contains(&0) {
        return Err("binary data in a text file".into());
    }
    Ok(())
}

fn head(p: &Path) -> std::io::Result<Vec<u8>> {
    let mut v = Vec::with_capacity(64 << 10);
    std::fs::File::open(p)?.take(64 << 10).read_to_end(&mut v)?;
    Ok(v)
}

/// The relative link text from the folder of `from` to `to` (both theme-relative).
pub fn relative(from: &str, to: &str) -> String {
    let mut a: Vec<&str> = from.split('/').collect();
    a.pop();
    let b: Vec<&str> = to.split('/').collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<&str> = vec![".."; a.len() - common];
    parts.extend(&b[common..]);
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

fn mkdirs(out: &Path, rel_parent: &str) -> Result<(), String> {
    if rel_parent.is_empty() {
        return Ok(());
    }
    let mut p = out.to_path_buf();
    for part in rel_parent.split('/') {
        p.push(part);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(format!("`{rel_parent}` would be created through a link or file")),
            Err(_) => {
                std::fs::create_dir(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

fn under<'a>(p: &'a str, dir: &str) -> Option<&'a str> {
    if dir == "." {
        return Some(p);
    }
    p.strip_prefix(dir).and_then(|r| r.strip_prefix('/'))
}

/// Builds `out` (an empty folder) from the staged source. Staged theme files are moved, not copied.
pub fn build(st: &Staged, spec: &Spec, out: &Path, lim: &Limits) -> Result<Built, String> {
    let mut b = Built::default();
    let mut kept: std::collections::BTreeSet<String> = Default::default();
    let skip = |b: &mut Built, rel: &str, why: Option<String>| {
        b.skipped += 1;
        if b.skipped_sample.len() < 8 {
            b.skipped_sample.push(match why {
                Some(w) => format!("{rel} ({w})"),
                None => rel.to_string(),
            });
        }
    };
    // cursors built from PNGs first: their sources are read from the staging folder and not installed themselves
    let mut consumed: std::collections::BTreeSet<String> = Default::default();
    if !spec.xcursors.is_empty() {
        for c in &spec.xcursors {
            for n in std::iter::once(&c.name).chain(&c.aliases) {
                let shipped = if spec.path == "." { format!("cursors/{n}") } else { format!("{}/cursors/{n}", spec.path) };
                if st.nodes.contains_key(&shipped) {
                    return Err(format!("cursor `{n}` is both shipped in cursors/ and built from PNGs"));
                }
            }
        }
        let full = |rel: &str| if spec.path == "." { rel.to_string() } else { format!("{}/{rel}", spec.path) };
        let read = |rel: &str| -> Result<Vec<u8>, String> {
            match st.nodes.get(&full(rel)) {
                Some(Node::File { staged: true, size, .. }) if *size <= lim.max_file => std::fs::read(st.file_path(&full(rel))).map_err(|e| e.to_string()),
                Some(Node::Link(_)) => match st.resolve(&full(rel)) {
                    Resolved::File(t) => std::fs::read(st.file_path(&t)).map_err(|e| e.to_string()),
                    _ => Err(format!("cursor image `{rel}` is a link to nothing")),
                },
                _ => Err(format!("cursor image `{rel}` is not in the pack")),
            }
        };
        for u in super::xcursor::build(&spec.xcursors, out, &read)? {
            consumed.insert(u);
        }
        b.cursors_built = spec.xcursors.len();
    }
    let mut links: Vec<(String, String)> = Vec::new();
    for (p, node) in &st.nodes {
        let Some(rel) = under(p, &spec.path) else { continue };
        if consumed.contains(rel) {
            skip(&mut b, rel, Some("built into cursors/".into()));
            continue;
        }
        if !spec.provides.contains(&Provides::Icons) && is_image(rel) && !rel.starts_with("cursors/") && !rel.starts_with("cursors_scalable/") {
            skip(&mut b, rel, Some("an image a cursor theme does not use".into()));
            continue;
        }
        match node {
            Node::Dir => {}
            Node::Link(_) => links.push((p.clone(), rel.to_string())),
            Node::File { staged: false, .. } => skip(&mut b, rel, None),
            Node::File { .. } if !keep(rel) => skip(&mut b, rel, Some("only read".into())),
            Node::File { size, .. } => {
                let src = st.file_path(p);
                let h = head(&src).map_err(|e| e.to_string())?;
                if let Err(why) = content_ok(rel, &h, &src, *size, lim) {
                    skip(&mut b, rel, Some(why));
                    continue;
                }
                mkdirs(out, parent(rel))?;
                let dst = out.join(rel);
                std::fs::rename(&src, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
                std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
                kept.insert(rel.to_string());
                b.files += 1;
            }
        }
    }
    // file links first, folder links last (nothing is ever created below a link)
    let mut dir_links: Vec<(String, String)> = Vec::new();
    for (p, rel) in links {
        match st.resolve(&p) {
            Resolved::File(t) => {
                if !keep(&rel) {
                    skip(&mut b, &rel, Some("link".into()));
                    continue;
                }
                match under(&t, &spec.path) {
                    Some(trel) if kept.contains(trel) => {
                        mkdirs(out, parent(&rel))?;
                        std::os::unix::fs::symlink(relative(&rel, trel), out.join(&rel)).map_err(|e| e.to_string())?;
                        b.links += 1;
                    }
                    Some(trel) => {
                        b.dropped_links += 1;
                        b.warnings.push(format!("link `{rel}` points at the left-out `{trel}`, dropped"));
                    }
                    None => match st.nodes.get(&t) {
                        Some(Node::File { staged: true, size, .. }) => {
                            let src = st.file_path(&t);
                            let h = head(&src).map_err(|e| e.to_string())?;
                            if let Err(why) = content_ok(&rel, &h, &src, *size, lim) {
                                skip(&mut b, &rel, Some(why));
                                continue;
                            }
                            mkdirs(out, parent(&rel))?;
                            std::fs::copy(&src, out.join(&rel)).map_err(|e| e.to_string())?;
                            std::fs::set_permissions(out.join(&rel), std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
                            b.copies += 1;
                            b.files += 1;
                        }
                        _ => {
                            b.dropped_links += 1;
                            b.warnings.push(format!("link `{rel}` points outside the theme at `{t}` (add its folder to link_sources), dropped"));
                        }
                    },
                }
            }
            Resolved::Dir(t) => dir_links.push((rel, t)),
            Resolved::Missing(t) => {
                b.dropped_links += 1;
                if b.dropped_links <= 20 {
                    b.warnings.push(format!("link `{rel}` points at nothing (`{t}`), dropped"));
                }
            }
            Resolved::Loop => {
                b.dropped_links += 1;
                b.warnings.push(format!("link `{rel}` is a loop, dropped"));
            }
            Resolved::Escapes => return Err(format!("link `{rel}` leaves the source")),
        }
    }
    for (rel, t) in dir_links {
        if hidden(&rel) {
            skip(&mut b, &rel, Some("hidden".into()));
            continue;
        }
        match under(&t, &spec.path) {
            Some(trel) if !(trel.is_empty() || rel == trel || rel.starts_with(&format!("{trel}/"))) && out.join(trel).is_dir() => {
                mkdirs(out, parent(&rel))?;
                std::os::unix::fs::symlink(relative(&rel, trel), out.join(&rel)).map_err(|e| e.to_string())?;
                b.links += 1;
            }
            _ => {
                b.dropped_links += 1;
                b.warnings.push(format!("folder link `{rel}` -> `{t}` is outside the theme, empty or encloses itself, dropped"));
            }
        }
    }
    if b.dropped_links > 20 {
        b.warnings.push(format!("{} links dropped in all", b.dropped_links));
    }
    index_from_files(st, spec, out, lim, &mut b)?;
    validate(out, spec, &mut b)?;
    licences(st, spec, out, &kept, lim, &mut b)?;
    let (hash, _, _) = content_hash(out)?;
    b.content_hash = hash;
    Ok(b)
}

/// Writes `index.theme` from the declared source files (each checked against its git blob id).
fn index_from_files(st: &Staged, spec: &Spec, out: &Path, lim: &Limits, b: &mut Built) -> Result<(), String> {
    if spec.index_files.is_empty() {
        return Ok(());
    }
    let dst = out.join("index.theme");
    if std::fs::symlink_metadata(&dst).is_ok() {
        b.warnings.push("the theme folder has its own index.theme; `index` is not used".into());
        return Ok(());
    }
    let mut text = Vec::new();
    for (p, pin) in &spec.index_files {
        let Some(Node::File { staged: true, oid, size, .. }) = st.nodes.get(p) else { return Err(format!("index file `{p}` is missing from the source")) };
        if let Some(want) = pin {
            let got = super::gitoid::hex(oid);
            if *want != got {
                return Err(format!("index file `{p}`: pinned blob {want}, read {got}"));
            }
        }
        if *size > lim.max_text {
            return Err(format!("index file `{p}` is larger than {} KiB", lim.max_text >> 10));
        }
        let d = std::fs::read(st.file_path(p)).map_err(|e| format!("index file `{p}`: {e}"))?;
        if std::str::from_utf8(&d).is_err() {
            return Err(format!("index file `{p}` is not UTF-8 text"));
        }
        if !text.is_empty() && !text.ends_with(b"\n") {
            text.push(b'\n');
        }
        text.extend_from_slice(&d);
    }
    std::fs::write(&dst, &text).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    b.files += 1;
    Ok(())
}

fn validate(out: &Path, spec: &Spec, b: &mut Built) -> Result<(), String> {
    let index = out.join("index.theme");
    let text = std::fs::read_to_string(&index).ok();
    if let Some(t) = &text {
        if !t.lines().any(|l| l.trim().eq_ignore_ascii_case("[Icon Theme]")) {
            return Err("index.theme has no [Icon Theme] section".into());
        }
        let ix = crate::icons::index::parse(t);
        for i in &ix.inherits {
            if !super::safe_theme_name(i) {
                return Err(format!("index.theme: Inherits names `{i}`, not a theme name"));
            }
        }
        b.inherits = ix.inherits.iter().filter(|i| **i != spec.install_name).cloned().collect();
        if spec.provides.contains(&Provides::Icons) {
            if ix.dirs.is_empty() {
                return Err("index.theme lists no icon Directories (a cursor-only theme? use provides = [\"cursors\"])".into());
            }
            b.icon_dirs = ix.dirs.iter().filter(|d| super::safe_rel(&d.path).is_ok() && out.join(&d.path).is_dir()).count();
            if b.icon_dirs == 0 {
                return Err("none of the Directories of index.theme exists in the pack".into());
            }
            let missing = ix.dirs.len() - b.icon_dirs;
            if missing > 0 {
                b.warnings.push(format!("{missing} of {} icon directories listed in index.theme are missing", ix.dirs.len()));
            }
        }
    } else if spec.provides.contains(&Provides::Icons) {
        return Err("no index.theme in the theme folder".into());
    }
    if spec.provides.contains(&Provides::Cursors) {
        let c = out.join("cursors");
        if !cursor::CORE.iter().any(|n| c.join(n).is_file()) {
            return Err("cursors/ has no core pointer (left_ptr or default)".into());
        }
        b.cursors = std::fs::read_dir(&c).map(|r| r.count()).unwrap_or(0);
        if text.is_none() {
            let name: String = spec.display_name.chars().filter(|c| !c.is_control()).collect();
            std::fs::write(&index, format!("[Icon Theme]\nName={name}\n")).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&index, std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
            b.warnings.push("no index.theme: a two-line one was written".into());
        }
    }
    Ok(())
}

fn licences(st: &Staged, spec: &Spec, out: &Path, kept: &std::collections::BTreeSet<String>, lim: &Limits, b: &mut Built) -> Result<(), String> {
    if spec.license_files.is_empty() {
        return Err("no licence file given: an asset pack is only installed with its licence".into());
    }
    let dir = out.join("LICENSES");
    for lf in &spec.license_files {
        let from = match under(lf, &spec.path) {
            Some(rel) if kept.contains(rel) => out.join(rel),
            _ => match st.nodes.get(lf) {
                Some(Node::File { staged: true, .. }) => st.file_path(lf),
                _ => return Err(format!("licence file `{lf}` is missing from the source")),
            },
        };
        let data = std::fs::read(&from).map_err(|e| format!("licence file `{lf}`: {e}"))?;
        if data.len() as u64 > lim.max_text || data.contains(&0) {
            return Err(format!("licence file `{lf}` is not a text file of at most {} KiB", lim.max_text >> 10));
        }
        if !dir.exists() {
            mkdirs(out, "LICENSES")?;
        }
        let base = lf.rsplit('/').next().unwrap_or(lf);
        let name = if b.licenses.iter().any(|l| l == &format!("LICENSES/{base}")) { lf.replace('/', "_") } else { base.to_string() };
        let dst = dir.join(&name);
        std::fs::write(&dst, &data).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
        b.licenses.push(format!("LICENSES/{name}"));
    }
    Ok(())
}

/// `sha256:` over the sorted entries of an installed theme folder (files and links, folders implied, the provenance
/// file left out), with the number of files and links. Links are hashed as their text, never followed.
pub fn content_hash(dir: &Path) -> Result<(String, usize, usize), String> {
    fn walk(base: &Path, rel: &str, out: &mut Vec<(String, Vec<u8>)>, n: &mut (usize, usize)) -> Result<(), String> {
        let here = if rel.is_empty() { base.to_path_buf() } else { base.join(rel) };
        let mut names: Vec<String> = std::fs::read_dir(&here).map_err(|e| format!("{}: {e}", here.display()))?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names {
            let p = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if p == PROVENANCE {
                continue;
            }
            let full = base.join(&p);
            let m = std::fs::symlink_metadata(&full).map_err(|e| e.to_string())?;
            if m.file_type().is_symlink() {
                let t = std::fs::read_link(&full).map_err(|e| e.to_string())?;
                out.push((p.clone(), format!("{p}\0L\0{}\n", t.to_string_lossy()).into_bytes()));
                n.1 += 1;
            } else if m.is_dir() {
                walk(base, &p, out, n)?;
            } else if m.is_file() {
                let mut h = Sha256::default();
                let mut f = std::fs::File::open(&full).map_err(|e| e.to_string())?;
                let mut buf = vec![0u8; 64 << 10];
                loop {
                    let k = f.read(&mut buf).map_err(|e| e.to_string())?;
                    if k == 0 {
                        break;
                    }
                    h.update(&buf[..k]);
                }
                out.push((p.clone(), format!("{p}\0F\0{}\0{}\n", m.len(), sha256::hex(&h.finish())).into_bytes()));
                n.0 += 1;
            } else {
                return Err(format!("`{p}` is not a file, folder or link"));
            }
        }
        Ok(())
    }
    let mut lines = Vec::new();
    let mut n = (0, 0);
    walk(dir, "", &mut lines, &mut n)?;
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::default();
    for (_, l) in &lines {
        h.update(l);
    }
    Ok((format!("sha256:{}", sha256::hex(&h.finish())), n.0, n.1))
}

#[cfg(test)]
mod tests {
    use super::super::source::{from_tar, Filter};
    use super::super::tar::Format;
    use super::super::testkit::*;
    use super::*;

    fn spec(path: &str, provides: &[Provides], lic: &[&str]) -> Spec {
        Spec {
            provides: provides.to_vec(),
            path: path.into(),
            install_name: path.rsplit('/').next().unwrap().into(),
            display_name: "Fixture".into(),
            license_files: lic.iter().map(|s| s.to_string()).collect(),
            index_files: vec![],
            xcursors: vec![],
        }
    }

    fn run(entries: &[E], sp: &Spec, link_sources: &[&str]) -> (Result<Built, String>, Scratch) {
        let s = Scratch::new("asm");
        std::fs::create_dir_all(s.path().join("src")).unwrap();
        std::fs::create_dir_all(s.path().join("out")).unwrap();
        let tgz = targz(entries, None);
        let files = sp.license_files.iter().chain(sp.index_files.iter().map(|(p, _)| p)).cloned().collect();
        let f = Filter { strip: 1, path: sp.path.clone(), files, link_sources: link_sources.iter().map(|x| x.to_string()).collect() };
        let st = match from_tar(&tgz[..], Format::TarGz, s.path().join("src"), f, &Limits::default(), &keep) {
            Ok(st) => st,
            Err(e) => return (Err(e), s),
        };
        (build(&st, sp, &s.path().join("out"), &Limits::default()), s)
    }

    #[test]
    fn a_theme_is_built_with_safe_files_links_and_its_licence() {
        let (r, s) = run(&icon_source("bloom", vec![]), &spec("bloom", &[Provides::Icons, Provides::Cursors], &["COPYING"]), &[]);
        let b = r.unwrap();
        let out = s.path().join("out");
        assert!(out.join("index.theme").is_file() && out.join("LICENSES/COPYING").is_file());
        assert_eq!(std::fs::read_link(out.join("apps/48/terminal.svg")).unwrap().to_str(), Some("utilities-terminal.svg"));
        assert_eq!(std::fs::read_link(out.join("places/scalable/user-home.svg")).unwrap().to_str(), Some("folder.svg"), "rewritten to the direct target");
        assert_eq!(std::fs::read_link(out.join("cursors/arrow")).unwrap().to_str(), Some("left_ptr"), "chains collapse");
        assert!(!out.join("places/scalable/render.sh").exists() && !out.join("places/scalable/icon-theme.cache").exists());
        let mode = std::fs::metadata(out.join("apps/48/org.example.App.svg")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "executable bits never survive");
        assert_eq!((b.icon_dirs, b.skipped), (2, 2), "{b:?}");
        assert_eq!(b.licenses, ["LICENSES/COPYING"]);
        assert_eq!(b.inherits, ["hicolor"]);
        assert_eq!(content_hash(&out).unwrap().0, b.content_hash);
        std::fs::write(out.join(PROVENANCE), "{}").unwrap();
        assert_eq!(content_hash(&out).unwrap().0, b.content_hash, "the provenance file is not content");
        std::fs::write(out.join("apps/48/utilities-terminal.svg"), "<svg>changed</svg>").unwrap();
        assert_ne!(content_hash(&out).unwrap().0, b.content_hash);
    }

    #[test]
    fn missing_licence_or_structure_refuses_the_pack() {
        let src = icon_source("bloom", vec![]);
        let (r, _s) = run(&src, &spec("bloom", &[Provides::Icons], &[]), &[]);
        assert!(r.unwrap_err().contains("licence"));
        let (r, _s) = run(&src, &spec("bloom", &[Provides::Icons], &["LICENSE"]), &[]);
        assert!(r.unwrap_err().contains("`LICENSE` is missing"));
        let no_index: Vec<E> = src.iter().filter(|e| !e.name.ends_with("index.theme")).cloned().collect();
        let (r, _s) = run(&no_index, &spec("bloom", &[Provides::Icons], &["COPYING"]), &[]);
        assert!(r.unwrap_err().contains("no index.theme"));
        let no_ptr: Vec<E> = src.iter().filter(|e| !e.name.contains("cursors/")).cloned().collect();
        let (r, _s) = run(&no_ptr, &spec("bloom", &[Provides::Cursors], &["COPYING"]), &[]);
        assert!(r.unwrap_err().contains("core pointer"));
        let bad_inherit = vec![E::dir("top/"), E::file("top/COPYING", b"x"), E::file("top/c/index.theme", b"[Icon Theme]\nInherits=../../etc\n"), E::file("top/c/cursors/left_ptr", &xcursor(24))];
        let (r, _s) = run(&bad_inherit, &spec("c", &[Provides::Cursors], &["COPYING"]), &[]);
        assert!(r.unwrap_err().contains("not a theme name"));
    }

    #[test]
    fn cursor_only_packs_get_an_index_and_bad_files_are_left_out() {
        let src = vec![
            E::dir("top/"),
            E::file("top/LICENSE", b"CC-BY-SA-3.0\n"),
            E::file("top/DMZ/cursor.theme", b"[Icon Theme]\nInherits=DMZ\n"),
            E::file("top/DMZ/cursors/left_ptr", &xcursor(32)),
            E::file("top/DMZ/cursors/xterm", b"#!/bin/sh\necho not a cursor\n"),
            E::file("top/DMZ/cursors/fake.png", b"not a png"),
            E::link("top/DMZ/cursors/dangling", "nothing"),
        ];
        let (r, s) = run(&src, &spec("DMZ", &[Provides::Cursors], &["LICENSE"]), &[]);
        let b = r.unwrap();
        let out = s.path().join("out");
        assert!(std::fs::read_to_string(out.join("index.theme")).unwrap().starts_with("[Icon Theme]\nName=Fixture"));
        assert!(!out.join("cursors/xterm").exists() && !out.join("cursors/fake.png").exists() && !out.join("cursors/dangling").exists());
        assert_eq!((b.cursors, b.skipped, b.dropped_links), (1, 2, 1));
    }

    #[test]
    fn links_into_link_sources_become_copies_others_are_dropped() {
        let extra = vec![
            E::file("top/ocean/apps/48/shared.svg", SVG),
            E::link("top/dark/apps/48/shared.svg", "../../../ocean/apps/48/shared.svg"),
            E::link("top/dark/apps/48/outside.svg", "../../../COPYING"),
            E::link("top/dark/apps/24", "48"),
            E::link("top/dark/apps/self", "."),
        ];
        let mut src = icon_source("dark", extra);
        src.retain(|e| !e.name.contains("cursors/"));
        let (r, s) = run(&src, &spec("dark", &[Provides::Icons], &["COPYING"]), &["ocean"]);
        let b = r.unwrap();
        let out = s.path().join("out");
        let shared = std::fs::symlink_metadata(out.join("apps/48/shared.svg")).unwrap();
        assert!(shared.is_file(), "a copy, not a link out of the theme");
        assert!(!out.join("apps/48/outside.svg").exists(), "COPYING is not an svg: left out");
        assert_eq!(std::fs::read_link(out.join("apps/24")).unwrap().to_str(), Some("48"), "folder links inside the theme stay");
        assert!(std::fs::symlink_metadata(out.join("apps/self")).is_err(), "a link to its own folder is dropped");
        assert_eq!(b.copies, 1);
        let (r, s) = run(&src, &spec("dark", &[Provides::Icons], &["COPYING"]), &[]);
        r.unwrap();
        assert!(!s.path().join("out/apps/48/shared.svg").exists(), "without link_sources: dropped");
    }

    #[test]
    fn index_files_outside_or_inside_the_theme_are_pinned_and_concatenated() {
        use super::super::gitoid::{blob_oid, hex};
        let head = b"[Icon Theme]\nName=Breeze\nInherits=hicolor\n";
        let common = b"Directories=apps/48\n\n[apps/48]\nSize=48\nType=Fixed\n";
        let src = vec![
            E::dir("top/"),
            E::file("top/COPYING", b"LGPL\n"),
            E::file("top/index.theme", head),
            E::file("top/icons/breeze.theme.in", head),
            E::file("top/icons/commonthemeinfo.theme.in", common),
            E::file("top/icons/apps/48/a.svg", SVG),
        ];
        // Adwaita: one file in the repository root; it lists no directories, so add the common part too
        let mut sp = spec("icons", &[Provides::Icons], &["COPYING"]);
        sp.index_files = vec![("index.theme".into(), Some(hex(&blob_oid(head)))), ("icons/commonthemeinfo.theme.in".into(), Some(hex(&blob_oid(common))))];
        let (r, s) = run(&src, &sp, &[]);
        let b = r.unwrap();
        let ix = std::fs::read_to_string(s.path().join("out/index.theme")).unwrap();
        assert!(ix.starts_with("[Icon Theme]\nName=Breeze") && ix.contains("[apps/48]"), "{ix}");
        assert_eq!(b.icon_dirs, 1);
        assert!(!s.path().join("out/breeze.theme.in").exists() && !s.path().join("out/commonthemeinfo.theme.in").exists(), "templates are read, not installed");
        // Breeze: two files inside the theme folder
        sp.index_files = vec![("icons/breeze.theme.in".into(), None), ("icons/commonthemeinfo.theme.in".into(), None)];
        let (r, _s) = run(&src, &sp, &[]);
        r.unwrap();
        // a pin that does not match refuses the pack
        sp.index_files = vec![("index.theme".into(), Some("0".repeat(40)))];
        let (r, _s) = run(&src, &sp, &[]);
        assert!(r.unwrap_err().contains("pinned blob"));
        sp.index_files = vec![("missing.theme".into(), None)];
        let (r, _s) = run(&src, &sp, &[]);
        assert!(r.unwrap_err().contains("missing"));
    }

    #[test]
    fn relative_links_are_computed_between_folders() {
        assert_eq!(relative("apps/48/a.svg", "apps/48/b.svg"), "b.svg");
        assert_eq!(relative("apps/24/a.svg", "apps/48/b.svg"), "../48/b.svg");
        assert_eq!(relative("a.svg", "x/y/b.svg"), "x/y/b.svg");
        assert_eq!(relative("x/y/a.svg", "b.svg"), "../../b.svg");
        assert_eq!(relative("apps/24", "apps/48"), "48");
    }
}
