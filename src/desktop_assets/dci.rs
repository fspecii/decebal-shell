//! Native conversion of DSG icon containers (DCI, Deepin's icon format) into a freedesktop PNG theme, opt-in per pack
//! (`[pack.dci_convert]`, see `docs/desktop-asset-packs.md`). Independent implementation of the published format (the
//! "DSG icon file specification"); nothing from dtk is used and nothing is run: the WebP layer is decoded by the `image`
//! crate already in the tree.
//!
//! Every container is hostile input. Its bytes come from the staged source, whose git tree id (and the licence blob ids)
//! were verified before this runs. Every length is checked against its container before it is used, nesting, entries and
//! names are bounded, and only a *plain* still WebP layer is converted; anything else refuses that icon, with its reason.

use super::source::{Node, Resolved, Staged};
use std::collections::BTreeMap;
use std::path::Path;

pub const MAX_DCI_BYTES: u64 = 8 << 20;
pub const MAX_ENTRIES_PER_LEVEL: usize = 256;
pub const MAX_DEPTH: usize = 6;
pub const MAX_WEBP_BYTES: usize = 4 << 20;
pub const MAX_SIDE: u32 = 1536;
pub const MAX_LAYER_SIZE: u32 = 1024;
pub const MAX_SCALE: u32 = 4;
/// Over a whole pack: containers, decoded pixels and PNG output.
pub const MAX_CONTAINERS: usize = 20_000;
pub const MAX_DECODED_PIXELS: u64 = 1 << 31;
pub const MAX_OUTPUT_BYTES: u64 = 256 << 20;
/// The aggregate limits of one pack's conversion (the defaults above; tests use small ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub containers: usize,
    pub pixels: u64,
    pub output: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Budget { containers: MAX_CONTAINERS, pixels: MAX_DECODED_PIXELS, output: MAX_OUTPUT_BYTES }
    }
}

/// The output sizes a pack may ask for.
pub const SIZES: [u32; 5] = [64, 96, 128, 192, 256];

/// The conversion a pack asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convert {
    pub size: u32,
    pub inherits: Vec<String>,
    /// Who made the images and under which licence, from the catalogue (filled in by the installer; never invented).
    pub attribution: String,
}

/// One converted icon: git blob of the container, its sha256, the layer used, its source pixels, the PNG's sha256 and size.
pub type IconRecord = (String, String, String, u32, String, u64);

/// What a conversion did, for the report, the provenance and `CONVERSION.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// icon -> what was converted (see [`IconRecord`])
    pub icons: BTreeMap<String, IconRecord>,
    /// icon -> the icon it links to
    pub links: BTreeMap<String, String>,
    /// icon -> why it was not converted
    pub refused: BTreeMap<String, String>,
}

/// An icon name the theme may use: `[A-Za-z0-9][A-Za-z0-9._+-]{0,127}`.
pub fn icon_name_ok(n: &str) -> bool {
    let mut c = n.chars();
    n.len() <= 128 && c.next().is_some_and(|c| c.is_ascii_alphanumeric()) && c.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

/// A plain positive decimal of at most `digits` digits (`^[1-9][0-9]{0,digits-1}$`).
fn plain_decimal(s: &str, digits: usize) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > digits || b[0] == b'0' || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    s.parse().ok()
}

/// One entry: type (1 file, 2 folder, 3 link), path inside the container, content.
pub type Entry<'a> = (u8, String, &'a [u8]);

/// The entries of one folder level; every length is checked before it is used.
fn read_entries(buf: &[u8], depth: usize) -> Result<Vec<Entry<'_>>, String> {
    if depth > MAX_DEPTH {
        return Err(format!("directories nested deeper than {MAX_DEPTH}"));
    }
    let mut out = Vec::new();
    let mut off = 0usize;
    while off < buf.len() {
        if out.len() >= MAX_ENTRIES_PER_LEVEL {
            return Err(format!("more than {MAX_ENTRIES_PER_LEVEL} entries in one directory"));
        }
        if buf.len() - off < 72 {
            return Err(format!("truncated entry header: {} of 72 bytes remain", buf.len() - off));
        }
        let kind = buf[off];
        let raw = &buf[off + 1..off + 64];
        let end = raw.iter().position(|&c| c == 0).ok_or("entry name is not NUL-terminated")?;
        let name = std::str::from_utf8(&raw[..end]).map_err(|_| "entry name is not UTF-8".to_string())?;
        if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.chars().any(char::is_control) {
            return Err(format!("invalid entry name {name:?}"));
        }
        let size = u64::from_le_bytes(buf[off + 64..off + 72].try_into().expect("8 bytes"));
        let remain = (buf.len() - off - 72) as u64;
        if size > remain {
            return Err(format!("entry '{name}' declares {size} bytes but only {remain} remain"));
        }
        let start = off + 72;
        out.push((kind, name.to_string(), &buf[start..start + size as usize]));
        off = start + size as usize;
    }
    Ok(out)
}

fn flatten<'a>(entries: Vec<Entry<'a>>, prefix: &str, depth: usize, out: &mut Vec<Entry<'a>>) -> Result<(), String> {
    for (kind, name, content) in entries {
        let path = format!("{prefix}/{name}");
        match kind {
            2 => flatten(read_entries(content, depth + 1)?, &path, depth + 1, out)?,
            1 | 3 => out.push((kind, path, content)),
            k => return Err(format!("entry type {k} at {path}")),
        }
    }
    Ok(())
}

/// Every file and link of a container, with its path (`/256/normal.light/3/1.webp`).
pub fn parse(data: &[u8]) -> Result<Vec<Entry<'_>>, String> {
    if data.len() as u64 > MAX_DCI_BYTES {
        return Err(format!("{} bytes exceeds the {MAX_DCI_BYTES} byte limit", data.len()));
    }
    if data.len() < 8 || &data[..3] != b"DCI" {
        return Err("not a DCI file (magic)".into());
    }
    if data[3] != 0 {
        return Err("magic is not NUL-terminated".into());
    }
    if data[4] != 1 {
        return Err(format!("format version {}, only 1 is understood", data[4]));
    }
    let count = usize::from(data[5]) | usize::from(data[6]) << 8 | usize::from(data[7]) << 16;
    let root = read_entries(&data[8..], 0)?;
    if root.len() != count {
        return Err(format!("header says {count} root entries, found {}", root.len()));
    }
    let mut out = Vec::new();
    flatten(root, "", 0, &mut out)?;
    Ok(out)
}

/// The layer a container's icon is drawn from: the single plain WebP of `normal.light` at the largest size and scale.
pub struct Layer<'a> {
    pub size: u32,
    pub scale: u32,
    pub name: String,
    pub webp: &'a [u8],
}

/// The `normal.light` layers of a container by (size, scale): their file names and contents.
type LayersBySize<'a> = BTreeMap<(u32, u32), Vec<(String, &'a [u8])>>;

/// Picks the layer, or says why the icon cannot be converted. Never fails on a hostile path: it refuses.
pub fn best_layer<'a>(entries: &[Entry<'a>]) -> Result<Layer<'a>, String> {
    let mut files: LayersBySize<'a> = BTreeMap::new();
    for (kind, path, content) in entries {
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if *kind != 1 || parts.len() != 4 || parts[1] != "normal.light" {
            continue;
        }
        let (Some(size), Some(scale)) = (plain_decimal(parts[0], 4), plain_decimal(parts[2], 4)) else {
            return Err(format!("malformed size or scale directory in {path:?}"));
        };
        if !(1..=MAX_LAYER_SIZE).contains(&size) || !(1..=MAX_SCALE).contains(&scale) {
            return Err(format!("size {size} / scale {scale} outside 1..{MAX_LAYER_SIZE} / 1..{MAX_SCALE}"));
        }
        files.entry((size, scale)).or_default().push((parts[3].to_string(), content));
    }
    let Some(((size, scale), layers)) = files.into_iter().next_back() else { return Err("no normal.light image".into()) };
    if layers.len() != 1 {
        return Err(format!("{} layers at {size}@{scale}", layers.len()));
    }
    let (name, webp) = layers.into_iter().next().expect("one");
    let plain = name.strip_suffix(".webp").is_some_and(|p| plain_decimal(p, 3).is_some());
    if !plain {
        return Err(format!("layer '{name}' is not a plain `<prior>.webp` (padding, palette, colour-adjust or alpha8 fields need the palette engine)"));
    }
    if webp.len() < 12 || &webp[..4] != b"RIFF" || &webp[8..12] != b"WEBP" {
        return Err(format!("layer {name} is not a WebP"));
    }
    if webp.len() > MAX_WEBP_BYTES {
        return Err(format!("layer {name} is {} bytes, over the {MAX_WEBP_BYTES} byte limit", webp.len()));
    }
    let riff = u32::from_le_bytes(webp[4..8].try_into().expect("4 bytes"));
    if u64::from(riff) + 8 != webp.len() as u64 {
        return Err(format!("layer {name}: RIFF size {riff} does not match {} bytes", webp.len()));
    }
    Ok(Layer { size, scale, name, webp })
}

fn reader(webp: &[u8]) -> image::ImageReader<std::io::Cursor<&[u8]>> {
    let mut r = image::ImageReader::with_format(std::io::Cursor::new(webp), image::ImageFormat::WebP);
    let mut l = image::Limits::default();
    l.max_image_width = Some(MAX_SIDE);
    l.max_image_height = Some(MAX_SIDE);
    l.max_alloc = Some(64 << 20);
    r.limits(l);
    r
}

/// A still WebP's dimensions, from its header, before anything is decoded. An animation is refused.
pub fn still_dimensions(webp: &[u8]) -> Result<(u32, u32), String> {
    match webp.get(12..16) {
        Some(b"VP8 " | b"VP8L") => {}
        Some(b"VP8X") => {
            if webp.get(20).is_some_and(|f| f & 0x02 != 0) {
                return Err("an animated WebP".into());
            }
        }
        _ => return Err("an unknown WebP chunk".into()),
    }
    let (w, h) = reader(webp).into_dimensions().map_err(|e| format!("WebP header: {e}"))?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w} x {h} pixels (at most {MAX_SIDE} a side)"));
    }
    Ok((w, h))
}

/// Decodes the layer (bounded), shrinks it to fit `px` x `px` (never enlarges) and encodes a PNG.
pub fn to_png(webp: &[u8], px: u32) -> Result<Vec<u8>, String> {
    let img = reader(webp).decode().map_err(|e| format!("WebP decode: {e}"))?.to_rgba8();
    let (w, h) = img.dimensions();
    let img = if w.max(h) > px {
        let k = f64::from(px) / f64::from(w.max(h));
        let (nw, nh) = (((f64::from(w) * k).round() as u32).max(1), ((f64::from(h) * k).round() as u32).max(1));
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let mut out = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("PNG encode: {e}"))?;
    Ok(out)
}

/// Converts one container's bytes; `Err` is the reason the icon is refused. Charges `pixels` before decoding.
fn convert_one(data: &[u8], px: u32, pixels: &mut u64, max_pixels: u64) -> Result<(String, u32, Vec<u8>), String> {
    let entries = parse(data).map_err(|e| format!("refused: {e}"))?;
    let layer = best_layer(&entries)?;
    let (w, h) = still_dimensions(layer.webp).map_err(|e| format!("layer {}: {e}", layer.name))?;
    *pixels += u64::from(w) * u64::from(h);
    if *pixels > max_pixels {
        return Err(format!("the pack's decode budget of {max_pixels} pixels is spent"));
    }
    let png = to_png(layer.webp, px)?;
    Ok((format!("{}/normal.light/{}/{}", layer.size, layer.scale, layer.name), layer.size * layer.scale, png))
}

fn write_new(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).custom_flags(libc::O_NOFOLLOW).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(data).map_err(|e| e.to_string())
}

fn parent(p: &str) -> &str {
    p.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Converts every `*.dci` directly inside the pack folder `pack` of `st` into `out/<size>/<icon>.png`, writes
/// `index.theme`, `NOTICE.txt` and `CONVERSION.json`, and returns what it did with the pack-relative paths it consumed.
/// An error refuses the whole pack (a broken budget, an index.theme already in the folder); a broken icon only refuses
/// that icon.
pub fn build(st: &Staged, pack: &str, display_name: &str, plan: &Convert, out: &Path, total_input: u64) -> Result<(Outcome, Vec<String>), String> {
    build_within(st, pack, display_name, plan, out, total_input, Budget::default())
}

/// [`build`] with explicit aggregate limits.
pub fn build_within(st: &Staged, pack: &str, display_name: &str, plan: &Convert, out: &Path, total_input: u64, budget: Budget) -> Result<(Outcome, Vec<String>), String> {
    use super::gitoid::hex;
    let under = |p: &'_ str| -> Option<String> {
        if pack == "." {
            return Some(p.to_string());
        }
        p.strip_prefix(pack).and_then(|r| r.strip_prefix('/')).map(String::from)
    };
    if !SIZES.contains(&plan.size) {
        return Err(format!("dci_convert size {} is not one of {SIZES:?}", plan.size));
    }
    let index = if pack == "." { "index.theme".to_string() } else { format!("{pack}/index.theme") };
    if st.nodes.contains_key(&index) {
        return Err("dci_convert writes index.theme, but the pack folder has one".into());
    }
    let mut containers: Vec<(String, String)> = Vec::new(); // (source path, icon)
    for k in st.nodes.keys() {
        let Some(rel) = under(k) else { continue };
        if rel.contains('/') {
            continue;
        }
        if let Some(icon) = rel.strip_suffix(".dci") {
            containers.push((k.clone(), icon.to_string()));
        }
    }
    if containers.len() > budget.containers {
        return Err(format!("{} DCI containers, at most {}", containers.len(), budget.containers));
    }
    // The ordinary assembly pass must not replace generated images, aliases or their
    // provenance. Refuse source collisions before writing any converted output.
    let mut reserved = vec!["NOTICE.txt".to_string(), "CONVERSION.json".to_string()];
    reserved.extend(containers.iter().map(|(_, icon)| format!("{}/{icon}.png", plan.size)));
    for rel in reserved {
        let source = if pack == "." { rel.clone() } else { format!("{pack}/{rel}") };
        if st.nodes.contains_key(&source) {
            return Err(format!("dci_convert output `{rel}` collides with a source entry"));
        }
    }
    let size_dir = out.join(plan.size.to_string());
    std::fs::create_dir_all(&size_dir).map_err(|e| e.to_string())?;
    let mut o = Outcome::default();
    let mut consumed = Vec::new();
    let mut links: Vec<(String, String, String)> = Vec::new(); // (icon, source path, target source path)
    let (mut pixels, mut output, mut input) = (0u64, 0u64, 0u64);
    for (k, icon) in &containers {
        consumed.push(under(k).expect("inside"));
        if !icon_name_ok(icon) {
            o.refused.insert(icon.clone(), "icon name has characters outside [A-Za-z0-9._+-] or is too long".into());
            continue;
        }
        match st.nodes.get(k) {
            Some(Node::Link(_)) => match st.resolve(k) {
                Resolved::File(t) if parent(&t) == parent(k) && t.ends_with(".dci") => links.push((icon.clone(), k.clone(), t)),
                Resolved::File(_) => {
                    o.refused.insert(icon.clone(), "a link that does not point at a container of the same folder".into());
                }
                _ => {
                    o.refused.insert(icon.clone(), "a link to nothing".into());
                }
            },
            Some(Node::File { staged: true, size, oid, sha256, .. }) => {
                if *size > MAX_DCI_BYTES {
                    o.refused.insert(icon.clone(), format!("refused: {size} bytes exceeds the {MAX_DCI_BYTES} byte limit"));
                    continue;
                }
                input += size;
                if input > total_input {
                    return Err("the DCI containers exceed the install's input budget".into());
                }
                let data = std::fs::read(st.file_path(k)).map_err(|e| format!("{k}: {e}"))?;
                match convert_one(&data, plan.size, &mut pixels, budget.pixels) {
                    Ok((layer, source_px, png)) => {
                        output += png.len() as u64;
                        if output > budget.output {
                            return Err(format!("the converted icons exceed the output budget of {} bytes", budget.output));
                        }
                        write_new(&size_dir.join(format!("{icon}.png")), &png)?;
                        let png_sha = crate::theme::engine::sha256::digest_hex(&png);
                        o.icons.insert(icon.clone(), (hex(oid), crate::theme::engine::sha256::hex(sha256), layer, source_px, png_sha, png.len() as u64));
                    }
                    Err(why) if why.contains("budget") => return Err(why),
                    Err(why) => {
                        o.refused.insert(icon.clone(), why);
                    }
                }
            }
            _ => {
                o.refused.insert(icon.clone(), "not read from the source".into());
            }
        }
    }
    for (icon, _k, t) in links {
        let target = under(&t).and_then(|r| r.strip_suffix(".dci").map(String::from)).unwrap_or_default();
        if o.icons.contains_key(&target) {
            std::os::unix::fs::symlink(format!("{target}.png"), size_dir.join(format!("{icon}.png"))).map_err(|e| e.to_string())?;
            o.links.insert(icon, target);
        } else {
            let why = o.refused.get(&target).cloned().unwrap_or_else(|| "its container was not converted".into());
            o.refused.insert(icon, format!("links to {target}, which was refused: {why}"));
        }
    }
    let s = plan.size;
    let inherits = plan.inherits.join(",");
    write_new(
        &out.join("index.theme"),
        format!(
            "[Icon Theme]\nName={display_name}\nComment=Converted from DSG icon containers (DCI)\nInherits={inherits}\nExample=x-directory-normal\n\nDirectories={s}\n\n[{s}]\nSize={s}\nContext=Applications\nType=Scalable\nMinSize=16\nMaxSize=512\n"
        )
        .as_bytes(),
    )?;
    write_new(
        &out.join("NOTICE.txt"),
        format!(
            "{}\nModified by decebal's asset installer: each icon's WebP layer was decoded and shrunk to at most {s} x {s} pixels as a PNG (never enlarged); \
             icons whose container could not be converted are not included (see CONVERSION.json). The licence texts are in LICENSES/.\n",
            plan.attribution
        )
        .as_bytes(),
    )?;
    let icons: serde_json::Map<String, serde_json::Value> = o
        .icons
        .iter()
        .map(|(n, (blob, src, layer, px, png, bytes))| (n.clone(), serde_json::json!({"git_blob": blob, "source_sha256": src, "layer": layer, "source_pixels": px, "png_sha256": png, "bytes": bytes})))
        .collect();
    let doc = serde_json::json!({
        "format": 1, "converter": concat!("decebal ", env!("CARGO_PKG_VERSION"), " native DCI conversion (image crate)"), "size": s,
        "attribution": plan.attribution, "converted": o.icons.len(), "linked": o.links.len(), "refused_count": o.refused.len(),
        "icons": icons, "links": o.links, "refused": o.refused,
    });
    write_new(&out.join("CONVERSION.json"), (serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n").as_bytes())?;
    Ok((o, consumed))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::source::{from_tar, Filter};
    use super::super::tar::Format;
    use super::super::testkit::*;
    use super::super::Limits;
    use super::*;

    /// A lossless WebP of `w` x `h` pixels.
    pub fn webp(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 120, 255]));
        let mut out = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut out).encode(img.as_raw(), w, h, image::ExtendedColorType::Rgba8).unwrap();
        out
    }

    pub fn entry(kind: u8, name: &str, content: &[u8]) -> Vec<u8> {
        let mut v = vec![kind];
        let mut n = [0u8; 63];
        n[..name.len()].copy_from_slice(name.as_bytes());
        v.extend_from_slice(&n);
        v.extend_from_slice(&(content.len() as u64).to_le_bytes());
        v.extend_from_slice(content);
        v
    }

    pub fn dir(name: &str, children: &[Vec<u8>]) -> Vec<u8> {
        entry(2, name, &children.concat())
    }

    pub fn container(root: &[Vec<u8>]) -> Vec<u8> {
        let mut v = b"DCI\0\x01".to_vec();
        let n = root.len() as u32;
        v.extend_from_slice(&n.to_le_bytes()[..3]);
        v.extend_from_slice(&root.concat());
        v
    }

    /// An icon as nirvana stores it: 256/normal.light/3/<layer>, and normal.dark as a link to it.
    pub fn icon_with(layer: &str, data: &[u8]) -> Vec<u8> {
        container(&[dir("256", &[dir("normal.light", &[dir("3", &[entry(1, layer, data)])]), entry(3, "normal.dark", b"normal.light")])])
    }

    pub fn icon(w: u32) -> Vec<u8> {
        icon_with("1.webp", &webp(w, w))
    }

    #[test]
    fn containers_parse_and_the_largest_plain_layer_is_chosen() {
        let c = container(&[
            dir("16", &[dir("normal.light", &[dir("1", &[entry(1, "1.webp", &webp(16, 16))])])]),
            dir("256", &[dir("normal.light", &[dir("3", &[entry(1, "1.webp", &webp(48, 48))]), dir("1", &[entry(1, "1.webp", &webp(8, 8))])]), entry(3, "normal.dark", b"normal.light")]),
        ]);
        let e = parse(&c).unwrap();
        assert!(e.iter().any(|(k, p, _)| *k == 3 && p == "/256/normal.dark"));
        let l = best_layer(&e).unwrap();
        assert_eq!((l.size, l.scale, l.name.as_str()), (256, 3, "1.webp"));
        assert_eq!(still_dimensions(l.webp).unwrap(), (48, 48));
    }

    #[test]
    fn malformed_containers_are_refused_before_their_bytes_are_used() {
        let good = icon(4);
        let mut wide = Vec::new();
        for i in 0..257 {
            wide.push(entry(1, &format!("f{i}"), b""));
        }
        let mut deep = entry(1, "1.webp", b"x");
        for i in 0..8 {
            deep = dir(&format!("d{i}"), &[deep]);
        }
        let mut overflow = container(&[entry(1, "a", b"abc")]);
        overflow[8 + 64..8 + 72].copy_from_slice(&u64::MAX.to_le_bytes());
        let mut noterm = container(&[entry(1, "a", b"")]);
        noterm[9..9 + 63].fill(b'a');
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            ("magic", b"PNG\0\x01\0\0\0".to_vec(), "magic"),
            ("unterminated magic", b"DCIX\x01\0\0\0".to_vec(), "NUL-terminated"),
            ("version", [&b"DCI\0\x02"[..], &good[5..]].concat(), "version 2"),
            ("count", [&good[..5], &[2, 0, 0][..], &good[8..]].concat(), "header says 2"),
            ("truncated header", [&good[..20]].concat(), "truncated entry header"),
            ("truncated content", good[..good.len() - 10].to_vec(), "declares"),
            ("length overflow", overflow, "declares 18446744073709551615 bytes"),
            ("unterminated name", noterm, "not NUL-terminated"),
            ("non-UTF-8 name", container(&[entry(1, "a", b"")]).iter().enumerate().map(|(i, b)| if i == 9 { 0xff } else { *b }).collect(), "not UTF-8"),
            ("slash in a name", container(&[entry(1, "a/b", b"")]), "invalid entry name"),
            ("control character", container(&[entry(1, "a\x07b", b"")]), "invalid entry name"),
            ("dot-dot", container(&[entry(1, "..", b"")]), "invalid entry name"),
            ("unknown type", container(&[entry(7, "a", b"")]), "entry type 7"),
            ("too wide", container(&[dir("256", &wide)]), "more than 256 entries"),
            ("too deep", container(&[deep]), "nested deeper"),
        ];
        for (what, data, want) in cases {
            let e = parse(&data).map(|_| ()).unwrap_err();
            assert!(e.contains(want), "{what}: {e}");
        }
        let big = vec![0u8; MAX_DCI_BYTES as usize + 1];
        assert!(parse(&big).map(|_| ()).unwrap_err().contains("byte limit"));
    }

    #[test]
    fn only_a_single_plain_still_webp_layer_converts() {
        let w = webp(8, 8);
        let mut wrong_riff = w.clone();
        wrong_riff[4] ^= 1;
        let mut animated = w.clone();
        animated.splice(12..16, *b"VP8X");
        animated[20] |= 0x02;
        for (what, c, want) in [
            ("palette", icon_with("1.0.webp", &w), "not a plain"),
            ("padding", icon_with("1.5p.webp", &w), "not a plain"),
            ("alpha8", icon_with("1.webp.alpha8", &w), "not a plain"),
            ("other format", icon_with("1.png", &w), "not a plain"),
            ("two layers", container(&[dir("256", &[dir("normal.light", &[dir("3", &[entry(1, "1.webp", &w), entry(1, "2.webp", &w)])])])]), "2 layers"),
            ("leading zero size", container(&[dir("0256", &[dir("normal.light", &[dir("3", &[entry(1, "1.webp", &w)])])])]), "malformed size"),
            ("size out of range", container(&[dir("2048", &[dir("normal.light", &[dir("3", &[entry(1, "1.webp", &w)])])])]), "outside"),
            ("scale out of range", container(&[dir("256", &[dir("normal.light", &[dir("5", &[entry(1, "1.webp", &w)])])])]), "outside"),
            ("no light image", container(&[dir("256", &[dir("normal.dark", &[dir("3", &[entry(1, "1.webp", &w)])])])]), "no normal.light"),
            ("not WebP", icon_with("1.webp", b"RIFF\x04\0\0\0WAVEdata"), "not a WebP"),
            ("RIFF size", icon_with("1.webp", &wrong_riff), "RIFF size"),
        ] {
            let e = parse(&c).and_then(|en| best_layer(&en).map(|_| ())).unwrap_err();
            assert!(e.contains(want), "{what}: {e}");
        }
        // animation and oversized images are refused from the header, before decoding
        assert!(still_dimensions(&animated).unwrap_err().contains("animated"));
        // wider than allowed: refused from the header (the reader's own width limit or ours), never decoded
        let wide = webp(MAX_SIDE + 1, 1);
        let e = still_dimensions(&wide).unwrap_err();
        assert!(e.contains("1537") || e.contains("limit"), "{e}");
        // shrink only
        let png = to_png(&webp(300, 150), 128).unwrap();
        let d = image::load_from_memory(&png).unwrap();
        assert_eq!((d.width(), d.height()), (128, 64));
        let png = to_png(&webp(32, 32), 128).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 32, "never enlarged");
    }

    fn stage(entries: &[E]) -> (super::super::source::Staged, Scratch) {
        let s = Scratch::new("dci");
        let f = Filter { strip: 1, path: "icons".into(), ..Filter::default() };
        let keep = |r: &str| super::super::assemble::keep(r) || (!r.contains('/') && r.ends_with(".dci"));
        let st = from_tar(&targz(entries, None)[..], Format::TarGz, s.path().join("src"), f, &Limits::default(), &keep).unwrap();
        (st, s)
    }

    fn plan() -> Convert {
        Convert { size: 64, inherits: vec!["bloom".into()], attribution: "fixture icons, CC-BY-4.0".into() }
    }

    #[test]
    fn source_entries_cannot_replace_generated_images_aliases_or_provenance() {
        for rel in ["64/good.png", "64/alias.png", "NOTICE.txt", "CONVERSION.json"] {
            for as_link in [false, true] {
                let mut entries = pack();
                let path = format!("top/icons/{rel}");
                entries.push(if as_link { E::link(&path, "good.dci") } else { E::file(&path, b"foreign bytes") });
                let (st, scratch) = stage(&entries);
                let out = scratch.path().join("collision-out");
                std::fs::create_dir(&out).unwrap();
                let err = build(&st, "icons", "Fixture", &plan(), &out, 1 << 20).unwrap_err();
                assert!(err.contains("collides with a source entry"), "{rel}: {err}");
                assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "refuse before conversion writes");
            }
        }
    }

    fn pack() -> Vec<E> {
        vec![
            E::dir("top/"),
            E::file("top/icons/good.dci", &icon(96)),
            E::file("top/icons/palette.dci", &icon_with("1.0.webp", &webp(8, 8))),
            E::file("top/icons/broken.dci", &icon(8)[..30]),
            E::link("top/icons/alias.dci", "good.dci"),
            E::link("top/icons/alias-of-palette.dci", "palette.dci"),
            E::link("top/icons/gone.dci", "nothing.dci"),
            E::file("top/other/x.dci", &icon(8)),
            E::link("top/icons/elsewhere.dci", "../other/x.dci"),
            E::file("top/icons/sub/y.dci", &icon(8)),
            E::link("top/icons/outside.dci", "sub/y.dci"),
            E::file("top/icons/-bad.dci", &icon(8)),
        ]
    }

    #[test]
    fn a_pack_converts_plain_icons_links_them_and_lists_every_refusal() {
        let (st, s) = stage(&pack());
        let out = s.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let (o, used) = build(&st, "icons", "fixture", &plan(), &out, 1 << 30).unwrap();
        assert_eq!(o.icons.keys().collect::<Vec<_>>(), ["good"]);
        assert_eq!(o.links.get("alias").map(String::as_str), Some("good"));
        assert_eq!(std::fs::read_link(out.join("64/alias.png")).unwrap().display().to_string(), "good.png");
        let png = image::load_from_memory(&std::fs::read(out.join("64/good.png")).unwrap()).unwrap();
        assert_eq!(png.width(), 64);
        let r = &o.refused;
        assert!(r["palette"].contains("not a plain"));
        assert!(r["broken"].starts_with("refused: "));
        assert!(r["alias-of-palette"].contains("links to palette, which was refused"));
        assert!(r["gone"].contains("a link to nothing"));
        assert!(r["outside"].contains("same folder"), "a container of the pack, but not a sibling");
        assert!(r["elsewhere"].contains("a link to nothing"), "outside the pack: never staged, never followed");
        assert!(r["-bad"].contains("icon name"));
        assert_eq!((o.icons.len() + o.links.len(), r.len()), (2, 7));
        assert_eq!(used.len(), 9, "every container and link directly in the folder was an input");
        let conv: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out.join("CONVERSION.json")).unwrap()).unwrap();
        assert_eq!(conv["icons"]["good"]["git_blob"], super::super::gitoid::hex(&super::super::gitoid::blob_oid(&icon(96))));
        assert_eq!((conv["converted"].as_u64(), conv["refused_count"].as_u64()), (Some(1), Some(7)));
        let notice = std::fs::read_to_string(out.join("NOTICE.txt")).unwrap();
        assert!(notice.starts_with("fixture icons, CC-BY-4.0") && notice.contains("shrunk to at most 64"));
        assert!(std::fs::read_to_string(out.join("index.theme")).unwrap().contains("Inherits=bloom\n"));
    }

    #[test]
    fn aggregate_budgets_and_an_existing_index_refuse_the_pack() {
        let mut many = vec![E::dir("top/")];
        for i in 0..4 {
            many.push(E::file(&format!("top/icons/i{i}.dci"), &icon(40)));
        }
        for (b, want) in [
            (Budget { containers: 3, ..Budget::default() }, "4 DCI containers, at most 3"),
            (Budget { pixels: 3 * 40 * 40, ..Budget::default() }, "decode budget"),
            (Budget { output: 100, ..Budget::default() }, "output budget"),
        ] {
            let (st, s) = stage(&many);
            let out = s.path().join("out");
            std::fs::create_dir_all(&out).unwrap();
            let e = build_within(&st, "icons", "f", &plan(), &out, 1 << 30, b).unwrap_err();
            assert!(e.contains(want), "{b:?}: {e}");
        }
        let (st, s) = stage(&many);
        let out = s.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        assert!(build(&st, "icons", "f", &plan(), &out, 100).unwrap_err().contains("input budget"));
        let mut with_index = many.clone();
        with_index.push(E::file("top/icons/index.theme", b"[Icon Theme]\nName=x\n"));
        let (st, s) = stage(&with_index);
        let out = s.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        assert!(build(&st, "icons", "f", &plan(), &out, 1 << 30).unwrap_err().contains("has one"));
    }
}
