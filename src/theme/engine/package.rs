//! Theme packages as sets of files: reading a directory, validating the set, hashing, packing, unpacking and installing.
//!
//! The same [`check`] runs on files read from a directory (`theme validate DIR`, `theme pack`, `theme install DIR`) and on
//! entries of a `.dtheme` (`theme install FILE`), so a package cannot pass as an archive and fail as a folder or vice versa.
//! Nothing is written outside the destination folder, nothing is followed through a symlink, and nothing but data is accepted.

use super::archive::{self, Entry, Limits};
use super::manifest::{self, Analysis, Diag, Level};
use super::schema::AssetKind;
use super::sha256;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "theme.toml";
/// Longest side of a bundled image, and its pixel budget (a decompression bomb guard: decoding 100 MP needs 400 MB).
pub const MAX_IMAGE_SIDE: u32 = 16384;
pub const MAX_IMAGE_PIXELS: u64 = 100_000_000;
pub const MAX_MANIFEST: usize = 256 << 10;
pub const MAX_WIDGET_FILE: usize = 64 << 10;
pub const MAX_FONT: usize = 16 << 20;

/// What `check` found out about a package.
#[derive(Debug, Clone)]
pub struct Report {
    pub analysis: Analysis,
    /// Problems outside the manifest (bad files, missing assets, hash mismatch...).
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    pub id: Option<String>,
    pub content_hash: String,
    pub total_bytes: u64,
    pub files: usize,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.analysis.ok() && self.problems.is_empty()
    }

    /// Every error, one per line, manifest errors first.
    pub fn errors(&self) -> Vec<String> {
        let mut v: Vec<String> = self.analysis.errors().map(|d| format!("{MANIFEST}: {d}")).collect();
        v.extend(self.problems.iter().cloned());
        v
    }

    pub fn all_warnings(&self) -> Vec<String> {
        let mut v: Vec<String> = self.analysis.diags.iter().filter(|d| d.level == Level::Warning).map(|d: &Diag| format!("{MANIFEST}: {d}")).collect();
        v.extend(self.warnings.iter().cloned());
        v
    }
}

// ---------------------------------------------------------------- reading a directory

/// Reads every file under `root`. Symlinks, special files, hidden files, deep nesting and anything past the caps are errors.
pub fn read_dir(root: &Path, lim: &Limits) -> Result<Vec<Entry>, String> {
    let meta = std::fs::symlink_metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let mut out = Vec::new();
    let mut total = 0u64;
    walk(root, "", 0, lim, &mut out, &mut total)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, depth: usize, lim: &Limits, out: &mut Vec<Entry>, total: &mut u64) -> Result<(), String> {
    if depth > 6 {
        return Err(format!("{} is nested too deeply", dir.display()));
    }
    let mut names: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().collect();
    names.sort_by_key(|e| e.file_name());
    for e in names {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let path = e.path();
        let m = std::fs::symlink_metadata(&path).map_err(|e| format!("{rel}: {e}"))?;
        let ft = m.file_type();
        if ft.is_symlink() {
            return Err(format!("`{rel}` is a symlink (links are not allowed in themes)"));
        }
        if ft.is_dir() {
            walk(&path, &rel, depth + 1, lim, out, total)?;
        } else if ft.is_file() {
            archive::safe_rel_path(&rel).map_err(|e| format!("`{rel}`: {e}"))?;
            if m.len() > lim.max_file {
                return Err(format!("`{rel}` is {} MiB, the per-file limit is {} MiB", m.len() >> 20, lim.max_file >> 20));
            }
            *total += m.len();
            if *total > lim.max_total {
                return Err(format!("package is larger than {} MiB", lim.max_total >> 20));
            }
            if out.len() >= lim.max_files {
                return Err(format!("more than {} files", lim.max_files));
            }
            out.push(Entry { path: rel, data: std::fs::read(&path).map_err(|e| format!("`{path:?}`: {e}"))? });
        } else {
            return Err(format!("`{rel}` is not a regular file"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- hashing

/// The text of `theme.toml` as it is hashed: without its `[integrity]` table, so the hash can live inside the manifest.
pub fn manifest_for_hash(text: &str) -> String {
    match text.parse::<toml_edit::DocumentMut>() {
        Ok(mut d) => {
            d.remove("integrity");
            d.to_string()
        }
        Err(_) => text.to_string(),
    }
}

/// `sha256:<hex>` over `path NUL length NUL sha256(content) LF` of every file sorted by path.
pub fn content_hash(entries: &[Entry]) -> String {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut h = sha256::Sha256::default();
    for e in sorted {
        let data = if e.path == MANIFEST { manifest_for_hash(&String::from_utf8_lossy(&e.data)).into_bytes() } else { e.data.clone() };
        h.update(format!("{}\0{}\0{}\n", e.path, data.len(), sha256::digest_hex(&data)).as_bytes());
    }
    format!("sha256:{}", sha256::hex(&h.finish()))
}

/// Sets `[integrity] content_hash` in the manifest text, keeping the rest of the file as it is.
pub fn with_hash(text: &str, hash: &str) -> Result<String, String> {
    let mut d: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{MANIFEST}: {e}"))?;
    if !d.contains_table("integrity") {
        d["integrity"] = toml_edit::table();
    }
    d["integrity"]["content_hash"] = toml_edit::value(hash);
    Ok(d.to_string())
}

// ---------------------------------------------------------------- checking

fn ext(path: &str) -> String {
    path.rsplit_once('.').map(|x| x.1.to_ascii_lowercase()).unwrap_or_default()
}

/// Verifies the magic bytes of an image and its size; returns a problem text.
pub fn check_image(path: &str, data: &[u8]) -> Option<String> {
    let want = match ext(path).as_str() {
        "png" => image::ImageFormat::Png,
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        "webp" => image::ImageFormat::WebP,
        _ => return Some(format!("`{path}`: not an image type decebal accepts (png, jpg, webp)")),
    };
    match image::guess_format(data) {
        Ok(f) if f == want => {}
        _ => return Some(format!("`{path}`: the content is not a real {} image", ext(path))),
    }
    let dims = image::ImageReader::with_format(std::io::Cursor::new(data), want).into_dimensions();
    match dims {
        Ok((w, h)) if w == 0 || h == 0 => Some(format!("`{path}`: empty image")),
        Ok((w, h)) if w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE || u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS => {
            Some(format!("`{path}`: {w}x{h} pixels is too large (limit {MAX_IMAGE_SIDE} px a side, {} MP)", MAX_IMAGE_PIXELS / 1_000_000))
        }
        Ok(_) => None,
        Err(e) => Some(format!("`{path}`: unreadable image ({e})")),
    }
}

fn check_font(path: &str, data: &[u8]) -> Option<String> {
    if data.len() > MAX_FONT {
        return Some(format!("`{path}`: font is larger than {} MiB", MAX_FONT >> 20));
    }
    match data.get(..4) {
        Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") => None,
        _ => Some(format!("`{path}`: not a TrueType/OpenType font")),
    }
}

/// Where each kind of file may live.
fn classify(path: &str) -> Result<&'static str, String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.starts_with('.') {
        return Err(format!("`{path}`: hidden files are not allowed"));
    }
    match (path, ext(path).as_str()) {
        (MANIFEST, _) => Ok("manifest"),
        (p, "toml") if p.starts_with("widgets/") && p.matches('/').count() == 1 => Ok("widget"),
        (p, "md" | "txt") if !p.contains('/') => Ok("doc"),
        (_, "png" | "jpg" | "jpeg" | "webp") => Ok("image"),
        (p, "ttf" | "otf") if p.starts_with("fonts/") => Ok("font"),
        (_, "toml") => Err(format!("`{path}`: .toml files are only allowed as `{MANIFEST}` and `widgets/<name>.toml`")),
        (_, e) => Err(format!("`{path}`: files of type `.{e}` are not allowed in a theme (data only: images, fonts, widget definitions)")),
    }
}

fn asset_list(def: &crate::theme::model::Def) -> Vec<(String, AssetKind, &'static str)> {
    let mut v = Vec::new();
    let mut add = |val: Option<&Value>, kind: AssetKind, what: &'static str| match val {
        Some(Value::String(s)) => v.push((s.clone(), kind, what)),
        Some(Value::Array(a)) => v.extend(a.iter().filter_map(|x| x.as_str()).map(|s| (s.to_string(), kind, what))),
        _ => {}
    };
    let e = &def.extra;
    add(e.get("meta").and_then(|m| m.get("screenshots")), AssetKind::Image, "screenshot");
    add(e.get("wallpaper").and_then(|m| m.get("image")), AssetKind::Image, "wallpaper");
    add(e.get("wallpaper").and_then(|m| m.get("images")), AssetKind::Image, "wallpaper");
    add(e.get("font").and_then(|m| m.get("files")), AssetKind::Font, "font");
    if let Some(Value::String(s)) = e.get("branding").and_then(|b| b.get("logo")) {
        if matches!(crate::panel::branding::parse_logo(s), Ok(crate::panel::branding::Logo::File(_))) {
            v.push((s.clone(), AssetKind::Image, "logo"));
        }
    }
    v
}

/// Validates a set of package files. Never writes anything.
pub fn check(entries: &[Entry], lim: &Limits) -> Report {
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let total: u64 = entries.iter().map(|e| e.data.len() as u64).sum();
    if entries.len() > lim.max_files {
        problems.push(format!("more than {} files", lim.max_files));
    }
    if total > lim.max_total {
        problems.push(format!("package is larger than {} MiB", lim.max_total >> 20));
    }
    let mut seen = BTreeSet::new();
    let lower: BTreeSet<String> = entries.iter().map(|e| e.path.to_ascii_lowercase()).collect();
    if lower.len() != entries.len() {
        problems.push("two files differ only in letter case".into());
    }
    let paths: BTreeSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();

    let Some(man) = entries.iter().find(|e| e.path == MANIFEST) else {
        let analysis = Analysis { def: None, diags: vec![Diag { level: Level::Error, line: 0, key: MANIFEST.into(), msg: "missing: a theme package has a `theme.toml` at its root".into() }] };
        return Report { analysis, problems, warnings, id: None, content_hash: content_hash(entries), total_bytes: total, files: entries.len() };
    };
    if man.data.len() > MAX_MANIFEST {
        problems.push(format!("{MANIFEST} is larger than {} KiB", MAX_MANIFEST >> 10));
    }
    let text = String::from_utf8_lossy(&man.data).into_owned();
    let analysis = manifest::analyze(&text);

    for e in entries {
        if let Err(m) = archive::safe_rel_path(&e.path) {
            problems.push(m);
            continue;
        }
        if !seen.insert(&e.path) {
            problems.push(format!("`{}` appears twice", e.path));
        }
        match classify(&e.path) {
            Err(m) => problems.push(m),
            Ok("image") => problems.extend(check_image(&e.path, &e.data)),
            Ok("font") => problems.extend(check_font(&e.path, &e.data)),
            Ok("widget") => {
                if e.data.len() > MAX_WIDGET_FILE {
                    problems.push(format!("`{}` is larger than {} KiB", e.path, MAX_WIDGET_FILE >> 10));
                } else if let Err(m) = crate::widgets::dsl::parse(&String::from_utf8_lossy(&e.data), crate::widgets::Trust::Theme) {
                    problems.push(format!("{}: {m}", e.path));
                }
            }
            _ => {}
        }
    }

    let mut id = None;
    if let Some(def) = &analysis.def {
        let meta = def.extra.get("meta");
        id = meta.and_then(|m| m.get("id")).and_then(Value::as_str).map(String::from);
        if id.is_none() {
            problems.push(format!("{MANIFEST}: `id` is required in a theme package"));
        }
        if meta.and_then(|m| m.get("version")).is_none() {
            problems.push(format!("{MANIFEST}: `version` is required in a theme package"));
        }
        for k in ["author", "license"] {
            if meta.and_then(|m| m.get(k)).is_none() {
                warnings.push(format!("{MANIFEST}: no `{k}` (the theme market will require it)"));
            }
        }
        for (path, kind, what) in asset_list(def) {
            match entries.iter().find(|e| e.path == path) {
                None => problems.push(format!("{what} `{path}` is not in the package")),
                Some(e) => match kind {
                    AssetKind::Image => problems.extend(check_image(&path, &e.data)),
                    AssetKind::Font => problems.extend(check_font(&path, &e.data)),
                },
            }
        }
        for w in def.extra.get("widgets").and_then(Value::as_array).into_iter().flatten() {
            if let Some(u) = w.get("use").and_then(Value::as_str).filter(|u| !u.starts_with("builtin:")) {
                if !paths.contains(format!("widgets/{u}.toml").as_str()) {
                    warnings.push(format!("widget `{u}` is not in this package; it is looked up in the parent theme and ~/.config/decebal/widgets at apply time"));
                }
            }
        }
        if let Some(Value::String(want)) = def.extra.get("integrity").and_then(|i| i.get("content_hash")) {
            let got = content_hash(entries);
            if *want != got {
                problems.push(format!("content hash mismatch: the manifest says {want}, the files hash to {got} (the package was modified after it was packed)"));
            }
        }
    }
    Report { analysis, problems, warnings, id, content_hash: content_hash(entries), total_bytes: total, files: entries.len() }
}

// ---------------------------------------------------------------- pack / unpack / install

/// Packs a validated directory into `.dtheme` bytes, recording the content hash in the manifest. Refuses an invalid package.
pub fn pack_dir(dir: &Path, lim: &Limits) -> Result<(Vec<u8>, Report), String> {
    let mut entries = read_dir(dir, lim)?;
    let report = check_ignoring_hash(&mut entries, lim);
    if !report.ok() {
        return Err(report.errors().join("\n"));
    }
    let hash = content_hash(&entries);
    if let Some(m) = entries.iter_mut().find(|e| e.path == MANIFEST) {
        m.data = with_hash(&String::from_utf8_lossy(&m.data), &hash)?.into_bytes();
    }
    let bytes = archive::write_targz(&entries)?;
    Ok((bytes, Report { content_hash: hash, ..report }))
}

/// `check` for a directory that is about to be (re)hashed: a stale hash in the manifest is not an error there.
fn check_ignoring_hash(entries: &mut [Entry], lim: &Limits) -> Report {
    let fresh = content_hash(entries);
    if let Some(m) = entries.iter_mut().find(|e| e.path == MANIFEST) {
        if let Ok(t) = with_hash(&String::from_utf8_lossy(&m.data), &fresh) {
            if String::from_utf8_lossy(&m.data).contains("content_hash") {
                m.data = t.into_bytes();
            }
        }
    }
    check(entries, lim)
}

pub fn read_archive_file(path: &Path, lim: &Limits) -> Result<Vec<Entry>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if meta.len() > lim.max_archive {
        return Err(format!("{} is larger than {} MiB", path.display(), lim.max_archive >> 20));
    }
    archive::read_targz(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?, lim)
}

/// Entries of a path that is a directory or a `.dtheme`.
pub fn read_any(path: &Path, lim: &Limits) -> Result<Vec<Entry>, String> {
    if std::fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false) {
        read_dir(path, lim)
    } else {
        read_archive_file(path, lim)
    }
}

fn write_entries(dest: &Path, entries: &[Entry]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    for e in entries {
        archive::safe_rel_path(&e.path)?;
        let target = dest.join(&e.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&target).map_err(|er| format!("{}: {er}", target.display()))?;
        f.write_all(&e.data).map_err(|er| format!("{}: {er}", target.display()))?;
    }
    Ok(())
}

/// Writes the entries of an already validated package into the new directory `dest` (which must not exist).
pub fn unpack_to(entries: &[Entry], dest: &Path) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    write_entries(dest, entries).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(dest);
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub dir: PathBuf,
    pub replaced: bool,
}

/// Installs a validated package as `<themes_dir>/<id>/`. The files are written to a hidden temporary folder first and renamed
/// into place, so a failed install leaves nothing half-written and an existing theme is only replaced with `force`.
pub fn install_entries(entries: &[Entry], themes_dir: &Path, force: bool, lim: &Limits) -> Result<(Installed, Report), String> {
    let report = check(entries, lim);
    if !report.ok() {
        return Err(report.errors().join("\n"));
    }
    let id = report.id.clone().ok_or("the package has no id")?;
    if crate::theme::builtin::source(&id).is_some() && !force {
        return Err(format!("`{id}` is the id of a built-in theme; installing it would replace the built-in look (use --force, or change the id)"));
    }
    std::fs::create_dir_all(themes_dir).map_err(|e| format!("{}: {e}", themes_dir.display()))?;
    let dest = themes_dir.join(&id);
    let exists = std::fs::symlink_metadata(&dest).is_ok();
    if exists && !force {
        return Err(format!("`{id}` is already installed (use --force to replace it, or `decthemectl theme uninstall {id}`)"));
    }
    let tmp = themes_dir.join(format!(".install-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if let Err(e) = write_entries(&tmp, entries) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let old = themes_dir.join(format!(".old-{id}-{}", std::process::id()));
    if exists {
        std::fs::rename(&dest, &old).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        if exists {
            let _ = std::fs::rename(&old, &dest);
        }
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("{}: {e}", dest.display()));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok((Installed { id, dir: dest, replaced: exists }, report))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb(rgb))).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    pub const MANI: &str = "format = 1\nid = \"demo\"\nname = \"Demo\"\nversion = \"1.0.0\"\nauthor = \"t\"\nlicense = \"MIT\"\nscreenshots = [\"shots/a.png\"]\n[wallpaper]\nimage = \"wallpapers/w.png\"\n[colors.dark]\nbackground = \"#101010\"\ntext = \"#eeeeee\"\naccent = \"#ff0000\"\n";

    pub fn good() -> Vec<Entry> {
        vec![
            Entry { path: "theme.toml".into(), data: MANI.as_bytes().to_vec() },
            Entry { path: "shots/a.png".into(), data: png(8, 6, [1, 2, 3]) },
            Entry { path: "wallpapers/w.png".into(), data: png(16, 9, [9, 9, 9]) },
        ]
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decebal-pkg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_good_package_passes() {
        let r = check(&good(), &Limits::default());
        assert!(r.ok(), "{:?}", r.errors());
        assert_eq!(r.id.as_deref(), Some("demo"));
        assert!(r.content_hash.starts_with("sha256:") && r.content_hash.len() == 71);
    }

    #[test]
    fn bad_files_are_named() {
        let lim = Limits::default();
        let mut p = good();
        p.push(Entry { path: "run.sh".into(), data: b"#!/bin/sh\nrm -rf ~\n".to_vec() });
        p.push(Entry { path: "bin/evil.so".into(), data: vec![0x7f, b'E', b'L', b'F'] });
        p.push(Entry { path: ".hidden.png".into(), data: png(2, 2, [0; 3]) });
        p.push(Entry { path: "extra.toml".into(), data: b"x=1".to_vec() });
        p.push(Entry { path: "notes/readme.md".into(), data: b"hi".to_vec() });
        let e = check(&p, &lim).errors().join("\n");
        for needle in ["run.sh", "evil.so", "hidden files", "extra.toml", "notes/readme.md"] {
            assert!(e.contains(needle), "{needle} in:\n{e}");
        }
        // a PNG that is really a script, and an image bomb header
        let mut q = good();
        q[2].data = b"#!/bin/sh\necho pwned".to_vec();
        assert!(check(&q, &lim).errors().join("\n").contains("not a real png"));
        let mut b = png(4, 4, [0; 3]);
        b[16..20].copy_from_slice(&60000u32.to_be_bytes()); // width 60000
        b[20..24].copy_from_slice(&60000u32.to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(&b[12..29]);
        b[29..33].copy_from_slice(&crc.sum().to_be_bytes()); // a valid header that claims a huge image
        let mut z = good();
        z[2].data = b;
        assert!(check(&z, &lim).errors().join("\n").contains("too large"), "{:?}", check(&z, &lim).errors());
    }

    #[test]
    fn missing_assets_and_manifest_are_reported() {
        let mut p = good();
        p.remove(2);
        assert!(check(&p, &Limits::default()).errors().join("\n").contains("wallpaper `wallpapers/w.png` is not in the package"));
        assert!(check(&[], &Limits::default()).errors().join("\n").contains("missing"));
        let mut q = good();
        q[0].data = MANI.replace("id = \"demo\"\n", "").into_bytes();
        assert!(check(&q, &Limits::default()).errors().join("\n").contains("`id` is required"));
    }

    #[test]
    fn hash_covers_every_file_and_ignores_only_the_integrity_table() {
        let lim = Limits::default();
        let base = good();
        let h = content_hash(&base);
        assert_eq!(h, content_hash(&base), "stable");
        let hashed = with_hash(MANI, &h).unwrap();
        let mut sealed = base.clone();
        sealed[0].data = hashed.into_bytes();
        assert_eq!(content_hash(&sealed), h, "adding the hash line does not change the hash");
        assert!(check(&sealed, &lim).ok());
        let mut t = sealed.clone();
        t[2].data = png(16, 9, [8, 8, 8]);
        assert!(check(&t, &lim).errors().join("\n").contains("content hash mismatch"));
        let mut u = sealed.clone();
        u[0].data = String::from_utf8_lossy(&u[0].data).replace("#ff0000", "#00ff00").into_bytes();
        assert!(check(&u, &lim).errors().join("\n").contains("content hash mismatch"), "colours are covered too");
    }

    #[test]
    fn directories_refuse_symlinks_and_oversize() {
        let d = tmp("dir");
        unpack_to(&good(), &d.join("p")).unwrap();
        assert_eq!(read_dir(&d.join("p"), &Limits::default()).unwrap().len(), 3);
        std::os::unix::fs::symlink("/etc/passwd", d.join("p/passwd.png")).unwrap();
        let e = read_dir(&d.join("p"), &Limits::default()).unwrap_err();
        assert!(e.contains("symlink"), "{e}");
        std::fs::remove_file(d.join("p/passwd.png")).unwrap();
        std::os::unix::fs::symlink(&d, d.join("p/loop")).unwrap();
        assert!(read_dir(&d.join("p"), &Limits::default()).unwrap_err().contains("symlink"));
        std::fs::remove_file(d.join("p/loop")).unwrap();
        let tiny = Limits { max_file: 100, ..Limits::default() };
        assert!(read_dir(&d.join("p"), &tiny).unwrap_err().contains("per-file limit"));
        // the root itself may not be a link
        std::os::unix::fs::symlink(d.join("p"), d.join("link")).unwrap();
        assert!(read_dir(&d.join("link"), &Limits::default()).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn pack_unpack_round_trip_and_tamper_detection() {
        let d = tmp("pack");
        unpack_to(&good(), &d.join("src")).unwrap();
        let (bytes, rep) = pack_dir(&d.join("src"), &Limits::default()).unwrap();
        let (bytes2, _) = pack_dir(&d.join("src"), &Limits::default()).unwrap();
        assert_eq!(bytes, bytes2, "packing is reproducible");
        let entries = archive::read_targz(&bytes, &Limits::default()).unwrap();
        let r = check(&entries, &Limits::default());
        assert!(r.ok(), "{:?}", r.errors());
        assert_eq!(r.content_hash, rep.content_hash);
        unpack_to(&entries, &d.join("out")).unwrap();
        assert_eq!(std::fs::read(d.join("out/wallpapers/w.png")).unwrap(), good()[2].data);
        assert!(unpack_to(&entries, &d.join("out")).is_err(), "never into an existing folder");
        let mut t = entries.clone();
        t[2].data.push(0);
        assert!(!check(&t, &Limits::default()).ok());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn install_is_atomic_and_guards_ids() {
        let d = tmp("inst");
        let lim = Limits::default();
        let (i, _) = install_entries(&good(), &d, false, &lim).unwrap();
        assert_eq!((i.id.as_str(), i.replaced), ("demo", false));
        assert!(d.join("demo/theme.toml").is_file() && d.join("demo/wallpapers/w.png").is_file());
        assert!(install_entries(&good(), &d, false, &lim).unwrap_err().contains("already installed"));
        let (again, _) = install_entries(&good(), &d, true, &lim).unwrap();
        assert!(again.replaced);
        let leftovers: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(leftovers, ["demo"], "no temporary folders left");
        let mut nord = good();
        nord[0].data = MANI.replace("\"demo\"", "\"nord\"").into_bytes();
        assert!(install_entries(&nord, &d, false, &lim).unwrap_err().contains("built-in"));
        let mut bad = good();
        bad.push(Entry { path: "x.sh".into(), data: vec![] });
        assert!(install_entries(&bad, &d, true, &lim).is_err());
        assert!(!d.join("x.sh").exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
