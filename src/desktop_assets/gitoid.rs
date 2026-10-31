//! Git object ids (SHA-1) of what a source contained, so a pack can be pinned to `git rev-parse <commit>:<path>`.
//!
//! A blob is `sha1("blob <len>\0" + bytes)`; a symlink is a blob of its target text with mode `120000`; a tree is
//! `sha1("tree <len>\0" + entries)`, each entry `"<mode> <name>\0" + 20 raw bytes`, sorted by name with folders
//! compared as `name/`. Folders without files are not part of a git tree. Only SHA-1 repositories are supported.

use std::collections::BTreeMap;

/// A small SHA-1 (FIPS 180-4). Used for git object ids only, never for security decisions on its own: a pack is
/// also covered by the SHA-256 of its archive when one is pinned, and by the SHA-256 content hash after install.
#[derive(Clone)]
pub struct Sha1 {
    h: [u32; 5],
    buf: [u8; 64],
    n: usize,
    len: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Sha1 { h: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0], buf: [0; 64], n: 0, len: 0 }
    }
}

impl Sha1 {
    fn block(&mut self, b: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b_, mut c, mut d, mut e] = self.h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b_ & c) | (!b_ & d), 0x5a82_7999),
                20..=39 => (b_ ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b_ & c) | (b_ & d) | (c & d), 0x8f1b_bcdc),
                _ => (b_ ^ c ^ d, 0xca62_c1d6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b_.rotate_left(30);
            b_ = a;
            a = t;
        }
        for (h, v) in self.h.iter_mut().zip([a, b_, c, d, e]) {
            *h = h.wrapping_add(v);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u64;
        while !data.is_empty() {
            let take = (64 - self.n).min(data.len());
            self.buf[self.n..self.n + take].copy_from_slice(&data[..take]);
            self.n += take;
            data = &data[take..];
            if self.n == 64 {
                let b = self.buf;
                self.block(&b);
                self.n = 0;
            }
        }
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.n != 56 {
            self.update(&[0]);
        }
        let mut b = self.buf;
        b[56..].copy_from_slice(&bits.to_be_bytes());
        self.block(&b);
        let mut out = [0u8; 20];
        for (i, h) in self.h.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&h.to_be_bytes());
        }
        out
    }
}

pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// A blob hasher: feed the content after construction (the length must be known up front, as in git).
pub fn blob_hasher(len: u64) -> Sha1 {
    let mut s = Sha1::default();
    s.update(format!("blob {len}\0").as_bytes());
    s
}

pub fn blob_oid(data: &[u8]) -> [u8; 20] {
    let mut s = blob_hasher(data.len() as u64);
    s.update(data);
    s.finish()
}

/// What a tree entry is, for its mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    File,
    Exec,
    Link,
}

impl Mode {
    fn text(self) -> &'static str {
        match self {
            Mode::File => "100644",
            Mode::Exec => "100755",
            Mode::Link => "120000",
        }
    }
}

#[derive(Default)]
struct Dir {
    leaves: BTreeMap<String, (Mode, [u8; 20])>,
    dirs: BTreeMap<String, Dir>,
}

impl Dir {
    fn oid(&self) -> Option<[u8; 20]> {
        // git's order: names compared bytewise, a folder as if it ended in '/'
        let mut items: Vec<(String, &'static str, [u8; 20])> = self.leaves.iter().map(|(n, (m, o))| (n.clone(), m.text(), *o)).collect();
        for (n, d) in &self.dirs {
            if let Some(o) = d.oid() {
                items.push((format!("{n}/"), "40000", o));
            }
        }
        if items.is_empty() {
            return None;
        }
        items.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut body = Vec::new();
        for (n, m, o) in items {
            body.extend_from_slice(m.as_bytes());
            body.push(b' ');
            body.extend_from_slice(n.trim_end_matches('/').as_bytes());
            body.push(0);
            body.extend_from_slice(&o);
        }
        let mut s = Sha1::default();
        s.update(format!("tree {}\0", body.len()).as_bytes());
        s.update(&body);
        Some(s.finish())
    }
}

/// Collects the files and links of one subtree and gives its git tree id.
#[derive(Default)]
pub struct TreeBuilder {
    root: Dir,
}

impl TreeBuilder {
    /// `rel` is relative to the subtree root, already validated.
    pub fn add(&mut self, rel: &str, mode: Mode, oid: [u8; 20]) {
        let mut d = &mut self.root;
        let mut parts: Vec<&str> = rel.split('/').collect();
        let leaf = parts.pop().unwrap_or_default();
        for p in parts {
            d = d.dirs.entry(p.to_string()).or_default();
        }
        d.leaves.insert(leaf.to_string(), (mode, oid));
    }

    /// The tree id, or `None` for a subtree without any file.
    pub fn oid(&self) -> Option<String> {
        self.root.oid().map(|o| hex(&o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_known_answers() {
        let h = |s: &[u8]| {
            let mut x = Sha1::default();
            x.update(s);
            hex(&x.finish())
        };
        assert_eq!(h(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(h(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(h(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        let mut x = Sha1::default();
        for _ in 0..1000 {
            x.update(&[b'a'; 1000]);
        }
        assert_eq!(hex(&x.finish()), "34aa973cd4c4daa4f61eeb2bdbad27316534016f", "a million a's, fed in pieces");
    }

    #[test]
    fn git_blob_and_tree_ids_match_git() {
        // `printf 'hello\n' | git hash-object --stdin`
        assert_eq!(hex(&blob_oid(b"hello\n")), "ce013625030ba8dba906f756967f9e9ca394464a");
        assert_eq!(hex(&blob_oid(b"")), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        // `git write-tree` of: a (hello), a.b ("1"), b/c (empty), l -> a
        let mut t = TreeBuilder::default();
        t.add("a", Mode::File, blob_oid(b"hello\n"));
        t.add("b/c", Mode::File, blob_oid(b""));
        t.add("l", Mode::Link, blob_oid(b"a"));
        assert_eq!(t.oid().as_deref(), Some("01365efb79db53120043c5dd983935eb589dad28"));
        t.add("a.b", Mode::File, blob_oid(b"1"));
        assert_eq!(t.oid().as_deref(), Some("97869d93e0bd712ec1730a7a683cb429609cd901"));
        t.add("x/x", Mode::File, blob_oid(b"2"));
        t.root.leaves.remove("a.b");
        assert_eq!(t.oid().as_deref(), Some("396a3095731a58b862de90ea8693ba1e43f61271"));
        let mut e = TreeBuilder::default();
        e.add("x", Mode::Exec, blob_oid(b"2"));
        assert_ne!(
            e.oid(),
            {
                let mut f = TreeBuilder::default();
                f.add("x", Mode::File, blob_oid(b"2"));
                f.oid()
            },
            "the executable bit is part of the id"
        );
        assert_eq!(TreeBuilder::default().oid(), None);
    }

    #[test]
    fn folders_sort_as_if_they_ended_in_a_slash() {
        // `a.b` (file) sorts before `a` (folder, compared as `a/`) because '.' < '/'; a plain name sort would differ.
        let mut t1 = TreeBuilder::default();
        t1.add("a.b", Mode::File, blob_oid(b"1"));
        t1.add("a/x", Mode::File, blob_oid(b"2"));
        let mut t2 = TreeBuilder::default();
        t2.add("a/x", Mode::File, blob_oid(b"2"));
        t2.add("a.b", Mode::File, blob_oid(b"1"));
        assert_eq!(t1.oid(), t2.oid(), "insertion order does not matter");
        assert_eq!(t1.oid().as_deref(), Some("2d93e296d7c17d0e9756f87391c6991cb5b01867"), "`git write-tree` of a.b + a/x");
    }
}
