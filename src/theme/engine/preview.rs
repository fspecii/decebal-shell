//! Content-addressed, isolated native theme previews. No theme application or live provider is run.
use super::{apply, sha256, writer};
use crate::settings::io::Paths;
use crate::theme::{
    builtin,
    live::Source,
    model::{Mode, Selection},
};
use crate::widgets::{
    config::{self, WidgetsConfig},
    data, paint,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock, Weak,
    },
    time::{Duration, Instant},
};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

pub const VERSION: &str = "native-theme-preview-7";
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
static SERIAL: AtomicU64 = AtomicU64::new(0);
static CHANGED: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
type Results = HashMap<String, (Instant, Result<Option<PathBuf>, String>)>;
static RESULTS: OnceLock<Mutex<Results>> = OnceLock::new();

type CompletionWake = Arc<dyn Fn() + Send + Sync>;
#[derive(Default)]
struct CompletionListeners {
    next: AtomicU64,
    callbacks: Mutex<Vec<(u64, CompletionWake)>>,
}
impl CompletionListeners {
    fn subscribe(self: &Arc<Self>, wake: CompletionWake) -> PreviewUpdates {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.callbacks.lock().unwrap_or_else(|e| e.into_inner()).push((id, wake));
        PreviewUpdates { listeners: Arc::downgrade(self), id }
    }

    fn notify(&self) {
        let callbacks: Vec<_> = self.callbacks.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(_, wake)| wake.clone()).collect();
        // Never call a UI callback while holding the registry lock.
        for wake in callbacks {
            wake();
        }
    }
}
static COMPLETIONS: OnceLock<Arc<CompletionListeners>> = OnceLock::new();

/// Keeps a window subscribed to background gallery/preview completion.
pub struct PreviewUpdates {
    listeners: Weak<CompletionListeners>,
    id: u64,
}
impl Drop for PreviewUpdates {
    fn drop(&mut self) {
        if let Some(listeners) = self.listeners.upgrade() {
            listeners.callbacks.lock().unwrap_or_else(|e| e.into_inner()).retain(|(id, _)| *id != self.id);
        }
    }
}

/// Wake an event-driven window when discovery or image generation completes.
pub fn subscribe_updates(wake: CompletionWake) -> PreviewUpdates {
    let updates = COMPLETIONS.get_or_init(|| Arc::new(CompletionListeners::default())).subscribe(wake.clone());
    if CHANGED.load(Ordering::Acquire) {
        wake();
    }
    updates
}

fn request_id(src: &Source, selection: &Selection) -> String {
    format!("{:?}:{:?}:{}", src.themes_dir, super::store::installed_roots(), selection.to_json())
}

#[derive(Serialize, Deserialize)]
struct SavedPreview {
    version: String,
    key: String,
}
fn saved_file(dir: &Path, id: &str) -> PathBuf {
    let mut hash = sha256::Sha256::default();
    hash.update(id.as_bytes());
    dir.join("saved").join(format!("{}.json", sha256::hex(&hash.finish())))
}
fn read_saved(dir: &Path, id: &str) -> Option<PathBuf> {
    let entry: SavedPreview = serde_json::from_slice(&fs::read(saved_file(dir, id)).ok()?).ok()?;
    if entry.version != VERSION || entry.key.len() != 64 || !entry.key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let path = dir.join(format!("{}.png", entry.key));
    path.is_file().then_some(path)
}
fn save_preview(dir: &Path, id: &str, key: &str) -> Result<(), String> {
    let path = saved_file(dir, id);
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&SavedPreview { version: VERSION.into(), key: key.into() }).map_err(|e| e.to_string())?;
    // Avoid disk writes on every background validation of an unchanged preview.
    if fs::read(&path).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    let tmp = path.with_extension(format!("{}-{}.tmp", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
    fs::write(&tmp, bytes).and_then(|_| fs::rename(&tmp, &path)).map_err(|e| e.to_string())
}

pub fn cached(src: &Source, selection: &Selection) -> Result<Option<PathBuf>, String> {
    let id = request_id(src, selection);
    let results = RESULTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((_, result)) = results.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned() {
        return result;
    }
    // One tiny saved reference is enough to show last session's PNG immediately.
    // Asset/font validation remains on the worker; never recompute a fingerprint here.
    let result = Ok(read_saved(&cache_dir(), &id));
    results.lock().unwrap_or_else(|e| e.into_inner()).insert(id, (Instant::now() - Duration::from_secs(2), result.clone()));
    result
}

#[derive(Clone, Serialize, Deserialize)]
struct Job {
    themes: PathBuf,
    /// The installed theme roots the parent resolved, in priority order. The worker's own `HOME`/`XDG_*` are private, so it is handed
    /// exactly these (see [`super::store::ROOTS_ENV`]) and refuses to run if it does not see the same list.
    system_roots: Vec<PathBuf>,
    palettes: PathBuf,
    selection: serde_json::Value,
    output: PathBuf,
    work: PathBuf,
    deco: Option<PathBuf>,
    font_dirs: Vec<PathBuf>,
    #[serde(default)]
    wallpaper: Option<PathBuf>,
    /// The user's own panel choices (`bar.edge` / `split` / `applets`, the dock's `edge`): a preview shows the theme the
    /// way it would look with them applied.
    #[serde(default)]
    panel: serde_json::Value,
}

/// The user's panel keys from the real configuration next to `decebal.json` (see docs/universal-panels.md).
fn user_panel(src: &Source) -> serde_json::Value {
    let shell = crate::settings::io::json_load(&src.shell_json);
    let mut bar = serde_json::Map::new();
    for k in ["edge", "split", "applets"] {
        if let Some(v) = shell.get("bar").and_then(|b| b.get(k)) {
            bar.insert(k.into(), v.clone());
        }
    }
    let dock = src.shell_json.parent().map(|d| crate::settings::io::json_load(&d.join(crate::config::SETTINGS_FILE))).unwrap_or_default();
    json!({"bar": bar, "dock_edge": dock.get("edge").cloned().unwrap_or(serde_json::Value::Null)})
}

/// What the preview's panels depend on: the previewed theme's renderer, edge, split and applets after the user's
/// choices, and the dock's edge. The cache key changes exactly when one of them does.
fn panel_fingerprint(src: &Source, selection: &Selection, def: &crate::theme::model::Def) -> String {
    let user_keys = user_panel(src);
    let user = crate::panel::UserPanel::from_bar_json(&user_keys["bar"]);
    let theme = crate::panel::ThemePanel::from_extra(&def.extra);
    let style = def.extra.get("bar").and_then(|b| b.get("style")).and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| def.resolve(&selection.name, true, selection).bar_style);
    let dux = crate::config::BarLayout::from_json(def.extra.get("bar").unwrap_or(&serde_json::Value::Null));
    let i = crate::panel::Inputs {
        renderer: crate::panel::Renderer::for_bar_style(&style),
        user: &user,
        theme: &theme,
        legacy_position: None,
        legacy_workspaces: None,
        dux: Some(&dux),
        dock_user_edge: None,
    };
    let user_dock = user_keys["dock_edge"].as_str().and_then(crate::panel::Edge::parse);
    let dock_edge = user_dock.or(theme.dock_edge).unwrap_or(crate::panel::Edge::Bottom);
    // the taskbar has applets of its own (Start, search, task buttons, show desktop) with their own defaults
    let taskbar = crate::panel::resolve(&crate::panel::Inputs { renderer: crate::panel::Renderer::Taskbar, legacy_workspaces: None, dux: None, dock_user_edge: user_dock, ..i });
    format!("{}/dock:{}/taskbar:{}", crate::panel::resolve(&i).fingerprint(), dock_edge.as_str(), taskbar.fingerprint())
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".cache")).join("decebal/theme-previews")
}

fn renderer() -> PathBuf {
    std::env::var_os("DECEBAL_THEME_PREVIEW_BIN").map(PathBuf::from).unwrap_or_else(|| std::env::current_exe().unwrap_or_default().with_file_name("decthemectl"))
}
fn deco_renderer() -> Option<PathBuf> {
    std::env::var_os("DECEBAL_DECO_RENDERER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| std::env::current_exe().ok().map(|p| p.with_file_name("decebal-deco-preview")).filter(|p| p.is_file()))
}

// Metadata is only consulted on the background worker. ctime catches same-size writes
// even when a producer restores mtime; content remains the authoritative cache key.
type DigestCache = HashMap<PathBuf, (String, [u8; 32])>;
static DIGESTS: Mutex<Option<DigestCache>> = Mutex::new(None);
fn asset_digest(path: &Path) -> Result<[u8; 32], String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err("preview asset is not a regular file".into());
    }
    #[cfg(unix)]
    let stamp = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}:{}:{}:{}:{}", m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec())
    };
    #[cfg(not(unix))]
    let stamp = format!("{}:{:?}:{:?}", m.len(), m.modified(), m.created());
    let mut g = DIGESTS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = g.get_or_insert_with(HashMap::new);
    if let Some((old, digest)) = cache.get(path) {
        if old == &stamp {
            return Ok(*digest);
        }
    }
    if m.len() > 32 << 20 {
        return Err("preview asset too large".into());
    }
    let mut h = sha256::Sha256::default();
    h.update(&fs::read(path).map_err(|e| e.to_string())?);
    let digest = h.finish();
    if cache.len() >= 4096 {
        cache.clear();
    }
    cache.insert(path.to_owned(), (stamp, digest));
    Ok(digest)
}
fn hash_assets(root: &Path, dir: &Path, depth: usize, count: &mut usize, total: &mut u64, hash: &mut sha256::Sha256) -> Result<(), String> {
    if depth > 6 {
        return Err("preview assets nested too deeply".into());
    }
    let mut paths = fs::read_dir(dir).map_err(|e| e.to_string())?.map(|e| e.map(|e| e.path())).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    paths.sort();
    for path in paths {
        *count += 1;
        if *count > 512 {
            return Err("too many preview assets".into());
        }
        let rel = path.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy();
        if rel.starts_with("screenshots/") || rel == "screenshots" || rel.ends_with(".md") || rel.ends_with(".txt") {
            continue;
        }
        let m = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if m.is_dir() {
            hash_assets(root, &path, depth + 1, count, total, hash)?;
        } else {
            *total = total.saturating_add(m.len());
            if *total > 128 << 20 {
                return Err("preview package too large".into());
            }
            hash.update(rel.as_bytes());
            hash.update(&asset_digest(&path)?);
        }
    }
    Ok(())
}

// Candidate backgrounds are an explicitly separate visual sample, never part of theme apply.
fn preview_wallpaper(src: &Source, selection: &Selection) -> Option<PathBuf> {
    let id = src.find(&selection.name).map(|v| v.0).unwrap_or_else(|| selection.name.clone());
    let dark = selection.mode != Mode::Light;
    let picks = crate::settings::wallhaven::curated::choices(&id).ok()?;
    if !picks.is_empty() {
        return crate::settings::wallhaven::curated::cached(&id, dark, &src.shell_json.parent()?.join("backgrounds"));
    }
    // Old built-in palette aliases must not reintroduce generated shipped artwork.
    if builtin::source(&id).is_some() {
        return None;
    }
    let def = src.load_def(&id).ok()?;
    let wp = def.extra.get("wallpaper")?;
    let path = wp.get(if dark { "dark" } else { "light" }).or_else(|| wp.get("image")).and_then(serde_json::Value::as_str)?;
    super::archive::safe_rel_path(path).ok()?;
    let path = config::package_dir(&src.themes_dir, &id)?.join(path);
    fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= crate::settings::wallhaven::client::MAX_IMAGE_BYTES).then_some(path)
}

/// Raw manifests, inherited assets, builtin references, effective palette and native renderer identity.
/// Screenshots/docs are deliberately excluded: generated evidence must not invalidate itself.
pub fn fingerprint(src: &Source, selection: &Selection) -> Result<String, String> {
    let mut hash = sha256::Sha256::default();
    hash.update(VERSION.as_bytes());
    hash.update(selection.to_json().to_string().as_bytes());
    let mut current = Some(selection.name.clone());
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) || seen.len() > 9 {
            return Err("preview inheritance loop".into());
        }
        // `load_def` returns the merged theme, whose `inherit` is already consumed: the parent comes from this package's own manifest.
        let own = if let Some(dir) = config::package_dir(&src.themes_dir, &id) {
            hash_assets(&dir, &dir, 0, &mut 0, &mut 0, &mut hash)?;
            fs::read_to_string(dir.join("theme.toml")).ok()
        } else if let Ok(text) = fs::read(src.themes_dir.join(format!("{id}.toml"))) {
            hash.update(&text);
            Some(String::from_utf8_lossy(&text).into_owned())
        } else if let Some(text) = builtin::source(&id) {
            hash.update(text.as_bytes());
            Some(text.to_string())
        } else {
            None
        };
        src.load_def(&id)?;
        current = own.and_then(|text| crate::theme::model::Def::parse(&text).ok()).and_then(|d| d.inherit);
        // a user theme named like its built-in parent extends that built-in one
        if current.as_deref() == Some(id.as_str()) {
            current = None;
            if let Some(text) = builtin::source(&id) {
                hash.update(text.as_bytes());
            }
        }
    }
    let def = src.load_def(&selection.name)?;
    hash.update(writer::def_to_manifest(&def, &writer::Header { id: selection.name.clone(), name: def.name.clone(), ..Default::default() }).as_bytes());
    hash.update(panel_fingerprint(src, selection, &def).as_bytes());
    if let Some(id) = &selection.palette {
        if let Some(p) = crate::theme::palette::load(id) {
            hash.update(writer::def_to_manifest(&p, &writer::Header { id: id.clone(), name: p.name.clone(), ..Default::default() }).as_bytes());
        }
    }
    for spec in def.extra.get("widgets").and_then(|v| v.as_array()).into_iter().flatten() {
        if let Some(name) = spec.get("use").and_then(|v| v.as_str()).and_then(|s| s.strip_prefix("builtin:")) {
            if let Some(text) = crate::widgets::samples::source(name) {
                hash.update(text.as_bytes());
            }
        }
    }
    if let Ok(picks) = crate::settings::wallhaven::curated::choices(&src.find(&selection.name).map(|v| v.0).unwrap_or_else(|| selection.name.clone())) {
        for pick in picks {
            hash.update(pick.wall.id.as_bytes());
        }
    }
    if let Some(path) = preview_wallpaper(src, selection) {
        hash.update(path.as_os_str().as_encoded_bytes());
        hash.update(&asset_digest(&path)?);
    }
    let fonts = crate::theme::fontdb::get();
    let plan = def.resolve(&selection.name, selection.mode != Mode::Light, selection).fonts;
    let resolved = plan.resolve(&fonts);
    for family in resolved.roles.iter().flatten() {
        hash.update(family.as_bytes());
        if let Some(face) = fonts.face(family, 400) {
            hash.update(face.file.as_os_str().as_encoded_bytes());
            if let Ok(m) = fs::metadata(face.file) {
                hash.update(&m.len().to_le_bytes());
                hash.update(format!("{:?}", m.modified()).as_bytes());
            }
        }
    }
    // VERSION is the output contract: unrelated rebuilds/install locations must
    // not invalidate every stored image. Bump it when native rendering changes.
    hash.update(if deco_renderer().is_some() { b"native-decorations" } else { b"no-decorations" });
    // Explicit custom renderers remain content-addressed, independent of mtime/path.
    for variable in ["DECEBAL_THEME_PREVIEW_BIN", "DECEBAL_DECO_RENDERER"] {
        if let Some(path) = std::env::var_os(variable) {
            hash.update(variable.as_bytes());
            hash.update(&fs::read(path).map_err(|e| e.to_string())?);
        }
    }
    Ok(sha256::hex(&hash.finish()))
}

fn job(src: &Source, selection: &Selection, output: PathBuf) -> Job {
    let work = output.parent().unwrap_or(Path::new(".")).join(format!(".preview-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
    Job {
        themes: src.themes_dir.clone(),
        system_roots: super::store::installed_roots(),
        palettes: crate::theme::palette::user_dir(),
        selection: selection.to_json(),
        output,
        work,
        deco: deco_renderer(),
        wallpaper: preview_wallpaper(src, selection),
        panel: user_panel(src),
        font_dirs: std::env::var_os("HOME").map(PathBuf::from).into_iter().flat_map(|p| [p.join(".local/share/fonts"), p.join(".fonts")]).collect(),
    }
}

fn run(j: &Job) -> Result<(), String> {
    fs::create_dir_all(&j.work).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&j.work, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let result = (|| {
        let file = j.work.join("job.json");
        fs::write(&file, serde_json::to_vec(j).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let xml = |p: &Path| p.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        fs::create_dir_all(cache_dir().join("fontconfig")).map_err(|e| e.to_string())?;
        let config = format!("<?xml version=\"1.0\"?><!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\"><fontconfig><include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>{}<cachedir>{}</cachedir></fontconfig>", j.font_dirs.iter().map(|p|format!("<dir>{}</dir>",xml(p))).collect::<String>(),xml(&cache_dir().join("fontconfig")));
        fs::write(j.work.join("fonts.conf"), config).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(renderer());
        cmd.env("FONTCONFIG_FILE", j.work.join("fonts.conf"));
        // An explicit DECEBAL_SYSTEM_THEMES stays the exclusive root as before; otherwise the resolved list travels as is.
        match std::env::var_os("DECEBAL_SYSTEM_THEMES").filter(|v| !v.is_empty()) {
            Some(explicit) => cmd.env("DECEBAL_SYSTEM_THEMES", explicit).env_remove(super::store::ROOTS_ENV),
            None => cmd.env_remove("DECEBAL_SYSTEM_THEMES").env(super::store::ROOTS_ENV, super::store::encode_roots(&j.system_roots)?),
        };
        cmd.args(["--theme-preview-worker"])
            .arg(&file)
            .env("HOME", &j.work)
            .env("XDG_CONFIG_HOME", &j.work)
            .env("XDG_DATA_HOME", &j.work)
            .env("XDG_STATE_HOME", &j.work)
            .env("XDG_CACHE_HOME", &j.work)
            .env("XDG_RUNTIME_DIR", &j.work)
            .env("DECEBAL_CONFIG", &j.work)
            .env("DECSETTINGS_CONFIG", j.work.join("rdwm.toml"))
            .env("RDWMCTL", "/nonexistent/preview-rdwmctl")
            .env_remove("RDWM_SOCK")
            .env("DECEBAL_THEMES", &j.themes)
            .env("DECEBAL_PALETTES", &j.palettes)
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", "")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .env("DECEBAL_SYSTEM_AUTOMATIC", "0")
            .env("DECEBAL_SCALE", "1")
            // the preview draws the wanted window style with its own decoration helper and never talks to a window manager
            .env(super::decoration_compat::PREVIEW_ENV, "preview")
            // Candidate previews are deterministic theme samples, not session overrides.
            .env_remove("DECEBAL_FONT_SCALE")
            .env_remove("DECEBAL_HIGH_CONTRAST")
            .env_remove("DECEBAL_REDUCE_MOTION")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(fs::File::create(j.work.join("error.log")).map_err(|e| e.to_string())?));
        let mut child = cmd.spawn().map_err(|e| format!("native preview helper unavailable: {e}"))?;
        let until = Instant::now() + Duration::from_secs(45);
        loop {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(s) if s.success() => return Ok(()),
                Some(_) => return Err(fs::read_to_string(j.work.join("error.log")).unwrap_or_else(|_| "native render failed".into())),
                None if Instant::now() >= until => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("native preview timed out".into());
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
    })();
    let _ = fs::remove_dir_all(&j.work);
    result
}

pub fn generate(src: &Source, selection: &Selection) -> Result<PathBuf, String> {
    let key = fingerprint(src, selection)?;
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let output = dir.join(format!("{key}.png"));
    if !output.is_file() {
        if output.with_extension("error").is_file() {
            return Err("Native preview unavailable".into());
        }
        if let Err(e) = run(&job(src, selection, output.clone())) {
            let _ = fs::write(output.with_extension("error"), &e);
            return Err(e);
        }
    }
    save_preview(&dir, &request_id(src, selection), &key)?;
    prune(&dir);
    Ok(output)
}

fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    // A PNG and its JSON sidecar are one preview, not two independent artifacts.
    let mut groups = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if stem.len() != 64 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) || !matches!(path.extension().and_then(|e| e.to_str()), Some("png" | "json" | "error")) {
            continue;
        }
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        groups.entry(stem.to_owned()).and_modify(|old: &mut Option<std::time::SystemTime>| *old = (*old).max(modified)).or_insert(modified);
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(_, modified)| *modified);
    let excess = groups.len().saturating_sub(256);
    for (key, _) in groups.into_iter().take(excess) {
        for extension in ["png", "json", "error"] {
            let _ = fs::remove_file(dir.join(format!("{key}.{extension}")));
        }
    }
}

/// Automatic lazy generation on gallery discovery/import/change. One bounded worker, no per-card process storm.
pub fn request(src: &Source, selection: &Selection) -> Result<Option<PathBuf>, String> {
    // Memory/saved-reference lookup and bounded enqueue only; no asset/font fingerprinting on the GUI thread.
    static QUEUE: OnceLock<mpsc::SyncSender<(String, Source, Selection)>> = OnceLock::new();
    let _ = cached(src, selection);
    let id = request_id(src, selection);
    let results = RESULTS.get_or_init(|| Mutex::new(HashMap::new()));
    let cached = results.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned();
    let queue = QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<(String, Source, Selection)>(32);
        std::thread::spawn(move || {
            for (id, src, selection) in rx {
                let result = generate(&src, &selection).map(Some);
                let mut g = RESULTS.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
                let changed = g.get(&id).is_none_or(|(_, old)| old != &result);
                if g.len() >= 256 {
                    g.clear();
                }
                g.insert(id.clone(), (Instant::now(), result));
                drop(g);
                if let Some(p) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    p.remove(&id);
                }
                if changed {
                    notify_changed();
                }
            }
        });
        tx
    });
    if cached.as_ref().is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(1)) {
        let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if guard.get_or_insert_with(HashSet::new).insert(id.clone()) && queue.try_send((id.clone(), src.clone(), selection.clone())).is_err() {
            guard.as_mut().unwrap().remove(&id);
        }
    }
    cached.map(|(_, result)| result).unwrap_or(Ok(None))
}
pub(crate) fn notify_changed() {
    CHANGED.store(true, Ordering::Release);
    if let Some(listeners) = COMPLETIONS.get() {
        listeners.notify();
    }
}
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::AcqRel)
}

fn composite(canvas: &mut Pixmap, layer: &Pixmap, x: i32, y: i32) {
    canvas.draw_pixmap(x, y, layer.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}
fn image(path: &Path) -> Result<image::DynamicImage, String> {
    let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 << 20);
    let mut reader = reader;
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}

/// Private subprocess entry. Apply *plans* supply the exact production mappings; only private plan files are written.
pub fn worker(file: &Path) -> Result<(), String> {
    let j: Job = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if std::env::var_os("DECEBAL_CONFIG").as_deref() != Some(j.work.as_os_str()) || std::env::var("DECEBAL_SYSTEM_AUTOMATIC").as_deref() != Ok("0") {
        return Err("preview isolation missing".into());
    }
    if super::store::installed_roots() != j.system_roots {
        return Err("preview theme roots missing".into());
    }
    let sel = Selection::from_json(&j.selection);
    let src = Source { shell_json: j.work.join("decebal.json"), themes_dir: j.themes.clone() };
    let paths = Paths { shell_dir: j.work.clone(), rdwm_config: j.work.join("rdwm.toml"), rdwmctl: "/nonexistent/preview-rdwmctl".into(), socket: j.work.join("no.sock") };
    fs::write(src.shell_json.clone(), json!({"theme":sel.to_json(),"system_theme":{"automatic":false}}).to_string()).map_err(|e| e.to_string())?;
    let plan = apply::plan_with_widgets(&src, &paths, &sel.name, &sel, apply::WidgetChoice::Replace)?;
    for write in &plan.writes {
        if !write.path.starts_with(&j.work) {
            return Err("preview plan escaped private config".into());
        }
        fs::write(&write.path, &write.new).map_err(|e| e.to_string())?;
    }
    // the user's panel choices, into the private copy only
    if let Some(user) = j.panel.get("bar").and_then(|b| b.as_object()).filter(|b| !b.is_empty()) {
        let mut shell = crate::settings::io::json_load(&src.shell_json);
        if !shell["bar"].is_object() {
            shell["bar"] = json!({});
        }
        for (k, v) in user {
            shell["bar"][k] = v.clone();
        }
        crate::settings::io::json_set(&src.shell_json, "bar", Some(shell["bar"].clone()))?;
    }
    if let Some(edge) = j.panel.get("dock_edge").and_then(|e| e.as_str()) {
        crate::settings::io::json_set(&paths.dock_settings(), "edge", Some(json!(edge)))?;
    }
    if let Some(wallpaper) = &j.wallpaper {
        let mut config = crate::settings::tomlcfg::Cfg::parse(&fs::read_to_string(&paths.rdwm_config).unwrap_or_default())?;
        config.set("wallpaper", "image", &crate::settings::tomlcfg::Val::Str(wallpaper.display().to_string()))?;
        let mode = if crate::settings::wallhaven::curated::choices(&sel.name).is_ok_and(|p| !p.is_empty()) {
            "fill".to_string()
        } else {
            src.load_def(&sel.name)?.extra.get("wallpaper").and_then(|w| w.get("mode")).and_then(|s| s.as_str()).unwrap_or("fill").to_string()
        };
        config.set("wallpaper", "mode", &crate::settings::tomlcfg::Val::Str(mode))?;
        fs::write(&paths.rdwm_config, config.text()).map_err(|e| e.to_string())?;
    }
    // A requested independent palette is part of the effective preview, not lost by complete-theme reset.
    if sel.palette.is_some() || sel.accent.is_some() || sel.dynamic.is_some() {
        let mut effective = plan.selection.clone();
        effective.palette = sel.palette.clone();
        effective.accent = sel.accent;
        effective.dynamic = sel.dynamic.clone();
        src.write_selection(&effective)?;
    }
    let selection = src.selection();
    let (th, problem) = src.build(&selection, sel.mode != Mode::Light);
    if let Some(e) = problem {
        return Err(e);
    }
    crate::theme::live::publish_theme(th.clone());
    let mut canvas = Pixmap::new(WIDTH, HEIGHT + 120).ok_or("preview canvas")?;
    canvas.fill(th.background.color(1.0));
    let mut notes = vec!["Native foreground samples; compositor glass and motion are not simulated".to_string()];
    let rdwm = crate::settings::tomlcfg::Cfg::parse(&fs::read_to_string(&paths.rdwm_config).unwrap_or_default())?;
    if let Some(crate::settings::tomlcfg::Val::Str(path)) = rdwm.get("wallpaper", "image") {
        let source = image(Path::new(&path))?;
        let mode = rdwm.get("wallpaper", "mode").and_then(|v| if let crate::settings::tomlcfg::Val::Str(s) = v { Some(s) } else { None }).unwrap_or_else(|| "fill".into());
        // The image crate's RGBA is converted through PNG so premultiplication is correct.
        let wall = match mode.as_str() {
            "stretch" => source.resize_exact(WIDTH, HEIGHT, image::imageops::FilterType::Lanczos3),
            "fit" => source.resize(WIDTH, HEIGHT, image::imageops::FilterType::Lanczos3),
            "center" | "tile" => source,
            _ => source.resize_to_fill(WIDTH, HEIGHT, image::imageops::FilterType::Lanczos3),
        };
        let mut png = std::io::Cursor::new(Vec::new());
        wall.write_to(&mut png, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        let wall = Pixmap::decode_png(png.get_ref()).map_err(|e| e.to_string())?;
        if mode == "tile" {
            for y in (0..HEIGHT).step_by(wall.height() as usize) {
                for x in (0..WIDTH).step_by(wall.width() as usize) {
                    composite(&mut canvas, &wall, x as i32, y as i32);
                }
            }
        } else {
            composite(&mut canvas, &wall, (WIDTH as i32 - wall.width() as i32) / 2, (HEIGHT as i32 - wall.height() as i32) / 2);
        }
    } else {
        notes.push("Wallpaper chosen separately; browse Wallpaper → For this theme for suggested photos".to_string());
    }
    let mut fonts = crate::text::Fonts::load().ok_or("preview font unavailable")?;
    let widgets = WidgetsConfig::load(&src.shell_json);
    let mut count = 0;
    if widgets.enabled {
        for inst in widgets.items.iter().filter(|i| i.enabled) {
            if inst.place != config::Place::Desktop {
                notes.push(format!("{}: bar widget unsupported", inst.use_));
                continue;
            }
            match config::resolve(inst, &src.themes_dir, false) {
                Ok(r) => {
                    let mut sample = data::sample();
                    for (k, v) in &inst.options {
                        sample.set(
                            &format!("opt.{k}"),
                            match v {
                                serde_json::Value::Bool(b) => data::Val::Bool(*b),
                                serde_json::Value::Number(n) => data::Val::Num(n.as_f64().unwrap_or(0.0)),
                                serde_json::Value::String(s) => data::Val::Text(s.clone()),
                                _ => continue,
                            },
                        );
                    }
                    if let Some(card) = paint::render(&r.def, &sample, &th, &mut fonts, &r.base, 0) {
                        let (x, y) = config::place_clear(&inst.anchor, inst.x, inst.y, (0, 0, WIDTH as i32, HEIGHT as i32), card.width() as i32, card.height() as i32, th.bar_insets());
                        composite(&mut canvas, &card, x, y);
                        count += 1;
                    }
                }
                Err(e) => notes.push(e),
            }
        }
    }
    let shell = crate::config::shell_settings();
    // the bar and the dock on their effective edges (each draws itself on a desktop-sized canvas)
    if shell.bar_visible(crate::config::dock_settings().style) {
        let bar = crate::menubar::DecBar::preview_frame(WIDTH, HEIGHT, false)?;
        composite(&mut canvas, &bar, 0, 0);
        if th.bar_panel().split && th.bar_vertical() {
            let bar = crate::menubar::DecBar::preview_frame(WIDTH, HEIGHT, true)?;
            composite(&mut canvas, &bar, 0, 0);
        }
    }
    if shell.dock_visible(th.bar_vertical()) {
        let dock = crate::dock_app::DockApp::preview_desktop(WIDTH, HEIGHT)?;
        composite(&mut canvas, &dock, 0, 0);
    }
    let dock = crate::config::dock_settings();
    let dock_panel = if dock.style == crate::config::DockStyle::Windows {
        th.taskbar_panel(&dock).fingerprint()
    } else {
        format!("dock:{}", dock.edge.or(th.panel_theme.dock_edge).unwrap_or(crate::panel::Edge::Bottom).as_str())
    };
    notes.push(format!("Panels: {} / {}", th.bar_panel().fingerprint(), dock_panel));
    crate::gfx::fill_rounded(&mut canvas, 0.0, HEIGHT as f32, WIDTH as f32, 120.0, 0.0, th.background.color(1.0));
    // Separate caption sample footer, not an invented window layout over the desktop.
    if let Some(helper) = j.deco {
        let out = j.work.join("deco.png");
        let status = Command::new(helper).arg("theme").arg(&paths.rdwm_config).arg(&out).status().map_err(|e| e.to_string())?;
        if status.success() {
            let deco = Pixmap::load_png(out).map_err(|e| e.to_string())?;
            composite(&mut canvas, &deco, WIDTH as i32 - 720, HEIGHT as i32 + 28);
        } else {
            notes.push("Decoration renderer failed".into());
        }
    } else {
        notes.push("Decoration native helper unavailable (no simulated caption)".into());
    }
    fonts.draw(
        &mut canvas,
        &format!("{} / {} / NATIVE RENDER / FIXED SAMPLE DATA / {count} desktop widgets", th.name, if th.dark { "DARK" } else { "LIGHT" }),
        20.0,
        HEIGHT as f32 + 12.0,
        16.0,
        crate::text::Weight::Regular,
        th.text.color(1.0),
    );
    for (i, note) in notes.iter().take(3).enumerate() {
        fonts.draw(&mut canvas, note, 20.0, HEIGHT as f32 + 40.0 + i as f32 * 18.0, 13.0, crate::text::Weight::Regular, th.text_secondary.color(1.0));
    }
    let tmp = j.output.with_extension(format!("{}.tmp", std::process::id()));
    canvas.save_png(&tmp).map_err(|e| e.to_string())?;
    fs::rename(tmp, &j.output).map_err(|e| e.to_string())?;
    fs::write(j.output.with_extension("json"), json!({"renderer":VERSION,"selection":selection.to_json(),"widgets":count,"notes":notes,"desktop":[WIDTH,HEIGHT]}).to_string())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    #[test]
    fn completion_wakes_registered_windows_and_unsubscribes_on_drop() {
        let listeners = Arc::new(CompletionListeners::default());
        let (tx, rx) = mpsc::channel();
        let registration = listeners.subscribe(Arc::new(move || {
            tx.send(()).unwrap();
        }));
        let worker = listeners.clone();
        std::thread::spawn(move || worker.notify()).join().unwrap();
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(registration);
        listeners.notify();
        assert!(rx.try_recv().is_err(), "a closed window must not retain a callback");
        assert!(listeners.callbacks.lock().unwrap().is_empty());
    }

    #[test]
    fn the_cache_key_follows_the_effective_panel_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("panel-preview-key-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("themes")).unwrap();
        let src = Source { shell_json: dir.join("decebal.json"), themes_dir: dir.join("themes") };
        let sel = Selection { name: "nord".into(), mode: Mode::Dark, ..Default::default() };
        let key = |shell: serde_json::Value, dock: serde_json::Value| {
            fs::write(&src.shell_json, shell.to_string()).unwrap();
            fs::write(dir.join(crate::config::SETTINGS_FILE), dock.to_string()).unwrap();
            fingerprint(&src, &sel).unwrap()
        };
        let base = key(json!({}), json!({}));
        assert_eq!(key(json!({"bar": {"something_else": 1}, "clock_hour12": true}), json!({"magnification": 2.0})), base, "unrelated settings keep the cached image");
        let moved = key(json!({"bar": {"edge": "left"}}), json!({}));
        let fewer = key(json!({"bar": {"applets": {"clock": false}}}), json!({}));
        let docked = key(json!({}), json!({"edge": "right"}));
        assert!(base != moved && base != fewer && base != docked && moved != fewer, "edge, applets and dock edge each regenerate it");
        // applets only the taskbar draws by default still change what a taskbar theme's preview shows
        let no_tasks = key(json!({"bar": {"applets": {"tasks": false}}}), json!({}));
        let no_desktop = key(json!({"bar": {"applets": {"show_desktop": false}}}), json!({}));
        let no_start = key(json!({"bar": {"applets": {"launcher": false}}}), json!({}));
        assert!(no_tasks != base && no_desktop != base && no_start != base && no_tasks != no_desktop, "taskbar-only applet choices regenerate it");
        // an override equal to the default changes nothing visible, but it is the user's choice: it is part of the key too
        assert_eq!(key(json!({"bar": {"edge": "left"}}), json!({})), moved, "stable for the same inputs");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cache_key_covers_the_files_of_every_parent_in_the_extends_chain() {
        let dir = std::env::temp_dir().join(format!("preview-parent-key-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (user, installed) = (dir.join("themes"), dir.join("installed"));
        let _roots = super::super::store::test_roots(vec![installed.clone()]);
        for (root, id, extends) in [(&user, "kid", "mid"), (&installed, "mid", "root"), (&installed, "root", "")] {
            fs::create_dir_all(root.join(id).join("icons")).unwrap();
            let extends = if extends.is_empty() { String::new() } else { format!("extends = \"{extends}\"\n") };
            fs::write(root.join(id).join("theme.toml"), format!("format = 1\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nauthor = \"t\"\nlicense = \"MIT\"\n{extends}")).unwrap();
            fs::write(root.join(id).join("icons/logo.png"), id).unwrap();
        }
        let src = Source { shell_json: dir.join("decebal.json"), themes_dir: user };
        let sel = Selection { name: "kid".into(), mode: Mode::Dark, ..Default::default() };
        let base = fingerprint(&src, &sel).unwrap();
        assert_eq!(fingerprint(&src, &sel).unwrap(), base);
        for (id, bytes) in [("mid", "changed mid"), ("root", "changed root")] {
            fs::write(installed.join(id).join("icons/logo.png"), bytes).unwrap();
            assert_ne!(fingerprint(&src, &sel).unwrap(), base, "{id}");
            fs::write(installed.join(id).join("icons/logo.png"), id).unwrap();
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn official_preview_only_uses_downloaded_recommendations_and_tracks_their_content() {
        let dir = std::env::temp_dir().join(format!("curated-preview-{}", std::process::id()));
        fs::create_dir_all(dir.join("backgrounds")).unwrap();
        let src = Source { shell_json: dir.join("decebal.json"), themes_dir: dir.join("themes") };
        let sel = Selection { name: "aster".into(), mode: Mode::Dark, ..Default::default() };
        let uncached = fingerprint(&src, &sel).unwrap();
        assert!(preview_wallpaper(&src, &sel).is_none());
        let choice = crate::settings::wallhaven::curated::choices("aster").unwrap().remove(0);
        let photo = dir.join(format!("backgrounds/wallhaven-{}.png", choice.wall.id));
        fs::write(&photo, b"cached photography test bytes").unwrap();
        assert_eq!(preview_wallpaper(&src, &sel), Some(photo.clone()));
        let cached = fingerprint(&src, &sel).unwrap();
        assert_ne!(cached, uncached);
        fs::write(&photo, b"changed photography test bytes").unwrap();
        assert_ne!(fingerprint(&src, &sel).unwrap(), cached);
        assert!(preview_wallpaper(&src, &Selection { mode: Mode::Light, ..sel }).is_none());
        assert!(!src.shell_json.exists(), "preview lookup never applies configuration");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn asset_digest_reuses_unchanged_content_and_tracks_same_size_rewrites() {
        let dir = std::env::temp_dir().join(format!("preview-digest-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wallpaper");
        fs::write(&path, vec![1u8; 4 << 20]).unwrap();
        let first = asset_digest(&path).unwrap();
        let before = Instant::now();
        for _ in 0..100 {
            assert_eq!(asset_digest(&path).unwrap(), first);
        }
        eprintln!("100 warm 4MiB asset digest lookups: {:?}", before.elapsed());
        let old_mtime = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, vec![2u8; 4 << 20]).unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_times(fs::FileTimes::new().set_modified(old_mtime)).unwrap();
        assert_ne!(asset_digest(&path).unwrap(), first);
        fs::remove_file(&path).unwrap();
        assert!(asset_digest(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn saved_reference_survives_restart_without_fingerprinting_assets() {
        let dir = std::env::temp_dir().join(format!("preview-saved-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let key = "a".repeat(64);
        let png = dir.join(format!("{key}.png"));
        fs::write(&png, b"saved preview").unwrap();
        save_preview(&dir, "theme and selection", &key).unwrap();
        let stamp = fs::metadata(saved_file(&dir, "theme and selection")).unwrap().modified().unwrap();
        assert_eq!(read_saved(&dir, "theme and selection"), Some(png.clone()));
        save_preview(&dir, "theme and selection", &key).unwrap();
        assert_eq!(fs::metadata(saved_file(&dir, "theme and selection")).unwrap().modified().unwrap(), stamp);
        fs::remove_file(png).unwrap();
        assert_eq!(read_saved(&dir, "theme and selection"), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pruning_counts_complete_previews_instead_of_sidecars() {
        let dir = std::env::temp_dir().join(format!("preview-prune-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for n in 0..256 {
            for ext in ["png", "json"] {
                fs::write(dir.join(format!("{n:064x}.{ext}")), b"preview").unwrap();
            }
        }
        prune(&dir);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 512, "256 images and metadata must all survive");
        for ext in ["png", "json"] {
            fs::write(dir.join(format!("{:064x}.{ext}", 256)), b"new").unwrap();
        }
        prune(&dir);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 512);
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            assert!(entry.path().with_extension("png").is_file());
            assert!(entry.path().with_extension("json").is_file());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gui_request_never_waits_for_asset_digest_lock() {
        let _guard = DIGESTS.lock().unwrap_or_else(|e| e.into_inner());
        let src = Source { shell_json: "/nonexistent/preview.json".into(), themes_dir: "/nonexistent/themes".into() };
        let start = Instant::now();
        let _ = request(&src, &Selection::default());
        assert!(start.elapsed() < Duration::from_millis(100));
    }
}
