//! The process-wide icon state: the configured theme turned into a lookup chain, kept up to date.
//!
//! * [`State`] (pure apart from reading files) owns the config, the chosen theme and the [`Resolver`] with its path cache;
//!   [`State::poll`] re-reads what changed (macshell.json, GTK / dconf files, the icon directories).
//! * Process-wide: [`lookup`], [`generation`] (bumped whenever icons must be redrawn), [`poll_now`] (called by the
//!   theme watcher, which also wakes the event loop), [`status`].
//!   Programs without that watcher still follow: [`generation`] polls itself at most every 700 ms.

use super::config::{self, IconConfig};
use super::resolve::{self, Found, Resolver, Roots, ThemeInfo, ThemeKind};
use crate::config::Stamp;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What the config and the installed themes add up to.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The installed theme id in use; `None` = nothing configured or detected: hicolor and the fallback only.
    pub active: Option<String>,
    pub problem: Option<String>,
}

/// Pure choice logic. `desktop` is what GSettings / GTK say (see [`config::desktop_choice`]).
pub fn choose(cfg: &IconConfig, themes: &[ThemeInfo], desktop: Option<&str>) -> Choice {
    let mut problem = None;
    if !cfg.is_auto() {
        match resolve::find_installed(themes, &cfg.theme) {
            Some(id) if themes.iter().any(|t| t.id == id && t.kind.selectable()) => return Choice { active: Some(id), problem: None },
            Some(id) => {
                let kind = themes.iter().find(|t| t.id == id).map(|t| t.kind).unwrap_or(ThemeKind::Empty);
                problem = Some(format!("icon theme `{}` has no application icons ({}); following the desktop instead", cfg.theme, kind.label()));
            }
            None => problem = Some(format!("icon theme `{}` is not installed; following the desktop instead", cfg.theme)),
        }
    }
    Choice { active: desktop.and_then(|d| resolve::find_installed(themes, d)), problem }
}

/// The icon base directories plus, per theme of the chain, its folder and `icon-theme.cache` (what package installs
/// and `gtk-update-icon-cache` touch): a change drops the directory listings the lookup keeps.
fn watch(roots: &Roots, chain: &[String]) -> Vec<Stamp> {
    let mut v = roots.stamps();
    for id in chain {
        for r in &roots.icon_dirs {
            v.push(Stamp::of(&r.join(id)));
            v.push(Stamp::of(&r.join(id).join("icon-theme.cache")));
        }
    }
    v
}

pub struct State {
    cfg_path: PathBuf,
    roots: Roots,
    pub cfg: IconConfig,
    pub desktop: Option<String>,
    pub choice: Choice,
    resolver: Resolver,
    shell_stamp: Stamp,
    desktop_stamps: Vec<Stamp>,
    root_stamps: Vec<Stamp>,
}

impl State {
    pub fn new(cfg_path: PathBuf, roots: Roots) -> State {
        let cfg = IconConfig::read(&cfg_path);
        let desktop = config::desktop_choice();
        Self::build(cfg_path, roots, cfg, desktop)
    }

    fn build(cfg_path: PathBuf, roots: Roots, cfg: IconConfig, desktop: Option<String>) -> State {
        let themes = resolve::list_themes(&roots);
        let choice = choose(&cfg, &themes, desktop.as_deref());
        let first: Vec<String> = choice.active.iter().cloned().collect();
        let chain = resolve::expand_chain(&roots, &first, Some(&cfg.fallback));
        State {
            shell_stamp: Stamp::of(&cfg_path),
            desktop_stamps: config::desktop_files().iter().map(|p| Stamp::of(p)).collect(),
            root_stamps: watch(&roots, &chain),
            resolver: Resolver::new(roots.clone(), chain),
            cfg_path,
            roots,
            cfg,
            desktop,
            choice,
        }
    }

    pub fn chain(&self) -> &[String] {
        &self.resolver.chain
    }

    pub fn resolver(&mut self) -> &mut Resolver {
        &mut self.resolver
    }

    /// Re-reads whatever changed on disk. True when icons must be redrawn (a different theme, setting or
    /// installed set of themes).
    pub fn poll(&mut self) -> bool {
        let shell = Stamp::of(&self.cfg_path);
        let desk: Vec<Stamp> = config::desktop_files().iter().map(|p| Stamp::of(p)).collect();
        let roots = watch(&self.roots, &self.resolver.chain);
        if shell == self.shell_stamp && desk == self.desktop_stamps && roots == self.root_stamps {
            return false;
        }
        let cfg = IconConfig::read(&self.cfg_path);
        if cfg == self.cfg && desk == self.desktop_stamps && roots == self.root_stamps {
            self.shell_stamp = shell; // some other key of macshell.json changed
            return false;
        }
        // GSettings is only asked when the desktop files moved (or the config started following the desktop)
        let desktop = if desk != self.desktop_stamps || (cfg.is_auto() && !self.cfg.is_auto()) { config::desktop_choice() } else { self.desktop.clone() };
        let next = State::build(self.cfg_path.clone(), self.roots.clone(), cfg, desktop);
        let changed = next.cfg != self.cfg || next.choice != self.choice || next.resolver.chain != self.resolver.chain || next.root_stamps != self.root_stamps;
        *self = next;
        changed
    }
}

// ---------------------------------------------------------------- process-wide

static STATE: Mutex<Option<State>> = Mutex::new(None);
static GEN: AtomicU64 = AtomicU64::new(1);
static LAST_POLL_MS: AtomicU64 = AtomicU64::new(0);
static SCALE_BITS: AtomicU32 = AtomicU32::new(0x3f80_0000);
static TINT: AtomicBool = AtomicBool::new(false);

fn clock_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

fn publish(s: &State) {
    SCALE_BITS.store(s.cfg.size_scale.to_bits(), Ordering::Relaxed);
    TINT.store(s.cfg.monochrome_tint, Ordering::Relaxed);
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let s = g.get_or_insert_with(|| {
        let s = State::new(config::shell_json(), Roots::from_env());
        publish(&s);
        s
    });
    f(s)
}

/// Re-checks the files now; true when the icons changed (the generation was bumped).
pub fn poll_now() -> bool {
    LAST_POLL_MS.store(clock_ms(), Ordering::Relaxed);
    let changed = with(|s| {
        let c = s.poll();
        if c {
            publish(s);
        }
        c
    });
    if changed {
        GEN.fetch_add(1, Ordering::Relaxed);
    }
    changed
}

/// Changes whenever icons must be redrawn. Cheap (an atomic), and polls the files at most every 700 ms itself.
pub fn generation() -> u64 {
    let now = clock_ms();
    let last = LAST_POLL_MS.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= Duration::from_millis(700).as_millis() as u64 && LAST_POLL_MS.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
        poll_now();
    }
    GEN.load(Ordering::Relaxed)
}

/// The icon file for `id` at `px` physical pixels in the current chain.
pub fn lookup(id: &str, px: u32, generic: bool) -> Option<Found> {
    with(|s| s.resolver().lookup(id, px, generic))
}

/// `icons.size_scale` and `icons.monochrome_tint` as last read.
pub fn render_settings() -> (f32, bool) {
    with(|_| ());
    (f32::from_bits(SCALE_BITS.load(Ordering::Relaxed)), TINT.load(Ordering::Relaxed))
}

/// Everything `macthemectl icons status` and the settings pane show.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub config: IconConfig,
    pub desktop: Option<String>,
    pub active: Option<String>,
    pub problem: Option<String>,
    pub chain: Vec<String>,
    pub cached_paths: usize,
}

pub fn status() -> Status {
    with(|s| Status {
        config: s.cfg.clone(),
        desktop: s.desktop.clone(),
        active: s.choice.active.clone(),
        problem: s.choice.problem.clone(),
        chain: s.chain().to_vec(),
        cached_paths: s.resolver().cached_paths(),
    })
}

/// The chain a theme id would give with the current fallback (for previews of a theme that is not active).
pub fn chain_for(theme: Option<&str>) -> Vec<String> {
    let fallback = with(|s| s.cfg.fallback.clone());
    let first: Vec<String> = theme.map(String::from).into_iter().collect();
    resolve::expand_chain(&Roots::from_env(), &first, Some(&fallback))
}

#[cfg(test)]
mod tests {
    use super::resolve::testkit::*;
    use super::*;

    fn themes(t: &Tree) -> Vec<ThemeInfo> {
        resolve::list_themes(&t.roots())
    }

    fn tree() -> Tree {
        let t = Tree::new("live");
        t.theme("hicolor", PLAIN).theme("Adwaita", PLAIN).theme("Tela", &PLAIN.replace("Plain", "Tela")).theme("Papirus", &PLAIN.replace("Plain", "Papirus"));
        t.theme("Mono", "[Icon Theme]\nName=Mono\nDirectories=symbolic/apps\n[symbolic/apps]\nSize=16\nType=Scalable\n");
        t
    }

    fn cfg(theme: &str) -> IconConfig {
        IconConfig { theme: theme.into(), ..IconConfig::default() }
    }

    #[test]
    fn explicit_theme_wins_over_the_desktop() {
        let t = tree();
        let c = choose(&cfg("Tela"), &themes(&t), Some("Papirus"));
        assert_eq!(c, Choice { active: Some("Tela".into()), problem: None });
        assert_eq!(choose(&cfg("tela"), &themes(&t), None).active.as_deref(), Some("Tela"), "any case");
    }

    #[test]
    fn auto_follows_the_desktop_and_ignores_what_is_not_installed() {
        let t = tree();
        let th = themes(&t);
        assert_eq!(choose(&cfg(""), &th, Some("Papirus")).active.as_deref(), Some("Papirus"));
        assert_eq!(choose(&cfg("auto"), &th, Some("Papirus")).active.as_deref(), Some("Papirus"));
        let none = choose(&cfg("auto"), &th, Some("NotInstalled"));
        assert_eq!(none, Choice { active: None, problem: None }, "hicolor and the fallback only, and no complaint in auto mode");
        assert_eq!(choose(&cfg("auto"), &th, None).active, None);
    }

    #[test]
    fn a_missing_or_unusable_explicit_theme_reports_and_follows_the_desktop() {
        let t = tree();
        let th = themes(&t);
        let c = choose(&cfg("Ghost"), &th, Some("Tela"));
        assert_eq!(c.active.as_deref(), Some("Tela"));
        assert!(c.problem.unwrap().contains("not installed"));
        let c = choose(&cfg("Mono"), &th, None);
        assert_eq!(c.active, None);
        assert!(c.problem.unwrap().contains("symbolic"));
    }

    #[test]
    fn state_chain_uses_the_choice_hicolor_and_the_fallback() {
        let t = tree();
        let f = t.root.join("macshell.json");
        std::fs::write(&f, r#"{"icons": {"theme": "Tela"}}"#).unwrap();
        let s = State::build(f.clone(), t.roots(), IconConfig::read(&f), None);
        assert_eq!(s.chain(), ["Tela", "hicolor", "Adwaita"]);
        let s = State::build(f.clone(), t.roots(), IconConfig { fallback: "none".into(), ..cfg("Tela") }, None);
        assert_eq!(s.chain(), ["Tela", "hicolor"]);
        let s = State::build(f, t.roots(), cfg("auto"), None);
        assert_eq!(s.chain(), ["hicolor", "Adwaita"]);
    }

    #[test]
    fn polling_follows_config_edits_and_ignores_other_keys() {
        let t = tree();
        t.icon("Tela", "scalable/apps/only-tela.svg").icon("Papirus", "scalable/apps/only-papirus.svg");
        let f = t.root.join("macshell.json");
        std::fs::write(&f, r#"{"icons": {"theme": "Tela"}}"#).unwrap();
        let mut s = State::build(f.clone(), t.roots(), IconConfig::read(&f), None);
        assert!(s.resolver().lookup("only-tela", 48, false).is_some() && s.resolver().lookup("only-papirus", 48, false).is_none());
        assert!(!s.poll(), "nothing changed");
        // another key: the stamp moves, the result does not
        std::fs::write(&f, r#"{"icons": {"theme": "Tela"}, "workspaces": "off"}"#).unwrap();
        assert!(!s.poll(), "an unrelated key does not repaint anything");
        std::fs::write(&f, r#"{"icons": {"theme": "Papirus", "size_scale": 0.9}, "workspaces": "off"}"#).unwrap();
        assert!(s.poll());
        assert_eq!(s.cfg.size_scale, 0.9);
        assert!(s.resolver().lookup("only-papirus", 48, false).is_some() && s.resolver().lookup("only-tela", 48, false).is_none(), "caches were rebuilt for the new chain");
        std::fs::write(&f, r#"{"icons": {"theme": "Papirus", "size_scale": 0.9, "monochrome_tint": true}}"#).unwrap();
        assert!(s.poll(), "the tint toggle is a change too");
        assert!(s.cfg.monochrome_tint);
        std::fs::write(&f, "{ broken json").unwrap();
        assert!(s.poll(), "a broken file means the defaults");
        assert!(s.cfg.is_auto());
        assert!(!s.poll());
    }

    #[test]
    fn newly_installed_themes_are_noticed_through_the_directory_stamp() {
        let t = tree();
        let f = t.root.join("macshell.json");
        std::fs::write(&f, r#"{"icons": {"theme": "NewTheme"}}"#).unwrap();
        let mut s = State::build(f, t.roots(), cfg("NewTheme"), None);
        assert!(s.choice.problem.is_some());
        std::thread::sleep(Duration::from_millis(20));
        t.theme("NewTheme", &PLAIN.replace("Plain", "New"));
        assert!(s.poll(), "installing the theme fixes the selection");
        assert_eq!(s.choice, Choice { active: Some("NewTheme".into()), problem: None });
    }

    #[test]
    fn the_desktop_choice_is_only_asked_when_its_files_moved() {
        let t = tree();
        let f = t.root.join("macshell.json");
        let mut s = State::build(f.clone(), t.roots(), cfg("auto"), Some("Tela".into()));
        assert_eq!(s.choice.active.as_deref(), Some("Tela"));
        std::fs::write(&f, r#"{"icons": {"size_scale": 0.8}}"#).unwrap();
        assert!(s.poll());
        assert_eq!(s.desktop.as_deref(), Some("Tela"), "the remembered desktop answer is kept");
        assert_eq!(s.choice.active.as_deref(), Some("Tela"));
    }
}
