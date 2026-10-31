//! Declared aliases that an upstream build would otherwise create, opt-in per pack (see `docs/desktop-asset-packs.md`).
//!
//! * `[[pack.text_links]]`: a folder of the source whose **regular files** each hold a symlink target as text (a link
//!   tree stored without its link modes, as Lingmo's `links/`). Only a folder declared here is read this way, and only
//!   after its own git tree id was verified. Each text must be a plain relative path (UTF-8, at most
//!   [`MAX_ALIAS_TEXT`] bytes, no control character, backslash or leading `/`) that resolves inside the pack folder from
//!   the place the file lands (`onto`); anything else refuses the pack.
//! * `[[pack.dir_alias]]`: a folder link the build creates (Lingmo's `actions@2x -> actions`), checked to point at a
//!   folder inside the pack that does not enclose it.
//!
//! Both become ordinary link nodes of the staged source. Everything after that is the installer's existing, bounded
//! handling of links: chains collapse (at most `MAX_HOPS`), loops and links to nothing are dropped with a bounded
//! number of warnings, links never leave the pack.

use super::source::{join_link, Node, Resolved, Staged};

/// Longest alias text read (real ones are under 80 bytes).
pub const MAX_ALIAS_TEXT: u64 = 1024;
pub const MAX_TEXT_LINK_FOLDERS: usize = 4;
pub const MAX_DIR_ALIASES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLinks {
    /// Source-relative folder holding the alias files.
    pub from: String,
    /// `git rev-parse <commit>:<from>`.
    pub tree: String,
    /// Where the folder's files land, relative to the pack folder (`.` = its root).
    pub onto: String,
    /// An alias named like a file of the pack replaces it (upstream copied the links over the files); otherwise refused.
    pub replace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirAlias {
    /// Pack-relative name of the folder link.
    pub name: String,
    /// Pack-relative folder it points at.
    pub target: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub text_links: usize,
    /// Pack files an alias replaced.
    pub replaced: usize,
    pub dir_aliases: usize,
}

/// The target an alias file holds, or why it is not one.
pub fn parse_target(data: &[u8]) -> Result<&str, String> {
    if data.is_empty() {
        return Err("it is empty".into());
    }
    if data.len() as u64 > MAX_ALIAS_TEXT {
        return Err(format!("it is longer than {MAX_ALIAS_TEXT} bytes"));
    }
    let t = std::str::from_utf8(data).map_err(|_| "it is not UTF-8 text".to_string())?;
    if t.chars().any(char::is_control) {
        return Err("it contains a control character or line break".into());
    }
    if t.contains('\\') {
        return Err("it contains a backslash".into());
    }
    if t.starts_with('/') {
        return Err("it is an absolute path".into());
    }
    if t.split('/').any(str::is_empty) {
        return Err("it has an empty path component".into());
    }
    Ok(t)
}

fn under<'a>(p: &'a str, dir: &str) -> Option<&'a str> {
    if dir == "." {
        return Some(p);
    }
    p.strip_prefix(dir).and_then(|r| r.strip_prefix('/'))
}

fn join(a: &str, b: &str) -> String {
    match (a, b) {
        (".", b) => b.to_string(),
        (a, ".") => a.to_string(),
        (a, b) => format!("{a}/{b}"),
    }
}

/// Refuses `p` when one of its ancestors is a file or a link of the staged source.
fn check_ancestors(st: &Staged, p: &str) -> Result<(), String> {
    let parts: Vec<&str> = p.split('/').collect();
    for i in 1..parts.len() {
        let a = parts[..i].join("/");
        if matches!(st.nodes.get(&a), Some(Node::File { .. } | Node::Link(_))) {
            return Err(format!("alias `{p}` would lie below `{a}`, which is not a folder"));
        }
    }
    Ok(())
}

fn is_folder(st: &Staged, p: &str) -> bool {
    matches!(st.nodes.get(p), Some(Node::Dir)) || st.nodes.range(format!("{p}/")..).next().is_some_and(|(k, _)| k.starts_with(&format!("{p}/")))
}

/// Turns the declared aliases of a pack (`pack` = its source-relative folder) into link nodes of `st`. Call after the
/// pins of the text-link folders were verified.
pub fn apply(st: &mut Staged, pack: &str, text: &[TextLinks], dirs: &[DirAlias]) -> Result<Report, String> {
    let mut r = Report::default();
    for t in text {
        let prefix = format!("{}/", t.from);
        let mut found: Vec<(String, String)> = Vec::new();
        for (k, node) in st.nodes.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)) {
            let rel = &k[prefix.len()..];
            match node {
                Node::Dir => {}
                Node::Link(_) => return Err(format!("text_links folder `{}` holds the symlink `{rel}`: only regular files are read as aliases", t.from)),
                Node::File { staged: false, .. } => return Err(format!("alias `{k}` was not read")),
                Node::File { size, .. } => {
                    if *size > MAX_ALIAS_TEXT {
                        return Err(format!("alias `{k}` is not a link target: it is longer than {MAX_ALIAS_TEXT} bytes"));
                    }
                    let data = std::fs::read(st.file_path(k)).map_err(|e| format!("alias `{k}`: {e}"))?;
                    let target = parse_target(&data).map_err(|e| format!("alias `{k}` is not a link target: {e}"))?.to_string();
                    let dest = join(&join(pack, &t.onto), rel);
                    super::safe_rel(&dest)?;
                    // relative to where the alias lands, it must stay inside the pack folder
                    match join_link(&dest, &target) {
                        Some(to) if under(&to, pack).is_some_and(|x| !x.is_empty()) => {}
                        _ => return Err(format!("alias `{k}` -> `{target}` leaves the pack folder")),
                    }
                    found.push((dest, target));
                }
            }
        }
        for (dest, target) in found {
            check_ancestors(st, &dest)?;
            match st.nodes.get(&dest) {
                None if is_folder(st, &dest) => return Err(format!("alias `{dest}` names a folder of the pack")),
                None => {}
                Some(Node::Dir) => return Err(format!("alias `{dest}` names a folder of the pack")),
                Some(_) if t.replace => r.replaced += 1,
                Some(_) => return Err(format!("alias `{dest}` would replace a file of the pack; `replace = true` follows a build that copies the links over the files")),
            }
            st.nodes.insert(dest, Node::Link(target));
            r.text_links += 1;
        }
    }
    for d in dirs {
        for p in [&d.name, &d.target] {
            super::safe_rel(p)?;
        }
        let (name, target) = (join(pack, &d.name), join(pack, &d.target));
        if d.name == d.target || d.name.starts_with(&format!("{}/", d.target)) || d.target.starts_with(&format!("{}/", d.name)) {
            return Err(format!("folder alias `{}` -> `{}` encloses itself", d.name, d.target));
        }
        match st.resolve(&target) {
            Resolved::Dir(t) if under(&t, pack).is_some_and(|x| !x.is_empty()) => {}
            _ => return Err(format!("folder alias `{}`: `{}` is not a folder of the pack", d.name, d.target)),
        }
        check_ancestors(st, &name)?;
        if st.nodes.contains_key(&name) || is_folder(st, &name) {
            return Err(format!("folder alias `{}` names something the pack already has", d.name));
        }
        st.nodes.insert(name.clone(), Node::Link(super::assemble::relative(&d.name, &d.target)));
        r.dir_aliases += 1;
    }
    Ok(r)
}

/// Refuses a pack whose folders, as they would be published, form a cycle: the graph is every real folder of the pack
/// (implicit parents included) with its containment edges (parent -> child) plus, for every link inside the pack that
/// resolves to a folder inside the pack (declared `dir_alias`, a text alias naming a folder, or an ordinary symlink of
/// the source), an edge from the link's folder to that folder. A cycle there means a walker following folder links
/// recurses forever (`a/to-b -> b`, `b/to-a -> a`, or longer through nested folders), although each link resolves to a
/// real folder. Links the build drops anyway (to the pack root, to themselves or to a folder enclosing them) are not
/// edges. Links into `link_sources` are expanded into copies by the build, with their own guard, and are not covered.
/// Iterative depth-first search: linear in folders and links, no recursion. Returns the number of folder links seen.
pub fn check_dir_graph(st: &Staged, pack: &str) -> Result<usize, String> {
    use std::collections::HashMap;
    let parent = |p: &str| -> String { p.rsplit_once('/').map_or(String::new(), |(d, _)| d.to_string()) };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let id = |p: &str, index: &mut HashMap<String, usize>, names: &mut Vec<String>| -> usize {
        if let Some(&i) = index.get(p) {
            return i;
        }
        index.insert(p.to_string(), names.len());
        names.push(p.to_string());
        names.len() - 1
    };
    id("", &mut index, &mut names);
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut links = 0;
    // every folder: the Dir nodes and the parents of every node, with their containment edges
    for (k, node) in st.nodes.iter() {
        let Some(rel) = under(k, pack).filter(|r| !r.is_empty()) else { continue };
        let mut d = if matches!(node, Node::Dir) { rel.to_string() } else { parent(rel) };
        while !d.is_empty() && !index.contains_key(&d) {
            let (child, up) = (id(&d, &mut index, &mut names), parent(&d));
            let p = id(&up, &mut index, &mut names);
            edges.push((p, child));
            d = up;
        }
    }
    for (k, node) in st.nodes.iter() {
        let (Node::Link(_), Some(lrel)) = (node, under(k, pack)) else { continue };
        let Resolved::Dir(t) = st.resolve(k) else { continue };
        let Some(trel) = under(&t, pack) else { continue };
        if trel.is_empty() || lrel == trel || lrel.starts_with(&format!("{trel}/")) {
            continue; // dropped by the build, never published
        }
        let (from, to) = (id(&parent(lrel), &mut index, &mut names), id(trel, &mut index, &mut names));
        edges.push((from, to));
        links += 1;
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    for (a, b) in edges {
        adj[a].push(b);
    }
    // 0 = not seen, 1 = on the current path, 2 = done
    let mut colour = vec![0u8; names.len()];
    for start in 0..names.len() {
        if colour[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        colour[start] = 1;
        while let Some(&mut (n, ref mut next)) = stack.last_mut() {
            if let Some(&m) = adj[n].get(*next) {
                *next += 1;
                match colour[m] {
                    0 => {
                        colour[m] = 1;
                        stack.push((m, 0));
                    }
                    1 => {
                        let from = stack.iter().position(|(x, _)| *x == m).unwrap_or(0);
                        let mut path: Vec<&str> = stack[from..].iter().map(|(x, _)| if names[*x].is_empty() { "." } else { names[*x].as_str() }).collect();
                        let more = path.len().saturating_sub(8);
                        path.truncate(8);
                        return Err(format!(
                            "folder links form a cycle: {} -> {}{}",
                            path.join(" -> "),
                            if names[m].is_empty() { "." } else { &names[m] },
                            if more > 0 { format!(" ({more} more folders on the way)") } else { String::new() }
                        ));
                    }
                    _ => {}
                }
            } else {
                colour[n] = 2;
                stack.pop();
            }
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::super::assemble::{build, keep, Spec};
    use super::super::source::{from_tar, Filter};
    use super::super::tar::Format;
    use super::super::testkit::*;
    use super::super::{Limits, Provides};
    use super::*;

    const INDEX: &[u8] = b"[Icon Theme]\nName=T\nDirectories=apps/48,apps@2x/48\n[apps/48]\nSize=48\nType=Fixed\n[apps@2x/48]\nSize=48\nScale=2\nType=Fixed\n";

    /// `src/` (the pack) with real icons, and `links/` with alias files as Lingmo stores them (regular files of text).
    fn source(extra: Vec<E>) -> Vec<E> {
        let mut v = vec![
            E::dir("top/"),
            E::file("top/LICENSE", b"GPL\n"),
            E::file("top/src/index.theme", INDEX),
            E::file("top/src/apps/48/real.svg", SVG),
            E::file("top/src/apps/48/over.svg", SVG),
            E::file("top/links/apps/48/alias.svg", b"real.svg"),
            E::file("top/links/apps/48/chain.svg", b"alias.svg"),
            E::file("top/links/apps/48/over.svg", b"real.svg"),
            E::file("top/links/apps/48/deeper.svg", b"../48/real.svg"),
            E::file("top/links/apps/48/dangling.svg", b"nothing.svg"),
            E::file("top/links/apps/48/loop1.svg", b"loop2.svg"),
            E::file("top/links/apps/48/loop2.svg", b"loop1.svg"),
        ];
        v.extend(extra);
        v
    }

    fn spec(replace: bool, dirs: Vec<DirAlias>) -> Spec {
        Spec {
            provides: vec![Provides::Icons],
            path: "src".into(),
            install_name: "t".into(),
            display_name: "T".into(),
            license_files: vec!["LICENSE".into()],
            license_oids: Default::default(),
            index_files: vec![],
            xcursors: vec![],
            text_links: vec![TextLinks { from: "links".into(), tree: String::new(), onto: ".".into(), replace }],
            dir_aliases: dirs,
        }
    }

    fn at2x() -> Vec<DirAlias> {
        vec![DirAlias { name: "apps@2x".into(), target: "apps".into() }]
    }

    /// Stages `entries`, applies the aliases of `sp`, builds into a fresh folder.
    fn run(entries: &[E], sp: &Spec, lim: &Limits) -> (Result<(Report, super::super::assemble::Built), String>, Scratch) {
        let s = Scratch::new("aliases");
        std::fs::create_dir_all(s.path().join("src")).unwrap();
        std::fs::create_dir_all(s.path().join("out")).unwrap();
        let f = Filter { strip: 1, path: sp.path.clone(), files: sp.license_files.clone(), text_links: sp.text_links.iter().map(|t| t.from.clone()).collect(), ..Filter::default() };
        let r = (|| {
            let mut st = from_tar(&targz(entries, None)[..], Format::TarGz, s.path().join("src"), f, lim, &keep)?;
            let rep = apply(&mut st, &sp.path, &sp.text_links, &sp.dir_aliases)?;
            check_dir_graph(&st, &sp.path)?;
            Ok((rep, build(&st, sp, &s.path().join("out"), lim)?))
        })();
        (r, s)
    }

    #[test]
    fn alias_texts_are_plain_relative_paths() {
        for ok in [&b"real.svg"[..], b"../48/real.svg", b"a/b/c.svg", "ünïcode.svg".as_bytes()] {
            assert!(parse_target(ok).is_ok(), "{ok:?}");
        }
        for (bad, why) in [
            (&b""[..], "empty"),
            (b"real.svg\n", "control"),
            (b"a\tb", "control"),
            (b"\xff\xfe", "UTF-8"),
            (b"/etc/passwd", "absolute"),
            (b"a\\b", "backslash"),
            (b"a//b", "empty path"),
            (b"a/", "empty path"),
        ] {
            let e = parse_target(bad).unwrap_err();
            assert!(e.contains(why), "{bad:?}: {e}");
        }
        assert!(parse_target(&vec![b'a'; MAX_ALIAS_TEXT as usize + 1]).unwrap_err().contains("longer"));
    }

    #[test]
    fn declared_text_links_and_folder_aliases_install_as_links_inside_the_pack() {
        let (r, s) = run(&source(vec![]), &spec(true, at2x()), &Limits::default());
        let (rep, b) = r.unwrap();
        assert_eq!(rep, Report { text_links: 7, replaced: 1, dir_aliases: 1 });
        let out = s.path().join("out");
        let link = |p: &str| std::fs::read_link(out.join(p)).ok().map(|t| t.display().to_string());
        assert_eq!(link("apps/48/alias.svg").as_deref(), Some("real.svg"));
        assert_eq!(link("apps/48/chain.svg").as_deref(), Some("real.svg"), "chains collapse to the final file");
        assert_eq!(link("apps/48/over.svg").as_deref(), Some("real.svg"), "replace = true: the alias wins, as upstream's copy order");
        assert_eq!(link("apps/48/deeper.svg").as_deref(), Some("real.svg"));
        assert_eq!(link("apps@2x").as_deref(), Some("apps"), "the folder alias");
        for gone in ["apps/48/dangling.svg", "apps/48/loop1.svg", "apps/48/loop2.svg"] {
            assert!(std::fs::symlink_metadata(out.join(gone)).is_err(), "{gone} dropped");
        }
        assert!(!out.join("links").exists(), "the alias files themselves are not installed");
        assert_eq!((b.icon_dirs, b.icon_dirs_declared), (2, 2), "the scaled folder exists through the alias");
        assert_eq!(b.dropped_links, 3);
        assert!(b.warnings.iter().any(|w| w.contains("dangling.svg") && w.contains("points at nothing")));
        assert!(b.warnings.iter().any(|w| w.contains("loop")));
    }

    #[test]
    fn anything_but_a_declared_well_formed_alias_inside_the_pack_is_refused() {
        let sp = spec(true, vec![]);
        for (what, extra, want) in [
            ("traversal out of the source", vec![E::file("top/links/apps/48/x.svg", b"../../../../../etc/passwd")], "leaves the pack folder"),
            ("out of the pack into its source", vec![E::file("top/links/apps/48/x.svg", b"../../../../LICENSE")], "leaves the pack folder"),
            ("absolute", vec![E::file("top/links/apps/48/x.svg", b"/usr/share/icons/x.svg")], "absolute"),
            ("malformed", vec![E::file("top/links/apps/48/x.svg", b"real.svg\nrm -rf")], "control"),
            ("binary", vec![E::file("top/links/apps/48/x.svg", SVG)], "is not a link target"),
            ("a real symlink among the texts", vec![E::link("top/links/apps/48/x.svg", "real.svg")], "only regular files"),
            ("below a file of the pack", vec![E::file("top/links/apps/48/real.svg/x.svg", b"../real.svg")], "would lie below `src/apps/48/real.svg`"),
        ] {
            let (r, _s) = run(&source(extra), &sp, &Limits::default());
            let e = r.map(|_| ()).unwrap_err();
            assert!(e.contains(want), "{what}: {e}");
        }
        // an alias file that lands on a folder of the pack (from a second declared folder, mapped onto `apps`)
        let mut two = spec(true, vec![]);
        two.text_links.push(TextLinks { from: "more".into(), tree: String::new(), onto: "apps".into(), replace: true });
        let (r, _s) = run(&source(vec![E::file("top/more/48", b"48/real.svg")]), &two, &Limits::default());
        assert!(r.map(|_| ()).unwrap_err().contains("alias `src/apps/48` names a folder of the pack"));
        // replacing a pack file needs the opt-in
        let (r, _s) = run(&source(vec![]), &spec(false, vec![]), &Limits::default());
        assert!(r.map(|_| ()).unwrap_err().contains("replace = true"));
        // folder aliases: to nothing, to a file, enclosing, over something that exists
        for (d, want) in [
            (DirAlias { name: "apps@3x".into(), target: "nothing".into() }, "not a folder of the pack"),
            (DirAlias { name: "apps@3x".into(), target: "index.theme".into() }, "not a folder of the pack"),
            (DirAlias { name: "apps/sub".into(), target: "apps".into() }, "encloses itself"),
            (DirAlias { name: "apps".into(), target: "apps/48".into() }, "encloses itself"),
            (DirAlias { name: "index.theme".into(), target: "apps".into() }, "already has"),
            (DirAlias { name: "../x".into(), target: "apps".into() }, "escapes"),
        ] {
            let (r, _s) = run(&source(vec![]), &spec(true, vec![d.clone()]), &Limits::default());
            let e = r.map(|_| ()).unwrap_err();
            assert!(e.contains(want), "{d:?}: {e}");
        }
    }

    #[test]
    fn resolution_and_reporting_stay_bounded() {
        // a chain longer than the hop bound is a loop, not an endless walk
        let mut long = Vec::new();
        for i in 0..40 {
            long.push(E::file(&format!("top/links/apps/48/c{i}.svg"), format!("c{}.svg", i + 1).as_bytes()));
        }
        long.push(E::file("top/links/apps/48/c40.svg", b"real.svg"));
        // and many links to nothing produce a bounded number of warnings
        for i in 0..200 {
            long.push(E::file(&format!("top/links/apps/48/gone{i}.svg"), b"nothing.svg"));
        }
        let (r, s) = run(&source(long), &spec(true, vec![]), &Limits::default());
        let (_, b) = r.unwrap();
        assert!(std::fs::symlink_metadata(s.path().join("out/apps/48/c0.svg")).is_err(), "41 hops: cut");
        assert_eq!(std::fs::read_link(s.path().join("out/apps/48/c39.svg")).unwrap().display().to_string(), "real.svg", "2 hops: kept");
        assert!(b.dropped_links >= 203);
        assert!(b.warnings.len() <= 30, "{} warnings for {} dropped links", b.warnings.len(), b.dropped_links);
        assert!(b.warnings.iter().any(|w| w.contains("links dropped in all")));
        // the alias files count towards the entry limit of the source
        let many: Vec<E> = (0..50).map(|i| E::file(&format!("top/links/apps/48/m{i}.svg"), b"real.svg")).collect();
        let (r, _s) = run(&source(many), &spec(true, vec![]), &Limits { max_entries: 40, ..Limits::default() });
        assert!(r.map(|_| ()).unwrap_err().contains("more than 40 entries"));
    }

    fn real(dirs: &[&str]) -> Vec<E> {
        let mut v = vec![E::dir("top/"), E::file("top/LICENSE", b"GPL\n"), E::file("top/src/index.theme", INDEX), E::file("top/src/apps/48/real.svg", SVG)];
        for d in dirs {
            v.push(E::file(&format!("top/src/{d}/f.svg"), SVG));
        }
        v
    }

    fn aliases(pairs: &[(&str, &str)]) -> Vec<DirAlias> {
        pairs.iter().map(|(n, t)| DirAlias { name: n.to_string(), target: t.to_string() }).collect()
    }

    fn no_text(dirs: Vec<DirAlias>) -> Spec {
        let mut sp = spec(true, dirs);
        sp.text_links.clear();
        sp
    }

    #[test]
    fn folder_alias_cycles_through_real_folders_are_refused() {
        // the two-folder cycle each of whose links resolves to a real folder
        let (r, s) = run(&real(&["a", "b"]), &no_text(aliases(&[("a/to-b", "b"), ("b/to-a", "a")])), &Limits::default());
        let e = r.map(|_| ()).unwrap_err();
        assert!(e.contains("folder links form a cycle") && e.contains("a") && e.contains("b"), "{e}");
        assert_eq!(std::fs::read_dir(s.path().join("out")).unwrap().count(), 0, "refused before anything was built");
        // longer, through nested real folders: a/x -> c, c/y -> b, b/z -> a
        let (r, _s) = run(&real(&["a/x", "b/z", "c/y"]), &no_text(aliases(&[("a/x/to-c", "c"), ("c/y/to-b", "b"), ("b/z/to-a", "a")])), &Limits::default());
        assert!(r.map(|_| ()).unwrap_err().contains("folder links form a cycle"));
        // a text alias that names a folder takes part too
        let mut sp = spec(true, aliases(&[("b/to-a", "a")]));
        sp.text_links[0].replace = true;
        let src = {
            let mut v = real(&["a", "b"]);
            v.push(E::file("top/links/a/to-b", b"../b"));
            v
        };
        let (r, _s) = run(&src, &sp, &Limits::default());
        assert!(r.map(|_| ()).unwrap_err().contains("folder links form a cycle"));
    }

    #[test]
    fn ordinary_folder_symlinks_of_the_pack_are_covered_too() {
        let mut v = real(&["a", "b"]);
        v.push(E::link("top/src/a/to-b", "../b"));
        v.push(E::link("top/src/b/to-a", "../a"));
        let (r, _s) = run(&v, &no_text(vec![]), &Limits::default());
        assert!(r.map(|_| ()).unwrap_err().contains("folder links form a cycle"), "no declared alias needed");
        // a link to an enclosing folder is still just dropped by the build, as before (it is never published)
        let mut v = real(&["a/b"]);
        v.push(E::link("top/src/a/b/up", "../.."));
        v.push(E::link("top/src/a/self", "."));
        let (r, s) = run(&v, &no_text(vec![]), &Limits::default());
        r.unwrap();
        assert!(std::fs::symlink_metadata(s.path().join("out/a/b/up")).is_err() && std::fs::symlink_metadata(s.path().join("out/a/self")).is_err());
    }

    #[test]
    fn acyclic_folder_aliases_install_including_the_lingmo_scale_set() {
        let ctx = ["actions", "apps", "categories", "devices", "emblems", "mimetypes", "places", "status"];
        let mut pairs: Vec<(String, String)> = Vec::new();
        for c in ctx {
            for x in ["@2x", "@3x"] {
                pairs.push((format!("{c}{x}"), c.to_string()));
            }
        }
        let dirs: Vec<DirAlias> = pairs.iter().map(|(n, t)| DirAlias { name: n.clone(), target: t.clone() }).collect();
        let reals: Vec<String> = ctx.iter().map(|c| format!("{c}/16")).collect();
        let (r, s) = run(&real(&reals.iter().map(String::as_str).collect::<Vec<_>>()), &no_text(dirs), &Limits::default());
        let (rep, _) = r.unwrap();
        assert_eq!(rep.dir_aliases, 16);
        assert_eq!(std::fs::read_link(s.path().join("out/places@3x")).unwrap().display().to_string(), "places");
        // two links to one folder (a diamond) and a link across siblings are not cycles
        let (r, _s) = run(&real(&["a", "b/c"]), &no_text(aliases(&[("a/to-c", "b/c"), ("x", "a"), ("y", "a")])), &Limits::default());
        r.unwrap();
    }

    #[test]
    fn the_graph_check_is_linear_and_iterative_on_long_link_chains() {
        // 3000 folders d0..d2999, each with an ordinary link to the next: a long acyclic chain of link edges
        let n = 3000;
        let mut v = real(&[]);
        for i in 0..n {
            v.push(E::file(&format!("top/src/d{i}/f.svg"), SVG));
            if i + 1 < n {
                v.push(E::link(&format!("top/src/d{i}/next"), &format!("../d{}", i + 1)));
            }
        }
        let s = Scratch::new("graph-long");
        let f = Filter { strip: 1, path: "src".into(), ..Filter::default() };
        let st = from_tar(&targz(&v, None)[..], Format::TarGz, s.path().to_path_buf(), f.clone(), &Limits::default(), &keep).unwrap();
        let t = std::time::Instant::now();
        assert_eq!(check_dir_graph(&st, "src").unwrap(), n - 1);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
        // closing the chain makes one 3000-folder cycle: found without recursion, reported briefly
        v.push(E::link(&format!("top/src/d{}/next", n - 1), "../d0"));
        let s2 = Scratch::new("graph-long2");
        let st = from_tar(&targz(&v, None)[..], Format::TarGz, s2.path().to_path_buf(), f, &Limits::default(), &keep).unwrap();
        let e = check_dir_graph(&st, "src").unwrap_err();
        assert!(e.contains("folder links form a cycle") && e.contains("more folders on the way") && e.len() < 400, "{e}");
    }
}
