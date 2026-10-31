//! `decthemectl assets ...`: install, inspect, select and preview icon / cursor packs (also what the tests drive).

use super::install::{self, Action, Existing, Input, Integrity, Pins, Request, SourceRecord};
use super::policy::{self, Choice, Owner};
use super::{assemble, catalog, cursor, preview, Limits, Provides};
use serde_json::json;
use std::path::{Path, PathBuf};

pub const HELP: &str = "decthemectl assets list [--json]                 catalogue packs and installed decebal packs
decthemectl assets info ID|NAME [--json]         pins, licence, installed state of one pack
decthemectl assets install ID [--archive FILE [--strip N] | --source-dir DIR | --git] [--force] [--dry-run] [--allow-degraded]
                                                 install a catalogue pack: download its pinned https archive (sha256 checked),
                                                 or fetch only its pinned git objects from git_url (ids checked against the commit),
                                                 or a downloaded archive (sha256 checked), or a local clone (git tree id checked)
decthemectl assets import PATH --provides icons[,cursors] --license SPDX --license-file REL [--license-file REL]...
                          (REL paths are relative to the source root, after --strip)
                          [--name NAME] [--path REL] [--strip N] [--link-source REL]... [--index REL]... [--tree OID] [--sha256 HEX] [--force] [--dry-run] [--allow-degraded]
                                                 install an archive (.tar, .tar.gz) or folder that is not in a catalogue
decthemectl assets verify [NAME...] [--json]     re-hash installed packs against their provenance
decthemectl assets remove NAME [--force]         delete a pack decebal installed (--force: also when it is in use)
decthemectl assets use icons|cursors NAME|auto   choose explicitly (kept when themes change)
decthemectl assets use icons|cursors theme       follow the theme's suggestion again
decthemectl assets status [--json]               icon and cursor selection, who chose it, the theme's suggestion
decthemectl assets preview NAME [--png OUT] [--json]   which common icons / cursors resolve in a pack
decthemectl assets catalog check FILE...         validate companion metadata (asset-packs.toml)
Options: --root DIR (default $DECEBAL_ASSET_ROOT or ~/.local/share/icons). Nothing from a pack is ever run.
See docs/desktop-asset-packs.md.";

/// Where a command acts.
pub struct Ctx {
    pub shell_json: PathBuf,
    pub root: PathBuf,
    pub set: catalog::Set,
    pub limits: Limits,
}

impl Ctx {
    pub fn from_env() -> Ctx {
        Ctx { shell_json: crate::icons::config::shell_json(), root: super::default_root(), set: catalog::Set::load(), limits: Limits::default() }
    }

    fn roots(&self) -> crate::icons::resolve::Roots {
        super::roots_with(&self.root)
    }
}

struct Args {
    pos: Vec<String>,
    flags: Vec<String>,
    opts: Vec<(String, String)>,
}

const VALUED: &[&str] = &["--index", "--root", "--archive", "--source-dir", "--provides", "--license", "--license-file", "--name", "--path", "--strip", "--tree", "--sha256", "--png", "--link-source"];

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args { pos: vec![], flags: vec![], opts: vec![] };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        if VALUED.contains(&x.as_str()) {
            let v = it.next().ok_or_else(|| format!("{x} needs a value"))?;
            a.opts.push((x.clone(), v.clone()));
        } else if let Some((k, v)) = x.split_once('=').filter(|(k, _)| VALUED.contains(k)) {
            a.opts.push((k.to_string(), v.to_string()));
        } else if x.starts_with("--") {
            if !["--json", "--force", "--dry-run", "--allow-degraded"].contains(&x.as_str()) {
                return Err(format!("unknown option {x}"));
            }
            a.flags.push(x.clone());
        } else {
            a.pos.push(x.clone());
        }
    }
    Ok(a)
}

impl Args {
    fn flag(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }
    fn opt(&self, k: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(o, _)| o == k).map(|(_, v)| v.as_str())
    }
    fn all(&self, k: &str) -> Vec<String> {
        self.opts.iter().filter(|(o, _)| o == k).map(|(_, v)| v.clone()).collect()
    }
}

/// Runs one subcommand (`args` without the leading `assets`); the text to print or an error.
pub fn run(args: &[String], ctx: &mut Ctx) -> Result<String, String> {
    let a = parse(args)?;
    if let Some(r) = a.opt("--root") {
        ctx.root = PathBuf::from(r);
    }
    let json = a.flag("--json");
    let rest: Vec<&str> = a.pos.iter().skip(1).map(String::as_str).collect();
    match a.pos.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => Ok(HELP.into()),
        Some("list") => Ok(list(ctx, json)),
        Some("info") => info(ctx, rest.first().ok_or("info needs a pack id or name")?, json),
        Some("install") => {
            let id = rest.first().ok_or("install needs a pack id (decthemectl assets list)")?;
            let e = ctx.set.find(id).ok_or_else(|| format!("no catalogue pack `{id}` (decthemectl assets list)"))?;
            let input = match (a.opt("--archive"), a.opt("--source-dir"), a.flag("--git")) {
                (Some(_), Some(_), _) | (Some(_), _, true) | (_, Some(_), true) => return Err("--archive, --source-dir and --git exclude each other".into()),
                (Some(f), None, false) => Some(Input::Archive(f.into())),
                (None, Some(d), false) => Some(Input::Dir(d.into())),
                (None, None, true) => Some(Input::Git(e.source.git_url.clone().ok_or_else(|| format!("`{id}` has no git_url in its catalogue"))?)),
                (None, None, false) => None,
            };
            let mut req = Request::from_catalog(e, input, ctx.root.clone())?;
            if let Some(s) = a.opt("--strip") {
                if matches!(req.input, Input::Dir(_) | Input::Git(_)) {
                    return Err("--strip applies to archives, not to --source-dir or --git".into());
                }
                req.strip = s.parse().ok().filter(|n| *n <= 4).ok_or("--strip takes 0..4")?;
            }
            req.force = a.flag("--force");
            req.dry_run = a.flag("--dry-run");
            req.allow_degraded = a.flag("--allow-degraded");
            report(&install::install(&req, &ctx.limits)?, json)
        }
        Some("import") => import(ctx, &a, rest.first().ok_or("import needs an archive or folder")?, json),
        Some("verify") => verify(ctx, &rest, json),
        Some("remove") => {
            let name = rest.first().ok_or("remove needs a theme name")?;
            // read under the install lock, right before removing
            let in_use = |n: &str| {
                let doc = crate::settings::io::json_load(&ctx.shell_json);
                let used: Vec<&str> = [Provides::Icons, Provides::Cursors].into_iter().filter(|k| policy::current(&doc, *k).as_deref() == Some(n)).map(Provides::id).collect();
                (!used.is_empty()).then(|| used.join(" and "))
            };
            let p = install::remove(&ctx.root, name, a.flag("--force"), &in_use)?;
            Ok(format!("removed {}", p.display()))
        }
        Some("use") => {
            let (Some(k), Some(v)) = (rest.first(), rest.get(1)) else { return Err("use needs icons|cursors and NAME|auto|theme".into()) };
            let k = Provides::parse(k).ok_or("use needs icons or cursors")?;
            let c = match *v {
                "auto" | "none" => Choice::Auto,
                "theme" => Choice::Theme,
                n => {
                    if !policy::is_installed(&ctx.roots(), k, n) {
                        return Err(format!("no installed {} theme `{n}`", k.id().trim_end_matches('s')));
                    }
                    Choice::Name(n.to_string())
                }
            };
            policy::choose_in_file(&ctx.shell_json, k, &c)?;
            let doc = crate::settings::io::json_load(&ctx.shell_json);
            let mut out = format!("{} theme: {} ({})", k.id().trim_end_matches('s'), policy::current(&doc, k).as_deref().unwrap_or("auto"), owner_text(&policy::owner(&doc, k)));
            if k == Provides::Cursors {
                out.push_str("\nnote: applications pick the cursor up through \"Apply to all apps\" (decthemectl apply-system) and new sessions");
            }
            Ok(out)
        }
        Some("status") => Ok(status(ctx, json)),
        Some("preview") => preview_cmd(ctx, rest.first().ok_or("preview needs a theme name")?, a.opt("--png"), json),
        Some("catalog") => match rest.first().copied() {
            Some("check") if rest.len() > 1 => {
                let mut out = Vec::new();
                let mut bad = Vec::new();
                for f in &rest[1..] {
                    let t = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                    match catalog::parse(&t, f) {
                        Ok(c) => out.push(format!("{f}: ok ({} sources, {} packs, {} recommendations)", c.sources.len(), c.packs.len(), c.recommends.len())),
                        Err(e) => bad.extend(e),
                    }
                }
                if bad.is_empty() {
                    Ok(out.join("\n"))
                } else {
                    Err(bad.join("\n"))
                }
            }
            _ => Err("catalog check needs one or more files".into()),
        },
        Some(other) => Err(format!("unknown assets command `{other}`\n{HELP}")),
    }
}

fn owner_text(o: &Owner) -> String {
    match o {
        Owner::User => "your choice".into(),
        Owner::Theme(t) => format!("suggested by the theme {t}"),
        Owner::Default => "not chosen".into(),
    }
}

fn state_of(ctx: &Ctx, e: &catalog::Entry) -> &'static str {
    match install::existing(&ctx.root, &e.pack.install_name) {
        Existing::Absent => "not installed",
        Existing::Foreign => "name taken by a theme decebal did not install",
        Existing::Ours(p) if p.pack != e.pack.id => "name taken by another pack",
        Existing::Ours(p) if p.source.tree.as_deref() != Some(e.pack.tree.as_str()) => "installed, the catalogue pins a newer revision (install --force)",
        Existing::Ours(_) if install::health(&ctx.root.join(&e.pack.install_name)).is_some_and(|(p, l)| p * 2 < l) => "installed, DEGRADED (few icon directories)",
        Existing::Ours(_) => "installed",
    }
}

fn list(ctx: &Ctx, json: bool) -> String {
    let installed = install::installed(&ctx.root);
    if json {
        let cat: Vec<_> = ctx
            .set
            .entries()
            .map(|e| {
                json!({"id": e.pack.id, "name": e.pack.name, "family": e.catalog.family, "provides": e.pack.provides.iter().map(|p| p.id()).collect::<Vec<_>>(),
                       "install_name": e.pack.install_name, "license": e.license(), "commit": e.source.commit, "tree": e.pack.tree,
                       "downloadable": e.source.downloadable(), "state": state_of(ctx, &e)})
            })
            .collect();
        let inst: Vec<_> =
            installed.iter().map(|(d, p)| json!({"path": d, "pack": p.pack, "install_name": p.install_name, "origin": p.origin, "license": p.license, "commit": p.source.commit})).collect();
        return serde_json::to_string_pretty(&json!({"root": ctx.root, "catalog": cat, "installed": inst, "problems": ctx.set.problems})).unwrap_or_default();
    }
    let mut out = String::new();
    if ctx.set.catalogs.is_empty() {
        out.push_str("no catalogue packs (ports add assets/desktop-ports/<family>/asset-packs.toml)\n");
    }
    for e in ctx.set.entries() {
        let kinds: Vec<&str> = e.pack.provides.iter().map(|p| p.id()).collect();
        out.push_str(&format!("{:<28} {:<22} {:<16} {:<18} {}\n", e.pack.id, e.pack.install_name, kinds.join("+"), e.license(), state_of(ctx, &e)));
    }
    if !installed.is_empty() {
        out.push_str(&format!("installed in {}:\n", ctx.root.display()));
        for (_, p) in &installed {
            out.push_str(&format!("  {:<22} {:<28} {:<8} {}\n", p.install_name, p.pack, p.origin, p.license));
        }
    }
    for p in &ctx.set.problems {
        out.push_str(&format!("problem: {p}\n"));
    }
    out.trim_end().to_string()
}

fn info(ctx: &Ctx, id: &str, json: bool) -> Result<String, String> {
    let e = ctx.set.find(id);
    let name = e.map(|e| e.pack.install_name.clone()).unwrap_or_else(|| id.to_string());
    let prov = install::Provenance::read(&ctx.root.join(&name));
    if e.is_none() && prov.is_none() {
        return Err(format!("no catalogue pack or installed decebal pack `{id}`"));
    }
    let recommended_by: Vec<&str> = ctx
        .set
        .catalogs
        .iter()
        .flat_map(|c| c.recommends.iter())
        .filter(|r| e.is_some_and(|e| r.icons.as_deref() == Some(&e.pack.id) || r.cursors.as_deref() == Some(&e.pack.id)))
        .map(|r| r.theme.as_str())
        .collect();
    if json {
        return Ok(serde_json::to_string_pretty(&json!({
            "catalog": e.map(|e| json!({"id": e.pack.id, "name": e.pack.name, "family": e.catalog.family, "origin": e.catalog.origin, "path": e.pack.path, "tree": e.pack.tree,
                "install_name": e.pack.install_name, "license": e.license(), "license_files": e.license_files(), "repo": e.source.repo, "revision": e.source.revision,
                "commit": e.source.commit, "archive_url": e.source.archive_url, "archive_sha256": e.source.archive_sha256, "git_url": e.source.git_url})),
            "installed": prov, "recommended_by": recommended_by,
        }))
        .unwrap_or_default());
    }
    let mut out = String::new();
    if let Some(e) = e {
        out.push_str(&format!(
            "{} ({}), family {}\n  provides:  {}\n  source:    {} @ {} ({})\n  path:      {} (tree {})\n  licence:   {} [{}]\n  installs:  {}\n",
            e.pack.name,
            e.pack.id,
            e.catalog.family,
            e.pack.provides.iter().map(|p| p.id()).collect::<Vec<_>>().join(", "),
            e.source.repo,
            e.source.commit,
            e.source.revision,
            e.pack.path,
            e.pack.tree,
            e.license(),
            e.license_files().join(", "),
            ctx.root.join(&e.pack.install_name).display()
        ));
        let download = match (&e.source.archive_url, &e.source.git_url) {
            (Some(u), _) => u.clone(),
            (None, Some(g)) => format!("{g} (git: only the pinned objects)"),
            (None, None) => "none (use --archive or --source-dir)".into(),
        };
        out.push_str(&format!("  download:  {download}\n"));
        if !recommended_by.is_empty() {
            out.push_str(&format!("  suggested by the themes: {}\n", recommended_by.join(", ")));
        }
    }
    match prov {
        Some(p) => out.push_str(&format!(
            "installed: {} from {} ({}), commit {}, tree {}, verified tree={} sha256={} commit={}, {} files, {} links, {} left out\n  {}{}\n  licence files: {}",
            p.install_name,
            p.source.fetched_from,
            p.origin,
            p.source.commit.as_deref().unwrap_or("?"),
            p.source.tree.as_deref().unwrap_or("?"),
            p.verified.tree,
            p.verified.archive_sha256,
            p.verified.commit,
            p.files,
            p.symlinks,
            p.skipped,
            verification_line(&p.verified),
            match install::health(&ctx.root.join(&p.install_name)) {
                Some((present, listed)) if present * 2 < listed => format!("\n  DEGRADED: {present} of {listed} icon directories exist"),
                _ => String::new(),
            },
            p.license_files.join(", ")
        )),
        None => out.push_str("not installed"),
    }
    Ok(out.trim_end().to_string())
}

fn report(r: &install::Report, json: bool) -> Result<String, String> {
    if json {
        return Ok(serde_json::to_string_pretty(&json!({
            "action": format!("{:?}", r.action).to_lowercase(), "target": r.target, "provenance": r.provenance, "warnings": r.built.warnings,
            "skipped": r.built.skipped_sample, "shadows": r.shadows, "missing_parents": r.missing_parents,
        }))
        .unwrap_or_default());
    }
    let what = match r.action {
        Action::Installed => "installed",
        Action::Replaced => "replaced",
        Action::Unchanged => "already installed, unchanged",
        Action::DryRun => "would install (dry run)",
    };
    let p = &r.provenance;
    let mut out = format!(
        "{what}: {} -> {}\n  {} files, {} links ({} copied from link sources), {} left out; {} icon directories, {} cursors ({} built from PNGs)\n  verified: tree {} sha256 {} commit {}\n  licence: {} [{}]\n  content: {}",
        p.pack,
        r.target.display(),
        p.files,
        p.symlinks,
        r.built.copies,
        r.built.skipped,
        r.built.icon_dirs,
        r.built.cursors,
        r.built.cursors_built,
        yes(p.verified.tree),
        yes(p.verified.archive_sha256),
        yes(p.verified.commit),
        p.license,
        p.license_files.join(", "),
        p.content_hash
    );
    out.push_str(&format!("\n  {}", verification_line(&p.verified)));
    if let Some(g) = &p.source.git {
        out.push_str(&format!(
            "\n  git fetch: {} requests, {} bytes ({}), {} objects; every pin found below commit {}: {}",
            g.requests.len(),
            g.requests.iter().map(|x| x.bytes).sum::<u64>(),
            g.requests.iter().map(|x| format!("{} {}", x.what, x.bytes)).collect::<Vec<_>>().join(", "),
            g.objects,
            p.source.commit.as_deref().unwrap_or("?"),
            yes(g.commit_chain)
        ));
    }
    if r.built.expanded_dirs + r.built.copies > 0 {
        out.push_str(&format!(
            "\n  from link_sources: {} folders expanded, {} files ({} KiB) copied, {} links to expanded copies",
            r.built.expanded_dirs,
            r.built.expanded_files + r.built.copies,
            r.built.expanded_bytes >> 10,
            r.built.expanded_links
        ));
    }
    if let Some(o) = &r.built.dci {
        let total = o.icons.len() + o.links.len() + o.refused.len();
        out.push_str(&format!(
            "\n  DCI conversion: {} of {} icons ({} converted, {} links to them), {} REFUSED (CONVERSION.json lists every reason)",
            o.icons.len() + o.links.len(),
            total,
            o.icons.len(),
            o.links.len(),
            o.refused.len()
        ));
        for (n, why) in o.refused.iter().take(8) {
            out.push_str(&format!("\n    refused {n}: {why}"));
        }
        if o.refused.len() > 8 {
            out.push_str(&format!("\n    ... {} more refused", o.refused.len() - 8));
        }
    }
    if p.aliases != (0, 0, 0) {
        out.push_str(&format!("\n  declared aliases: {} text links ({} replacing pack files), {} folder aliases", p.aliases.0, p.aliases.1, p.aliases.2));
    }
    if let Some((present, listed)) = p.icon_dirs {
        out.push_str(&format!("\n  icon directories: {present} of {listed} listed in index.theme{}", if p.degraded { " (DEGRADED)" } else { "" }));
    }
    if !r.built.skipped_sample.is_empty() {
        out.push_str(&format!("\n  left out, e.g.: {}", r.built.skipped_sample.join(", ")));
    }
    for w in &r.built.warnings {
        out.push_str(&format!("\nwarning: {w}"));
    }
    for s in &r.shadows {
        out.push_str(&format!("\nnote: shadows {}", s.display()));
    }
    for m in &r.missing_parents {
        out.push_str(&format!("\nnote: inherits `{m}`, which is not installed (its icons fall back to hicolor)"));
    }
    Ok(out)
}

/// One honest line: the pack folder's own check, and each thing taken from outside it.
fn verification_line(v: &install::Verified) -> String {
    let mut parts = Vec::new();
    for o in &v.outside {
        parts.push(format!("{} {}", o.what, if o.verified { format!("verified ({})", o.by) } else { "NOT pinned".into() }));
    }
    let outside = if parts.is_empty() { "nothing taken from outside the pack folder".to_string() } else { format!("outside the pack folder: {}", parts.join(", ")) };
    format!("{outside}; complete: {}", if v.complete { "yes" } else { "NO" })
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn import(ctx: &Ctx, a: &Args, path: &str, json: bool) -> Result<String, String> {
    let src = PathBuf::from(path);
    let provides = Provides::parse_list(a.opt("--provides").ok_or("import needs --provides icons, cursors or icons,cursors")?)?;
    let license = a.opt("--license").map(str::trim).filter(|s| !s.is_empty()).ok_or("import needs --license SPDX (the licence the assets are under)")?.to_string();
    let license_files = a.all("--license-file");
    if license_files.is_empty() {
        return Err("import needs --license-file REL (the licence text in the source, e.g. COPYING): packs are only installed with their licence".into());
    }
    let theme_path = a.opt("--path").unwrap_or(".").trim_end_matches('/').to_string();
    for p in license_files.iter().chain(a.all("--link-source").iter()).chain(a.all("--index").iter()).chain(std::iter::once(&theme_path).filter(|p| *p != ".")) {
        super::safe_rel(p)?;
    }
    let base = || {
        let n = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        [".tar.gz", ".tgz", ".tar"].iter().find_map(|e| n.strip_suffix(e).map(String::from)).unwrap_or(n)
    };
    let name = match a.opt("--name") {
        Some(n) => n.to_string(),
        None if theme_path != "." => theme_path.rsplit('/').next().unwrap_or_default().to_string(),
        None => base(),
    };
    if !super::safe_theme_name(&name) {
        return Err(format!("`{name}` is not a usable theme folder name (choose one with --name)"));
    }
    let is_dir = std::fs::symlink_metadata(&src).map_err(|e| format!("{path}: {e}"))?.is_dir();
    let strip: usize = match a.opt("--strip") {
        Some(s) => s.parse().ok().filter(|n| *n <= 4).ok_or("--strip takes 0..4")?,
        None => 0,
    };
    let hexpin = |k: &str, n: usize| -> Result<Option<String>, String> {
        match a.opt(k) {
            None => Ok(None),
            Some(v) if v.len() == n && v.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(Some(v.to_ascii_lowercase())),
            Some(_) => Err(format!("{k} takes {n} hex digits")),
        }
    };
    let pack_id: String = format!("local-{}", name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>());
    let req = Request {
        pack: pack_id,
        name: name.clone(),
        family: String::new(),
        origin: "local".into(),
        spec: assemble::Spec {
            provides,
            path: theme_path.clone(),
            install_name: name.clone(),
            display_name: name.clone(),
            license_files,
            license_oids: Default::default(),
            index_files: a.all("--index").into_iter().map(|p| (p, None)).collect(),
            xcursors: vec![],
            text_links: vec![],
            dir_aliases: vec![],
            dci: None,
        },
        strip: if is_dir { 0 } else { strip },
        link_sources: a.all("--link-source"),
        input: if is_dir { Input::Dir(src.clone()) } else { Input::Archive(src.clone()) },
        pins: Pins { tree: hexpin("--tree", 40)?, archive_sha256: if is_dir { None } else { hexpin("--sha256", 64)? }, commit: None, link_trees: vec![] },
        source: SourceRecord { id: "local".into(), repo: std::fs::canonicalize(&src).unwrap_or(src).display().to_string(), revision: String::new(), path: theme_path, ..SourceRecord::default() },
        license,
        root: ctx.root.clone(),
        force: a.flag("--force"),
        dry_run: a.flag("--dry-run"),
        allow_degraded: a.flag("--allow-degraded"),
    };
    report(&install::install(&req, &ctx.limits)?, json)
}

fn verify(ctx: &Ctx, names: &[&str], json: bool) -> Result<String, String> {
    let dirs: Vec<PathBuf> = if names.is_empty() { install::installed(&ctx.root).into_iter().map(|(d, _)| d).collect() } else { names.iter().map(|n| ctx.root.join(n)).collect() };
    let mut rows = Vec::new();
    let mut bad = false;
    for d in &dirs {
        let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match install::verify(d) {
            Ok((_, Integrity::Ok)) => match install::health(d) {
                Some((present, listed)) if present * 2 < listed => {
                    bad = true;
                    rows.push((name, format!("content ok, DEGRADED: {present} of {listed} icon directories exist")));
                }
                _ => rows.push((name, "ok".to_string())),
            },
            Ok((_, Integrity::Modified { recorded, now })) => {
                bad = true;
                rows.push((name, format!("MODIFIED (recorded {recorded}, now {now})")));
            }
            Err(e) => {
                bad = true;
                rows.push((name, format!("error: {e}")));
            }
        }
    }
    let text = if json {
        serde_json::to_string_pretty(&rows.iter().map(|(n, s)| json!({"name": n, "state": s})).collect::<Vec<_>>()).unwrap_or_default()
    } else if rows.is_empty() {
        format!("no decebal packs in {}", ctx.root.display())
    } else {
        rows.iter().map(|(n, s)| format!("{n}: {s}")).collect::<Vec<_>>().join("\n")
    };
    if bad {
        Err(text)
    } else {
        Ok(text)
    }
}

fn status(ctx: &Ctx, json: bool) -> String {
    let doc = crate::settings::io::json_load(&ctx.shell_json);
    let rec = policy::Recommendation::from_doc(&doc);
    let roots = ctx.roots();
    let slot = |k: Provides| {
        let cur = policy::current(&doc, k);
        let ok = cur.as_deref().is_none_or(|c| policy::is_installed(&roots, k, c));
        (cur, policy::owner(&doc, k), ok)
    };
    let (ic, co) = (slot(Provides::Icons), slot(Provides::Cursors));
    if json {
        let j = |s: &(Option<String>, Owner, bool)| json!({"theme": s.0, "owner": owner_text(&s.1), "installed": s.2});
        return serde_json::to_string_pretty(&json!({"icons": j(&ic), "cursors": j(&co), "recommended": rec.as_ref().map(|r| json!({"theme": r.theme, "icons": r.icons, "cursors": r.cursors, "missing": r.missing})), "root": ctx.root}))
            .unwrap_or_default();
    }
    let line = |label: &str, s: &(Option<String>, Owner, bool)| format!("{label}{} ({}){}", s.0.as_deref().unwrap_or("auto"), owner_text(&s.1), if s.2 { "" } else { " - NOT INSTALLED" });
    let mut out = format!("{}\n{}", line("icons:   ", &ic), line("cursors: ", &co));
    if let Some(r) = rec {
        out.push_str(&format!("\ntheme {} suggests: icons {}, cursors {}", r.theme, r.icons.as_deref().unwrap_or("-"), r.cursors.as_deref().unwrap_or("-")));
        for m in r.missing {
            out.push_str(&format!("\n  not installed: {m} (decthemectl assets install {m})"));
        }
    }
    out
}

fn preview_cmd(ctx: &Ctx, name: &str, png: Option<&str>, json: bool) -> Result<String, String> {
    if !super::safe_theme_name(name) {
        return Err(format!("`{name}` is not a theme name"));
    }
    let roots = ctx.roots();
    let has_icons = policy::is_installed(&roots, Provides::Icons, name);
    let has_cursors = cursor::usable(&roots, name);
    if !has_icons && !has_cursors {
        return Err(format!("no installed icon or cursor theme `{name}`"));
    }
    let p = preview::resolve(&roots, name, has_icons, has_cursors);
    let mut drawn = None;
    if let Some(out) = png {
        let (pm, n) = preview::sheet(&p).ok_or("could not draw the preview")?;
        pm.save_png(Path::new(out)).map_err(|e| format!("{out}: {e}"))?;
        drawn = Some(n);
    }
    if json {
        let icons: Vec<_> = p.icons.iter().map(|h| json!({"name": h.name, "path": h.path, "own": h.own})).collect();
        let cursors: Vec<_> = p.cursors.iter().map(|h| json!({"caption": h.caption, "name": h.name, "path": h.path, "theme": h.theme})).collect();
        return Ok(serde_json::to_string_pretty(&json!({"theme": name, "chain": p.chain, "icons": icons, "cursors": cursors, "drawn": drawn})).unwrap_or_default());
    }
    let mut out = String::new();
    if has_icons {
        let own = p.icons.iter().filter(|h| h.own).count();
        let any = p.icons.iter().filter(|h| h.path.is_some()).count();
        out.push_str(&format!("icons: {own} of {} samples from {name}, {any} resolved in all (chain {})\n", p.icons.len(), p.chain.join(" > ")));
        for h in &p.icons {
            out.push_str(&format!("  {:<12} {}\n", h.caption, h.path.as_ref().map_or("-".to_string(), |p| p.display().to_string())));
        }
    }
    if has_cursors {
        let n = p.cursors.iter().filter(|h| h.path.is_some()).count();
        out.push_str(&format!("cursors: {n} of {} resolved\n", p.cursors.len()));
        for h in &p.cursors {
            out.push_str(&format!("  {:<12} {}\n", h.caption, h.path.as_ref().map_or("-".to_string(), |p| format!("{} ({})", p.display(), h.theme.as_deref().unwrap_or("")))));
        }
    }
    if let (Some(n), Some(f)) = (drawn, png) {
        out.push_str(&format!("drew {n} samples to {f}"));
    }
    Ok(out.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::super::install::tests::{fixture, COMMIT};
    use super::super::testkit::*;
    use super::*;

    fn ctx(s: &Scratch, catalog_text: Option<String>) -> Ctx {
        let mut set = catalog::Set::default();
        if let Some(t) = catalog_text {
            set.push(catalog::parse(&t, "test").unwrap());
        }
        Ctx { shell_json: s.path().join("decebal.json"), root: s.path().join("icons"), set, limits: Limits::default() }
    }

    fn go(c: &mut Ctx, a: &[&str]) -> Result<String, String> {
        run(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>(), c)
    }

    fn catalog_for(entries: &[E], archive: &Path) -> String {
        format!(
            "format = 1\nfamily = \"fixture\"\n[[source]]\nid = \"fx\"\nrepo = \"https://example.org/fx\"\nrevision = \"main\"\ncommit = \"{COMMIT}\"\nlicense = \"GPL-3.0-or-later\"\nlicense_files = [\"COPYING\"]\nstrip_components = 1\n\
             [[pack]]\nid = \"fixture-bloom\"\nname = \"Bloom\"\nprovides = [\"icons\", \"cursors\"]\nsource = \"fx\"\npath = \"bloom\"\ntree = \"{}\"\n\
             [[recommend]]\ntheme = \"fixture-theme\"\nicons = \"fixture-bloom\"\ncursors = \"fixture-bloom\"\n# archive {}\n",
            tree_of(entries, "top/bloom"),
            archive.display()
        )
    }

    #[test]
    fn install_select_preview_and_remove_through_the_cli() {
        let s = Scratch::new("cli");
        let (a, entries) = fixture(&s, vec![], "a.tgz");
        let mut c = ctx(&s, Some(catalog_for(&entries, &a)));
        assert!(go(&mut c, &["list"]).unwrap().contains("not installed"));
        assert!(go(&mut c, &["install", "fixture-bloom"]).unwrap_err().contains("--archive"), "no archive_url: told how");
        let out = go(&mut c, &["install", "fixture-bloom", "--archive", a.to_str().unwrap()]).unwrap();
        assert!(out.starts_with("installed: fixture-bloom") && out.contains("verified: tree yes sha256 no commit yes"), "{out}");
        assert!(go(&mut c, &["list"]).unwrap().lines().any(|l| l.starts_with("fixture-bloom") && l.ends_with(" installed")));
        let mut moved = ctx(&s, Some(catalog_for(&entries, &a).replace(&tree_of(&entries, "top/bloom"), &"a".repeat(40))));
        assert!(go(&mut moved, &["list"]).unwrap().contains("newer revision"), "a new pin is shown as an update");
        let info: serde_json::Value = serde_json::from_str(&go(&mut c, &["info", "bloom", "--json"]).unwrap()).unwrap();
        assert_eq!(info["installed"]["license"], "GPL-3.0-or-later");
        assert_eq!(info["recommended_by"][0], "fixture-theme");
        let pv = go(&mut c, &["preview", "bloom", "--png", s.path().join("p.png").to_str().unwrap()]).unwrap();
        assert!(pv.contains("icons: 2 of 16 samples from bloom") && pv.contains("cursors: 1 of 10"), "{pv}");
        assert!(s.path().join("p.png").is_file());
        go(&mut c, &["use", "icons", "bloom"]).unwrap();
        go(&mut c, &["use", "cursors", "bloom"]).unwrap();
        let st = go(&mut c, &["status"]).unwrap();
        assert!(st.contains("icons:   bloom (your choice)") && st.contains("cursors: bloom (your choice)"), "{st}");
        assert!(go(&mut c, &["use", "icons", "nope"]).is_err());
        assert!(go(&mut c, &["remove", "bloom"]).unwrap_err().contains("in use"));
        go(&mut c, &["use", "icons", "auto"]).unwrap();
        go(&mut c, &["use", "cursors", "auto"]).unwrap();
        assert_eq!(go(&mut c, &["verify"]).unwrap(), "bloom: ok");
        assert!(go(&mut c, &["remove", "bloom"]).unwrap().starts_with("removed"));
        assert!(!c.root.join("bloom").exists());
    }

    #[test]
    fn import_requires_a_licence_and_a_usable_name() {
        let s = Scratch::new("import");
        let (a, _) = fixture(&s, vec![], "Bloom-1.0.tar.gz");
        let mut c = ctx(&s, None);
        let p = a.to_str().unwrap();
        assert!(go(&mut c, &["import", p, "--provides", "icons", "--license", "GPL-3.0"]).unwrap_err().contains("--license-file"));
        assert!(go(&mut c, &["import", p, "--provides", "icons", "--license-file", "COPYING"]).unwrap_err().contains("--license"));
        assert!(go(&mut c, &["import", p, "--provides", "fonts", "--license", "x", "--license-file", "COPYING"]).is_err());
        let out = go(&mut c, &["import", p, "--provides", "icons,cursors", "--license", "GPL-3.0-or-later", "--license-file", "COPYING", "--path", "bloom", "--strip", "1"]).unwrap();
        assert!(out.contains("installed: local-bloom"), "{out}");
        let prov = install::Provenance::read(&c.root.join("bloom")).unwrap();
        assert_eq!((prov.origin.as_str(), prov.verified.tree), ("local", false));
        assert!(prov.source.archive_sha256.is_some() && prov.source.tree.is_some(), "recorded even without pins");
        let e = go(&mut c, &["import", p, "--provides", "icons", "--license", "x", "--license-file", "COPYING", "--path", "bloom", "--strip", "1", "--tree", &"a".repeat(40), "--name", "other"])
            .unwrap_err();
        assert!(e.contains("tree mismatch"), "{e}");
        assert!(go(&mut c, &["import", p, "--provides", "icons", "--license", "x", "--license-file", "../COPYING"]).is_err());
        assert!(go(&mut c, &["catalog", "check", a.to_str().unwrap()]).is_err());
        assert!(go(&mut c, &["bogus"]).unwrap_err().contains("unknown"));
        assert!(go(&mut c, &["list", "--bogus"]).unwrap_err().contains("unknown option"));
    }
}
