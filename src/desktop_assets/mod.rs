//! Desktop asset packs: icon and cursor themes installed from pinned upstream sources (see `docs/desktop-asset-packs.md`).
//!
//! * [`catalog`]: the companion metadata ports ship (`assets/desktop-ports/<family>/asset-packs.toml`) and where it is found.
//! * [`tar`], [`source`]: a streaming, hostile-input tar(.gz) / folder reader that stages only the wanted subtree and
//!   records links without ever creating them; [`gitoid`] verifies the pinned git tree OID of what was read.
//! * [`assemble`]: the theme folder built from the staged source (allowlisted files, safe links, licences, validation).
//! * [`install`]: the staging area, the overwrite policy, atomic install / rollback, provenance, verify and remove.
//! * [`fetch`]: the HTTPS download of a pinned archive. [`cursor`]: Xcursor files and cursor theme lookup.
//! * [`policy`]: who chose the icon and cursor theme (the user or a theme) and theme recommendations.
//! * [`cli`] (`decthemectl assets`), [`pane`] (the Icons pane rows), [`preview`] (resolution smoke and PNG sheet).
//!
//! Nothing here runs a program from a pack or interpolates a value into a shell.

pub mod aliases;
pub mod assemble;
pub mod builtin;
pub mod catalog;
pub mod cli;
pub mod cursor;
pub mod dci;
pub mod fetch;
pub mod gitoid;
pub mod install;
pub mod pane;
pub mod policy;
pub mod preview;
pub mod source;
pub mod tar;
pub mod xcursor;

#[cfg(test)]
pub mod testkit;

use std::path::PathBuf;

/// What a pack gives the desktop. One folder may provide both (Deepin's `bloom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provides {
    Icons,
    Cursors,
}

impl Provides {
    pub fn id(self) -> &'static str {
        match self {
            Provides::Icons => "icons",
            Provides::Cursors => "cursors",
        }
    }

    pub fn parse(s: &str) -> Option<Provides> {
        match s.trim() {
            "icons" | "icon" => Some(Provides::Icons),
            "cursors" | "cursor" => Some(Provides::Cursors),
            _ => None,
        }
    }

    /// `icons,cursors` / `icons` / ...; empty or unknown parts are an error.
    pub fn parse_list(s: &str) -> Result<Vec<Provides>, String> {
        let mut v = Vec::new();
        for p in s.split(',') {
            let k = Provides::parse(p).ok_or_else(|| format!("`{}` is not icons or cursors", p.trim()))?;
            if !v.contains(&k) {
                v.push(k);
            }
        }
        v.sort();
        Ok(v)
    }
}

/// Hard caps for one install. The defaults fit the largest real icon themes (Papirus, Deepin's bloom: ~35 000
/// entries) with room to spare; tests use small ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Entries of the archive or folder (all of them, also the ones skipped unread).
    pub max_entries: usize,
    /// Largest file that is read.
    pub max_file: u64,
    /// Sum of the files that are read.
    pub max_total: u64,
    /// The decompressed tar stream, skipped parts included (a compression bomb stops here).
    pub max_stream: u64,
    /// The archive file itself (compressed).
    pub max_archive: u64,
    /// One licence / text file.
    pub max_text: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_entries: 120_000, max_file: 16 << 20, max_total: 1 << 30, max_stream: 4 << 30, max_archive: 512 << 20, max_text: 1 << 20 }
    }
}

/// Longest path accepted inside a source, in bytes, and deepest nesting.
pub const MAX_PATH: usize = 255;
pub const MAX_DEPTH: usize = 24;
/// Symlink hops followed when resolving a link inside a source.
pub const MAX_HOPS: usize = 16;

/// A path inside a source: relative, `/`-separated, no empty / `.` / `..` parts, no backslash or control character,
/// at most [`MAX_PATH`] bytes and [`MAX_DEPTH`] parts.
pub fn safe_rel(p: &str) -> Result<(), String> {
    if p.is_empty() {
        return Err("empty path".into());
    }
    if p.len() > MAX_PATH {
        return Err(format!("path `{}...` is longer than {MAX_PATH} bytes", &p[..p.char_indices().nth(40).map_or(p.len(), |c| c.0)]));
    }
    if p.starts_with('/') {
        return Err(format!("`{p}` is an absolute path"));
    }
    if p.contains('\\') || p.chars().any(char::is_control) {
        return Err(format!("`{}` contains a backslash or control character", p.escape_debug()));
    }
    let mut depth = 0;
    for part in p.split('/') {
        match part {
            "" => return Err(format!("`{p}` has an empty path component")),
            "." | ".." => return Err(format!("`{p}` escapes its folder (`{part}`)")),
            _ => depth += 1,
        }
    }
    if depth > MAX_DEPTH {
        return Err(format!("`{p}` is nested deeper than {MAX_DEPTH} folders"));
    }
    Ok(())
}

/// A theme folder name (`install_name`, `Inherits=` entries): `[A-Za-z0-9][A-Za-z0-9._+-]*`, at most 64.
pub fn safe_theme_name(n: &str) -> bool {
    let mut c = n.chars();
    !n.is_empty() && n.len() <= 64 && c.next().is_some_and(|c| c.is_ascii_alphanumeric()) && c.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

/// `$DECEBAL_ASSET_ROOT`, else `$XDG_DATA_HOME/icons` (default `~/.local/share/icons`): where packs are installed.
pub fn default_root() -> PathBuf {
    if let Some(r) = std::env::var_os("DECEBAL_ASSET_ROOT").filter(|s| !s.is_empty()) {
        return PathBuf::from(r);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("icons")
}

/// The icon roots with `root` first (so a pack just installed into a private root resolves).
pub fn roots_with(root: &std::path::Path) -> crate::icons::resolve::Roots {
    let mut r = crate::icons::resolve::Roots::from_env();
    r.icon_dirs.retain(|d| d != root);
    r.icon_dirs.insert(0, root.to_path_buf());
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_paths_and_theme_names() {
        for ok in ["bloom", "bloom/apps/48/a.svg", "a b/c+d"] {
            assert!(safe_rel(ok).is_ok(), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "a/../b", "a//b", "./a", "a/.", "a\\b", "a\u{0}b", "a\nb"] {
            assert!(safe_rel(bad).is_err(), "{bad:?}");
        }
        assert!(safe_rel("a/".repeat(30).trim_end_matches('/')).is_err(), "too deep");
        assert!(safe_rel(&"x".repeat(300)).is_err(), "too long");
        for ok in ["bloom", "DMZ-White", "ukui-icon-theme-default", "Papirus_Dark+1.0"] {
            assert!(safe_theme_name(ok), "{ok}");
        }
        for bad in ["", "-x", ".hidden", "a/b", "..", "a b", &"x".repeat(65)] {
            assert!(!safe_theme_name(bad), "{bad}");
        }
        assert_eq!(Provides::parse_list("cursors, icons").unwrap(), vec![Provides::Icons, Provides::Cursors]);
        assert!(Provides::parse_list("icons,fonts").is_err());
    }
}
