//! Pinned git objects fetched over git's smart HTTP protocol, version 2 (git's `gitprotocol-v2`, `gitprotocol-http`,
//! `gitprotocol-common` and `gitformat-pack` documents), for packs whose source has no usable archive: only the pinned
//! tree (and the pinned licence / index blobs) is requested, not the repository.
//!
//! Three requests, every response untrusted and capped while it streams:
//!
//! 1. `GET <git_url>/info/refs?service=git-upload-pack` with `Git-Protocol: version=2`: the server must speak v2 and
//!    offer `fetch` with `shallow` and `filter` (and SHA-1 object ids).
//! 2. `fetch` of the pinned commit, `deepen 1`, `filter blob:none` (the commit and its trees, no file content): every
//!    pinned tree and blob id must be found at its path below the commit's own tree. This binds the pins to the commit.
//! 3. `fetch` of exactly the pinned ids, no filter: the pack holds them and everything below them.
//!
//! A pack is read from an owned file: version 2, exact object count, nothing after the SHA-1 trailer; types 1-4, 6, 7;
//! no thin packs (a delta's base is in the pack); delta chains of at most [`MAX_CHAIN`]; every delta instruction bounds
//! checked; each object at most `max_file` bytes, and all inflating together at most `max_stream` bytes of work. Objects
//! are resolved lazily from the file, so memory follows the largest object, not the pack. Every object's id is
//! recomputed from its bytes: a server can only send the objects that were asked for, or be refused.

use super::gitoid::{self, Sha1};
use super::Limits;
use crate::theme::engine::sha256::{self, Sha256};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::os::unix::fs::FileExt;
use std::path::Path;

/// Longest delta chain resolved (git writes at most 50 by default).
pub const MAX_CHAIN: usize = 64;
/// The capability advertisement and the commit-and-trees response.
pub const MAX_CAPS: u64 = 64 << 10;
pub const MAX_CHAIN_RESPONSE: u64 = 64 << 20;
/// Largest pkt-line (`LARGE_PACKET_MAX`).
const MAX_PKT: usize = 65520;
/// Progress text accepted on side band 2 (`no-progress` is requested; anything beyond this is refused).
const MAX_PROGRESS: u64 = 1 << 20;

pub const OBJ_COMMIT: u8 = 1;
pub const OBJ_TREE: u8 = 2;
pub const OBJ_BLOB: u8 = 3;
const OBJ_TAG: u8 = 4;
const OBJ_OFS_DELTA: u8 = 6;
const OBJ_REF_DELTA: u8 = 7;

fn kind_name(k: u8) -> &'static str {
    match k {
        OBJ_COMMIT => "commit",
        OBJ_TREE => "tree",
        OBJ_BLOB => "blob",
        _ => "tag",
    }
}

pub fn parse_oid(h: &str) -> Option<[u8; 20]> {
    if h.len() != 40 || !h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    let mut o = [0u8; 20];
    for (i, b) in o.iter_mut().enumerate() {
        *b = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(o)
}

// ---------------------------------------------------------------- pkt-lines

fn pkt(s: &str) -> Vec<u8> {
    let mut v = format!("{:04x}", s.len() + 4).into_bytes();
    v.extend_from_slice(s.as_bytes());
    v
}

/// A v2 `fetch` command: `wants` (hex ids) and argument lines, ending with `done`.
pub fn fetch_request(wants: &[String], args: &[&str]) -> Vec<u8> {
    let mut b = pkt("command=fetch\n");
    b.extend(pkt("object-format=sha1\n"));
    b.extend_from_slice(b"0001");
    for w in wants {
        b.extend(pkt(&format!("want {w}\n")));
    }
    for a in args {
        b.extend(pkt(&format!("{a}\n")));
    }
    b.extend(pkt("ofs-delta\n"));
    b.extend(pkt("no-progress\n"));
    b.extend(pkt("done\n"));
    b.extend_from_slice(b"0000");
    b
}

#[derive(Debug, PartialEq, Eq)]
enum Pkt {
    Flush,
    Delim,
    End,
    Data(Vec<u8>),
}

/// The next pkt-line; `None` at a clean end of the stream.
fn read_pkt(r: &mut impl Read) -> Result<Option<Pkt>, String> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        let k = r.read(&mut len[got..]).map_err(|e| format!("reading the response: {e}"))?;
        if k == 0 {
            return if got == 0 { Ok(None) } else { Err("the response ends inside a pkt-line length".into()) };
        }
        got += k;
    }
    let n = std::str::from_utf8(&len).ok().filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit())).and_then(|s| usize::from_str_radix(s, 16).ok()).ok_or("the response is not pkt-lines")?;
    match n {
        0 => return Ok(Some(Pkt::Flush)),
        1 => return Ok(Some(Pkt::Delim)),
        2 => return Ok(Some(Pkt::End)),
        3 => return Err("invalid pkt-line length 3".into()),
        n if n > MAX_PKT => return Err(format!("pkt-line of {n} bytes (the limit is {MAX_PKT})")),
        _ => {}
    }
    let mut d = vec![0u8; n - 4];
    r.read_exact(&mut d).map_err(|e| if e.kind() == std::io::ErrorKind::UnexpectedEof { "the response ends inside a pkt-line".to_string() } else { format!("reading the response: {e}") })?;
    Ok(Some(Pkt::Data(d)))
}

fn text(d: &[u8]) -> String {
    String::from_utf8_lossy(d).trim_end().chars().take(300).map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// Checks a v2 capability advertisement: `fetch` with `shallow` and `filter`, SHA-1 objects.
pub fn check_caps(body: &[u8]) -> Result<(), String> {
    // gitprotocol-http: the entity must start with a pkt-line length and '#'
    if body.len() < 5 || !body[..4].iter().all(u8::is_ascii_hexdigit) || body[4] != b'#' {
        return Err("not a smart git server (the reply does not start with `# service=`)".into());
    }
    let mut r = body;
    let mut lines = Vec::new();
    while let Some(p) = read_pkt(&mut r)? {
        if let Pkt::Data(d) = p {
            lines.push(String::from_utf8_lossy(&d).trim_end_matches('\n').to_string());
        }
    }
    if lines.first().map(String::as_str) != Some("# service=git-upload-pack") {
        return Err("the server did not answer for git-upload-pack".into());
    }
    if lines.get(1).map(String::as_str) != Some("version 2") {
        return Err("the server does not speak git protocol version 2".into());
    }
    let fetch = lines.iter().find_map(|l| if l == "fetch" { Some("") } else { l.strip_prefix("fetch=") }).ok_or("the server offers no `fetch` command")?;
    let features: Vec<&str> = fetch.split(' ').collect();
    for f in ["shallow", "filter"] {
        if !features.contains(&f) {
            return Err(format!("the server's fetch has no `{f}` (it offers: {fetch})"));
        }
    }
    if let Some(of) = lines.iter().find_map(|l| l.strip_prefix("object-format=")) {
        if of != "sha1" {
            return Err(format!("the repository uses {of} object ids; pins are SHA-1"));
        }
    }
    Ok(())
}

/// Reads a `fetch` response, writes the pack (side band 1) to `out`; returns the pack's size. The whole response is
/// capped by the reader it comes through.
pub fn demux(mut r: impl Read, out: &Path) -> Result<u64, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let (mut n, mut progress) = (0u64, 0u64);
    // sections before the pack: shallow-info (a shallow fetch), acknowledgments; nothing else was asked for
    loop {
        match read_pkt(&mut r)? {
            None => return Err("the response ended before a pack".into()),
            Some(Pkt::Flush | Pkt::End) => return Err("the server sent no pack".into()),
            Some(Pkt::Delim) => {}
            Some(Pkt::Data(d)) => {
                if let Some(e) = d.strip_prefix(b"ERR ") {
                    return Err(format!("the server refused: {}", text(e)));
                }
                match d.as_slice() {
                    b"packfile\n" => break,
                    b"packfile-uris\n" | b"wanted-refs\n" => return Err(format!("unexpected section `{}`", text(&d))),
                    _ => {}
                }
            }
        }
    }
    loop {
        match read_pkt(&mut r)? {
            None => return Err("the response ended inside the pack".into()),
            Some(Pkt::Flush) => break,
            Some(Pkt::Delim | Pkt::End) => return Err("the pack section is not terminated by a flush".into()),
            Some(Pkt::Data(d)) => match d.first() {
                Some(1) => {
                    n += (d.len() - 1) as u64;
                    f.write_all(&d[1..]).map_err(|e| e.to_string())?;
                }
                Some(2) => {
                    progress += d.len() as u64;
                    if progress > MAX_PROGRESS {
                        return Err("too much progress text from the server".into());
                    }
                }
                Some(3) => return Err(format!("the server failed: {}", text(&d[1..]))),
                _ => return Err("a pack line without a side band".into()),
            },
        }
    }
    // nothing may follow the final flush (an optional response-end packet aside)
    match read_pkt(&mut r)? {
        None | Some(Pkt::End) => {}
        Some(_) => return Err("data after the end of the response".into()),
    }
    let mut rest = [0u8; 1];
    if r.read(&mut rest).map_err(|e| e.to_string())? != 0 {
        return Err("data after the end of the response".into());
    }
    f.sync_all().map_err(|e| e.to_string())?;
    Ok(n)
}

/// A reader that counts and hashes what passes and refuses more than `cap` bytes.
pub struct Capped<R> {
    pub inner: R,
    pub cap: u64,
    pub n: u64,
    pub sha: Sha256,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        super::check_cancel().map_err(std::io::Error::other)?;
        let k = self.inner.read(buf)?;
        self.n += k as u64;
        if self.n > self.cap {
            return Err(std::io::Error::other(format!("the response is larger than {} KiB", self.cap >> 10)));
        }
        self.sha.update(&buf[..k]);
        Ok(k)
    }
}

// ---------------------------------------------------------------- packs

#[derive(Debug, Clone, Copy)]
enum Base {
    None,
    Ofs(u64),
    Ref([u8; 20]),
}

#[derive(Debug)]
struct Ent {
    /// Where the zlib data starts.
    data: u64,
    /// The type as stored (6/7 for deltas) and the inflated size of what is stored.
    stored: u8,
    size: u64,
    base: Base,
    /// The object's type and id, once resolved.
    kind: u8,
    oid: Option<[u8; 20]>,
}

/// A verified pack file: every object's id recomputed, deltas resolved.
pub struct Pack {
    file: std::fs::File,
    end: u64,
    ents: Vec<Ent>,
    by_oid: HashMap<[u8; 20], usize>,
    /// Entry start offset -> entry.
    by_off: HashMap<u64, usize>,
    max_obj: u64,
    max_work: u64,
    work: Cell<u64>,
}

impl Pack {
    fn read_at(&self, buf: &mut [u8], at: u64) -> Result<usize, String> {
        let n = (buf.len() as u64).min(self.end.saturating_sub(at)) as usize;
        let mut got = 0;
        while got < n {
            let k = self.file.read_at(&mut buf[got..n], at + got as u64).map_err(|e| e.to_string())?;
            if k == 0 {
                return Err("the pack file is shorter than it was".into());
            }
            got += k;
        }
        Ok(n)
    }

    fn charge(&self, n: u64) -> Result<(), String> {
        let w = self.work.get() + n;
        self.work.set(w);
        if w > self.max_work {
            return Err(format!("inflating the pack takes more than {} MiB of work (a compression bomb?)", self.max_work >> 20));
        }
        Ok(())
    }

    /// Inflates the zlib stream at `at` that must hold exactly `size` bytes; returns the compressed length.
    fn inflate(&self, at: u64, size: u64, sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<u64, String> {
        use flate2::{Decompress, FlushDecompress, Status};
        let mut d = Decompress::new(true);
        let mut inb = vec![0u8; 64 << 10];
        let mut out = vec![0u8; 64 << 10];
        let (mut s, mut e, mut next) = (0usize, 0usize, at);
        loop {
            if s == e {
                e = self.read_at(&mut inb, next)?;
                if e == 0 {
                    return Err("an object's data runs past the end of the pack".into());
                }
                next += e as u64;
                s = 0;
            }
            let (ti, to) = (d.total_in(), d.total_out());
            // one byte more than the object may have, so a longer stream is caught
            let room = (out.len() as u64).min(size - to + 1) as usize;
            let st = d.decompress(&inb[s..e], &mut out[..room], FlushDecompress::None).map_err(|_| "corrupt compressed data in the pack".to_string())?;
            let (used, made) = ((d.total_in() - ti) as usize, (d.total_out() - to) as usize);
            s += used;
            if d.total_out() > size {
                return Err("an object inflates to more than its header says".into());
            }
            self.charge(made as u64)?;
            sink(&out[..made])?;
            match st {
                Status::StreamEnd if d.total_out() == size => return Ok(d.total_in()),
                Status::StreamEnd => return Err("an object inflates to less than its header says".into()),
                _ if used == 0 && made == 0 && s < e => return Err("corrupt compressed data in the pack".into()),
                _ => {}
            }
        }
    }

    fn inflate_vec(&self, i: usize) -> Result<Vec<u8>, String> {
        let e = &self.ents[i];
        let mut v = Vec::with_capacity(e.size as usize);
        self.inflate(e.data, e.size, &mut |b| {
            v.extend_from_slice(b);
            Ok(())
        })?;
        Ok(v)
    }

    /// Reads and verifies `path`: the trailer, every entry, every delta. `max_objects` bounds the count.
    pub fn open(path: &Path, lim: &Limits) -> Result<Pack, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        if len < 32 {
            return Err("the pack is too short".into());
        }
        let mut p = Pack { file, end: len, ents: Vec::new(), by_oid: HashMap::new(), by_off: HashMap::new(), max_obj: lim.max_file, max_work: lim.max_stream, work: Cell::new(0) };
        // the trailer: SHA-1 of everything before it
        let mut h = Sha1::default();
        let mut buf = vec![0u8; 64 << 10];
        let mut at = 0u64;
        while at < len - 20 {
            let want = (buf.len() as u64).min(len - 20 - at) as usize;
            let k = p.read_at(&mut buf[..want], at)?;
            h.update(&buf[..k]);
            at += k as u64;
        }
        let mut trailer = [0u8; 20];
        p.read_at(&mut trailer, len - 20)?;
        if h.finish() != trailer {
            return Err("the pack's checksum does not match (damaged or truncated)".into());
        }
        p.end = len - 20;
        let mut head = [0u8; 12];
        p.read_at(&mut head, 0)?;
        if &head[..4] != b"PACK" || u32::from_be_bytes([head[4], head[5], head[6], head[7]]) != 2 {
            return Err("not a version 2 pack".into());
        }
        let count = u32::from_be_bytes([head[8], head[9], head[10], head[11]]) as usize;
        if count > lim.max_entries {
            return Err(format!("the pack holds {count} objects, the limit is {}", lim.max_entries));
        }
        let mut pos = 12u64;
        for _ in 0..count {
            if pos >= p.end {
                return Err("the pack holds fewer objects than it says".into());
            }
            let start = pos;
            let mut hb = [0u8; 32];
            let n = p.read_at(&mut hb, pos)?;
            let mut j = 0;
            let mut c = hb[0];
            j += 1;
            let stored = (c >> 4) & 7;
            let mut size = u64::from(c & 15);
            let mut shift = 4;
            while c & 0x80 != 0 {
                if j >= n || shift > 60 {
                    return Err("an object's size does not fit".into());
                }
                c = hb[j];
                j += 1;
                if u64::from(c & 0x7f) > (u64::MAX >> shift) {
                    return Err("an object's size does not fit".into());
                }
                size |= u64::from(c & 0x7f) << shift;
                shift += 7;
            }
            let base = match stored {
                OBJ_COMMIT | OBJ_TREE | OBJ_BLOB | OBJ_TAG => Base::None,
                OBJ_OFS_DELTA => {
                    // offset encoding: 7 bits per byte, plus 2^7 + 2^14 + ... for each continuation
                    let mut c = *hb.get(j).ok_or("a delta offset runs past the end")?;
                    j += 1;
                    let mut off = u64::from(c & 0x7f);
                    while c & 0x80 != 0 {
                        c = *hb.get(j).filter(|_| j < n && off < (1 << 56)).ok_or("a delta offset does not fit")?;
                        j += 1;
                        off = ((off + 1) << 7) | u64::from(c & 0x7f);
                    }
                    if off == 0 || off > start {
                        return Err("a delta's base lies outside the pack".into());
                    }
                    if !p.by_off.contains_key(&(start - off)) {
                        return Err("a delta's base offset is not the start of an object".into());
                    }
                    Base::Ofs(start - off)
                }
                OBJ_REF_DELTA => {
                    if j + 20 > n {
                        return Err("a delta's base id runs past the end".into());
                    }
                    let mut o = [0u8; 20];
                    o.copy_from_slice(&hb[j..j + 20]);
                    j += 20;
                    Base::Ref(o)
                }
                t => return Err(format!("unknown object type {t} in the pack")),
            };
            // a delta is at most about as large as what it builds; an object is at most max_file
            let cap = if matches!(base, Base::None) { p.max_obj } else { p.max_obj.saturating_mul(2) };
            if size > cap {
                return Err(format!("an object of the pack is {} KiB, the limit is {} KiB", size >> 10, cap >> 10));
            }
            let data = pos + j as u64;
            let (kind, oid, used) = if matches!(base, Base::None) {
                let mut s = Sha1::default();
                s.update(format!("{} {size}\0", kind_name(stored)).as_bytes());
                let used = p.inflate(data, size, &mut |b| {
                    s.update(b);
                    Ok(())
                })?;
                (stored, Some(s.finish()), used)
            } else {
                (0, None, p.inflate(data, size, &mut |_| Ok(()))?)
            };
            pos = data + used;
            p.by_off.insert(start, p.ents.len());
            if let Some(o) = oid {
                if p.by_oid.insert(o, p.ents.len()).is_some() {
                    return Err(format!("object {} appears twice in the pack", gitoid::hex(&o)));
                }
            }
            p.ents.push(Ent { data, stored, size, base, kind, oid });
        }
        if pos != p.end {
            return Err("data between the last object and the pack's checksum".into());
        }
        // deltas: resolved in rounds, as their bases become known (a REF_DELTA base may itself be a delta)
        let mut pending: Vec<usize> = (0..p.ents.len()).filter(|&i| p.ents[i].oid.is_none()).collect();
        while !pending.is_empty() {
            let before = pending.len();
            let mut left = Vec::new();
            for i in pending {
                match p.build(i)? {
                    Some((kind, data)) => {
                        let o = object_id(kind, &data);
                        if p.by_oid.insert(o, i).is_some() {
                            return Err(format!("object {} appears twice in the pack", gitoid::hex(&o)));
                        }
                        p.ents[i].kind = kind;
                        p.ents[i].oid = Some(o);
                    }
                    None => left.push(i),
                }
            }
            if left.len() == before {
                return Err(format!("{} deltas of the pack refer to objects that are not in it (a thin pack)", left.len()));
            }
            pending = left;
        }
        Ok(p)
    }

    /// The type and content of entry `i`, rebuilt through its delta chain; `None` while a REF_DELTA base in the chain
    /// is not resolved yet.
    fn build(&self, i: usize) -> Result<Option<(u8, Vec<u8>)>, String> {
        let mut ds = Vec::new();
        let mut j = i;
        loop {
            let next = match self.ents[j].base {
                Base::None => break,
                Base::Ofs(off) => self.by_off[&off],
                Base::Ref(o) => match self.by_oid.get(&o) {
                    Some(&k) => k,
                    None => return Ok(None),
                },
            };
            ds.push(j);
            if ds.len() > MAX_CHAIN {
                return Err(format!("a delta chain of the pack is longer than {MAX_CHAIN}"));
            }
            j = next;
        }
        let mut data = self.inflate_vec(j)?;
        for &d in ds.iter().rev() {
            let delta = self.inflate_vec(d)?;
            data = apply_delta(&data, &delta, self.max_obj)?;
            self.charge(data.len() as u64)?;
        }
        Ok(Some((self.ents[j].stored, data)))
    }

    pub fn len(&self) -> usize {
        self.ents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ents.is_empty()
    }

    pub fn contains(&self, oid: &[u8; 20]) -> bool {
        self.by_oid.contains_key(oid)
    }

    /// The type and content of object `oid`, if the pack has it.
    pub fn get(&self, oid: &[u8; 20]) -> Result<Option<(u8, Vec<u8>)>, String> {
        match self.by_oid.get(oid) {
            None => Ok(None),
            Some(&i) => {
                let (k, d) = self.build(i)?.ok_or("a delta base disappeared")?;
                // the id was computed from these bytes when the pack was opened; a file changed since is refused
                if object_id(k, &d) != *oid {
                    return Err(format!("object {} changed in the pack file", gitoid::hex(oid)));
                }
                Ok(Some((k, d)))
            }
        }
    }

    pub fn work(&self) -> u64 {
        self.work.get()
    }
}

pub fn object_id(kind: u8, data: &[u8]) -> [u8; 20] {
    let mut s = Sha1::default();
    s.update(format!("{} {}\0", kind_name(kind), data.len()).as_bytes());
    s.update(data);
    s.finish()
}

fn delta_size(d: &[u8], j: &mut usize) -> Result<u64, String> {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let c = *d.get(*j).ok_or("a delta header runs past its end")?;
        *j += 1;
        if shift > 63 || u64::from(c & 0x7f) > (u64::MAX >> shift) {
            return Err("a delta size does not fit".into());
        }
        v |= u64::from(c & 0x7f) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            return Ok(v);
        }
    }
}

/// Applies a git delta (gitformat-pack, "Deltified representation") to `base`; the result is at most `max` bytes.
pub fn apply_delta(base: &[u8], d: &[u8], max: u64) -> Result<Vec<u8>, String> {
    let mut j = 0;
    if delta_size(d, &mut j)? != base.len() as u64 {
        return Err("a delta was made for a base of another size".into());
    }
    let size = delta_size(d, &mut j)?;
    if size > max {
        return Err(format!("a delta builds an object of {} KiB, the limit is {} KiB", size >> 10, max >> 10));
    }
    let mut out = Vec::with_capacity(size as usize);
    while j < d.len() {
        let c = d[j];
        j += 1;
        if c & 0x80 != 0 {
            let (mut off, mut n) = (0u64, 0u64);
            for k in 0..4 {
                if c & (1 << k) != 0 {
                    off |= u64::from(*d.get(j).ok_or("a delta copy runs past its end")?) << (8 * k);
                    j += 1;
                }
            }
            for k in 0..3 {
                if c & (1 << (4 + k)) != 0 {
                    n |= u64::from(*d.get(j).ok_or("a delta copy runs past its end")?) << (8 * k);
                    j += 1;
                }
            }
            if n == 0 {
                n = 0x10000;
            }
            if off + n > base.len() as u64 || out.len() as u64 + n > size {
                return Err("a delta copies outside its base or past its result".into());
            }
            out.extend_from_slice(&base[off as usize..(off + n) as usize]);
        } else if c != 0 {
            let n = usize::from(c);
            if j + n > d.len() || (out.len() + n) as u64 > size {
                return Err("a delta insert runs past its end or its result".into());
            }
            out.extend_from_slice(&d[j..j + n]);
            j += n;
        } else {
            return Err("a delta uses the reserved instruction 0".into());
        }
    }
    if out.len() as u64 != size {
        return Err("a delta builds less than it says".into());
    }
    Ok(out)
}

// ---------------------------------------------------------------- trees and commits

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dir,
    File,
    Exec,
    Link,
    /// A submodule: a commit of another repository.
    Gitlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: Mode,
    pub name: String,
    pub oid: [u8; 20],
}

/// The entries of a tree object, refused unless canonical: known modes as git writes them, plain names, sorted in
/// git's order without duplicates (what `gitoid` computes the same tree id from).
pub fn parse_tree(d: &[u8]) -> Result<Vec<TreeEntry>, String> {
    let mut v: Vec<TreeEntry> = Vec::new();
    let mut prev: Option<Vec<u8>> = None;
    let mut names = HashSet::new();
    let mut j = 0;
    while j < d.len() {
        let sp = d[j..].iter().position(|&b| b == b' ').ok_or("a tree entry has no mode")? + j;
        let mode = match &d[j..sp] {
            b"40000" => Mode::Dir,
            b"100644" => Mode::File,
            b"100755" => Mode::Exec,
            b"120000" => Mode::Link,
            b"160000" => Mode::Gitlink,
            m => return Err(format!("a tree entry has the mode `{}` (not one git writes)", text(m))),
        };
        let nul = d[sp + 1..].iter().position(|&b| b == 0).ok_or("a tree entry has no name end")? + sp + 1;
        let raw = &d[sp + 1..nul];
        let name = std::str::from_utf8(raw).map_err(|_| "a tree entry name is not UTF-8".to_string())?;
        if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.len() > 255 || name.chars().any(char::is_control) || name.contains('\\') {
            return Err(format!("a tree entry has the name `{}`", name.escape_debug()));
        }
        if nul + 21 > d.len() {
            return Err("a tree entry's id runs past the end".into());
        }
        let mut oid = [0u8; 20];
        oid.copy_from_slice(&d[nul + 1..nul + 21]);
        let mut key = raw.to_vec();
        if mode == Mode::Dir {
            key.push(b'/');
        }
        if prev.as_ref().is_some_and(|p| *p >= key) {
            return Err(format!("tree entries are not in git's order (or repeat) at `{name}`"));
        }
        if !names.insert(name) {
            return Err(format!("tree entries repeat the name `{name}`"));
        }
        prev = Some(key);
        v.push(TreeEntry { mode, name: name.to_string(), oid });
        j = nul + 21;
    }
    Ok(v)
}

/// The tree id of a commit object.
pub fn commit_tree(d: &[u8]) -> Result<[u8; 20], String> {
    let line = d.strip_prefix(b"tree ").and_then(|r| r.get(..41)).filter(|l| l[40] == b'\n').ok_or("the commit does not start with its tree")?;
    parse_oid(std::str::from_utf8(&line[..40]).map_err(|_| "bad commit")?).ok_or_else(|| "the commit's tree id is not hex".into())
}

/// Follows `path` (source-relative, `.` for the root) from tree `root` through the trees of `pack`.
pub fn lookup(pack: &Pack, root: [u8; 20], path: &str) -> Result<(Mode, [u8; 20]), String> {
    let mut cur = (Mode::Dir, root);
    if path == "." || path.is_empty() {
        return Ok(cur);
    }
    let mut walked = String::new();
    for part in path.split('/') {
        if cur.0 != Mode::Dir {
            return Err(format!("`{walked}` is not a folder in the pinned commit"));
        }
        let (k, d) = pack.get(&cur.1)?.ok_or_else(|| format!("the server did not send the tree of `{}`", if walked.is_empty() { "." } else { &walked }))?;
        if k != OBJ_TREE {
            return Err(format!("`{walked}` is not a tree object"));
        }
        let e = parse_tree(&d)?.into_iter().find(|e| e.name == part).ok_or_else(|| format!("`{path}` does not exist in the pinned commit"))?;
        if !walked.is_empty() {
            walked.push('/');
        }
        walked.push_str(part);
        cur = (e.mode, e.oid);
    }
    Ok(cur)
}

// ---------------------------------------------------------------- the fetch

/// The network side, replaceable in tests.
pub trait Transport {
    /// GET `url`, at most `cap` bytes, into memory.
    fn get(&self, url: &str, cap: u64) -> Result<Vec<u8>, String>;
    /// POST a v2 request to `url`; the body of a 200 reply.
    fn post(&self, url: &str, body: &[u8]) -> Result<Box<dyn Read>, String>;
}

/// What to fetch: the pinned commit and every pinned tree / blob, by source-relative path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub commit: String,
    pub trees: Vec<(String, String)>,
    pub blobs: Vec<(String, String)>,
}

/// One request of a fetch, for the provenance.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Exchange {
    pub what: String,
    pub bytes: u64,
    pub sha256: String,
}

pub struct Fetched {
    pub pack: Pack,
    pub exchanges: Vec<Exchange>,
    /// Every pin was found at its path below the pinned commit's tree.
    pub chain: bool,
}

fn post_pack(t: &dyn Transport, url: &str, body: &[u8], out: &Path, cap: u64, what: &str, lim: &Limits) -> Result<(Pack, Exchange), String> {
    let r = t.post(&format!("{url}/git-upload-pack"), body)?;
    let mut c = Capped { inner: r, cap, n: 0, sha: Sha256::default() };
    demux(&mut c, out).map_err(|e| format!("{what}: {e}"))?;
    let ex = Exchange { what: what.into(), bytes: c.n, sha256: sha256::hex(&c.sha.finish()) };
    let pack = Pack::open(out, lim).map_err(|e| format!("{what}: {e}"))?;
    Ok((pack, ex))
}

/// Runs the three requests into `dir` (a private job folder) and checks every pin against the commit.
pub fn fetch(t: &dyn Transport, url: &str, plan: &Plan, dir: &Path, lim: &Limits) -> Result<Fetched, String> {
    let commit = parse_oid(&plan.commit).ok_or("the pinned commit is not 40 hex digits")?;
    let caps = t.get(&format!("{url}/info/refs?service=git-upload-pack"), MAX_CAPS)?;
    check_caps(&caps)?;
    let mut h = Sha256::default();
    h.update(&caps);
    let mut exchanges = vec![Exchange { what: "capabilities".into(), bytes: caps.len() as u64, sha256: sha256::hex(&h.finish()) }];
    // 2: the commit and its trees, to find every pin at its path
    let body = fetch_request(std::slice::from_ref(&plan.commit), &["deepen 1", "filter blob:none"]);
    let (trees, ex) = post_pack(t, url, &body, &dir.join("commit.pack"), MAX_CHAIN_RESPONSE, "the pinned commit's trees", lim)?;
    exchanges.push(ex);
    let (k, c) = trees.get(&commit)?.ok_or_else(|| format!("the server did not send commit {}", plan.commit))?;
    if k != OBJ_COMMIT {
        return Err(format!("{} is not a commit", plan.commit));
    }
    let root = commit_tree(&c)?;
    for (p, want) in &plan.trees {
        let (m, o) = lookup(&trees, root, p)?;
        if m != Mode::Dir || gitoid::hex(&o) != *want {
            return Err(format!("`{p}` in commit {} is {} {}, the pin is tree {want}", plan.commit, if m == Mode::Dir { "tree" } else { "not a tree:" }, gitoid::hex(&o)));
        }
    }
    for (p, want) in &plan.blobs {
        let (m, o) = lookup(&trees, root, p)?;
        if !matches!(m, Mode::File | Mode::Exec) || gitoid::hex(&o) != *want {
            return Err(format!("`{p}` in commit {} is {}, the pin is blob {want}", plan.commit, gitoid::hex(&o)));
        }
    }
    drop(trees);
    let _ = std::fs::remove_file(dir.join("commit.pack"));
    // 3: exactly the pinned objects (and what is below the trees)
    let mut wants: Vec<String> = plan.trees.iter().chain(&plan.blobs).map(|(_, o)| o.clone()).collect();
    wants.sort();
    wants.dedup();
    let body = fetch_request(&wants, &[]);
    let (pack, ex) = post_pack(t, url, &body, &dir.join("content.pack"), lim.max_archive, "the pinned objects", lim)?;
    exchanges.push(ex);
    for w in &wants {
        if !pack.contains(&parse_oid(w).expect("validated")) {
            return Err(format!("the server did not send object {w}"));
        }
    }
    Ok(Fetched { pack, exchanges, chain: true })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::desktop_assets::testkit::{Scratch, E};
    use std::collections::BTreeMap;

    /// A tiny object store that writes packs and answers v2 fetches like a git server.
    #[derive(Default, Clone)]
    pub struct Repo {
        pub objects: BTreeMap<[u8; 20], (u8, Vec<u8>)>,
        pub commit: String,
    }

    #[derive(Default)]
    struct D {
        files: BTreeMap<String, (Mode, Vec<u8>)>,
        dirs: BTreeMap<String, D>,
    }

    impl Repo {
        pub fn add(&mut self, kind: u8, data: Vec<u8>) -> [u8; 20] {
            let o = object_id(kind, &data);
            self.objects.insert(o, (kind, data));
            o
        }

        fn tree(&mut self, d: &D) -> [u8; 20] {
            let mut items: Vec<(Vec<u8>, &str, String, [u8; 20])> = Vec::new();
            for (n, (m, data)) in &d.files {
                let o = self.add(OBJ_BLOB, data.clone());
                items.push((
                    n.clone().into_bytes(),
                    if *m == Mode::Exec {
                        "100755"
                    } else if *m == Mode::Link {
                        "120000"
                    } else {
                        "100644"
                    },
                    n.clone(),
                    o,
                ));
            }
            for (n, sub) in &d.dirs {
                let o = self.tree(sub);
                items.push((format!("{n}/").into_bytes(), "40000", n.clone(), o));
            }
            items.sort();
            let mut body = Vec::new();
            for (_, m, n, o) in items {
                body.extend_from_slice(format!("{m} {n}\0").as_bytes());
                body.extend_from_slice(&o);
            }
            self.add(OBJ_TREE, body)
        }

        /// The entries below `top/` as a commit.
        pub fn from_entries(entries: &[E]) -> Repo {
            let mut root = D::default();
            for e in entries {
                let Some(rel) = e.name.strip_prefix("top/").map(|r| r.trim_end_matches('/')).filter(|r| !r.is_empty()) else { continue };
                let mut parts: Vec<&str> = rel.split('/').collect();
                let leaf = parts.pop().unwrap();
                let mut d = &mut root;
                for p in parts {
                    d = d.dirs.entry(p.to_string()).or_default();
                }
                match e.flag {
                    b'0' => {
                        d.files.insert(leaf.into(), (if e.mode & 0o100 != 0 { Mode::Exec } else { Mode::File }, e.data.clone()));
                    }
                    b'2' => {
                        d.files.insert(leaf.into(), (Mode::Link, e.link.clone().into_bytes()));
                    }
                    b'5' => {
                        d.dirs.entry(leaf.to_string()).or_default();
                    }
                    _ => {}
                }
            }
            let mut r = Repo::default();
            let t = r.tree(&root);
            let c = r.add(OBJ_COMMIT, format!("tree {}\nauthor a <a> 0 +0000\ncommitter a <a> 0 +0000\n\nfixture\n", gitoid::hex(&t)).into_bytes());
            r.commit = gitoid::hex(&c);
            r
        }

        /// Everything reachable from `oid`; trees only (and the commit) when `blobs` is false.
        pub fn closure(&self, oid: [u8; 20], blobs: bool, out: &mut Vec<[u8; 20]>) {
            if out.contains(&oid) {
                return;
            }
            let Some((k, d)) = self.objects.get(&oid) else { return };
            if *k == OBJ_BLOB && !blobs {
                return;
            }
            out.push(oid);
            match *k {
                OBJ_COMMIT => self.closure(commit_tree(d).unwrap(), blobs, out),
                OBJ_TREE => {
                    for e in parse_tree(d).unwrap() {
                        self.closure(e.oid, blobs, out);
                    }
                }
                _ => {}
            }
        }
    }

    fn zlib(d: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(d).unwrap();
        e.finish().unwrap()
    }

    fn obj_header(kind: u8, size: u64) -> Vec<u8> {
        let mut v = Vec::new();
        let mut c = (kind << 4) | (size & 15) as u8;
        let mut s = size >> 4;
        while s > 0 {
            v.push(c | 0x80);
            c = (s & 0x7f) as u8;
            s >>= 7;
        }
        v.push(c);
        v
    }

    /// One raw pack entry.
    #[derive(Clone)]
    pub enum Raw {
        Whole(u8, Vec<u8>),
        /// A delta on the entry `n` before this one.
        Ofs(usize, Vec<u8>),
        Ref([u8; 20], Vec<u8>),
        /// Bytes as they are (a hostile entry).
        Bytes(Vec<u8>),
    }

    pub fn pack(entries: &[Raw]) -> Vec<u8> {
        let mut p = b"PACK".to_vec();
        p.extend_from_slice(&2u32.to_be_bytes());
        p.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        let mut starts = Vec::new();
        for e in entries {
            let start = p.len() as u64;
            match e {
                Raw::Whole(k, d) => {
                    p.extend(obj_header(*k, d.len() as u64));
                    p.extend(zlib(d));
                }
                Raw::Ofs(back, d) => {
                    p.extend(obj_header(OBJ_OFS_DELTA, d.len() as u64));
                    let mut off: u64 = start - starts[starts.len() - back];
                    let mut enc = vec![(off & 0x7f) as u8];
                    off >>= 7;
                    while off > 0 {
                        off -= 1;
                        enc.push(0x80 | (off & 0x7f) as u8);
                        off >>= 7;
                    }
                    enc.reverse();
                    p.extend(enc);
                    p.extend(zlib(d));
                }
                Raw::Ref(o, d) => {
                    p.extend(obj_header(OBJ_REF_DELTA, d.len() as u64));
                    p.extend_from_slice(o);
                    p.extend(zlib(d));
                }
                Raw::Bytes(b) => p.extend_from_slice(b),
            }
            starts.push(start);
        }
        let mut h = Sha1::default();
        h.update(&p);
        p.extend_from_slice(&h.finish());
        p
    }

    /// A v2 fetch response carrying `pack` on side band 1.
    pub fn response(pack: &[u8], shallow: Option<&str>) -> Vec<u8> {
        let mut r = Vec::new();
        if let Some(c) = shallow {
            r.extend(pkt("shallow-info\n"));
            r.extend(pkt(&format!("shallow {c}\n")));
            r.extend_from_slice(b"0001");
        }
        r.extend(pkt("packfile\n"));
        for ch in pack.chunks(65515) {
            let mut l = format!("{:04x}", ch.len() + 5).into_bytes();
            l.push(1);
            l.extend_from_slice(ch);
            r.extend(l);
        }
        r.extend_from_slice(b"0000");
        r
    }

    pub const CAPS: &[u8] = b"001e# service=git-upload-pack\n0000000eversion 2\n0013ls-refs=unborn\n0027fetch=shallow wait-for-done filter\n0012server-option\n0017object-format=sha1\n0000";

    /// Answers like a v2 server from a [`Repo`]: what is reachable from the wants (trees only with `filter blob:none`).
    pub struct Fake {
        pub repo: Repo,
        pub caps: Vec<u8>,
        pub requests: std::cell::RefCell<Vec<String>>,
        /// Replaces the answer to the n-th POST (0-based).
        pub tamper: Option<(usize, Vec<u8>)>,
    }

    impl Fake {
        pub fn new(repo: Repo) -> Fake {
            Fake { repo, caps: CAPS.to_vec(), requests: Default::default(), tamper: None }
        }
    }

    impl Transport for Fake {
        fn get(&self, url: &str, _cap: u64) -> Result<Vec<u8>, String> {
            self.requests.borrow_mut().push(format!("GET {url}"));
            Ok(self.caps.clone())
        }
        fn post(&self, url: &str, body: &[u8]) -> Result<Box<dyn Read>, String> {
            let n = self.requests.borrow().iter().filter(|r| r.starts_with("POST")).count();
            self.requests.borrow_mut().push(format!("POST {url}"));
            if let Some((k, b)) = &self.tamper {
                if *k == n {
                    return Ok(Box::new(std::io::Cursor::new(b.clone())));
                }
            }
            let mut r = body;
            let (mut wants, mut blobs) = (Vec::new(), true);
            while let Some(p) = read_pkt(&mut r).unwrap() {
                if let Pkt::Data(d) = p {
                    let l = String::from_utf8(d).unwrap();
                    if let Some(w) = l.strip_prefix("want ") {
                        wants.push(parse_oid(w.trim_end()).unwrap());
                    }
                    if l.starts_with("filter blob:none") {
                        blobs = false;
                    }
                }
            }
            let mut all = Vec::new();
            for w in &wants {
                if !self.repo.objects.contains_key(w) {
                    let mut e = pkt(&format!("ERR upload-pack: not our ref {}\n", gitoid::hex(w)));
                    e.extend_from_slice(b"0000");
                    return Ok(Box::new(std::io::Cursor::new(e)));
                }
                self.repo.closure(*w, blobs, &mut all);
            }
            let raws: Vec<Raw> = all.iter().map(|o| Raw::Whole(self.repo.objects[o].0, self.repo.objects[o].1.clone())).collect();
            Ok(Box::new(std::io::Cursor::new(response(&pack(&raws), if blobs { None } else { Some(&self.repo.commit) }))))
        }
    }

    fn open(s: &Scratch, name: &str, bytes: &[u8], lim: &Limits) -> Result<Pack, String> {
        let p = s.write(name, bytes);
        Pack::open(&p, lim)
    }

    fn lim() -> Limits {
        Limits { max_entries: 1000, max_file: 1 << 20, max_total: 8 << 20, max_stream: 16 << 20, max_archive: 8 << 20, max_text: 64 << 10 }
    }

    /// A delta that copies `base[a..b]` and inserts `ins`.
    fn delta(base_len: usize, a: u32, n: u32, ins: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        let var = |mut v: usize, d: &mut Vec<u8>| loop {
            let c = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                d.push(c);
                break;
            }
            d.push(c | 0x80);
        };
        var(base_len, &mut d);
        var(n as usize + ins.len(), &mut d);
        d.push(0x80 | 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20);
        d.extend_from_slice(&a.to_le_bytes());
        d.extend_from_slice(&n.to_le_bytes()[..2]);
        if !ins.is_empty() {
            d.push(ins.len() as u8);
            d.extend_from_slice(ins);
        }
        d
    }

    #[test]
    fn packs_with_deltas_open_and_every_id_is_recomputed() {
        let s = Scratch::new("gitpack");
        let base = b"hello, world of icons\n".to_vec();
        let d1 = delta(base.len(), 0, 5, b" there");
        let built = b"hello there".to_vec();
        let d2 = delta(built.len(), 6, 5, b"!");
        let p = pack(&[Raw::Whole(OBJ_BLOB, base.clone()), Raw::Ofs(1, d1.clone()), Raw::Ref(object_id(OBJ_BLOB, &built), d2), Raw::Whole(OBJ_TREE, vec![])]);
        let pk = open(&s, "a.pack", &p, &lim()).unwrap();
        assert_eq!(pk.len(), 4);
        assert_eq!(pk.get(&object_id(OBJ_BLOB, &base)).unwrap().unwrap(), (OBJ_BLOB, base.clone()));
        assert_eq!(pk.get(&object_id(OBJ_BLOB, b"hello there")).unwrap().unwrap().1, b"hello there");
        assert_eq!(pk.get(&object_id(OBJ_BLOB, b"there!")).unwrap().unwrap().1, b"there!", "a REF_DELTA on a delta");
        assert!(pk.get(&[7; 20]).unwrap().is_none());
        // a REF_DELTA before its base is resolved in a later round
        let q = pack(&[Raw::Ref(object_id(OBJ_BLOB, &base), d1), Raw::Whole(OBJ_BLOB, base.clone())]);
        assert_eq!(open(&s, "b.pack", &q, &lim()).unwrap().get(&object_id(OBJ_BLOB, b"hello there")).unwrap().unwrap().1, b"hello there");
    }

    #[test]
    fn hostile_packs_are_refused() {
        let s = Scratch::new("gitpack-bad");
        let l = lim();
        let blob = Raw::Whole(OBJ_BLOB, b"x".to_vec());
        let mut ok = pack(std::slice::from_ref(&blob));
        let check = |name: &str, bytes: &[u8], want: &str| {
            let e = open(&s, name, bytes, &l).err().unwrap_or_else(|| panic!("{name} opened"));
            assert!(e.contains(want), "{name}: {e}");
        };
        // checksum, truncation, version, count, trailing data
        let n = ok.len();
        ok[n - 1] ^= 1;
        check("sum", &ok, "checksum");
        check("short", b"PACK", "too short");
        let mut v3 = pack(std::slice::from_ref(&blob));
        v3[7] = 3;
        let mut h = Sha1::default();
        h.update(&v3[..v3.len() - 20]);
        let t = h.finish();
        let k = v3.len();
        v3[k - 20..].copy_from_slice(&t);
        check("v3", &v3, "version 2");
        let resum = |mut b: Vec<u8>| {
            let k = b.len();
            let mut h = Sha1::default();
            h.update(&b[..k - 20]);
            let t = h.finish();
            b[k - 20..].copy_from_slice(&t);
            b
        };
        let mut more = pack(std::slice::from_ref(&blob));
        more[11] = 2;
        check("count", &resum(more), "fewer objects");
        let mut trail = pack(std::slice::from_ref(&blob));
        trail.splice(trail.len() - 20..trail.len() - 20, [0u8; 3]);
        check("trailing", &resum(trail), "between the last object");
        // types, sizes, bombs
        check("type5", &pack(&[Raw::Bytes(vec![0x51, 0x78, 0x9c, 0x03, 0x00])]), "unknown object type 5");
        check("huge", &pack(&[Raw::Bytes([obj_header(OBJ_BLOB, 2 << 20), zlib(b"x")].concat())]), "limit is 1024 KiB");
        check("longer", &pack(&[Raw::Bytes([obj_header(OBJ_BLOB, 1), zlib(b"xyz")].concat())]), "more than its header");
        check("shorter", &pack(&[Raw::Bytes([obj_header(OBJ_BLOB, 9), zlib(b"xyz")].concat())]), "less than its header");
        check("corrupt", &pack(&[Raw::Bytes([obj_header(OBJ_BLOB, 3), vec![0x78, 0x9c, 0xff, 0xff, 0xff]].concat())]), "corrupt");
        let zeros = vec![0u8; 900 << 10];
        let many: Vec<Raw> = (0..20).map(|i| Raw::Whole(OBJ_BLOB, [zeros.clone(), vec![i]].concat())).collect();
        let e = Pack::open(&s.write("bomb.pack", &pack(&many)), &Limits { max_stream: 4 << 20, ..l }).err().unwrap();
        assert!(e.contains("work"), "{e}");
        check("dup", &pack(&[blob.clone(), blob.clone()]), "appears twice");
        let wide: Vec<Raw> = (0..1001u32).map(|i| Raw::Whole(OBJ_BLOB, i.to_le_bytes().to_vec())).collect();
        check("wide", &pack(&wide), "limit is 1000");
        // deltas: thin, out of range, reserved, wrong base size, deep chains
        check("thin", &pack(&[Raw::Ref([9; 20], delta(1, 0, 1, b""))]), "thin pack");
        check("copy", &pack(&[blob.clone(), Raw::Ofs(1, delta(1, 0, 5, b""))]), "outside its base");
        check("reserved", &pack(&[blob.clone(), Raw::Ofs(1, vec![1, 1, 0])]), "reserved");
        check("basesize", &pack(&[blob.clone(), Raw::Ofs(1, delta(4, 0, 1, b""))]), "another size");
        let mut deep = vec![blob.clone()];
        for i in 0..=MAX_CHAIN {
            let len = 1 + i;
            deep.push(Raw::Ofs(1, delta(len, 0, len as u32, b"y")));
        }
        check("deep", &pack(&deep), "longer than 64");
        let mut ofs_far = pack(std::slice::from_ref(&blob));
        ofs_far.splice(ofs_far.len() - 20..ofs_far.len() - 20, [obj_header(OBJ_OFS_DELTA, 3), vec![0x7f], zlib(&delta(1, 0, 1, b""))].concat());
        ofs_far[11] = 2;
        check("ofs-outside", &resum(ofs_far), "outside the pack");
        let mut ofs_mid = pack(std::slice::from_ref(&blob));
        let at = ofs_mid.len() - 20;
        ofs_mid.splice(at..at, [obj_header(OBJ_OFS_DELTA, 3), vec![(at - 13) as u8], zlib(&delta(1, 0, 1, b""))].concat());
        ofs_mid[11] = 2;
        check("ofs-middle", &resum(ofs_mid), "not the start of an object");
    }

    #[test]
    fn trees_must_be_canonical_and_plain() {
        let e = |m: &str, n: &str| [format!("{m} {n}\0").into_bytes(), vec![1; 20]].concat();
        assert_eq!(parse_tree(&[e("100644", "a"), e("40000", "b")].concat()).unwrap().len(), 2);
        let bad = |d: Vec<u8>, want: &str| {
            let err = parse_tree(&d).unwrap_err();
            assert!(err.contains(want), "{err}");
        };
        bad(e("040000", "a"), "not one git writes");
        bad(e("100664", "a"), "not one git writes");
        bad(e("100644", ".."), "name");
        bad(e("100644", "a\x01b"), "name");
        bad(e("100644", "a\\b"), "name");
        bad([e("100644", "b"), e("100644", "a")].concat(), "order");
        bad([e("100644", "a"), e("100644", "a")].concat(), "order");
        bad([e("100644", "a"), e("100644", "a.b"), e("40000", "a")].concat(), "repeat");
        // git's order: the folder `a` sorts as `a/`, after the file `a.b`
        assert!(parse_tree(&[e("100644", "a.b"), e("40000", "a")].concat()).is_ok());
        bad([e("40000", "a"), e("100644", "a.b")].concat(), "order");
        bad(e("100644", "a")[..10].to_vec(), "runs past");
        assert_eq!(parse_tree(&e("160000", "sub")).unwrap()[0].mode, Mode::Gitlink);
        assert!(commit_tree(b"tree 0123\n").is_err());
    }

    #[test]
    fn overflowing_size_encodings_are_refused_before_truncation() {
        let mut delta = vec![0x80; 9];
        delta.push(2);
        assert!(delta_size(&delta, &mut 0).unwrap_err().contains("does not fit"));
        delta[9] = 1;
        assert_eq!(delta_size(&delta, &mut 0).unwrap(), 1u64 << 63);
        let s = Scratch::new("gitpack-size-overflow");
        let mut p = b"PACK\0\0\0\x02\0\0\0\x01".to_vec();
        p.push(0xb0); // blob, continued low four size bits
        p.extend_from_slice(&[0x80; 8]);
        p.push(0x10); // shift 60: this bit would truncate to zero
        p.extend_from_slice(&zlib(b""));
        let mut h = Sha1::default();
        h.update(&p);
        p.extend_from_slice(&h.finish());
        let error = open(&s, "overflow.pack", &p, &lim()).err().expect("oversized header was accepted");
        assert!(error.contains("does not fit"), "{error}");
    }

    #[test]
    fn responses_are_demultiplexed_strictly() {
        let s = Scratch::new("gitpack-demux");
        let p = pack(&[Raw::Whole(OBJ_BLOB, b"x".to_vec())]);
        let n = demux(&response(&p, Some(&"a".repeat(40)))[..], &s.path().join("ok")).unwrap();
        assert_eq!(n, p.len() as u64);
        let bad = |name: &str, b: Vec<u8>, want: &str| {
            let e = demux(&b[..], &s.path().join(name)).unwrap_err();
            assert!(e.contains(want), "{name}: {e}");
        };
        bad("err", [pkt("ERR upload-pack: not our ref 0123\n"), b"0000".to_vec()].concat(), "the server refused: upload-pack: not our ref");
        bad("band3", [pkt("packfile\n"), b"0009\x03boom".to_vec(), b"0000".to_vec()].concat(), "the server failed: boom");
        bad("noband", [pkt("packfile\n"), b"0009\x07boom".to_vec()].concat(), "side band");
        bad("long", [pkt("packfile\n"), b"fff1".to_vec()].concat(), "limit is 65520");
        bad("cut", [pkt("packfile\n"), b"0009\x01ab".to_vec()].concat(), "ends inside a pkt-line");
        bad("open", pkt("packfile\n"), "ended inside the pack");
        bad("trail", [response(&p, None), b"x".to_vec()].concat(), "ends inside a pkt-line length");
        bad("after", [response(&p, None), pkt("more\n")].concat(), "after the end");
        bad("nopack", b"0000".to_vec(), "no pack");
        bad("uris", [pkt("packfile-uris\n"), b"0000".to_vec()].concat(), "unexpected section");
        bad("hex", b"zzzz".to_vec(), "not pkt-lines");
        let progress: Vec<u8> = [pkt("packfile\n"), (0..20).flat_map(|_| [b"fff0\x02".to_vec(), vec![b'.'; 0xfff0 - 5]].concat()).collect(), b"0000".to_vec()].concat();
        bad("progress", progress, "progress");
        // the cap holds for the response as it streams
        let big = response(&p, None);
        let mut c = Capped { inner: &big[..], cap: 10, n: 0, sha: Sha256::default() };
        assert!(demux(&mut c, &s.path().join("cap")).unwrap_err().contains("larger than"));
    }

    #[test]
    fn capabilities_must_offer_v2_fetch_with_shallow_and_filter() {
        assert!(check_caps(CAPS).is_ok());
        let adv = |lines: &[&str]| [pkt("# service=git-upload-pack\n"), b"0000".to_vec(), lines.iter().flat_map(|l| pkt(&format!("{l}\n"))).collect(), b"0000".to_vec()].concat();
        assert!(check_caps(&adv(&["version 2", "fetch=shallow filter"])).is_ok());
        assert!(check_caps(&adv(&["0123456789012345678901234567890123456789 HEAD\0multi_ack"])).unwrap_err().contains("version 2"));
        assert!(check_caps(b"<html>").unwrap_err().contains("smart"));
        assert!(check_caps(&adv(&["version 2", "fetch=shallow"])).unwrap_err().contains("filter"));
        assert!(check_caps(&adv(&["version 2", "ls-refs"])).unwrap_err().contains("no `fetch`"));
        assert!(check_caps(&adv(&["version 2", "fetch=shallow filter", "object-format=sha256"])).unwrap_err().contains("sha256"));
    }

    fn fixture() -> (Vec<E>, Repo) {
        let v = vec![
            E::file("top/LICENSE", b"licence text\n"),
            E::dir("top/theme/"),
            E::file("top/theme/index.theme", b"[Icon Theme]\nName=T\nDirectories=a\n[a]\nSize=16\n"),
            E::file("top/theme/a/x.png", b"png"),
            E::link("top/theme/a/y.png", "x.png"),
            E::file("top/other/big.bin", &[0u8; 4096]),
        ];
        let r = Repo::from_entries(&v);
        (v, r)
    }

    fn fresh(s: &Scratch, d: &str) -> std::path::PathBuf {
        let p = s.path().join(d);
        std::fs::create_dir(&p).unwrap();
        p
    }

    fn plan(v: &[E], r: &Repo) -> Plan {
        Plan {
            commit: r.commit.clone(),
            trees: vec![("theme".into(), crate::desktop_assets::testkit::tree_of(v, "top/theme"))],
            blobs: vec![("LICENSE".into(), gitoid::hex(&gitoid::blob_oid(b"licence text\n")))],
        }
    }

    #[test]
    fn a_fetch_takes_three_requests_and_binds_every_pin_to_the_commit() {
        let s = Scratch::new("gitfetch");
        let (v, r) = fixture();
        let f = Fake::new(r.clone());
        let got = fetch(&f, "https://example.org/x.git", &plan(&v, &r), s.path(), &lim()).unwrap();
        assert_eq!(f.requests.borrow().len(), 3);
        assert_eq!(got.exchanges.len(), 3);
        assert!(got.chain);
        assert!(!got.pack.contains(&gitoid::blob_oid(&[0u8; 4096])), "only the pinned objects, not the rest of the repository");
        assert!(got.pack.contains(&gitoid::blob_oid(b"png")));
        // a pin the commit does not have: refused after the commit's trees, before any content
        let mut p = plan(&v, &r);
        p.trees[0].1 = "1".repeat(40);
        let f2 = Fake::new(r.clone());
        let e = fetch(&f2, "https://example.org/x.git", &p, &fresh(&s, "x"), &lim()).err().unwrap();
        assert!(e.contains("the pin is tree 1111"), "{e}");
        assert_eq!(f2.requests.borrow().len(), 2, "no content was requested");
        let mut p = plan(&v, &r);
        p.blobs[0].1 = "2".repeat(40);
        assert!(fetch(&Fake::new(r.clone()), "u", &p, &fresh(&s, "y"), &lim()).err().unwrap().contains("the pin is blob 2222"));
        let mut p = plan(&v, &r);
        p.trees[0].0 = "nothere".into();
        assert!(fetch(&Fake::new(r.clone()), "u", &p, &fresh(&s, "z"), &lim()).err().unwrap().contains("does not exist in the pinned commit"));
        // an unknown commit: the server's own refusal is reported
        let mut p = plan(&v, &r);
        p.commit = "3".repeat(40);
        assert!(fetch(&Fake::new(r.clone()), "u", &p, &fresh(&s, "w"), &lim()).err().unwrap().contains("not our ref"));
        // a server that answers the content request with other objects: refused by id
        let mut f3 = Fake::new(r.clone());
        f3.tamper = Some((1, response(&pack(&[Raw::Whole(OBJ_BLOB, b"other".to_vec())]), None)));
        assert!(fetch(&f3, "u", &plan(&v, &r), &fresh(&s, "v"), &lim()).err().unwrap().contains("did not send object"));
        let mut f4 = Fake::new(r);
        f4.caps = [pkt("# service=git-upload-pack\n"), b"0000".to_vec(), pkt("version 2\n"), pkt("fetch=shallow\n"), b"0000".to_vec()].concat();
        assert!(fetch(&f4, "u", &plan(&v, &f4.repo), &fresh(&s, "u"), &lim()).err().unwrap().contains("no `filter`"));
    }
}
