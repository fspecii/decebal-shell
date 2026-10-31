//! Fixture archives and trees for the asset pack tests: a tar writer that can produce every entry type (also the
//! hostile ones), small valid icon / cursor themes, and a scratch folder.

use super::gitoid::{blob_oid, Mode, TreeBuilder};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use std::path::{Path, PathBuf};

/// One tar entry.
#[derive(Clone)]
pub struct E {
    pub name: String,
    pub flag: u8,
    pub data: Vec<u8>,
    pub link: String,
    pub mode: u32,
}

impl E {
    pub fn file(name: &str, data: &[u8]) -> E {
        E { name: name.into(), flag: b'0', data: data.to_vec(), link: String::new(), mode: 0o644 }
    }
    pub fn exec(name: &str, data: &[u8]) -> E {
        E { mode: 0o755, ..E::file(name, data) }
    }
    pub fn dir(name: &str) -> E {
        E { name: name.into(), flag: b'5', data: vec![], link: String::new(), mode: 0o755 }
    }
    pub fn link(name: &str, target: &str) -> E {
        E { name: name.into(), flag: b'2', data: vec![], link: target.into(), mode: 0o777 }
    }
    pub fn hard(name: &str, target: &str) -> E {
        E { name: name.into(), flag: b'1', data: vec![], link: target.into(), mode: 0o644 }
    }
    pub fn raw(name: &str, flag: u8) -> E {
        E { name: name.into(), flag, data: vec![], link: String::new(), mode: 0o644 }
    }
}

fn octal(buf: &mut [u8], v: u64) {
    let s = format!("{:0width$o}", v, width = buf.len() - 1);
    buf[..s.len()].copy_from_slice(s.as_bytes());
}

fn header(name: &str, flag: u8, size: u64, link: &str, mode: u32) -> [u8; 512] {
    let mut h = [0u8; 512];
    let n = name.as_bytes();
    h[..n.len().min(100)].copy_from_slice(&n[..n.len().min(100)]);
    octal(&mut h[100..108], u64::from(mode));
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], 0);
    h[156] = flag;
    let l = link.as_bytes();
    h[157..157 + l.len().min(100)].copy_from_slice(&l[..l.len().min(100)]);
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].fill(b' ');
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    h
}

fn pax(records: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in records {
        let body = format!(" {k}={v}\n");
        let mut len = body.len() + 1;
        while format!("{len}{body}").len() != len {
            len += 1;
        }
        out.extend_from_slice(format!("{len}{body}").as_bytes());
    }
    out
}

#[derive(Default)]
pub struct TarBuilder {
    out: Vec<u8>,
}

impl TarBuilder {
    pub fn new() -> TarBuilder {
        TarBuilder::default()
    }

    fn block(&mut self, h: [u8; 512], data: &[u8]) {
        self.out.extend_from_slice(&h);
        self.out.extend_from_slice(data);
        self.out.resize(self.out.len().div_ceil(512) * 512, 0);
    }

    /// An entry; names over 100 bytes get a pax `path` record (as `git archive` writes them).
    pub fn push(mut self, e: E) -> TarBuilder {
        let mut recs: Vec<(&str, &str)> = Vec::new();
        if e.name.len() > 100 {
            recs.push(("path", &e.name));
        }
        if e.link.len() > 100 {
            recs.push(("linkpath", &e.link));
        }
        if !recs.is_empty() {
            let d = pax(&recs);
            self.block(header("pax", b'x', d.len() as u64, "", 0o644), &d);
        }
        self.block(header(&e.name, e.flag, e.data.len() as u64, &e.link, e.mode), &e.data);
        self
    }

    /// An entry whose pax `path` record says something else than its header.
    pub fn pax_path(mut self, e: E, path: &str) -> TarBuilder {
        let d = pax(&[("path", path)]);
        self.block(header("pax", b'x', d.len() as u64, "", 0o644), &d);
        self.block(header(&e.name, e.flag, e.data.len() as u64, &e.link, e.mode), &e.data);
        self
    }

    /// An entry with a GNU `L` long name record.
    pub fn gnu_long(mut self, e: E) -> TarBuilder {
        let mut n = e.name.as_bytes().to_vec();
        n.push(0);
        self.block(header("././@LongLink", b'L', n.len() as u64, "", 0o644), &n);
        self.block(header(&e.name, e.flag, e.data.len() as u64, &e.link, e.mode), &e.data);
        self
    }

    /// The pax global header `git archive` starts with.
    pub fn global_comment(mut self, commit: &str) -> TarBuilder {
        let d = pax(&[("comment", commit)]);
        self.block(header("pax_global_header", b'g', d.len() as u64, "", 0o644), &d);
        self
    }

    pub fn tar(mut self) -> Vec<u8> {
        self.out.extend_from_slice(&[0u8; 1024]);
        self.out
    }

    pub fn gz(self) -> Vec<u8> {
        let mut g = GzEncoder::new(Vec::new(), Compression::fast());
        g.write_all(&self.tar()).unwrap();
        g.finish().unwrap()
    }
}

/// A scratch folder removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let base = std::env::var_os("TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let d = base.join(format!("decebal-assets-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"><rect width=\"16\" height=\"16\" fill=\"#3a7\"/></svg>";

pub const ICON_INDEX: &str = "[Icon Theme]\nName=Fixture Icons\nInherits=hicolor\nDirectories=apps/48,places/scalable\n\n[apps/48]\nSize=48\nContext=Applications\nType=Fixed\n\n[places/scalable]\nSize=48\nMinSize=8\nMaxSize=512\nContext=Places\nType=Scalable\n";

/// A tiny but real Xcursor file: one 2x2 ARGB image of nominal size `size`, hotspot (1, 1).
pub fn xcursor(size: u32) -> Vec<u8> {
    let mut v = Vec::new();
    let u = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    v.extend_from_slice(b"Xcur");
    u(&mut v, 16); // header size
    u(&mut v, 0x1_0000); // version
    u(&mut v, 1); // toc entries
    u(&mut v, 0xfffd_0002); // image chunk type
    u(&mut v, size); // subtype = nominal size
    u(&mut v, 28); // position of the chunk
                   // chunk header
    u(&mut v, 36);
    u(&mut v, 0xfffd_0002);
    u(&mut v, size);
    u(&mut v, 1); // version
    u(&mut v, 2); // width
    u(&mut v, 2); // height
    u(&mut v, 1); // xhot
    u(&mut v, 1); // yhot
    u(&mut v, 50); // delay
    for px in [0xff00_0000u32, 0xffff_ffff, 0xffff_ffff, 0xff00_0000] {
        u(&mut v, px);
    }
    v
}

/// Entries of a source tree `top/` (git-archive style) holding an icon theme at `top/<theme>`, its licence at
/// `top/COPYING`, internal alias links, a build script that must never be installed or run, and `extra`.
pub fn icon_source(theme: &str, extra: Vec<E>) -> Vec<E> {
    let t = |p: &str| format!("top/{theme}/{p}");
    let mut v = vec![
        E::dir("top/"),
        E::file("top/COPYING", b"GNU GENERAL PUBLIC LICENSE\nVersion 3\n"),
        E::exec("top/install.sh", b"#!/bin/sh\nrm -rf \"$HOME\"\n"),
        E::file("top/CMakeLists.txt", b"install(DIRECTORY ...)\n"),
        E::file(&t("index.theme"), ICON_INDEX.as_bytes()),
        E::exec(&t("apps/48/org.example.App.svg"), SVG),
        E::file(&t("apps/48/utilities-terminal.svg"), SVG),
        E::link(&t("apps/48/terminal.svg"), "utilities-terminal.svg"),
        E::file(&t("places/scalable/folder.svg"), SVG),
        E::link(&t("places/scalable/user-home.svg"), "../scalable/folder.svg"),
        E::file(&t("places/scalable/icon-theme.cache"), b"stale binary cache"),
        E::exec(&t("places/scalable/render.sh"), b"#!/bin/sh\ninkscape ...\n"),
        E::file(&t("cursors/left_ptr"), &xcursor(24)),
        E::link(&t("cursors/default"), "left_ptr"),
        E::link(&t("cursors/arrow"), "default"),
    ];
    v.extend(extra);
    v
}

pub fn targz(entries: &[E], commit: Option<&str>) -> Vec<u8> {
    let mut b = TarBuilder::new();
    if let Some(c) = commit {
        b = b.global_comment(c);
    }
    entries.iter().cloned().fold(b, TarBuilder::push).gz()
}

/// The git tree id of the entries under `prefix/` (what `git rev-parse <commit>:<path>` prints for them).
pub fn tree_of(entries: &[E], prefix: &str) -> String {
    let mut t = TreeBuilder::default();
    let p = format!("{}/", prefix.trim_end_matches('/'));
    let file_of = |name: &str| entries.iter().find(|e| e.name == name && e.flag == b'0').map(|e| (e.data.clone(), e.mode));
    for e in entries {
        let Some(rel) = e.name.strip_prefix(&p) else { continue };
        match e.flag {
            b'0' => t.add(rel, if e.mode & 0o100 != 0 { Mode::Exec } else { Mode::File }, blob_oid(&e.data)),
            b'2' => t.add(rel, Mode::Link, blob_oid(e.link.as_bytes())),
            b'1' => {
                let (d, m) = file_of(&e.link).expect("hard link target");
                t.add(rel, if m & 0o100 != 0 { Mode::Exec } else { Mode::File }, blob_oid(&d));
            }
            _ => {}
        }
    }
    t.oid().expect("files")
}

/// The same entries as a folder on disk (links as real symlinks), rooted at `dir` (the `top/` part dropped).
pub fn write_tree(entries: &[E], dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for e in entries {
        let Some(rel) = e.name.strip_prefix("top/").filter(|r| !r.is_empty()) else { continue };
        let p = dir.join(rel.trim_end_matches('/'));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        match e.flag {
            b'0' => {
                std::fs::write(&p, &e.data).unwrap();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(e.mode)).unwrap();
            }
            b'5' => std::fs::create_dir_all(&p).unwrap(),
            b'2' => std::os::unix::fs::symlink(&e.link, &p).unwrap(),
            _ => {}
        }
    }
}
