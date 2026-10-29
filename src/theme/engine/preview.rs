//! Content-addressed, isolated native theme previews. No theme application or live provider is run.
use super::{apply, archive::Limits, package, sha256, writer};
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
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

pub const VERSION: &str = "native-theme-preview-1";
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
static SERIAL: AtomicU64 = AtomicU64::new(0);
static CHANGED: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

#[derive(Clone, Serialize, Deserialize)]
struct Job {
    themes: PathBuf,
    system: PathBuf,
    palettes: PathBuf,
    selection: serde_json::Value,
    output: PathBuf,
    work: PathBuf,
    deco: Option<PathBuf>,
    font_dirs: Vec<PathBuf>,
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
        if let Some(dir) = config::package_dir(&src.themes_dir, &id) {
            let entries = package::read_dir(&dir, &Limits::default())?;
            for entry in entries.iter().filter(|e| {
                e.path == "theme.toml"
                    || e.path.starts_with("widgets/")
                    || e.path.starts_with("wallpapers/")
                    || e.path.starts_with("fonts/")
                    || !(e.path.starts_with("screenshots/") || e.path.ends_with(".md") || e.path.ends_with(".txt"))
            }) {
                hash.update(entry.path.as_bytes());
                hash.update(&entry.data);
            }
        } else if let Ok(text) = fs::read(src.themes_dir.join(format!("{id}.toml"))) {
            hash.update(&text);
        } else if let Some(text) = builtin::source(&id) {
            hash.update(text.as_bytes());
        }
        let own = src.load_def(&id)?;
        current = own.inherit.clone();
    }
    let def = src.load_def(&selection.name)?;
    hash.update(writer::def_to_manifest(&def, &writer::Header { id: selection.name.clone(), name: def.name.clone(), ..Default::default() }).as_bytes());
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
    for path in [Some(renderer()), deco_renderer()].into_iter().flatten() {
        hash.update(path.as_os_str().as_encoded_bytes());
        if let Ok(m) = fs::metadata(path) {
            hash.update(&m.len().to_le_bytes());
            hash.update(format!("{:?}", m.modified()).as_bytes());
        }
    }
    Ok(sha256::hex(&hash.finish()))
}

fn job(src: &Source, selection: &Selection, output: PathBuf) -> Job {
    let work = output.parent().unwrap_or(Path::new(".")).join(format!(".preview-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
    Job {
        themes: src.themes_dir.clone(),
        system: super::store::system_dir(),
        palettes: crate::theme::palette::user_dir(),
        selection: selection.to_json(),
        output,
        work,
        deco: deco_renderer(),
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
        let config = format!("<?xml version=\"1.0\"?><!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\"><fontconfig><include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>{}<cachedir>{}</cachedir></fontconfig>", j.font_dirs.iter().map(|p|format!("<dir>{}</dir>",xml(p))).collect::<String>(),xml(&j.work.join("font-cache")));
        fs::write(j.work.join("fonts.conf"), config).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(renderer());
        cmd.env("FONTCONFIG_FILE", j.work.join("fonts.conf"));
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
            .env("DECEBAL_SYSTEM_THEMES", &j.system)
            .env("DECEBAL_PALETTES", &j.palettes)
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", "")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .env("DECEBAL_SYSTEM_AUTOMATIC", "0")
            .env("DECEBAL_SCALE", "1")
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
        run(&job(src, selection, output.clone()))?;
    }
    prune(&dir);
    Ok(output)
}

fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let stem = p.file_stem()?.to_str()?;
            (stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit()) && matches!(p.extension()?.to_str()?, "png" | "json" | "error"))
                .then(|| (e.metadata().ok().and_then(|m| m.modified().ok()), p))
        })
        .collect();
    files.sort_by_key(|e| e.0);
    let excess = files.len().saturating_sub(256);
    for (_, p) in files.into_iter().take(excess) {
        let _ = fs::remove_file(p);
    }
}

/// Automatic lazy generation on gallery discovery/import/change. One bounded worker, no per-card process storm.
pub fn request(src: &Source, selection: &Selection) -> Result<Option<PathBuf>, String> {
    let key = fingerprint(src, selection)?;
    let dir = cache_dir();
    let output = dir.join(format!("{key}.png"));
    if output.is_file() {
        return Ok(Some(output));
    }
    if dir.join(format!("{key}.error")).is_file() {
        return Err("Native preview unavailable".into());
    }
    static QUEUE: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Job>(32);
        std::thread::spawn(move || {
            for j in rx {
                let key = j.output.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let _ = fs::create_dir_all(j.output.parent().unwrap());
                if let Err(e) = run(&j) {
                    let _ = fs::write(j.output.with_extension("error"), e);
                }
                if let Some(p) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    p.remove(&key);
                }
                prune(j.output.parent().unwrap());
                CHANGED.store(true, Ordering::Release);
            }
        });
        tx
    });
    let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if guard.get_or_insert_with(HashSet::new).insert(key.clone()) && queue.try_send(job(src, selection, output)).is_err() {
        guard.as_mut().unwrap().remove(&key);
    }
    Ok(None)
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
        let mode = src.load_def(&sel.name)?.extra.get("wallpaper").and_then(|w| w.get("mode")).and_then(|s| s.as_str()).unwrap_or("fill").to_string();
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
        notes.push("Wallpaper asset unavailable".to_string());
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
    if shell.bar_visible(crate::config::dock_settings().style) {
        let bar = crate::menubar::DecBar::preview_frame(WIDTH, HEIGHT, false)?;
        composite(&mut canvas, &bar, 0, 0);
        if th.bar_position == crate::config::BarPosition::Split {
            let bar = crate::menubar::DecBar::preview_frame(WIDTH, HEIGHT, true)?;
            composite(&mut canvas, &bar, 0, 0);
        }
    }
    if shell.dock_visible(th.bar_vertical()) {
        let dock = crate::dock_app::DockApp::preview_frame(WIDTH)?;
        let y = if crate::config::dock_settings().windows_position == crate::config::WinPosition::Top {
            0
        } else {
            HEIGHT as i32 - dock.height() as i32 - if crate::config::dock_settings().style == crate::config::DockStyle::Windows { 0 } else { 13 }
        };
        composite(&mut canvas, &dock, (WIDTH as i32 - dock.width() as i32) / 2, y);
    }
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
