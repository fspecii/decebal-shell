//! Icon lookup per the freedesktop Icon Theme specification: search roots, theme chains (`Inherits=`, then
//! hicolor, then the configured fallback theme), best directory for a pixel size (scale included), `pixmaps`.
//!
//! Everything here is explicit about its roots (no hidden globals) so the tests can build themes in a temp dir.

use super::cache::Lru;
use super::index::{self, Dir, DirType, Index};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Svg,
    Png,
    Xpm,
}

/// Extensions in lookup order. SVG first: it is crisp at every size when a directory offers both.
const EXTS: [(&str, Kind); 3] = [("svg", Kind::Svg), ("png", Kind::Png), ("xpm", Kind::Xpm)];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub kind: Kind,
}

/// Where themes live: icon base directories in search order, and the unthemed `pixmaps` directories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    pub icon_dirs: Vec<PathBuf>,
    pub pixmaps: Vec<PathBuf>,
}

impl Roots {
    /// `$XDG_DATA_HOME/icons` (default `~/.local/share/icons`), `~/.icons`, `$XDG_DATA_DIRS/icons` (default
    /// `/usr/local/share:/usr/share`) and the matching `pixmaps` directories.
    pub fn from_env() -> Roots {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data_home = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        let data_dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let mut r = Roots::default();
        let push = |v: &mut Vec<PathBuf>, p: PathBuf| {
            if !v.contains(&p) {
                v.push(p);
            }
        };
        if let Some(d) = &data_home {
            push(&mut r.icon_dirs, d.join("icons"));
        }
        if let Some(h) = &home {
            push(&mut r.icon_dirs, h.join(".icons"));
        }
        for d in data_dirs.split(':').filter(|s| !s.is_empty()) {
            push(&mut r.icon_dirs, Path::new(d).join("icons"));
            push(&mut r.pixmaps, Path::new(d).join("pixmaps"));
        }
        r
    }

    /// A cheap change detector: the modification times of the base directories.
    pub fn stamps(&self) -> Vec<crate::config::Stamp> {
        self.icon_dirs.iter().chain(&self.pixmaps).map(|p| crate::config::Stamp::of(p)).collect()
    }
}

/// An installed theme: its parsed index and where its directories are.
#[derive(Debug)]
pub struct Theme {
    pub id: String,
    pub index: Index,
    /// `<root>/<id>` for every root that has it (the first one holds the `index.theme` that was read).
    pub bases: Vec<PathBuf>,
    /// Directories that exist on disk, with their full paths (one per base).
    dirs: Vec<(Dir, Vec<PathBuf>)>,
    /// File stem -> indices (into `dirs`) of the directories holding such a file, built on the first lookup. One
    /// directory listing per directory instead of a `stat` per directory, extension and name makes a miss O(1).
    names: std::sync::OnceLock<HashMap<Box<str>, Box<[u16]>>>,
}

impl Theme {
    pub fn load(roots: &Roots, id: &str) -> Option<Theme> {
        if id.is_empty() || id.contains('/') || id.starts_with('.') {
            return None;
        }
        let bases: Vec<PathBuf> = roots.icon_dirs.iter().map(|r| r.join(id)).filter(|b| b.is_dir()).collect();
        let text = bases.iter().find_map(|b| std::fs::read_to_string(b.join("index.theme")).ok())?;
        let index = index::parse(&text);
        let dirs = index
            .dirs
            .iter()
            .filter_map(|d| {
                let full: Vec<PathBuf> = bases.iter().map(|b| b.join(&d.path)).filter(|p| p.is_dir()).collect();
                (!full.is_empty()).then(|| (d.clone(), full))
            })
            .collect();
        Some(Theme { id: id.to_string(), index, bases, dirs, names: std::sync::OnceLock::new() })
    }

    /// The best file for `name` at `px` physical pixels: directories by `DirectorySizeDistance`, ties to the larger
    /// (shrinking looks better than growing), then scale 1.
    pub fn find(&self, name: &str, px: u32) -> Option<Found> {
        let names = self.names.get_or_init(|| self.scan());
        let mut order: Vec<u16> = names.get(name)?.to_vec();
        order.sort_by_key(|&i| {
            let d = &self.dirs[i as usize].0;
            (d.distance(px), std::cmp::Reverse(d.pixels()), d.scale, d.kind != DirType::Scalable)
        });
        for i in order {
            for dir in &self.dirs[i as usize].1 {
                if let Some(f) = file_in(dir, name) {
                    return Some(f);
                }
            }
        }
        None
    }

    fn scan(&self) -> HashMap<Box<str>, Box<[u16]>> {
        let mut tmp: HashMap<String, Vec<u16>> = HashMap::new();
        for (i, (_, fulls)) in self.dirs.iter().enumerate() {
            for dir in fulls {
                let Ok(rd) = std::fs::read_dir(dir) else { continue };
                for e in rd.flatten() {
                    let file = e.file_name();
                    let file = file.to_string_lossy();
                    let Some((stem, ext)) = file.rsplit_once('.') else { continue };
                    if EXTS.iter().any(|(x, _)| *x == ext) {
                        let v = tmp.entry(stem.to_string()).or_default();
                        if v.last() != Some(&(i as u16)) {
                            v.push(i as u16);
                        }
                    }
                }
            }
        }
        tmp.into_iter().map(|(k, v)| (k.into_boxed_str(), v.into_boxed_slice())).collect()
    }
}

fn file_in(dir: &Path, name: &str) -> Option<Found> {
    for (ext, kind) in EXTS {
        let path = dir.join(format!("{name}.{ext}"));
        if std::fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            return Some(Found { path, kind });
        }
    }
    None
}

/// `first` and everything they inherit (depth first, each once), then `hicolor`, then `fallback` and what it
/// inherits. Names of themes that are not installed stay in the list (they are skipped when looking up).
pub fn expand_chain(roots: &Roots, first: &[String], fallback: Option<&str>) -> Vec<String> {
    fn walk(roots: &Roots, id: &str, out: &mut Vec<String>, depth: usize) {
        if depth > 12 || out.iter().any(|o| o == id) {
            return;
        }
        out.push(id.to_string());
        if let Some(t) = Theme::load(roots, id) {
            for p in &t.index.inherits {
                walk(roots, p, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    for f in first.iter().filter(|f| !f.is_empty()) {
        walk(roots, f, &mut out, 0);
    }
    walk(roots, "hicolor", &mut out, 0);
    if let Some(fb) = fallback.filter(|f| !f.is_empty() && !f.eq_ignore_ascii_case("none")) {
        walk(roots, fb, &mut out, 0);
    }
    out
}

/// Looks icons up through one theme chain. Resolved paths (and misses) are cached in a small LRU.
pub struct Resolver {
    pub roots: Roots,
    pub chain: Vec<String>,
    themes: HashMap<String, Option<Arc<Theme>>>,
    cache: Lru<(String, u32, bool), Option<Found>>,
}

/// Bounds of the path cache: enough for a few hundred apps at a handful of sizes, a few tens of KB.
pub const PATH_CACHE_ENTRIES: usize = 768;

impl Resolver {
    pub fn new(roots: Roots, chain: Vec<String>) -> Resolver {
        Resolver { roots, chain, themes: HashMap::new(), cache: Lru::new(PATH_CACHE_ENTRIES, usize::MAX) }
    }

    pub fn cached_paths(&self) -> usize {
        self.cache.len()
    }

    fn theme(&mut self, id: &str) -> Option<Arc<Theme>> {
        if !self.themes.contains_key(id) {
            let t = Theme::load(&self.roots, id).map(Arc::new);
            self.themes.insert(id.to_string(), t);
        }
        self.themes[id].clone()
    }

    /// One exact icon name over the whole chain, then the unthemed `pixmaps`.
    pub fn exact(&mut self, name: &str, px: u32) -> Option<Found> {
        if name.is_empty() || name.contains("..") || name.contains('/') {
            return None;
        }
        for id in self.chain.clone() {
            if let Some(f) = self.theme(&id).and_then(|t| t.find(name, px)) {
                return Some(f);
            }
        }
        self.roots.pixmaps.iter().find_map(|d| file_in(d, name))
    }

    /// The icon for a dock id / `Icon=` value: every candidate name (aliases, lower case, generalisations) in
    /// order over the chain. `generic` ends with the theme's default application icon.
    pub fn lookup(&mut self, id: &str, px: u32, generic: bool) -> Option<Found> {
        let key = (id.to_string(), px, generic);
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let found = super::names::candidates(id, generic).iter().find_map(|n| self.exact(n, px));
        self.cache.put(key, found.clone(), 1);
        found
    }
}

// ---------------------------------------------------------------- installed themes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    /// Application / folder / MIME icons: selectable.
    Icons,
    /// Only `symbolic` directories (monochrome UI glyphs): useless for a dock.
    SymbolicOnly,
    /// A cursor theme (`cursors/`, no icon directories).
    CursorOnly,
    /// No icon directories (and no cursors): an alias like `default`.
    Empty,
}

impl ThemeKind {
    pub fn selectable(self) -> bool {
        self == ThemeKind::Icons
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Icons => "icons",
            ThemeKind::SymbolicOnly => "symbolic only",
            ThemeKind::CursorOnly => "cursors only",
            ThemeKind::Empty => "no icons",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeInfo {
    /// Directory name: what `icons.theme` holds.
    pub id: String,
    pub name: String,
    pub comment: String,
    pub kind: ThemeKind,
    pub inherits: Vec<String>,
    /// Where the `index.theme` came from.
    pub path: PathBuf,
}

pub fn classify(base: &Path, ix: &Index) -> ThemeKind {
    if ix.dirs.is_empty() {
        return if base.join("cursors").is_dir() { ThemeKind::CursorOnly } else { ThemeKind::Empty };
    }
    if ix.dirs.iter().all(|d| d.path.contains("symbolic") || d.context.to_ascii_lowercase().contains("symbolic")) {
        return ThemeKind::SymbolicOnly;
    }
    ThemeKind::Icons
}

/// Every theme found in the roots (the first root holding an id wins), sorted by display name.
pub fn list_themes(roots: &Roots) -> Vec<ThemeInfo> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for root in &roots.icon_dirs {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for id in names {
            if seen.contains(&id) {
                continue;
            }
            let base = root.join(&id);
            let Ok(text) = std::fs::read_to_string(base.join("index.theme")) else { continue };
            seen.push(id.clone());
            let ix = index::parse(&text);
            out.push(ThemeInfo {
                name: if ix.name.is_empty() { id.clone() } else { ix.name.clone() },
                comment: ix.comment.clone(),
                kind: classify(&base, &ix),
                inherits: ix.inherits.clone(),
                path: base,
                id,
            });
        }
    }
    out.sort_by_key(|t| (!t.kind.selectable(), t.name.to_lowercase()));
    out
}

/// Maps what a user typed (directory name, display name, any case, spaces for dashes) to an installed theme id.
pub fn find_installed(themes: &[ThemeInfo], query: &str) -> Option<String> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let norm = |s: &str| s.to_lowercase().replace([' ', '_'], "-");
    themes
        .iter()
        .find(|t| t.id == q)
        .or_else(|| themes.iter().find(|t| t.id.eq_ignore_ascii_case(q)))
        .or_else(|| themes.iter().find(|t| norm(&t.id) == norm(q) || norm(&t.name) == norm(q)))
        .map(|t| t.id.clone())
}

#[cfg(test)]
pub mod testkit {
    //! A throw-away icon tree for tests.
    use super::*;

    pub struct Tree {
        pub root: PathBuf,
    }

    impl Tree {
        pub fn new(tag: &str) -> Tree {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("macshell-icons-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("icons")).unwrap();
            std::fs::create_dir_all(root.join("pixmaps")).unwrap();
            Tree { root }
        }
        pub fn roots(&self) -> Roots {
            Roots { icon_dirs: vec![self.root.join("icons")], pixmaps: vec![self.root.join("pixmaps")] }
        }
        /// `index` is the whole `index.theme` text.
        pub fn theme(&self, id: &str, index: &str) -> &Tree {
            let d = self.root.join("icons").join(id);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("index.theme"), index).unwrap();
            self
        }
        /// An empty icon file `rel` (like `48x48/apps/foo.png`) inside theme `id`.
        pub fn icon(&self, id: &str, rel: &str) -> &Tree {
            let p = self.root.join("icons").join(id).join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
            self
        }
        pub fn pixmap(&self, file: &str) -> &Tree {
            std::fs::write(self.root.join("pixmaps").join(file), b"x").unwrap();
            self
        }
        pub fn rel(&self, f: &Found) -> String {
            f.path.strip_prefix(self.root.join("icons")).unwrap_or(&f.path).to_string_lossy().into_owned()
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    pub const PLAIN: &str = "[Icon Theme]\nName=Plain\nDirectories=16x16/apps,32x32/apps,64x64/apps,scalable/apps\n\
[16x16/apps]\nSize=16\nType=Fixed\n[32x32/apps]\nSize=32\nType=Fixed\n[64x64/apps]\nSize=64\nType=Fixed\n\
[scalable/apps]\nSize=64\nMinSize=16\nMaxSize=256\nType=Scalable\n";
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    fn resolver(t: &Tree, first: &[&str], fallback: Option<&str>) -> Resolver {
        let chain = expand_chain(&t.roots(), &first.iter().map(|s| s.to_string()).collect::<Vec<_>>(), fallback);
        Resolver::new(t.roots(), chain)
    }

    #[test]
    fn chain_follows_inherits_depth_first_then_hicolor_then_fallback() {
        let t = Tree::new("chain");
        t.theme("A", "[Icon Theme]\nName=A\nInherits=B,C\n").theme("B", "[Icon Theme]\nName=B\nInherits=D\n").theme("C", "[Icon Theme]\nName=C\n").theme("D", "[Icon Theme]\nName=D\nInherits=A\n");
        t.theme("hicolor", PLAIN).theme("Adwaita", PLAIN);
        let c = expand_chain(&t.roots(), &["A".to_string()], Some("Adwaita"));
        assert_eq!(c, ["A", "B", "D", "C", "hicolor", "Adwaita"], "loop A -> B -> D -> A ends, hicolor and the fallback close the chain");
        let c = expand_chain(&t.roots(), &[], None);
        assert_eq!(c, ["hicolor"]);
        let c = expand_chain(&t.roots(), &["hicolor".to_string()], Some("hicolor"));
        assert_eq!(c, ["hicolor"], "no duplicates");
        let c = expand_chain(&t.roots(), &["Ghost".to_string()], Some("none"));
        assert_eq!(c, ["Ghost", "hicolor"], "uninstalled themes stay but find nothing");
    }

    #[test]
    fn an_exact_size_hits_its_directory() {
        let t = Tree::new("size");
        t.theme("T", PLAIN);
        t.icon("T", "16x16/apps/foo.png").icon("T", "32x32/apps/foo.png").icon("T", "64x64/apps/foo.png");
        let mut r = resolver(&t, &["T"], None);
        assert_eq!(t.rel(&r.exact("foo", 32).unwrap()), "T/32x32/apps/foo.png");
        assert_eq!(t.rel(&r.exact("foo", 47).unwrap()), "T/32x32/apps/foo.png", "32 is 15 away, 64 is 17");
        assert_eq!(t.rel(&r.exact("foo", 16).unwrap()), "T/16x16/apps/foo.png");
    }

    #[test]
    fn the_closest_directory_wins_and_scalable_beats_everything_in_range() {
        let t = Tree::new("odd");
        t.theme("T", PLAIN);
        t.icon("T", "16x16/apps/foo.png").icon("T", "32x32/apps/foo.png").icon("T", "64x64/apps/foo.png");
        let mut r = resolver(&t, &["T"], None);
        assert_eq!(t.rel(&r.exact("foo", 48).unwrap()), "T/64x64/apps/foo.png", "tie at distance 16: the larger one");
        assert_eq!(t.rel(&r.exact("foo", 40).unwrap()), "T/32x32/apps/foo.png");
        assert_eq!(t.rel(&r.exact("foo", 36).unwrap()), "T/32x32/apps/foo.png");
        assert_eq!(t.rel(&r.exact("foo", 5).unwrap()), "T/16x16/apps/foo.png");
        assert_eq!(t.rel(&r.exact("foo", 500).unwrap()), "T/64x64/apps/foo.png");
        t.icon("T", "scalable/apps/foo.svg");
        let mut r = resolver(&t, &["T"], None);
        for px in [16, 24, 47, 128, 256] {
            assert_eq!(t.rel(&r.exact("foo", px).unwrap()), "T/scalable/apps/foo.svg", "{px}");
        }
        assert_eq!(t.rel(&r.exact("foo", 8).unwrap()), "T/scalable/apps/foo.svg", "a tie in distance goes to the vector file");
        assert_eq!(r.exact("foo", 47).unwrap().kind, Kind::Svg);
    }

    #[test]
    fn at_2x_directories_count_their_physical_pixels() {
        let t = Tree::new("scale");
        t.theme("S", "[Icon Theme]\nName=S\nDirectories=48x48/apps,48x48@2x/apps\n[48x48/apps]\nSize=48\nType=Fixed\n[48x48@2x/apps]\nSize=48\nScale=2\nType=Fixed\n");
        t.icon("S", "48x48/apps/foo.png").icon("S", "48x48@2x/apps/foo.png");
        let mut r = resolver(&t, &["S"], None);
        assert_eq!(t.rel(&r.exact("foo", 48).unwrap()), "S/48x48/apps/foo.png");
        assert_eq!(t.rel(&r.exact("foo", 96).unwrap()), "S/48x48@2x/apps/foo.png", "a 96 px request takes the @2x directory");
        assert_eq!(t.rel(&r.exact("foo", 90).unwrap()), "S/48x48@2x/apps/foo.png");
    }

    #[test]
    fn missing_icons_fall_through_inherits_then_hicolor_then_fallback() {
        let t = Tree::new("fall");
        t.theme("Child", &PLAIN.replace("Plain", "Child").replace("[Icon Theme]\n", "[Icon Theme]\nInherits=Parent\n"));
        t.theme("Parent", PLAIN).theme("hicolor", PLAIN).theme("Adwaita", PLAIN);
        t.icon("Child", "scalable/apps/own.svg");
        t.icon("Parent", "scalable/apps/inherited.svg").icon("Parent", "scalable/apps/own.svg");
        t.icon("hicolor", "32x32/apps/hi.png").icon("Adwaita", "scalable/apps/adw.svg");
        let mut r = resolver(&t, &["Child"], Some("Adwaita"));
        assert_eq!(t.rel(&r.exact("own", 48).unwrap()), "Child/scalable/apps/own.svg", "the selected theme wins over its parent");
        assert_eq!(t.rel(&r.exact("inherited", 48).unwrap()), "Parent/scalable/apps/inherited.svg");
        assert_eq!(t.rel(&r.exact("hi", 48).unwrap()), "hicolor/32x32/apps/hi.png");
        assert_eq!(t.rel(&r.exact("adw", 48).unwrap()), "Adwaita/scalable/apps/adw.svg");
        assert!(r.exact("nothing", 48).is_none());
        let mut r = resolver(&t, &["Child"], None);
        assert!(r.exact("adw", 48).is_none(), "no fallback theme configured");
    }

    #[test]
    fn unthemed_pixmaps_come_last_and_all_formats_are_found() {
        let t = Tree::new("pix");
        t.theme("hicolor", PLAIN);
        t.pixmap("old.xpm").pixmap("plain.png").pixmap("vec.svg");
        t.icon("hicolor", "32x32/apps/plain.png");
        let mut r = resolver(&t, &[], None);
        assert_eq!(r.exact("old", 32).unwrap().kind, Kind::Xpm);
        assert_eq!(t.rel(&r.exact("plain", 32).unwrap()), "hicolor/32x32/apps/plain.png", "a theme beats pixmaps");
        assert_eq!(r.exact("vec", 32).unwrap().kind, Kind::Svg);
        assert!(r.exact("../etc/passwd", 32).is_none() && r.exact("a/b", 32).is_none() && r.exact("", 32).is_none());
    }

    #[test]
    fn lookup_uses_aliases_mime_fallbacks_and_the_generic_icon() {
        let t = Tree::new("names");
        t.theme("hicolor", PLAIN);
        t.icon("hicolor", "scalable/apps/user-trash.svg").icon("hicolor", "scalable/apps/text-x-generic.svg").icon("hicolor", "scalable/apps/application-x-executable.svg");
        t.icon("hicolor", "scalable/apps/firefox.svg");
        let mut r = resolver(&t, &[], None);
        assert_eq!(t.rel(&r.lookup("trash", 48, false).unwrap()), "hicolor/scalable/apps/user-trash.svg");
        assert_eq!(t.rel(&r.lookup("text-x-python", 48, false).unwrap()), "hicolor/scalable/apps/text-x-generic.svg");
        assert_eq!(t.rel(&r.lookup("org.mozilla.firefox", 48, false).unwrap()), "hicolor/scalable/apps/firefox.svg");
        assert_eq!(t.rel(&r.lookup("Firefox", 48, false).unwrap()), "hicolor/scalable/apps/firefox.svg", "case-insensitive fallback");
        assert!(r.lookup("unknown-thing", 48, false).is_none());
        assert_eq!(t.rel(&r.lookup("unknown-thing", 48, true).unwrap()), "hicolor/scalable/apps/application-x-executable.svg");
        assert_eq!(t.rel(&r.lookup("application-x-foo", 48, false).unwrap()), "hicolor/scalable/apps/application-x-executable.svg");
    }

    #[test]
    fn path_cache_is_bounded_and_remembers_misses() {
        let t = Tree::new("bound");
        t.theme("hicolor", PLAIN);
        t.icon("hicolor", "scalable/apps/foo.svg");
        let mut r = resolver(&t, &[], None);
        for i in 0..(PATH_CACHE_ENTRIES * 3) {
            let _ = r.lookup(&format!("missing-{i}"), 48, false);
        }
        assert!(r.cached_paths() <= PATH_CACHE_ENTRIES, "{}", r.cached_paths());
        // a cached answer survives the file disappearing (the cache is dropped on a config change / stamp change)
        assert!(r.lookup("foo", 48, false).is_some());
        std::fs::remove_file(t.root.join("icons/hicolor/scalable/apps/foo.svg")).unwrap();
        assert!(r.lookup("foo", 48, false).is_some());
        let mut fresh = resolver(&t, &[], None);
        assert!(fresh.lookup("foo", 48, false).is_none());
    }

    #[test]
    fn themes_are_listed_with_their_kind() {
        let t = Tree::new("list");
        t.theme("Colourful", &PLAIN.replace("Plain", "Colourful Icons").replace("Name=", "Comment=Pretty\nName="));
        t.theme("Mono", "[Icon Theme]\nName=Mono\nDirectories=symbolic/apps\n[symbolic/apps]\nSize=16\nType=Scalable\n");
        t.theme("Pointer", "[Icon Theme]\nName=Pointer\n").icon("Pointer", "cursors/left_ptr");
        t.theme("Hidden", "[Icon Theme]\nName=Hid\nHidden=true\nDirectories=a\n[a]\nSize=1\n");
        t.theme("alias", "[Icon Theme]\nInherits=Colourful\n");
        std::fs::create_dir_all(t.root.join("icons/NoIndex")).unwrap();
        let l = list_themes(&t.roots());
        let by = |id: &str| l.iter().find(|x| x.id == id).unwrap_or_else(|| panic!("{id} not listed: {l:?}"));
        assert_eq!(by("Colourful").kind, ThemeKind::Icons);
        assert_eq!((by("Colourful").name.as_str(), by("Colourful").comment.as_str()), ("Colourful Icons", "Pretty"));
        assert_eq!(by("Mono").kind, ThemeKind::SymbolicOnly);
        assert_eq!(by("Pointer").kind, ThemeKind::CursorOnly);
        assert_eq!(by("Hidden").kind, ThemeKind::Icons, "Hidden=true only keeps a theme out of GTK pickers (Adwaita is hidden); its icons are fine");
        assert_eq!(by("alias").kind, ThemeKind::Empty);
        assert!(l.iter().all(|x| x.id != "NoIndex"), "a folder without index.theme is not a theme");
        assert!(l[0].kind.selectable() && !l.last().unwrap().kind.selectable(), "selectable themes first");
        assert!(by("Mono").kind.label().contains("symbolic") && by("Pointer").kind.label().contains("cursor"));
    }

    #[test]
    fn installed_themes_are_found_by_id_or_display_name() {
        let t = Tree::new("find");
        t.theme("WhiteSur-dark", "[Icon Theme]\nName=WhiteSur-dark\nDirectories=a\n[a]\nSize=48\n").theme("Tela-blue", "[Icon Theme]\nName=Tela blue\nDirectories=a\n[a]\nSize=48\n");
        let l = list_themes(&t.roots());
        assert_eq!(find_installed(&l, "WhiteSur-dark").as_deref(), Some("WhiteSur-dark"));
        assert_eq!(find_installed(&l, "whitesur-DARK").as_deref(), Some("WhiteSur-dark"));
        assert_eq!(find_installed(&l, "Tela blue").as_deref(), Some("Tela-blue"));
        assert_eq!(find_installed(&l, "tela_blue").as_deref(), Some("Tela-blue"));
        assert_eq!(find_installed(&l, "nope"), None);
        assert_eq!(find_installed(&l, "  "), None);
    }

    #[test]
    fn the_first_root_wins_and_directories_merge_across_roots() {
        let a = Tree::new("rootA");
        let b = Tree::new("rootB");
        a.theme("T", PLAIN).icon("T", "scalable/apps/in-a.svg");
        b.theme("T", "[Icon Theme]\nName=Other\nDirectories=scalable/apps\n[scalable/apps]\nSize=64\nType=Scalable\nMinSize=1\nMaxSize=512\n").icon("T", "scalable/apps/in-b.svg");
        let roots = Roots { icon_dirs: vec![a.root.join("icons"), b.root.join("icons")], pixmaps: vec![] };
        let l = list_themes(&roots);
        assert_eq!(l.iter().filter(|x| x.id == "T").count(), 1);
        assert_eq!(l[0].name, "Plain", "the first root's index.theme is used");
        let mut r = Resolver::new(roots, vec!["T".into()]);
        assert!(r.exact("in-a", 48).is_some());
        assert!(r.exact("in-b", 48).is_some(), "the same directory in a later root is searched too");
    }
}
