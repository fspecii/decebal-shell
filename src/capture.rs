//! Screenshot capture shared by Spotlight and Control Center.
use crate::launch::Session;
use std::path::{Path, PathBuf};
use std::process::Command;

fn have(name: &str) -> bool {
    crate::controlcenter::sys::have(name)
}

fn maim_args(path: &Path, fullscreen: bool) -> Vec<String> {
    let mut args = vec!["-u".into()];
    if !fullscreen {
        args.push("-s".into());
    }
    args.push(path.to_string_lossy().into_owned());
    args
}

pub fn command() -> Vec<String> {
    vec![crate::launch::sibling_or_path("decutil"), "screenshot".into()]
}

pub fn capture(fullscreen: bool) -> Result<Option<PathBuf>, String> {
    // The search popup must release its keyboard/pointer grab before selection starts.
    for _ in 0..100 {
        if let Some(guard) = crate::single::hold("decspot") {
            drop(guard);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    let dir = std::env::var_os("OMARCHY_SCREENSHOT_DIR").map(PathBuf::from).unwrap_or_else(|| {
        Command::new("xdg-user-dir")
            .arg("PICTURES")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Pictures"))
    });
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create screenshot folder: {e}"))?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let path = dir.join(format!("screenshot-{stamp}.png"));
    let mut cmd;
    match Session::detect() {
        Session::X11 if have("maim") => {
            cmd = Command::new("maim");
            cmd.args(maim_args(&path, fullscreen));
        }
        Session::X11 => {
            for (program, args) in [("flameshot", vec!["gui"]), ("gnome-screenshot", vec!["-i"])] {
                if have(program) {
                    let status = Command::new(program).args(args).status().map_err(|e| e.to_string())?;
                    return if status.success() { Ok(None) } else { Err(format!("{program} failed: {status}")) };
                }
            }
            return Err("No X11 screenshot tool is installed (maim, Flameshot or gnome-screenshot)".into());
        }
        Session::Wayland => {
            if !have("grim") {
                return Err("Wayland screenshots need grim".into());
            }
            cmd = Command::new("grim");
            if !fullscreen {
                if !have("slurp") {
                    return Err("Region selection needs slurp".into());
                }
                let selected = Command::new("slurp").output().map_err(|e| e.to_string())?;
                if !selected.status.success() {
                    return Ok(None);
                }
                let area = String::from_utf8(selected.stdout).map_err(|e| e.to_string())?;
                cmd.args(["-g", area.trim()]);
            }
            cmd.arg(&path);
        }
    }
    let out = cmd.output().map_err(|e| format!("Cannot start capture: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&path);
        let reason = String::from_utf8_lossy(&out.stderr).to_lowercase();
        if !fullscreen && out.status.code() == Some(1) && (reason.contains("cancel") || reason.contains("abort")) {
            return Ok(None);
        }
        return Err(format!("Capture failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    if std::fs::metadata(&path).map_or(true, |m| m.len() == 0) {
        return Err("Capture produced no image".into());
    }
    if Session::detect() == Session::X11 && have("xclip") {
        let _ = Command::new("xclip").args(["-selection", "clipboard", "-t", "image/png", "-i"]).arg(&path).status();
    } else if have("wl-copy") {
        if let Ok(file) = std::fs::File::open(&path) {
            let _ = Command::new("wl-copy").args(["--type", "image/png"]).stdin(file).status();
        }
    }
    if have("tensaku") {
        let copy = if Session::detect() == Session::X11 { "xclip -selection clipboard -t image/png -i" } else { "wl-copy" };
        crate::launch::spawn_argv(&[
            "tensaku".into(),
            "--filename".into(),
            path.to_string_lossy().into_owned(),
            "--output-filename".into(),
            path.to_string_lossy().into_owned(),
            "--actions-on-enter".into(),
            "save-to-clipboard".into(),
            "--save-after-copy".into(),
            "--copy-command".into(),
            copy.into(),
        ]);
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn x11_capture_always_has_a_destination_and_preserves_spaces() {
        assert_eq!(maim_args(Path::new("/tmp/my shots/a.png"), false), ["-u", "-s", "/tmp/my shots/a.png"]);
        assert_eq!(maim_args(Path::new("/tmp/full.png"), true), ["-u", "/tmp/full.png"]);
    }
}
