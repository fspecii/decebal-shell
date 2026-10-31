//! Xcursor files (validation and decoding of one image for previews) and cursor theme lookup the way libXcursor
//! does it: `<icon dir>/<theme>/cursors/<name>` in every icon root, then the themes of `Inherits=` in the theme's
//! `index.theme`, depth first, loops cut.

use crate::icons::resolve::Roots;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

const IMAGE: u32 = 0xfffd_0002;
/// The largest cursor image accepted (libXcursor allows 0x7fff; real themes stop at 256).
const MAX_SIDE: u32 = 1024;

/// The core pointer names a theme must have one of to be usable.
pub const CORE: &[&str] = &["left_ptr", "default"];

/// Cursors a preview shows: `(caption, names tried in order)` (CSS / freedesktop name, then the X11 one).
pub const SAMPLES: &[(&str, &[&str])] = &[
    ("Pointer", &["default", "left_ptr"]),
    ("Link", &["pointer", "hand2", "pointing_hand"]),
    ("Text", &["text", "xterm", "ibeam"]),
    ("Busy", &["wait", "watch"]),
    ("Progress", &["progress", "left_ptr_watch"]),
    ("Move", &["move", "fleur", "all-scroll"]),
    ("Resize", &["nwse-resize", "bottom_right_corner", "size_fdiag"]),
    ("Not allowed", &["not-allowed", "crossed_circle", "circle"]),
    ("Crosshair", &["crosshair", "cross"]),
    ("Help", &["help", "question_arrow"]),
];

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub nominal: u32,
    pub width: u32,
    pub height: u32,
    pub xhot: u32,
    pub yhot: u32,
    pos: usize,
}

/// The images of an Xcursor file, after checking every table entry lies inside the data.
pub fn images(d: &[u8]) -> Result<Vec<ImageInfo>, String> {
    if !d.starts_with(b"Xcur") {
        return Err("not an Xcursor file".into());
    }
    let hsize = u32_at(d, 4).ok_or("truncated Xcursor header")? as usize;
    let ntoc = u32_at(d, 12).ok_or("truncated Xcursor header")? as usize;
    if hsize < 16 || ntoc == 0 || ntoc > 0x1_0000 || hsize + ntoc * 12 > d.len() {
        return Err("corrupt Xcursor header".into());
    }
    let mut out = Vec::new();
    for i in 0..ntoc {
        let at = hsize + i * 12;
        let (ty, sub, pos) = (u32_at(d, at).unwrap_or(0), u32_at(d, at + 4).unwrap_or(0), u32_at(d, at + 8).unwrap_or(0) as usize);
        if ty != IMAGE {
            continue; // comments
        }
        let field = |k: usize| u32_at(d, pos + 4 * k).ok_or_else(|| "Xcursor image outside the file".to_string());
        if field(0)? != 36 || field(1)? != IMAGE || field(2)? != sub {
            return Err("corrupt Xcursor image chunk".into());
        }
        let (w, h) = (field(4)?, field(5)?);
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return Err(format!("Xcursor image of {w}x{h} pixels"));
        }
        let (xhot, yhot) = (field(6)?, field(7)?);
        if xhot > w || yhot > h {
            return Err("Xcursor hotspot outside its image".into());
        }
        let end = pos + 36 + (w as usize) * (h as usize) * 4;
        if end > d.len() {
            return Err("Xcursor pixels outside the file".into());
        }
        out.push(ImageInfo { nominal: sub, width: w, height: h, xhot, yhot, pos });
    }
    if out.is_empty() {
        return Err("Xcursor file without an image".into());
    }
    Ok(out)
}

/// The image whose nominal size is closest to `size` (the first frame of an animation), as a pixmap.
pub fn decode(d: &[u8], size: u32) -> Result<(Pixmap, ImageInfo), String> {
    let imgs = images(d)?;
    let best = *imgs.iter().min_by_key(|i| (i.nominal.abs_diff(size), std::cmp::Reverse(i.nominal))).expect("non-empty");
    let mut pm = Pixmap::new(best.width, best.height).ok_or("empty cursor")?;
    let src = &d[best.pos + 36..best.pos + 36 + (best.width * best.height * 4) as usize];
    for (o, px) in pm.data_mut().chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        // little-endian ARGB words, already premultiplied: B G R A -> R G B A, alpha clamps the colour
        let a = px[3];
        o.copy_from_slice(&[px[2].min(a), px[1].min(a), px[0].min(a), a]);
    }
    Ok((pm, best))
}

fn inherits_of(roots: &Roots, theme: &str) -> Vec<String> {
    for r in &roots.icon_dirs {
        if let Ok(t) = std::fs::read_to_string(r.join(theme).join("index.theme")) {
            return crate::icons::index::parse(&t).inherits;
        }
    }
    Vec::new()
}

/// The file for cursor `name` in `theme` or the themes it inherits, and the theme that had it.
pub fn find(roots: &Roots, theme: &str, name: &str) -> Option<(PathBuf, String)> {
    fn go(roots: &Roots, theme: &str, name: &str, seen: &mut Vec<String>) -> Option<(PathBuf, String)> {
        if seen.iter().any(|s| s == theme) || seen.len() > 16 || !super::safe_theme_name(theme) {
            return None;
        }
        seen.push(theme.to_string());
        for r in &roots.icon_dirs {
            let p = r.join(theme).join("cursors").join(name);
            if p.is_file() {
                return Some((p, theme.to_string()));
            }
        }
        inherits_of(roots, theme).iter().find_map(|t| go(roots, t, name, seen))
    }
    go(roots, theme, name, &mut Vec::new())
}

/// Installed cursor themes (a `cursors/` folder in some root), sorted, each once.
pub fn list_themes(roots: &Roots) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for r in &roots.icon_dirs {
        let Ok(rd) = std::fs::read_dir(r) else { continue };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if id.starts_with('.') || out.iter().any(|(i, _)| *i == id) {
                continue;
            }
            let c = e.path().join("cursors");
            if CORE.iter().any(|n| c.join(n).is_file()) {
                out.push((id, e.path()));
            }
        }
    }
    out.sort_by_key(|(i, _)| i.to_lowercase());
    out
}

/// Is `theme` a usable cursor theme (directly or through what it inherits)?
pub fn usable(roots: &Roots, theme: &str) -> bool {
    CORE.iter().any(|n| find(roots, theme, n).is_some())
}

/// A file inside a staged pack: is it a valid Xcursor file?
pub fn check_file(p: &Path) -> Result<(), String> {
    let d = std::fs::read(p).map_err(|e| e.to_string())?;
    images(&d).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{xcursor, Scratch};
    use super::*;

    #[test]
    fn xcursor_files_are_validated_and_decoded() {
        let d = xcursor(24);
        let im = images(&d).unwrap();
        assert_eq!((im[0].nominal, im[0].width, im[0].xhot), (24, 2, 1));
        let (pm, info) = decode(&d, 32).unwrap();
        assert_eq!((pm.width(), info.nominal), (2, 24));
        assert_eq!(&pm.data()[..8], &[0, 0, 0, 255, 255, 255, 255, 255], "black then white, opaque");
        assert!(images(b"Xcur").is_err());
        assert!(images(b"\x89PNG....").is_err());
        let mut bad = d.clone();
        bad[48..52].copy_from_slice(&5000u32.to_le_bytes()); // width
        assert!(images(&bad).is_err());
        assert!(images(&d[..d.len() - 4]).unwrap_err().contains("outside"), "truncated pixels");
    }

    #[test]
    fn lookup_follows_inherits_across_roots_and_cuts_loops() {
        let s = Scratch::new("cursors");
        s.write("a/Child/index.theme", b"[Icon Theme]\nName=Child\nInherits=Parent,Child\n");
        s.write("a/Child/cursors/text", &xcursor(24));
        s.write("b/Parent/index.theme", b"[Icon Theme]\nInherits=Child\n");
        s.write("b/Parent/cursors/left_ptr", &xcursor(24));
        let roots = Roots { icon_dirs: vec![s.path().join("a"), s.path().join("b")], pixmaps: vec![] };
        assert_eq!(find(&roots, "Child", "text").unwrap().1, "Child");
        assert_eq!(find(&roots, "Child", "left_ptr").unwrap().1, "Parent");
        assert!(find(&roots, "Child", "nope").is_none(), "the loop terminates");
        assert!(usable(&roots, "Child") && usable(&roots, "Parent"));
        let l: Vec<String> = list_themes(&roots).into_iter().map(|x| x.0).collect();
        assert_eq!(l, ["Parent"], "only themes with a core pointer of their own are listed");
    }
}
