//! `.dtheme` archives: a gzip-compressed ustar tarball with `theme.toml` at its root.
//!
//! The reader is a hostile-input reader. It only ever produces in-memory `(path, bytes)` pairs, never touches the
//! file system, and refuses everything it does not need: links, devices, pax/GNU extensions, absolute and `..` paths,
//! duplicate names, too many entries, oversized entries and decompression bombs (the decompressed stream is capped
//! while it is read, not after).

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Hard caps for a theme package (an archive, or a directory about to be packed or installed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_files: usize,
    /// Largest single file.
    pub max_file: u64,
    /// Sum of all files.
    pub max_total: u64,
    /// The `.dtheme` file itself.
    pub max_archive: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_files: 512, max_file: 32 << 20, max_total: 128 << 20, max_archive: 128 << 20 }
    }
}

/// One regular file of an archive; `path` is relative, `/`-separated and already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
}

/// A path inside a package: relative, `/`-separated, no empty / `.` / `..` components, no backslashes or control characters,
/// at most 160 bytes. Returns the path unchanged.
pub fn safe_rel_path(p: &str) -> Result<&str, String> {
    if p.is_empty() {
        return Err("empty path".into());
    }
    if p.len() > 160 {
        return Err(format!("path `{p}` is too long"));
    }
    if p.starts_with('/') {
        return Err(format!("`{p}` is an absolute path"));
    }
    if p.contains('\\') || p.chars().any(|c| c.is_control()) {
        return Err(format!("`{p}` contains a backslash or control character"));
    }
    for part in p.split('/') {
        match part {
            "" => return Err(format!("`{p}` has an empty path component")),
            "." | ".." => return Err(format!("`{p}` escapes the package (`{part}`)")),
            _ => {}
        }
    }
    Ok(p)
}

// ---------------------------------------------------------------- writing

fn octal(buf: &mut [u8], v: u64) {
    let s = format!("{:0width$o}", v, width = buf.len() - 1);
    buf[..s.len()].copy_from_slice(s.as_bytes());
    buf[buf.len() - 1] = 0;
}

fn header(name: &str, size: u64) -> Result<[u8; 512], String> {
    let mut h = [0u8; 512];
    let (prefix, base) = if name.len() <= 100 {
        ("", name)
    } else {
        // split at a '/' so that the name fits 100 and the prefix 155
        let cut = name.match_indices('/').map(|(i, _)| i).find(|&i| name.len() - i - 1 <= 100 && i <= 155).ok_or_else(|| format!("`{name}` is too long for a tar header"))?;
        (&name[..cut], &name[cut + 1..])
    };
    h[..base.len()].copy_from_slice(base.as_bytes());
    octal(&mut h[100..108], 0o644);
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], 0); // mtime 0: the same input always packs to the same bytes
    h[148..156].fill(b' ');
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

/// Packs `entries` (sorted by the caller for reproducibility) into a `.dtheme`.
pub fn write_targz(entries: &[Entry]) -> Result<Vec<u8>, String> {
    let mut tar = Vec::new();
    for e in entries {
        safe_rel_path(&e.path)?;
        tar.extend_from_slice(&header(&e.path, e.data.len() as u64)?);
        tar.extend_from_slice(&e.data);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    tar.extend_from_slice(&[0u8; 1024]);
    let mut gz = GzEncoder::new(Vec::new(), Compression::new(9));
    gz.write_all(&tar).map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- reading

fn cstr(b: &[u8]) -> Result<&str, String> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    std::str::from_utf8(&b[..end]).map_err(|_| "a name is not UTF-8".to_string())
}

fn parse_octal(b: &[u8]) -> Result<u64, String> {
    if b.first().is_some_and(|c| c & 0x80 != 0) {
        return Err("base-256 sizes are not supported".into());
    }
    let s = cstr(b)?.trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| format!("bad number `{s}` in a tar header"))
}

/// Reads a `.dtheme` from memory. Every entry path is validated with [`safe_rel_path`]; a single wrapping directory
/// (`mytheme/theme.toml`) is stripped so both `tar czf x.dtheme .` and `tar czf x.dtheme mytheme` work.
pub fn read_targz(bytes: &[u8], lim: &Limits) -> Result<Vec<Entry>, String> {
    if bytes.len() as u64 > lim.max_archive {
        return Err(format!("archive is {} MiB, the limit is {} MiB", bytes.len() >> 20, lim.max_archive >> 20));
    }
    if bytes.len() < 2 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Err("not a .dtheme archive (a gzip-compressed tar file)".into());
    }
    // headers and padding add a little to the payload cap
    let cap = lim.max_total + (lim.max_files as u64 + 2) * 1024 + 1024;
    let mut tar = Vec::new();
    GzDecoder::new(bytes).take(cap + 1).read_to_end(&mut tar).map_err(|e| format!("corrupt archive: {e}"))?;
    if tar.len() as u64 > cap {
        return Err(format!("archive expands past {} MiB (decompression bomb?)", lim.max_total >> 20));
    }
    let mut out: Vec<Entry> = Vec::new();
    let mut total = 0u64;
    let mut pos = 0usize;
    while pos + 512 <= tar.len() {
        let h = &tar[pos..pos + 512];
        if h.iter().all(|&b| b == 0) {
            break;
        }
        let stored = parse_octal(&h[148..156])?;
        let sum: u32 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { u32::from(b) }).sum();
        if u64::from(sum) != stored {
            return Err("corrupt archive: tar header checksum mismatch".into());
        }
        let size = parse_octal(&h[124..136])?;
        let name = {
            let base = cstr(&h[..100])?;
            let prefix = if &h[257..262] == b"ustar" { cstr(&h[345..500])? } else { "" };
            if prefix.is_empty() {
                base.to_string()
            } else {
                format!("{prefix}/{base}")
            }
        };
        let kind = h[156];
        pos += 512;
        match kind {
            b'0' | 0 => {
                let path = name.trim_start_matches("./").to_string();
                safe_rel_path(&path).map_err(|e| format!("archive entry rejected: {e}"))?;
                if size > lim.max_file {
                    return Err(format!("`{path}` is {} MiB, the per-file limit is {} MiB", size >> 20, lim.max_file >> 20));
                }
                total += size;
                if total > lim.max_total {
                    return Err(format!("package is larger than {} MiB", lim.max_total >> 20));
                }
                if out.len() >= lim.max_files {
                    return Err(format!("more than {} files", lim.max_files));
                }
                let end = pos.checked_add(size as usize).filter(|&e| e <= tar.len()).ok_or("corrupt archive: truncated file")?;
                if out.iter().any(|e| e.path == path) {
                    return Err(format!("archive entry rejected: `{path}` appears twice"));
                }
                out.push(Entry { path, data: tar[pos..end].to_vec() });
                pos = end.div_ceil(512) * 512;
            }
            b'5' => {
                // a directory: validate the name, nothing to create
                let d = name.trim_start_matches("./").trim_end_matches('/');
                if !d.is_empty() && d != "." {
                    safe_rel_path(d).map_err(|e| format!("archive entry rejected: {e}"))?;
                }
            }
            b'1' => return Err(format!("archive entry rejected: `{name}` is a hard link")),
            b'2' => return Err(format!("archive entry rejected: `{name}` is a symlink (links are not allowed in themes)")),
            b'3' | b'4' | b'6' | b'7' => return Err(format!("archive entry rejected: `{name}` is a device, fifo or special file")),
            b'x' | b'g' | b'L' | b'K' => return Err("archive uses tar extensions (pax / GNU long names); repack with `decthemectl theme pack`".into()),
            other => return Err(format!("archive entry rejected: `{name}` has unknown type {:?}", other as char)),
        }
    }
    strip_wrapper(&mut out);
    Ok(out)
}

fn strip_wrapper(entries: &mut [Entry]) {
    if entries.is_empty() || entries.iter().any(|e| e.path == "theme.toml") {
        return;
    }
    let first = entries[0].path.split('/').next().unwrap_or("").to_string();
    if entries.iter().all(|e| e.path.split_once('/').is_some_and(|(top, _)| top == first)) {
        for e in entries.iter_mut() {
            e.path = e.path[first.len() + 1..].to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(p: &str, d: &[u8]) -> Entry {
        Entry { path: p.into(), data: d.to_vec() }
    }

    #[test]
    fn round_trip_is_byte_stable() {
        let files = vec![e("theme.toml", b"format = 1\n"), e("wallpapers/a.png", &[1, 2, 3, 4, 5]), e("widgets/clock.toml", &vec![b'x'; 5000])];
        let a = write_targz(&files).unwrap();
        let b = write_targz(&files).unwrap();
        assert_eq!(a, b, "packing is reproducible");
        assert_eq!(read_targz(&a, &Limits::default()).unwrap(), files);
    }

    #[test]
    fn long_names_use_the_ustar_prefix() {
        let name = format!("{}/{}.png", "wallpapers/".repeat(6).trim_end_matches('/'), "x".repeat(60));
        assert!(name.len() > 100 && name.len() < 160);
        let a = write_targz(&[e("theme.toml", b"0"), e(&name, b"1")]).unwrap();
        assert_eq!(read_targz(&a, &Limits::default()).unwrap()[1].path, name);
    }

    #[test]
    fn path_rules() {
        for ok in ["theme.toml", "a/b/c.png", "w/Ünï.png"] {
            assert!(safe_rel_path(ok).is_ok(), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "a/../../x", "a//b", "./a", "a/./b", "a\\b", "a\nb", &"x".repeat(200)] {
            assert!(safe_rel_path(bad).is_err(), "{bad:?}");
        }
        assert!(write_targz(&[e("../evil", b"x")]).is_err());
    }

    /// Builds a raw tar entry by hand so that things the writer refuses can be tested.
    fn raw(name: &str, kind: u8, data: &[u8]) -> Vec<u8> {
        let mut h = header("placeholder", data.len() as u64).unwrap();
        h[..100].fill(0);
        h[..name.len().min(100)].copy_from_slice(&name.as_bytes()[..name.len().min(100)]);
        h[156] = kind;
        h[148..156].fill(b' ');
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut v = h.to_vec();
        v.extend_from_slice(data);
        v.resize(v.len().div_ceil(512) * 512, 0);
        v
    }

    fn gz(tar: &[u8]) -> Vec<u8> {
        let mut t = tar.to_vec();
        t.extend_from_slice(&[0u8; 1024]);
        let mut g = GzEncoder::new(Vec::new(), Compression::fast());
        g.write_all(&t).unwrap();
        g.finish().unwrap()
    }

    #[test]
    fn traversal_links_and_devices_are_refused() {
        let lim = Limits::default();
        let err = |tar: Vec<u8>| read_targz(&gz(&tar), &lim).unwrap_err();
        assert!(err(raw("../../etc/cron.d/x", b'0', b"boom")).contains("escapes the package"));
        assert!(err(raw("/abs/path", b'0', b"boom")).contains("absolute"));
        assert!(err(raw("a/../../b", b'0', b"boom")).contains("escapes"));
        assert!(err(raw("link", b'2', b"")).contains("symlink"));
        assert!(err(raw("hard", b'1', b"")).contains("hard link"));
        assert!(err(raw("dev", b'3', b"")).contains("device"));
        assert!(err(raw("pax", b'x', b"")).contains("extensions"));
        let mut twice = raw("theme.toml", b'0', b"a");
        twice.extend(raw("theme.toml", b'0', b"b"));
        assert!(err(twice).contains("twice"));
    }

    #[test]
    fn corrupt_and_foreign_input_is_refused() {
        let lim = Limits::default();
        assert!(read_targz(b"PK\x03\x04zip", &lim).unwrap_err().contains("not a .dtheme"));
        let good = write_targz(&[e("theme.toml", b"x")]).unwrap();
        assert!(read_targz(&good[..good.len() / 2], &lim).is_err());
        let mut bad = raw("theme.toml", b'0', b"abc");
        bad[10] ^= 0xff; // breaks the checksum
        assert!(read_targz(&gz(&bad), &lim).unwrap_err().contains("checksum"));
    }

    #[test]
    fn size_caps_apply_while_decompressing() {
        let lim = Limits { max_files: 3, max_file: 1000, max_total: 2000, max_archive: 1 << 20 };
        let big = write_targz(&[e("a", &vec![0u8; 1001])]).unwrap();
        assert!(read_targz(&big, &lim).unwrap_err().contains("per-file limit"));
        let many = write_targz(&(0..4).map(|i| e(&format!("f{i}"), b"x")).collect::<Vec<_>>()).unwrap();
        assert!(read_targz(&many, &lim).unwrap_err().contains("more than 3 files"));
        let total = write_targz(&[e("a", &[0u8; 900]), e("b", &[0u8; 900]), e("c", &[0u8; 900])]).unwrap();
        assert!(read_targz(&total, &lim).unwrap_err().contains("larger than"));
        // a bomb: a megabyte-scale tar of zeros compresses to a few hundred bytes but is refused by the stream cap
        let bomb = gz(&vec![b'A'; 8 << 20]);
        assert!(bomb.len() < 100_000);
        let small = Limits { max_files: 4, max_file: 1000, max_total: 1000, max_archive: 1 << 20 };
        assert!(read_targz(&bomb, &small).unwrap_err().contains("bomb"));
    }

    #[test]
    fn a_wrapping_directory_is_stripped() {
        let a = write_targz(&[e("mytheme/theme.toml", b"x"), e("mytheme/w/a.png", b"y")]).unwrap();
        let got = read_targz(&a, &Limits::default()).unwrap();
        assert_eq!(got.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["theme.toml", "w/a.png"]);
    }
}
