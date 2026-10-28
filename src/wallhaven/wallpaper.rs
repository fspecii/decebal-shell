//! Downloading wallpapers and setting them as the desktop background.
//!
//! Matches the original plugin, which runs the user's `wallhaven --set ID URL` helper (that
//! saves into Omarchy's theme backgrounds and calls `omarchy theme bg set`). When that helper or
//! Omarchy is missing, falls back to downloading into `~/Pictures/Wallpapers/Wallhaven` and
//! calling `swww` / `swaybg` (Wayland) or `feh` (X11).

use super::api::Item;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const MAX_DOWNLOAD: u64 = 100 * 1024 * 1024;

/// Wallhaven ids are short lowercase alphanumerics.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 16 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Only wallhaven's own image host is accepted, like the original helper script.
pub fn valid_url(url: &str) -> bool {
    url.starts_with("https://w.wallhaven.cc/") && !url.contains(char::is_whitespace)
}

/// File extension of `url` (lowercase, at most 4 alphanumerics) or `jpg`.
pub fn extension(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or("");
    match name.rsplit_once('.') {
        Some((_, ext)) if (1..=4).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) => ext.to_ascii_lowercase(),
        _ => "jpg".into(),
    }
}

pub fn file_name(id: &str, url: &str) -> String {
    format!("wallhaven-{id}.{}", extension(url))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// Where downloads go: `$DECHAVEN_DOWNLOAD_DIR` if set, else `~/Pictures/Wallpapers/Wallhaven`.
/// Wallhaven id of the wallpaper currently in use, taken from the file name (`wallhaven-<id>.<ext>`) of the
/// current-background symlink. `DECHAVEN_CURRENT_BG` overrides the path; unknown or non-wallhaven files give `None`.
pub fn current_wallpaper_id() -> Option<String> {
    let path = std::env::var_os("DECHAVEN_CURRENT_BG").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/omarchy/current/background")))?;
    id_from_file_name(path.canonicalize().ok()?.file_name()?.to_str()?)
}

fn id_from_file_name(name: &str) -> Option<String> {
    let id = name.strip_prefix("wallhaven-")?.rsplit_once('.')?.0;
    valid_id(id).then(|| id.to_string())
}

pub fn download_dir() -> PathBuf {
    match std::env::var_os("DECHAVEN_DOWNLOAD_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => home().join("Pictures/Wallpapers/Wallhaven"),
    }
}

/// True when `cmd` is an executable on `PATH`.
pub fn have(cmd: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| std::fs::metadata(dir.join(cmd)).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setter {
    Swww,
    Swaybg,
    Feh,
}

/// Chooses a wallpaper tool for the current session, in order of preference.
pub fn choose_setter(wayland: bool, have: &dyn Fn(&str) -> bool) -> Option<Setter> {
    let order: &[(Setter, &str)] = if wayland { &[(Setter::Swww, "swww"), (Setter::Swaybg, "swaybg"), (Setter::Feh, "feh")] } else { &[(Setter::Feh, "feh")] };
    order.iter().find(|(_, bin)| have(bin)).map(|(s, _)| *s)
}

/// Program and arguments for `setter`.
pub fn setter_command(setter: Setter, path: &Path) -> (&'static str, Vec<String>) {
    let p = path.to_string_lossy().into_owned();
    match setter {
        Setter::Swww => ("swww", vec!["img".into(), p]),
        Setter::Swaybg => ("swaybg", vec!["-i".into(), p, "-m".into(), "fill".into()]),
        Setter::Feh => ("feh", vec!["--bg-fill".into(), p]),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(120)).user_agent("decebal-dechaven/0.1").build()
}

/// Downloads `url` to the Wallhaven folder (once) and returns the file path.
pub fn download(id: &str, url: &str) -> Result<PathBuf, String> {
    if !valid_id(id) || !valid_url(url) {
        return Err(format!("Not a wallhaven image: {url}"));
    }
    let dir = download_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    let target = dir.join(file_name(id, url));
    if std::fs::metadata(&target).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(target);
    }
    let resp = agent().get(url).call().map_err(|_| "Download failed".to_string())?;
    let part = target.with_extension("part");
    let mut out = std::fs::File::create(&part).map_err(|e| format!("Can't write {}: {e}", part.display()))?;
    let copied = std::io::copy(&mut resp.into_reader().take(MAX_DOWNLOAD + 1), &mut out).map_err(|_| "Download failed".to_string());
    drop(out);
    match copied {
        Ok(n) if n > 0 && n <= MAX_DOWNLOAD => {
            std::fs::rename(&part, &target).map_err(|e| format!("Can't save wallpaper: {e}"))?;
            Ok(target)
        }
        _ => {
            let _ = std::fs::remove_file(&part);
            Err("Download failed".into())
        }
    }
}

/// Saves a wallpaper without applying it. Returns a status line.
pub fn save_only(item: &Item) -> Result<String, String> {
    if item.is_local() {
        return Ok(format!("Already saved: {}", item.full));
    }
    download(&item.id, &item.full).map(|p| format!("Saved to {}", p.display()))
}

/// Downloads (if needed) and sets `item` as the wallpaper. Returns a status line.
pub fn apply(item: &Item) -> Result<String, String> {
    if item.is_local() {
        return apply_local(&item.full);
    }
    if !valid_id(&item.id) || !valid_url(&item.full) {
        return Err(format!("Not a wallhaven image: {}", item.full));
    }
    if have("wallhaven") && have("omarchy") {
        let done = run_helper(&["--set", &item.id, &item.full]).map(|_| "Wallpaper set".into());
        if done.is_ok() {
            // the helper saves the image itself: the colours follow whatever the shell now shows as the wallpaper
            crate::theme::dynamic::on_wallpaper_set(None);
        }
        return done;
    }
    let path = download(&item.id, &item.full)?;
    apply_file(&path)
}

pub fn apply_local(path: &str) -> Result<String, String> {
    if have("wallhaven") && have("omarchy") && run_helper(&["--use", path]).is_ok() {
        crate::theme::dynamic::on_wallpaper_set(Some(Path::new(path)));
        return Ok("Wallpaper set".into());
    }
    apply_file(Path::new(path))
}

fn run_helper(args: &[&str]) -> Result<(), String> {
    let out = Command::new("wallhaven").args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| format!("wallhaven: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().last().unwrap_or("wallhaven failed").trim_start_matches("wallhaven: ").to_string())
    }
}

/// Sets an image file as the background with whichever tool this session has.
pub fn apply_file(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("No such wallpaper: {}", path.display()));
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let Some(setter) = choose_setter(wayland, &have) else {
        let hint = if wayland { "swww, swaybg or feh" } else { "feh" };
        return Err(format!("Saved to {}, but no wallpaper tool found (install {hint})", path.display()));
    };
    let (prog, args) = setter_command(setter, path);
    if setter == Setter::Swaybg {
        let old = pids_of("swaybg");
        spawn_detached(prog, &args)?;
        std::thread::sleep(Duration::from_millis(300));
        for pid in old {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
        crate::theme::dynamic::on_wallpaper_set(Some(path));
        return Ok("Wallpaper set".into());
    }
    let status = Command::new(prog).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| format!("{prog}: {e}"))?;
    if status.success() {
        crate::theme::dynamic::on_wallpaper_set(Some(path));
        Ok("Wallpaper set".into())
    } else {
        Err(format!("{prog} failed"))
    }
}

fn pids_of(name: &str) -> Vec<u32> {
    Command::new("pgrep").args(["-x", name]).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()).unwrap_or_default()
}

fn spawn_detached(prog: &str, args: &[String]) -> Result<(), String> {
    let mut child = Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("{prog}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Wallpapers used before, newest first: the helper's history when present, else the
/// download folder by modification time.
pub fn recent_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    if have("wallhaven") {
        if let Ok(out) = Command::new("wallhaven").arg("--recent").stdin(Stdio::null()).stderr(Stdio::null()).output() {
            files = String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect();
        }
    }
    if files.is_empty() {
        files = newest_files(&download_dir());
    }
    files.retain(|f| f.is_file());
    files.truncate(200);
    files
}

fn newest_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let m = e.metadata().ok()?;
            (m.is_file() && is_image_name(&p)).then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), p))
        })
        .collect();
    v.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    v.into_iter().map(|(_, p)| p).collect()
}

fn is_image_name(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("jpg" | "jpeg" | "png" | "webp"))
}

/// Builds a result item for a wallpaper file on disk.
pub fn local_item(path: &Path) -> Item {
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Item { id: stem.clone(), thumb: format!("file:{}", path.display()), full: path.display().to_string(), resolution: stem.trim_start_matches("wallhaven-").to_string(), favorites: None }
}

#[cfg(test)]
mod tests {
    #[test]
    fn current_id_comes_from_file_name() {
        assert_eq!(super::id_from_file_name("wallhaven-eo6e3r.png").as_deref(), Some("eo6e3r"));
        assert_eq!(super::id_from_file_name("holiday.jpg"), None);
        assert_eq!(super::id_from_file_name("wallhaven-.png"), None);
        assert_eq!(super::id_from_file_name("wallhaven-../x.png"), None);
    }

    use super::*;

    #[test]
    fn id_and_url_validation() {
        assert!(valid_id("x8m2yp"));
        assert!(!valid_id(""));
        assert!(!valid_id("../etc"));
        assert!(!valid_id("UPPER"));
        assert!(valid_url("https://w.wallhaven.cc/full/x8/wallhaven-x8m2yp.jpg"));
        assert!(!valid_url("http://w.wallhaven.cc/full/x8/a.jpg"));
        assert!(!valid_url("https://evil.example/w.wallhaven.cc/a.jpg"));
        assert!(!valid_url("https://w.wallhaven.cc/a.jpg; rm -rf"));
    }

    #[test]
    fn extension_and_file_name() {
        assert_eq!(extension("https://w.wallhaven.cc/full/x8/wallhaven-x8m2yp.PNG"), "png");
        assert_eq!(extension("https://w.wallhaven.cc/full/x8/noext"), "jpg");
        assert_eq!(extension("https://w.wallhaven.cc/full/x8/a.toolongext"), "jpg");
        assert_eq!(file_name("abc", "https://w.wallhaven.cc/f/abc.webp"), "wallhaven-abc.webp");
    }

    #[test]
    fn setter_fallback_order() {
        let all = |_: &str| true;
        assert_eq!(choose_setter(true, &all), Some(Setter::Swww));
        let no_swww = |b: &str| b != "swww";
        assert_eq!(choose_setter(true, &no_swww), Some(Setter::Swaybg));
        let only_feh = |b: &str| b == "feh";
        assert_eq!(choose_setter(true, &only_feh), Some(Setter::Feh));
        assert_eq!(choose_setter(false, &all), Some(Setter::Feh), "X11 only uses feh");
        let none = |_: &str| false;
        assert_eq!(choose_setter(true, &none), None);
        assert_eq!(choose_setter(false, &none), None);
    }

    #[test]
    fn setter_commands() {
        let p = Path::new("/tmp/a b.jpg");
        assert_eq!(setter_command(Setter::Swww, p), ("swww", vec!["img".into(), "/tmp/a b.jpg".into()]));
        assert_eq!(setter_command(Setter::Feh, p).1, vec!["--bg-fill", "/tmp/a b.jpg"]);
        assert_eq!(setter_command(Setter::Swaybg, p).1[0], "-i");
    }

    #[test]
    fn local_item_from_path() {
        let it = local_item(Path::new("/x/wallhaven-k3j9.jpg"));
        assert_eq!(it.id, "wallhaven-k3j9");
        assert_eq!(it.resolution, "k3j9");
        assert!(it.is_local());
        assert_eq!(it.thumb, "file:/x/wallhaven-k3j9.jpg");
        assert_eq!(it.favorites, None);
    }

    #[test]
    fn apply_missing_file_is_an_error() {
        assert!(apply_file(Path::new("/definitely/not/here.jpg")).is_err());
    }

    #[test]
    fn rejects_non_wallhaven_downloads() {
        assert!(download("abc", "https://example.com/a.jpg").is_err());
        assert!(download("../x", "https://w.wallhaven.cc/a.jpg").is_err());
    }
}
