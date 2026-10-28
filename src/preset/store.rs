//! Presets on disk: capture the current configuration, list/save/rename/duplicate/delete, import and export bundles,
//! preview ([`plan`]) and [`apply`] a preset. All paths come from an [`Env`], so tests run in a temp directory.

use super::bundle::{self, MANIFEST};
use super::fsx;
use super::manifest::{self, Manifest, Origin};
use super::parts::{self, Part, PartSet, RunItem};
use super::validate::{self, FileKind};
use crate::settings::io::Paths;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Env {
    pub shell_dir: PathBuf,
    pub rdwm_config: PathBuf,
    pub presets_dir: PathBuf,
    pub themes_dir: PathBuf,
    pub backgrounds_dir: PathBuf,
    pub rdwmctl: PathBuf,
}

/// `$DECEBAL_PRESETS`, else `$XDG_CONFIG_HOME/decebal/presets`, else `~/.config/decebal/presets`.
pub fn presets_dir() -> PathBuf {
    std::env::var_os("DECEBAL_PRESETS")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(|x| PathBuf::from(x).join("decebal/presets")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/decebal/presets")))
        .unwrap_or_else(|| PathBuf::from("presets"))
}

impl Env {
    pub fn from_paths(p: &Paths) -> Env {
        Env {
            shell_dir: p.shell_dir.clone(),
            rdwm_config: p.rdwm_config.clone(),
            presets_dir: presets_dir(),
            themes_dir: crate::theme::live::Source::discover().themes_dir,
            backgrounds_dir: p.backgrounds(),
            rdwmctl: p.rdwmctl.clone(),
        }
    }

    pub fn discover() -> Env {
        Env::from_paths(&Paths::discover())
    }

    /// Where a bundle file name lives on this machine.
    pub fn target(&self, file: &str) -> PathBuf {
        match file {
            "rdwm.toml" => self.rdwm_config.clone(),
            other => self.shell_dir.join(other),
        }
    }

    fn kind_of(file: &str) -> Option<FileKind> {
        Some(match file {
            "decebal.json" => FileKind::Shell,
            "dock-settings.json" => FileKind::Dock,
            "dock-pinned-macos.json" => FileKind::Pinned,
            "rdwm.toml" => FileKind::Rdwm,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------- the preset value

#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// Folder name (`caelestia-night`), or `builtin/<name>` for the shipped ones.
    pub id: String,
    pub manifest: Manifest,
    /// Everything except `manifest.toml`, by bundle name.
    pub files: BTreeMap<String, Vec<u8>>,
}

pub const BUILTIN_PREFIX: &str = "builtin/";

impl Preset {
    pub fn is_builtin(&self) -> bool {
        self.manifest.origin == Origin::Builtin
    }

    pub fn is_backup(&self) -> bool {
        self.manifest.origin == Origin::Backup
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.files.get(name).and_then(|b| std::str::from_utf8(b).ok())
    }

    pub fn set_text(&mut self, name: &str, text: String) {
        self.files.insert(name.to_string(), text.into_bytes());
    }

    /// Manifest included.
    pub fn bundle_files(&self) -> BTreeMap<String, Vec<u8>> {
        let mut f = self.files.clone();
        f.insert(MANIFEST.into(), self.manifest.render().into_bytes());
        f
    }

    pub fn to_bundle(&self) -> Result<Vec<u8>, String> {
        bundle::write_tar_gz(&self.bundle_files())
    }

    /// Parses and fully validates a bundle without touching the disk.
    pub fn from_bundle(bytes: &[u8]) -> Result<Preset, String> {
        Preset::from_files(bundle::read_tar_gz(bytes)?)
    }

    pub fn from_files(mut files: BTreeMap<String, Vec<u8>>) -> Result<Preset, String> {
        // presets exported (or kept in the presets folder) before the rename call the shell settings `macshell.json`
        if !files.contains_key("decebal.json") {
            if let Some(old) = files.remove(bundle::LEGACY_SHELL_FILE) {
                files.insert("decebal.json".into(), old);
            }
        }
        bundle::check_files(&files)?;
        let m = files.remove(MANIFEST).ok_or("manifest.toml is missing")?;
        let manifest = Manifest::parse(std::str::from_utf8(&m).map_err(|_| "manifest.toml is not UTF-8 text")?)?;
        let p = Preset { id: manifest::slug(&manifest.name), manifest, files };
        p.validate()?;
        Ok(p)
    }

    /// Consistency between manifest and files, and every file parses.
    pub fn validate(&self) -> Result<(), String> {
        let m = &self.manifest;
        for part in &m.parts.0 {
            if !self.files.contains_key(part.file()) {
                return Err(format!("the preset lists the part `{}` but has no {}", part.id(), part.file()));
            }
        }
        for (name, data) in &self.files {
            let kind = bundle::check_name(name)?;
            match kind {
                bundle::NameKind::Text => {
                    if !m.parts.any_in(name) {
                        return Err(format!("`{name}` is in the preset but none of its parts uses it"));
                    }
                    let text = std::str::from_utf8(data).map_err(|_| format!("`{name}` is not UTF-8 text"))?;
                    let fk = Env::kind_of(name).ok_or("unknown file")?;
                    if fk == FileKind::Rdwm {
                        parts::parse_toml(text).map_err(|e| format!("rdwm.toml: {e}"))?;
                    } else if let Some(d) = validate::validate(fk, &without_nulls(text, m.merge), Path::new("rdwmctl"), Path::new("x")).first() {
                        return Err(format!("{name}: {}", validate::describe(d)));
                    }
                }
                bundle::NameKind::Theme => {
                    if m.theme_file.as_deref() != Some(name) {
                        return Err(format!("`{name}` is not announced by the manifest"));
                    }
                    let text = std::str::from_utf8(data).map_err(|_| format!("`{name}` is not UTF-8 text"))?;
                    validate::validate_theme(text).map_err(|e| format!("{name}: {e}"))?;
                }
                bundle::NameKind::Wallpaper => {
                    if m.wallpaper.as_deref() != Some(name) {
                        return Err(format!("`{name}` is not announced by the manifest"));
                    }
                    if !looks_like_image(data) {
                        return Err(format!("`{name}` is not a PNG, JPEG or WebP image"));
                    }
                }
                bundle::NameKind::Manifest => {}
            }
        }
        for (what, name) in [("theme_file", &m.theme_file), ("wallpaper_file", &m.wallpaper)] {
            if let Some(n) = name {
                if !self.files.contains_key(n) {
                    return Err(format!("the manifest names {what} `{n}` but it is not in the preset"));
                }
            }
        }
        if m.wallpaper.is_some() && !m.parts.has(Part::Wallpaper) {
            return Err("a wallpaper image without the wallpaper part".into());
        }
        Ok(())
    }

    /// Everything this preset would make the desktop run.
    pub fn commands(&self) -> Vec<RunItem> {
        let mut out = Vec::new();
        for file in ["decebal.json", "dock-settings.json", "dock-pinned-macos.json"] {
            if let Some(v) = self.text(file).and_then(|t| serde_json::from_str::<Value>(t).ok()) {
                out.extend(parts::json_commands(file, &v));
            }
        }
        if let Some(t) = self.text("rdwm.toml") {
            out.extend(parts::toml_commands(t));
        }
        out
    }

    /// Removes every command (see `parts::strip_toml_commands`) and the autostart part.
    pub fn strip_commands(&mut self) -> Result<(), String> {
        for file in ["decebal.json", "dock-settings.json", "dock-pinned-macos.json"] {
            if let Some(t) = self.text(file) {
                let mut v: Value = serde_json::from_str(t).map_err(|e| format!("{file}: {e}"))?;
                parts::strip_json_commands(&mut v);
                self.set_text(file, parts::pretty_json(&v));
            }
        }
        if let Some(t) = self.text("rdwm.toml") {
            let s = parts::strip_toml_commands(t)?;
            self.set_text("rdwm.toml", s);
        }
        self.manifest.parts.toggle(Part::Autostart, false);
        self.manifest.commands_stripped = true;
        if self.manifest.parts.is_empty() {
            return Err("nothing is left once the commands are removed".into());
        }
        Ok(())
    }
}

/// Merge-mode presets use `null` for "remove this key"; the typed checks do not know that, so look at the text without them.
fn without_nulls(text: &str, merge: bool) -> String {
    fn walk(v: &mut Value) {
        if let Value::Object(o) = v {
            let nulls: Vec<String> = o.iter().filter(|(_, x)| x.is_null()).map(|(k, _)| k.clone()).collect();
            for k in nulls {
                o.shift_remove(&k);
            }
            o.values_mut().for_each(walk);
        }
    }
    match (merge, serde_json::from_str::<Value>(text)) {
        (true, Ok(mut v)) => {
            walk(&mut v);
            v.to_string()
        }
        _ => text.to_string(),
    }
}

fn looks_like_image(d: &[u8]) -> bool {
    d.starts_with(&[0x89, b'P', b'N', b'G']) || d.starts_with(&[0xff, 0xd8, 0xff]) || (d.len() > 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP")
}

// ---------------------------------------------------------------- reading the live configuration

fn read_json(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(t) if t.trim().is_empty() => Ok(Value::Object(Default::default())),
        Ok(t) => serde_json::from_str::<Value>(&t).ok().filter(Value::is_object).ok_or_else(|| format!("{} is not valid JSON right now", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Default::default())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn theme_name(doc: &Value) -> Option<String> {
    match doc.get("theme")? {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("name").and_then(Value::as_str).map(String::from),
        _ => None,
    }
}

fn image_name(path: &str) -> Option<String> {
    let f = Path::new(path).file_name()?.to_string_lossy().into_owned();
    let clean: String = f.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect();
    let clean = clean.trim_start_matches('.').to_string();
    let ext = clean.rsplit_once('.')?.1.to_ascii_lowercase();
    (IMAGE_OK.contains(&ext.as_str()) && clean.len() <= 80).then_some(clean)
}

const IMAGE_OK: [&str; 4] = bundle::IMAGE_EXTS;

#[derive(Debug, Clone)]
pub struct CaptureOpts {
    pub name: String,
    pub description: String,
    pub author: String,
    pub parts: PartSet,
    pub embed_wallpaper: bool,
}

/// Snapshot of the live files for `opts.parts`. The second value lists things that could not be included.
pub fn capture(env: &Env, opts: &CaptureOpts) -> Result<(Preset, Vec<String>), String> {
    manifest::check_name(&opts.name)?;
    if opts.parts.is_empty() {
        return Err("tick at least one part".into());
    }
    let mut notes = Vec::new();
    let mut m = Manifest::new(&opts.name, opts.parts.clone());
    m.description = opts.description.trim().to_string();
    m.author = opts.author.trim().to_string();
    m.created = fsx::now_iso();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    if opts.parts.any_in("decebal.json") {
        let doc = read_json(&env.target("decebal.json"))?;
        let share = parts::extract_json(&doc, &opts.parts);
        files.insert("decebal.json".into(), parts::pretty_json(&share).into_bytes());
        if opts.parts.has(Part::Shell) {
            if let Some(id) = theme_name(&doc).map(|n| n.trim().to_ascii_lowercase()) {
                let name = format!("themes/{id}.toml");
                if bundle::theme_id(&name).is_some() && crate::theme::builtin::source(&id).is_none() {
                    match std::fs::read_to_string(env.themes_dir.join(format!("{id}.toml"))) {
                        Ok(t) if validate::validate_theme(&t).is_ok() && t.len() as u64 <= bundle::MAX_TEXT => {
                            files.insert(name.clone(), t.into_bytes());
                            m.theme_file = Some(name);
                        }
                        Ok(_) => notes.push(format!("your theme `{id}` could not be included (invalid or too large)")),
                        Err(_) => {}
                    }
                }
            }
        }
    }
    for (part, file) in [(Part::Dock, "dock-settings.json"), (Part::Pinned, "dock-pinned-macos.json")] {
        if opts.parts.has(part) {
            let mut doc = read_json(&env.target(file))?;
            parts::strip_secrets_json(&mut doc);
            files.insert(file.into(), parts::pretty_json(&doc).into_bytes());
        }
    }
    if opts.parts.any_in("rdwm.toml") {
        let text = read_text(&env.rdwm_config)?;
        let share = parts::extract_toml(&text, &opts.parts).map_err(|e| format!("your rdwm config has an error: {e}"))?;
        if opts.parts.has(Part::Wallpaper) && opts.embed_wallpaper {
            let image = parts::parse_toml(&share).ok().and_then(|d| d.get("wallpaper").and_then(|w| w.get("image")).and_then(|i| i.as_str()).map(String::from));
            match image {
                Some(img) => {
                    let path = PathBuf::from(crate::settings::panes::expand_tilde(&img));
                    match (image_name(&img), std::fs::metadata(&path)) {
                        (Some(name), Ok(md)) if md.is_file() && md.len() <= bundle::MAX_IMAGE => match std::fs::read(&path) {
                            Ok(data) if looks_like_image(&data) => {
                                files.insert(format!("wallpaper/{name}"), data);
                                m.wallpaper = Some(format!("wallpaper/{name}"));
                            }
                            _ => notes.push("the wallpaper could not be embedded (not a PNG, JPEG or WebP image)".into()),
                        },
                        (None, _) => notes.push("the wallpaper could not be embedded (unsupported file type)".into()),
                        (_, Ok(md)) if md.len() > bundle::MAX_IMAGE => notes.push("the wallpaper is too large to embed (limit 24 MB); only its path is saved".into()),
                        _ => notes.push("the wallpaper file was not found; only its path is saved".into()),
                    }
                }
                None => notes.push("the config has no wallpaper image to embed".into()),
            }
        }
        files.insert("rdwm.toml".into(), share.into_bytes());
    }
    let p = Preset { id: manifest::slug(&m.name), manifest: m, files };
    p.validate()?;
    Ok((p, notes))
}

// ---------------------------------------------------------------- folders

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && !id.starts_with('-') && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Reads one preset folder: plain files only, no symlinks, size limits.
pub fn load_dir(dir: &Path) -> Result<Preset, String> {
    let id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !valid_id(&id) {
        return Err(format!("`{id}` is not a valid preset folder name"));
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        return Err("not a folder".into());
    }
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    fn walk(base: &Path, rel: &str, files: &mut BTreeMap<String, Vec<u8>>, depth: usize) -> Result<(), String> {
        let dir = if rel.is_empty() { base.to_path_buf() } else { base.join(rel) };
        for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let relname = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let md = std::fs::symlink_metadata(e.path()).map_err(|e| e.to_string())?;
            if md.file_type().is_symlink() {
                return Err(format!("`{relname}` is a symbolic link"));
            }
            if md.is_dir() {
                if depth == 0 && matches!(name.as_str(), "themes" | "wallpaper") {
                    walk(base, &relname, files, depth + 1)?;
                    continue;
                }
                return Err(format!("unexpected folder `{relname}`"));
            }
            let kind = bundle::check_name(&relname)?;
            if md.len() > bundle::size_cap(kind) {
                return Err(format!("`{relname}` is too large"));
            }
            files.insert(relname, std::fs::read(e.path()).map_err(|e| e.to_string())?);
        }
        Ok(())
    }
    walk(dir, "", &mut files, 0)?;
    let mut p = Preset::from_files(files)?;
    p.id = id;
    Ok(p)
}

/// Writes the preset to `presets_dir/<id>/` (all or nothing). `overwrite` replaces an existing folder.
pub fn save(env: &Env, p: &Preset, overwrite: bool) -> Result<(), String> {
    if !valid_id(&p.id) {
        return Err(format!("`{}` is not a valid preset id", p.id));
    }
    if p.is_builtin() {
        return Err("built-in presets cannot be saved over; duplicate one first".into());
    }
    bundle::check_files(&p.bundle_files())?;
    let dest = env.presets_dir.join(&p.id);
    if dest.exists() && !overwrite {
        return Err(format!("a preset called `{}` already exists", p.manifest.name));
    }
    std::fs::create_dir_all(&env.presets_dir).map_err(|e| format!("{}: {e}", env.presets_dir.display()))?;
    let tmp = env.presets_dir.join(format!(".{}.tmp-{}", p.id, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let res = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&tmp)?;
        for (name, data) in p.bundle_files() {
            let path = tmp.join(&name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data)?;
        }
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        std::fs::rename(&tmp, &dest)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("{}: {e}", dest.display()));
    }
    Ok(())
}

/// A folder name not used yet, derived from `name`.
pub fn unique_id(env: &Env, name: &str) -> String {
    let base = manifest::slug(name);
    let base = if base.is_empty() { "preset".to_string() } else { base };
    let mut id = base.clone();
    let mut n = 1;
    while env.presets_dir.join(&id).exists() {
        n += 1;
        id = format!("{}-{n}", base.chars().take(44).collect::<String>());
    }
    id
}

/// A display name not used yet: `Name`, `Name (2)`, ...
pub fn unique_name(env: &Env, name: &str) -> String {
    let taken: Vec<String> = list(env).0.into_iter().map(|p| p.manifest.name.to_lowercase()).collect();
    let name = name.trim();
    let mut n = 1;
    let mut cand = name.to_string();
    while taken.contains(&cand.to_lowercase()) || env.presets_dir.join(manifest::slug(&cand)).exists() {
        n += 1;
        cand = format!("{name} ({n})");
    }
    cand
}

/// Built-ins, then the user's presets by name. Folders that do not load are returned as (folder, reason).
pub fn list(env: &Env) -> (Vec<Preset>, Vec<(String, String)>) {
    let mut out = super::builtin::all();
    let mut bad = Vec::new();
    let mut user = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&env.presets_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            match load_dir(&e.path()) {
                Ok(p) => user.push(p),
                Err(err) => bad.push((name, err)),
            }
        }
    }
    user.sort_by_key(|p| (p.is_backup(), p.manifest.name.to_lowercase()));
    out.extend(user);
    bad.sort();
    (out, bad)
}

pub fn find(env: &Env, id_or_name: &str) -> Option<Preset> {
    let (all, _) = list(env);
    let q = id_or_name.trim().to_lowercase();
    all.iter()
        .find(|p| p.id.to_lowercase() == q)
        .or_else(|| all.iter().find(|p| p.manifest.name.to_lowercase() == q))
        .or_else(|| all.iter().find(|p| p.id.rsplit('/').next() == Some(q.as_str())))
        .cloned()
}

pub fn delete(env: &Env, id: &str) -> Result<(), String> {
    if id.starts_with(BUILTIN_PREFIX) || !valid_id(id) {
        return Err("built-in presets cannot be deleted".into());
    }
    let dir = env.presets_dir.join(id);
    std::fs::symlink_metadata(&dir).map_err(|_| format!("no preset `{id}`"))?;
    std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))
}

pub fn rename(env: &Env, id: &str, new_name: &str) -> Result<Preset, String> {
    manifest::check_name(new_name)?;
    let mut p = find(env, id).ok_or_else(|| format!("no preset `{id}`"))?;
    if p.is_builtin() {
        return Err("built-in presets cannot be renamed; duplicate one first".into());
    }
    let new_id = manifest::slug(new_name);
    if new_id != p.id && env.presets_dir.join(&new_id).exists() {
        return Err(format!("a preset called `{}` already exists", new_name.trim()));
    }
    let old = p.id.clone();
    p.manifest.name = new_name.trim().to_string();
    p.id = new_id;
    save(env, &p, true)?;
    if old != p.id {
        let _ = delete(env, &old);
    }
    Ok(p)
}

pub fn duplicate(env: &Env, id: &str, new_name: Option<&str>) -> Result<Preset, String> {
    let mut p = find(env, id).ok_or_else(|| format!("no preset `{id}`"))?;
    let name = unique_name(env, new_name.unwrap_or(&format!("{} copy", p.manifest.name)));
    manifest::check_name(&name)?;
    p.manifest.name = name;
    p.manifest.origin = Origin::Local;
    p.id = unique_id(env, &p.manifest.name);
    save(env, &p, false)?;
    Ok(p)
}

/// Stores an imported (already parsed) preset. Commands are removed when `strip` is set.
pub fn install(env: &Env, mut p: Preset, strip: bool) -> Result<Preset, String> {
    if strip {
        p.strip_commands()?;
    }
    p.manifest.origin = Origin::Imported;
    p.manifest.name = unique_name(env, &p.manifest.name);
    p.id = unique_id(env, &p.manifest.name);
    p.validate()?;
    save(env, &p, false)?;
    Ok(p)
}

pub fn export_to(p: &Preset, path: &Path) -> Result<usize, String> {
    let bytes = p.to_bundle()?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })?;
    Ok(bytes.len())
}

/// Reads a bundle file (size-checked before reading).
pub fn read_bundle_file(path: &Path) -> Result<Preset, String> {
    let md = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !md.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if md.len() as usize > bundle::MAX_BUNDLE {
        return Err("the file is too large to be a preset".into());
    }
    Preset::from_bundle(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
}

// ---------------------------------------------------------------- plan (preview) and apply

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    New,
    Changed,
    Same,
}

#[derive(Debug, Clone)]
pub struct Change {
    /// What the user knows the file as.
    pub label: String,
    pub path: PathBuf,
    pub status: Status,
    /// Key-level differences, capped.
    pub lines: Vec<String>,
    pub total: usize,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub path: PathBuf,
    pub label: String,
    pub bytes: Vec<u8>,
    pub kind: Option<FileKind>,
    pub status: Status,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub targets: Vec<Target>,
    /// Everything the preset would make the desktop run.
    pub commands: Vec<RunItem>,
    /// The subset not already configured on this machine.
    pub new_commands: Vec<RunItem>,
    pub notes: Vec<String>,
    pub errors: Vec<String>,
}

impl Plan {
    pub fn changed(&self) -> bool {
        self.targets.iter().any(|t| t.status != Status::Same)
    }
}

fn label_of(file: &str) -> String {
    match file {
        "decebal.json" => "Shell settings (decebal.json)".into(),
        "dock-settings.json" => "Dock (dock-settings.json)".into(),
        "dock-pinned-macos.json" => "Pinned apps (dock-pinned-macos.json)".into(),
        "rdwm.toml" => "Window manager (config.toml)".into(),
        other => other.to_string(),
    }
}

fn wallpaper_dest(env: &Env, p: &Preset) -> Option<PathBuf> {
    let f = p.manifest.wallpaper.as_deref()?.strip_prefix("wallpaper/")?;
    Some(env.backgrounds_dir.join("presets").join(manifest::slug(&p.manifest.name)).join(f))
}

/// What applying `p` would write. Pure with respect to the files (nothing is changed).
pub fn plan(env: &Env, p: &Preset) -> Plan {
    let mut pl = Plan::default();
    let m = &p.manifest;
    let mut cur_commands: Vec<RunItem> = Vec::new();

    let push = |pl: &mut Plan, file: &str, path: PathBuf, status: Status, bytes: Vec<u8>, old_flat: BTreeMap<String, String>, new_flat: BTreeMap<String, String>| {
        let (lines, total) = if status == Status::Same { (Vec::new(), 0) } else { parts::diff_lines(&old_flat, &new_flat, 14) };
        pl.changes.push(Change { label: label_of(file), path: path.clone(), status, lines, total });
        pl.targets.push(Target { path, label: label_of(file), bytes, kind: Env::kind_of(file), status });
    };

    // --- decebal.json
    if m.parts.any_in("decebal.json") {
        let path = env.target("decebal.json");
        match (read_json(&path), p.text("decebal.json").and_then(|t| serde_json::from_str::<Value>(t).ok())) {
            (Ok(cur), Some(pre)) => {
                cur_commands.extend(parts::json_commands("decebal.json", &cur));
                let new = parts::merge_json(&cur, &pre, &m.parts, m.merge);
                let (a, b) = (parts::flatten_json(&cur), parts::flatten_json(&new));
                let status = if path.exists() {
                    if a == b {
                        Status::Same
                    } else {
                        Status::Changed
                    }
                } else {
                    Status::New
                };
                push(&mut pl, "decebal.json", path, status, parts::pretty_json(&new).into_bytes(), a, b);
            }
            (Err(e), _) => pl.errors.push(e),
            (_, None) => pl.errors.push("decebal.json in the preset is not valid".into()),
        }
    }
    // --- dock files
    for (part, file) in [(Part::Dock, "dock-settings.json"), (Part::Pinned, "dock-pinned-macos.json")] {
        if !m.parts.has(part) {
            continue;
        }
        let path = env.target(file);
        match (read_json(&path), p.text(file).and_then(|t| serde_json::from_str::<Value>(t).ok())) {
            (Ok(cur), Some(pre)) => {
                cur_commands.extend(parts::json_commands(file, &cur));
                let new = if part == Part::Dock || m.merge { parts::overlay_file_json(&cur, &pre) } else { pre };
                let (a, b) = (parts::flatten_json(&cur), parts::flatten_json(&new));
                let status = if path.exists() {
                    if a == b {
                        Status::Same
                    } else {
                        Status::Changed
                    }
                } else {
                    Status::New
                };
                push(&mut pl, file, path, status, parts::pretty_json(&new).into_bytes(), a, b);
            }
            (Err(e), _) => pl.errors.push(e),
            (_, None) => pl.errors.push(format!("{file} in the preset is not valid")),
        }
    }
    // --- rdwm
    if m.parts.any_in("rdwm.toml") {
        let path = env.rdwm_config.clone();
        match (read_text(&path), p.text("rdwm.toml")) {
            (Ok(cur), Some(pre)) => {
                cur_commands.extend(parts::toml_commands(&cur));
                let mut pre = pre.to_string();
                if let Some(dest) = wallpaper_dest(env, p) {
                    if let Ok(mut doc) = parts::parse_toml(&pre) {
                        if let Some(w) = doc.get_mut("wallpaper").and_then(|w| w.as_table_like_mut()) {
                            w.insert("image", toml_edit::value(dest.to_string_lossy().into_owned()));
                            pre = doc.to_string();
                        }
                    }
                } else if m.parts.has(Part::Wallpaper) {
                    if let Some(img) = parts::parse_toml(&pre).ok().and_then(|d| d.get("wallpaper").and_then(|w| w.get("image")).and_then(|i| i.as_str()).map(String::from)) {
                        if !Path::new(&crate::settings::panes::expand_tilde(&img)).exists() {
                            pl.notes.push(format!("the wallpaper {img} does not exist on this computer"));
                        }
                    }
                }
                match parts::merge_toml(&cur, &pre, &m.parts, m.merge) {
                    Ok(new) => {
                        let (a, b) = (parts::flatten_toml(&cur), parts::flatten_toml(&new));
                        let status = if path.exists() {
                            if a == b {
                                Status::Same
                            } else {
                                Status::Changed
                            }
                        } else {
                            Status::New
                        };
                        push(&mut pl, "rdwm.toml", path, status, new.into_bytes(), a, b);
                    }
                    Err(e) => pl.errors.push(e),
                }
            }
            (Err(e), _) => pl.errors.push(e),
            (_, None) => pl.errors.push("rdwm.toml in the preset is missing".into()),
        }
    }
    // --- custom theme and wallpaper image
    if let Some(name) = &m.theme_file {
        if let (Some(id), Some(data)) = (bundle::theme_id(name), p.files.get(name)) {
            let path = env.themes_dir.join(format!("{id}.toml"));
            let status = match std::fs::read(&path) {
                Ok(old) if old == *data => Status::Same,
                Ok(_) => Status::Changed,
                Err(_) => Status::New,
            };
            pl.changes.push(Change { label: format!("Theme file ({id}.toml)"), path: path.clone(), status, lines: Vec::new(), total: 0 });
            pl.targets.push(Target { path, label: format!("theme {id}"), bytes: data.clone(), kind: None, status });
        }
    }
    if let (Some(dest), Some(name)) = (wallpaper_dest(env, p), &m.wallpaper) {
        if let Some(data) = p.files.get(name) {
            let status = match std::fs::read(&dest) {
                Ok(old) if old == *data => Status::Same,
                Ok(_) => Status::Changed,
                Err(_) => Status::New,
            };
            pl.changes.push(Change { label: format!("Wallpaper image ({} KB)", data.len() / 1024), path: dest.clone(), status, lines: Vec::new(), total: 0 });
            pl.targets.push(Target { path: dest, label: "wallpaper image".into(), bytes: data.clone(), kind: None, status });
        }
    }
    // --- commands: only those of parts being applied
    let all: Vec<RunItem> = p.commands();
    let applied = |r: &RunItem| -> bool {
        if r.source.starts_with("hot corner") {
            m.parts.has(Part::HotCorners)
        } else if r.source.starts_with("rdwm autostart") || r.source.starts_with("rdwm plugin") || r.source.starts_with("rdwm scratchpad") {
            m.parts.has(Part::Autostart)
        } else if r.source.starts_with("rdwm") {
            m.parts.has(Part::Keys)
        } else if r.source.starts_with("decebal.json") {
            m.parts.has(Part::Shell)
        } else if r.source.starts_with("dock-settings") {
            m.parts.has(Part::Dock)
        } else {
            m.parts.has(Part::Pinned)
        }
    };
    pl.commands = all.into_iter().filter(applied).collect();
    pl.new_commands = pl.commands.iter().filter(|c| !cur_commands.iter().any(|x| x.command == c.command)).cloned().collect();
    pl
}

#[derive(Debug, Clone, Default)]
pub struct Applied {
    /// Id of the restore point written before applying.
    pub restore_point: Option<String>,
    pub written: Vec<String>,
    pub rdwm_changed: bool,
    pub warnings: Vec<String>,
}

pub fn restore_point_name(p: &Preset) -> String {
    let n = format!("before-{}", manifest::slug(&p.manifest.name));
    n.chars().take(60).collect::<String>().trim_end_matches('-').to_string()
}

/// Validates everything first, writes a restore point, then writes the files. A failure while writing restores the files
/// already written. `restore_point = false` is used when applying a restore point itself.
pub fn apply(env: &Env, p: &Preset, restore_point: bool) -> Result<Applied, String> {
    let pl = plan(env, p);
    if !pl.errors.is_empty() {
        return Err(pl.errors.join("; "));
    }
    // 1. validate every file that will be written (rdwm through rdwmctl check-config)
    for t in &pl.targets {
        if t.status == Status::Same {
            continue;
        }
        if let Some(kind) = t.kind {
            let text = String::from_utf8(t.bytes.clone()).map_err(|_| format!("{}: not UTF-8", t.label))?;
            if let Some(d) = validate::validate(kind, &text, &env.rdwmctl, &t.path).first() {
                return Err(format!("{} would not be valid ({}); nothing was changed", t.label, validate::describe(d)));
            }
        }
    }
    let mut out = Applied::default();
    // 2. restore point of the parts being replaced
    if restore_point {
        let parts_set = p.manifest.parts.clone();
        let opts = CaptureOpts {
            name: restore_point_name(p),
            description: format!("Automatic restore point made before applying \"{}\".", p.manifest.name),
            author: String::new(),
            parts: parts_set,
            embed_wallpaper: false,
        };
        match capture(env, &opts) {
            Ok((mut rp, _)) => {
                rp.manifest.origin = Origin::Backup;
                rp.id = manifest::slug(&rp.manifest.name);
                match save(env, &rp, true) {
                    Ok(()) => out.restore_point = Some(rp.id.clone()),
                    Err(e) => out.warnings.push(format!("no restore point: {e}")),
                }
            }
            Err(e) => out.warnings.push(format!("no restore point: {e} (the files are still backed up one by one)")),
        }
    }
    // 3. write, remembering what to put back
    let mut done: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    for t in pl.targets.iter().filter(|t| t.status != Status::Same) {
        let old = std::fs::read(&t.path).ok();
        let res = (|| -> Result<(), String> {
            if let Some(parent) = t.path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            if t.kind.is_some() {
                fsx::save_with_backup(&t.path, std::str::from_utf8(&t.bytes).map_err(|_| "not UTF-8".to_string())?)?;
            } else {
                write_bytes(&t.path, &t.bytes)?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => {
                done.push((t.path.clone(), old));
                out.written.push(t.label.clone());
                if t.path == env.rdwm_config {
                    out.rdwm_changed = true;
                }
            }
            Err(e) => {
                for (path, old) in done.into_iter().rev() {
                    match old {
                        Some(b) => {
                            let _ = write_bytes(&path, &b);
                        }
                        None => {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
                return Err(format!("{}: {e}; the earlier changes were undone", t.label));
            }
        }
    }
    Ok(out)
}

fn write_bytes(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub fn sandbox(name: &str) -> Env {
        let d = std::env::temp_dir().join(format!("decebal-preset-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        std::fs::create_dir_all(d.join("cfg/rdwm")).unwrap();
        Env {
            shell_dir: d.join("shell"),
            rdwm_config: d.join("cfg/rdwm/config.toml"),
            presets_dir: d.join("presets"),
            themes_dir: d.join("themes"),
            backgrounds_dir: d.join("shell/backgrounds"),
            rdwmctl: PathBuf::from("/nonexistent/rdwmctl"),
        }
    }

    const RDWM: &str = "# mine\n[general]\nborder_px = 2\n\n[wallpaper]\nimage = \"/nonexistent/wall.png\"\nmode = \"fill\"\n\n[[bind]]\nmods = [\"super\"]\nkey = \"Return\"\naction = \"spawn\"\nargs = [\"foot\"]\n\n[[autostart]]\ncommand = \"decbar &\"\n\n[[output]]\nname = \"DP-1\"\n";

    fn fill(env: &Env) {
        std::fs::write(
            env.shell_dir.join("decebal.json"),
            r#"{"theme": {"name": "nord", "mode": "dark", "font": "Inter"}, "bar": {"enabled": true}, "hot_corners": {"top_left": "dashboard", "bottom_right": "custom:notify-send hi"}}"#,
        )
        .unwrap();
        std::fs::write(env.shell_dir.join("dock-settings.json"), r#"{"appSpacing": 8, "style": "mac"}"#).unwrap();
        std::fs::write(env.shell_dir.join("dock-pinned-macos.json"), r#"{"pinned": ["firefox"], "order": ["firefox"]}"#).unwrap();
        std::fs::write(&env.rdwm_config, RDWM).unwrap();
    }

    fn opts(name: &str, p: &[Part]) -> CaptureOpts {
        CaptureOpts { name: name.into(), description: "d".into(), author: String::new(), parts: PartSet::new(p.iter().copied()), embed_wallpaper: false }
    }

    #[test]
    fn capture_save_list_load_round_trip() {
        let env = sandbox("capture");
        fill(&env);
        let (p, notes) = capture(&env, &opts("My Setup", &[Part::Shell, Part::Fonts, Part::HotCorners, Part::Dock, Part::Rdwm, Part::Keys])).unwrap();
        assert!(notes.is_empty());
        assert_eq!(p.id, "my-setup");
        assert!(p.files.contains_key("decebal.json") && p.files.contains_key("dock-settings.json") && p.files.contains_key("rdwm.toml"));
        assert!(!p.files.contains_key("dock-pinned-macos.json"));
        let rd = p.text("rdwm.toml").unwrap();
        assert!(rd.contains("border_px") && rd.contains("[[bind]]") && !rd.contains("autostart") && !rd.contains("DP-1") && !rd.contains("wallpaper"), "{rd}");
        save(&env, &p, false).unwrap();
        assert!(save(&env, &p, false).unwrap_err().contains("already exists"));
        let (all, bad) = list(&env);
        assert!(bad.is_empty(), "{bad:?}");
        let mine = all.iter().find(|x| x.id == "my-setup").expect("listed");
        assert_eq!(mine.files, p.files);
        assert_eq!(mine.manifest.name, "My Setup");
        assert!(all.iter().any(|x| x.is_builtin()), "built-ins come first");
        assert!(all[0].is_builtin());
        // bundle round trip
        let b = p.to_bundle().unwrap();
        let back = Preset::from_bundle(&b).unwrap();
        assert_eq!(back.files, p.files);
        assert_eq!(back.manifest, p.manifest);
    }

    #[test]
    fn presets_made_before_the_rename_still_load() {
        let env = sandbox("legacy-preset");
        fill(&env);
        let (p, _) = capture(&env, &opts("Old", &[Part::Shell, Part::Dock])).unwrap();
        // the same files as the old version wrote them: the shell settings are `macshell.json`
        let mut old = p.bundle_files();
        let shell = old.remove("decebal.json").expect("shell part");
        old.insert("macshell.json".into(), shell.clone());
        let bundle = bundle::write_tar_gz(&old).unwrap();
        let back = Preset::from_bundle(&bundle).unwrap();
        assert_eq!(back.files.get("decebal.json"), Some(&shell));
        assert!(!back.files.contains_key("macshell.json"));
        // and a preset folder copied from ~/.config/macshell/presets
        let dir = env.shell_dir.join("presets/old");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in &old {
            std::fs::write(dir.join(name), data).unwrap();
        }
        let loaded = load_dir(&dir).unwrap();
        assert_eq!(loaded.files.get("decebal.json"), Some(&shell));
        // text exported by the old version starts with MACSHELL-PRESET-1:
        let text = bundle::to_text(&bundle).replacen(bundle::TEXT_HEADER, bundle::LEGACY_TEXT_HEADER, 1);
        assert_eq!(Preset::from_bundle(&bundle::from_text(&text).unwrap()).unwrap().files, back.files);
    }

    #[test]
    fn apply_writes_files_makes_a_restore_point_and_can_be_undone() {
        let env = sandbox("apply");
        fill(&env);
        let (p, _) = capture(&env, &opts("Original", &[Part::Shell, Part::Dock, Part::Rdwm])).unwrap();
        save(&env, &p, false).unwrap();
        // change the live config, then apply the saved preset
        std::fs::write(env.shell_dir.join("decebal.json"), r#"{"theme": {"name": "dracula"}, "workspaces": "off"}"#).unwrap();
        std::fs::write(env.shell_dir.join("dock-settings.json"), r#"{"appSpacing": 20, "magnification": 2}"#).unwrap();
        std::fs::write(&env.rdwm_config, "[general]\nborder_px = 9\n[[output]]\nname = \"HDMI-1\"\n").unwrap();
        let pl = plan(&env, &p);
        assert!(pl.errors.is_empty(), "{:?}", pl.errors);
        assert!(pl.changed());
        assert!(pl.changes.iter().any(|c| c.label.contains("decebal.json") && c.lines.iter().any(|l| l.contains("theme.name"))), "{:?}", pl.changes);
        let r = apply(&env, &p, true).unwrap();
        assert_eq!(r.restore_point.as_deref(), Some("before-original"));
        assert!(r.rdwm_changed && r.written.len() == 3);
        let shell: Value = serde_json::from_str(&std::fs::read_to_string(env.shell_dir.join("decebal.json")).unwrap()).unwrap();
        assert_eq!(shell["theme"]["name"], "nord");
        assert!(shell.get("workspaces").is_none(), "replace mode removes what the preset does not have");
        let dock: Value = serde_json::from_str(&std::fs::read_to_string(env.shell_dir.join("dock-settings.json")).unwrap()).unwrap();
        assert_eq!(dock["appSpacing"], 8);
        assert_eq!(dock["magnification"], 2, "the dock part overlays");
        let rd = std::fs::read_to_string(&env.rdwm_config).unwrap();
        assert!(rd.contains("border_px = 2") && rd.contains("HDMI-1"), "outputs are never replaced: {rd}");
        // timestamped backups of the replaced files exist
        assert!(!fsx::backups(&env.shell_dir.join("decebal.json")).is_empty());
        // the restore point puts the earlier state back
        let rp = find(&env, "before-original").expect("restore point");
        assert!(rp.is_backup());
        apply(&env, &rp, false).unwrap();
        let shell: Value = serde_json::from_str(&std::fs::read_to_string(env.shell_dir.join("decebal.json")).unwrap()).unwrap();
        assert_eq!(shell["theme"]["name"], "dracula");
        assert_eq!(shell["workspaces"], "off");
        assert!(std::fs::read_to_string(&env.rdwm_config).unwrap().contains("border_px = 9"));
        // applying an unchanged preset again reports nothing to do
        apply(&env, &rp, false).unwrap();
        assert!(!plan(&env, &rp).changed());
    }

    #[test]
    fn invalid_results_are_never_written() {
        let env = sandbox("invalid");
        fill(&env);
        let (mut p, _) = capture(&env, &opts("Broken", &[Part::Dock])).unwrap();
        p.set_text("dock-settings.json", "{\"appSpacing\": \"wide\"}".into());
        let before = std::fs::read_to_string(env.shell_dir.join("dock-settings.json")).unwrap();
        let e = apply(&env, &p, true).unwrap_err();
        assert!(e.contains("would not be valid"), "{e}");
        assert_eq!(std::fs::read_to_string(env.shell_dir.join("dock-settings.json")).unwrap(), before);
        assert!(find(&env, "before-broken").is_none(), "no restore point for a refused apply");
        // a fake rdwmctl that refuses the merged rdwm config
        let ctl = env.shell_dir.join("rdwmctl");
        std::fs::write(&ctl, "#!/bin/sh\necho 'config error: nope at line 1' >&2\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut env2 = env.clone();
        env2.rdwmctl = ctl;
        let (mut p2, _) = capture(&env2, &opts("Rd", &[Part::Rdwm])).unwrap();
        p2.set_text("rdwm.toml", "[general]\nborder_px = 9\n".into());
        let e = apply(&env2, &p2, true).unwrap_err();
        assert!(e.contains("Line 1") && e.contains("nothing was changed"), "{e}");
    }

    #[test]
    fn rename_duplicate_delete() {
        let env = sandbox("manage");
        fill(&env);
        let (p, _) = capture(&env, &opts("One", &[Part::Dock])).unwrap();
        save(&env, &p, false).unwrap();
        let q = duplicate(&env, "one", None).unwrap();
        assert_eq!(q.manifest.name, "One copy");
        assert_eq!(q.id, "one-copy");
        let q2 = duplicate(&env, "one", Some("One copy")).unwrap();
        assert_eq!(q2.manifest.name, "One copy (2)");
        let r = rename(&env, "one-copy", "Renamed!").unwrap();
        assert_eq!(r.id, "renamed");
        assert!(find(&env, "one-copy").is_none() && find(&env, "renamed").is_some());
        assert!(rename(&env, "renamed", "One").unwrap_err().contains("already exists"));
        assert!(rename(&env, "renamed", "  ").is_err());
        delete(&env, "renamed").unwrap();
        assert!(delete(&env, "renamed").is_err());
        // built-ins are read only but can be duplicated
        let b = list(&env).0.into_iter().find(|x| x.is_builtin()).unwrap();
        assert!(delete(&env, &b.id).is_err());
        assert!(rename(&env, &b.id, "x").is_err());
        assert!(save(&env, &b, true).is_err());
        let d = duplicate(&env, &b.id, Some("Mine")).unwrap();
        assert_eq!(d.manifest.origin, Origin::Local);
        assert!(d.files == b.files && d.id == "mine");
    }

    #[test]
    fn import_strips_commands_and_lists_them() {
        let env = sandbox("import");
        fill(&env);
        let (p, _) = capture(&env, &opts("Shared", &[Part::Shell, Part::HotCorners, Part::Rdwm, Part::Keys, Part::Autostart])).unwrap();
        let cmds = p.commands();
        let srcs: Vec<&str> = cmds.iter().map(|c| c.command.as_str()).collect();
        assert!(srcs.contains(&"decbar &") && srcs.contains(&"foot") && srcs.contains(&"notify-send hi"), "{cmds:?}");
        let bytes = p.to_bundle().unwrap();
        // a second machine
        let env2 = sandbox("import2");
        let parsed = Preset::from_bundle(&bytes).unwrap();
        let pl = plan(&env2, &parsed);
        assert_eq!(pl.new_commands.len(), 3, "an empty config has none of them: {:?}", pl.new_commands);
        let kept = install(&env2, parsed.clone(), false).unwrap();
        assert_eq!(kept.manifest.origin, Origin::Imported);
        assert_eq!(kept.commands().len(), 3);
        let stripped = install(&env2, parsed, true).unwrap();
        assert!(stripped.commands().is_empty(), "{:?}", stripped.commands());
        assert!(stripped.manifest.commands_stripped && !stripped.manifest.parts.has(Part::Autostart));
        assert_eq!(stripped.manifest.name, "Shared (2)");
        assert!(!stripped.text("rdwm.toml").unwrap().contains("decbar"));
        // reload from disk keeps the flag
        let again = find(&env2, &stripped.id).unwrap();
        assert!(again.manifest.commands_stripped);
        // applying the stripped copy never creates a command
        apply(&env2, &stripped, true).unwrap();
        let cfg = std::fs::read_to_string(&env2.rdwm_config).unwrap();
        assert!(parts::toml_commands(&cfg).is_empty(), "{cfg}");
        let shell = std::fs::read_to_string(env2.shell_dir.join("decebal.json")).unwrap();
        assert!(!shell.contains("notify-send"), "{shell}");
    }

    #[test]
    fn preset_folders_with_symlinks_or_strays_are_refused() {
        let env = sandbox("hostile");
        fill(&env);
        let (p, _) = capture(&env, &opts("Fine", &[Part::Dock])).unwrap();
        save(&env, &p, false).unwrap();
        let dir = env.presets_dir.join("fine");
        assert!(load_dir(&dir).is_ok());
        // a symlink inside
        std::os::unix::fs::symlink("/etc/passwd", dir.join("decebal.json")).unwrap();
        assert!(load_dir(&dir).unwrap_err().contains("symbolic link"));
        std::fs::remove_file(dir.join("decebal.json")).unwrap();
        // an unknown file
        std::fs::write(dir.join("run.sh"), "echo").unwrap();
        assert!(load_dir(&dir).unwrap_err().contains("unknown"));
        std::fs::remove_file(dir.join("run.sh")).unwrap();
        // a folder that is itself a symlink
        std::os::unix::fs::symlink(&dir, env.presets_dir.join("linked")).unwrap();
        assert!(load_dir(&env.presets_dir.join("linked")).is_err());
        // list reports it instead of failing
        let (_, bad) = list(&env);
        assert!(bad.iter().any(|(n, _)| n == "linked"));
        // oversized file
        let big = vec![b' '; bundle::MAX_TEXT as usize + 1];
        std::fs::write(dir.join("dock-settings.json"), big).unwrap();
        assert!(load_dir(&dir).unwrap_err().contains("too large"));
    }

    #[test]
    fn validation_catches_inconsistent_presets() {
        let env = sandbox("consistency");
        fill(&env);
        let (p, _) = capture(&env, &opts("C", &[Part::Dock, Part::Rdwm])).unwrap();
        let mut q = p.clone();
        q.files.remove("dock-settings.json");
        assert!(q.validate().unwrap_err().contains("no dock-settings.json"));
        let mut q = p.clone();
        q.files.insert("dock-pinned-macos.json".into(), b"{}".to_vec());
        assert!(q.validate().unwrap_err().contains("none of its parts"));
        let mut q = p.clone();
        q.files.insert("wallpaper/x.png".into(), vec![1, 2, 3]);
        assert!(q.validate().unwrap_err().contains("not announced"));
        let mut q = p.clone();
        q.set_text("rdwm.toml", "[general".into());
        assert!(q.validate().is_err());
        let mut q = p.clone();
        q.manifest.wallpaper = Some("wallpaper/x.png".into());
        assert!(q.validate().is_err());
        let mut q = p.clone();
        q.manifest.parts.toggle(Part::Wallpaper, true);
        q.manifest.wallpaper = Some("wallpaper/x.png".into());
        q.files.insert("wallpaper/x.png".into(), b"not an image".to_vec());
        assert!(q.validate().unwrap_err().contains("not a PNG"));
        assert!(p.validate().is_ok());
    }

    #[test]
    fn embedded_wallpaper_and_theme_travel() {
        let env = sandbox("embed");
        fill(&env);
        let img = env.shell_dir.join("pic.png");
        let png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];
        std::fs::write(&img, &png).unwrap();
        std::fs::write(&env.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\nmode = \"fit\"\n", img.display())).unwrap();
        std::fs::create_dir_all(&env.themes_dir).unwrap();
        std::fs::write(env.themes_dir.join("mine.toml"), "name = \"Mine\"\n").unwrap();
        std::fs::write(env.shell_dir.join("decebal.json"), r#"{"theme": {"name": "mine"}}"#).unwrap();
        let mut o = opts("Pic", &[Part::Shell, Part::Wallpaper]);
        o.embed_wallpaper = true;
        let (p, notes) = capture(&env, &o).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(p.manifest.wallpaper.as_deref(), Some("wallpaper/pic.png"));
        assert_eq!(p.manifest.theme_file.as_deref(), Some("themes/mine.toml"));
        let bytes = p.to_bundle().unwrap();
        // import into another machine and apply
        let env2 = sandbox("embed2");
        let got = install(&env2, Preset::from_bundle(&bytes).unwrap(), true).unwrap();
        let pl = plan(&env2, &got);
        assert!(pl.errors.is_empty(), "{:?}", pl.errors);
        assert!(pl.changes.iter().any(|c| c.label.starts_with("Wallpaper image")) && pl.changes.iter().any(|c| c.label.starts_with("Theme file")));
        apply(&env2, &got, true).unwrap();
        let dest = env2.backgrounds_dir.join("presets/pic/pic.png");
        assert_eq!(std::fs::read(&dest).unwrap(), png);
        let rd = std::fs::read_to_string(&env2.rdwm_config).unwrap();
        assert!(rd.contains(&dest.display().to_string()) && rd.contains("fit"), "{rd}");
        assert!(env2.themes_dir.join("mine.toml").exists());
        // a missing wallpaper file only adds a note
        let (p3, notes) = capture(&env, &{
            std::fs::remove_file(&img).unwrap();
            o.clone()
        })
        .unwrap();
        assert!(p3.manifest.wallpaper.is_none() && !notes.is_empty());
    }

    #[test]
    fn rollback_when_a_write_fails() {
        use std::os::unix::fs::PermissionsExt;
        let mut env = sandbox("rollback");
        fill(&env);
        let ro = env.shell_dir.join("ro");
        std::fs::create_dir_all(&ro).unwrap();
        env.themes_dir = ro.join("themes");
        let (mut p, _) = capture(&env, &opts("RB", &[Part::Dock])).unwrap();
        p.set_text("dock-settings.json", "{\"appSpacing\": 1}".into());
        p.manifest.theme_file = Some("themes/x.toml".into());
        p.files.insert("themes/x.toml".into(), b"name = \"X\"\n".to_vec());
        p.validate().unwrap();
        // the theme folder cannot be created, so the second write fails after the dock file was written
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let e = apply(&env, &p, false);
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = e.unwrap_err();
        assert!(e.contains("undone"), "{e}");
        let dock: Value = serde_json::from_str(&std::fs::read_to_string(env.shell_dir.join("dock-settings.json")).unwrap()).unwrap();
        assert_eq!(dock, json!({"appSpacing": 8, "style": "mac"}), "the first write was undone");
    }
}
