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
            '"' => { in_q = !in_q; has = true; }
            '\\' if in_q => { if let Some(n) = chars.next() { cur.push(n); } }
            ' ' | '\t' if !in_q => {
                if has || !cur.is_empty() { out.push(std::mem::take(&mut cur)); has = false; }
            }
            _ => cur.push(c),
        }
    }
    if has || !cur.is_empty() { out.push(cur); }
    out.retain(|a| !(a.len() == 2 && a.starts_with('%')));
    out.into_iter().map(|a| a.replace("%%", "%")).collect()
}

/// Starts the app detached from us. Returns false if it could not be found or spawned.
pub fn launch(id: &str) -> bool {
    let argv = match id {
        "trash" => vec!["xdg-open".to_string(), "trash:///".to_string()],
        "launchpad" => match std::env::var("MACSHELL_LAUNCHPAD") {
            Ok(cmd) if !cmd.is_empty() => exec_argv(&cmd),
            _ => return false,
        },
        _ => match find_exec(id) {
            Some(e) => exec_argv(&e),
            None => return false,
        },
    };
    let Some((prog, args)) = argv.split_first() else { return false };
    Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut child| {
            // Reap in the background so finished apps don't linger as zombies.
            std::thread::spawn(move || { let _ = child.wait(); });
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
    fn unknown_app_does_not_launch() {
        assert!(!launch("definitely-not-installed-app"));
    }
}
