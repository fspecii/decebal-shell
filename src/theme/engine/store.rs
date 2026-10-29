//! The theme folders and what is in them: the user's (`~/.config/decebal/themes/<id>/`), the system's
//! (`/usr/share/decebal/themes/<id>/`) and the built-ins compiled into the programs.
//!
//! A user folder entry is either a package directory (`<id>/theme.toml` plus assets) or, as before, a single `<id>.toml`.

use super::archive::Limits;
use super::package::{self, Installed, Report};
use super::writer::{def_to_manifest, Header};
use crate::theme::builtin;
use crate::theme::live::Source;
use crate::theme::model::{Def, Selection};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// `$DECEBAL_SYSTEM_THEMES`, else `/usr/share/decebal/themes`.
pub fn system_dir() -> PathBuf {
    std::env::var_os("DECEBAL_SYSTEM_THEMES").filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/usr/share/decebal/themes"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    User,
    System,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Builtin => "built-in",
            Origin::User => "user",
            Origin::System => "system",
        }
    }
}

/// One theme as the gallery and `theme list` show it.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub license: String,
    pub description: String,
    pub tags: Vec<String>,
    pub origin: Origin,
    /// The package folder (user and system themes).
    pub dir: Option<PathBuf>,
    pub has_dark: bool,
    pub has_light: bool,
    pub error: Option<String>,
    /// Absolute paths of the screenshots that exist in the package.
    pub screenshots: Vec<PathBuf>,
    /// background, bar, surface, titlebar, text, accent, dock (dark variant if there is one)
    pub swatches: [(u8, u8, u8); 7],
    pub widgets: usize,
    /// Whether the package has a `[integrity] content_hash` that matches its files.
    pub hash: HashState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashState {
    None,
    Ok,
    Mismatch,
}

fn meta<'a>(d: &'a Def, k: &str) -> Option<&'a Value> {
    d.extra.get("meta").and_then(|m| m.get(k))
}

fn meta_str(d: &Def, k: &str) -> String {
    meta(d, k).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Where the files of `id` live for the user: a package folder, or none (flat file or built-in).
pub fn user_package(src: &Source, id: &str) -> Option<PathBuf> {
    crate::theme::engine::schema::valid_ident(id).then_some(())?;
    let d = src.themes_dir.join(id);
    (std::fs::symlink_metadata(&d).map(|m| m.is_dir()).unwrap_or(false) && d.join("theme.toml").is_file()).then_some(d)
}

fn hash_state(dir: &Path, def: &Def) -> HashState {
    let Some(want) = def.extra.get("integrity").and_then(|i| i.get("content_hash")).and_then(Value::as_str) else { return HashState::None };
    match package::read_dir(dir, &Limits::default()) {
        Ok(e) if package::content_hash(&e) == want => HashState::Ok,
        _ => HashState::Mismatch,
    }
}

pub fn info_for(src: &Source, id: &str) -> Info {
    let sel = Selection::default();
    let dir = user_package(src, id).or_else(|| {
        let s = system_dir().join(id);
        s.join("theme.toml").is_file().then_some(s)
    });
    let is_user_dir = dir.as_ref().is_some_and(|d| d.starts_with(&src.themes_dir));
    let flat = src.themes_dir.join(format!("{id}.toml")).is_file();
    let origin = if dir.is_some() && !is_user_dir {
        Origin::System
    } else if dir.is_some() || flat {
        Origin::User
    } else {
        Origin::Builtin
    };
    let blank = |name: &str, error: Option<String>| Info {
        id: id.into(),
        name: name.into(),
        version: String::new(),
        author: String::new(),
        license: String::new(),
        description: String::new(),
        tags: Vec::new(),
        origin,
        dir: dir.clone(),
        has_dark: false,
        has_light: false,
        error,
        screenshots: Vec::new(),
        swatches: [(60, 20, 20); 7],
        widgets: 0,
        hash: HashState::None,
    };
    let def = match src.load_def(id) {
        Ok(d) => d,
        Err(e) => return blank(id, Some(e)),
    };
    let dark = def.pick_dark(crate::theme::model::Mode::Dark, true);
    let t = def.resolve(id, dark, &sel);
    let shots = meta(&def, "screenshots").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>()).unwrap_or_default();
    // screenshots are the theme's own: take them from its folder only (an inherited list was dropped by the merge)
    let screenshots = dir.as_ref().map(|d| shots.iter().map(|s| d.join(s)).filter(|p| std::fs::symlink_metadata(p).map(|m| m.is_file()).unwrap_or(false)).collect()).unwrap_or_default();
    Info {
        name: def.name.clone(),
        version: meta_str(&def, "version"),
        author: meta_str(&def, "author"),
        license: meta_str(&def, "license"),
        description: def.description.clone(),
        tags: meta(&def, "tags").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default(),
        has_dark: def.has_dark(),
        has_light: def.has_light(),
        screenshots,
        swatches: [t.background.t(), t.bar.t(), t.surface.t(), t.titlebar.t(), t.text.t(), t.accent.t(), t.dock.t()],
        widgets: def.extra.get("widgets").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
        hash: dir.as_ref().map(|d| hash_state(d, &def)).unwrap_or(HashState::None),
        ..blank(id, None)
    }
}

/// Every theme, built-ins first; broken user themes are listed with their error.
pub fn list(src: &Source) -> Vec<Info> {
    src.list()
        .into_iter()
        .map(|e| {
            let mut i = info_for(src, &e.id);
            if let Some(err) = e.error {
                i.error = Some(err);
            }
            i
        })
        .collect()
}

// Metadata-only inventory for the integrity snapshot: include directory identity,
// ctime and all entries (including docs/screenshots covered by package integrity).
fn gallery_stamp(roots: &[PathBuf]) -> Result<String, String> {
    fn walk(path: &Path, depth: usize, count: &mut usize, out: &mut String) -> Result<(), String> {
        if depth > 8 || *count > 8192 {
            return Err("gallery inventory limit".into());
        }
        *count += 1;
        out.push_str(&path.to_string_lossy());
        let m = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                out.push_str("missing");
                return Ok(());
            }
            Err(e) => return Err(e.to_string()),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            out.push_str(&format!("{:?}", (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec())));
        }
        #[cfg(not(unix))]
        out.push_str(&format!("{:?}", (m.len(), m.modified(), m.created())));
        if m.is_dir() {
            let mut paths = std::fs::read_dir(path).map_err(|e| e.to_string())?.map(|e| e.map(|e| e.path())).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            paths.sort();
            for child in paths {
                walk(&child, depth + 1, count, out)?;
            }
        }
        Ok(())
    }
    let mut out = String::new();
    for root in roots {
        walk(root, 0, &mut 0, &mut out)?;
    }
    Ok(out)
}

/// Nonblocking gallery snapshot. Integrity verification and metadata I/O stay off the UI thread.
/// A bounded single worker refreshes at most once per second while the gallery is requested.
pub fn gallery_list(src: &Source) -> Vec<Info> {
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    type Snapshot = (PathBuf, Instant, Vec<Info>);
    static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);
    static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static QUEUE: OnceLock<mpsc::SyncSender<Source>> = OnceLock::new();
    let tx = QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Source>(1);
        std::thread::spawn(move || {
            let mut previous: Option<(PathBuf, String, Vec<Info>)> = None;
            for src in rx {
                let stamp = gallery_stamp(&[src.themes_dir.clone(), system_dir()]);
                let infos = match (&previous, &stamp) {
                    (Some((path, old, infos)), Ok(stamp)) if path == &src.themes_dir && old == stamp => infos.clone(),
                    _ => list(&src),
                };
                if let Ok(stamp) = stamp {
                    previous = Some((src.themes_dir.clone(), stamp, infos.clone()));
                }
                let mut g = SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner());
                let changed = g.as_ref().is_none_or(|(path, _, old)| path != &src.themes_dir || old != &infos);
                *g = Some((src.themes_dir, Instant::now(), infos));
                PENDING.store(false, std::sync::atomic::Ordering::Release);
                if changed {
                    super::preview::notify_changed();
                }
            }
        });
        tx
    });
    let g = SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner());
    let current = g.as_ref().filter(|(path, _, _)| path == &src.themes_dir);
    if current.is_none_or(|(_, at, _)| at.elapsed() >= Duration::from_secs(1)) && !PENDING.swap(true, std::sync::atomic::Ordering::AcqRel) && tx.try_send(src.clone()).is_err() {
        PENDING.store(false, std::sync::atomic::Ordering::Release);
    }
    current.map(|(_, _, infos)| infos.clone()).unwrap_or_default()
}

/// Installs a package from a directory or `.dtheme`. Returns where it went and what the check said (warnings, hash).
pub fn install(src: &Source, from: &Path, force: bool) -> Result<(Installed, Report), String> {
    let lim = Limits::default();
    let entries = package::read_any(from, &lim)?;
    package::install_entries(&entries, &src.themes_dir, force, &lim)
}

/// Removes an installed user theme. Built-ins and system themes cannot be removed; neither can the selected one.
pub fn uninstall(src: &Source, id: &str) -> Result<PathBuf, String> {
    if !crate::theme::engine::schema::valid_ident(id) {
        return Err(format!("`{id}` is not a theme id"));
    }
    let sel = src.selection();
    if src.find(&sel.name).is_some_and(|f| f.0 == id) {
        return Err(format!("`{id}` is the theme in use; apply another one first"));
    }
    if let Some(dir) = user_package(src, id) {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        return Ok(dir);
    }
    let flat = src.themes_dir.join(format!("{id}.toml"));
    if flat.is_file() {
        std::fs::remove_file(&flat).map_err(|e| format!("{}: {e}", flat.display()))?;
        return Ok(flat);
    }
    if builtin::source(id).is_some() {
        return Err(format!("`{id}` is built into decebal and cannot be uninstalled"));
    }
    if system_dir().join(id).is_dir() {
        return Err(format!("`{id}` is installed system-wide ({}); remove it with your package manager", system_dir().join(id).display()));
    }
    Err(format!("no installed theme `{id}`"))
}

/// A fresh package folder `<themes>/<id>/` with a commented-out-free minimal `theme.toml` to edit.
pub fn scaffold(src: &Source, id: &str, name: Option<&str>, base: Option<&str>) -> Result<PathBuf, String> {
    if !crate::theme::engine::schema::valid_ident(id) {
        return Err(format!("`{id}` is not a valid id (lower-case letters, digits, `.`, `_`, `-`)"));
    }
    let dest = src.themes_dir.join(id);
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Err(format!("{} already exists", dest.display()));
    }
    let name = name.map(String::from).unwrap_or_else(|| id.split(['-', '_', '.']).map(|w| w[..1].to_uppercase() + &w[1..]).collect::<Vec<_>>().join(" "));
    let def = match base {
        Some(b) => src.load_def(b).map_err(|e| format!("base `{b}`: {e}"))?,
        None => builtin::def(builtin::DEFAULT_ID).ok_or("no default theme")?,
    };
    let mut d = def.clone();
    d.extra.shift_remove("widgets");
    d.name = name.clone();
    let text = def_to_manifest(
        &d,
        &Header {
            id: id.into(),
            name,
            version: Some("0.1.0".into()),
            author: Some(std::env::var("USER").unwrap_or_default()).filter(|a| !a.is_empty()),
            license: Some("MIT".into()),
            description: Some("My theme.".into()),
            tags: vec![],
            extends: None,
            screenshots: vec![],
        },
    );
    std::fs::create_dir_all(dest.join("widgets")).map_err(|e| format!("{}: {e}", dest.display()))?;
    std::fs::write(dest.join("theme.toml"), text).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// A copy of theme `id` under a new id, editable in the user folder. Package folders are copied whole (assets included).
pub fn duplicate(src: &Source, id: &str, new_id: &str, new_name: Option<&str>) -> Result<PathBuf, String> {
    if !crate::theme::engine::schema::valid_ident(new_id) {
        return Err(format!("`{new_id}` is not a valid id"));
    }
    let dest = src.themes_dir.join(new_id);
    if std::fs::symlink_metadata(&dest).is_ok() || src.themes_dir.join(format!("{new_id}.toml")).exists() {
        return Err(format!("`{new_id}` already exists"));
    }
    let info = info_for(src, id);
    if let Some(e) = &info.error {
        return Err(format!("`{id}` is broken: {e}"));
    }
    let name = new_name.map(String::from).unwrap_or_else(|| format!("{} copy", info.name));
    // package folder: copy every validated file, then rewrite the manifest header
    let tmp = src.themes_dir.join(format!(".dup-{new_id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let mut entries = match info.dir.as_ref().filter(|_| info.origin != Origin::Builtin) {
        Some(d) => package::read_dir(d, &Limits::default())?,
        None => Vec::new(),
    };
    let def = src.load_def(id)?;
    // the copy is standalone: flatten the extends chain into it, but keep the assets of this theme's own folder
    let own =
        if let Some(d) = info.dir.as_ref().filter(|_| info.origin != Origin::Builtin) { Def::parse(&std::fs::read_to_string(d.join("theme.toml")).map_err(|e| e.to_string())?)? } else { def.clone() };
    let shots = meta(&own, "screenshots").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
    let mut flat = def.clone();
    flat.inherit = None;
    flat.name = name.clone();
    let text = def_to_manifest(
        &flat,
        &Header {
            id: new_id.into(),
            name,
            version: Some("1.0.0".into()),
            author: Some(info.author.clone()).filter(|a| !a.is_empty()),
            license: Some(info.license.clone()).filter(|a| !a.is_empty()),
            description: Some(info.description.clone()),
            tags: info.tags.clone(),
            extends: None,
            screenshots: shots,
        },
    );
    entries.retain(|e| e.path != "theme.toml");
    entries.push(super::archive::Entry { path: "theme.toml".into(), data: text.into_bytes() });
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let report = package::check(&entries, &Limits::default());
    if !report.ok() {
        return Err(report.errors().join("\n"));
    }
    package::unpack_to(&entries, &tmp)?;
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        format!("{}: {e}", dest.display())
    })?;
    Ok(dest)
}

/// Writes theme `id` as a `.dtheme` file at `dest`.
pub fn export(src: &Source, id: &str, dest: &Path) -> Result<Report, String> {
    let lim = Limits::default();
    let info = info_for(src, id);
    let entries = match info.dir.as_ref().filter(|_| info.origin != Origin::Builtin) {
        Some(d) => {
            let (bytes, rep) = package::pack_dir(d, &lim)?;
            std::fs::write(dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
            return Ok(rep);
        }
        None => {
            // a built-in or flat theme: write a fresh manifest from its resolved definition
            let def = src.load_def(id)?;
            let text = def_to_manifest(
                &def,
                &Header {
                    id: id.into(),
                    name: def.name.clone(),
                    version: Some(if info.version.is_empty() { "1.0.0".into() } else { info.version.clone() }),
                    author: Some(info.author.clone()).filter(|a| !a.is_empty()),
                    license: Some(info.license.clone()).filter(|a| !a.is_empty()),
                    description: Some(info.description.clone()),
                    tags: info.tags.clone(),
                    extends: None,
                    screenshots: vec![],
                },
            );
            vec![super::archive::Entry { path: "theme.toml".into(), data: text.into_bytes() }]
        }
    };
    let hash = package::content_hash(&entries);
    let mut entries = entries;
    entries[0].data = package::with_hash(&String::from_utf8_lossy(&entries[0].data), &hash)?.into_bytes();
    let report = package::check(&entries, &lim);
    if !report.ok() {
        return Err(report.errors().join("\n"));
    }
    std::fs::write(dest, super::archive::write_targz(&entries)?).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gallery_metadata_stamp_tracks_same_size_asset_and_inventory_changes() {
        let root = std::env::temp_dir().join(format!("gallery-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let roots = [root.clone()];
        std::fs::write(root.join("a"), "abc").unwrap();
        let a = gallery_stamp(&roots).unwrap();
        assert_eq!(a, gallery_stamp(&roots).unwrap());
        std::fs::write(root.join("a"), "def").unwrap();
        let b = gallery_stamp(&roots).unwrap();
        assert_ne!(a, b);
        std::fs::write(root.join("b"), "def").unwrap();
        assert_ne!(b, gallery_stamp(&roots).unwrap());
        std::fs::remove_file(root.join("a")).unwrap();
        assert_ne!(b, gallery_stamp(&roots).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    use crate::theme::engine::package::tests::{good, png};

    fn sandbox(name: &str) -> (PathBuf, Source) {
        let d = std::env::temp_dir().join(format!("decebal-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let src = Source { shell_json: d.join("decebal.json"), themes_dir: d.join("themes") };
        (d, src)
    }

    #[test]
    fn installed_packages_are_listed_resolved_and_removable() {
        let (d, src) = sandbox("list");
        std::env::set_var("DECEBAL_SYSTEM_THEMES", d.join("nosystem"));
        let (inst, _) = install_dir(&src, &good());
        assert_eq!(inst.id, "demo");
        let all = list(&src);
        assert_eq!(all.len(), crate::theme::builtin::BUILTIN.len() + 1);
        let demo = all.iter().find(|i| i.id == "demo").unwrap();
        assert_eq!((demo.origin, demo.version.as_str(), demo.author.as_str(), demo.has_dark, demo.has_light), (Origin::User, "1.0.0", "t", true, false));
        assert_eq!(demo.screenshots.len(), 1);
        assert_eq!(demo.swatches[0], (0x10, 0x10, 0x10));
        assert!(all.iter().find(|i| i.id == "nord").unwrap().origin == Origin::Builtin);
        // the shell resolves it like any theme
        let (t, problem) = src.build(&Selection { name: "demo".into(), ..Selection::default() }, true);
        assert!(problem.is_none() && t.accent == crate::theme::Rgb(255, 0, 0), "{problem:?}");
        // removal
        assert!(uninstall(&src, "nord").unwrap_err().contains("built into"));
        assert!(uninstall(&src, "nope").is_err());
        assert!(uninstall(&src, "../x").is_err());
        let sel = Selection { name: "demo".into(), ..Selection::default() };
        src.write_selection(&sel).unwrap();
        assert!(uninstall(&src, "demo").unwrap_err().contains("in use"));
        src.write_selection(&Selection::default()).unwrap();
        uninstall(&src, "demo").unwrap();
        assert!(!src.themes_dir.join("demo").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    fn install_dir(src: &Source, entries: &[crate::theme::engine::archive::Entry]) -> (Installed, Report) {
        package::install_entries(entries, &src.themes_dir, false, &Limits::default()).unwrap()
    }

    #[test]
    fn scaffold_duplicate_and_export_round_trip() {
        let (d, src) = sandbox("dup");
        std::env::set_var("DECEBAL_SYSTEM_THEMES", d.join("nosystem"));
        let dir = scaffold(&src, "mine", None, Some("nord")).unwrap();
        assert!(dir.join("theme.toml").is_file());
        assert!(scaffold(&src, "mine", None, None).is_err());
        assert!(scaffold(&src, "Bad Id", None, None).is_err());
        let i = info_for(&src, "mine");
        assert!(i.error.is_none(), "{:?}", i.error);
        assert_eq!(i.name, "Mine");
        // the scaffold of a base looks like the base
        let a = src.load_def("mine").unwrap().resolve("m", true, &Selection::default());
        let b = src.load_def("nord").unwrap().resolve("m", true, &Selection::default());
        assert_eq!((a.background, a.accent, a.radius), (b.background, b.accent, b.radius));
        // duplicating a package copies its assets
        install_dir(&src, &good());
        let copy = duplicate(&src, "demo", "demo-2", None).unwrap();
        assert!(copy.join("wallpapers/w.png").is_file());
        assert_eq!(info_for(&src, "demo-2").name, "Demo copy");
        assert!(duplicate(&src, "demo", "demo-2", None).is_err());
        // duplicating a built-in makes a standalone user copy
        duplicate(&src, "dracula", "drac2", Some("Drac 2")).unwrap();
        let (x, y) = (src.load_def("dracula").unwrap().resolve("a", true, &Selection::default()), src.load_def("drac2").unwrap().resolve("a", true, &Selection::default()));
        assert_eq!((x.background, x.popup_alpha), (y.background, y.popup_alpha));
        // export and re-install elsewhere
        let out = d.join("demo.dtheme");
        export(&src, "demo", &out).unwrap();
        let (_, other) = sandbox("dup2");
        std::env::set_var("DECEBAL_SYSTEM_THEMES", d.join("nosystem"));
        let (got, _) = install(&other, &out, false).unwrap();
        assert_eq!(got.id, "demo");
        let hash_ok = info_for(&other, "demo").hash;
        assert_eq!(hash_ok, HashState::Ok);
        // a built-in exports and installs under its id only with --force
        let nord = d.join("nord.dtheme");
        export(&src, "nord", &nord).unwrap();
        assert!(install(&other, &nord, false).unwrap_err().contains("built-in"));
        let _ = png(1, 1, [0; 3]);
        let _ = std::fs::remove_dir_all(d);
    }
}
