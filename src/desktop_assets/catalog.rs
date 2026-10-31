//! Companion metadata of the desktop ports: `assets/desktop-ports/<family>/asset-packs.toml` (see
//! `docs/desktop-asset-packs.md` for the format). Data only; every error names the line and the key.

use super::Provides;
use std::path::{Path, PathBuf};
use toml_edit::{Document, Item, Table};

pub const FORMAT: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub repo: String,
    pub revision: String,
    pub commit: String,
    pub license: String,
    pub license_files: Vec<String>,
    /// `git rev-parse <commit>:<file>` of each licence file, same order (optional).
    pub license_oids: Vec<String>,
    pub archive_url: Option<String>,
    pub archive_sha256: Option<String>,
    pub strip: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub provides: Vec<Provides>,
    pub source: String,
    pub path: String,
    pub tree: String,
    pub install_name: String,
    pub license: Option<String>,
    pub license_files: Vec<String>,
    pub license_oids: Vec<String>,
    pub link_sources: Vec<String>,
    /// `git rev-parse <commit>:<folder>` of each `link_sources` folder, same order (optional when `archive_sha256` pins
    /// the whole archive; required otherwise for an install that uses them).
    pub link_source_trees: Vec<String>,
    /// Source files that, concatenated, become the theme's `index.theme` when its folder has none (Adwaita keeps it
    /// in the repository root, Breeze in two `.theme.in` files), each pinned by its git blob id in `index_oids`.
    pub index: Vec<String>,
    pub index_oids: Vec<String>,
    /// Cursors built natively from PNGs and declared hotspots (`[[pack.xcursor]]`), for sources that only ship a
    /// build script (openKylin's dark-sense).
    pub xcursor: Vec<super::xcursor::Cursor>,
    /// Folders of regular files holding link targets (`[[pack.text_links]]`) and folder links (`[[pack.dir_alias]]`)
    /// an upstream build would create; see [`super::aliases`].
    pub text_links: Vec<super::aliases::TextLinks>,
    pub dir_aliases: Vec<super::aliases::DirAlias>,
    /// Convert the pack folder's DSG icon containers (`[pack.dci_convert]`, see [`super::dci`]).
    pub dci: Option<super::dci::Convert>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommend {
    pub theme: String,
    pub icons: Option<String>,
    pub cursors: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub family: String,
    /// The file it came from, or `builtin:<family>`.
    pub origin: String,
    pub sources: Vec<Source>,
    pub packs: Vec<Pack>,
    pub recommends: Vec<Recommend>,
}

impl Pack {
    pub fn provides(&self, k: Provides) -> bool {
        self.provides.contains(&k)
    }
}

/// A pack with what it is cut from.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub catalog: &'a Catalog,
    pub pack: &'a Pack,
    pub source: &'a Source,
}

impl Entry<'_> {
    pub fn license(&self) -> &str {
        self.pack.license.as_deref().unwrap_or(&self.source.license)
    }

    /// Pinned blob ids of licence files, by path (from the source's and the pack's `license_oids`).
    pub fn license_oids(&self) -> std::collections::BTreeMap<String, String> {
        let mut m = std::collections::BTreeMap::new();
        for (f, o) in self.source.license_files.iter().zip(&self.source.license_oids).chain(self.pack.license_files.iter().zip(&self.pack.license_oids)) {
            m.insert(f.clone(), o.clone());
        }
        m
    }

    /// Source-relative licence files: the source's, then the pack's own.
    pub fn license_files(&self) -> Vec<String> {
        let mut v = self.source.license_files.clone();
        for f in &self.pack.license_files {
            if !v.contains(f) {
                v.push(f.clone());
            }
        }
        v
    }
}

// ---------------------------------------------------------------- parsing

struct Cx<'t> {
    text: &'t str,
    errors: Vec<String>,
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn is_id(s: &str) -> bool {
    let mut c = s.chars();
    !s.is_empty() && s.len() <= 64 && c.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) && c.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn https(s: &str) -> bool {
    s.strip_prefix("https://").is_some_and(|r| !r.is_empty() && !r.starts_with('/') && !r.chars().any(|c| c.is_whitespace() || c.is_control()))
}

impl Cx<'_> {
    fn line_of(&self, item: &Item) -> usize {
        item.span().map(|s| self.text[..s.start.min(self.text.len())].matches('\n').count() + 1).unwrap_or(0)
    }

    fn err(&mut self, item: &Item, key: &str, msg: impl AsRef<str>) {
        let l = self.line_of(item);
        self.errors.push(if l > 0 { format!("line {l}: `{key}`: {}", msg.as_ref()) } else { format!("`{key}`: {}", msg.as_ref()) });
    }

    fn known(&mut self, t: &Table, scope: &str, keys: &[&str]) {
        for (k, item) in t.iter() {
            if !keys.contains(&k) {
                let hint = keys.iter().filter(|c| edit_distance(k, c) <= 2.max(k.len() / 4)).min_by_key(|c| edit_distance(k, c)).map(|c| format!(" (did you mean `{c}`?)")).unwrap_or_default();
                self.err(item, &format!("{scope}{k}"), format!("unknown key{hint}"));
            }
        }
    }

    fn text(&mut self, t: &Table, scope: &str, key: &str, required: bool, max: usize) -> Option<String> {
        let Some(item) = t.get(key) else {
            if required {
                self.errors.push(format!("`{scope}{key}` is required"));
            }
            return None;
        };
        match item.as_str() {
            Some(s) if s.chars().count() > max => {
                self.err(item, &format!("{scope}{key}"), format!("is longer than {max} characters"));
                None
            }
            Some(s) if s.chars().any(char::is_control) => {
                self.err(item, &format!("{scope}{key}"), "contains a control character");
                None
            }
            Some(s) if required && s.trim().is_empty() => {
                self.err(item, &format!("{scope}{key}"), "must not be empty");
                None
            }
            Some(s) => Some(s.to_string()),
            None => {
                self.err(item, &format!("{scope}{key}"), "must be a string");
                None
            }
        }
    }

    /// A required string that may span lines (no other control characters).
    fn text_multiline(&mut self, t: &Table, scope: &str, key: &str) -> String {
        match t.get(key) {
            None => {
                self.errors.push(format!("`{scope}{key}` is required"));
                String::new()
            }
            Some(item) => match item.as_str() {
                Some(s) if s.len() <= 16 << 10 && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r') => s.to_string(),
                Some(_) => {
                    self.err(item, &format!("{scope}{key}"), "is longer than 16 KiB or contains control characters");
                    String::new()
                }
                None => {
                    self.err(item, &format!("{scope}{key}"), "must be a string");
                    String::new()
                }
            },
        }
    }

    fn list(&mut self, t: &Table, scope: &str, key: &str) -> Vec<String> {
        let Some(item) = t.get(key) else { return Vec::new() };
        let Some(a) = item.as_array() else {
            self.err(item, &format!("{scope}{key}"), "must be a list of strings");
            return Vec::new();
        };
        if a.len() > 32 {
            self.err(item, &format!("{scope}{key}"), "has more than 32 entries");
        }
        let v: Vec<String> = a.iter().filter_map(|x| x.as_str().map(String::from)).collect();
        if v.len() != a.len() {
            self.err(item, &format!("{scope}{key}"), "must be a list of strings");
        }
        v
    }

    fn rel(&mut self, t: &Table, scope: &str, key: &str, p: &str, dot_ok: bool) {
        if dot_ok && p == "." {
            return;
        }
        if let Err(e) = super::safe_rel(p) {
            let item = t.get(key).cloned().unwrap_or(Item::None);
            self.err(&item, &format!("{scope}{key}"), e);
        }
    }

    fn tables<'d>(&mut self, root: &'d Table, key: &str) -> Vec<(usize, &'d Table)> {
        match root.get(key) {
            None => Vec::new(),
            Some(item) => match item.as_array_of_tables() {
                Some(a) => a.iter().enumerate().collect(),
                None => {
                    self.err(item, key, format!("must be an array of tables ([[{key}]])"));
                    Vec::new()
                }
            },
        }
    }
}

/// Parses and validates one catalogue. `origin` is only used in messages and kept in the result.
pub fn parse(text: &str, origin: &str) -> Result<Catalog, Vec<String>> {
    let doc = match Document::parse(text.to_string()) {
        Ok(d) => d,
        Err(e) => {
            let line = e.span().map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1).unwrap_or(0);
            return Err(vec![format!("line {line}: {}", e.message())]);
        }
    };
    let root = doc.as_table();
    let mut cx = Cx { text, errors: Vec::new() };
    cx.known(root, "", &["format", "family", "source", "pack", "recommend"]);
    match root.get("format").map(|i| (i, i.as_integer())) {
        Some((_, Some(FORMAT))) => {}
        Some((i, _)) => cx.err(i, "format", format!("must be {FORMAT} (this decebal reads format {FORMAT})")),
        None => cx.errors.push("`format` is required (format = 1)".into()),
    }
    let family = cx.text(root, "", "family", true, 32).unwrap_or_default();
    if !family.is_empty() && !is_id(&family) {
        cx.errors.push(format!("`family`: `{family}` must be lower-case letters, digits and dashes"));
    }

    let mut sources = Vec::new();
    for (i, t) in cx.tables(root, "source") {
        let sc = format!("source[{i}].");
        cx.known(t, &sc, &["id", "repo", "revision", "commit", "license", "license_files", "license_oids", "archive_url", "archive_sha256", "strip_components"]);
        let id = cx.text(t, &sc, "id", true, 64).unwrap_or_default();
        if !id.is_empty() && !is_id(&id) {
            cx.errors.push(format!("`{sc}id`: `{id}` must be lower-case letters, digits and dashes"));
        }
        let repo = cx.text(t, &sc, "repo", true, 300).unwrap_or_default();
        if !repo.is_empty() && !https(&repo) {
            cx.errors.push(format!("`{sc}repo` must be an https:// URL"));
        }
        let revision = cx.text(t, &sc, "revision", true, 120).unwrap_or_default();
        let commit = cx.text(t, &sc, "commit", true, 40).unwrap_or_default();
        if !commit.is_empty() && !is_hex(&commit, 40) {
            cx.errors.push(format!("`{sc}commit` must be 40 lower-case hex digits"));
        }
        let license = cx.text(t, &sc, "license", true, 200).unwrap_or_default();
        let license_files = cx.list(t, &sc, "license_files");
        for f in &license_files {
            cx.rel(t, &sc, "license_files", f, false);
        }
        let archive_url = cx.text(t, &sc, "archive_url", false, 400);
        if archive_url.as_deref().is_some_and(|u| !https(u)) {
            cx.errors.push(format!("`{sc}archive_url` must be an https:// URL"));
        }
        let archive_sha256 = cx.text(t, &sc, "archive_sha256", false, 64);
        if archive_sha256.as_deref().is_some_and(|h| !is_hex(h, 64)) {
            cx.errors.push(format!("`{sc}archive_sha256` must be 64 lower-case hex digits"));
        }
        // forge-generated archives (GitHub, Gitee) are not byte-stable: the tree id pins the content, a sha256 is optional
        if archive_sha256.is_some() && archive_url.is_none() {
            cx.errors.push(format!("`{sc}archive_sha256` needs an `archive_url`"));
        }
        let strip = match t.get("strip_components") {
            None => 0,
            Some(item) => match item.as_integer() {
                Some(n @ 0..=4) => n as usize,
                _ => {
                    cx.err(item, &format!("{sc}strip_components"), "must be a whole number 0..4");
                    0
                }
            },
        };
        let license_oids = cx.list(t, &sc, "license_oids");
        if !license_oids.is_empty() && (license_oids.len() != license_files.len() || license_oids.iter().any(|o| !is_hex(o, 40))) {
            cx.errors.push(format!("`{sc}license_oids` pins every licence file with 40 hex digits (git rev-parse <commit>:<file>), in the same order"));
        }
        sources.push(Source { id, repo, revision, commit, license, license_files, license_oids, archive_url, archive_sha256, strip });
    }

    let mut packs = Vec::new();
    for (i, t) in cx.tables(root, "pack") {
        let sc = format!("pack[{i}].");
        cx.known(
            t,
            &sc,
            &[
                "id",
                "name",
                "provides",
                "source",
                "path",
                "tree",
                "install_name",
                "license",
                "license_files",
                "license_oids",
                "link_sources",
                "link_source_trees",
                "index",
                "index_oids",
                "xcursor",
                "text_links",
                "dir_alias",
                "dci_convert",
                "description",
            ],
        );
        let id = cx.text(t, &sc, "id", true, 64).unwrap_or_default();
        if !id.is_empty() && !is_id(&id) {
            cx.errors.push(format!("`{sc}id`: `{id}` must be lower-case letters, digits and dashes"));
        }
        let name = cx.text(t, &sc, "name", true, 80).unwrap_or_default();
        let mut provides = Vec::new();
        let raw = cx.list(t, &sc, "provides");
        if raw.is_empty() {
            cx.errors.push(format!("`{sc}provides` must list \"icons\", \"cursors\" or both"));
        }
        for p in raw {
            match Provides::parse(&p) {
                Some(k) if !provides.contains(&k) => provides.push(k),
                Some(_) => {}
                None => cx.errors.push(format!("`{sc}provides`: `{p}` is not icons or cursors")),
            }
        }
        provides.sort();
        let source = cx.text(t, &sc, "source", true, 64).unwrap_or_default();
        let path = cx.text(t, &sc, "path", true, 200).unwrap_or_default();
        if !path.is_empty() {
            cx.rel(t, &sc, "path", &path, true);
        }
        let tree = cx.text(t, &sc, "tree", true, 40).unwrap_or_default();
        if !tree.is_empty() && !is_hex(&tree, 40) {
            cx.errors.push(format!("`{sc}tree` must be 40 lower-case hex digits (git rev-parse <commit>:<path>)"));
        }
        let install_name = match cx.text(t, &sc, "install_name", false, 64) {
            Some(n) => n,
            None => path.rsplit('/').next().filter(|s| *s != ".").unwrap_or(&id).to_string(),
        };
        if !super::safe_theme_name(&install_name) {
            cx.errors.push(format!("`{sc}install_name`: `{install_name}` is not a usable theme folder name"));
        }
        let license = cx.text(t, &sc, "license", false, 200).filter(|s| !s.trim().is_empty());
        let license_files = cx.list(t, &sc, "license_files");
        for f in &license_files {
            cx.rel(t, &sc, "license_files", f, false);
        }
        let license_oids = cx.list(t, &sc, "license_oids");
        if !license_oids.is_empty() && (license_oids.len() != license_files.len() || license_oids.iter().any(|o| !is_hex(o, 40))) {
            cx.errors.push(format!("`{sc}license_oids` pins every licence file with 40 hex digits (git rev-parse <commit>:<file>), in the same order"));
        }
        let link_sources = cx.list(t, &sc, "link_sources");
        for f in &link_sources {
            cx.rel(t, &sc, "link_sources", f, false);
            let inside = |a: &str, b: &str| b == "." || a == b || a.starts_with(&format!("{b}/"));
            if !path.is_empty() && (inside(f, &path) || inside(&path, f)) {
                cx.errors.push(format!("`{sc}link_sources`: `{f}` overlaps the pack's path `{path}`"));
            }
        }
        let link_source_trees = cx.list(t, &sc, "link_source_trees");
        if !link_source_trees.is_empty() && link_source_trees.len() != link_sources.len() {
            cx.errors.push(format!("`{sc}link_source_trees` must pin every `link_sources` folder, in the same order"));
        }
        if link_source_trees.iter().any(|o| !is_hex(o, 40)) {
            cx.errors.push(format!("`{sc}link_source_trees` are 40 lower-case hex digits (git rev-parse <commit>:<folder>)"));
        }
        let index = match t.get("index").map(|i| (i, i.as_str())) {
            Some((_, Some(s))) => vec![s.to_string()],
            _ => cx.list(t, &sc, "index"),
        };
        for f in &index {
            cx.rel(t, &sc, "index", f, false);
        }
        let index_oids = cx.list(t, &sc, "index_oids");
        if index.len() != index_oids.len() {
            cx.errors.push(format!("`{sc}index_oids` must pin every `index` file (git rev-parse <commit>:<file>), in the same order"));
        }
        if index_oids.iter().any(|o| !is_hex(o, 40)) {
            cx.errors.push(format!("`{sc}index_oids` are 40 lower-case hex digits"));
        }
        let mut xcursor = Vec::new();
        for (j, x) in cx.tables(t, "xcursor") {
            let xs = format!("{sc}xcursor[{j}].");
            cx.known(x, &xs, &["name", "config", "aliases"]);
            let name = cx.text(x, &xs, "name", true, 64).unwrap_or_default();
            let config = cx.text_multiline(x, &xs, "config");
            let aliases = cx.list(x, &xs, "aliases");
            for n in std::iter::once(&name).chain(&aliases) {
                if !n.is_empty() && !super::xcursor::safe_name(n) {
                    cx.errors.push(format!("`{xs}`: `{n}` is not a cursor file name"));
                }
            }
            match super::xcursor::parse_config(&config) {
                Ok(images) => xcursor.push(super::xcursor::Cursor { name, images, aliases }),
                Err(e) => cx.errors.push(format!("`{xs}config`: {e}")),
            }
        }
        if xcursor.len() > super::xcursor::MAX_CURSORS {
            cx.errors.push(format!("`{sc}xcursor`: more than {} cursors", super::xcursor::MAX_CURSORS));
        }
        if !xcursor.is_empty() && !provides.contains(&Provides::Cursors) {
            cx.errors.push(format!("`{sc}xcursor` builds cursors: add \"cursors\" to provides"));
        }
        let mut text_links = Vec::new();
        for (j, x) in cx.tables(t, "text_links") {
            let xs = format!("{sc}text_links[{j}].");
            cx.known(x, &xs, &["from", "tree", "onto", "replace"]);
            let from = cx.text(x, &xs, "from", true, 200).unwrap_or_default();
            if !from.is_empty() {
                cx.rel(x, &xs, "from", &from, false);
                let inside = |a: &str, b: &str| b == "." || a == b || a.starts_with(&format!("{b}/"));
                if !path.is_empty() && (inside(&from, &path) || inside(&path, &from)) {
                    cx.errors.push(format!("`{xs}from`: `{from}` overlaps the pack's path `{path}`"));
                }
                if link_sources.iter().any(|l| inside(&from, l) || inside(l, &from)) {
                    cx.errors.push(format!("`{xs}from`: `{from}` overlaps a link_sources folder"));
                }
            }
            let tree = cx.text(x, &xs, "tree", true, 40).unwrap_or_default();
            if !tree.is_empty() && !is_hex(&tree, 40) {
                cx.errors.push(format!("`{xs}tree` must be 40 lower-case hex digits (git rev-parse <commit>:<from>)"));
            }
            let onto = cx.text(x, &xs, "onto", false, 200).unwrap_or_else(|| ".".into());
            cx.rel(x, &xs, "onto", &onto, true);
            let replace = match x.get("replace") {
                None => false,
                Some(i) => match i.as_bool() {
                    Some(b) => b,
                    None => {
                        cx.err(i, &format!("{xs}replace"), "must be true or false");
                        false
                    }
                },
            };
            text_links.push(super::aliases::TextLinks { from, tree, onto, replace });
        }
        if text_links.len() > super::aliases::MAX_TEXT_LINK_FOLDERS {
            cx.errors.push(format!("`{sc}text_links`: more than {} folders", super::aliases::MAX_TEXT_LINK_FOLDERS));
        }
        let mut dir_aliases = Vec::new();
        for (j, x) in cx.tables(t, "dir_alias") {
            let xs = format!("{sc}dir_alias[{j}].");
            cx.known(x, &xs, &["name", "target"]);
            let name = cx.text(x, &xs, "name", true, 200).unwrap_or_default();
            let target = cx.text(x, &xs, "target", true, 200).unwrap_or_default();
            for (k, v) in [("name", &name), ("target", &target)] {
                if !v.is_empty() {
                    cx.rel(x, &xs, k, v, false);
                }
            }
            if !name.is_empty() && (name == target || name.starts_with(&format!("{target}/")) || target.starts_with(&format!("{name}/"))) {
                cx.errors.push(format!("`{xs}`: `{name}` -> `{target}` encloses itself"));
            }
            dir_aliases.push(super::aliases::DirAlias { name, target });
        }
        if dir_aliases.len() > super::aliases::MAX_DIR_ALIASES {
            cx.errors.push(format!("`{sc}dir_alias`: more than {} folder aliases", super::aliases::MAX_DIR_ALIASES));
        }
        let dci = match t.get("dci_convert") {
            None => None,
            Some(item) => match item.as_table() {
                None => {
                    cx.err(item, &format!("{sc}dci_convert"), "must be a table ([pack.dci_convert])");
                    None
                }
                Some(x) => {
                    let xs = format!("{sc}dci_convert.");
                    cx.known(x, &xs, &["size", "inherits"]);
                    let size = match x.get("size").map(|i| (i, i.as_integer())) {
                        Some((_, Some(n))) if super::dci::SIZES.iter().any(|s| i64::from(*s) == n) => n as u32,
                        Some((i, _)) => {
                            cx.err(i, &format!("{xs}size"), format!("must be one of {:?}", super::dci::SIZES));
                            128
                        }
                        None => {
                            cx.errors.push(format!("`{xs}size` is required"));
                            128
                        }
                    };
                    let inherits = cx.list(x, &xs, "inherits");
                    if inherits.is_empty() || inherits.len() > 8 || inherits.iter().any(|n| !super::safe_theme_name(n)) {
                        cx.errors.push(format!("`{xs}inherits` lists 1 to 8 theme names"));
                    }
                    if provides != [Provides::Icons] {
                        cx.errors.push(format!("`{xs}` converts icons: provides must be [\"icons\"]"));
                    }
                    Some(super::dci::Convert { size, inherits, attribution: String::new() })
                }
            },
        };
        if dci.is_some() && (!xcursor.is_empty() || !text_links.is_empty() || !dir_aliases.is_empty() || !index.is_empty()) {
            cx.errors.push(format!("`{sc}dci_convert` does not combine with xcursor, text_links, dir_alias or index"));
        }
        let description = cx.text(t, &sc, "description", false, 400).unwrap_or_default();
        packs.push(Pack {
            id,
            name,
            provides,
            source,
            path,
            tree,
            install_name,
            license,
            license_files,
            license_oids,
            link_sources,
            link_source_trees,
            index,
            index_oids,
            xcursor,
            text_links,
            dir_aliases,
            dci,
            description,
        });
    }

    let mut recommends = Vec::new();
    for (i, t) in cx.tables(root, "recommend") {
        let sc = format!("recommend[{i}].");
        cx.known(t, &sc, &["theme", "icons", "cursors"]);
        let theme = cx.text(t, &sc, "theme", true, 64).unwrap_or_default();
        let icons = cx.text(t, &sc, "icons", false, 64);
        let cursors = cx.text(t, &sc, "cursors", false, 64);
        if icons.is_none() && cursors.is_none() {
            cx.errors.push(format!("`{sc}` recommends nothing (icons or cursors)"));
        }
        recommends.push(Recommend { theme, icons, cursors });
    }

    // references and uniqueness
    let mut errs = std::mem::take(&mut cx.errors);
    let dup = |what: &str, ids: Vec<&str>, errs: &mut Vec<String>| {
        let mut seen: Vec<&str> = Vec::new();
        for id in ids {
            if seen.contains(&id) {
                errs.push(format!("{what} id `{id}` is used twice"));
            }
            seen.push(id);
        }
    };
    dup("source", sources.iter().map(|s| s.id.as_str()).collect(), &mut errs);
    dup("pack", packs.iter().map(|s| s.id.as_str()).collect(), &mut errs);
    dup("recommend theme", recommends.iter().map(|s| s.theme.as_str()).collect(), &mut errs);
    let mut names: Vec<&str> = Vec::new();
    for p in &packs {
        if names.contains(&p.install_name.as_str()) {
            errs.push(format!("install_name `{}` is used by two packs", p.install_name));
        }
        names.push(&p.install_name);
        match sources.iter().find(|s| s.id == p.source) {
            None => errs.push(format!("pack `{}`: source `{}` is not a [[source]] of this file", p.id, p.source)),
            Some(s) => {
                if s.license_files.is_empty() && p.license_files.is_empty() {
                    errs.push(format!("pack `{}`: no licence file (license_files of the source or the pack)", p.id));
                }
                if s.license.trim().is_empty() && p.license.is_none() {
                    errs.push(format!("pack `{}`: no licence", p.id));
                }
            }
        }
    }
    for r in &recommends {
        for (k, v) in [(Provides::Icons, &r.icons), (Provides::Cursors, &r.cursors)] {
            let Some(v) = v else { continue };
            match packs.iter().find(|p| &p.id == v) {
                None => errs.push(format!("recommend `{}`: no pack `{v}` in this file", r.theme)),
                Some(p) if !p.provides(k) => errs.push(format!("recommend `{}`: pack `{v}` does not provide {}", r.theme, k.id())),
                _ => {}
            }
        }
    }
    if errs.is_empty() {
        Ok(Catalog { family, origin: origin.to_string(), sources, packs, recommends })
    } else {
        Err(errs.into_iter().map(|e| format!("{origin}: {e}")).collect())
    }
}

// ---------------------------------------------------------------- the set of catalogues

#[derive(Debug, Clone, Default)]
pub struct Set {
    pub catalogs: Vec<Catalog>,
    /// Catalogues that did not load (shown by `assets list` and the pane, never fatal).
    pub problems: Vec<String>,
}

impl Set {
    /// Adds a catalogue unless a pack id or a theme recommendation is already taken (the first one wins).
    pub fn push(&mut self, c: Catalog) {
        for p in &c.packs {
            if self.entries().any(|e| e.pack.id == p.id) {
                self.problems.push(format!("{}: pack id `{}` is already defined by another catalogue, file ignored", c.origin, p.id));
                return;
            }
        }
        self.catalogs.push(c);
    }

    pub fn entries(&self) -> impl Iterator<Item = Entry<'_>> {
        self.catalogs.iter().flat_map(|c| c.packs.iter().filter_map(move |p| c.sources.iter().find(|s| s.id == p.source).map(|s| Entry { catalog: c, pack: p, source: s })))
    }

    pub fn find(&self, id: &str) -> Option<Entry<'_>> {
        self.entries().find(|e| e.pack.id == id).or_else(|| self.entries().find(|e| e.pack.install_name == id))
    }

    /// What the theme `theme_id` recommends, from the first catalogue that says something about it.
    pub fn recommendation(&self, theme_id: &str) -> Option<&Recommend> {
        self.catalogs.iter().flat_map(|c| c.recommends.iter()).find(|r| r.theme == theme_id)
    }

    /// The compiled-in catalogues and the ones found at run time (see [`runtime_files`]).
    pub fn load() -> Set {
        let mut s = Set::default();
        for (family, text) in super::builtin::CATALOGS {
            match parse(text, &format!("builtin:{family}")) {
                Ok(c) => s.push(c),
                Err(e) => s.problems.extend(e),
            }
        }
        for f in runtime_files() {
            match std::fs::read_to_string(&f) {
                Ok(t) if t.len() <= 256 << 10 => match parse(&t, &f.display().to_string()) {
                    Ok(c) => s.push(c),
                    Err(e) => s.problems.extend(e),
                },
                Ok(_) => s.problems.push(format!("{}: larger than 256 KiB, ignored", f.display())),
                Err(e) => s.problems.push(format!("{}: {e}", f.display())),
            }
        }
        s
    }
}

/// `$DECEBAL_ASSET_CATALOGS` (files or folders, `:`-separated), `~/.config/decebal/asset-packs/*.toml`,
/// `$XDG_DATA_DIRS/decebal/asset-packs/*.toml`.
pub fn runtime_files() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(v) = std::env::var("DECEBAL_ASSET_CATALOGS") {
        for p in v.split(':').filter(|s| !s.is_empty()) {
            let p = PathBuf::from(p);
            if p.is_dir() {
                dirs.push(p);
            } else {
                out.push(p);
            }
        }
    }
    let cfg = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(c) = cfg {
        dirs.push(c.join("decebal/asset-packs"));
    }
    let data = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for d in data.split(':').filter(|s| !s.is_empty()) {
        dirs.push(Path::new(d).join("decebal/asset-packs"));
    }
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
        v.sort();
        out.extend(v);
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const GOOD: &str = r#"format = 1
family = "deepin"

[[source]]
id = "deepin-icon-theme"
repo = "https://github.com/linuxdeepin/deepin-icon-theme"
revision = "master"
commit = "5d11ba4e4013a4d1e1c601265d55c66b95184922"
license = "GPL-3.0-or-later"
license_files = ["COPYING"]
archive_url = "https://github.com/linuxdeepin/deepin-icon-theme/archive/5d11ba4e4013a4d1e1c601265d55c66b95184922.tar.gz"
archive_sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
strip_components = 1

[[pack]]
id = "deepin-bloom"
name = "Bloom"
provides = ["icons", "cursors"]
source = "deepin-icon-theme"
path = "bloom"
tree = "0000000000000000000000000000000000000000"

[[recommend]]
theme = "deepin"
icons = "deepin-bloom"
cursors = "deepin-bloom"
"#;

    #[test]
    fn a_good_catalogue_parses_with_defaults() {
        let c = parse(GOOD, "t").unwrap();
        assert_eq!(c.family, "deepin");
        let p = &c.packs[0];
        assert_eq!((p.install_name.as_str(), p.provides.as_slice()), ("bloom", &[Provides::Icons, Provides::Cursors][..]));
        assert_eq!(c.sources[0].strip, 1);
        let mut s = Set::default();
        s.push(c);
        let e = s.find("deepin-bloom").unwrap();
        assert_eq!((e.license(), e.license_files()), ("GPL-3.0-or-later", vec!["COPYING".to_string()]));
        assert!(s.find("bloom").is_some(), "found by install name too");
        assert_eq!(s.recommendation("deepin").unwrap().cursors.as_deref(), Some("deepin-bloom"));
        assert!(s.recommendation("nord").is_none());
    }

    #[test]
    fn errors_name_the_line_and_suggest_keys() {
        let bad = GOOD.replace("provides = [", "provide = [");
        let e = parse(&bad, "t").unwrap_err();
        assert!(e.iter().any(|x| x.contains("line 18") && x.contains("did you mean `provides`")), "{e:?}");
        for (from, to, want) in [
            ("format = 1", "format = 2", "must be 1"),
            ("https://github.com/linuxdeepin/deepin-icon-theme\"", "http://github.com/x\"", "https"),
            ("commit = \"5d11", "commit = \"XYZ", "40 lower-case hex"),
            ("path = \"bloom\"", "path = \"../bloom\"", "escapes"),
            ("license_files = [\"COPYING\"]", "license_files = []", "no licence file"),
            ("license = \"GPL-3.0-or-later\"", "license = \"\"", "must not be empty"),
            ("icons = \"deepin-bloom\"", "icons = \"nope\"", "no pack `nope`"),
            ("source = \"deepin-icon-theme\"", "source = \"other\"", "not a [[source]]"),
            ("archive_url = \"https://github.com/linuxdeepin/deepin-icon-theme/archive/5d11ba4e4013a4d1e1c601265d55c66b95184922.tar.gz\"\n", "", "needs an `archive_url`"),
            ("tree = \"0000000000000000000000000000000000000000\"", "tree = \"abc\"", "git rev-parse"),
        ] {
            assert!(GOOD.contains(from), "{from}");
            let e = parse(&GOOD.replacen(from, to, 1), "t").unwrap_err();
            assert!(e.iter().any(|x| x.contains(want)), "{from} -> {to}: {e:?}");
        }
        let e = parse(&GOOD.replace("provides = [\"icons\", \"cursors\"]", "provides = [\"cursors\"]"), "t").unwrap_err();
        assert!(e.iter().any(|x| x.contains("does not provide icons")), "{e:?}");
        assert!(parse("not toml = = =", "t").unwrap_err()[0].contains("line 1"));
    }

    #[test]
    fn the_documented_example_catalogues_stay_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/desktop-asset-packs/examples");
        let mut set = Set::default();
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let t = std::fs::read_to_string(e.path()).unwrap();
            let c = parse(&t, &e.path().display().to_string()).unwrap_or_else(|e| panic!("{}", e.join("\n")));
            assert!(!c.packs.is_empty() && !c.recommends.is_empty(), "{}", e.path().display());
            set.push(c);
            n += 1;
        }
        assert_eq!(n, 5, "deepin, openkylin, lingmo, kde, gnome");
        assert!(set.problems.is_empty(), "{:?}", set.problems);
    }

    #[test]
    fn index_files_need_one_pin_each() {
        let with = |extra: &str| GOOD.replace("tree = \"0000000000000000000000000000000000000000\"", &format!("tree = \"0000000000000000000000000000000000000000\"\n{extra}"));
        let c = parse(&with("index = \"index.theme\"\nindex_oids = [\"9e21cb977f3c28f5440234d2bc4618d22c14d32d\"]"), "t").unwrap();
        assert_eq!((c.packs[0].index.as_slice(), c.packs[0].index_oids.len()), (&["index.theme".to_string()][..], 1));
        let c = parse(
            &with("index = [\"icons/breeze.theme.in\", \"icons/commonthemeinfo.theme.in\"]\nindex_oids = [\"1ca7213bfb3c4655f42877764e42da3a2af5f0e6\", \"1ca7213bfb3c4655f42877764e42da3a2af5f0e6\"]"),
            "t",
        )
        .unwrap();
        assert_eq!(c.packs[0].index.len(), 2);
        assert!(parse(&with("index = \"index.theme\""), "t").unwrap_err().iter().any(|e| e.contains("must pin every")));
        assert!(parse(&with("index = \"../index.theme\"\nindex_oids = [\"9e21cb977f3c28f5440234d2bc4618d22c14d32d\"]"), "t").is_err());
        assert!(parse(&with("index = \"index.theme\"\nindex_oids = [\"xyz\"]"), "t").unwrap_err().iter().any(|e| e.contains("hex")));
    }

    #[test]
    fn xcursor_tables_parse_and_are_checked() {
        let x = "[[pack.xcursor]]\nname = \"left_ptr\"\nconfig = \"\"\"\n24 4 0 24x24/left_ptr.png\n32 5 0 32x32/left_ptr.png\n\"\"\"\naliases = [\"default\", \"arrow\"]\n\n[[recommend]]";
        let c = parse(&GOOD.replacen("[[recommend]]", x, 1), "t").unwrap();
        let xc = &c.packs[0].xcursor;
        assert_eq!((xc.len(), xc[0].images.len(), xc[0].aliases.len()), (1, 2, 2));
        let bad = |from: &str, to: &str| parse(&GOOD.replacen("[[recommend]]", &x.replace(from, to), 1), "t").unwrap_err();
        assert!(bad("24 4 0 24x24", "24 4 0 ../24x24").iter().any(|e| e.contains("escapes")));
        assert!(bad("\"arrow\"", "\"../arrow\"").iter().any(|e| e.contains("not a cursor file name")));
        assert!(bad("aliases", "alias").iter().any(|e| e.contains("did you mean `aliases`")));
        let icons_only = GOOD.replace("provides = [\"icons\", \"cursors\"]", "provides = [\"icons\"]").replace("cursors = \"deepin-bloom\"\n", "");
        let e = parse(&icons_only.replacen("[[recommend]]", x, 1), "t").unwrap_err();
        assert!(e.iter().any(|e| e.contains("add \"cursors\" to provides")), "{e:?}");
    }

    #[test]
    fn link_source_trees_pin_each_folder_and_folders_do_not_overlap() {
        let with = |extra: &str| GOOD.replace("path = \"bloom\"", &format!("path = \"bloom-dark\"\n{extra}"));
        let c = parse(&with("link_sources = [\"bloom\"]\nlink_source_trees = [\"9baca758b5bb6a8b8423d32ce3f83da07b95af6e\"]"), "t").unwrap();
        assert_eq!(c.packs[0].link_source_trees.len(), 1);
        assert!(parse(&with("link_sources = [\"bloom\"]"), "t").is_ok(), "optional: an archive_sha256 may cover it");
        assert!(parse(&with("link_sources = [\"bloom\", \"x\"]\nlink_source_trees = [\"9baca758b5bb6a8b8423d32ce3f83da07b95af6e\"]"), "t").unwrap_err().iter().any(|e| e.contains("every")));
        assert!(parse(&with("link_sources = [\"bloom\"]\nlink_source_trees = [\"nothex\"]"), "t").unwrap_err().iter().any(|e| e.contains("hex")));
        assert!(parse(&with("link_sources = [\"bloom-dark/sub\"]"), "t").unwrap_err().iter().any(|e| e.contains("overlaps")));
        assert!(parse(&GOOD.replace("path = \"bloom\"", "path = \"a/b\"\nlink_sources = [\"a\"]"), "t").unwrap_err().iter().any(|e| e.contains("overlaps")));
    }

    #[test]
    fn text_links_and_folder_aliases_are_declared_and_checked() {
        let x =
            "[[pack.text_links]]\nfrom = \"links\"\ntree = \"09d28c3591d256643c992999dc742b60336e6fc6\"\nreplace = true\n\n[[pack.dir_alias]]\nname = \"apps@2x\"\ntarget = \"apps\"\n\n[[recommend]]";
        let c = parse(&GOOD.replacen("[[recommend]]", x, 1), "t").unwrap();
        let p = &c.packs[0];
        assert_eq!((p.text_links.len(), p.text_links[0].onto.as_str(), p.text_links[0].replace, p.dir_aliases.len()), (1, ".", true, 1));
        assert!(parse(GOOD, "t").unwrap().packs[0].text_links.is_empty(), "existing manifests are unchanged");
        let bad = |from: &str, to: &str| parse(&GOOD.replacen("[[recommend]]", &x.replace(from, to), 1), "t").unwrap_err();
        assert!(bad("from = \"links\"", "from = \"bloom/links\"").iter().any(|e| e.contains("overlaps the pack's path")));
        assert!(bad("from = \"links\"", "from = \"../links\"").iter().any(|e| e.contains("escapes")));
        assert!(bad("tree = \"09d2", "tree = \"zz").iter().any(|e| e.contains("hex")));
        assert!(bad("replace = true", "replace = \"yes\"").iter().any(|e| e.contains("true or false")));
        assert!(bad("target = \"apps\"", "target = \"apps@2x/x\"").iter().any(|e| e.contains("encloses itself")));
        assert!(bad("target = \"apps\"", "target = \"/apps\"").iter().any(|e| e.contains("absolute")));
        assert!(bad("replace = true", "replace = true\nfollow = true").iter().any(|e| e.contains("unknown key")));
        assert!(bad("tree = \"09d28c3591d256643c992999dc742b60336e6fc6\"\n", "").iter().any(|e| e.contains("required")), "the pin is mandatory");
    }

    #[test]
    fn dci_conversion_is_one_typed_table() {
        let x = "[pack.dci_convert]\nsize = 128\ninherits = [\"bloom\"]\n\n[[recommend]]";
        let icons_only = GOOD.replace("provides = [\"icons\", \"cursors\"]", "provides = [\"icons\"]").replace("cursors = \"deepin-bloom\"\n", "");
        let c = parse(&icons_only.replacen("[[recommend]]", x, 1), "t").unwrap();
        assert_eq!(c.packs[0].dci.as_ref().map(|d| (d.size, d.inherits.clone())), Some((128, vec!["bloom".to_string()])));
        assert!(parse(GOOD, "t").unwrap().packs[0].dci.is_none(), "existing manifests are unchanged");
        let bad = |from: &str, to: &str| parse(&icons_only.replacen("[[recommend]]", &x.replace(from, to), 1), "t").unwrap_err();
        assert!(bad("size = 128", "size = 100").iter().any(|e| e.contains("must be one of")));
        assert!(bad("size = 128\n", "").iter().any(|e| e.contains("required")));
        assert!(bad("[\"bloom\"]", "[\"../etc\"]").iter().any(|e| e.contains("theme names")));
        assert!(bad("size = 128", "size = 128\nformat = \"png\"").iter().any(|e| e.contains("unknown key")));
        assert!(bad("[pack.dci_convert]", "[[pack.dci_convert]]").iter().any(|e| e.contains("must be a table")));
        let e = parse(&GOOD.replacen("[[recommend]]", x, 1), "t").unwrap_err();
        assert!(e.iter().any(|e| e.contains("provides must be")), "{e:?}");
    }

    #[test]
    fn the_first_catalogue_owns_a_pack_id() {
        let mut s = Set::default();
        s.push(parse(GOOD, "a").unwrap());
        s.push(parse(GOOD, "b").unwrap());
        assert_eq!(s.catalogs.len(), 1);
        assert!(s.problems[0].contains("already defined"));
    }
}
