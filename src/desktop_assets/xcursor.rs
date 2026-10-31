//! Cursors built from PNGs and declared hotspots, natively: what `xcursorgen` does, without running it or any script.
//!
//! A pack lists its cursors as data (`[[pack.xcursor]]` in the catalogue): a name, the lines of an xcursorgen config
//! (`size xhot yhot file.png [delay_ms]`, files relative to the pack folder) and aliases. The PNGs are read from the
//! verified source; each cursor becomes an Xcursor file in `cursors/`, each alias a link to it.

use std::path::Path;

/// One image of a cursor (one size, or one frame of an animation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub size: u32,
    pub xhot: u32,
    pub yhot: u32,
    /// Relative to the pack folder.
    pub file: String,
    pub delay: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub name: String,
    pub images: Vec<Image>,
    pub aliases: Vec<String>,
}

pub const MAX_CURSORS: usize = 512;
pub const MAX_IMAGES: usize = 64;
pub const MAX_SIDE: u32 = 256;
/// All cursors of one pack together: bytes of the generated Xcursor files and images in them (dark-sense, the
/// largest real recipe, makes ~6 MiB from 405 images).
pub const MAX_GENERATED_BYTES: u64 = 64 << 20;
pub const MAX_GENERATED_IMAGES: usize = 4096;

/// Width and height from a PNG's IHDR, read before anything is decoded.
pub fn png_dimensions(data: &[u8]) -> Result<(u32, u32), String> {
    if data.len() < 24 || !data.starts_with(b"\x89PNG\r\n\x1a\n") || &data[12..16] != b"IHDR" {
        return Err("not a PNG (no IHDR header)".into());
    }
    let be = |i: usize| u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
    Ok((be(16), be(20)))
}

/// A cursor file name: `[A-Za-z0-9][A-Za-z0-9_.+-]*` (the X11, CSS and hash names all fit), at most 64.
pub fn safe_name(n: &str) -> bool {
    super::safe_theme_name(n)
}

/// Parses xcursorgen config lines. Paths may start with `./`; `#` starts a comment line.
pub fn parse_config(text: &str) -> Result<Vec<Image>, String> {
    let mut v = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if !(4..=5).contains(&f.len()) {
            return Err(format!("line {}: expected `size xhot yhot file.png [delay]`", i + 1));
        }
        let num = |s: &str, what: &str| s.parse::<u32>().map_err(|_| format!("line {}: {what} `{s}` is not a number", i + 1));
        let (size, xhot, yhot) = (num(f[0], "size")?, num(f[1], "xhot")?, num(f[2], "yhot")?);
        let delay = if f.len() == 5 { num(f[4], "delay")? } else { 0 };
        if size == 0 || size > MAX_SIDE {
            return Err(format!("line {}: size {size} is outside 1..{MAX_SIDE}", i + 1));
        }
        let file = f[3].trim_start_matches("./").to_string();
        super::safe_rel(&file).map_err(|e| format!("line {}: {e}", i + 1))?;
        if !file.to_ascii_lowercase().ends_with(".png") {
            return Err(format!("line {}: `{file}` is not a .png", i + 1));
        }
        v.push(Image { size, xhot, yhot, file, delay });
    }
    if v.is_empty() {
        return Err("no images".into());
    }
    if v.len() > MAX_IMAGES {
        return Err(format!("more than {MAX_IMAGES} images"));
    }
    Ok(v)
}

/// Encodes an Xcursor file from `(image, width, height, premultiplied little-endian ARGB words)`.
pub fn encode(images: &[(&Image, u32, u32, Vec<u32>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let u = |o: &mut Vec<u8>, x: u32| o.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(b"Xcur");
    u(&mut out, 16);
    u(&mut out, 0x1_0000);
    u(&mut out, images.len() as u32);
    let mut pos = 16 + 12 * images.len() as u32;
    for (im, w, h, _) in images {
        u(&mut out, 0xfffd_0002);
        u(&mut out, im.size);
        u(&mut out, pos);
        pos += 36 + w * h * 4;
    }
    for (im, w, h, px) in images {
        for x in [36, 0xfffd_0002, im.size, 1, *w, *h, im.xhot, im.yhot, im.delay] {
            u(&mut out, x);
        }
        for p in px {
            u(&mut out, *p);
        }
    }
    out
}

/// A PNG as premultiplied ARGB words, with its size.
pub fn argb_from_png(data: &[u8]) -> Result<(u32, u32, Vec<u32>), String> {
    let (w, h) = png_dimensions(data)?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}x{h} pixels (cursor images are at most {MAX_SIDE} a side)"));
    }
    let img = image::load_from_memory_with_format(data, image::ImageFormat::Png).map_err(|e| format!("not a readable PNG: {e}"))?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}x{h} pixels (cursor images are at most {MAX_SIDE} a side)"));
    }
    let px = img
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            let pm = |c: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
            u32::from_be_bytes([a, pm(r), pm(g), pm(b)])
        })
        .collect();
    Ok((w, h, px))
}

/// Builds every cursor into `out/cursors/` from PNGs read through `read` (pack-relative path -> bytes).
/// Returns the pack-relative PNG paths that were used (they are not installed themselves).
pub fn build(cursors: &[Cursor], out: &Path, read: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<Vec<String>, String> {
    build_within(cursors, out, read, MAX_GENERATED_BYTES, MAX_GENERATED_IMAGES)
}

/// [`build`] with explicit limits on everything generated, across all cursors: each image's size comes from its PNG
/// header and is charged before the PNG is decoded, so an oversized recipe is refused before memory is used for it.
pub fn build_within(cursors: &[Cursor], out: &Path, read: &dyn Fn(&str) -> Result<Vec<u8>, String>, max_bytes: u64, max_images: usize) -> Result<Vec<String>, String> {
    use std::os::unix::fs::PermissionsExt;
    let total_images: usize = cursors.iter().map(|c| c.images.len()).sum();
    if total_images > max_images {
        return Err(format!("the cursors declare {total_images} images; the limit is {max_images}"));
    }
    let dir = out.join("cursors");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut used = Vec::new();
    let mut generated: u64 = 0;
    for c in cursors {
        let mut imgs = Vec::new();
        generated += 16;
        for im in &c.images {
            let data = read(&im.file)?;
            let (w, h) = png_dimensions(&data).map_err(|e| format!("cursor `{}`: {}: {e}", c.name, im.file))?;
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                return Err(format!("cursor `{}`: {}: {w}x{h} pixels (cursor images are at most {MAX_SIDE} a side)", c.name, im.file));
            }
            generated += 12 + 36 + u64::from(w) * u64::from(h) * 4;
            if generated > max_bytes {
                return Err(format!("the cursors would make more than {} MiB of Xcursor data", max_bytes >> 20));
            }
            let (w, h, px) = argb_from_png(&data).map_err(|e| format!("cursor `{}`: {}: {e}", c.name, im.file))?;
            if im.xhot >= w || im.yhot >= h {
                return Err(format!("cursor `{}`: hotspot {},{} lies outside the {w}x{h} image {}", c.name, im.xhot, im.yhot, im.file));
            }
            imgs.push((im, w, h, px));
            if !used.contains(&im.file) {
                used.push(im.file.clone());
            }
        }
        let p = dir.join(&c.name);
        if std::fs::symlink_metadata(&p).is_ok() {
            return Err(format!("cursor `{}` is defined twice", c.name));
        }
        std::fs::write(&p, encode(&imgs)).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    }
    for c in cursors {
        for a in &c.aliases {
            let p = dir.join(a);
            if std::fs::symlink_metadata(&p).is_ok() {
                return Err(format!("alias `{a}` of `{}` is already a cursor", c.name));
            }
            std::os::unix::fs::symlink(&c.name, &p).map_err(|e| e.to_string())?;
        }
    }
    Ok(used)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::Scratch;
    use super::*;

    pub fn png_rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba(f(x, y)));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn config_lines_parse_and_bad_ones_are_refused() {
        let v = parse_config("# dark-sense\n24 4 0 24x24/left_ptr.png\n32 5 0 ./32x32/left_ptr.png 0\n24 11 11 24x24/wait-01.png 30\n").unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!((v[1].file.as_str(), v[2].delay), ("32x32/left_ptr.png", 30));
        for bad in ["", "24 4 left.png", "24 a 0 x.png", "0 0 0 x.png", "999 0 0 x.png", "24 0 0 ../x.png", "24 0 0 /x.png", "24 0 0 x.svg"] {
            assert!(parse_config(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn cursors_are_built_with_hotspots_premultiplied_pixels_and_aliases() {
        let s = Scratch::new("xcur");
        let red_half = png_rgba(24, 24, |x, _| if x < 12 { [255, 0, 0, 128] } else { [0, 0, 0, 0] });
        let big = png_rgba(32, 32, |_, _| [0, 0, 255, 255]);
        let files = std::collections::HashMap::from([("24x24/left_ptr.png".to_string(), red_half), ("32x32/left_ptr.png".to_string(), big)]);
        let read = |p: &str| files.get(p).cloned().ok_or_else(|| format!("{p} missing"));
        let c = Cursor { name: "left_ptr".into(), images: parse_config("24 4 2 24x24/left_ptr.png\n32 5 3 32x32/left_ptr.png").unwrap(), aliases: vec!["default".into(), "arrow".into()] };
        let used = build(std::slice::from_ref(&c), s.path(), &read).unwrap();
        assert_eq!(used.len(), 2);
        let d = std::fs::read(s.path().join("cursors/left_ptr")).unwrap();
        let im = crate::desktop_assets::cursor::images(&d).unwrap();
        assert_eq!(im.iter().map(|i| (i.nominal, i.width, i.xhot, i.yhot)).collect::<Vec<_>>(), [(24, 24, 4, 2), (32, 32, 5, 3)]);
        let (pm, info) = crate::desktop_assets::cursor::decode(&d, 24).unwrap();
        assert_eq!(info.nominal, 24);
        let p = pm.pixel(0, 0).unwrap();
        assert_eq!((p.red(), p.green(), p.blue(), p.alpha()), (128, 0, 0, 128), "premultiplied: half-transparent red");
        assert_eq!(std::fs::read_link(s.path().join("cursors/default")).unwrap().to_str(), Some("left_ptr"));
        // what libXcursor-style lookup sees
        let roots = crate::icons::resolve::Roots { icon_dirs: vec![s.path().parent().unwrap().to_path_buf()], pixmaps: vec![] };
        let theme = s.path().file_name().unwrap().to_str().unwrap();
        assert!(crate::desktop_assets::cursor::find(&roots, theme, "arrow").is_some());
    }

    #[test]
    fn sizes_come_from_the_header_and_one_budget_covers_every_recipe() {
        let s = Scratch::new("xcur-budget");
        // an IHDR claiming 70000 x 70000 with nothing decodable behind it: refused by its header, not by a decoder
        let mut bomb = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bomb.extend_from_slice(&70000u32.to_be_bytes());
        bomb.extend_from_slice(&70000u32.to_be_bytes());
        bomb.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let small = png_rgba(32, 32, |_, _| [0, 0, 0, 255]);
        let files = std::collections::HashMap::from([("bomb.png".to_string(), bomb), ("s.png".to_string(), small)]);
        let read = |p: &str| files.get(p).cloned().ok_or_else(|| format!("{p} missing"));
        let one = |name: &str, f: &str| Cursor { name: name.into(), images: parse_config(&format!("24 0 0 {f}")).unwrap(), aliases: vec![] };
        let e = build(&[one("x", "bomb.png")], s.path(), &read).unwrap_err();
        assert!(e.contains("70000x70000 pixels"), "{e}");
        // 3 cursors of one 32x32 image make 3 * (16 + 12 + 36 + 4096) bytes: a budget just below that refuses the third
        let three: Vec<Cursor> = (0..3).map(|i| one(&format!("c{i}"), "s.png")).collect();
        let d = Scratch::new("xcur-budget2");
        let e = build_within(&three, d.path(), &read, 3 * (16 + 12 + 36 + 4096) - 1, 100).unwrap_err();
        assert!(e.contains("Xcursor data"), "{e}");
        let d = Scratch::new("xcur-budget3");
        assert!(build_within(&three, d.path(), &read, 1 << 20, 2).unwrap_err().contains("declare 3 images"), "counted across recipes before any work");
        let d = Scratch::new("xcur-budget4");
        assert_eq!(build_within(&three, d.path(), &read, 1 << 20, 3).unwrap().len(), 1, "one source file used");
    }

    #[test]
    fn bad_images_hotspots_and_clashes_are_refused() {
        let s = Scratch::new("xcur-bad");
        let ok = png_rgba(24, 24, |_, _| [0, 0, 0, 255]);
        let files = std::collections::HashMap::from([("a.png".to_string(), ok.clone()), ("notpng.png".to_string(), b"GIF89a".to_vec()), ("huge.png".to_string(), png_rgba(300, 2, |_, _| [0; 4]))]);
        let read = |p: &str| files.get(p).cloned().ok_or_else(|| format!("{p} missing"));
        let cur = |name: &str, cfg: &str, aliases: &[&str]| Cursor { name: name.into(), images: parse_config(cfg).unwrap(), aliases: aliases.iter().map(|a| a.to_string()).collect() };
        for (what, cs) in [
            ("hotspot outside", vec![cur("x", "24 24 0 a.png", &[])]),
            ("not a png", vec![cur("x", "24 0 0 notpng.png", &[])]),
            ("too large", vec![cur("x", "24 0 0 huge.png", &[])]),
            ("missing", vec![cur("x", "24 0 0 nope.png", &[])]),
            ("twice", vec![cur("x", "24 0 0 a.png", &[]), cur("x", "24 0 0 a.png", &[])]),
            ("alias clash", vec![cur("x", "24 0 0 a.png", &[]), cur("y", "24 0 0 a.png", &["x"])]),
        ] {
            let d = Scratch::new("xcur-case");
            assert!(build(&cs, d.path(), &read).is_err(), "{what} accepted");
        }
        drop(s);
    }
}
