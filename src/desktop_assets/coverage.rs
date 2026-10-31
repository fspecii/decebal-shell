//! Matching icons for every theme: decebal's coverage table (`coverage/themes.toml`) for the themes whose port
//! catalogue has no `[[recommend]]`, and the pinned packs it needs that no port catalogue provides
//! (`coverage/packs.toml`, loaded after the port catalogues). Each entry says whether the pack is the upstream set of the
//! desktop the theme recreates, the one its manifest names, or an honestly named fallback, and why.

use toml_edit::{Document, Item};

pub const PACKS: &str = include_str!("coverage/packs.toml");
const THEMES: &str = include_str!("coverage/themes.toml");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    pub theme: String,
    /// Pack ids (of any loaded catalogue).
    pub icons: Option<String>,
    pub cursors: Option<String>,
    /// `upstream`, `manifest` or `fallback`.
    pub basis: String,
    pub reason: String,
}

pub fn parse(text: &str) -> Result<Vec<Cover>, Vec<String>> {
    let doc: Document<String> = text.parse().map_err(|e| vec![format!("coverage table: {e}")])?;
    let mut errs = Vec::new();
    let root = doc.as_table();
    for (k, _) in root.iter() {
        if !matches!(k, "format" | "cover") {
            errs.push(format!("coverage table: unknown key `{k}`"));
        }
    }
    if root.get("format").and_then(Item::as_integer) != Some(1) {
        errs.push("coverage table: `format` must be 1".into());
    }
    let mut out: Vec<Cover> = Vec::new();
    for (i, t) in root.get("cover").and_then(Item::as_array_of_tables).into_iter().flatten().enumerate() {
        let s = |k: &str| t.get(k).and_then(Item::as_str).map(String::from);
        for (k, _) in t.iter() {
            if !matches!(k, "theme" | "icons" | "cursors" | "basis" | "reason") {
                errs.push(format!("cover[{i}]: unknown key `{k}`"));
            }
        }
        let theme = s("theme").unwrap_or_default();
        if !super::catalog::is_id(&theme) {
            errs.push(format!("cover[{i}]: `theme` must be a theme id"));
        }
        let basis = s("basis").unwrap_or_default();
        if !matches!(basis.as_str(), "upstream" | "manifest" | "fallback") {
            errs.push(format!("cover[{i}] ({theme}): `basis` is upstream, manifest or fallback"));
        }
        let reason = s("reason").unwrap_or_default();
        if reason.trim().is_empty() || reason.len() > 300 {
            errs.push(format!("cover[{i}] ({theme}): a `reason` (at most 300 bytes) says why this pack"));
        }
        let (icons, cursors) = (s("icons"), s("cursors"));
        if icons.is_none() && cursors.is_none() {
            errs.push(format!("cover[{i}] ({theme}): names no pack"));
        }
        if out.iter().any(|c| c.theme == theme) {
            errs.push(format!("cover[{i}]: `{theme}` is covered twice"));
        }
        out.push(Cover { theme, icons, cursors, basis, reason });
    }
    if errs.is_empty() {
        Ok(out)
    } else {
        Err(errs)
    }
}

fn table() -> &'static [Cover] {
    static T: std::sync::OnceLock<Vec<Cover>> = std::sync::OnceLock::new();
    // the compiled-in table is checked by the tests below; a broken one covers nothing rather than failing an apply
    T.get_or_init(|| parse(THEMES).unwrap_or_default())
}

/// The coverage entry of a theme.
pub fn find(theme: &str) -> Option<Cover> {
    table().iter().find(|c| c.theme == theme).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_assets::{catalog, Provides};

    fn builtin_set() -> catalog::Set {
        let mut s = catalog::Set::default();
        for (family, text) in crate::desktop_assets::builtin::CATALOGS {
            s.push(catalog::parse(text, family).unwrap());
        }
        s.push(catalog::parse(PACKS, "decebal").unwrap());
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        s
    }

    #[test]
    fn every_theme_has_an_honest_matching_recommendation() {
        let set = builtin_set();
        let covers = parse(THEMES).unwrap_or_else(|e| panic!("{}", e.join("\n")));
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        let mut ids: Vec<String> = std::fs::read_dir(&base).unwrap().flatten().filter(|e| e.path().join("theme.toml").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        ids.sort();
        assert!(ids.len() >= 40, "the themes folder is read");
        let mut uncovered = Vec::new();
        for id in &ids {
            let rec = crate::desktop_assets::policy::recommendation(&set, id, None, &|_, _| false);
            if rec.icons_pack.is_none() && rec.icons.is_none() {
                uncovered.push(id.clone());
            }
        }
        assert!(uncovered.is_empty(), "themes without matching icons (add a [[cover]] or a port [[recommend]]): {uncovered:?}");
        for c in &covers {
            assert!(ids.contains(&c.theme), "cover for a theme that does not exist: {}", c.theme);
            for (k, p) in [(Provides::Icons, &c.icons), (Provides::Cursors, &c.cursors)] {
                let Some(p) = p else { continue };
                let e = set.find(p).filter(|e| e.pack.id == *p).unwrap_or_else(|| panic!("{}: no pack `{p}`", c.theme));
                assert!(e.pack.provides(k), "{}: `{p}` does not provide {}", c.theme, k.id());
                // every pack a theme applies by itself can be fetched with its pins: no manual step
                assert!(e.source.downloadable(), "{}: `{p}` has no archive_url or git_url", c.theme);
            }
            if c.basis == "fallback" {
                let e = set.find(c.icons.as_deref().unwrap_or_default()).unwrap();
                assert!(!e.pack.name.to_lowercase().contains(&c.theme.replace('-', " ")), "{}: a fallback keeps its upstream name", c.theme);
            }
        }
        // the port catalogues' own recommendations win, and are downloadable too
        for id in &ids {
            if let Some(r) = set.recommendation(id) {
                for p in r.icons.iter().chain(&r.cursors) {
                    let e = set.find(p).unwrap();
                    assert!(e.source.downloadable(), "{id}: the port recommends `{p}`, which cannot be downloaded");
                }
            }
        }
    }

    #[test]
    fn the_table_is_strict() {
        let ok = "format = 1\n[[cover]]\ntheme = \"nord\"\nicons = \"x\"\nbasis = \"fallback\"\nreason = \"r\"\n";
        assert_eq!(parse(ok).unwrap()[0].icons.as_deref(), Some("x"));
        let bad = |from: &str, to: &str, want: &str| {
            let e = parse(&ok.replace(from, to)).unwrap_err();
            assert!(e.iter().any(|x| x.contains(want)), "{e:?}");
        };
        bad("basis = \"fallback\"", "basis = \"same\"", "upstream, manifest or fallback");
        bad("reason = \"r\"", "reason = \"\"", "reason");
        bad("icons = \"x\"\n", "", "names no pack");
        bad("theme = \"nord\"", "theme = \"../nord\"", "theme id");
        bad("reason = \"r\"", "reason = \"r\"\nalias = \"Dracula\"", "unknown key `alias`");
        bad("format = 1", "format = 2", "must be 1");
        assert!(parse(&format!("{ok}{}", &ok["format = 1\n".len()..])).unwrap_err().iter().any(|e| e.contains("twice")));
    }
}
