//! Compatibility with the names this project had before it became Decebal (it was "macshell", with `mac*` programs).
//!
//! Every `dec*` program calls [`init`] first thing in `main`. It does two things, both safe to repeat:
//!
//! 1. **Environment**: a variable set under an old name (`MACSHELL_CONFIG`, `MACBAR_POWER_SUPPLY`, ...) is copied to its new
//!    name (`DECEBAL_CONFIG`, `DECBAR_POWER_SUPPLY`, ...) unless the new one is set. The rest of the code only knows the new names.
//! 2. **Files**: the first time a program runs and `~/.config/decebal` does not exist yet but `~/.config/macshell` does, the old
//!    folder is *copied* (never moved or deleted) to the new name; the same for `~/.local/state/macshell` and for
//!    `macshell.json` in the shell settings folder (`decebal.json`). The old files stay exactly as they were, so going back to
//!    a `mac*` release needs nothing.
//!
//! The legacy names live only in this file and in the few places that must still recognise data written by the old version
//! (the preset reader, the system-theme block markers, the single-instance locks).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Once;

/// The program names without the `dec` / `mac` prefix; the old `mac<name>` still resolves to `dec<name>`.
pub const PROGRAMS: [&str; 18] = ["bar", "dock", "spot", "launch", "settings", "notify", "themectl", "wm", "hud", "clip", "hot", "dash", "session", "switch", "expose", "util", "haven", "widgets"];

/// True for the name of one of our programs (new or legacy), e.g. `decbar`, `/usr/bin/macspot`.
pub fn is_own_program(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    base.strip_prefix("dec").or_else(|| base.strip_prefix("mac")).is_some_and(|rest| PROGRAMS.contains(&rest))
}

/// The new name of an environment variable that used to be called `old` (`MACSHELL_X` -> `DECEBAL_X`, `MACBAR_X` -> `DECBAR_X`).
pub fn new_env_name(old: &str) -> Option<String> {
    if let Some(rest) = old.strip_prefix("MACSHELL_") {
        return Some(format!("DECEBAL_{rest}"));
    }
    let rest = old.strip_prefix("MAC")?;
    let (prog, tail) = rest.split_once('_')?;
    (prog.chars().all(|c| c.is_ascii_uppercase()) && PROGRAMS.contains(&prog.to_lowercase().as_str())).then(|| format!("DEC{prog}_{tail}"))
}

/// The legacy single-instance lock name for `name` (`decbar-bottom` -> `macbar-bottom`), when `name` is one of ours.
pub fn legacy_lock_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix("dec")?;
    let prog = rest.split('-').next()?;
    PROGRAMS.contains(&prog).then(|| format!("mac{rest}"))
}

/// Copies every legacy-named variable that has no new-named counterpart. Returns the names that were set.
pub fn apply_env_fallbacks() -> Vec<String> {
    let wanted: Vec<(String, OsString)> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let k = k.into_string().ok()?;
            let new = new_env_name(&k)?;
            std::env::var_os(&new).is_none().then_some((new, v))
        })
        .collect();
    let mut set = Vec::new();
    for (k, v) in wanted {
        std::env::set_var(&k, v);
        set.push(k);
    }
    set
}

/// Where the data lives. Built from the environment by [`Dirs::from_env`]; tests build their own.
#[derive(Debug, Clone)]
pub struct Dirs {
    /// `$XDG_CONFIG_HOME` or `~/.config`
    pub config_home: PathBuf,
    /// `$XDG_STATE_HOME` or `~/.local/state`
    pub state_home: PathBuf,
    /// Folder of `decebal.json` / `dock-settings.json` (`$DECEBAL_CONFIG`, else `<config home>/omarchy`)
    pub shell_dir: PathBuf,
}

impl Dirs {
    pub fn from_env() -> Option<Dirs> {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
        let nonempty = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let config_home = nonempty("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")))?;
        let state_home = nonempty("XDG_STATE_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/state")))?;
        Some(Dirs { shell_dir: crate::config::config_dir(), config_home, state_home })
    }
}

/// What [`migrate`] did, for the one-line note a program prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Marker file written into a migrated folder.
pub const MIGRATED_NOTE: &str = "MIGRATED-FROM-MACSHELL.txt";

/// The one-time copy of the old data to the new names. Never deletes or changes anything that exists.
pub fn migrate(d: &Dirs) -> Vec<Copied> {
    let mut done = Vec::new();
    for (old, new) in [(d.config_home.join("macshell"), d.config_home.join("decebal")), (d.state_home.join("macshell"), d.state_home.join("decebal"))] {
        if old.is_dir() && !new.exists() && copy_dir_once(&old, &new) {
            done.push(Copied { from: old, to: new });
        }
    }
    let (old, new) = (d.shell_dir.join("macshell.json"), d.shell_dir.join("decebal.json"));
    if old.is_file() && !new.exists() && copy_file_once(&old, &new) {
        done.push(Copied { from: old, to: new });
    }
    done
}

/// Copies `from` to `to` through a temporary sibling that is renamed into place, so concurrent first runs of several programs
/// cannot see (or build on) a half-copied folder. False when someone else got there first or the copy failed.
fn copy_dir_once(from: &Path, to: &Path) -> bool {
    let Some(parent) = to.parent() else { return false };
    let tmp = parent.join(format!(".decebal-migrating-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let ok = copy_tree(from, &tmp, 0).is_ok()
        && std::fs::write(
            tmp.join(MIGRATED_NOTE),
            format!("This folder was copied from {} by Decebal (the project was called macshell before).\nThe original is untouched; delete it when you no longer need to go back.\n", from.display()),
        )
        .is_ok()
        && std::fs::rename(&tmp, to).is_ok();
    if !ok {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    ok
}

fn copy_tree(from: &Path, to: &Path, depth: usize) -> std::io::Result<()> {
    if depth > 24 {
        return Err(std::io::Error::other("too deep"));
    }
    let md = std::fs::metadata(from)?;
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let ft = e.file_type()?;
        if ft.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
        } else if ft.is_dir() {
            copy_tree(&src, &dst, depth + 1)?;
        } else if ft.is_file() {
            std::fs::copy(&src, &dst)?; // keeps the permission bits (wallhaven.key, clipboard-history are 0600)
        }
    }
    std::fs::set_permissions(to, md.permissions())
}

fn copy_file_once(from: &Path, to: &Path) -> bool {
    let Some(parent) = to.parent() else { return false };
    let tmp = parent.join(format!(".decebal-migrating-{}.json", std::process::id()));
    let linked = std::fs::copy(from, &tmp).is_ok() && std::fs::hard_link(&tmp, to).is_ok(); // fails when the target exists: no clobbering
    let _ = std::fs::remove_file(&tmp);
    linked
}

static INIT: Once = Once::new();

/// Call first thing in `main` (before any thread starts): old environment variables, then the one-time file copy.
pub fn init() {
    INIT.call_once(|| {
        apply_env_fallbacks();
        crate::dlog::init();
        if let Some(d) = Dirs::from_env() {
            for c in migrate(&d) {
                eprintln!("decebal: copied {} to {} (the old one is left untouched)", c.from.display(), c.to.display());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decebal-compat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn env_names_map_to_the_new_prefixes() {
        assert_eq!(new_env_name("MACSHELL_CONFIG").as_deref(), Some("DECEBAL_CONFIG"));
        assert_eq!(new_env_name("MACBAR_POWER_SUPPLY").as_deref(), Some("DECBAR_POWER_SUPPLY"));
        assert_eq!(new_env_name("MACSETTINGS_DEBUG_LAYOUT").as_deref(), Some("DECSETTINGS_DEBUG_LAYOUT"));
        assert_eq!(new_env_name("MACHAVEN_DOWNLOAD_DIR").as_deref(), Some("DECHAVEN_DOWNLOAD_DIR"));
        assert_eq!(new_env_name("MACHINE_ID"), None);
        assert_eq!(new_env_name("MACOS_X"), None);
        assert_eq!(new_env_name("DECEBAL_CONFIG"), None);
        assert_eq!(new_env_name("HOME"), None);
    }

    #[test]
    fn program_and_lock_names() {
        assert!(is_own_program("decbar") && is_own_program("/usr/bin/macspot") && is_own_program("Declaunch"));
        assert!(!is_own_program("machine") && !is_own_program("decoder") && !is_own_program("foot"));
        assert_eq!(legacy_lock_name("decbar-bottom").as_deref(), Some("macbar-bottom"));
        assert_eq!(legacy_lock_name("decclip-daemon").as_deref(), Some("macclip-daemon"));
        assert_eq!(legacy_lock_name("dechot").as_deref(), Some("machot"));
        assert_eq!(legacy_lock_name("decebal-test-1"), None);
        assert_eq!(legacy_lock_name("foot"), None);
    }

    #[test]
    fn old_environment_variables_are_honoured_and_new_ones_win() {
        std::env::set_var("MACHUD_COMPAT_TEST_A", "old");
        std::env::set_var("MACHUD_COMPAT_TEST_B", "old");
        std::env::set_var("DECHUD_COMPAT_TEST_B", "new");
        let set = apply_env_fallbacks();
        assert!(set.contains(&"DECHUD_COMPAT_TEST_A".to_string()) && !set.contains(&"DECHUD_COMPAT_TEST_B".to_string()));
        assert_eq!(std::env::var("DECHUD_COMPAT_TEST_A").as_deref(), Ok("old"));
        assert_eq!(std::env::var("DECHUD_COMPAT_TEST_B").as_deref(), Ok("new"));
        for k in ["MACHUD_COMPAT_TEST_A", "MACHUD_COMPAT_TEST_B", "DECHUD_COMPAT_TEST_A", "DECHUD_COMPAT_TEST_B"] {
            std::env::remove_var(k);
        }
    }

    fn dirs(root: &Path) -> Dirs {
        Dirs { config_home: root.join("cfg"), state_home: root.join("state"), shell_dir: root.join("cfg/omarchy") }
    }

    fn seed(root: &Path) {
        let d = dirs(root);
        std::fs::create_dir_all(d.config_home.join("macshell/presets/mine")).unwrap();
        std::fs::write(d.config_home.join("macshell/presets/mine/manifest.toml"), "name = \"mine\"\n").unwrap();
        std::fs::write(d.config_home.join("macshell/wallhaven.key"), "secret").unwrap();
        std::fs::set_permissions(d.config_home.join("macshell/wallhaven.key"), std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        std::fs::create_dir_all(d.state_home.join("macshell")).unwrap();
        std::fs::write(d.state_home.join("macshell/spotlight-history"), "firefox 3\n").unwrap();
        std::fs::create_dir_all(&d.shell_dir).unwrap();
        std::fs::write(d.shell_dir.join("macshell.json"), r#"{"theme":"nord"}"#).unwrap();
        std::fs::write(d.shell_dir.join("dock-settings.json"), "{}").unwrap();
    }

    #[test]
    fn the_old_folders_are_copied_once_and_left_untouched() {
        let root = tmp("copy");
        seed(&root);
        let d = dirs(&root);
        let done = migrate(&d);
        assert_eq!(done.len(), 3, "{done:?}");
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap();
        assert_eq!(read(d.config_home.join("decebal/presets/mine/manifest.toml")), "name = \"mine\"\n");
        assert_eq!(read(d.state_home.join("decebal/spotlight-history")), "firefox 3\n");
        assert_eq!(read(d.shell_dir.join("decebal.json")), r#"{"theme":"nord"}"#);
        assert!(d.config_home.join("decebal").join(MIGRATED_NOTE).is_file(), "the copy is noted");
        let mode = std::fs::metadata(d.config_home.join("decebal/wallhaven.key")).unwrap();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777, 0o600, "private files stay private");
        // the originals are exactly as they were
        assert_eq!(read(d.config_home.join("macshell/wallhaven.key")), "secret");
        assert_eq!(read(d.shell_dir.join("macshell.json")), r#"{"theme":"nord"}"#);
        assert!(!d.config_home.join("macshell").join(MIGRATED_NOTE).exists());
        // a second run does nothing, and edits made after the copy are not overwritten
        std::fs::write(d.shell_dir.join("decebal.json"), r#"{"theme":"dracula"}"#).unwrap();
        assert!(migrate(&d).is_empty());
        assert_eq!(read(d.shell_dir.join("decebal.json")), r#"{"theme":"dracula"}"#);
        assert!(!std::fs::read_dir(&d.config_home).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("migrating")), "no temp leftovers");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_existing_new_folder_is_never_touched() {
        let root = tmp("keep");
        seed(&root);
        let d = dirs(&root);
        std::fs::create_dir_all(d.config_home.join("decebal")).unwrap();
        std::fs::write(d.config_home.join("decebal/mine.txt"), "new").unwrap();
        std::fs::write(d.shell_dir.join("decebal.json"), "{}").unwrap();
        let done = migrate(&d);
        assert_eq!(done.len(), 1, "only the state folder is copied: {done:?}");
        assert!(!d.config_home.join("decebal/presets").exists());
        assert_eq!(std::fs::read_to_string(d.shell_dir.join("decebal.json")).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_to_migrate_is_fine() {
        let root = tmp("none");
        assert!(migrate(&dirs(&root)).is_empty());
        assert!(!root.join("cfg/decebal").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
