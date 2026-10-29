//! Video wallpapers: local media, Commons discovery and an optional lightweight player host.
pub mod catalog;
pub mod host;
pub mod pane;

use crate::settings::{
    io::{self, Paths},
    tomlcfg::{Cfg, Val},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub video: PathBuf,
    pub poster: PathBuf,
    pub fps: u32,
    pub pause_fullscreen: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self { enabled: false, video: PathBuf::new(), poster: PathBuf::new(), fps: 24, pause_fullscreen: true }
    }
}
impl Config {
    pub fn load() -> Self {
        std::fs::read(config_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        io::write_atomic(&config_path(), &serde_json::to_string_pretty(self).map_err(|e| e.to_string())?)
    }
}
pub fn config_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("decebal/live-wallpaper.json")
}
pub fn library() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("decebal/live-wallpapers")
}
pub fn cache() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("decebal/live-wallpapers")
}
fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).filter(|x| !x.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback))
}
pub fn supported(p: &Path) -> bool {
    p.extension().and_then(|s| s.to_str()).is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "mp4" | "webm" | "mkv" | "mov" | "ogv"))
}
/// Bounded helper execution. No shell; failed/slow decoders never change the current wallpaper.
fn run_bounded(mut cmd: Command, timeout: Duration) -> Result<(), String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("ffmpeg: {e}"))?;
    let end = Instant::now() + timeout;
    loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            return if s.success() { Ok(()) } else { Err("Could not decode a video frame".into()) };
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Video frame decoding timed out".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
pub fn prepare(video: &Path) -> Result<Config, String> {
    let video = std::fs::canonicalize(video).map_err(|e| e.to_string())?;
    if !supported(&video) || !video.is_file() {
        return Err("Choose an MP4, WebM, MKV, MOV or OGV video file".into());
    }
    if std::fs::metadata(&video).map_err(|e| e.to_string())?.len() > 512 * 1024 * 1024 {
        return Err("Video exceeds the 512 MB limit".into());
    }
    std::fs::create_dir_all(cache()).map_err(|e| e.to_string())?;
    let poster = cache().join(format!("poster-{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    let part = poster.with_extension("part.png");
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-threads", "1", "-protocol_whitelist", "file,pipe", "-i"])
        .arg(&video)
        .args(["-map", "0:v:0", "-frames:v", "1", "-vf", "scale=w='min(1920,iw)':h=-2", "-threads", "1", "-y"])
        .arg(&part);
    if let Err(e) = run_bounded(cmd, Duration::from_secs(20)) {
        let _ = std::fs::remove_file(part);
        return Err(e);
    }
    std::fs::rename(&part, &poster).map_err(|e| e.to_string())?;
    let old = Config::load();
    Ok(Config { enabled: true, video, poster, fps: old.fps.clamp(1, 60), pause_fullscreen: old.pause_fullscreen })
}
/// Persist the static fallback in rdwm before enabling video, so a missing player leaves a real wallpaper.
pub fn activate(cfg: Config, paths: &Paths) -> Result<(), String> {
    let mut rdwm = Cfg::parse(&std::fs::read_to_string(&paths.rdwm_config).unwrap_or_default())?;
    rdwm.set("wallpaper", "image", &Val::Str(cfg.poster.to_string_lossy().into_owned()))?;
    let text = rdwm.text();
    io::validate(&paths.rdwmctl, &paths.rdwm_config, &text)?;
    io::write_atomic(&paths.rdwm_config, &text)?;
    cfg.save()?;
    if paths.socket.exists() {
        io::ipc_call(&paths.socket, &json!({"cmd":"reload"}))?;
    }
    ensure_running()
}
pub fn disable() -> Result<(), String> {
    let mut c = Config::load();
    c.enabled = false;
    c.save()
}
pub fn ensure_running() -> Result<(), String> {
    if cfg!(test) || crate::single::signal("decwall", 0) {
        return Ok(());
    }
    if crate::launch::spawn_argv(&[crate::launch::sibling_or_path("decwall"), "daemon".into()]) {
        Ok(())
    } else {
        Err("Could not start decwall; the still frame remains as wallpaper".into())
    }
}
/// Only visible, non-minimised fullscreen clients on this output suspend its playback.
pub fn covered(state: &Value, output: u64) -> bool {
    let Some(mon) = state["monitors"].as_array().and_then(|ms| ms.iter().find(|m| m["id"].as_u64() == Some(output))) else { return false };
    let view = mon["view"].as_u64().unwrap_or(0);
    state["windows"].as_array().is_some_and(|ws| {
        ws.iter().any(|w| {
            w["mon"].as_u64() == Some(output)
                && w["fullscreen"].as_bool() == Some(true)
                && w["minimized"].as_bool() != Some(true)
                && w["hidden"].as_bool() != Some(true)
                && (w["sticky"].as_bool() == Some(true) || w["tags"].as_u64().unwrap_or(0) & view != 0)
        })
    })
}

pub fn cli(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("set") => {
            let path = args.get(1).ok_or("set needs a local video path")?;
            let c = prepare(Path::new(&crate::settings::panes::expand_tilde(path)))?;
            activate(c, &Paths::discover())?;
            Ok("Live wallpaper enabled".into())
        }
        Some("off") => {
            disable()?;
            Ok("Live wallpaper stopped; still frame kept".into())
        }
        Some("status") => Ok(json!({"config":Config::load(),"runtime": io::json_load(&host::status_path())}).to_string()),
        Some("search") => Ok(serde_json::to_string_pretty(&catalog::search(&args[1..].join(" "), 0)?).map_err(|e| e.to_string())?),
        Some("download") => {
            let id = args.get(1).ok_or("download needs the numeric Commons page id from search")?.parse::<u64>().map_err(|_| "Invalid Commons page id")?;
            let media = catalog::lookup(id)?;
            let path = catalog::download(&media)?;
            Ok(path.display().to_string())
        }
        _ => Err("decwall daemon | set VIDEO | off | status | search WORDS | download COMMONS_ID".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fullscreen_pause_is_per_output_and_respects_workspace_and_minimize() {
        let mut s = json!({"monitors":[{"id":1,"view":1},{"id":2,"view":2}],"windows":[{"mon":1,"tags":1,"fullscreen":true,"minimized":false}]});
        assert!(covered(&s, 1));
        assert!(!covered(&s, 2));
        s["windows"][0]["tags"] = json!(2);
        assert!(!covered(&s, 1));
        s["windows"][0]["sticky"] = json!(true);
        assert!(covered(&s, 1));
        s["windows"][0]["hidden"] = json!(true);
        assert!(!covered(&s, 1));
        s["windows"][0]["hidden"] = json!(false);
        s["windows"][0]["tags"] = json!(1);
        s["windows"][0]["minimized"] = json!(true);
        assert!(!covered(&s, 1));
        s["windows"][0]["minimized"] = json!(false);
        s["windows"][0]["fullscreen"] = json!(false);
        assert!(!covered(&s, 1));
    }
}
