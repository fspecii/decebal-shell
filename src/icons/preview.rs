//! Icons of a theme that is not (yet) the one in use, for the settings pane's preview grid. Own tiny resolvers
//! and a small bounded pixmap cache; both are dropped whenever the icon settings change.

use super::cache::Lru;
use super::resolve::{Resolver, Roots};
use std::collections::HashMap;
use std::sync::Mutex;
use tiny_skia::Pixmap;

/// (caption, icon name): the common application, folder and MIME icons the grid shows.
pub const SAMPLES: [(&str, &str); 16] = [
    ("Firefox", "firefox"),
    ("Terminal", "utilities-terminal"),
    ("Files", "system-file-manager"),
    ("Settings", "preferences-system"),
    ("Trash", "user-trash"),
    ("Folder", "folder"),
    ("Image", "image-x-generic"),
    ("Music", "audio-x-generic"),
    ("Video", "video-x-generic"),
    ("Text", "text-x-generic"),
    ("PDF", "application-pdf"),
    ("Archive", "package-x-generic"),
    ("Mail", "mail-client"),
    ("Calculator", "accessories-calculator"),
    ("Games", "applications-games"),
    ("Program", "application-x-executable"),
];

struct Preview {
    gen: u64,
    resolvers: HashMap<String, Resolver>,
    pix: Lru<(String, String, u32), Option<Pixmap>>,
}

static PREVIEW: Mutex<Option<Preview>> = Mutex::new(None);

/// Pixmaps kept: the 16 samples of a few themes. Bounded by count and bytes.
pub const PIXMAPS: usize = 128;
pub const BYTES: usize = 6 << 20;

/// `name` from theme `theme` (`None` = the one in use) at `px`, drawn the way the shell would (size scale, tint).
pub fn icon(theme: Option<&str>, name: &str, px: u32) -> Option<Pixmap> {
    let gen = super::live::generation();
    let mut g = PREVIEW.lock().unwrap_or_else(|e| e.into_inner());
    let p = g.get_or_insert_with(|| Preview { gen, resolvers: HashMap::new(), pix: Lru::new(PIXMAPS, BYTES) });
    if p.gen != gen {
        p.gen = gen;
        p.resolvers.clear();
        p.pix.clear();
    }
    let key = (theme.unwrap_or("\0").to_string(), name.to_string(), px);
    if let Some(hit) = p.pix.get(&key) {
        return hit.clone();
    }
    let (scale, tint) = super::live::render_settings();
    let content = ((px as f32 * scale).round() as u32).max(1);
    if p.resolvers.len() >= 3 && !p.resolvers.contains_key(&key.0) {
        p.resolvers.clear();
    }
    let r = p.resolvers.entry(key.0.clone()).or_insert_with(|| {
        let chain = match theme {
            Some(t) => super::live::chain_for(Some(t)),
            None => super::live::status().chain,
        };
        Resolver::new(Roots::from_env(), chain)
    });
    let pm = r.lookup(name, content, false).and_then(|f| super::raster::render(&f, px, super::raster::Opts { content, tint: tint.then(|| crate::theme::th().text.t()) }));
    let bytes = pm.as_ref().map_or(1, |p| p.data().len());
    p.pix.put(key, pm.clone(), bytes);
    pm
}

/// For tests: how many pixmaps are cached.
pub fn cached() -> usize {
    PREVIEW.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(0, |p| p.pix.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_has_sixteen_distinct_names() {
        let mut n: Vec<_> = SAMPLES.iter().map(|s| s.1).collect();
        n.sort();
        n.dedup();
        assert_eq!(n.len(), 16);
    }

    #[test]
    fn previews_are_cached_within_bounds() {
        for i in 0..400 {
            let _ = icon(Some("hicolor"), &format!("nothing-{i}"), 48);
        }
        assert!(cached() <= PIXMAPS, "{}", cached());
    }
}
