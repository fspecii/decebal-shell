//! A streaming reader for `.tar` and `.tar.gz` sources of asset packs (hostile input).
//!
//! Unlike the `.dtheme` reader it never holds the archive in memory: headers are parsed one by one and the content of
//! an entry is handed to a callback or skipped. It understands ustar, pax `x` (path, linkpath, size) and `g` (the
//! `comment` git puts its commit id in) records and GNU long names (`L`, `K`). Every header checksum is verified; the
//! decompressed stream is capped while it is read. Devices, FIFOs, sparse files, multi-volume parts and base-256
//! numbers are refused. Entry names are validated with [`super::safe_rel`] before anyone sees them.

use flate2::read::GzDecoder;
use std::io::Read;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// The link's target text, unvalidated (the source decides what it may point at).
    Symlink(String),
    /// The path of an earlier entry.
    Hardlink(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Validated, without a leading `./` or trailing `/`.
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub mode: u32,
}

/// What the first bytes of a file say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Tar,
    TarGz,
}

/// Recognises the container by its magic bytes; formats there is no decoder for get an explanation.
pub fn sniff(head: &[u8]) -> Result<Format, String> {
    if head.starts_with(&[0x1f, 0x8b]) {
        return Ok(Format::TarGz);
    }
    if head.len() >= 262 && &head[257..262] == b"ustar" {
        return Ok(Format::Tar);
    }
    let what = if head.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
        "an xz archive"
    } else if head.starts_with(b"PK\x03\x04") {
        "a zip archive"
    } else if head.starts_with(b"BZh") {
        "a bzip2 archive"
    } else if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        "a zstd archive"
    } else if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
        "a 7z archive"
    } else {
        return Err("not a tar or tar.gz archive".into());
    };
    Err(format!("{what} is not supported: unpack it yourself and import the folder, or use the .tar.gz download"))
}

/// Counts and caps the bytes read from the (decompressed) stream.
struct Capped<R> {
    inner: R,
    read: u64,
    cap: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.read >= self.cap {
            return Err(std::io::Error::other(format!("archive expands past {} MiB (decompression bomb?)", self.cap >> 20)));
        }
        let max = buf.len().min((self.cap - self.read).min(usize::MAX as u64) as usize);
        let n = self.inner.read(&mut buf[..max])?;
        self.read += n as u64;
        Ok(n)
    }
}

pub struct Reader<'a> {
    r: Capped<Box<dyn Read + 'a>>,
    /// Content bytes of the current entry not consumed yet, and its padding.
    left: u64,
    pad: u64,
    max_entries: usize,
    entries: usize,
    /// `comment` of a pax global header (git archive: the commit id).
    pub global_comment: Option<String>,
    done: bool,
}

fn cstr(b: &[u8]) -> Result<String, String> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8(b[..end].to_vec()).map_err(|_| "an entry name is not UTF-8".to_string())
}

fn octal(b: &[u8]) -> Result<u64, String> {
    if b.first().is_some_and(|c| c & 0x80 != 0) {
        return Err("base-256 numbers in tar headers are not supported".into());
    }
    let s = cstr(b)?;
    let s = s.trim_matches([' ', '\0']);
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| format!("bad number `{s}` in a tar header"))
}

/// `"%d key=value\n"` records of a pax header.
fn pax_records(data: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let sp = rest.iter().position(|&c| c == b' ').ok_or("corrupt pax header")?;
        let len: usize = std::str::from_utf8(&rest[..sp]).ok().and_then(|s| s.parse().ok()).ok_or("corrupt pax header length")?;
        if len <= sp + 1 || len > rest.len() || rest[len - 1] != b'\n' {
            return Err("corrupt pax header record".into());
        }
        let rec = std::str::from_utf8(&rest[sp + 1..len - 1]).map_err(|_| "pax header is not UTF-8")?;
        let (k, v) = rec.split_once('=').ok_or("corrupt pax header record")?;
        out.push((k.to_string(), v.to_string()));
        rest = &rest[len..];
    }
    Ok(out)
}

/// Metadata entries (pax, GNU long names) are small; anything bigger is refused.
const MAX_META: u64 = 64 << 10;

impl<'a> Reader<'a> {
    /// A reader over `r`, which may be gzip-compressed (`fmt`). `max_stream` caps the decompressed bytes.
    pub fn new(r: impl Read + 'a, fmt: Format, max_stream: u64, max_entries: usize) -> Reader<'a> {
        let inner: Box<dyn Read + 'a> = match fmt {
            Format::Tar => Box::new(r),
            Format::TarGz => Box::new(GzDecoder::new(r)),
        };
        Reader { r: Capped { inner, read: 0, cap: max_stream }, left: 0, pad: 0, max_entries, entries: 0, global_comment: None, done: false }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), String> {
        self.r.read_exact(buf).map_err(|e| if e.kind() == std::io::ErrorKind::UnexpectedEof { "corrupt archive: truncated".to_string() } else { format!("corrupt archive: {e}") })
    }

    fn skip(&mut self, mut n: u64) -> Result<(), String> {
        let mut buf = [0u8; 8192];
        while n > 0 {
            let k = n.min(buf.len() as u64) as usize;
            self.read_exact(&mut buf[..k])?;
            n -= k as u64;
        }
        Ok(())
    }

    fn finish_entry(&mut self) -> Result<(), String> {
        let n = self.left + self.pad;
        self.left = 0;
        self.pad = 0;
        self.skip(n)
    }

    fn meta(&mut self, size: u64) -> Result<Vec<u8>, String> {
        if size > MAX_META {
            return Err("an extended tar header is too large".into());
        }
        let mut v = vec![0u8; size as usize];
        self.read_exact(&mut v)?;
        self.skip(size.div_ceil(512) * 512 - size)?;
        Ok(v)
    }

    /// The next entry; its content is consumed by [`Reader::content`] or skipped by the next call of [`Reader::next_entry`].
    pub fn next_entry(&mut self) -> Result<Option<Header>, String> {
        if self.done {
            return Ok(None);
        }
        self.finish_entry()?;
        let (mut long_name, mut long_link, mut pax): (Option<String>, Option<String>, Vec<(String, String)>) = (None, None, Vec::new());
        loop {
            let mut h = [0u8; 512];
            match self.r.read(&mut h[..1]) {
                Ok(0) => {
                    self.done = true;
                    return Ok(None); // end of stream without the two zero blocks: accepted (many writers omit them)
                }
                Ok(_) => self.read_exact(&mut h[1..])?,
                Err(e) => return Err(format!("corrupt archive: {e}")),
            }
            if h.iter().all(|&b| b == 0) {
                self.done = true;
                return Ok(None);
            }
            let stored = octal(&h[148..156])?;
            let sum: u64 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { u64::from(b) }).sum();
            if sum != stored {
                return Err("corrupt archive: tar header checksum mismatch".into());
            }
            let mut size = octal(&h[124..136])?;
            let mode = octal(&h[100..108])? as u32;
            let flag = h[156];
            match flag {
                b'x' => {
                    let d = self.meta(size)?;
                    pax = pax_records(&d)?;
                    continue;
                }
                b'g' => {
                    let d = self.meta(size)?;
                    if let Some((_, c)) = pax_records(&d)?.into_iter().find(|(k, _)| k == "comment") {
                        self.global_comment = Some(c.trim().to_string());
                    }
                    continue;
                }
                b'L' => {
                    let d = self.meta(size)?;
                    long_name = Some(cstr(&d)?);
                    continue;
                }
                b'K' => {
                    let d = self.meta(size)?;
                    long_link = Some(cstr(&d)?);
                    continue;
                }
                _ => {}
            }
            self.entries += 1;
            if self.entries > self.max_entries {
                return Err(format!("more than {} entries", self.max_entries));
            }
            let mut name = match long_name.take() {
                Some(n) => n,
                None => {
                    let base = cstr(&h[..100])?;
                    let prefix = if &h[257..262] == b"ustar" { cstr(&h[345..500])? } else { String::new() };
                    if prefix.is_empty() {
                        base
                    } else {
                        format!("{prefix}/{base}")
                    }
                }
            };
            let mut link = long_link.take().unwrap_or(cstr(&h[157..257])?);
            for (k, v) in pax.drain(..) {
                match k.as_str() {
                    "path" => name = v,
                    "linkpath" => link = v,
                    "size" => size = v.parse().map_err(|_| "bad pax size")?,
                    _ => {}
                }
            }
            let shown = name.escape_debug().to_string();
            let mut path = name.as_str();
            while let Some(p) = path.strip_prefix("./") {
                path = p;
            }
            let path = path.trim_end_matches('/');
            let kind = match flag {
                b'0' | 0 | b'7' => Kind::File,
                b'5' => Kind::Dir,
                b'2' => Kind::Symlink(link),
                b'1' => Kind::Hardlink(link.trim_start_matches("./").to_string()),
                b'3' | b'4' => return Err(format!("archive entry rejected: `{shown}` is a device")),
                b'6' => return Err(format!("archive entry rejected: `{shown}` is a FIFO")),
                b'S' => return Err(format!("archive entry rejected: `{shown}` is a sparse file")),
                other => return Err(format!("archive entry rejected: `{shown}` has the unsupported type `{}`", other as char)),
            };
            if path.is_empty() || path == "." {
                if kind == Kind::Dir {
                    continue; // the archive's own root
                }
                return Err(format!("archive entry rejected: `{shown}` has no name"));
            }
            super::safe_rel(path).map_err(|e| format!("archive entry rejected: {e}"))?;
            // a link or folder that claims content: the bytes are skipped, never parsed as headers
            self.left = size;
            self.pad = size.div_ceil(512) * 512 - size;
            return Ok(Some(Header { path: path.to_string(), kind, size, mode }));
        }
    }

    /// Streams the content of the current entry to `sink` in pieces.
    pub fn content(&mut self, sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<(), String> {
        let mut buf = vec![0u8; 64 << 10];
        while self.left > 0 {
            let k = self.left.min(buf.len() as u64) as usize;
            self.read_exact(&mut buf[..k])?;
            self.left -= k as u64;
            sink(&buf[..k])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{TarBuilder, E};
    use super::*;

    fn all(bytes: &[u8]) -> Result<Vec<(Header, Vec<u8>)>, String> {
        let mut r = Reader::new(bytes, sniff(bytes)?, 1 << 30, 1000);
        let mut out = Vec::new();
        while let Some(h) = r.next_entry()? {
            let mut d = Vec::new();
            r.content(&mut |b| {
                d.extend_from_slice(b);
                Ok(())
            })?;
            out.push((h, d));
        }
        Ok(out)
    }

    #[test]
    fn reads_files_dirs_links_long_names_and_the_git_comment() {
        let long = format!("top/{}/icon.svg", "d".repeat(150));
        let tgz = TarBuilder::new()
            .global_comment("5d11ba4e4013a4d1e1c601265d55c66b95184922")
            .push(E::dir("top/"))
            .push(E::file("top/a.txt", b"hello"))
            .push(E::file(&long, b"<svg/>"))
            .push(E::link("top/l", "a.txt"))
            .push(E::hard("top/h", "top/a.txt"))
            .gnu_long(E::file(&format!("top/{}", "g".repeat(120)), b"x"))
            .gz();
        let mut r = Reader::new(&tgz[..], sniff(&tgz).unwrap(), 1 << 30, 100);
        let mut seen = Vec::new();
        while let Some(h) = r.next_entry().unwrap() {
            seen.push(h); // content skipped by the next call
        }
        assert_eq!(r.global_comment.as_deref(), Some("5d11ba4e4013a4d1e1c601265d55c66b95184922"));
        let paths: Vec<&str> = seen.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, ["top", "top/a.txt", long.as_str(), "top/l", "top/h", &format!("top/{}", "g".repeat(120))]);
        assert_eq!(seen[3].kind, Kind::Symlink("a.txt".into()));
        assert_eq!(seen[4].kind, Kind::Hardlink("top/a.txt".into()));
        let got = all(&tgz).unwrap();
        assert_eq!(got[1].1, b"hello");
        assert_eq!(got[2].1, b"<svg/>");
        let plain = TarBuilder::new().push(E::file("x", b"1")).tar();
        assert_eq!(sniff(&plain).unwrap(), Format::Tar);
        assert_eq!(all(&plain).unwrap()[0].1, b"1");
    }

    #[test]
    fn hostile_headers_are_refused() {
        for (what, b) in [
            ("absolute", TarBuilder::new().push(E::file("/etc/passwd", b"x")).gz()),
            ("dotdot", TarBuilder::new().push(E::file("a/../../x", b"x")).gz()),
            ("pax traversal", TarBuilder::new().pax_path(E::file("ok", b"x"), "../../evil").gz()),
            ("device", TarBuilder::new().push(E::raw("dev", b'3')).gz()),
            ("fifo", TarBuilder::new().push(E::raw("fifo", b'6')).gz()),
            ("sparse", TarBuilder::new().push(E::raw("sp", b'S')).gz()),
            ("control", TarBuilder::new().push(E::file("a\nb", b"x")).gz()),
        ] {
            assert!(all(&b).is_err(), "{what} accepted");
        }
        let mut bad = TarBuilder::new().push(E::file("a", b"x")).tar();
        bad[0] = b'b'; // checksum no longer matches
        assert!(all(&bad).unwrap_err().contains("checksum"));
        let trunc = TarBuilder::new().push(E::file("a", &[7u8; 2000])).tar();
        assert!(all(&trunc[..1200]).unwrap_err().contains("truncated"));
        assert!(sniff(&[0xfd, b'7', b'z', b'X', b'Z', 0, 0]).unwrap_err().contains("xz"));
        assert!(sniff(b"PK\x03\x04....").unwrap_err().contains("zip"));
        assert!(sniff(b"hello").is_err());
    }

    #[test]
    fn stream_and_entry_caps_hold() {
        let big = TarBuilder::new().push(E::file("z", &vec![0u8; 4 << 20])).gz();
        assert!(big.len() < 64 << 10, "zeros compress well: a small bomb");
        let mut r = Reader::new(&big[..], Format::TarGz, 1 << 20, 10);
        let h = r.next_entry().unwrap().unwrap();
        assert_eq!(h.size, 4 << 20);
        let e = r.content(&mut |_| Ok(())).unwrap_err();
        assert!(e.contains("decompression bomb"), "{e}");
        let many = (0..20).fold(TarBuilder::new(), |b, i| b.push(E::file(&format!("f{i}"), b"x"))).gz();
        let mut r = Reader::new(&many[..], Format::TarGz, 1 << 20, 10);
        let mut err = None;
        for _ in 0..30 {
            match r.next_entry() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        assert!(err.unwrap().contains("more than 10 entries"));
    }
}
