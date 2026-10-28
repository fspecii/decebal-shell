//! `manifest.toml`: what a preset is called, what it contains and how it applies.

use super::bundle;
use super::parts::{Part, PartSet};

pub const FORMAT: i64 = 1;
pub const MAX_NAME: usize = 64;
pub const MAX_DESC: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    /// Saved from this desktop.
    #[default]
    Local,
    /// Came in through an import.
    Imported,
    /// Shipped with decebal (never on disk).
    Builtin,
    /// Written by "Apply" as a restore point.
    Backup,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Local => "local",
            Origin::Imported => "imported",
            Origin::Builtin => "builtin",
            Origin::Backup => "backup",
        }
    }
    fn parse(s: &str) -> Origin {
        match s {
            "imported" => Origin::Imported,
            "builtin" => Origin::Builtin,
            "backup" => Origin::Backup,
            _ => Origin::Local,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    pub author: String,
    /// ISO 8601 UTC, informational.
    pub created: String,
    pub app_version: String,
    pub parts: PartSet,
    /// Overlay (`true`) or replace (`false`, the default) the parts on apply; see `parts::merge_json`.
    pub merge: bool,
    /// Bundle name of the embedded wallpaper (`wallpaper/<file>`).
    pub wallpaper: Option<String>,
    /// Bundle name of the embedded custom theme (`themes/<id>.toml`).
    pub theme_file: Option<String>,
    /// Commands were removed when this preset was imported.
    pub commands_stripped: bool,
    pub origin: Origin,
}

impl Manifest {
    pub fn new(name: &str, parts: PartSet) -> Manifest {
        Manifest {
            name: name.trim().to_string(),
            description: String::new(),
            author: String::new(),
            created: String::new(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            parts,
            merge: false,
            wallpaper: None,
            theme_file: None,
            commands_stripped: false,
            origin: Origin::Local,
        }
    }

    pub fn render(&self) -> String {
        let q = |s: &str| toml_edit::Value::from(s).to_string().trim().to_string();
        let mut s = format!("format = {FORMAT}\nname = {}\n", q(&self.name));
        if !self.description.is_empty() {
            s.push_str(&format!("description = {}\n", q(&self.description)));
        }
        if !self.author.is_empty() {
            s.push_str(&format!("author = {}\n", q(&self.author)));
        }
        if !self.created.is_empty() {
            s.push_str(&format!("created = {}\n", q(&self.created)));
        }
        s.push_str(&format!("app_version = {}\n", q(&self.app_version)));
        s.push_str(&format!("parts = [{}]\n", self.parts.ids().iter().map(|p| q(p)).collect::<Vec<_>>().join(", ")));
        s.push_str(&format!("mode = {}\n", q(if self.merge { "merge" } else { "replace" })));
        if let Some(w) = &self.wallpaper {
            s.push_str(&format!("wallpaper_file = {}\n", q(w)));
        }
        if let Some(t) = &self.theme_file {
            s.push_str(&format!("theme_file = {}\n", q(t)));
        }
        if self.commands_stripped {
            s.push_str("commands_stripped = true\n");
        }
        if self.origin != Origin::Local {
            s.push_str(&format!("origin = {}\n", q(self.origin.as_str())));
        }
        s
    }

    /// Parses and validates. Everything a bundle's author controls is checked here.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| format!("manifest.toml: {}", e.message()))?;
        let get_str = |k: &str| -> Result<Option<String>, String> {
            match doc.get(k) {
                None => Ok(None),
                Some(i) => i.as_str().map(|s| Some(s.to_string())).ok_or_else(|| format!("manifest.toml: `{k}` must be text")),
            }
        };
        match doc.get("format").and_then(|f| f.as_integer()) {
            Some(FORMAT) => {}
            Some(n) => return Err(format!("this preset uses format {n}; this decebal understands format {FORMAT}")),
            None => return Err("manifest.toml: `format` is missing".into()),
        }
        let name = get_str("name")?.ok_or("manifest.toml: `name` is missing")?.trim().to_string();
        check_name(&name)?;
        let description = get_str("description")?.unwrap_or_default();
        if description.chars().count() > MAX_DESC || description.chars().any(|c| c.is_control() && c != '\n') {
            return Err(format!("description is too long or has control characters (max {MAX_DESC})"));
        }
        let author = get_str("author")?.unwrap_or_default();
        if author.chars().count() > 64 || author.chars().any(|c| c.is_control()) {
            return Err("author is too long".into());
        }
        let created = get_str("created")?.unwrap_or_default();
        if created.len() > 40 || created.chars().any(|c| c.is_control()) {
            return Err("bad `created` date".into());
        }
        let app_version = get_str("app_version")?.unwrap_or_default();
        if app_version.len() > 32 || app_version.chars().any(|c| c.is_control()) {
            return Err("bad `app_version`".into());
        }
        let arr = doc.get("parts").and_then(|p| p.as_array()).ok_or("manifest.toml: `parts` must be a list")?;
        let mut parts = Vec::new();
        for v in arr.iter() {
            let id = v.as_str().ok_or("manifest.toml: `parts` must be a list of names")?;
            let p = Part::parse(id).ok_or_else(|| format!("unknown part `{}`", id.chars().take(30).collect::<String>().escape_default()))?;
            if parts.contains(&p) {
                return Err(format!("part `{id}` is listed twice"));
            }
            parts.push(p);
        }
        if parts.is_empty() {
            return Err("the preset has no parts".into());
        }
        let merge = match get_str("mode")?.as_deref() {
            None | Some("replace") => false,
            Some("merge") => true,
            Some(other) => return Err(format!("unknown mode `{}`", other.chars().take(20).collect::<String>().escape_default())),
        };
        let wallpaper = get_str("wallpaper_file")?;
        if let Some(w) = &wallpaper {
            if bundle::check_name(w) != Ok(bundle::NameKind::Wallpaper) {
                return Err(format!("`wallpaper_file` must name a file under wallpaper/ (got `{}`)", w.escape_default()));
            }
        }
        let theme_file = get_str("theme_file")?;
        if let Some(t) = &theme_file {
            if bundle::theme_id(t).is_none() {
                return Err(format!("`theme_file` must name a file under themes/ (got `{}`)", t.escape_default()));
            }
        }
        let commands_stripped = doc.get("commands_stripped").and_then(|b| b.as_bool()).unwrap_or(false);
        let origin = Origin::parse(get_str("origin")?.as_deref().unwrap_or("local"));
        Ok(Manifest { name, description, author, created, app_version, parts: PartSet::new(parts), merge, wallpaper, theme_file, commands_stripped, origin })
    }
}

/// A preset name people can read and a folder name can be derived from.
pub fn check_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("give the preset a name".into());
    }
    if n.chars().count() > MAX_NAME {
        return Err(format!("the name is longer than {MAX_NAME} characters"));
    }
    if n.chars().any(|c| c.is_control()) {
        return Err("the name has control characters".into());
    }
    if slug(n).is_empty() {
        return Err("the name needs letters or digits".into());
    }
    Ok(())
}

/// Folder name for a preset: lowercase ASCII letters, digits and dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.truncate(48);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let mut m = Manifest::new("Caelestia \"night\"", PartSet::new([Part::Shell, Part::Dock, Part::Rdwm]));
        m.description = "Dark & cosy.\nSecond line".into();
        m.author = "me".into();
        m.created = "2026-10-28T12:00:00Z".into();
        m.wallpaper = Some("wallpaper/a.png".into());
        m.theme_file = Some("themes/mine.toml".into());
        m.merge = true;
        m.commands_stripped = true;
        m.origin = Origin::Imported;
        m
    }

    #[test]
    fn round_trip() {
        let m = sample();
        let t = m.render();
        assert_eq!(Manifest::parse(&t).unwrap(), m, "{t}");
        let plain = Manifest::new("x", PartSet::new([Part::Shell]));
        assert_eq!(Manifest::parse(&plain.render()).unwrap(), plain);
    }

    #[test]
    fn validation() {
        let ok = "format = 1\nname = \"a\"\nparts = [\"shell\"]\n";
        assert!(Manifest::parse(ok).is_ok());
        let cases = [
            ("format = 2\nname = \"a\"\nparts = [\"shell\"]\n", "format 2"),
            ("name = \"a\"\nparts = [\"shell\"]\n", "`format`"),
            ("format = 1\nparts = [\"shell\"]\n", "`name`"),
            ("format = 1\nname = \"  \"\nparts = [\"shell\"]\n", "name"),
            ("format = 1\nname = \"a\"\nparts = []\n", "no parts"),
            ("format = 1\nname = \"a\"\nparts = [\"shell\", \"shell\"]\n", "twice"),
            ("format = 1\nname = \"a\"\nparts = [\"root\"]\n", "unknown part"),
            ("format = 1\nname = \"a\"\nparts = [\"shell\"]\nmode = \"evil\"\n", "unknown mode"),
            ("format = 1\nname = \"a\"\nparts = [\"shell\"]\nwallpaper_file = \"../x.png\"\n", "wallpaper_file"),
            ("format = 1\nname = \"a\"\nparts = [\"shell\"]\ntheme_file = \"themes/x.sh\"\n", "theme_file"),
            ("format = 1\nname = 5\nparts = [\"shell\"]\n", "text"),
            ("format = 1\nname = \"a\"\nparts = \"shell\"\n", "list"),
            ("format = 1\nname = \"a\"\nparts = [1]\n", "names"),
            ("format = 1\nname = \"a\u{7}\"\nparts = [\"shell\"]\n", "manifest.toml"),
            ("this is = = not toml", "manifest.toml"),
        ];
        for (text, want) in cases {
            let e = Manifest::parse(text).unwrap_err();
            assert!(e.contains(want), "{text:?}: {e}");
        }
        let long = format!("format = 1\nname = \"{}\"\nparts = [\"shell\"]\n", "a".repeat(65));
        assert!(Manifest::parse(&long).is_err());
        let long = format!("format = 1\nname = \"a\"\ndescription = \"{}\"\nparts = [\"shell\"]\n", "d".repeat(401));
        assert!(Manifest::parse(&long).is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Caelestia Night!"), "caelestia-night");
        assert_eq!(slug("  --A__b  "), "a-b");
        assert_eq!(slug("../../etc"), "etc");
        assert_eq!(slug("日本語"), "");
        assert!(check_name("日本語").is_err());
        assert!(slug(&"ab ".repeat(40)).len() <= 48);
        assert!(!slug(&"ab ".repeat(40)).ends_with('-'));
    }
}
