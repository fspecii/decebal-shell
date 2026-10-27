//! Launching applications from freedesktop `.desktop` files, without a shell where possible.

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(h).join("applications"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("applications")));
    dirs
}

/// Path of `<id>.desktop` (also tried lowercase) in the first applications directory that has it.
pub fn find_desktop_file(id: &str) -> Option<PathBuf> {
    if id.is_empty() || id.contains('/') {
        return None;
    }
    for dir in app_dirs() {
        for name in [format!("{id}.desktop"), format!("{}.desktop", id.to_lowercase())] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `StartupWMClass=` of the `[Desktop Entry]` group.
pub fn parse_wm_class(desktop: &str) -> Option<String> {
    let mut in_entry = false;
    for line in desktop.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("StartupWMClass=") {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Desktop-file id (`org.gnome.Nautilus`) for a window's app id / WM_CLASS: a file named like it, else one
/// whose `StartupWMClass` or trailing name component matches. `None` for windows without an installed app.
pub fn resolve_desktop_id(app_id: &str) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    if let Some(p) = find_desktop_file(app_id) {
        return p.file_stem().map(|s| s.to_string_lossy().into_owned());
    }
    let want = app_id.to_lowercase();
    for dir in app_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "desktop")).collect();
        files.sort();
        for path in files {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let by_name = stem.to_lowercase().ends_with(&format!(".{want}"));
            let by_class = std::fs::read_to_string(&path).ok().and_then(|t| parse_wm_class(&t)).is_some_and(|c| c.to_lowercase() == want);
            if by_name || by_class {
                return Some(stem);
            }
        }
    }
    None
}

/// Command that opens a file, folder or URL: `$MACSHELL_OPEN` (tests, custom file managers) or `xdg-open`.
pub fn open_argv(target: &str) -> Vec<String> {
    let mut argv = match std::env::var("MACSHELL_OPEN") {
        Ok(c) if !c.trim().is_empty() => exec_argv(&c),
        _ => vec!["xdg-open".to_string()],
    };
    argv.push(target.to_string());
    argv
}

/// Command that empties the trash: `$MACSHELL_TRASH_EMPTY` or `gio trash --empty`.
pub fn empty_trash_argv() -> Vec<String> {
    match std::env::var("MACSHELL_TRASH_EMPTY") {
        Ok(c) if !c.trim().is_empty() => exec_argv(&c),
        _ => vec!["gio".to_string(), "trash".to_string(), "--empty".to_string()],
    }
}

/// Finds `<id>.desktop` and returns its `Exec=` line.
pub fn find_exec(id: &str) -> Option<String> {
    for dir in app_dirs() {
        for name in [format!("{id}.desktop"), format!("{}.desktop", id.to_lowercase())] {
            if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
                if let Some(e) = parse_exec(&text) {
                    return Some(e);
                }
            }
        }
    }
    None
}

/// Extracts `Exec=` from the `[Desktop Entry]` group only.
pub fn parse_exec(desktop: &str) -> Option<String> {
    let mut in_entry = false;
    for line in desktop.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("Exec=") {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Splits an Exec line into argv, dropping `%f %U ...` field codes and honouring simple quotes.
pub fn exec_argv(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_q, mut has) = (false, false);
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_q = !in_q;
                has = true;
            }
            '\\' if in_q => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !in_q => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|a| !(a.len() == 2 && a.starts_with('%')));
    out.into_iter().map(|a| a.replace("%%", "%")).collect()
}

/// Starts the app detached from us. Returns false if it could not be found or spawned.
pub fn launch(id: &str) -> bool {
    let argv = match id {
        "trash" => open_argv("trash:///"),
        "launchpad" => match std::env::var("MACSHELL_LAUNCHPAD") {
            Ok(cmd) if !cmd.is_empty() => exec_argv(&cmd),
            _ => vec![sibling_or_path("maclaunch")],
        },
        _ => match find_exec(id) {
            Some(e) => exec_argv(&e),
            None => return false,
        },
    };
    spawn_argv(&argv)
}

/// `name` next to our own executable when it is there (an uninstalled build), otherwise the bare name for `PATH`.
pub fn sibling_or_path(name: &str) -> String {
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join(name))).filter(|p| p.is_file()).map_or_else(|| name.to_string(), |p| p.to_string_lossy().into_owned())
}

/// Starts an `Exec=` line detached from us.
pub fn spawn_exec(exec: &str) -> bool {
    spawn_argv(&exec_argv(exec))
}

pub fn spawn_argv(argv: &[String]) -> bool {
    let Some((prog, args)) = argv.split_first() else {
        return false;
    };
    Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut child| {
            // Reap in the background so finished apps don't linger as zombies.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exec_from_entry_group_only() {
        let t = "[Desktop Action x]\nExec=wrong\n[Desktop Entry]\nName=A\nExec=foot --title %U\n";
        assert_eq!(parse_exec(t).as_deref(), Some("foot --title %U"));
    }

    #[test]
    fn strips_field_codes() {
        assert_eq!(exec_argv("nautilus --new-window %U"), vec!["nautilus", "--new-window"]);
    }

    #[test]
    fn keeps_quoted_args() {
        assert_eq!(exec_argv(r#"sh -c "echo hi there""#), vec!["sh", "-c", "echo hi there"]);
    }

    #[test]
    fn literal_percent() {
        assert_eq!(exec_argv("printf 100%%"), vec!["printf", "100%"]);
    }

    #[test]
    fn wm_class_from_desktop_entry() {
        assert_eq!(parse_wm_class("[Desktop Entry]\nName=A\nStartupWMClass=Foo\n").as_deref(), Some("Foo"));
        assert_eq!(parse_wm_class("[Desktop Action x]\nStartupWMClass=No\n"), None);
    }

    #[test]
    fn open_and_trash_commands_can_be_overridden() {
        // Only this test touches these variables.
        std::env::set_var("MACSHELL_OPEN", "my-open --flag");
        assert_eq!(open_argv("/tmp"), vec!["my-open", "--flag", "/tmp"]);
        std::env::remove_var("MACSHELL_OPEN");
        assert_eq!(open_argv("/tmp"), vec!["xdg-open", "/tmp"]);
        std::env::set_var("MACSHELL_TRASH_EMPTY", "true");
        assert_eq!(empty_trash_argv(), vec!["true"]);
        std::env::remove_var("MACSHELL_TRASH_EMPTY");
        assert_eq!(empty_trash_argv(), vec!["gio", "trash", "--empty"]);
    }

    #[test]
    fn path_like_ids_never_resolve() {
        assert_eq!(find_desktop_file("../etc/passwd"), None);
        assert_eq!(resolve_desktop_id(""), None);
    }

    #[test]
    fn unknown_app_does_not_launch() {
        assert!(!launch("definitely-not-installed-app"));
    }
}
