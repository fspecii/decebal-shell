//! Checking a configuration file's text and turning every complaint into a message with a line number, so the editor
//! can mark the line. Nothing here writes anything.

use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// `decebal.json`
    Shell,
    /// `dock-settings.json`
    Dock,
    /// `dock-pinned-macos.json`
    Pinned,
    /// rdwm `config.toml`
    Rdwm,
}

impl FileKind {
    pub fn is_json(self) -> bool {
        self != FileKind::Rdwm
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    /// 1-based.
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub msg: String,
}

impl Diag {
    pub fn at(line: usize, col: Option<usize>, msg: impl Into<String>) -> Diag {
        Diag { line: Some(line), col, msg: msg.into() }
    }
    pub fn plain(msg: impl Into<String>) -> Diag {
        Diag { line: None, col: None, msg: msg.into() }
    }
}

/// "Line 3: expected `,`" for the inline note.
pub fn describe(d: &Diag) -> String {
    match (d.line, d.col) {
        (Some(l), Some(c)) => format!("Line {l}, column {c}: {}", d.msg),
        (Some(l), None) => format!("Line {l}: {}", d.msg),
        _ => d.msg.clone(),
    }
}

/// Finds `line N` / `line N, column M` / `line N column M` in a message from a parser or from rdwm.
/// The first line of the remaining text is the message; `config.toml`-style prefixes are dropped.
pub fn map_message(msg: &str) -> Diag {
    let cleaned = msg.trim().trim_start_matches("rdwmctl: ").trim_start_matches("config error: ").trim_start_matches("invalid TOML: ").trim();
    let lower = cleaned.to_ascii_lowercase();
    let mut line = None;
    let mut col = None;
    if let Some(i) = lower.find("line ") {
        let rest = &lower[i + 5..];
        let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = n.parse::<usize>() {
            line = Some(n);
            let after = &rest[n.to_string().len()..];
            if let Some(j) = after.find("column ") {
                let c: String = after[j + 7..].chars().take_while(|c| c.is_ascii_digit()).collect();
                col = c.parse().ok();
            }
        }
    }
    // the human part: for toml_edit's multi-line report take the last non-empty line(s) after the code frame
    let mut lines: Vec<&str> = cleaned.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let text = if lines.len() > 1 && lines[0].to_ascii_lowercase().contains("parse error") {
        lines.retain(|l| !l.starts_with('|') && !l.contains(" | ") && !l.chars().all(|c| c == '^' || c == '|' || c == ' ') && !l.to_ascii_lowercase().contains("parse error"));
        lines.join("; ")
    } else {
        lines.first().map(|s| s.to_string()).unwrap_or_default()
    };
    Diag { line, col, msg: if text.is_empty() { cleaned.to_string() } else { text } }
}

fn line_col_of(text: &str, byte: usize) -> (usize, usize) {
    let b = byte.min(text.len());
    let before = &text[..b];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
    (line, col)
}

/// The 1-based line of the first `"key"` in a JSON text.
pub fn find_key_line(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    text.lines().position(|l| l.contains(&needle)).map(|i| i + 1)
}

fn json_syntax(text: &str) -> Result<Value, Diag> {
    serde_json::from_str::<Value>(text).map_err(|e| {
        let msg = e.to_string();
        // serde_json appends " at line L column C"; the position is carried separately
        let short = msg.split(" at line ").next().unwrap_or(&msg).to_string();
        Diag::at(e.line().max(1), Some(e.column().max(1)), short)
    })
}

fn typed<T: serde::de::DeserializeOwned>(text: &str) -> Option<Diag> {
    serde_json::from_str::<T>(text).err().map(|e| {
        let msg = e.to_string();
        Diag::at(e.line().max(1), Some(e.column().max(1)), msg.split(" at line ").next().unwrap_or(&msg).to_string())
    })
}

/// Syntax and typed checks only: no scratch files, no rdwmctl. Cheap enough to run on every keystroke.
pub fn quick(kind: FileKind, text: &str) -> Vec<Diag> {
    match kind {
        FileKind::Rdwm => match text.parse::<toml_edit::DocumentMut>() {
            Ok(_) => Vec::new(),
            Err(e) => {
                let (line, col) = e.span().map(|s| line_col_of(text, s.start)).unwrap_or((1, 1));
                vec![Diag::at(line, Some(col), e.message().to_string())]
            }
        },
        _ => validate_json(kind, text),
    }
}

/// All problems found in `text` for `kind`. Empty = valid. `target` (the real file) only gives the rdwm check a place
/// for its scratch copy; nothing is written there permanently.
pub fn validate(kind: FileKind, text: &str, rdwmctl: &Path, target: &Path) -> Vec<Diag> {
    match kind {
        FileKind::Rdwm => validate_rdwm(text, rdwmctl, target),
        _ => validate_json(kind, text),
    }
}

fn validate_json(kind: FileKind, text: &str) -> Vec<Diag> {
    let doc = match json_syntax(text) {
        Ok(v) => v,
        Err(d) => return vec![d],
    };
    if !doc.is_object() {
        return vec![Diag::at(1, Some(1), "the file must be one JSON object, { ... }")];
    }
    let mut out = Vec::new();
    let typed_err = match kind {
        FileKind::Shell => typed::<crate::config::ShellSettings>(text),
        FileKind::Dock => typed::<crate::config::DockSettings>(text),
        FileKind::Pinned => typed::<crate::config::Pinned>(text),
        FileKind::Rdwm => None,
    };
    out.extend(typed_err);
    if kind == FileKind::Shell {
        if let Some(hc) = doc.get("hot_corners") {
            match hc.as_object() {
                None => out.push(Diag::at(find_key_line(text, "hot_corners").unwrap_or(1), None, "`hot_corners` must be an object")),
                Some(o) => {
                    for (k, v) in o {
                        let line = find_key_line(text, k).unwrap_or(1);
                        if k == "dwell_ms" {
                            if !v.is_number() {
                                out.push(Diag::at(line, None, "`dwell_ms` must be a number"));
                            }
                        } else if crate::hot::Corner::parse(k).is_none() {
                            out.push(Diag::at(line, None, format!("`{k}` is not a corner (top_left, top_right, bottom_left, bottom_right)")));
                        } else if v.as_str().and_then(crate::hot::Action::parse).is_none() {
                            out.push(Diag::at(line, None, format!("`{k}` has an unknown action; use one of {} or custom:COMMAND", crate::hot::ACTION_NAMES.join(", "))));
                        }
                    }
                }
            }
        }
        if let Some(mode) = doc.get("theme").and_then(|t| t.get("mode")).and_then(Value::as_str) {
            if crate::theme::model::Mode::parse(mode).is_none() {
                out.push(Diag::at(find_key_line(text, "mode").unwrap_or(1), None, "theme mode must be light, dark or auto"));
            }
        }
    }
    if kind == FileKind::Pinned {
        for key in ["pinned", "order"] {
            if let Some(v) = doc.get(key) {
                if !v.as_array().is_some_and(|a| a.iter().all(Value::is_string)) {
                    out.push(Diag::at(find_key_line(text, key).unwrap_or(1), None, format!("`{key}` must be a list of app ids (text)")));
                }
            }
        }
    }
    out
}

fn validate_rdwm(text: &str, rdwmctl: &Path, target: &Path) -> Vec<Diag> {
    if let Err(e) = text.parse::<toml_edit::DocumentMut>() {
        let (line, col) = e.span().map(|s| line_col_of(text, s.start)).unwrap_or((1, 1));
        return vec![Diag::at(line, Some(col), e.message().to_string())];
    }
    match crate::settings::io::validate(rdwmctl, target, text) {
        Ok(()) => Vec::new(),
        Err(e) if e.contains("usage: rdwmctl") => Vec::new(),
        Err(e) => vec![map_message(&e)],
    }
}

/// Checks a custom theme file.
pub fn validate_theme(text: &str) -> Result<(), String> {
    crate::theme::model::Def::parse(text).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rd(text: &str) -> Vec<Diag> {
        validate(FileKind::Rdwm, text, Path::new("/nonexistent/rdwmctl"), Path::new("/tmp/decebal-validate-test/config.toml"))
    }

    #[test]
    fn messages_map_to_lines() {
        let d = map_message("TOML parse error at line 3, column 5\n  |\n3 | foo = \n  |      ^\ninvalid value\n");
        assert_eq!((d.line, d.col), (Some(3), Some(5)));
        assert_eq!(d.msg, "invalid value");
        let d = map_message("rdwmctl: config error: invalid type: string \"x\", expected i64 for key `border_px` in config.toml at line 7 column 12");
        assert_eq!((d.line, d.col), (Some(7), Some(12)));
        assert!(d.msg.contains("border_px"));
        let d = map_message("[[bind]] #2: unknown action \"frobnicate\"");
        assert_eq!(d.line, None);
        assert!(d.msg.contains("frobnicate"));
        let d = map_message("duplicate key at line 9");
        assert_eq!((d.line, d.col), (Some(9), None));
        assert_eq!(describe(&Diag::at(2, Some(3), "bad")), "Line 2, column 3: bad");
        assert_eq!(describe(&Diag::at(2, None, "bad")), "Line 2: bad");
        assert_eq!(describe(&Diag::plain("bad")), "bad");
    }

    #[test]
    fn json_syntax_errors_carry_line_and_column() {
        let d = validate(FileKind::Dock, "{\n  \"a\": 1,\n  \"b\": ,\n}", Path::new("x"), Path::new("y"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, Some(3));
        assert!(d[0].col.is_some() && !d[0].msg.contains("at line"), "{d:?}");
        let d = validate(FileKind::Shell, "[1, 2]", Path::new("x"), Path::new("y"));
        assert!(d[0].msg.contains("object"));
        assert!(validate(FileKind::Shell, "", Path::new("x"), Path::new("y"))[0].line.is_some(), "an empty file is not JSON");
    }

    #[test]
    fn typed_checks_find_the_line() {
        let d = validate(FileKind::Dock, "{\n  \"appSpacing\": \"wide\"\n}", Path::new("x"), Path::new("y"));
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].line, Some(2));
        let text = "{\n  \"hot_corners\": {\n    \"top_left\": \"dashboard\",\n    \"top_right\": \"warp-speed\",\n    \"middle\": \"lock\"\n  },\n  \"theme\": {\"mode\": \"purple\"}\n}";
        let d = validate(FileKind::Shell, text, Path::new("x"), Path::new("y"));
        assert!(d.iter().any(|d| d.line == Some(4) && d.msg.contains("unknown action")), "{d:?}");
        assert!(d.iter().any(|d| d.line == Some(5) && d.msg.contains("not a corner")), "{d:?}");
        assert!(d.iter().any(|d| d.line == Some(7) && d.msg.contains("mode")), "{d:?}");
        assert!(validate(FileKind::Shell, "{\"hot_corners\": {\"top_left\": \"custom:echo hi\", \"dwell_ms\": 200}}", Path::new("x"), Path::new("y")).is_empty());
        let d = validate(FileKind::Pinned, "{\"pinned\": [1]}", Path::new("x"), Path::new("y"));
        assert!(!d.is_empty());
        assert!(validate(FileKind::Pinned, "{\"pinned\": [\"a\"], \"order\": [\"a\"]}", Path::new("x"), Path::new("y")).is_empty());
        assert!(validate(FileKind::Shell, "{}", Path::new("x"), Path::new("y")).is_empty());
    }

    #[test]
    fn toml_errors_carry_line_and_column() {
        let d = rd("[general]\nborder_px = 2\n\n[decoration\nenabled = true\n");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].line, Some(4), "{d:?}");
        assert!(rd("[general]\nborder_px = 2\n").is_empty());
        assert!(rd("").is_empty());
        let d = rd("a = 1\na = 2\n");
        assert_eq!(d[0].line, Some(2), "{d:?}");
    }

    #[test]
    fn rdwmctl_complaints_are_mapped() {
        // a fake rdwmctl that rejects everything with a positioned message
        let dir = std::env::temp_dir().join(format!("decebal-validate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctl = dir.join("rdwmctl");
        std::fs::write(&ctl, "#!/bin/sh\necho 'rdwmctl: config error: unknown key `bogus` in [general] at line 2 column 1' >&2\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let d = validate(FileKind::Rdwm, "[general]\nbogus = 1\n", &ctl, &dir.join("config.toml"));
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].line, d[0].col), (Some(2), Some(1)));
        assert!(d[0].msg.contains("bogus"), "{d:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn themes_are_checked() {
        assert!(validate_theme("name = \"x\"\n").is_ok());
        assert!(validate_theme("name = [").is_err());
    }
}
