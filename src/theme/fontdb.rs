//! The fonts installed on this machine.
//!
//! One `fc-list` call (plus `fc-match` for the generic `sans-serif` / `monospace` aliases, run in parallel
//! with it) is parsed into a table of faces: family names, weight, slant, width, file and index. Everything
//! else (is a family installed, which file is its bold face, which families does the picker offer) is answered
//! from that table, so no process runs per lookup. The table is built once per process ([`get`]); a program
//! that wants to notice newly installed fonts calls [`rescan`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Case, space and punctuation-insensitive family key: "JetBrains Mono" == "JetBrainsMono".
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// fontconfig weight values for the four weights macshell draws with.
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
    pub gen: u64,
}

static GEN: AtomicU64 = AtomicU64::new(1);

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
        let mut db = FontDb { sans: sans.to_string(), mono: mono.to_string(), lang: lang.to_ascii_lowercase().replace('_', "-"), gen: GEN.fetch_add(1, Ordering::Relaxed), ..FontDb::default() };
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
        let mut seen = HashSet::new();
        for (i, f) in db.faces.iter().enumerate() {
            for n in &f.names {
                db.by_norm.entry(norm(n)).or_default().push(i);
            }
            if seen.insert(norm(&f.names[0])) {
                db.families.push(f.names[0].clone());
            }
        }
        db.families.sort_by_key(|f| f.to_lowercase());
        db
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
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

static DB: Mutex<Option<(Arc<FontDb>, std::time::Instant)>> = Mutex::new(None);

/// The installed fonts; scanned on first use.
pub fn get() -> Arc<FontDb> {
    let mut g = DB.lock().unwrap();
    if let Some((db, _)) = g.as_ref() {
        return db.clone();
    }
    let db = Arc::new(FontDb::scan());
    *g = Some((db.clone(), std::time::Instant::now()));
    db
}

/// Scans again when the last scan is older than `max_age` (a settings window that stays open for hours).
pub fn rescan_if_older(max_age: std::time::Duration) -> Arc<FontDb> {
    let stale = DB.lock().unwrap().as_ref().is_none_or(|(_, t)| t.elapsed() > max_age);
    if stale {
        let db = Arc::new(FontDb::scan());
        *DB.lock().unwrap() = Some((db.clone(), std::time::Instant::now()));
        return db;
    }
    get()
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
    fn real_fc_list_output_parses() {
        let db = FontDb::scan();
        if db.is_empty() {
            return; // no fontconfig here
        }
        assert!(db.family_count() > 0 && !db.picker_families().is_empty());
        let sans = db.resolve("sans-serif").expect("fontconfig has a default sans");
        assert!(db.face(&sans, FC_REGULAR).is_some_and(|f| f.file.is_file()));
    }
}
