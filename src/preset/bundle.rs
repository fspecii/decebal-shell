//! The `.macshell-preset` bundle: a gzip-compressed ustar archive holding `manifest.toml` and a few fixed file names.
//!
//! Reading is deliberately strict because bundles come from other people. Nothing is ever extracted to disk here:
//! [`read_tar_gz`] returns the whitelisted entries in memory, or an error naming the first thing that is wrong. Rejected:
//! absolute paths, `..` and empty components, backslashes, symlinks and hard links, devices, unknown names, duplicate
//! names, oversized entries, too many entries, a decompressed size over the cap, bad header checksums.

use std::collections::BTreeMap;
use std::io::{Read, Write};

pub const MAX_BUNDLE: usize = 40 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 48 * 1024 * 1024;
pub const MAX_MANIFEST: u64 = 16 * 1024;
pub const MAX_TEXT: u64 = 1024 * 1024;
pub const MAX_IMAGE: u64 = 24 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 24;
pub const MAX_WALLPAPERS: usize = 4;
/// What fits on a clipboard line-wise; bigger bundles go to a file.
pub const MAX_CLIPBOARD: usize = 3 * 1024 * 1024;

pub const MANIFEST: &str = "manifest.toml";
pub const FIXED_FILES: [&str; 5] = [MANIFEST, "macshell.json", "dock-settings.json", "dock-pinned-macos.json", "rdwm.toml"];
pub const IMAGE_EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Manifest,
    Text,
    Theme,
    Wallpaper,
}

fn safe_component(c: &str) -> bool {
    !c.is_empty() && c.len() <= 80 && !c.starts_with('.') && c.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ' ' | '(' | ')')) && !c.ends_with(' ')
}

/// `themes/<id>.toml` -> `<id>`.
pub fn theme_id(name: &str) -> Option<&str> {
    name.strip_prefix("themes/")?.strip_suffix(".toml").filter(|id| safe_component(id) && !id.contains(' '))
}

/// Whether `name` is a file a preset may contain. Also rejects anything that is not a plain relative path.
pub fn check_name(name: &str) -> Result<NameKind, String> {
    if name.is_empty() {
        return Err("empty file name".into());
    }
    if name.starts_with('/') {
        return Err(format!("absolute path `{name}`"));
    }
    if name.contains('\\') || name.contains('\0') || name.chars().any(|c| c.is_control()) {
        return Err(format!("bad characters in file name `{}`", name.escape_default()));
    }
    for c in name.split('/') {
        if c == ".." {
            return Err(format!("path traversal in `{name}`"));
        }
        if c.is_empty() || c == "." {
            return Err(format!("odd path `{name}`"));
        }
    }
    if name == MANIFEST {
        return Ok(NameKind::Manifest);
    }
    if FIXED_FILES.contains(&name) {
        return Ok(NameKind::Text);
    }
    if theme_id(name).is_some() {
        return Ok(NameKind::Theme);
    }
    if let Some(f) = name.strip_prefix("wallpaper/") {
        let ext = f.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if safe_component(f) && ext.is_some_and(|e| IMAGE_EXTS.contains(&e.as_str())) {
            return Ok(NameKind::Wallpaper);
        }
    }
    Err(format!("unknown file `{name}` in the preset"))
}

pub fn size_cap(kind: NameKind) -> u64 {
    match kind {
        NameKind::Manifest => MAX_MANIFEST,
        NameKind::Text | NameKind::Theme => MAX_TEXT,
        NameKind::Wallpaper => MAX_IMAGE,
    }
}

/// Checks a file set (bundle entries or a preset folder): names, sizes, counts.
pub fn check_files(files: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    if files.len() > MAX_ENTRIES {
        return Err(format!("too many files ({})", files.len()));
    }
    let mut total = 0u64;
    let mut walls = 0;
    for (name, data) in files {
        let kind = check_name(name)?;
        if data.len() as u64 > size_cap(kind) {
            return Err(format!("`{name}` is too large ({} bytes)", data.len()));
        }
        if kind == NameKind::Wallpaper {
            walls += 1;
        }
        total += data.len() as u64;
    }
    if walls > MAX_WALLPAPERS {
        return Err("too many wallpapers".into());
    }
    if total > MAX_TOTAL {
        return Err("the preset is too large".into());
    }
    if !files.contains_key(MANIFEST) {
        return Err("manifest.toml is missing".into());
    }
    Ok(())
}

// ---------------------------------------------------------------- tar

fn octal(field: &mut [u8], v: u64) {
    let n = field.len();
    let s = format!("{v:0width$o}", width = n - 1);
    field[..n - 1].copy_from_slice(s.as_bytes());
    field[n - 1] = 0;
}

fn header(name: &str, size: u64) -> Result<[u8; 512], String> {
    if name.len() > 100 {
        return Err(format!("name too long: {name}"));
    }
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut h[100..108], 0o644);
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], 0);
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].fill(b' ');
    let sum: u32 = h.iter().map(|b| *b as u32).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

/// The bundle bytes (`manifest.toml` first, then the other names in order). Validates names and sizes.
pub fn write_tar_gz(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    check_files(files)?;
    let mut tar: Vec<u8> = Vec::new();
    let mut order: Vec<&String> = files.keys().collect();
    order.sort_by_key(|n| (n.as_str() != MANIFEST, n.as_str()));
    for name in order {
        let data = &files[name];
        tar.extend_from_slice(&header(name, data.len() as u64)?);
        tar.extend_from_slice(data);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    tar.extend_from_slice(&[0u8; 1024]);
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&tar).map_err(|e| e.to_string())?;
    enc.finish().map_err(|e| e.to_string())
}

fn cstr(field: &[u8]) -> Result<&str, String> {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    std::str::from_utf8(&field[..end]).map_err(|_| "file name is not UTF-8".to_string())
}

fn parse_octal(field: &[u8]) -> Result<u64, String> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return Err("binary (base-256) sizes are not accepted".into());
    }
    let s = cstr(field)?.trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| "bad number in archive header".to_string())
}

/// Reads a bundle into memory. See the module docs for what is rejected.
pub fn read_tar_gz(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if bytes.len() > MAX_BUNDLE {
        return Err("the file is too large to be a preset".into());
    }
    if bytes.len() < 18 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Err("not a macshell preset (not a gzip file)".into());
    }
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(bytes).take(MAX_TOTAL + 1024 * 1024).read_to_end(&mut tar).map_err(|e| format!("damaged archive: {e}"))?;
    if tar.len() as u64 > MAX_TOTAL + 512 * 1024 {
        return Err("the archive expands to more than the allowed size".into());
    }
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut pos = 0usize;
    let mut entries = 0usize;
    let mut total = 0u64;
    loop {
        if pos + 512 > tar.len() {
            break;
        }
        let h = &tar[pos..pos + 512];
        if h.iter().all(|b| *b == 0) {
            break;
        }
        // checksum: sum of all bytes with the checksum field read as spaces
        let want = parse_octal(&h[148..156])?;
        let got: u64 = h.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32u64 } else { *b as u64 }).sum();
        if want != got {
            return Err("damaged archive (header checksum)".into());
        }
        if &h[257..262] != b"ustar" {
            return Err("unsupported archive format (not ustar)".into());
        }
        let mut name = cstr(&h[..100])?.to_string();
        let prefix = cstr(&h[345..500])?;
        if !prefix.is_empty() {
            name = format!("{prefix}/{name}");
        }
        let size = parse_octal(&h[124..136])?;
        let flag = h[156];
        pos += 512;
        let padded = size.div_ceil(512) * 512;
        let end = pos.checked_add(padded as usize).ok_or("bad size")?;
        if end > tar.len() {
            return Err("damaged archive (truncated)".into());
        }
        entries += 1;
        if entries > MAX_ENTRIES + 4 {
            return Err("too many entries in the archive".into());
        }
        let clean = name.strip_prefix("./").unwrap_or(&name).trim_end_matches('/').to_string();
        match flag {
            b'0' | 0 => {
                let kind = check_name(&clean)?;
                if size > size_cap(kind) {
                    return Err(format!("`{clean}` is too large ({size} bytes)"));
                }
                total += size;
                if total > MAX_TOTAL {
                    return Err("the archive is too large".into());
                }
                if files.insert(clean.clone(), tar[pos..pos + size as usize].to_vec()).is_some() {
                    return Err(format!("`{clean}` appears twice"));
                }
            }
            b'5' => {
                // a folder entry: harmless, but its name must still be a plain relative path
                if !clean.is_empty() && clean != "." {
                    check_dir_name(&clean)?;
                }
            }
            b'2' => return Err(format!("`{clean}` is a symbolic link; presets may only contain plain files")),
            b'1' => return Err(format!("`{clean}` is a hard link; presets may only contain plain files")),
            b'x' | b'g' => {} // pax extended header: attributes are ignored, never applied
            b'L' | b'K' => return Err("GNU long names are not accepted".into()),
            other => return Err(format!("`{clean}` has an unsupported entry type ({})", other as char)),
        }
        pos = end;
    }
    check_files(&files)?;
    Ok(files)
}

fn check_dir_name(name: &str) -> Result<(), String> {
    if name.starts_with('/') || name.contains('\\') || name.split('/').any(|c| c == ".." || c.is_empty() || c == ".") {
        return Err(format!("bad folder name `{name}`"));
    }
    if !matches!(name, "themes" | "wallpaper") {
        return Err(format!("unknown folder `{name}` in the preset"));
    }
    Ok(())
}

// ---------------------------------------------------------------- clipboard text form

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub const TEXT_HEADER: &str = "MACSHELL-PRESET-1:";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn unbase64(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for ch in s.chars().filter(|c| !c.is_whitespace()) {
        if ch == '=' {
            break;
        }
        let v = B64.iter().position(|b| *b as char == ch).ok_or("not valid base64 text")? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// A bundle as copyable text (`MACSHELL-PRESET-1:` + base64, wrapped).
pub fn to_text(bundle: &[u8]) -> String {
    let b = base64(bundle);
    let mut s = String::from(TEXT_HEADER);
    s.push('\n');
    for chunk in b.as_bytes().chunks(76) {
        s.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        s.push('\n');
    }
    s
}

/// The inverse of [`to_text`]; surrounding blanks are fine.
pub fn from_text(text: &str) -> Result<Vec<u8>, String> {
    let t = text.trim();
    let body = t.strip_prefix(TEXT_HEADER).ok_or("the text is not a macshell preset (it should start with MACSHELL-PRESET-1:)")?;
    if body.len() > MAX_CLIPBOARD * 2 {
        return Err("the text is too large".into());
    }
    unbase64(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BTreeMap<String, Vec<u8>> {
        let mut f = BTreeMap::new();
        f.insert(MANIFEST.into(), b"format = 1\nname = \"x\"\n".to_vec());
        f.insert("macshell.json".into(), b"{\"theme\":\"nord\"}\n".to_vec());
        f.insert("rdwm.toml".into(), b"[general]\nborder_px = 3\n".to_vec());
        f.insert("themes/mine.toml".into(), b"name = \"mine\"\n".to_vec());
        f.insert("wallpaper/pic.png".into(), vec![0x89, b'P', b'N', b'G', 1, 2, 3]);
        f
    }

    /// A hand-made archive with one arbitrary entry (type flag, name, data), for the hostile cases.
    fn tar_with(name: &str, flag: u8, data: &[u8], link: &str) -> Vec<u8> {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut h[100..108], 0o644);
        octal(&mut h[124..136], data.len() as u64);
        h[156] = flag;
        h[157..157 + link.len()].copy_from_slice(link.as_bytes());
        h[257..263].copy_from_slice(b"ustar\0");
        h[148..156].fill(b' ');
        let sum: u32 = h.iter().map(|b| *b as u32).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut tar = h.to_vec();
        tar.extend_from_slice(data);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
        tar.extend_from_slice(&[0u8; 1024]);
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(&tar).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn round_trip() {
        let f = sample();
        let b = write_tar_gz(&f).unwrap();
        assert_eq!(&b[..2], &[0x1f, 0x8b]);
        assert_eq!(read_tar_gz(&b).unwrap(), f);
        // deterministic
        assert_eq!(b, write_tar_gz(&f).unwrap());
    }

    #[test]
    fn text_form_round_trips() {
        let b = write_tar_gz(&sample()).unwrap();
        let t = to_text(&b);
        assert!(t.starts_with(TEXT_HEADER) && t.lines().all(|l| l.len() <= 76 + TEXT_HEADER.len()));
        assert_eq!(from_text(&t).unwrap(), b);
        assert_eq!(from_text(&format!("  \n{t}\n\n")).unwrap(), b);
        assert!(from_text("hello").is_err());
        assert!(from_text("MACSHELL-PRESET-1:!!!").is_err());
        for n in 0..6 {
            let d: Vec<u8> = (0..n as u8).collect();
            assert_eq!(unbase64(&base64(&d)).unwrap(), d);
        }
    }

    #[test]
    fn names_are_whitelisted() {
        assert_eq!(check_name("manifest.toml"), Ok(NameKind::Manifest));
        assert_eq!(check_name("rdwm.toml"), Ok(NameKind::Text));
        assert_eq!(check_name("themes/nord-2.toml"), Ok(NameKind::Theme));
        assert_eq!(check_name("wallpaper/My Pic (1).JPG"), Ok(NameKind::Wallpaper));
        for bad in [
            "/etc/passwd",
            "../x",
            "themes/../../x.toml",
            "themes/..toml",
            "wallpaper/x.sh",
            "wallpaper/.hidden.png",
            "a/b.json",
            "macshell.json/",
            "themes/a/b.toml",
            "evil.sh",
            "themes\\x.toml",
            "",
            "./rdwm.toml",
            "wallpaper/x.png\0",
        ] {
            assert!(check_name(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn hostile_archives_are_rejected() {
        let ok = |name: &str| read_tar_gz(&tar_with(name, b'0', b"x", ""));
        // path traversal and absolute paths
        for bad in ["../evil", "../../etc/cron.d/x", "/etc/passwd", "themes/../../evil.toml", "rdwm.toml/../../x"] {
            let e = ok(bad).unwrap_err();
            assert!(e.contains("traversal") || e.contains("absolute") || e.contains("unknown") || e.contains("odd"), "{bad}: {e}");
        }
        // symlink and hard link
        let e = read_tar_gz(&tar_with("rdwm.toml", b'2', b"", "/etc/shadow")).unwrap_err();
        assert!(e.contains("symbolic link"), "{e}");
        let e = read_tar_gz(&tar_with("rdwm.toml", b'1', b"", "macshell.json")).unwrap_err();
        assert!(e.contains("hard link"), "{e}");
        // devices, fifos
        for flag in [b'3', b'4', b'6'] {
            assert!(read_tar_gz(&tar_with("rdwm.toml", flag, b"", "")).is_err());
        }
        // unknown names
        assert!(ok("run.sh").unwrap_err().contains("unknown"));
        assert!(ok("wallpaper/a.svg").is_err());
        // an unknown folder entry
        assert!(read_tar_gz(&tar_with("evil/", b'5', b"", "")).is_err());
        // not gzip / truncated / bad checksum
        assert!(read_tar_gz(b"hello world, this is not a preset at all").unwrap_err().contains("gzip"));
        let good = write_tar_gz(&sample()).unwrap();
        assert!(read_tar_gz(&good[..good.len() / 2]).is_err());
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut bad = tar_with_raw("manifest.toml", b"x");
        bad[0] ^= 0x01; // corrupt the name so the checksum fails
        e.write_all(&bad).unwrap();
        assert!(read_tar_gz(&e.finish().unwrap()).unwrap_err().contains("checksum"));
    }

    fn tar_with_raw(name: &str, data: &[u8]) -> Vec<u8> {
        let mut t = header(name, data.len() as u64).unwrap().to_vec();
        t.extend_from_slice(data);
        t.resize(t.len().div_ceil(512) * 512, 0);
        t.extend_from_slice(&[0u8; 1024]);
        t
    }

    #[test]
    fn oversized_and_duplicate_entries_are_rejected() {
        // a text file over the cap
        let big = vec![b'a'; MAX_TEXT as usize + 1];
        let e = read_tar_gz(&tar_with("rdwm.toml", b'0', &big, "")).unwrap_err();
        assert!(e.contains("too large"), "{e}");
        // the writer refuses too
        let mut f = sample();
        f.insert("rdwm.toml".into(), big);
        assert!(write_tar_gz(&f).is_err());
        // manifest over its cap
        let big = vec![b'a'; MAX_MANIFEST as usize + 1];
        assert!(read_tar_gz(&tar_with("manifest.toml", b'0', &big, "")).is_err());
        // a decompression bomb: far more zeros than allowed
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..70 {
            e.write_all(&chunk).unwrap();
        }
        let bomb = e.finish().unwrap();
        assert!(bomb.len() < 200_000, "the test archive is small, the content is not");
        assert!(read_tar_gz(&bomb).is_err());
        // duplicate names
        let mut t = tar_with_raw("manifest.toml", b"a");
        t.truncate(t.len() - 1024);
        t.extend_from_slice(&tar_with_raw("manifest.toml", b"b"));
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&t).unwrap();
        assert!(read_tar_gz(&enc.finish().unwrap()).unwrap_err().contains("twice"));
        // missing manifest
        assert!(read_tar_gz(&tar_with("rdwm.toml", b'0', b"x", "")).unwrap_err().contains("manifest"));
        // too many wallpapers
        let mut f = sample();
        for i in 0..MAX_WALLPAPERS + 1 {
            f.insert(format!("wallpaper/{i}.png"), vec![1]);
        }
        assert!(write_tar_gz(&f).is_err());
    }

    #[test]
    fn pax_headers_are_skipped_not_obeyed() {
        // bsdtar writes `x` records; their `path=` must not rename the next entry
        let pax = b"20 path=../../evil\n";
        let mut t = Vec::new();
        t.extend_from_slice(&{
            let mut h = header("PaxHeader", pax.len() as u64).unwrap();
            h[156] = b'x';
            h[148..156].fill(b' ');
            let sum: u32 = h.iter().map(|b| *b as u32).sum();
            h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
            h
        });
        t.extend_from_slice(pax);
        t.resize(t.len().div_ceil(512) * 512, 0);
        t.extend_from_slice(&tar_with_raw("manifest.toml", b"format = 1\n"));
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(&t).unwrap();
        let files = read_tar_gz(&e.finish().unwrap()).unwrap();
        assert_eq!(files.keys().collect::<Vec<_>>(), ["manifest.toml"]);
    }
}
