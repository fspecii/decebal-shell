//! Bounded application-provided icon directory lookup, performed only by the bus worker.
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
const MAX_FILE: u64 = 2 << 20;
fn bounded(path: &Path) -> Option<Vec<u8>> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(MAX_FILE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= MAX_FILE as usize).then_some(bytes)
}
fn safe_relative(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && Path::new(s).components().all(|c| matches!(c, Component::Normal(_)))
}
fn decode(path: &Path) -> Option<Pixmap> {
    let bytes = bounded(path)?;
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => {
            if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
                return None;
            }
            let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
            let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
            if !(1..=512).contains(&w) || !(1..=512).contains(&h) {
                return None;
            }
            crate::icons::raster::fit(&Pixmap::decode_png(&bytes).ok()?, 32, 32)
        }
        "svg" => {
            // An icon may draw paths and gradients, but cannot embed another unbounded bitmap/file.
            let text = std::str::from_utf8(&bytes).ok()?;
            if text.to_ascii_lowercase().contains("<image") || text.contains("<!ENTITY") {
                return None;
            }
            let options =
                resvg::usvg::Options { image_href_resolver: resvg::usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) }, ..Default::default() };
            let tree = resvg::usvg::Tree::from_data(&bytes, &options).ok()?;
            let mut pm = Pixmap::new(32, 32)?;
            let size = tree.size();
            let scale = (32.0 / size.width()).min(32.0 / size.height());
            let t = Transform::from_scale(scale, scale).post_translate(((32.0 - size.width() * scale) / 2.0).floor(), ((32.0 - size.height() * scale) / 2.0).floor());
            resvg::render(&tree, t, &mut pm.as_mut());
            Some(pm)
        }
        "xpm" => {
            let text = std::str::from_utf8(&bytes).ok()?;
            let header = text.split('"').nth(1)?;
            let mut dims = header.split_whitespace().map(str::parse::<u32>);
            if !(1..=512).contains(&dims.next()?.ok()?) || !(1..=512).contains(&dims.next()?.ok()?) {
                return None;
            }
            let pm = crate::icons::xpm::parse(text)?;
            if pm.width() > 512 || pm.height() > 512 {
                return None;
            }
            crate::icons::raster::fit(&pm, 32, 32)
        }
        _ => None,
    }
}
pub(super) fn custom(root: &str, name: &str) -> Option<Arc<Pixmap>> {
    if root.len() > 4096 || !Path::new(root).is_absolute() || !safe_relative(name) || name.contains('/') || name == "." || name == ".." {
        return None;
    }
    let root = std::fs::canonicalize(root).ok()?;
    if !root.is_dir() {
        return None;
    }
    let mut dirs = vec![root.clone()];
    if let Some(bytes) = bounded(&root.join("index.theme")) {
        let index = crate::icons::index::parse(std::str::from_utf8(&bytes).ok()?);
        let mut theme_dirs: Vec<_> = index.dirs.into_iter().take(128).filter(|d| safe_relative(&d.path)).collect();
        theme_dirs.sort_by_key(|d| d.distance(32));
        dirs.extend(theme_dirs.into_iter().map(|d| root.join(d.path)));
    }
    // Common appindicator paths omit index.theme while shipping ordinary size/context directories.
    for d in [
        "32x32/status",
        "32x32/apps",
        "24x24/status",
        "24x24/apps",
        "scalable/status",
        "scalable/apps",
        "hicolor/32x32/status",
        "hicolor/32x32/apps",
        "hicolor/scalable/status",
        "hicolor/scalable/apps",
    ] {
        dirs.push(root.join(d));
    }
    let names: Vec<String> = if Path::new(name).extension().is_some() { vec![name.into()] } else { ["png", "svg", "xpm"].into_iter().map(|e| format!("{name}.{e}")).collect() };
    for dir in dirs {
        for name in &names {
            let candidate: PathBuf = dir.join(name);
            if let Ok(canonical) = candidate.canonicalize() {
                if canonical.starts_with(&root) {
                    if let Some(icon) = decode(&canonical) {
                        return Some(Arc::new(icon));
                    }
                }
            }
        }
    }
    None
}
pub(super) fn overlay(base: Arc<Pixmap>, badge: Option<Arc<Pixmap>>) -> Arc<Pixmap> {
    let Some(badge) = badge else { return base };
    let side = (base.width().min(base.height()) / 2).max(1);
    let Some(small) = crate::icons::raster::fit(&badge, side, side) else { return base };
    let mut result = (*base).clone();
    result.draw_pixmap((result.width() - side) as i32, (result.height() - side) as i32, small.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    Arc::new(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlay_is_premultiplied_and_confined_to_corner() {
        let mut a = Pixmap::new(4, 4).unwrap();
        a.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let mut b = Pixmap::new(4, 4).unwrap();
        b.fill(tiny_skia::Color::from_rgba8(0, 255, 0, 128));
        let p = overlay(Arc::new(a), Some(Arc::new(b)));
        assert_eq!(&p.data()[..4], &[255, 0, 0, 255]);
        assert_eq!(&p.data()[60..64], &[127, 128, 0, 255]);
    }
    #[test]
    fn custom_index_svg_and_bounded_png() {
        let root = std::env::temp_dir().join(format!("decebal-sni-icon-{}-{}", std::process::id(), super::super::NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(root.join("scalable/status")).unwrap();
        std::fs::write(root.join("index.theme"), "[Icon Theme]\nName=Test\nDirectories=../outside,scalable/status\n[../outside]\nSize=32\n[scalable/status]\nSize=32\nType=Scalable\n").unwrap();
        std::fs::write(root.join("scalable/status/test.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><path fill="#123456" d="M0 0h32v32H0z"/></svg>"##).unwrap();
        let icon = custom(root.to_str().unwrap(), "test").unwrap();
        assert_eq!(&icon.data()[..4], &[18, 52, 86, 255]);
        let mut oversized = Pixmap::new(513, 1).unwrap();
        oversized.fill(tiny_skia::Color::WHITE);
        oversized.save_png(root.join("too-big.png")).unwrap();
        assert!(custom(root.to_str().unwrap(), "too-big").is_none());
        std::fs::write(root.join("embed.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><image href="file:///tmp/example.png"/></svg>"##).unwrap();
        assert!(custom(root.to_str().unwrap(), "embed").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn relative_paths_never_escape() {
        assert!(!safe_relative("../foo"));
        assert!(!safe_relative("/foo"));
        assert!(custom("/tmp", "../foo").is_none());
    }
}
