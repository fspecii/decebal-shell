//! Finding, selecting and live-reloading themes.
//!
//! * [`Source`]: where things live (`macshell.json`'s `"theme"` key, the user themes folder).
//! * [`Live`]: the resolved theme plus the file stamps it came from; `poll` re-resolves on change.
//! * Process-wide: [`current`] (lazy), [`start_watcher`] (one thread, 500 ms of `stat`s, wakes the UI),
//!   [`generation`] (bumped on every change; `tick` compares it with the last one drawn).

use super::builtin;
use super::model::{Def, Mode, Selection, Theme};
use crate::config::Stamp;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Source {
    /// `macshell.json` (the file macbar and macdock already read).
    pub shell_json: PathBuf,
    /// `~/.config/macshell/themes`.
    pub themes_dir: PathBuf,
}

/// One theme in the gallery.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub builtin: bool,
    pub has_dark: bool,
    pub has_light: bool,
    /// A user file that does not parse: shown in the list so the mistake is visible, never selectable.
    pub error: Option<String>,
}

impl Source {
    pub fn discover() -> Source {
        let themes_dir = std::env::var_os("MACSHELL_THEMES")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(|x| PathBuf::from(x).join("macshell/themes")))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/macshell/themes")))
            .unwrap_or_else(|| PathBuf::from("themes"));
        Source { shell_json: crate::config::config_dir().join("macshell.json"), themes_dir }
    }

    pub fn selection(&self) -> Selection {
        let doc: serde_json::Value = std::fs::read_to_string(&self.shell_json).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        doc.get("theme").map(Selection::from_json).unwrap_or_default()
    }

    /// Stores the selection under `"theme"` (atomic, every other key of the file is kept).
    pub fn write_selection(&self, sel: &Selection) -> Result<(), String> {
        crate::settings::io::json_set(&self.shell_json, "theme", Some(sel.to_json()))
    }

    fn user_file(&self, id: &str) -> PathBuf {
        self.themes_dir.join(format!("{id}.toml"))
    }

    fn user_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.themes_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension().is_some_and(|x| x == "toml")).then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned())).flatten()
            })
            .collect();
        v.sort();
        v
    }

    /// Loads `id` with its `inherit` chain applied. User files shadow built-ins of the same id.
    pub fn load_def(&self, id: &str) -> Result<Def, String> {
        self.load_chain(id, 0, &mut Vec::new())
    }

    fn load_chain(&self, id: &str, depth: usize, seen: &mut Vec<String>) -> Result<Def, String> {
        if depth > 8 || seen.iter().any(|s| s == id) {
            return Err(format!("inherit loop at `{id}`"));
        }
        seen.push(id.to_string());
        let own = match std::fs::read_to_string(self.user_file(id)) {
            Ok(text) => Def::parse(&text).map_err(|e| format!("{}: {e}", self.user_file(id).display()))?,
            Err(_) => builtin::def(id).ok_or_else(|| format!("no theme `{id}`"))?,
        };
        match own.inherit.clone() {
            None => Ok(own),
            Some(parent) => {
                // a user theme named like its parent (`inherit = "nord"` in nord.toml) extends the built-in one
                let base = if parent == id { builtin::def(id).ok_or_else(|| format!("no theme `{id}`"))? } else { self.load_chain(&parent, depth + 1, seen)? };
                Ok(own.over(&base))
            }
        }
    }

    /// Every theme: built-ins first, then user files, then broken user files.
    pub fn list(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for (id, _) in builtin::BUILTIN {
            // a user file of the same id replaces the built-in entry
            if let Some(e) = self.entry(id, true) {
                out.push(e);
            }
        }
        for id in self.user_ids() {
            if builtin::source(&id).is_some() {
                continue;
            }
            if let Some(e) = self.entry(&id, false) {
                out.push(e);
            }
        }
        out
    }

    fn entry(&self, id: &str, builtin_slot: bool) -> Option<Entry> {
        match self.load_def(id) {
            Ok(d) => Some(Entry { id: id.into(), name: d.name.clone(), description: d.description.clone(), builtin: builtin_slot && !self.user_file(id).exists(), has_dark: d.has_dark(), has_light: d.has_light(), error: None }),
            Err(e) if builtin_slot => {
                // a broken override of a built-in: keep the built-in usable
                let d = builtin::def(id)?;
                Some(Entry { id: id.into(), name: d.name.clone(), description: format!("user file ignored: {e}"), builtin: true, has_dark: d.has_dark(), has_light: d.has_light(), error: Some(e) })
            }
            Err(e) => Some(Entry { id: id.into(), name: id.into(), description: String::new(), builtin: false, has_dark: false, has_light: false, error: Some(e) }),
        }
    }

    /// Maps what the user typed ("Nord", "windows-11-dark", file stem) to (id, implied mode).
    pub fn find(&self, name: &str) -> Option<(String, Option<Mode>)> {
        let entries = self.list();
        let lookup = |n: &str| entries.iter().find(|e| e.id == n || builtin::norm(&e.id) == builtin::norm(n) || builtin::norm(&e.name) == builtin::norm(n)).map(|e| e.id.clone());
        if let Some(id) = lookup(name) {
            return Some((id, None));
        }
        let (base, mode) = builtin::split_mode(name);
        mode.and_then(|m| lookup(&base).map(|id| (id, Some(m))))
    }

    /// Resolves the selection. A missing or broken theme falls back to the default one (with a message).
    pub fn build(&self, sel: &Selection, system_dark: bool) -> (Theme, Option<String>) {
        let (id, implied) = self.find(&sel.name).unwrap_or_else(|| (sel.name.clone(), None));
        let mut sel = sel.clone();
        if let Some(m) = implied {
            sel.mode = m;
        }
        match self.load_def(&id) {
            Ok(def) => {
                let dark = def.pick_dark(sel.mode, system_dark);
                (def.resolve(&id, dark, &sel), None)
            }
            Err(e) => {
                let def = builtin::def(builtin::DEFAULT_ID).expect("default theme");
                let dark = def.pick_dark(sel.mode, system_dark);
                (def.resolve(builtin::DEFAULT_ID, dark, &sel), Some(e))
            }
        }
    }

    /// Everything whose change must trigger a rebuild.
    pub fn stamps(&self, sel: &Selection) -> Vec<Stamp> {
        let (id, _) = self.find(&sel.name).unwrap_or_else(|| (sel.name.clone(), None));
        let mut v = vec![Stamp::of(&self.shell_json), Stamp::of(&self.themes_dir), Stamp::of(&self.user_file(&id))];
        if let Ok(d) = self.load_def(&id) {
            if let Some(p) = d.inherit {
                v.push(Stamp::of(&self.user_file(&p)));
            }
        }
        if sel.mode == Mode::Auto {
            v.extend(system_scheme_files().iter().map(|p| Stamp::of(p)));
        }
        v
    }
}

fn system_scheme_files() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let cfg = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    vec![cfg.join("gtk-3.0/settings.ini"), cfg.join("omarchy/current/theme/light.mode")]
}

/// "auto" mode: `MACSHELL_COLOR_SCHEME`, Omarchy's `light.mode` marker, GTK's dark preference; dark otherwise.
pub fn system_dark() -> bool {
    if let Some(v) = std::env::var_os("MACSHELL_COLOR_SCHEME") {
        return !v.to_string_lossy().eq_ignore_ascii_case("light");
    }
    let files = system_scheme_files();
    if files[1].exists() {
        return false;
    }
    if let Ok(ini) = std::fs::read_to_string(&files[0]) {
        for line in ini.lines() {
            let l = line.replace(' ', "").to_ascii_lowercase();
            if let Some(v) = l.strip_prefix("gtk-application-prefer-dark-theme=") {
                return v == "1" || v == "true";
            }
        }
    }
    true
}

// ---------------------------------------------------------------- live state

pub struct Live {
    pub src: Source,
    sel: Selection,
    stamps: Vec<Stamp>,
    sys_dark: bool,
    pub theme: Arc<Theme>,
    /// Why the selected theme could not be used (shown by `macthemectl get` and the settings pane).
    pub problem: Option<String>,
}

impl Live {
    pub fn new(src: Source) -> Live {
        let sel = src.selection();
        let sys_dark = system_dark();
        let (theme, problem) = src.build(&sel, sys_dark);
        let stamps = src.stamps(&sel);
        Live { src, sel, stamps, sys_dark, theme: Arc::new(theme), problem }
    }

    /// Re-reads everything when a watched file changed; true when the resolved theme is different now.
    pub fn poll(&mut self) -> bool {
        let sel = self.src.selection();
        let stamps = self.src.stamps(&sel);
        let sys = if sel.mode == Mode::Auto { system_dark() } else { self.sys_dark };
        if stamps == self.stamps && sel == self.sel && sys == self.sys_dark {
            return false;
        }
        let (theme, problem) = self.src.build(&sel, sys);
        self.sel = sel;
        self.stamps = stamps;
        self.sys_dark = sys;
        self.problem = problem;
        if *self.theme == theme {
            return false;
        }
        self.theme = Arc::new(theme);
        true
    }
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);
static CURRENT: RwLock<Option<Arc<Theme>>> = RwLock::new(None);
static GEN: AtomicU64 = AtomicU64::new(1);
static SCALE_BITS: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

fn publish(t: Arc<Theme>) {
    SCALE_BITS.store(t.font_scale.to_bits(), Ordering::Relaxed);
    *CURRENT.write().unwrap() = Some(t);
    GEN.fetch_add(1, Ordering::Relaxed);
}

/// The theme every drawing routine uses. Loaded on first use.
pub fn current() -> Arc<Theme> {
    if let Some(t) = CURRENT.read().unwrap().as_ref() {
        return t.clone();
    }
    let mut live = LIVE.lock().unwrap();
    let l = live.get_or_insert_with(|| Live::new(Source::discover()));
    let t = l.theme.clone();
    publish(t.clone());
    t
}

/// Changes whenever [`current`] changed; compare with the value seen at the last draw.
pub fn generation() -> u64 {
    GEN.load(Ordering::Relaxed)
}

/// Text size multiplier of the current theme (read by the font code on every glyph, so it is an atomic).
pub fn font_scale() -> f32 {
    f32::from_bits(SCALE_BITS.load(Ordering::Relaxed))
}

/// Re-checks the files now; true when the theme changed (and was published).
pub fn reload() -> bool {
    let mut live = LIVE.lock().unwrap();
    let l = live.get_or_insert_with(|| Live::new(Source::discover()));
    if l.poll() {
        publish(l.theme.clone());
        true
    } else {
        false
    }
}

/// Starts the 500 ms polling thread; `wake` runs after every change so the event loop redraws.
/// Safe to call once per process; later calls add nothing.
pub fn start_watcher(wake: impl Fn() + Send + 'static) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    let mut wake = Some(wake);
    STARTED.call_once(|| {
        let _ = current();
        let wake = wake.take().unwrap();
        std::thread::Builder::new()
            .name("theme-watch".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(500));
                if reload() {
                    wake();
                }
            })
            .ok();
    });
}

/// For tests and the CLI: the folder the user themes live in.
pub fn themes_dir() -> PathBuf {
    Source::discover().themes_dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::model::Rgb;

    fn sandbox(name: &str) -> (PathBuf, Source) {
        let d = std::env::temp_dir().join(format!("macshell-theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("themes")).unwrap();
        let src = Source { shell_json: d.join("macshell.json"), themes_dir: d.join("themes") };
        (d, src)
    }

    #[test]
    fn nothing_configured_gives_the_default_dark_macos() {
        let (_d, src) = sandbox("default");
        let (t, problem) = src.build(&src.selection(), true);
        assert!(problem.is_none());
        assert_eq!((t.id.as_str(), t.dark), ("macos", true));
        assert_eq!(t.bar, Rgb(19, 19, 19), "keeps the original bar colour");
        assert_eq!(t.accent, Rgb(10, 132, 255));
    }

    #[test]
    fn selection_survives_other_keys_and_round_trips() {
        let (d, src) = sandbox("sel");
        std::fs::write(&src.shell_json, r#"{"workspaces": "off", "theme": "dracula"}"#).unwrap();
        assert_eq!(src.selection().name, "dracula");
        let sel = Selection { name: "nord".into(), mode: Mode::Light, ..Selection::default() };
        src.write_selection(&sel).unwrap();
        assert_eq!(src.selection(), sel);
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.join("macshell.json")).unwrap()).unwrap();
        assert_eq!(doc["workspaces"], "off", "other keys are kept");
    }

    #[test]
    fn user_themes_are_listed_inherit_and_can_shadow() {
        let (_d, src) = sandbox("user");
        std::fs::write(src.themes_dir.join("mine.toml"), "name = \"Mine\"\ninherit = \"nord\"\n[dark]\naccent = \"#ff00ff\"\n").unwrap();
        std::fs::write(src.themes_dir.join("broken.toml"), "name = \"Broken\"\n[dark]\ntext = \"oops\"\n").unwrap();
        let list = src.list();
        assert!(list.iter().any(|e| e.id == "mine" && e.name == "Mine" && e.error.is_none() && !e.builtin));
        assert!(list.iter().any(|e| e.id == "broken" && e.error.is_some()));
        assert_eq!(list.iter().filter(|e| e.builtin).count(), crate::theme::builtin::BUILTIN.len());
        let (t, _) = src.build(&Selection { name: "mine".into(), ..Selection::default() }, true);
        assert_eq!(t.accent, Rgb(255, 0, 255));
        assert_eq!(t.background, Rgb(0x2e, 0x34, 0x40), "the rest comes from nord");
        // a broken selected theme falls back and says why
        let (t, problem) = src.build(&Selection { name: "broken".into(), ..Selection::default() }, true);
        assert_eq!(t.id, "macos");
        assert!(problem.unwrap().contains("not a colour"));
        let (_, problem) = src.build(&Selection { name: "nonexistent".into(), ..Selection::default() }, true);
        assert!(problem.unwrap().contains("no theme"));
    }

    #[test]
    fn names_and_aliases_resolve() {
        let (_d, src) = sandbox("alias");
        assert_eq!(src.find("Nord"), Some(("nord".into(), None)));
        assert_eq!(src.find("Windows 11"), Some(("windows11".into(), None)));
        assert_eq!(src.find("macOS Light"), Some(("macos".into(), Some(Mode::Light))));
        assert_eq!(src.find("windows-11-dark"), Some(("windows11".into(), Some(Mode::Dark))));
        assert_eq!(src.find("high contrast"), Some(("high-contrast".into(), None)));
        assert_eq!(src.find("nope"), None);
        let (t, _) = src.build(&Selection { name: "macOS Light".into(), mode: Mode::Dark, ..Selection::default() }, true);
        assert!(!t.dark, "a name that implies a mode wins");
    }

    #[test]
    fn inherit_loops_are_rejected() {
        let (_d, src) = sandbox("loop");
        std::fs::write(src.themes_dir.join("a.toml"), "name = \"A\"\ninherit = \"b\"\n").unwrap();
        std::fs::write(src.themes_dir.join("b.toml"), "name = \"B\"\ninherit = \"a\"\n").unwrap();
        assert!(src.load_def("a").unwrap_err().contains("loop"));
    }

    #[test]
    fn live_reload_follows_selection_and_theme_file_edits() {
        let (_d, src) = sandbox("live");
        let mut live = Live::new(src.clone());
        assert_eq!(live.theme.id, "macos");
        assert!(!live.poll(), "nothing changed");
        src.write_selection(&Selection { name: "nord".into(), ..Selection::default() }).unwrap();
        assert!(live.poll());
        assert_eq!(live.theme.id, "nord");
        assert!(!live.poll());
        // editing a user theme file that is selected
        std::fs::write(src.themes_dir.join("mine.toml"), "name = \"Mine\"\n[dark]\naccent = \"#010203\"\n").unwrap();
        src.write_selection(&Selection { name: "mine".into(), ..Selection::default() }).unwrap();
        assert!(live.poll());
        assert_eq!(live.theme.accent, Rgb(1, 2, 3));
        std::fs::write(src.themes_dir.join("mine.toml"), "name = \"Mine\"\n[dark]\naccent = \"#040506\"\n").unwrap();
        assert!(live.poll());
        assert_eq!(live.theme.accent, Rgb(4, 5, 6));
        // a half-written (invalid) file keeps the app usable: default theme + a problem, then it recovers
        std::fs::write(src.themes_dir.join("mine.toml"), "name = \"Mine\"\n[dark]\naccent = ").unwrap();
        assert!(live.poll());
        assert!(live.problem.is_some() && live.theme.id == "macos");
        std::fs::write(src.themes_dir.join("mine.toml"), "name = \"Mine\"\n[dark]\naccent = \"#070809\"\n").unwrap();
        assert!(live.poll());
        assert!(live.problem.is_none());
        assert_eq!(live.theme.accent, Rgb(7, 8, 9));
        // an override change alone (accent slider) also reloads
        src.write_selection(&Selection { name: "mine".into(), accent: Rgb::parse("#aabbcc"), ..Selection::default() }).unwrap();
        assert!(live.poll());
        assert_eq!(live.theme.accent, Rgb(0xaa, 0xbb, 0xcc));
    }

    #[test]
    fn auto_mode_follows_the_environment_marker() {
        let (_d, src) = sandbox("auto");
        let sel = Selection { mode: Mode::Auto, ..Selection::default() };
        assert!(src.build(&sel, true).0.dark);
        assert!(!src.build(&sel, false).0.dark);
    }
}
