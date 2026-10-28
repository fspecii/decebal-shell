//! `index.theme` parsing and the directory arithmetic of the freedesktop Icon Theme specification.

/// `Type=` of a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirType {
    Fixed,
    Scalable,
    Threshold,
}

/// One entry of `Directories=` / `ScaledDirectories=` with its own section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dir {
    /// Relative to the theme folder, e.g. `48x48/apps` or `scalable/places`.
    pub path: String,
    pub size: u32,
    pub min: u32,
    pub max: u32,
    pub scale: u32,
    pub threshold: u32,
    pub kind: DirType,
    pub context: String,
}

impl Dir {
    /// Nominal pixels of the files in this directory (`Size * Scale`).
    pub fn pixels(&self) -> u32 {
        self.size * self.scale
    }

    /// `DirectorySizeDistance` of the specification, in physical pixels: 0 when this directory serves `px` pixels
    /// as is (a fixed directory of exactly that size, or a scalable / threshold one whose range contains it).
    pub fn distance(&self, px: u32) -> u32 {
        let (lo, hi) = match self.kind {
            DirType::Fixed => (self.pixels(), self.pixels()),
            DirType::Scalable => (self.min * self.scale, self.max * self.scale),
            DirType::Threshold => (self.size.saturating_sub(self.threshold) * self.scale, (self.size + self.threshold) * self.scale),
        };
        if px < lo {
            lo - px
        } else {
            px.saturating_sub(hi)
        }
    }
}

/// The parts of an `index.theme` the lookup and the settings list need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub comment: String,
    pub inherits: Vec<String>,
    pub hidden: bool,
    pub dirs: Vec<Dir>,
}

fn num(v: Option<&String>) -> Option<u32> {
    v.and_then(|s| s.trim().parse::<u32>().ok())
}

/// Parses the text of an `index.theme`. Directories without a section or without `Size=` are dropped; unknown
/// keys and localised names (`Name[de]`) are ignored. Never fails: a broken file gives an empty directory list.
pub fn parse(text: &str) -> Index {
    use std::collections::HashMap;
    let mut sections: Vec<(String, HashMap<String, String>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_string(), HashMap::new()));
        } else if let (Some((k, v)), Some((_, map))) = (line.split_once('='), sections.last_mut()) {
            map.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    let list = |s: Option<&String>| -> Vec<String> { s.map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default() };
    let mut ix = Index::default();
    let Some(head) = sections.iter().find(|(n, _)| n == "Icon Theme").map(|(_, m)| m) else { return ix };
    ix.name = head.get("Name").cloned().unwrap_or_default();
    ix.comment = head.get("Comment").cloned().unwrap_or_default();
    ix.inherits = list(head.get("Inherits"));
    ix.hidden = head.get("Hidden").is_some_and(|h| h.eq_ignore_ascii_case("true"));
    let mut names = list(head.get("Directories"));
    names.extend(list(head.get("ScaledDirectories")));
    for name in names {
        if ix.dirs.iter().any(|d| d.path == name) {
            continue;
        }
        let Some((_, m)) = sections.iter().find(|(n, _)| *n == name) else { continue };
        let Some(size) = num(m.get("Size")).filter(|s| *s > 0) else { continue };
        let kind = match m.get("Type").map(|t| t.to_ascii_lowercase()).as_deref() {
            Some("fixed") => DirType::Fixed,
            Some("scalable") => DirType::Scalable,
            _ => DirType::Threshold,
        };
        ix.dirs.push(Dir {
            path: name,
            size,
            min: num(m.get("MinSize")).unwrap_or(size),
            max: num(m.get("MaxSize")).unwrap_or(size),
            scale: num(m.get("Scale")).filter(|s| *s > 0).unwrap_or(1),
            threshold: num(m.get("Threshold")).unwrap_or(2),
            kind,
            context: m.get("Context").cloned().unwrap_or_default(),
        });
    }
    ix
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# comment
[Icon Theme]
Name=Sample
Name[de]=Beispiel
Comment=A theme
Inherits=Parent, hicolor
Directories=16x16/apps,48x48/apps,48x48@2x/apps,scalable/apps,missing/dir,noname
ScaledDirectories=24x24@2x/apps

[16x16/apps]
Size=16
Context=Applications
Type=Fixed

[48x48/apps]
Size=48
Type=Threshold
Threshold=4

[48x48@2x/apps]
Size=48
Scale=2
Type=Fixed

[scalable/apps]
Size=128
MinSize=8
MaxSize=512
Type=Scalable

[24x24@2x/apps]
Size=24
Scale=2

[noname]
Context=Nothing
";

    #[test]
    fn parses_header_inherits_and_directories() {
        let ix = parse(SAMPLE);
        assert_eq!((ix.name.as_str(), ix.comment.as_str()), ("Sample", "A theme"));
        assert_eq!(ix.inherits, ["Parent", "hicolor"]);
        let paths: Vec<_> = ix.dirs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["16x16/apps", "48x48/apps", "48x48@2x/apps", "scalable/apps", "24x24@2x/apps"], "no section / no Size is dropped, ScaledDirectories appended");
        let d = &ix.dirs[1];
        assert_eq!((d.kind, d.threshold, d.min, d.max, d.scale), (DirType::Threshold, 4, 48, 48, 1));
        assert_eq!(ix.dirs[0].context, "Applications");
        assert_eq!(ix.dirs[2].scale, 2);
        assert_eq!((ix.dirs[3].min, ix.dirs[3].max), (8, 512));
        assert_eq!(ix.dirs[4].kind, DirType::Threshold, "default Type is Threshold");
        assert_eq!(ix.dirs[4].threshold, 2, "default Threshold is 2");
        assert!(!ix.hidden);
    }

    #[test]
    fn distances_follow_the_specification() {
        let ix = parse(SAMPLE);
        let d = |i: usize, px: u32| ix.dirs[i].distance(px);
        assert_eq!((d(0, 16), d(0, 20), d(0, 8)), (0, 4, 8), "fixed: plain difference");
        assert_eq!((d(1, 48), d(1, 52), d(1, 44), d(1, 53), d(1, 43)), (0, 0, 0, 1, 1), "threshold 4 around 48");
        assert_eq!((d(2, 96), d(2, 48), d(2, 100)), (0, 48, 4), "a @2x directory holds 96 pixel files");
        assert_eq!((d(3, 8), d(3, 512), d(3, 300), d(3, 6), d(3, 600)), (0, 0, 0, 2, 88), "scalable: the whole range");
    }

    #[test]
    fn hidden_and_garbage_are_tolerated() {
        assert!(parse("[Icon Theme]\nName=X\nHidden=true\n").hidden);
        assert_eq!(parse("not an ini file at all"), Index::default());
        assert_eq!(parse("").dirs.len(), 0);
        let ix = parse("[Icon Theme]\nDirectories=a,a\n[a]\nSize=32\n");
        assert_eq!(ix.dirs.len(), 1, "a directory listed twice counts once");
    }
}
