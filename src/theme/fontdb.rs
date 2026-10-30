//! The fonts installed on this machine.
//!
//! One `fc-list` call (plus `fc-match` for the generic `sans-serif` / `monospace` aliases, run in parallel
//! with it) is parsed into a table of faces: family names, weight, slant, width, file and index. Everything
//! else (is a family installed, which file is its bold face, which families does the picker offer) is answered
//! from that table, so no process runs per lookup. The table is shared while somebody holds it ([`get`]);
//! the settings window keeps one and scans again when it is old ([`rescan_if_older`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Case, space and punctuation-insensitive family key: "JetBrains Mono" == "JetBrainsMono".
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// fontconfig weight values for the four weights decebal draws with.
pub const FC_REGULAR: i32 = 80;
pub const FC_MEDIUM: i32 = 100;
pub const FC_SEMIBOLD: i32 = 180;
pub const FC_BOLD: i32 = 200;

/// The CSS-style weight (400 ... 700) for a variable font's `wght` axis.
pub fn css_weight(fc: i32) -> f32 {
    match fc {
        ..=60 => 300.0,
        61..=90 => 400.0,
        91..=140 => 500.0,
        141..=190 => 600.0,
        _ => 700.0,
    }
}

/// One entry of `fc-list`.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// `[family, alias...]`; fontconfig lists style-specific family names ("Inter Black") after the family.
    pub names: Vec<String>,
    pub weight_lo: i32,
    pub weight_hi: i32,
    pub slant: i32,
    pub width: i32,
    pub file: PathBuf,
    /// Raw fontconfig index: the face inside a collection in the low 16 bits, a named variation instance above.
    pub index: u32,
    /// A variable font's axis-range entry.
    pub variable: bool,
    pub mono: bool,
}

/// Where a face lives: enough to open it.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceRef {
    pub file: PathBuf,
    /// Face inside a .ttc (the variation-instance bits are already removed).
    pub index: u32,
    /// The file is a variable font: set `wght` to get the requested weight.
    pub variable: bool,
    pub weight: i32,
}

/// Package font inputs are immutable snapshots shared by all icon caches in this process.
/// Metadata/version changes invalidate both positive and negative cache entries.
const MAX_PACKAGE_FONT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PACKAGE_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_PACKAGE_CACHE_ENTRIES: usize = 32;
type PackageVersion = (u64, u128);
type PackageKey = (PathBuf, PackageVersion);

struct PackageFont {
    bytes: Arc<[u8]>,
    face: Face,
}
#[derive(Default)]
struct PackageFonts {
    entries: HashMap<PackageKey, Option<Arc<PackageFont>>>,
    order: std::collections::VecDeque<PackageKey>,
    bytes: usize,
    #[cfg(test)]
    loads: usize,
}
impl PackageFonts {
    fn load(&mut self, path: &Path, version: PackageVersion) -> Option<Arc<PackageFont>> {
        let key = (path.to_path_buf(), version);
        if let Some(value) = self.entries.get(&key) {
            let value = value.clone();
            self.order.retain(|k| k != &key);
            self.order.push_back(key);
            return value;
        }
        #[cfg(test)]
        {
            self.loads += 1;
        }
        let loaded = match read_package_font(path, version) {
            Ok(font) => Some(Arc::new(font)),
            Err(problem) => {
                crate::log_warn!("package font {} rejected: {problem}", path.display());
                None
            }
        };
        self.bytes += loaded.as_ref().map_or(0, |f| f.bytes.len());
        self.entries.insert(key.clone(), loaded.clone());
        self.order.push_back(key);
        while self.entries.len() > MAX_PACKAGE_CACHE_ENTRIES || self.bytes > MAX_PACKAGE_CACHE_BYTES {
            if let Some(key) = self.order.pop_front() {
                if let Some(Some(font)) = self.entries.remove(&key) {
                    self.bytes -= font.bytes.len();
                }
            } else {
                break;
            }
        }
        loaded
    }
}
static PACKAGE_FONTS: std::sync::OnceLock<Mutex<PackageFonts>> = std::sync::OnceLock::new();

/// Source reference failures happen before a canonical file can enter the byte cache.
/// Bound and deduplicate their diagnostics without probing rejected/outside paths.
pub fn report_package_reference(path: &Path, problem: &str) {
    static REPORTED: std::sync::OnceLock<Mutex<std::collections::VecDeque<PathBuf>>> = std::sync::OnceLock::new();
    let mut seen = REPORTED.get_or_init(|| Mutex::new(std::collections::VecDeque::new())).lock().unwrap_or_else(|p| p.into_inner());
    if seen.iter().any(|p| p == path) {
        return;
    }
    if seen.len() == 128 {
        seen.pop_front();
    }
    seen.push_back(path.to_path_buf());
    crate::log_warn!("package font reference {} rejected: {problem}", path.display());
}

fn metadata_version(meta: &std::fs::Metadata) -> PackageVersion {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    meta.modified().ok().hash(&mut hash);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Atomic replacement with retained size/mtime is still a different input.
        (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()).hash(&mut hash);
    }
    (meta.len(), u128::from(hash.finish()))
}
/// Cheap version probe for the theme loader; the value is opaque, not a timestamp.
pub fn package_file_version(path: &Path) -> PackageVersion {
    std::fs::metadata(path).map(|m| metadata_version(&m)).unwrap_or_default()
}

fn read_package_font(path: &Path, version: PackageVersion) -> Result<PackageFont, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() == 0 || meta.len() > MAX_PACKAGE_FONT_BYTES {
        return Err(format!("size {} outside 1..={MAX_PACKAGE_FONT_BYTES} bytes", meta.len()));
    }
    if metadata_version(&meta) != version {
        return Err("changed before loading; retry on next theme refresh".into());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    (&file).take(MAX_PACKAGE_FONT_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != meta.len() || metadata_version(&file.metadata().map_err(|e| e.to_string())?) != version {
        return Err("changed while loading; retry on next theme refresh".into());
    }
    let face = ttf_parser::Face::parse(&bytes, 0).map_err(|e| format!("invalid font: {e:?}"))?;
    let family = |id| face.names().into_iter().filter(|n| n.name_id == id).find_map(|n| n.to_string());
    let name = family(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| family(ttf_parser::name_id::FAMILY)).ok_or("missing font family")?;
    let weight = match face.weight().to_number() {
        0..=349 => 50,
        350..=449 => FC_REGULAR,
        450..=549 => FC_MEDIUM,
        550..=649 => FC_SEMIBOLD,
        _ => FC_BOLD,
    };
    let metadata = Face {
        names: vec![name],
        weight_lo: weight,
        weight_hi: weight,
        slant: if face.is_italic() { 100 } else { 0 },
        width: 100,
        file: path.to_path_buf(),
        index: 0,
        variable: face.is_variable(),
        mono: face.is_monospaced(),
    };
    Ok(PackageFont { bytes: Arc::from(bytes), face: metadata })
}

/// Lookup only: callers prepare metadata/bytes during theme loading, never from paint.
/// A missing entry does not fall back to filesystem I/O.
pub fn package_font_bytes(path: &Path, version: PackageVersion) -> Option<Arc<[u8]>> {
    PACKAGE_FONTS.get()?.lock().unwrap_or_else(|p| p.into_inner()).entries.get(&(path.to_path_buf(), version))?.as_ref().map(|font| font.bytes.clone())
}

/// Suffixes that may follow a requested family name when matching "Noto Sans CJK" to "Noto Sans CJK JP".
const SUFFIXES: &[&str] = &["jp", "kr", "sc", "tc", "hk", "mono", "propo", "nerdfont", "nerdfontmono", "nerdfontpropo", "nf", "nfm", "nfp", "variable", "ui", "display", "text", "regular"];

/// Script-specific Noto families are hidden from the picker (they are picked automatically for their script).
const NOTO_SHOWN: &[&str] = &["", "display", "mono", "symbols", "symbols2", "math", "cjkjp", "cjkkr", "cjksc", "cjktc", "cjkhk", "monocjkjp", "monocjkkr", "monocjksc", "monocjktc", "monocjkhk"];

#[derive(Debug, Clone, Default)]
pub struct FontDb {
    faces: Vec<Face>,
    by_norm: HashMap<String, Vec<usize>>,
    /// Picker list: first family name of every face, deduplicated, sorted.
    families: Vec<String>,
    variable_files: HashSet<PathBuf>,
    sans: String,
    mono: String,
    /// Language of the session (`ja`, `ko`, `zh-tw`...): decides which Noto CJK variant "Noto Sans CJK" means.
    lang: String,
}

fn run(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd).args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn parse_weight(s: &str) -> (i32, i32) {
    let t = s.trim().trim_start_matches('[').trim_end_matches(']');
    let mut it = t.split_whitespace().filter_map(|x| x.parse::<f32>().ok());
    let lo = it.next().unwrap_or(80.0) as i32;
    (lo, it.next().map_or(lo, |h| h as i32))
}

impl FontDb {
    /// Runs fc-list (and two fc-match calls) once.
    pub fn scan() -> FontDb {
        let (list, sans, mono) = std::thread::scope(|sc| {
            let l = sc.spawn(|| run("fc-list", &["-f", "%{file}\t%{family}\t%{weight}\t%{slant}\t%{width}\t%{index}\t%{variable}\t%{spacing}\n"]));
            let s = sc.spawn(|| run("fc-match", &["-f", "%{family[0]}", "sans-serif"]));
            let m = sc.spawn(|| run("fc-match", &["-f", "%{family[0]}", "monospace"]));
            (l.join().unwrap_or_default(), s.join().unwrap_or_default(), m.join().unwrap_or_default())
        });
        let lang = std::env::var("LC_ALL").ok().filter(|l| !l.is_empty()).or_else(|| std::env::var("LANG").ok()).unwrap_or_default();
        FontDb::from_listing(&list, sans.trim(), mono.trim(), &lang)
    }

    /// Builds the table from `fc-list -f '%{file}\t%{family}\t%{weight}\t%{slant}\t%{width}\t%{index}\t%{variable}\t%{spacing}\n'`
    /// output (tests feed a fake list).
    pub fn from_listing(listing: &str, sans: &str, mono: &str, lang: &str) -> FontDb {
        let mut db = FontDb { sans: sans.to_string(), mono: mono.to_string(), lang: lang.to_ascii_lowercase().replace('_', "-"), ..FontDb::default() };
        for line in listing.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 || f[1].trim().is_empty() {
                continue;
            }
            let names: Vec<String> = f[1].split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
            if names.is_empty() {
                continue;
            }
            let (weight_lo, weight_hi) = parse_weight(f[2]);
            let face = Face {
                names,
                weight_lo,
                weight_hi,
                slant: f[3].trim().parse().unwrap_or(0),
                width: f[4].trim().parse().unwrap_or(100),
                file: PathBuf::from(f[0]),
                index: f[5].trim().parse().unwrap_or(0),
                variable: f[6].trim().eq_ignore_ascii_case("true"),
                mono: f.get(7).is_some_and(|s| s.trim() == "100" || s.trim() == "90"),
            };
            if face.variable {
                db.variable_files.insert(face.file.clone());
            }
            db.faces.push(face);
        }
        db.index();
        db
    }

    /// Adds bounded validated theme files to this process only. Never calls fc-cache,
    /// changes installed fonts, or runs during steady-state glyph painting.
    pub fn with_package_files(&self, files: &[PathBuf]) -> FontDb {
        self.with_package_faces(&Self::read_package_faces(files))
    }

    /// Parse each immutable file version once, outside native drawing.
    pub fn read_package_faces(files: &[PathBuf]) -> Vec<Face> {
        let versions: Vec<_> = files.iter().map(|p| package_file_version(p)).collect();
        Self::read_package_faces_versioned(files, &versions)
    }

    pub fn read_package_faces_versioned(files: &[PathBuf], versions: &[(u64, u128)]) -> Vec<Face> {
        let mut cache = PACKAGE_FONTS.get_or_init(|| Mutex::new(PackageFonts::default())).lock().unwrap_or_else(|p| p.into_inner());
        files.iter().zip(versions).take(8).filter_map(|(path, version)| cache.load(path, *version).map(|font| font.face.clone())).collect()
    }

    /// Pure merge of metadata prepared by the theme loader.
    pub fn with_package_faces(&self, faces: &[Face]) -> FontDb {
        let mut db = self.clone();
        for face in faces.iter().filter(|f| f.variable) {
            db.variable_files.insert(face.file.clone());
        }
        let bundled = faces.iter().cloned();
        db.faces.splice(0..0, bundled);
        db.by_norm.clear();
        db.families.clear();
        db.index();
        db
    }

    /// Builds the name lookup and the picker list from `faces`.
    fn index(&mut self) {
        let mut seen = HashSet::new();
        for (i, f) in self.faces.iter().enumerate() {
            for n in &f.names {
                self.by_norm.entry(norm(n)).or_default().push(i);
            }
            if seen.insert(norm(&f.names[0])) {
                self.families.push(f.names[0].clone());
            }
        }
        self.families.sort_by_key(|f| f.to_lowercase());
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// A table with only the faces of `families` (installed spellings, as [`Self::resolve`] returns them): all a
    /// resolved font chain needs to open its faces, a small fraction of a machine's thousands of faces.
    /// [`Self::resolve`] and [`Self::picker_families`] on it only know those families.
    pub fn subset(&self, families: &[String]) -> FontDb {
        let want: HashSet<String> = families.iter().map(|f| norm(f)).collect();
        let faces: Vec<Face> = self.faces.iter().filter(|f| f.names.iter().any(|n| want.contains(&norm(n)))).cloned().collect();
        let mut db = FontDb { sans: self.sans.clone(), mono: self.mono.clone(), lang: self.lang.clone(), ..FontDb::default() };
        db.variable_files = faces.iter().filter(|f| f.variable).map(|f| f.file.clone()).collect();
        db.faces = faces;
        db.index();
        db
    }

    /// Every family for the picker (script-specific Noto fonts left out), sorted.
    pub fn picker_families(&self) -> Vec<String> {
        self.families.iter().filter(|f| !hidden_in_picker(f)).cloned().collect()
    }

    pub fn family_count(&self) -> usize {
        self.families.len()
    }

    /// The family `wanted` stands for if it is installed (a generic alias, an exact name, or "Noto Sans CJK" for
    /// "Noto Sans CJK JP"): the name as fontconfig spells it.
    pub fn resolve(&self, wanted: &str) -> Option<String> {
        let w = wanted.trim();
        let generic = match norm(w).as_str() {
            "sansserif" | "sans" | "systemui" | "system" | "default" => Some(self.sans.as_str()),
            "monospace" | "mono" | "ui-monospace" => Some(self.mono.as_str()),
            "emoji" | "coloremoji" => return self.emoji(),
            "nerdfont" | "nerdfonts" | "symbolsnerdfont" => return self.nerd(),
            _ => None,
        };
        match generic {
            Some("") => None,
            Some(g) => self.resolve_plain(g).or_else(|| Some(g.to_string())),
            None => self.resolve_plain(w),
        }
    }

    /// An icon font: Symbols Nerd Font, else any Nerd Font patched family.
    fn nerd(&self) -> Option<String> {
        self.resolve_plain("Symbols Nerd Font Mono")
            .or_else(|| self.resolve_plain("Symbols Nerd Font"))
            .or_else(|| self.families.iter().find(|f| norm(f).contains("nerdfont") && !norm(f).contains("propo")).cloned())
    }

    fn emoji(&self) -> Option<String> {
        self.resolve_plain("Noto Color Emoji").or_else(|| self.families.iter().find(|f| f.to_lowercase().contains("emoji")).cloned())
    }

    fn resolve_plain(&self, w: &str) -> Option<String> {
        let key = norm(w);
        if key.is_empty() {
            return None;
        }
        if let Some(ix) = self.by_norm.get(&key) {
            // the family's own name when an alias was typed ("JetBrainsMono NF" -> "JetBrainsMono Nerd Font")
            return Some(self.faces[ix[0]].names[0].clone());
        }
        // "Noto Sans CJK" -> "Noto Sans CJK JP": a short suffix list keeps "Noto Sans" from meaning "Noto Sans Adlam"
        let mut best: Option<(usize, &str)> = None;
        for fam in &self.families {
            let fk = norm(fam);
            let Some(rest) = fk.strip_prefix(&key) else { continue };
            let Some(rank) = self.suffix_rank(rest) else { continue };
            if best.is_none_or(|(r, _)| rank < r) {
                best = Some((rank, fam));
            }
        }
        best.map(|(_, f)| f.to_string())
    }

    /// Order of preference among the suffixes; `None` when `rest` is not an accepted suffix.
    fn suffix_rank(&self, rest: &str) -> Option<usize> {
        let pos = SUFFIXES.iter().position(|s| *s == rest)?;
        // CJK variants: the session language first
        let cjk = ["jp", "sc", "kr", "tc", "hk"];
        let prefer = if self.lang.starts_with("ko") {
            "kr"
        } else if self.lang.starts_with("zh-tw") || self.lang.starts_with("zh-hant") {
            "tc"
        } else if self.lang.starts_with("zh-hk") {
            "hk"
        } else if self.lang.starts_with("zh") {
            "sc"
        } else {
            "jp"
        };
        if let Some(i) = cjk.iter().position(|c| *c == rest) {
            return Some(if rest == prefer { 0 } else { 1 + i });
        }
        Some(10 + pos)
    }

    pub fn has_family(&self, name: &str) -> bool {
        self.resolve(name).is_some()
    }

    pub fn is_mono(&self, family: &str) -> bool {
        let Some(f) = self.resolve(family) else { return false };
        self.by_norm.get(&norm(&f)).is_some_and(|ix| ix.iter().any(|i| self.faces[*i].mono))
    }

    /// The face of `family` (an installed name, see [`Self::resolve`]) closest to the fontconfig weight `fc_weight`.
    /// Upright and normal width faces win over italic / condensed ones.
    pub fn face(&self, family: &str, fc_weight: i32) -> Option<FaceRef> {
        let ix = self.by_norm.get(&norm(family))?;
        let dist = |f: &Face| if (f.weight_lo..=f.weight_hi).contains(&fc_weight) { 0 } else { (f.weight_lo - fc_weight).abs().min((f.weight_hi - fc_weight).abs()) };
        let best = ix.iter().map(|i| &self.faces[*i]).min_by_key(|f| (f.slant != 0, (f.width - 100).abs(), dist(f)))?;
        Some(FaceRef { file: best.file.clone(), index: best.index & 0xffff, variable: self.variable_files.contains(&best.file), weight: fc_weight })
    }
}

fn hidden_in_picker(f: &str) -> bool {
    let Some(rest) = f.strip_prefix("Noto Sans ").or_else(|| f.strip_prefix("Noto Serif ")).or_else(|| f.strip_prefix("Noto Rashi ")).or_else(|| f.strip_prefix("Noto Naskh ")) else {
        return f.starts_with("Noto ") && !matches!(f, "Noto Color Emoji" | "Noto Emoji" | "Noto Sans" | "Noto Serif" | "Noto Music");
    };
    !NOTO_SHOWN.contains(&norm(rest).as_str())
}

// ---------------------------------------------------------------- process-wide table

/// The table while somebody holds it (a program that wants the installed fonts for good uses [`rescan_if_older`]).
static DB: Mutex<Option<std::sync::Weak<FontDb>>> = Mutex::new(None);
/// What [`rescan_if_older`] keeps alive, with the time of its scan.
static KEPT: Mutex<Option<(Arc<FontDb>, std::time::Instant)>> = Mutex::new(None);

/// The installed fonts. The table is shared while anybody holds it and scanned again after the last holder let go,
/// so a daemon that only needs a few resolved families keeps no table of every installed face (see
/// [`FontDb::subset`]).
pub fn get() -> Arc<FontDb> {
    let mut g = DB.lock().unwrap();
    if let Some(db) = g.as_ref().and_then(std::sync::Weak::upgrade) {
        return db;
    }
    let db = Arc::new(FontDb::scan());
    *g = Some(Arc::downgrade(&db));
    db
}

/// Scans again when the last scan of this function is older than `max_age` (a settings window that stays open for
/// hours) and keeps the table for good.
pub fn rescan_if_older(max_age: std::time::Duration) -> Arc<FontDb> {
    let mut kept = KEPT.lock().unwrap();
    if let Some((db, t)) = kept.as_ref() {
        if t.elapsed() <= max_age {
            return db.clone();
        }
    }
    let db = Arc::new(FontDb::scan());
    *DB.lock().unwrap() = Some(Arc::downgrade(&db));
    *kept = Some((db.clone(), std::time::Instant::now()));
    db
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    /// A small machine: Inter (static, several weights), a variable Adwaita Sans, JetBrains Mono Nerd Font, Noto CJK, emoji.
    pub fn db() -> FontDb {
        let rows = [
            ("/f/Inter.ttc", "Inter", 80, 0, 100, 0, "False", ""),
            ("/f/Inter.ttc", "Inter,Inter Medium", 100, 0, 100, 10, "False", ""),
            ("/f/Inter.ttc", "Inter,Inter SemiBold", 180, 0, 100, 12, "False", ""),
            ("/f/Inter.ttc", "Inter", 200, 0, 100, 14, "False", ""),
            ("/f/Inter.ttc", "Inter", 80, 100, 100, 3, "False", ""),
            ("/f/Inter.ttc", "Inter Display", 80, 0, 100, 20, "False", ""),
            ("/f/AdwaitaSans.ttf", "Adwaita Sans", 80, 0, 100, 0, "False", ""),
            ("/f/AdwaitaSans.ttf", "Adwaita Sans", 200, 0, 100, 458752, "False", ""),
            ("/f/AdwaitaSans.ttf", "Adwaita Sans", 0, 0, 100, 0, "True", ""),
            ("/f/JBM.ttf", "JetBrainsMono Nerd Font,JetBrainsMono NF", 80, 0, 100, 0, "False", "100"),
            ("/f/JBM-Bold.ttf", "JetBrainsMono Nerd Font,JetBrainsMono NF", 200, 0, 100, 0, "False", "100"),
            ("/f/CJK.ttc", "Noto Sans CJK JP", 80, 0, 100, 0, "False", ""),
            ("/f/CJK.ttc", "Noto Sans CJK KR", 80, 0, 100, 1, "False", ""),
            ("/f/CJK.ttc", "Noto Sans CJK SC", 80, 0, 100, 2, "False", ""),
            ("/f/Noto.ttf", "Noto Sans", 80, 0, 100, 0, "False", ""),
            ("/f/NotoAdlam.ttf", "Noto Sans Adlam", 80, 0, 100, 0, "False", ""),
            ("/f/Emoji.ttf", "Noto Color Emoji", 80, 0, 100, 0, "False", "100"),
            ("/f/Lib.ttf", "Liberation Sans", 80, 0, 100, 0, "False", ""),
            ("/f/LibMono.ttf", "Liberation Mono", 80, 0, 100, 0, "False", "100"),
        ];
        let text: String = rows.iter().map(|r| format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7)).collect();
        FontDb::from_listing(&text, "Liberation Sans", "JetBrainsMono Nerd Font", "en_US.UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_loosely_and_report_the_installed_spelling() {
        let db = fake::db();
        assert_eq!(db.resolve("inter").as_deref(), Some("Inter"));
        assert_eq!(db.resolve("JetBrains Mono").as_deref(), Some("JetBrainsMono Nerd Font"), "spacing is ignored, the alias finds the family");
        assert_eq!(db.resolve("JetBrainsMono NF").as_deref(), Some("JetBrainsMono Nerd Font"));
        assert_eq!(db.resolve("Segoe UI"), None);
        assert_eq!(db.resolve(""), None);
    }

    #[test]
    fn generic_names_use_the_fontconfig_defaults() {
        let db = fake::db();
        assert_eq!(db.resolve("sans-serif").as_deref(), Some("Liberation Sans"));
        assert_eq!(db.resolve("monospace").as_deref(), Some("JetBrainsMono Nerd Font"));
        assert_eq!(db.resolve("emoji").as_deref(), Some("Noto Color Emoji"));
    }

    #[test]
    fn short_names_pick_the_session_language_variant() {
        let db = fake::db();
        assert_eq!(db.resolve("Noto Sans CJK").as_deref(), Some("Noto Sans CJK JP"));
        let list = "/f/CJK.ttc\tNoto Sans CJK JP\t80\t0\t100\t0\tFalse\t\n/f/CJK.ttc\tNoto Sans CJK SC\t80\t0\t100\t2\tFalse\t\n";
        assert_eq!(FontDb::from_listing(list, "", "", "zh_CN.UTF-8").resolve("Noto Sans CJK").as_deref(), Some("Noto Sans CJK SC"));
        assert_eq!(FontDb::from_listing(list, "", "", "ko_KR.UTF-8").resolve("Noto Sans CJK").as_deref(), Some("Noto Sans CJK JP"), "no KR installed: the next best");
    }

    #[test]
    fn a_short_name_is_not_a_script_font() {
        // "Noto Sans" must not mean "Noto Sans Adlam" when only the latter is installed
        let db = FontDb::from_listing("/f/a.ttf\tNoto Sans Adlam\t80\t0\t100\t0\tFalse\t\n", "", "", "");
        assert_eq!(db.resolve("Noto Sans"), None);
    }

    #[test]
    fn faces_follow_the_requested_weight_and_prefer_upright() {
        let db = fake::db();
        let reg = db.face("Inter", FC_REGULAR).unwrap();
        assert_eq!((reg.index, reg.variable), (0, false));
        assert_eq!(db.face("Inter", FC_MEDIUM).unwrap().index, 10);
        assert_eq!(db.face("Inter", FC_SEMIBOLD).unwrap().index, 12);
        assert_eq!(db.face("Inter", FC_BOLD).unwrap().index, 14);
        // a variable font: the instance bits of the index are dropped and the weight is applied through `wght`
        let b = db.face("Adwaita Sans", FC_BOLD).unwrap();
        assert!(b.variable && b.index == 0 && b.weight == FC_BOLD);
        // only a regular is installed for Noto Sans: bold falls back to it
        assert_eq!(db.face("Noto Sans", FC_BOLD).unwrap().file, PathBuf::from("/f/Noto.ttf"));
        assert!(db.face("Nope", FC_REGULAR).is_none());
    }

    #[test]
    fn the_picker_lists_each_family_once_and_hides_script_fonts() {
        let db = fake::db();
        let list = db.picker_families();
        assert_eq!(list.iter().filter(|f| *f == "Inter").count(), 1);
        assert!(list.contains(&"Noto Sans".to_string()) && list.contains(&"Noto Sans CJK JP".to_string()) && list.contains(&"Noto Color Emoji".to_string()));
        assert!(!list.contains(&"Noto Sans Adlam".to_string()));
        assert!(list.windows(2).all(|w| w[0].to_lowercase() <= w[1].to_lowercase()), "sorted: {list:?}");
        assert!(db.is_mono("JetBrains Mono") && !db.is_mono("Inter"));
    }

    #[test]
    fn a_subset_keeps_only_what_a_chain_needs_and_opens_the_same_faces() {
        let db = fake::db();
        let used = vec!["Inter".to_string(), "Adwaita Sans".to_string(), "JetBrainsMono Nerd Font".to_string()];
        let sub = db.subset(&used);
        assert!(sub.faces.len() < db.faces.len() && sub.family_count() == 3, "{} of {} faces", sub.faces.len(), db.faces.len());
        for (fam, w) in [("Inter", FC_REGULAR), ("Inter", FC_SEMIBOLD), ("Adwaita Sans", FC_BOLD), ("JetBrainsMono Nerd Font", FC_REGULAR)] {
            assert_eq!(sub.face(fam, w), db.face(fam, w), "{fam} {w}");
        }
        assert!(sub.face("Adwaita Sans", FC_BOLD).unwrap().variable, "variable files are remembered");
        assert!(sub.face("Liberation Sans", FC_REGULAR).is_none(), "not in the chain");
        assert!(sub.is_mono("JetBrainsMono NF") && !sub.is_mono("Inter"));
        assert_eq!(sub.resolve("sans-serif").as_deref(), Some("Liberation Sans"), "generic aliases keep the fontconfig defaults (unlisted family: name as is)");
    }

    #[test]
    fn the_table_is_shared_while_it_is_held() {
        let (a, b) = (get(), get());
        assert!(Arc::ptr_eq(&a, &b), "one scan for everybody who holds it");
    }

    #[test]
    fn real_fc_list_output_parses() {
        let db = FontDb::scan();
        if db.is_empty() {
            return; // no fontconfig here
        }
        assert!(db.family_count() > 0 && !db.picker_families().is_empty());
        let sans = db.resolve("sans-serif").expect("fontconfig has a default sans");
        assert!(db.face(&sans, FC_REGULAR).is_some_and(|f| f.file.is_file()));
    }
    #[test]
    fn package_font_registration_is_private_and_rejects_invalid_files() {
        let db = FontDb::default();
        let files = vec![std::env::temp_dir().join("missing-decebal-package-font.ttf")];
        assert!(db.with_package_files(&files).is_empty());
        assert!(db.is_empty());
    }

    #[test]
    fn package_snapshots_reuse_reads_and_invalidate_on_atomic_replacement() {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("font-cache-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(dir.clone());
        let path = dir.join("face.ttf");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes/dux-lavender/fonts/GoogleSansFlex-400.ttf");
        std::fs::copy(&fixture, &path).unwrap();
        let version = package_file_version(&path);
        let mut cache = PackageFonts::default();
        let first = cache.load(&path, version).unwrap();
        for _ in 0..20 {
            let next = cache.load(&path, package_file_version(&path)).unwrap();
            assert!(Arc::ptr_eq(&first, &next));
            assert!(Arc::ptr_eq(&first.bytes, &next.bytes));
        }
        assert_eq!(cache.loads, 1, "unchanged theme generations must not reopen/parse the file");
        let replacement = dir.join("next.ttf");
        std::fs::copy(&fixture, &replacement).unwrap();
        std::fs::File::open(&replacement).unwrap().set_times(std::fs::FileTimes::new().set_modified(std::fs::metadata(&path).unwrap().modified().unwrap())).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        let new_version = package_file_version(&path);
        assert_ne!(version, new_version, "same-size/mtime atomic replacement is a new input");
        let second = cache.load(&path, new_version).unwrap();
        assert!(!Arc::ptr_eq(&first.bytes, &second.bytes));
        assert_eq!(cache.loads, 2);
        assert_eq!(first.bytes, second.bytes, "the held old snapshot survives replacement unchanged");
    }

    #[test]
    fn rejected_package_fonts_are_bounded_and_negative_cached() {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("bad-font-cache-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(dir.clone());
        let path = dir.join("too-large.ttf");
        std::fs::File::create(&path).unwrap().set_len(MAX_PACKAGE_FONT_BYTES + 1).unwrap();
        let mut cache = PackageFonts::default();
        for _ in 0..10 {
            assert!(cache.load(&path, package_file_version(&path)).is_none());
        }
        assert_eq!(cache.loads, 1, "one rejection diagnostic/attempt per unchanged file version");
        assert_eq!(cache.bytes, 0, "oversize input is rejected before allocating file bytes");
        std::fs::write(&path, b"not a font").unwrap();
        assert!(cache.load(&path, package_file_version(&path)).is_none());
        assert_eq!(cache.loads, 2, "a corrected/replaced input gets a fresh attempt");
        for i in 0..MAX_PACKAGE_CACHE_ENTRIES + 4 {
            let missing = dir.join(format!("missing-{i}.ttf"));
            assert!(cache.load(&missing, (0, 0)).is_none());
        }
        assert!(cache.entries.len() <= MAX_PACKAGE_CACHE_ENTRIES);
        assert!(cache.bytes <= MAX_PACKAGE_CACHE_BYTES);
    }

    /// DUX_FONT_DIR=/readonly/upstream/fonts/google-sans-flex cargo test --lib package_google_sans -- --ignored
    #[test]
    #[ignore = "requires licensed upstream Google Sans Flex assets in DUX_FONT_DIR"]
    fn package_google_sans_faces_have_real_native_outlines() {
        let dir = PathBuf::from(std::env::var_os("DUX_FONT_DIR").expect("DUX_FONT_DIR"));
        let files: Vec<_> = ["GoogleSansFlex-400.ttf", "GoogleSansFlex-500.ttf", "GoogleSansFlex-700.ttf"].iter().map(|n| dir.join(n)).collect();
        let installed = FontDb::default();
        let registered = installed.with_package_files(&files);
        let family = registered.resolve("Google Sans Flex").expect("actual package family registered");
        assert!(!installed.has_family("Google Sans Flex"), "registration must not mutate installed database");
        for weight in [FC_REGULAR, FC_MEDIUM, FC_BOLD] {
            let face = registered.face(&family, weight).unwrap();
            assert!(files.contains(&face.file));
            let bytes = std::fs::read(&face.file).unwrap();
            for ch in ['A', 'g', '0', '5'] {
                let glyph = crate::textface::raster_outline(&bytes, &face, ch, 24.0).expect("native outline raster");
                assert!(glyph.advance > 0.0 && glyph.bitmap.iter().any(|v| *v > 0), "missing glyph {ch} at {weight}");
            }
        }
    }
}
