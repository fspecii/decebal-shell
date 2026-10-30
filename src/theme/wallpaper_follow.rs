//! Opt-in theme/photo following. The theme transaction queues a detached worker;
//! paint/preview paths never fetch photos or apply wallpaper.
use super::{engine::sha256, live::Source};
use crate::settings::{
    io::{self, Paths},
    wallhaven::{
        client::{self, Client, Progress},
        curated,
        store::{self, Dirs},
        throttle::Throttle,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    io::Read,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
thread_local! {
    // Refresh can apply a newly resolved light/dark variant. That nested colour
    // transaction must not cancel this worker or enqueue another photograph.
    static REFRESHING_COLORS: Cell<bool> = const { Cell::new(false) };
}
struct ColorRefreshScope(bool);
impl ColorRefreshScope {
    fn enter() -> Self {
        Self(REFRESHING_COLORS.with(|flag| flag.replace(true)))
    }
}
impl Drop for ColorRefreshScope {
    fn drop(&mut self) {
        REFRESHING_COLORS.with(|flag| flag.set(self.0));
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Job {
    generation: String,
    theme: String,
    dark: bool,
    selection: Value,
    wallpaper: String,
    runtime: Value,
    shell: PathBuf,
    config: PathBuf,
    ctl: PathBuf,
    socket: PathBuf,
    themes: PathBuf,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub generation: String,
    pub message: String,
    pub pending: bool,
    pub error: bool,
    pub pid: Option<u32>,
}
fn directory(paths: &Paths) -> PathBuf {
    paths.shell_dir.join("wallpaper-follow")
}
pub fn status_path(paths: &Paths) -> PathBuf {
    directory(paths).join("status.json")
}
pub fn enabled(paths: &Paths) -> bool {
    io::json_load(&paths.shell_json())["wallpaper"]["followTheme"].as_bool().unwrap_or(false)
}
pub fn status(paths: &Paths) -> Status {
    read_json(&status_path(paths)).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}
fn read_json(path: &Path) -> Option<Value> {
    let m = fs::symlink_metadata(path).ok()?;
    if !m.is_file() || m.len() > 65536 {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}
fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "Cannot encode wallpaper follow state")?;
    store::write_atomic(path, &bytes, 0o600)
}
fn lock(paths: &Paths) -> Result<File, String> {
    let dir = directory(paths);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return Err("Wallpaper follow state must be a regular directory".into());
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(dir.join("worker.lock")).map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_millis(200);
    while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        if Instant::now() >= until {
            return Err("Wallpaper update is busy; try again shortly".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(file)
}
fn wallpaper(text: &str) -> Result<String, String> {
    let doc = text.parse::<toml_edit::DocumentMut>().map_err(|_| "Wallpaper config is not valid TOML")?;
    Ok(doc.get("wallpaper").map(ToString::to_string).unwrap_or_default())
}
fn runtime(paths: &Paths) -> Value {
    io::ipc_call(&paths.socket, &json!({"cmd":"get_state"})).ok().map(|v| v["state"]["wallpaper"].clone()).unwrap_or(Value::Null)
}
fn selection(paths: &Paths) -> Value {
    let raw = io::json_load(&paths.shell_json());
    let parsed = super::model::Selection::from_json(&raw["theme"]);
    let value = parsed.to_json();
    json!({"name":value["name"],"mode":value["mode"]})
}
fn paths(job: &Job) -> Paths {
    Paths { rdwm_config: job.config.clone(), shell_dir: job.shell.clone(), rdwmctl: job.ctl.clone(), socket: job.socket.clone() }
}
fn token() -> String {
    format!("{}-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(), std::process::id())
}
fn valid_generation(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}
fn cleanup_stale(paths: &Paths) {
    let current = status(paths).generation;
    for entry in fs::read_dir(directory(paths)).into_iter().flatten().flatten().take(512) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let token = name.strip_prefix("download-").or_else(|| name.strip_suffix(".json"));
        if let Some(token) = token.filter(|t| valid_generation(t) && *t != current) {
            let _ = token;
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                let _ = fs::remove_dir_all(entry.path());
            } else if entry.file_type().is_ok_and(|t| t.is_file()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
fn stop_previous(paths: &Paths) {
    let s = status(paths);
    let Some(pid) = s.pid else {
        return;
    };
    if pid == std::process::id() {
        return;
    }
    let args = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let parts: Vec<_> = args.split(|b| *b == 0).collect();
    let expected = directory(paths).join(format!("{}.json", s.generation));
    if parts.iter().any(|p| *p == b"--wallpaper-follow-worker") && parts.iter().any(|p| *p == expected.as_os_str().as_encoded_bytes()) {
        unsafe { libc::kill(if libc::getpgid(pid as i32) == pid as i32 { -(pid as i32) } else { pid as i32 }, libc::SIGTERM) };
        let _ = fs::remove_file(&expected);
        let _ = fs::remove_dir_all(directory(paths).join(format!("download-{}", s.generation)));
    }
}
/// Invalidates a pending photo before a manual wallpaper choice. No preferences
/// are changed; the next full theme change follows again while the switch is on.
pub fn cancel(paths: &Paths) -> Result<(), String> {
    if REFRESHING_COLORS.with(Cell::get) {
        return Ok(());
    }
    if !enabled(paths) && !status(paths).pending {
        return Ok(());
    }
    let _guard = lock(paths)?;
    stop_previous(paths);
    save(&status_path(paths), &Status { generation: token(), message: "Wallpaper request cancelled; current background kept".into(), ..Status::default() })
}
fn set_preference(paths: &Paths, on: bool) -> Result<String, String> {
    let generation = token();
    {
        let _guard = lock(paths)?;
        let mut doc = match fs::read(paths.shell_json()) {
            Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|_| "Shell settings are not valid JSON; nothing changed")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
            Err(_) => return Err("Cannot read shell settings; nothing changed".into()),
        };
        if !doc.is_object() {
            return Err("Shell settings must be a JSON object; nothing changed".into());
        }
        if !doc["wallpaper"].is_null() && !doc["wallpaper"].is_object() {
            return Err("Wallpaper preferences are not a JSON object; nothing changed".into());
        }
        if doc["wallpaper"].is_null() {
            doc["wallpaper"] = json!({});
        }
        stop_previous(paths);
        doc["wallpaper"]["followTheme"] = json!(on);
        io::write_atomic(&paths.shell_json(), &serde_json::to_string_pretty(&doc).map_err(|_| "Cannot encode wallpaper preference")?)?;
        save(
            &status_path(paths),
            &Status { generation: generation.clone(), message: if on { "Preparing the matching theme photo…" } else { "Your wallpaper stays independent" }.into(), pending: on, ..Status::default() },
        )?;
    }
    Ok(generation)
}
fn switch_failure(paths: &Paths, generation: &str, error: &str) {
    if let Ok(_guard) = lock(paths) {
        if status(paths).generation == generation {
            let _ = save(&status_path(paths), &Status { generation: generation.into(), message: format!("Theme applied; wallpaper unchanged: {error}"), error: true, ..Status::default() });
        }
    }
}
fn request_selected(paths: &Paths, expected: &str) -> Result<(), String> {
    let src = Source { shell_json: paths.shell_json(), themes_dir: Source::discover().themes_dir };
    let (theme, problem) = src.build(&src.selection(), super::live::system_dark());
    if let Some(error) = problem {
        return Err(error);
    }
    request_expected(paths, &theme.id, theme.dark, Some(expected))
}
pub fn set_enabled(paths: &Paths, on: bool) -> Result<(), String> {
    let generation = set_preference(paths, on)?;
    if on {
        if let Err(error) = request_selected(paths, &generation) {
            switch_failure(paths, &generation, &error);
            return Err(error);
        }
    }
    Ok(())
}
/// The GUI does only the small local preference write; theme resolution, socket
/// probes and process startup happen off the UI thread. Switch generations keep
/// a delayed On request from reviving a later Off or newer On choice.
pub fn set_enabled_async(paths: &Paths, on: bool) -> Result<(), String> {
    let generation = set_preference(paths, on)?;
    if !on {
        return Ok(());
    }
    let worker_paths = paths.clone();
    let worker_generation = generation.clone();
    if std::thread::Builder::new()
        .name("wallpaper-follow-queue".into())
        .spawn(move || {
            if let Err(error) = request_selected(&worker_paths, &worker_generation) {
                switch_failure(&worker_paths, &worker_generation, &error);
            }
        })
        .is_err()
    {
        switch_failure(paths, &generation, "Wallpaper request could not be queued");
        return Err("Wallpaper request could not be queued".into());
    }
    Ok(())
}

#[cfg(test)]
fn prepare(paths: &Paths, theme: &str, dark: bool) -> Result<Option<(Job, PathBuf)>, String> {
    prepare_expected(paths, theme, dark, None)
}
fn prepare_expected(paths: &Paths, theme: &str, dark: bool, expected: Option<&str>) -> Result<Option<(Job, PathBuf)>, String> {
    if REFRESHING_COLORS.with(Cell::get) {
        return Ok(None);
    }
    if !enabled(paths) || expected.is_some_and(|token| status(paths).generation != token) {
        return Ok(None);
    }
    let source = Source { shell_json: paths.shell_json(), themes_dir: Source::discover().themes_dir };
    let selection = selection(paths);
    let (active, problem) = source.build(&source.selection(), super::live::system_dark());
    if problem.is_some() || active.id != theme || active.dark != dark {
        return Ok(None);
    }
    let cfg = fs::read_to_string(&paths.rdwm_config).map_err(|e| e.to_string())?;
    let baseline = wallpaper(&cfg)?;
    let runtime = runtime(paths);
    let _guard = lock(paths)?;
    if expected.is_some_and(|token| status(paths).generation != token)
        || self::selection(paths) != selection
        || fs::read_to_string(&paths.rdwm_config).ok().and_then(|s| wallpaper(&s).ok()).as_deref() != Some(baseline.as_str())
    {
        return Ok(None);
    }
    stop_previous(paths);
    cleanup_stale(paths);
    let generation = token();
    let job = Job {
        generation: generation.clone(),
        theme: theme.into(),
        dark,
        selection,
        wallpaper: baseline,
        runtime,
        shell: paths.shell_dir.clone(),
        config: paths.rdwm_config.clone(),
        ctl: paths.rdwmctl.clone(),
        socket: paths.socket.clone(),
        themes: source.themes_dir.clone(),
    };
    let path = directory(paths).join(format!("{generation}.json"));
    save(&path, &job)?;
    save(&status_path(paths), &Status { generation, message: "Finding the matching Wallhaven photo…".into(), pending: true, ..Status::default() })?;
    Ok(Some((job, path)))
}
pub fn request(paths: &Paths, theme: &str, dark: bool) -> Result<(), String> {
    request_expected(paths, theme, dark, None)
}
fn request_expected(paths: &Paths, theme: &str, dark: bool, expected: Option<&str>) -> Result<(), String> {
    let Some((job, path)) = prepare_expected(paths, theme, dark, expected)? else {
        return Ok(());
    };
    if cfg!(test) {
        return Ok(());
    }
    let exe = crate::launch::sibling_or_path("decthemectl");
    match Command::new(exe).process_group(0).arg("--wallpaper-follow-worker").arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(child) => {
            let _guard = lock(paths)?;
            let mut state = status(paths);
            if state.generation == job.generation {
                state.pid = Some(child.id());
                save(&status_path(paths), &state)?;
            } else {
                unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            }
            Ok(())
        }
        Err(_) => {
            finish(&job, "Theme applied; wallpaper unchanged: the wallpaper worker could not start", true);
            Err("Theme applied; wallpaper unchanged: the wallpaper worker could not start".into())
        }
    }
}
fn current(job: &Job) -> bool {
    let p = paths(job);
    if !enabled(&p) || (!cfg!(test) && crate::livewall::Config::load().enabled) || status(&p).generation != job.generation || selection(&p) != job.selection {
        return false;
    }
    let source = Source { shell_json: p.shell_json(), themes_dir: job.themes.clone() };
    let (theme, problem) = source.build(&source.selection(), super::live::system_dark());
    problem.is_none()
        && theme.id == job.theme
        && theme.dark == job.dark
        && fs::read_to_string(&job.config).ok().and_then(|s| wallpaper(&s).ok()).as_deref() == Some(job.wallpaper.as_str())
        && runtime(&p) == job.runtime
}

fn finish(job: &Job, message: &str, error: bool) {
    let p = paths(job);
    if let Ok(_guard) = lock(&p) {
        if status(&p).generation == job.generation {
            let _ = save(&status_path(&p), &Status { generation: job.generation.clone(), message: message.chars().take(2048).collect(), error, ..Status::default() });
        }
    }
}
fn image_data(path: &Path) -> Option<(Vec<u8>, [u32; 2])> {
    let mut f = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path).ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file() || m.len() == 0 || m.len() > client::MAX_IMAGE_BYTES {
        return None;
    }
    let mut data = Vec::new();
    f.by_ref().take(client::MAX_IMAGE_BYTES + 1).read_to_end(&mut data).ok()?;
    if data.len() as u64 > client::MAX_IMAGE_BYTES {
        return None;
    }
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&data)).with_guessed_format().ok()?.into_dimensions().ok()?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 120_000_000 {
        return None;
    }
    Some((data, [w, h]))
}
fn image_hash(path: &Path) -> Option<String> {
    image_data(path).map(|(data, _)| sha256::digest_hex(&data))
}

fn obtain(job: &Job, override_client: Option<&Client>) -> Result<(PathBuf, String), String> {
    let p = paths(job);
    let mode = if job.dark { "dark" } else { "light" };
    let choice = curated::choices(&job.theme)?.into_iter().find(|c| c.mode == mode).ok_or("This theme has no curated photograph; choose a wallpaper manually")?;
    let cache = p.backgrounds().join("theme-follow");
    let receipt = cache.join(format!("{}.json", choice.wall.id));
    let expected = read_json(&receipt).and_then(|r| r["sha256"].as_str().map(str::to_owned));
    let mut candidates = vec![cache.clone(), p.backgrounds()];
    if override_client.is_none() && !cfg!(test) {
        candidates.push(crate::wallhaven::wallpaper::download_dir());
        candidates.push(std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Pictures/Wallpapers"));
    }
    for dir in &candidates {
        for ext in ["jpg", "png", "webp"] {
            let file = dir.join(format!("wallhaven-{}.{}", choice.wall.id, ext));
            if let Some(hash) = image_hash(&file) {
                if curated::reviewed_digest(&choice.wall.id).is_some_and(|s| s == hash) || (dir == &cache && expected.as_deref() == Some(hash.as_str())) {
                    return Ok((file, if curated::reviewed_digest(&choice.wall.id) == Some(hash.as_str()) { "reviewed cached original" } else { "previously validated cached download" }.into()));
                }
            }
        }
    }
    let default_client = override_client.is_none().then(|| {
        let dirs = Dirs::discover();
        Client::new(client::api_base(), store::load_key(&dirs), Arc::new(Mutex::new(Throttle::default())))
    });
    let client = override_client.or(default_client.as_ref()).ok_or("Wallpaper client is unavailable")?;
    let (live, detail) = client.detail(&choice.wall.id).map_err(|e| e.friendly())?;
    if live.id != choice.wall.id {
        return Err("Photo metadata did not match the selected recommendation".into());
    }
    curated::validate_live(&live, &detail)?;
    if !current(job) {
        return Err("Wallpaper request superseded; newer choices kept".into());
    }
    // Fresh directory bypasses the legacy client's permissive existing-cache shortcut.
    let staging = directory(&p).join(format!("download-{}", job.generation));
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        let file = client.download(&live, &staging, [true, false, false], &Progress::default()).map_err(|e| e.friendly())?;
        let hash = image_hash(&file).ok_or("Downloaded photo failed bounded image validation")?;
        fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let dest = cache.join(file.file_name().ok_or("Photo filename is missing")?);
        fs::rename(&file, &dest).map_err(|e| e.to_string())?;
        save(&receipt, &json!({"sha256":hash,"id":live.id,"validated_at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()}))?;
        File::open(&cache).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
        Ok((dest, "checked with Wallhaven".into()))
    })();
    let _ = fs::remove_dir_all(staging);
    result
}
fn validate_config(paths: &Paths, text: &str) -> Result<(), String> {
    text.parse::<toml_edit::DocumentMut>().map_err(|_| "Wallpaper config is not valid TOML")?;
    let path = directory(paths).join(format!("check-{}.toml", token()));
    store::write_atomic(&path, text.as_bytes(), 0o600)?;
    let result = (|| {
        let mut child = match Command::new(&paths.rdwmctl).arg("check-config").arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("Could not start wallpaper config validation".into()),
        };
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return if status.success() { Ok(()) } else { Err("rdwm rejected the wallpaper configuration; previous wallpaper kept".into()) },
                Ok(None) => {}
                Err(_) => return Err("Wallpaper config validation failed".into()),
            }
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Wallpaper config validation timed out; previous wallpaper kept".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let _ = fs::remove_file(path);
    result
}

fn apply_photo(job: &Job, file: &Path) -> Result<(), String> {
    let p = paths(job);
    if !current(job) {
        return Err("Wallpaper request superseded; newer choices kept".into());
    }
    if job.runtime.is_null() {
        return Err("No running rdwm wallpaper service; current wallpaper kept".into());
    }
    let old = fs::read_to_string(&job.config).map_err(|e| e.to_string())?;
    let mut doc = old.parse::<toml_edit::DocumentMut>().map_err(|_| "Wallpaper config is not valid TOML")?;
    doc["wallpaper"]["image"] = toml_edit::value(file.to_string_lossy().as_ref());
    doc["wallpaper"]["mode"] = toml_edit::value("fill");
    let new = doc.to_string();
    validate_config(&p, &new)?;
    let _guard = lock(&p)?;
    if !current(job) || fs::read_to_string(&job.config).ok().as_deref() != Some(old.as_str()) {
        return Err("Wallpaper request superseded; newer choices kept".into());
    }
    io::write_atomic(&job.config, &new)?;
    match io::ipc_call(&p.socket, &json!({"cmd":"wallpaper","image":file,"mode":"fill"})) {
        Ok(reply) if reply["ok"] == true => Ok(()),
        _ => {
            if fs::read_to_string(&job.config).ok().as_deref() == Some(new.as_str()) {
                let _ = io::write_atomic(&job.config, &old);
            }
            Err("Wallpaper service did not apply the photo; previous wallpaper kept".into())
        }
    }
}

fn color_refresh_allowed(job: &Job, paths: &Paths, source: &Source) -> bool {
    enabled(paths) && status(paths).generation == job.generation && selection(paths) == job.selection && source.find(&source.selection().name).is_some_and(|(id, _)| id == job.theme)
}

/// Runs after apply_photo has returned and released its flock. Warm the current
/// source first, then reload the selection so palette/radius/source changes made
/// while decoding are preserved by the existing refresh transaction.
fn refresh_photo_colors(job: &Job) -> Result<(), String> {
    let paths = paths(job);
    let source = Source { shell_json: paths.shell_json(), themes_dir: job.themes.clone() };
    for _ in 0..3 {
        if !color_refresh_allowed(job, &paths, &source) {
            return Ok(());
        }
        let current = source.selection();
        let Some(dynamic) = current.dynamic.as_ref().filter(|d| d.enabled) else { return Ok(()) };
        let input = super::dynamic::input_of(dynamic, &paths, None)?;
        super::dynamic::read(&input)?;
        if !color_refresh_allowed(job, &paths, &source) {
            return Ok(());
        }
        let latest = source.selection();
        let Some(dynamic) = latest.dynamic.as_ref().filter(|d| d.enabled) else { return Ok(()) };
        if super::dynamic::input_of(dynamic, &paths, None)? != input {
            continue;
        }
        let _scope = ColorRefreshScope::enter();
        super::dynamic::refresh(&source, &paths, None)?;
        return Ok(());
    }
    Err("Wallpaper colour source changed repeatedly; refresh colours again when it settles".into())
}
pub fn worker(path: &Path) -> Result<(), String> {
    worker_with(path, None)
}
fn worker_with(path: &Path, override_client: Option<&Client>) -> Result<(), String> {
    let job: Job = read_json(path).ok_or("Wallpaper request is missing or invalid").and_then(|v| serde_json::from_value(v).map_err(|_| "Wallpaper request is invalid"))?;
    let p = paths(&job);
    if !valid_generation(&job.generation) {
        return Err("Wallpaper worker generation is invalid".into());
    }
    if path != directory(&p).join(format!("{}.json", job.generation)) {
        return Err("Wallpaper worker request path is invalid".into());
    }
    if !cfg!(test) && crate::livewall::Config::load().enabled {
        finish(&job, "Theme applied; wallpaper unchanged: turn off live wallpapers before following static theme photos", true);
        let _ = fs::remove_file(path);
        return Err("Live wallpaper is active; matching static photo was not applied".into());
    }
    if !current(&job) {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    let result = match obtain(&job, override_client).and_then(|(file, provenance)| apply_photo(&job, &file).map(|()| provenance)) {
        Ok(provenance) => match refresh_photo_colors(&job) {
            Ok(()) => {
                finish(&job, &format!("Matching Wallhaven photo applied ({provenance})"), false);
                Ok(())
            }
            Err(error) => {
                let message = format!("Wallpaper applied; colour refresh failed: {error}");
                finish(&job, &message, true);
                Err(message)
            }
        },
        Err(error) => {
            finish(&job, &format!("Theme applied; wallpaper unchanged: {error}"), true);
            Err(error)
        }
    };
    let _ = fs::remove_file(path);
    result
}

/// Public CLI and the Settings switch use the same opt-in path.
pub fn cli(args: &[String]) -> Result<String, String> {
    let paths = Paths::discover();
    match args.first().map(String::as_str).unwrap_or("status") {
        "on" | "retry" => {
            set_enabled(&paths, true)?;
            Ok("Wallpaper follows theme; matching photo is prepared in the background\n".into())
        }
        "off" => {
            set_enabled(&paths, false)?;
            Ok("Wallpaper stays independent\n".into())
        }
        "status" => {
            let state = status(&paths);
            if args.iter().any(|s| s == "--json") {
                Ok(format!("{}\n", json!({"enabled":enabled(&paths),"status":state})))
            } else {
                Ok(format!("Follow theme wallpaper: {}\n{}\n", if enabled(&paths) { "on" } else { "off" }, state.message))
            }
        }
        _ => Err("Usage: decthemectl wallpaper follow [on|off|retry|status [--json]]".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct ImportRecord {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub id: String,
    pub sha256: Option<String>,
    pub resolution: Option<[u32; 2]>,
    pub action: String,
}
/// Reviewable dry-run by default. Only byte-identical reviewed originals can be
/// copied; unknown/conflicting files are skipped, never rewritten or removed.
pub fn import_reviewed(paths: &Paths, source: &Path, apply: bool) -> Result<Vec<ImportRecord>, String> {
    let reviewed: std::collections::BTreeMap<_, _> = curated::reviewed_originals().into_iter().map(|(id, hash, w, h)| (id, (hash, w, h))).collect();
    let mut files: Vec<_> = fs::read_dir(source).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
    files.sort();
    if files.len() > 256 {
        return Err("Reviewed import accepts at most256 files at a time".into());
    }
    let mut records = Vec::new();
    for file in files {
        let name = file.file_name().and_then(|s| s.to_str()).unwrap_or_default();
        let id = name.strip_prefix("wallhaven-").and_then(|s| s.rsplit_once('.')).map(|(s, _)| s).unwrap_or_default();
        let dest = paths.backgrounds().join(name);
        let mut record = ImportRecord { source: file.clone(), destination: dest.clone(), id: id.into(), sha256: None, resolution: None, action: "skip: not a reviewed original".into() };
        if let Some((expected, w, h)) = reviewed.get(id) {
            if let Some((bytes, dims)) = image_data(&file) {
                let hash = sha256::digest_hex(&bytes);
                record.sha256 = Some(hash.clone());
                record.resolution = Some(dims);
                if hash == *expected && dims == [*w, *h] {
                    record.action = if dest.exists() {
                        if image_hash(&dest).as_deref() == Some(hash.as_str()) {
                            "already cached"
                        } else {
                            "skip: destination differs"
                        }
                    } else if apply {
                        store::write_atomic(&dest, &bytes, 0o600)?;
                        File::open(paths.backgrounds()).and_then(|f| f.sync_all()).map_err(|e| e.to_string())?;
                        "copied reviewed original"
                    } else {
                        "would copy reviewed original"
                    }
                    .into();
                }
            }
        }

        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::{UnixListener, UnixStream},
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        thread,
        time::Duration,
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    pub(super) struct Temp {
        pub(super) root: PathBuf,
        pub(super) paths: Paths,
        pub(super) state: Arc<Mutex<Value>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }
    impl Temp {
        pub(super) fn new() -> Self {
            let root = std::env::temp_dir().join(format!("dfollow-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(&root).unwrap();
            let paths = Paths { shell_dir: root.join("shell"), rdwm_config: root.join("rdwm.toml"), rdwmctl: "/bin/true".into(), socket: root.join("wm.sock") };
            fs::create_dir_all(&paths.shell_dir).unwrap();
            fs::write(paths.shell_json(), r#"{"theme":{"name":"tidal-loom","mode":"dark"},"wallpaper":{"followTheme":true},"unrelated":"kept"}"#).unwrap();
            fs::write(&paths.rdwm_config, "# fixture\n[general]\nborder_px=2\n[wallpaper]\nimage=\"/old.jpg\"\nmode=\"fit\"\ncolor=\"#142527\"\n").unwrap();
            let state = Arc::new(Mutex::new(json!({"image":"/old.jpg","mode":"fit","set":true})));
            let stop = Arc::new(AtomicBool::new(false));
            let listener = UnixListener::bind(&paths.socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            let (st, done) = (state.clone(), stop.clone());
            let handle = thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                            let mut line = String::new();
                            if BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap_or(0) == 0 {
                                continue;
                            }
                            let req: Value = serde_json::from_str(&line).unwrap();
                            let response = if req["cmd"] == "get_state" {
                                json!({"ok":true,"state":{"wallpaper":st.lock().unwrap().clone()}})
                            } else if req["cmd"] == "wallpaper" {
                                let mut v = st.lock().unwrap();
                                v["image"] = req["image"].clone();
                                v["mode"] = req["mode"].clone();
                                json!({"ok":true})
                            } else {
                                json!({"ok":true})
                            };
                            let _ = writeln!(stream, "{response}");
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(2)),
                        Err(_) => break,
                    }
                }
            });
            Self { root, paths, state, stop, thread: Some(handle) }
        }
        pub(super) fn job(&self) -> (Job, PathBuf) {
            let generation = token();
            let job = Job {
                generation: generation.clone(),
                theme: "tidal-loom".into(),
                dark: true,
                selection: selection(&self.paths),
                wallpaper: wallpaper(&fs::read_to_string(&self.paths.rdwm_config).unwrap()).unwrap(),
                runtime: runtime(&self.paths),
                shell: self.paths.shell_dir.clone(),
                config: self.paths.rdwm_config.clone(),
                ctl: self.paths.rdwmctl.clone(),
                socket: self.paths.socket.clone(),
                themes: self.root.join("no-themes"),
            };
            let _guard = lock(&self.paths).unwrap();
            let path = directory(&self.paths).join(format!("{generation}.json"));
            save(&path, &job).unwrap();
            save(&status_path(&self.paths), &Status { generation, message: "Pending".into(), pending: true, ..Status::default() }).unwrap();
            (job, path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = UnixStream::connect(&self.paths.socket);
            if let Some(h) = self.thread.take() {
                h.join().unwrap();
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    pub(super) fn client(base: String) -> Client {
        Client::new(base, None, Arc::new(Mutex::new(Throttle::default())))
    }
    pub(super) fn server(code: u16, ai: bool) -> (String, client::tests::Log) {
        let own = Arc::new(Mutex::new(String::new()));
        let url = own.clone();
        let png = client::tests::png_bytes(8, 6);
        let (base, log) = client::tests::serve(move |path, _| {
            if path.starts_with("/w/") {
                let base = url.lock().unwrap().clone();
                let id = path.rsplit('/').next().unwrap();
                let value = json!({"data":{"id":id,"purity":"sfw","category":"general","dimension_x":8,"dimension_y":6,"file_type":"image/png","path":format!("{base}/full/{id}.png"),"thumbs":{"small":format!("{base}/thumb.png"),"large":format!("{base}/thumb.png")},"ai_art":ai,"tags":[{"id":1,"name":"photography"}]}});
                (code, vec![("Content-Type", "application/json".into())], value.to_string().into_bytes())
            } else {
                (200, vec![("Content-Type", "image/png".into())], png.clone())
            }
        });
        *own.lock().unwrap() = base.clone();
        (base, log)
    }
    #[test]
    fn off_by_default_and_manual_choices_cancel_without_enabling_follow() {
        let t = Temp::new();
        let mut doc = io::json_load(&t.paths.shell_json());
        doc["wallpaper"].as_object_mut().unwrap().remove("followTheme");
        io::write_atomic(&t.paths.shell_json(), &doc.to_string()).unwrap();
        assert!(!enabled(&t.paths));
        assert!(prepare(&t.paths, "tidal-loom", true).unwrap().is_none());
        assert!(!directory(&t.paths).exists());
        assert!(cancel(&t.paths).is_ok());
        assert!(!directory(&t.paths).exists());
    }
    #[test]
    fn a_delayed_gui_on_request_cannot_revive_a_newer_switch_choice() {
        let t = Temp::new();
        let old = set_preference(&t.paths, true).unwrap();
        set_preference(&t.paths, false).unwrap();
        let newest = set_preference(&t.paths, true).unwrap();
        assert!(prepare_expected(&t.paths, "tidal-loom", true, Some(&old)).unwrap().is_none());
        assert_eq!(status(&t.paths).generation, newest);
        assert!(enabled(&t.paths));
        assert_eq!(t.state.lock().unwrap()["image"], "/old.jpg");
    }
    #[test]
    fn corrupt_or_unknown_shell_preferences_are_never_replaced_by_the_switch() {
        let t = Temp::new();
        for bad in ["{ unfinished", r#"{"wallpaper":"custom legacy value","theme":{"name":"tidal-loom"}}"#] {
            fs::write(t.paths.shell_json(), bad).unwrap();
            assert!(set_enabled(&t.paths, true).is_err());
            assert_eq!(fs::read_to_string(t.paths.shell_json()).unwrap(), bad);
            assert_eq!(t.state.lock().unwrap()["image"], "/old.jpg");
        }
    }
    #[test]
    fn loopback_photo_is_validated_applied_persisted_and_reused_without_network() {
        let t = Temp::new();
        let (_, path) = t.job();
        let (base, log) = server(200, false);
        let c = client(base);
        worker_with(&path, Some(&c)).unwrap();
        assert_eq!(log.lock().unwrap().len(), 2);
        let state = status(&t.paths);
        assert!(!state.pending && !state.error);
        assert!(state.message.contains("checked with Wallhaven"));
        let image = t.state.lock().unwrap()["image"].as_str().unwrap().to_string();
        assert!(Path::new(&image).is_file());
        let text = fs::read_to_string(&t.paths.rdwm_config).unwrap();
        assert!(text.contains(&image));
        assert!(text.contains("# fixture") && text.contains("border_px=2") && text.contains("color=\"#142527\""));
        assert_eq!(io::json_load(&t.paths.shell_json())["unrelated"], "kept");
        let (_, next) = t.job();
        worker_with(&next, Some(&c)).unwrap();
        assert_eq!(log.lock().unwrap().len(), 2);
        assert!(status(&t.paths).message.contains("previously validated cached"));
    }
    #[test]
    fn forbidden_or_changed_safety_metadata_keeps_previous_wallpaper_and_warns() {
        for (code, ai) in [(403, false), (200, true)] {
            let t = Temp::new();
            let (_, path) = t.job();
            let before = fs::read(&t.paths.rdwm_config).unwrap();
            let (base, log) = server(code, ai);
            assert!(worker_with(&path, Some(&client(base))).is_err());
            assert_eq!(fs::read(&t.paths.rdwm_config).unwrap(), before);
            assert_eq!(t.state.lock().unwrap()["image"], "/old.jpg");
            let s = status(&t.paths);
            assert!(s.error && !s.pending && s.message.contains("wallpaper unchanged"));
            assert_eq!(log.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn stale_generations_theme_changes_and_manual_wallpaper_never_apply_old_photo() {
        let t = Temp::new();
        let (old, oldpath) = t.job();
        let (_, newpath) = t.job();
        let (base, log) = server(200, false);
        let c = client(base);
        worker_with(&oldpath, Some(&c)).unwrap();
        assert!(log.lock().unwrap().is_empty());
        assert_ne!(status(&t.paths).generation, old.generation);
        let photo = client::tests::png_bytes(8, 6);
        let local = t.root.join("ready.png");
        fs::write(&local, photo).unwrap();
        let current_job: Job = serde_json::from_value(read_json(&newpath).unwrap()).unwrap();
        t.state.lock().unwrap()["image"] = json!("/manual.jpg");
        assert!(apply_photo(&current_job, &local).is_err());
        assert_eq!(t.state.lock().unwrap()["image"], "/manual.jpg");
        let mut doc = io::json_load(&t.paths.shell_json());
        doc["theme"]["name"] = json!("nord");
        io::write_atomic(&t.paths.shell_json(), &doc.to_string()).unwrap();
        assert!(!current(&current_job));
    }
    #[test]
    fn import_dry_run_never_copies_unknown_or_wrong_original_bytes() {
        let t = Temp::new();
        let source = t.root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("wallhaven-m9wr19.png"), client::tests::png_bytes(8, 6)).unwrap();
        fs::write(source.join("wallhaven-unknown.jpg"), b"not an image").unwrap();
        let records = import_reviewed(&t.paths, &source, false).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.action.starts_with("skip:")));
        assert!(!t.paths.backgrounds().exists());
        assert!(!enabled(&Paths { shell_dir: t.root.join("missing"), ..t.paths.clone() }));
    }
    #[test]
    fn generation_lock_is_bounded_and_preview_plan_suppresses_follow() {
        let t = Temp::new();
        let guard = lock(&t.paths).unwrap();
        let before = Instant::now();
        assert!(lock(&t.paths).is_err());
        assert!(before.elapsed() < Duration::from_secs(1));
        drop(guard);
        let src = Source { shell_json: t.paths.shell_json(), themes_dir: t.root.join("no-themes") };
        let before = fs::read_to_string(&t.paths.rdwm_config).unwrap();
        super::super::engine::apply::preview(&src, &t.paths, "nord", 0, &AtomicBool::new(true)).unwrap();
        assert!(!status_path(&t.paths).exists());
        assert_eq!(fs::read_to_string(&t.paths.rdwm_config).unwrap(), before);
        assert!(enabled(&t.paths));
    }
}

#[cfg(test)]
mod late_response_tests {
    use super::*;
    #[test]
    fn metadata_arriving_after_a_newer_request_cannot_download_or_apply() {
        // Use the same private fixture helpers as the main tests, without global env.
        use std::sync::mpsc;
        let t = super::tests::Temp::new();
        let (old, path) = t.job();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let root = Arc::new(Mutex::new(String::new()));
        let root_url = root.clone();
        let (base, log) = client::tests::serve(move |path, _| {
            if path.starts_with("/w/") {
                ready_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                let root = root_url.lock().unwrap().clone();
                (
                    200,
                    vec![("Content-Type", "application/json".into())],
                    json!({"data":{"id":"m9wr19","purity":"sfw","path":format!("{root}/full/photo.png"),"thumbs":{"small":format!("{root}/thumb.png")}}}).to_string().into_bytes(),
                )
            } else {
                (200, vec![("Content-Type", "image/png".into())], client::tests::png_bytes(8, 6))
            }
        });
        *root.lock().unwrap() = base.clone();
        let handle = std::thread::spawn(move || worker_with(&path, Some(&super::tests::client(base))));
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (new, _) = t.job();
        release_tx.send(()).unwrap();
        assert!(handle.join().unwrap().is_err());
        assert_eq!(log.lock().unwrap().len(), 1, "superseded metadata must not start image download");
        assert_eq!(status(&t.paths).generation, new.generation);
        assert_ne!(old.generation, new.generation);
        assert_eq!(t.state.lock().unwrap()["image"], "/old.jpg");
    }
}

#[cfg(test)]
mod color_refresh_tests {
    use super::super::model::{Mode, Origin, Rgb, Selection, Surfaces};
    use super::*;
    fn picture(dir: &Path, name: &str, rgb: [u8; 3]) -> PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_pixel(24, 16, image::Rgb(rgb)).save(&path).unwrap();
        path
    }
    fn setup(t: &super::tests::Temp, origin: Origin, follow_mode: bool) -> (Source, Selection) {
        let old = picture(&t.root, "old-poster.png", [10, 20, 35]);
        t.state.lock().unwrap()["image"] = json!(old);
        let src = Source { shell_json: t.paths.shell_json(), themes_dir: t.root.join("no-themes") };
        let mut sel = src.selection();
        if follow_mode {
            sel.mode = Mode::Auto;
        }
        let mut sel = super::super::dynamic::update_with(&sel, &t.paths, Some(&old), |d| {
            d.enabled = true;
            d.source = origin;
            d.surfaces = Surfaces::Full;
            d.strength = 0.73;
            d.contrast = 0.2;
            d.follow_mode = follow_mode;
        })
        .unwrap();
        sel.palette = Some("nord".into());
        sel.radius = Some(24.);
        sel.opacity = Some(0.82);
        src.write_selection(&sel).unwrap();
        (src, sel)
    }
    fn assert_only_cache_changed(before: &Selection, after: &Selection) {
        let mut expected = before.clone();
        expected.dynamic = after.dynamic.clone();
        assert_eq!(*after, expected);
        let b = before.dynamic.as_ref().unwrap();
        let a = after.dynamic.as_ref().unwrap();
        assert_eq!(
            (a.enabled, a.scheme, a.source.clone(), a.surfaces, a.strength, a.contrast, a.follow_mode),
            (b.enabled, b.scheme, b.source.clone(), b.surfaces, b.strength, b.contrast, b.follow_mode)
        );
    }
    #[test]
    fn worker_refreshes_wallpaper_color_cache_and_preserves_all_matching_preferences() {
        let t = super::tests::Temp::new();
        let (src, before) = setup(&t, Origin::Wallpaper, false);
        let (job, path) = t.job();
        let (base, log) = super::tests::server(200, false);
        worker_with(&path, Some(&super::tests::client(base))).unwrap();
        let after = src.selection();
        assert_only_cache_changed(&before, &after);
        let image = t.state.lock().unwrap()["image"].as_str().unwrap().to_string();
        assert_eq!(after.dynamic.as_ref().unwrap().image.as_deref(), Some(image.as_str()));
        assert_ne!(after.dynamic.as_ref().unwrap().colors.seed, before.dynamic.as_ref().unwrap().colors.seed);
        assert_eq!(status(&t.paths).generation, job.generation);
        assert!(!status(&t.paths).error);
        assert_eq!(log.lock().unwrap().len(), 2);
        let stored = io::json_load(&t.paths.shell_json());
        assert_eq!(stored["theme"]["dynamic"]["image"], image);
    }
    #[test]
    fn brightness_mode_flip_keeps_the_worker_generation_and_does_not_queue_another_photo() {
        let t = super::tests::Temp::new();
        let (src, before) = setup(&t, Origin::Wallpaper, true);
        assert!(src.build(&before, true).0.dark);
        let (job, _) = t.job();
        let bright = picture(&t.root, "bright-followed.png", [245, 248, 255]);
        apply_photo(&job, &bright).unwrap();
        let mut state = status(&t.paths);
        state.pid = Some(std::process::id());
        save(&status_path(&t.paths), &state).unwrap();
        refresh_photo_colors(&job).unwrap();
        let after = src.selection();
        assert!(!src.build(&after, true).0.dark, "the extracted brightness must really cross into light mode");
        assert_only_cache_changed(&before, &after);
        let state_after = status(&t.paths);
        assert_eq!(state_after.generation, job.generation);
        assert_eq!(state_after.pid, Some(std::process::id()), "the nested colour transaction must not cancel its own worker");
        let jobs: Vec<_> = fs::read_dir(directory(&t.paths)).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|e| e == "json") && e.file_name() != "status.json").collect();
        assert_eq!(jobs.len(), 1, "no second wallpaper job should be queued by the mode flip");
    }
    #[test]
    fn fixed_image_color_and_disabled_matching_are_not_retargeted_by_a_photo() {
        for kind in 0..3 {
            let t = super::tests::Temp::new();
            let fixed = picture(&t.root, "fixed.png", [20, 200, 60]);
            let origin = if kind == 0 {
                Origin::File(fixed.to_string_lossy().into_owned())
            } else if kind == 1 {
                Origin::Color(Rgb(180, 30, 70))
            } else {
                Origin::Wallpaper
            };
            let (src, mut before) = setup(&t, origin, false);
            if kind == 2 {
                before.dynamic.as_mut().unwrap().enabled = false;
                src.write_selection(&before).unwrap();
            }
            let (_, path) = t.job();
            let (base, _) = super::tests::server(200, false);
            worker_with(&path, Some(&super::tests::client(base))).unwrap();
            assert_eq!(src.selection(), before);
            assert!(!status(&t.paths).error);
        }
    }
    #[test]
    fn manual_current_wallpaper_and_newer_palette_are_used_instead_of_the_job_photo() {
        let t = super::tests::Temp::new();
        let (src, _) = setup(&t, Origin::Wallpaper, false);
        let (job, _) = t.job();
        let followed = picture(&t.root, "followed.png", [10, 120, 200]);
        apply_photo(&job, &followed).unwrap();
        let manual = picture(&t.root, "newer-manual.png", [20, 210, 60]);
        t.state.lock().unwrap()["image"] = json!(manual);
        let mut latest = src.selection();
        latest.palette = Some("rose-pine".into());
        latest.radius = Some(12.);
        src.write_selection(&latest).unwrap();
        refresh_photo_colors(&job).unwrap();
        let after = src.selection();
        assert_only_cache_changed(&latest, &after);
        assert_eq!(after.dynamic.as_ref().unwrap().image.as_deref(), manual.to_str());
        let mut newer = after;
        newer.name = "windows11".into();
        src.write_selection(&newer).unwrap();
        let unchanged = fs::read(t.paths.shell_json()).unwrap();
        refresh_photo_colors(&job).unwrap();
        assert_eq!(fs::read(t.paths.shell_json()).unwrap(), unchanged);
    }
    #[test]
    fn color_refresh_failure_reports_photo_applied_without_rolling_it_back() {
        let t = super::tests::Temp::new();
        let fixed = picture(&t.root, "missing-later.png", [220, 40, 20]);
        let (src, _) = setup(&t, Origin::File(fixed.to_string_lossy().into_owned()), false);
        let (_, path) = t.job();
        fs::remove_file(fixed).unwrap();
        let (base, _) = super::tests::server(200, false);
        let error = worker_with(&path, Some(&super::tests::client(base))).unwrap_err();
        let state = status(&t.paths);
        assert!(error.starts_with("Wallpaper applied; colour refresh failed:"));
        assert_eq!(state.message, error);
        assert!(state.error && !state.pending);
        assert!(!state.message.contains("wallpaper unchanged"));
        let image = t.state.lock().unwrap()["image"].as_str().unwrap().to_string();
        assert!(image.ends_with("wallhaven-m9wr19.png"));
        assert!(fs::read_to_string(&t.paths.rdwm_config).unwrap().contains(&image));
        assert!(src.selection().dynamic.unwrap().enabled);
    }
}
