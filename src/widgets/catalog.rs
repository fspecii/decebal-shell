//! Shared widget library. Definitions are discoverable regardless of the selected desktop theme.
use super::config::{self, Instance};
use super::{dsl, samples, Trust};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Entry {
    /// Stable control key, never a path or a command.
    pub id: String,
    pub name: String,
    pub description: String,
    pub instance: Instance,
    pub error: Option<String>,
}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn suggested(name: &str, source: Option<String>) -> Instance {
    let (anchor, x, y) = match name.strip_prefix("builtin:").unwrap_or(name) {
        "next-weather" => ("top-right", -40, 310),
        "resource-overview" => ("bottom-right", -40, -40),
        "focus-hub" => ("top-right", -40, 70),
        "next-system" => ("bottom-right", -40, -110),
        "cpu-ram-ring" => ("top-right", -40, 260),
        "now-playing-mini" => ("bottom-right", -40, -110),
        "sysmon-graph" => ("bottom-left", 40, -110),
        "workspace-dots" => ("bottom", 0, -70),
        "battery-pill" => ("top-right", -40, 430),
        _ => ("top-right", -40, 70),
    };
    let mut inst = Instance::desktop(name, anchor, x, y);
    inst.source = source;
    inst
}

fn entry(id: String, instance: Instance, text: Result<String, String>, trust: Trust) -> Entry {
    let name = if id.starts_with("builtin.") { instance.use_.strip_prefix("builtin:").unwrap_or(&instance.use_) } else { &instance.use_ };
    let valid = !name.is_empty() && instance.use_.len() <= 80 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let text = if valid { text } else { Err(format!("invalid widget name `{}`", instance.use_)) };
    match text.and_then(|t| dsl::parse(&t, trust)) {
        Ok(def) => Entry { id, name: if def.name.is_empty() { instance.use_.clone() } else { def.name }, description: def.description, instance, error: None },
        Err(error) => Entry { id, name: instance.use_.clone(), description: "This widget could not be loaded".into(), instance, error: Some(error) },
    }
}

fn widget_files(dir: &Path) -> Vec<std::path::PathBuf> {
    if !std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        return Vec::new();
    }
    let mut files: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
    files.sort();
    files
}

pub fn list(themes_dir: &Path, allow_exec: bool) -> Vec<Entry> {
    list_in(themes_dir, &config::user_dir(), &crate::theme::engine::store::system_dir(), allow_exec)
}

fn list_in(themes: &Path, user: &Path, system: &Path, allow_exec: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    for (id, text) in samples::SAMPLES {
        out.push(entry(format!("builtin.{id}"), suggested(&format!("builtin:{id}"), None), Ok((*text).into()), Trust::Theme));
    }
    for file in widget_files(user) {
        let Some(name) = file.file_stem().and_then(|n| n.to_str()) else { continue };
        out.push(entry(format!("user-{}", hex(name)), suggested(name, None), config::read_regular(&file), if allow_exec { Trust::UserExec } else { Trust::User }));
    }
    let mut packages = BTreeSet::new();
    let mut themed = BTreeMap::new();
    for root in [themes, system] {
        let mut dirs: Vec<_> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            let Some(id) = dir.file_name().and_then(|n| n.to_str()) else { continue };
            if !crate::theme::engine::schema::valid_ident(id) || !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) || !dir.join("theme.toml").is_file() || !packages.insert(id.to_string()) {
                continue;
            }
            for file in widget_files(&dir.join("widgets")) {
                let Some(name) = file.file_stem().and_then(|n| n.to_str()) else { continue };
                let key = format!("theme-{}-{}", hex(id), hex(name));
                themed.insert(key.clone(), entry(key, suggested(name, Some(id.into())), config::read_regular(&file), Trust::Theme));
            }
        }
    }
    out.extend(themed.into_values());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_invalid_names_are_unavailable_even_with_valid_toml() {
        for name in ["my clock", "café", "builtin:clock-card", "", &"x".repeat(81)] {
            let e = entry("test".into(), suggested(name, None), Ok(samples::source("clock-card").unwrap().into()), Trust::Theme);
            assert!(e.error.is_some(), "{name} should be unavailable");
        }
    }

    #[test]
    fn shared_catalog_includes_widgets_from_inactive_theme_packages_and_invalid_user_files() {
        let dir = std::env::temp_dir().join(format!("decebal-widget-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("themes/other/widgets")).unwrap();
        std::fs::create_dir_all(dir.join("user")).unwrap();
        std::fs::write(dir.join("themes/other/theme.toml"), "format = 1\n").unwrap();
        std::fs::write(dir.join("themes/other/widgets/clock.toml"), samples::source("clock-card").unwrap()).unwrap();
        std::fs::write(dir.join("user/broken.toml"), "nonsense = [").unwrap();
        let entries = list_in(&dir.join("themes"), &dir.join("user"), &dir.join("none"), false);
        assert_eq!(entries.len(), samples::SAMPLES.len() + 2);
        assert!(entries.iter().any(|e| e.instance.source.as_deref() == Some("other") && e.error.is_none()));
        assert!(entries.iter().any(|e| e.instance.use_ == "broken" && e.error.is_some()));
        let mut keys = BTreeSet::new();
        assert!(entries.iter().all(|e| keys.insert(&e.id)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
