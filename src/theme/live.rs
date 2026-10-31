//! Finding, selecting and live-reloading themes.
//!
//! * [`Source`]: where things live (`decebal.json`'s `"theme"` key, the user themes folder).
//! * [`Live`]: the resolved theme plus the file stamps it came from; `poll` re-resolves on change.
//! * Process-wide: [`current`] (lazy), [`start_watcher`] (one thread, 500 ms of `stat`s, wakes the UI),
//!   [`generation`] (bumped on every change; `tick` compares it with the last one drawn).

use super::builtin;
use super::engine;
use super::model::{Def, Mode, Selection, Theme};
use crate::config::Stamp;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// `decebal.json` (the file decbar and decdock already read).
    pub shell_json: PathBuf,
    /// `~/.config/decebal/themes`.
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
        let themes_dir = std::env::var_os("DECEBAL_THEMES")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            // Like config_dir(), unit tests must not discover the developer's installed overrides.
            .or_else(|| cfg!(test).then(|| crate::config::config_dir().join("themes")))
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(|x| PathBuf::from(x).join("decebal/themes")))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/decebal/themes")))
            .unwrap_or_else(|| PathBuf::from("themes"));
        Source { shell_json: crate::config::config_dir().join("decebal.json"), themes_dir }
    }

    pub fn selection(&self) -> Selection {
        let doc: serde_json::Value = std::fs::read_to_string(&self.shell_json).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        doc.get("theme").map(Selection::from_json).unwrap_or_default()
    }

    /// `bar.position` of the same file: where the Caelestia pill bar sits (left when absent or misspelt).
    pub fn bar_position(&self) -> crate::config::BarPosition {
        let doc: serde_json::Value = std::fs::read_to_string(&self.shell_json).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        doc.get("bar").and_then(|b| b.get("position")).and_then(|p| p.as_str()).and_then(crate::config::BarPosition::parse).unwrap_or_default()
    }

    pub fn bar_layout(&self) -> crate::config::BarLayout {
        let doc: serde_json::Value = std::fs::read_to_string(&self.shell_json).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        crate::config::BarLayout::from_json(&doc["bar"])
    }

    /// `control_center` of the same file (every default when absent).
    pub fn control_center(&self) -> crate::controlcenter::settings::Settings {
        let doc: serde_json::Value = std::fs::read_to_string(&self.shell_json).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        doc.get("control_center").map(crate::controlcenter::settings::Settings::from_json).unwrap_or_default()
    }

    /// Stores the selection under `"theme"` (atomic, every other key of the file is kept).
    pub fn write_selection(&self, sel: &Selection) -> Result<(), String> {
        crate::settings::io::json_set(&self.shell_json, "theme", Some(sel.to_json()))
    }

    /// The file a theme id is read from: `<id>/theme.toml` of a package (the user's folder, then each installed root in priority order),
    /// else the flat `<id>.toml`. Nothing installed ever outranks the user's package or flat file of the same id.
    pub(super) fn user_file(&self, id: &str) -> PathBuf {
        let pkg = self.themes_dir.join(id).join("theme.toml");
        if pkg.is_file() {
            return pkg;
        }
        let flat = self.themes_dir.join(format!("{id}.toml"));
        if !flat.exists() {
            if let Some(dir) = engine::store::installed_package(id) {
                return dir.join("theme.toml");
            }
        }
        flat
    }

    fn user_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let roots = engine::store::installed_roots();
        for (n, dir) in std::iter::once(&self.themes_dir).chain(roots.iter()).enumerate() {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                // flat `<id>.toml` files exist only in the user's folder; installed roots hold packages
                let id = if n == 0 && p.extension().is_some_and(|x| x == "toml") {
                    p.file_stem().map(|s| s.to_string_lossy().into_owned())
                } else if p.join("theme.toml").is_file() {
                    p.file_name().map(|s| s.to_string_lossy().into_owned()).filter(|n| !n.starts_with('.'))
                } else {
                    None
                };
                v.extend(id);
            }
        }
        v.sort();
        v.dedup();
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
            Ok(d) => Some(Entry {
                id: id.into(),
                name: d.name.clone(),
                description: d.description.clone(),
                builtin: builtin_slot && !self.user_file(id).exists(),
                has_dark: d.has_dark(),
                has_light: d.has_light(),
                error: None,
            }),
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

    /// The theme `id` resolved for `sel`: variant picked (the wallpaper's brightness decides when "match wallpaper"
    /// follows light/dark), overrides applied, then "match wallpaper" recolours it when it is on.
    pub fn theme_for(&self, id: &str, sel: &Selection, system_dark: bool) -> Result<Theme, String> {
        let def = self.load_def(id)?;
        let mut theme = resolve_with(&def, id, sel, system_dark);
        self.package_logo(id, &mut theme.branding);
        self.package_font_files(id, 0, &mut theme.fonts.files);
        theme.fonts.file_versions = theme.fonts.files.iter().map(|file| super::fontdb::package_file_version(file)).collect();
        theme.fonts.package_faces = super::fontdb::FontDb::read_package_faces_versioned(&theme.fonts.files, &theme.fonts.file_versions);
        Ok(theme)
    }

    /// A `[branding] logo` image becomes its path inside the package (canonical, never outside it); a missing or escaping
    /// file means the neutral logo, never the style's trademark one.
    fn package_logo(&self, id: &str, branding: &mut crate::panel::branding::Branding) {
        use crate::panel::branding::Logo;
        let Logo::File(rel) = &branding.logo else { return };
        let path = self.logo_owner(id, 0);
        let resolved = (|| {
            let path = path.as_ref()?;
            let rel = rel.to_str()?;
            engine::archive::safe_rel_path(rel).ok()?;
            let base = path.parent()?.canonicalize().ok()?;
            let full = base.join(rel).canonicalize().ok()?;
            (full.starts_with(&base) && full.is_file()).then_some(full)
        })();
        branding.logo = resolved.map_or(crate::panel::branding::NEUTRAL, Logo::File);
    }

    /// The package that declared the logo owns its relative asset path, including through inheritance.
    fn logo_owner(&self, id: &str, depth: usize) -> Option<PathBuf> {
        if depth > 8 {
            return None;
        }
        let path = self.user_file(id);
        let own = std::fs::read_to_string(&path).ok().and_then(|s| Def::parse(&s).ok()).or_else(|| builtin::def(id))?;
        if own.extra.get("branding").and_then(|b| b.get("logo")).is_some() {
            return Some(path);
        }
        own.inherit.filter(|parent| parent != id).and_then(|parent| self.logo_owner(&parent, depth + 1))
    }

    /// Package fonts are resolved outside painting and remain private to this process.
    /// Canonical containment rejects symlinks escaping the package root.
    fn package_font_files(&self, id: &str, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 8 || out.len() >= 8 {
            return;
        }
        let path = self.user_file(id);
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let Ok(def) = Def::parse(&text) else { return };
        let Some(base) = path.parent().and_then(|p| p.canonicalize().ok()) else { return };
        if let Some(files) = def.extra.get("font").and_then(|f| f.get("files")).and_then(|f| f.as_array()) {
            for file in files.iter().filter_map(|f| f.as_str()).take(8) {
                if out.len() >= 8 {
                    break;
                }
                let requested = base.join(file);
                if engine::archive::safe_rel_path(file).is_err() {
                    super::fontdb::report_package_reference(&requested, "unsafe relative path");
                    continue;
                }
                let full = match requested.canonicalize() {
                    Ok(path) => path,
                    Err(error) => {
                        super::fontdb::report_package_reference(&requested, &format!("cannot resolve font: {error}"));
                        continue;
                    }
                };
                if !full.starts_with(&base) {
                    super::fontdb::report_package_reference(&requested, "escapes package root");
                    continue;
                }
                // The cache performs bounded regular-file reads and reports size,
                // read and parse failures. Do not silently discard oversize fonts here.
                if !out.contains(&full) {
                    out.push(full);
                }
            }
        }
        if let Some(parent) = def.inherit {
            self.package_font_files(&parent, depth + 1, out);
        }
    }

    /// The theme with what `decebal.json` says about the bar, the panels and the Control Center filled in.
    fn with_shell(&self, t: Theme) -> Theme {
        let doc: serde_json::Value = std::fs::read_to_string(&self.shell_json).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let written = doc.get("bar").and_then(|b| b.get("position")).and_then(|p| p.as_str()).and_then(crate::config::BarPosition::parse);
        let workspaces = doc.get("workspaces").and_then(|w| serde_json::from_value(w.clone()).ok()).unwrap_or(crate::config::Toggle::Word(crate::config::Word::Auto));
        Theme {
            bar_layout: crate::config::BarLayout::from_json(&doc["bar"]),
            bar_position: written.unwrap_or_default(),
            bar_position_written: written,
            panel_user: crate::panel::UserPanel::from_bar_json(&doc["bar"]),
            panel_workspaces: workspaces,
            control_center: doc.get("control_center").map(crate::controlcenter::settings::Settings::from_json).unwrap_or_default(),
            ..t
        }
    }

    /// Resolves the selection. A missing or broken theme falls back to the default one (with a message).
    pub fn build(&self, sel: &Selection, system_dark: bool) -> (Theme, Option<String>) {
        let (id, implied) = self.find(&sel.name).unwrap_or_else(|| (sel.name.clone(), None));
        let mut sel = sel.clone();
        if let Some(m) = implied {
            sel.mode = m;
        }
        match self.theme_for(&id, &sel, system_dark) {
            Ok(t) => (self.with_shell(t), None),
            Err(e) => {
                let def = builtin::def(builtin::DEFAULT_ID).expect("default theme");
                (self.with_shell(resolve_with(&def, builtin::DEFAULT_ID, &sel, system_dark)), Some(e))
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

/// Variant choice, resolution and the optional "match wallpaper" layer, in one place.
fn resolve_with(def: &Def, id: &str, sel: &Selection, system_dark: bool) -> Theme {
    let dyn_on = sel.dynamic.as_ref().filter(|d| d.enabled);
    let mode = match dyn_on {
        Some(d) if d.follow_mode => {
            if d.colors.tone >= super::dynamic::LIGHT_TONE {
                Mode::Light
            } else {
                Mode::Dark
            }
        }
        _ => sel.mode,
    };
    // with a palette the variants are the palette's too: a light-only theme can be dark under a palette that has a dark set
    let laid;
    let def = match sel.palette.as_deref().and_then(super::palette::load) {
        Some(p) => {
            laid = def.with_palette(&p);
            &laid
        }
        None => def,
    };
    let theme = def.resolve(id, def.pick_dark(mode, system_dark), sel);
    match dyn_on {
        Some(d) => super::dynamic::apply_to(theme, d, sel.accent.is_some()),
        None => theme,
    }
}

fn system_scheme_files() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let cfg = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    vec![cfg.join("gtk-3.0/settings.ini"), cfg.join("omarchy/current/theme/light.mode"), cfg.join("omarchy/desktop-styles/appearance")]
}

/// "auto" mode: `DECEBAL_COLOR_SCHEME`, `desktop-style appearance`, Omarchy's `light.mode` marker, GTK's dark preference; dark otherwise.
pub fn system_dark() -> bool {
    if let Some(v) = std::env::var_os("DECEBAL_COLOR_SCHEME") {
        return !v.to_string_lossy().eq_ignore_ascii_case("light");
    }
    let files = system_scheme_files();
    if let Ok(v) = std::fs::read_to_string(&files[2]) {
        return v.trim() != "light"; // `desktop-style appearance`: the file this desktop's toggle writes
    }
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
    /// Why the selected theme could not be used (shown by `decthemectl get` and the settings pane).
    pub problem: Option<String>,
    accessibility: crate::accessibility::Preferences,
}

impl Live {
    pub fn new(src: Source) -> Live {
        Self::with_preferences(src, crate::accessibility::Preferences::from_env())
    }

    pub fn with_preferences(src: Source, accessibility: crate::accessibility::Preferences) -> Live {
        let sel = src.selection();
        let sys_dark = system_dark();
        let (theme, problem) = src.build(&sel, sys_dark);
        let theme = accessibility.apply(theme);
        let stamps = src.stamps(&sel);
        Live { src, sel, stamps, sys_dark, theme: Arc::new(theme), problem, accessibility }
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
        let theme = self.accessibility.apply(theme);
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
    #[cfg(test)]
    if let Some(t) = THREAD_THEME.with(|slot| slot.borrow().clone()) {
        return t;
    }
    if let Some(t) = CURRENT.read().unwrap().as_ref() {
        return t.clone();
    }
    let mut live = LIVE.lock().unwrap();
    let l = live.get_or_insert_with(|| Live::new(Source::discover()));
    let t = l.theme.clone();
    publish(t.clone());
    t
}

#[cfg(test)]
thread_local! {
    static THREAD_THEME: std::cell::RefCell<Option<Arc<Theme>>> = const { std::cell::RefCell::new(None) };
}

/// Test only: `t` is the current theme for the calling test thread alone, so parallel tests that publish other themes cannot
/// disturb it (and it cannot disturb them). Dropping the guard removes the override; nothing global changes.
#[cfg(test)]
pub fn thread_theme(t: Theme) -> ThreadTheme {
    THREAD_THEME.with(|slot| *slot.borrow_mut() = Some(Arc::new(t)));
    ThreadTheme
}

#[cfg(test)]
pub struct ThreadTheme;

#[cfg(test)]
impl Drop for ThreadTheme {
    fn drop(&mut self) {
        THREAD_THEME.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Makes `t` the current theme without touching any file (previews, screenshots, tests).
/// A later [`reload`] with changed files replaces it again.
pub fn publish_theme(t: Theme) {
    publish(Arc::new(t));
}

/// Changes whenever [`current`] changed; compare with the value seen at the last draw.
pub fn generation() -> u64 {
    GEN.load(Ordering::Relaxed)
}

static SEEN: AtomicU64 = AtomicU64::new(0);

/// True once per theme change, for the single UI of a process (call it from `tick`, then redraw).
pub fn take_changed() -> bool {
    SEEN.swap(generation(), Ordering::Relaxed) != generation()
}

/// Text size multiplier of the current theme (read by the font code on every glyph, so it is an atomic).
pub fn font_scale() -> f32 {
    f32::from_bits(SCALE_BITS.load(Ordering::Relaxed))
}

/// Re-checks the files now; true when the theme changed (and was published).
pub fn reload() -> bool {
    let mut live = LIVE.lock().unwrap();
    let l = live.get_or_insert_with(|| Live::new(Source::discover()));
    // icon settings (decebal.json `icons`) ride on the same watcher: a change repaints like a theme change
    let icons = crate::icons::live::poll_now();
    if l.poll() {
        publish(l.theme.clone());
        true
    } else if icons {
        GEN.fetch_add(1, Ordering::Relaxed);
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
        let d = std::env::temp_dir().join(format!("decebal-theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("themes")).unwrap();
        let src = Source { shell_json: d.join("decebal.json"), themes_dir: d.join("themes") };
        (d, src)
    }

    #[test]
    fn a_package_logo_resolves_inside_its_package_and_never_outside() {
        let (dir, src) = sandbox("brand-logo");
        let pkg = dir.join("themes/brandy");
        std::fs::create_dir_all(pkg.join("icons")).unwrap();
        tiny_skia::Pixmap::new(4, 4).unwrap().save_png(pkg.join("icons/logo.png")).unwrap();
        std::fs::write(dir.join("outside.png"), b"x").unwrap();
        let load = |logo: &str| {
            std::fs::write(pkg.join("theme.toml"), format!("format = 1\nid = \"brandy\"\nname = \"Brandy\"\n[branding]\nlogo = \"{logo}\"\ndesktop_label = \"Desktop\"\n")).unwrap();
            src.theme_for("brandy", &Selection::default(), true).map(|t| t.branding)
        };
        let theme = |logo: &str| load(logo).unwrap();
        let b = theme("icons/logo.png");
        assert_eq!(b.logo, crate::panel::branding::Logo::File(pkg.join("icons/logo.png").canonicalize().unwrap()));
        assert_eq!(b.desktop_label("Finder"), "Desktop");
        assert!(load("../../outside.png").unwrap_err().contains("escapes the package"), "a manifest pointing outside its package does not load");
        assert_eq!(theme("icons/missing.png").logo, crate::panel::branding::NEUTRAL, "a missing image is the neutral logo, not the style's");
        assert_eq!(theme("icon:start-here").logo, crate::panel::branding::Logo::Icon("start-here".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_inherited_logo_uses_its_declaring_package() {
        let (dir, src) = sandbox("inherited-brand-logo");
        let parent = dir.join("themes/brand-parent");
        let child = dir.join("themes/brand-child");
        std::fs::create_dir_all(parent.join("icons")).unwrap();
        std::fs::create_dir_all(child.join("icons")).unwrap();
        tiny_skia::Pixmap::new(4, 4).unwrap().save_png(parent.join("icons/logo.png")).unwrap();
        tiny_skia::Pixmap::new(8, 8).unwrap().save_png(child.join("icons/logo.png")).unwrap();
        std::fs::write(parent.join("theme.toml"), "format = 1\nid = \"brand-parent\"\nname = \"Parent\"\n[branding]\nlogo = \"icons/logo.png\"\n").unwrap();
        let write_child = |logo: &str| {
            std::fs::write(child.join("theme.toml"), format!("format = 1\nid = \"brand-child\"\nname = \"Child\"\nextends = \"brand-parent\"\n[branding]\ndesktop_label = \"Child desktop\"\n{logo}"))
                .unwrap()
        };
        write_child("");
        let load = || src.theme_for("brand-child", &Selection::default(), true).unwrap().branding.logo;
        assert_eq!(load(), crate::panel::branding::Logo::File(parent.join("icons/logo.png").canonicalize().unwrap()));
        write_child("logo = \"icons/logo.png\"");
        assert_eq!(load(), crate::panel::branding::Logo::File(child.join("icons/logo.png").canonicalize().unwrap()));
        write_child("logo = \"icons/missing.png\"");
        assert_eq!(load(), crate::panel::branding::NEUTRAL, "an explicit missing child image must not inherit a parent image");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oversized_package_font_reaches_the_diagnostic_cache() {
        let (dir, src) = sandbox("oversize-font-diagnostic");
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(dir);
        let package = src.themes_dir.join("font-limit");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("theme.toml"),
            r#"format = 1
id = "font-limit"
name = "Font limit"
version = "0.2.0"
author = "Test"
license = "MIT"
extends = "macos"
[font]
files = ["oversized.ttf"]
"#,
        )
        .unwrap();
        let file = package.join("oversized.ttf");
        std::fs::File::create(&file).unwrap().set_len(16 * 1024 * 1024 + 1).unwrap();
        let selection = Selection { name: "font-limit".into(), ..Selection::default() };
        let theme = src.theme_for("font-limit", &selection, true).unwrap();
        assert_eq!(theme.fonts.files, vec![file.canonicalize().unwrap()], "Source must not silently discard an oversize reference");
        assert!(theme.fonts.package_faces.is_empty(), "the bounded diagnostic loader rejects it");
        assert_eq!(theme.fonts.file_versions[0].0, 16 * 1024 * 1024 + 1);
    }

    fn installed(root: &std::path::Path, id: &str, extends: Option<&str>, body: &str, logo: Option<u32>, font: Option<&str>) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(dir.join("icons")).unwrap();
        std::fs::create_dir_all(dir.join("fonts")).unwrap();
        let mut text = format!("format = 1\nid = \"{id}\"\nname = \"{id}\"\n");
        text += &extends.map(|p| format!("extends = \"{p}\"\n")).unwrap_or_default();
        text += body;
        if logo.is_some() {
            text += "[branding]\nlogo = \"icons/logo.png\"\n";
        }
        if let Some(font) = font {
            text += &format!("[font]\nfiles = [\"fonts/{font}\"]\n");
            std::fs::write(dir.join("fonts").join(font), format!("not a real font: {id}")).unwrap();
        }
        if let Some(size) = logo {
            tiny_skia::Pixmap::new(size, size).unwrap().save_png(dir.join("icons/logo.png")).unwrap();
        }
        std::fs::write(dir.join("theme.toml"), text).unwrap();
        dir
    }

    #[test]
    fn installed_parent_logo_and_fonts_resolve_across_roots_and_the_user_folder_wins() {
        let (dir, src) = sandbox("installed-roots");
        let (local, usr) = (dir.join("usr-local/decebal/themes"), dir.join("usr/decebal/themes"));
        let _roots = engine::store::test_roots(vec![local.clone(), usr.clone()]);
        // the child (high root) extends a parent that lives in the low root and declares the logo and one font
        let parent = installed(&usr, "base", None, "", Some(4), Some("base.ttf"));
        let child = installed(&local, "kid", Some("base"), "", None, Some("kid.ttf"));
        // a lower-priority package of the child's id must not matter, and neither must a same-id parent in the higher root's shadow
        installed(&usr, "kid", None, "[colors.dark]\naccent = \"#ff0000\"\n", Some(9), None);
        let load = |src: &Source| src.theme_for("kid", &Selection::default(), true).unwrap();
        let t = load(&src);
        assert_eq!(t.branding.logo, crate::panel::branding::Logo::File(parent.join("icons/logo.png").canonicalize().unwrap()), "the logo belongs to the package that declared it");
        assert_eq!(t.fonts.files, vec![child.join("fonts/kid.ttf").canonicalize().unwrap(), parent.join("fonts/base.ttf").canonicalize().unwrap()], "own font first, then the parent's");
        assert_ne!(t.accent, Rgb(255, 0, 0), "the low-priority same-id package is shadowed");
        // a font symlinked out of its installed package stays out
        let outside = dir.join("outside.ttf");
        std::fs::write(&outside, b"x").unwrap();
        std::os::unix::fs::symlink(&outside, parent.join("fonts/escape.ttf")).unwrap();
        std::fs::write(
            parent.join("theme.toml"),
            std::fs::read_to_string(parent.join("theme.toml")).unwrap().replace("files = [\"fonts/base.ttf\"]", "files = [\"fonts/base.ttf\", \"fonts/escape.ttf\"]"),
        )
        .unwrap();
        let t = load(&src);
        assert_eq!(t.fonts.files.len(), 2, "{:?}", t.fonts.files);
        assert!(t.fonts.files.iter().all(|f| f.starts_with(local.canonicalize().unwrap()) || f.starts_with(usr.canonicalize().unwrap())));
        // the user's config folder outranks every installed root, for the parent too
        installed(&src.themes_dir, "base", None, "", Some(8), None);
        let t = load(&src);
        assert_eq!(t.branding.logo, crate::panel::branding::Logo::File(src.themes_dir.join("base/icons/logo.png").canonicalize().unwrap()));
        assert_eq!(t.fonts.files, vec![child.join("fonts/kid.ttf").canonicalize().unwrap()], "the user's parent has no font; the installed one is shadowed whole");
        // the listing shows each id once, built-ins untouched, and nothing was written into an installed root
        let ids: Vec<_> = src.list().into_iter().map(|e| e.id).collect();
        assert_eq!(ids.iter().filter(|i| ["base", "kid"].contains(&i.as_str())).count(), 2);
        assert!(!local.join("base").exists());
        // the watcher's stamps follow the resolved file
        let sel = Selection { name: "kid".into(), ..Selection::default() };
        let before = src.stamps(&sel);
        std::fs::write(child.join("theme.toml"), std::fs::read_to_string(child.join("theme.toml")).unwrap() + "# edit\n").unwrap();
        assert_ne!(src.stamps(&sel), before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn accessibility_is_effective_without_rewriting_theme_or_selection() {
        let (dir, src) = sandbox("accessibility-effective");
        let sel = Selection { name: "nord".into(), font_scale: Some(1.3), ..Selection::default() };
        src.write_selection(&sel).unwrap();
        let saved = std::fs::read(&src.shell_json).unwrap();
        for scale in [0.8, 2.0] {
            let prefs = crate::accessibility::Preferences { font_scale: Some(scale), high_contrast: true, ..Default::default() };
            let live = Live::with_preferences(src.clone(), prefs);
            assert_eq!(live.theme.font_scale, scale);
            assert_eq!(live.theme.id, "nord");
            assert!(super::super::model::contrast(live.theme.text, live.theme.background) >= 7.0);
            assert_eq!(src.build(&sel, true).0.font_scale, 1.3);
            assert_eq!(std::fs::read(&src.shell_json).unwrap(), saved);
        }
        std::fs::remove_dir_all(dir).unwrap();
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
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.join("decebal.json")).unwrap()).unwrap();
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
    fn the_bar_position_reaches_the_theme_and_a_change_is_a_reload() {
        use crate::config::BarPosition as P;
        let (_d, src) = sandbox("position");
        std::fs::write(&src.shell_json, r#"{"theme": "caelestia", "bar": {"enabled": true}}"#).unwrap();
        let mut live = Live::new(src.clone());
        assert_eq!((live.theme.pill_bar(), live.theme.bar_insets()), (Some(P::Left), crate::theme::Insets { left: 52, ..Default::default() }), "left is the default");
        for (word, want, insets) in [
            ("right", P::Right, crate::theme::Insets { right: 52, ..Default::default() }),
            ("top", P::Top, crate::theme::Insets { top: 40, ..Default::default() }),
            ("bottom", P::Bottom, crate::theme::Insets { bottom: 40, ..Default::default() }),
            ("split", P::Split, crate::theme::Insets { top: 40, bottom: 40, ..Default::default() }),
        ] {
            std::fs::write(&src.shell_json, format!(r#"{{"theme": "caelestia", "bar": {{"enabled": true, "position": "{word}"}}}}"#)).unwrap();
            assert!(live.poll(), "{word}: the theme changed, so every program redraws");
            assert_eq!((live.theme.pill_bar(), live.theme.bar_insets()), (Some(want), insets), "{word}");
            assert!(!live.poll());
        }
        // the ordinary menu bar has no pill bar and no insets, whatever the setting says
        std::fs::write(&src.shell_json, r#"{"theme": "nord", "bar": {"position": "split"}}"#).unwrap();
        assert!(live.poll());
        assert_eq!((live.theme.pill_bar(), live.theme.bar_insets()), (None, crate::theme::Insets::default()));
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
