//! Resolution smoke for an installed pack: which of the common icons and cursors the freedesktop lookup finds in it
//! (and in what it inherits), with an optional PNG sheet drawn from exactly those files.

use super::cursor;
use crate::icons::raster::{self, Opts};
use crate::icons::resolve::{expand_chain, Found, Resolver, Roots};
use std::path::{Path, PathBuf};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconHit {
    pub caption: &'static str,
    pub name: &'static str,
    pub path: Option<PathBuf>,
    /// Found in the pack itself (not a parent, hicolor or the fallback).
    pub own: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorHit {
    pub caption: &'static str,
    pub name: Option<&'static str>,
    pub path: Option<PathBuf>,
    pub theme: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preview {
    pub chain: Vec<String>,
    pub icons: Vec<IconHit>,
    pub cursors: Vec<CursorHit>,
}

pub const ICON_PX: u32 = 48;

/// Looks the samples up in `theme` the way the shell does (its chain: inherits, hicolor, no extra fallback).
pub fn resolve(roots: &Roots, theme: &str, icons: bool, cursors: bool) -> Preview {
    let mut p = Preview::default();
    if icons {
        p.chain = expand_chain(roots, &[theme.to_string()], None);
        let mut r = Resolver::new(roots.clone(), p.chain.clone());
        let own_dirs: Vec<PathBuf> = roots.icon_dirs.iter().map(|d| d.join(theme)).collect();
        for (caption, name) in crate::icons::preview::SAMPLES {
            let f = r.lookup(name, ICON_PX, false);
            let own = f.as_ref().is_some_and(|f| own_dirs.iter().any(|d| f.path.starts_with(d)));
            p.icons.push(IconHit { caption, name, path: f.map(|f| f.path), own });
        }
    }
    if cursors {
        for (caption, names) in cursor::SAMPLES {
            let hit = names.iter().find_map(|n| cursor::find(roots, theme, n).map(|(path, t)| (*n, path, t)));
            p.cursors.push(match hit {
                Some((n, path, t)) => CursorHit { caption, name: Some(n), path: Some(path), theme: Some(t) },
                None => CursorHit { caption, name: None, path: None, theme: None },
            });
        }
    }
    p
}

fn kind_of(p: &Path) -> Option<crate::icons::resolve::Kind> {
    use crate::icons::resolve::Kind;
    match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "svg" => Some(Kind::Svg),
        "png" => Some(Kind::Png),
        "xpm" => Some(Kind::Xpm),
        _ => None,
    }
}

/// A sheet: one row of icons (48 px cells), one row of cursors (nominal 32 px), on a neutral background.
/// Cells of samples that were not found stay empty. Returns the pixmap and how many cells were drawn.
pub fn sheet(p: &Preview) -> Option<(Pixmap, usize)> {
    let cell = 64u32;
    let cols = 8u32;
    let icon_rows = (p.icons.len() as u32).div_ceil(cols);
    let cursor_rows = (p.cursors.len() as u32).div_ceil(cols);
    let h = (icon_rows + cursor_rows).max(1) * cell;
    let mut pm = Pixmap::new(cols * cell, h)?;
    pm.fill(tiny_skia::Color::from_rgba8(0xee, 0xee, 0xec, 0xff));
    let mut drawn = 0;
    for (i, hit) in p.icons.iter().enumerate() {
        let Some(path) = &hit.path else { continue };
        let Some(kind) = kind_of(path) else { continue };
        let found = Found { path: path.clone(), kind };
        if let Some(icon) = raster::render(&found, ICON_PX, Opts { content: ICON_PX, tint: None }) {
            let (x, y) = ((i as u32 % cols) * cell + 8, (i as u32 / cols) * cell + 8);
            pm.draw_pixmap(x as i32, y as i32, icon.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            drawn += 1;
        }
    }
    for (i, hit) in p.cursors.iter().enumerate() {
        let Some(path) = &hit.path else { continue };
        let Ok(data) = std::fs::read(path) else { continue };
        let Ok((c, info)) = cursor::decode(&data, 32) else { continue };
        let (cx, cy) = ((i as u32 % cols) * cell + cell / 2, (icon_rows + i as u32 / cols) * cell + cell / 2);
        // the hotspot sits on the cell centre, as the pointer would on a click target
        pm.draw_pixmap(cx as i32 - info.xhot as i32, cy as i32 - info.yhot as i32, c.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        drawn += 1;
    }
    Some((pm, drawn))
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;

    #[test]
    fn a_pack_resolves_its_own_icons_and_cursors_and_draws_a_sheet() {
        let s = Scratch::new("preview");
        s.write("icons/fix/index.theme", ICON_INDEX.as_bytes());
        s.write("icons/fix/apps/48/utilities-terminal.svg", SVG);
        s.write("icons/fix/places/scalable/folder.svg", SVG);
        s.write("icons/fix/cursors/left_ptr", &xcursor(24));
        std::os::unix::fs::symlink("left_ptr", s.path().join("icons/fix/cursors/default")).unwrap();
        s.write("icons/hicolor/index.theme", b"[Icon Theme]\nName=Hicolor\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\nType=Fixed\n");
        s.write("icons/hicolor/48x48/apps/firefox.png", &crate::theme::engine::package::tests::png(48, 48, [200, 80, 20]));
        let roots = Roots { icon_dirs: vec![s.path().join("icons")], pixmaps: vec![] };
        let p = resolve(&roots, "fix", true, true);
        assert_eq!(p.chain, ["fix", "hicolor"]);
        let by = |n: &str| p.icons.iter().find(|h| h.name == n).unwrap();
        assert!(by("utilities-terminal").own && by("folder").own);
        assert!(by("firefox").path.is_some() && !by("firefox").own, "from hicolor");
        assert!(by("user-trash").path.is_none());
        assert_eq!(p.cursors[0].name, Some("default"));
        assert_eq!(p.cursors[0].theme.as_deref(), Some("fix"));
        assert!(p.cursors[1].path.is_none(), "no link cursor in the fixture");
        let (pm, drawn) = sheet(&p).unwrap();
        assert_eq!(drawn, 4, "terminal, folder, firefox and the pointer");
        assert_eq!((pm.width(), pm.height()), (512, 256));
        let px = pm.pixel(8 + 64 + 8, 8 + 8).unwrap();
        assert_ne!((px.red(), px.green(), px.blue()), (0xee, 0xee, 0xec), "the terminal icon was painted");
    }
}
