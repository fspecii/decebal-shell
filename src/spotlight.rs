//! Spotlight: one search box for apps, system commands, package install/remove and a calculator.
//!
//! The result list is built by [`search`] (pure apart from the `pacman` queries); [`Spotlight`] is
//! the window UI around it.

use crate::app::Button;
use crate::apps::{self, AppEntry};
use crate::calc;
use crate::fuzzy::{self, History};
use crate::gfx::{blit_scaled, color, fill_rect, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::launch;
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use crate::units;
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tiny_skia::{Color, Pixmap};

const CARD_W: u32 = 680;
const HEADER_H: f32 = 64.0;
const ROW_H: f32 = 48.0;
const MAX_ROWS: usize = 8;
const PAD: f32 = 12.0;
const ICON: u32 = 32;
const MAX_APPS: usize = 6;
const MAX_PACKAGES: usize = 30;
const MAX_FILES: usize = 4;

fn text_color() -> Color {
    crate::theme::th().text_c()
}
fn subtext_color() -> Color {
    crate::theme::th().ink(0.55)
}

/// What Enter does for a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// A `.desktop` `Exec=` line.
    Exec(String),
    /// An installed application (keeps `Path=`, `Terminal=`, `TryExec=`, `%i`/`%c`/`%k`).
    App(AppEntry),
    Argv(Vec<String>),
    /// A shell command shown in a terminal window (package installs need sudo and a password prompt).
    Terminal(String),
    /// A shell command run in the background (`sh -c`).
    Shell(String),
    Copy(String),
    Url(String),
    /// Open a file or folder with the default application.
    Open(String),
    /// Show the folder that contains a file.
    Reveal(String),
    /// Replace the search text (a suggestion).
    SetQuery(String),
    /// A built-in toggle handled by the bar's helpers.
    DarkMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    App(String),
    /// A symbol name on a coloured badge.
    Badge(&'static str, (u8, u8, u8)),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub subtitle: String,
    pub kind: &'static str,
    pub icon: Icon,
    pub act: Act,
    /// Needs a second Enter (log out, restart, shut down).
    pub danger: bool,
    /// What Ctrl+Enter / Alt+Enter does instead of `act`.
    pub alt: Option<Act>,
    /// Usage-history key (set for results worth learning from).
    pub key: Option<String>,
}

impl Item {
    fn new(title: String, subtitle: String, kind: &'static str, icon: Icon, act: Act) -> Item {
        Item { title, subtitle, kind, icon, act, danger: false, alt: None, key: None }
    }
}

const BLUE: (u8, u8, u8) = (10, 132, 255);
const GREEN: (u8, u8, u8) = (48, 176, 90);
const RED: (u8, u8, u8) = (235, 77, 75);
const GRAY: (u8, u8, u8) = (110, 110, 118);
const ORANGE: (u8, u8, u8) = (240, 150, 40);

fn cmd(title: &str, subtitle: &str, symbol: &'static str, badge: (u8, u8, u8), act: Act, danger: bool) -> Item {
    Item { danger, ..Item::new(title.into(), subtitle.into(), "Command", Icon::Badge(symbol, badge), act) }
}

fn argv(parts: &[&str]) -> Act {
    Act::Argv(parts.iter().map(|s| s.to_string()).collect())
}

/// True when `prog` is an executable file on `PATH` (or an existing absolute path).
pub fn in_path(prog: &str) -> bool {
    if prog.contains('/') {
        return std::path::Path::new(prog).is_file();
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
}

/// The first candidate whose program is installed.
fn first_available(cands: &[&[&str]]) -> Option<Act> {
    cands.iter().find(|c| in_path(c[0])).map(|c| argv(c))
}

fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn lock_act() -> Act {
    let c: &[&[&str]] = if wayland() {
        &[&["omarchy-lock-screen"], &["loginctl", "lock-session"]]
    } else {
        &[&["omarchy-lock-screen"], &["xsecurelock"], &["i3lock"], &["slock"], &["light-locker-command", "-l"], &["xdg-screensaver", "lock"], &["loginctl", "lock-session"]]
    };
    first_available(c).unwrap_or_else(|| argv(&["loginctl", "lock-session"]))
}

/// Whether Enter on `act` can work on this machine (its program exists).
fn runnable(act: &Act) -> bool {
    match act {
        Act::Argv(a) => a.first().is_some_and(|p| in_path(p)),
        Act::Terminal(c) | Act::Exec(c) => c.split_whitespace().next().is_some_and(|p| p == "sudo" || in_path(p)),
        _ => true,
    }
}

/// Built-in commands. The second field is extra search words. Commands whose program is missing are left out.
fn commands() -> &'static [(Item, &'static str)] {
    static CMDS: std::sync::OnceLock<Vec<(Item, &'static str)>> = std::sync::OnceLock::new();
    CMDS.get_or_init(|| {
        let user = std::env::var("USER").unwrap_or_default();
        let shot = Some(Act::Argv(crate::capture::command()));
        let mut v = vec![
            (cmd("Install Packages", "Browse and install Arch packages", "download", GREEN, Act::Terminal("omarchy-pkg-install".into()), false), "app store add program software pacman"),
            (cmd("Remove Packages", "Uninstall programs from this computer", "trash-can", RED, Act::Terminal("omarchy-pkg-remove".into()), false), "uninstall delete program software pacman"),
            (cmd("Update System", "Install the latest updates", "refresh", BLUE, Act::Terminal("omarchy-update".into()), false), "upgrade software update pacman"),
            (cmd("Lock Screen", "", "lock", GRAY, lock_act(), false), ""),
            (cmd("Sleep", "", "moon", GRAY, argv(&["systemctl", "suspend"]), false), "suspend"),
            (cmd("Restart", "Press Enter again to confirm", "refresh", ORANGE, argv(&["systemctl", "reboot"]), true), "reboot"),
            (cmd("Shut Down", "Press Enter again to confirm", "power", RED, argv(&["systemctl", "poweroff"]), true), "poweroff turn off"),
            (cmd("Log Out", "Press Enter again to confirm", "logout", ORANGE, argv(&["loginctl", "terminate-user", &user]), true), "sign out exit"),
            (cmd("Toggle Dark Mode", "Switch between light and dark", "dark-mode", GRAY, Act::DarkMode, false), "appearance theme light"),
            (cmd("Wallpapers", "Browse Wallhaven", "picture", BLUE, Act::Exec(launch::sibling_or_path("dechaven")), false), "wallhaven background wallpaper"),
            (cmd("Launchpad", "All applications", "grid", BLUE, Act::Exec(launch::sibling_or_path("declaunch")), false), "apps applications"),
        ];
        if let Some(a) = shot {
            v.push((cmd("Screenshot", "Capture the screen", "screenshot", GRAY, a, false), "capture screen"));
        }
        if cfg!(test) {
            return v;
        }
        v.retain(|(i, _)| runnable(&i.act));
        v
    })
}

/// `("install", "foo bar")` for `install foo bar`; also `i foo`.
fn package_verb(q: &str) -> Option<(&'static str, &str)> {
    let (verb, rest) = q.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    match verb.to_lowercase().as_str() {
        "install" | "i" | "get" => Some(("install", rest)),
        "remove" | "uninstall" | "rm" | "delete" => Some(("remove", rest)),
        _ => None,
    }
}

/// One `pacman -Ss` / `-Qs` hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub repo: String,
    pub version: String,
    pub installed: bool,
    pub desc: String,
}

pub fn parse_pacman(out: &str) -> Vec<Package> {
    let mut pkgs = Vec::new();
    let mut lines = out.lines();
    while let Some(head) = lines.next() {
        if head.starts_with(char::is_whitespace) {
            continue;
        }
        let mut words = head.split_whitespace();
        let Some((repo, name)) = words.next().and_then(|w| w.split_once('/')) else { continue };
        let version = words.next().unwrap_or("").to_string();
        let installed = head.contains("[installed") || repo == "local";
        let desc = lines.next().map(|l| l.trim().to_string()).unwrap_or_default();
        pkgs.push(Package { name: name.into(), repo: repo.into(), version, installed, desc });
    }
    pkgs
}

/// Package names pacman reports are `[a-z0-9@._+-]`; anything else is never put in a shell command.
fn safe_name(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('-') && n.chars().all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
}

fn pacman(flag: &str, terms: &str) -> Vec<Package> {
    let mut c = Command::new("pacman");
    c.arg(flag).arg("--");
    c.args(terms.split_whitespace());
    let out = c.stdin(Stdio::null()).stderr(Stdio::null()).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut pkgs = parse_pacman(&out);
    let q = terms.to_lowercase();
    // Exact name first, then names that start with the text, then the rest in pacman's order.
    pkgs.sort_by_key(|p| {
        if p.name == q {
            0
        } else if p.name.starts_with(&q) {
            1
        } else {
            2
        }
    });
    pkgs.truncate(MAX_PACKAGES);
    pkgs
}

fn package_items(verb: &str, terms: &str) -> Vec<Item> {
    let flag = if verb == "install" { "-Ss" } else { "-Qs" };
    pacman_items(verb, pacman(flag, terms))
}

pub fn pacman_items(verb: &str, pkgs: Vec<Package>) -> Vec<Item> {
    pkgs.into_iter()
        .filter(|p| safe_name(&p.name))
        .map(|p| {
            let (kind, badge, symbol, act) = if verb == "install" {
                ("Install", GREEN, "download", Act::Terminal(format!("sudo pacman -S --needed {}", p.name)))
            } else {
                ("Remove", RED, "trash-can", Act::Terminal(format!("sudo pacman -Rns {}", p.name)))
            };
            let title = if verb == "install" && p.installed { format!("{} (installed)", p.name) } else { p.name.clone() };
            let subtitle = if p.desc.is_empty() { format!("{} {}", p.repo, p.version) } else { p.desc.clone() };
            Item::new(title, subtitle, kind, Icon::Badge(symbol, badge), act)
        })
        .collect()
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Single-quotes `s` for `sh`.
fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./=:@%+,".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn app_key(name: &str) -> String {
    format!("app:{name}")
}

fn app_item(a: &AppEntry) -> Item {
    let cmdline = launch::exec_argv(&a.exec).iter().map(|s| shell_quote(s)).collect::<Vec<_>>().join(" ");
    Item {
        alt: if cmdline.is_empty() { None } else { Some(Act::Terminal(cmdline)) },
        key: Some(app_key(&a.name)),
        ..Item::new(a.name.clone(), String::new(), "Application", Icon::App(a.icon.clone()), Act::App(a.clone()))
    }
}

/// The program name of an `Exec=` line, for matching `code` to "Visual Studio Code".
fn exec_name(exec: &str) -> String {
    launch::exec_argv(exec).first().and_then(|p| p.rsplit('/').next()).unwrap_or("").to_string()
}

/// Best matches of `q` among the apps, with usage history folded into the score.
fn rank_apps(q: &str, apps: &[AppEntry], hist: &History, now: u64) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let s = fuzzy::best(q, &[&a.name, &exec_name(&a.exec)])?;
            Some((s + hist.boost(&app_key(&a.name), now), i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| apps[a.1].name.len().cmp(&apps[b.1].name.len())).then(a.1.cmp(&b.1)));
    scored.into_iter().take(MAX_APPS).map(|(_, i)| i).collect()
}

/// Everything shown for `query`, best first (no usage history).
pub fn search(query: &str, apps: &[AppEntry]) -> Vec<Item> {
    search_with(query, apps, &History::default(), 0)
}

/// Everything shown for `query`, best first. Files arrive later from a worker thread.
pub fn search_with(query: &str, apps: &[AppEntry], hist: &History, now: u64) -> Vec<Item> {
    let q = query.trim();
    if q.is_empty() {
        // Recently used apps, like opening Spotlight on a Mac.
        return hist.recent("app:", MAX_APPS).iter().filter_map(|k| apps.iter().find(|a| &app_key(&a.name) == k)).map(app_item).collect();
    }
    if let Some(c) = q.strip_prefix('>').or_else(|| q.strip_prefix('$')).map(str::trim).filter(|c| !c.is_empty()) {
        return vec![Item {
            alt: Some(Act::Shell(c.to_string())),
            ..Item::new(format!("Run “{c}”"), "Enter: in a terminal  ·  Ctrl+Enter: in the background".into(), "Command", Icon::Badge("terminal", GRAY), Act::Terminal(c.to_string()))
        }];
    }
    if let Some((verb, rest)) = package_verb(q) {
        if rest.chars().count() >= 2 {
            return package_items(verb, rest);
        }
    }
    let mut out = Vec::new();
    if let Some(v) = calc::eval(q) {
        let r = calc::format(v);
        out.push(Item {
            alt: Some(Act::Copy(format!("{q} = {r}"))),
            ..Item::new(format!("= {r}"), format!("{q}  ·  Enter copies the result"), "Calculator", Icon::Badge("equal", ORANGE), Act::Copy(r))
        });
    } else if let Some((r, desc)) = units::convert(q) {
        out.push(Item { alt: Some(Act::Copy(desc.clone())), ..Item::new(format!("= {r}"), format!("{desc}  ·  Enter copies the result"), "Conversion", Icon::Badge("equal", ORANGE), Act::Copy(r)) });
    }
    let lq = q.to_lowercase();
    let mut cmds: Vec<Item> = commands().iter().filter(|(i, extra)| fuzzy::best(&lq, &[&i.title, extra]).is_some_and(|s| s >= 450)).map(|(i, _)| i.clone()).collect();
    // An exact or prefix title match outranks apps, like a real Spotlight "Top Hit".
    let top: Vec<Item> = cmds.iter().filter(|i| i.title.to_lowercase().starts_with(&lq)).cloned().collect();
    cmds.retain(|i| !i.title.to_lowercase().starts_with(&lq));
    out.extend(top);
    out.extend(rank_apps(q, apps, hist, now).into_iter().map(|i| app_item(&apps[i])));
    out.extend(cmds);
    if out.is_empty() {
        out.push(Item::new(format!("Search packages for “{q}”"), "Find and install it".into(), "Packages", Icon::Badge("download", GREEN), Act::SetQuery(format!("install {q}"))));
    }
    if out.first().is_none_or(|i| i.kind != "Calculator" && i.kind != "Conversion") {
        out.push(Item::new(format!("Search the web for “{q}”"), String::new(), "Web", Icon::Badge("globe", BLUE), Act::Url(web_url(q))));
    }
    out
}

/// `DECSPOT_SEARCH_URL` (with `%s` for the query) or DuckDuckGo.
fn web_url(q: &str) -> String {
    let tpl = std::env::var("DECSPOT_SEARCH_URL").ok().filter(|t| t.contains("%s")).unwrap_or_else(|| "https://duckduckgo.com/?q=%s".into());
    tpl.replacen("%s", &url_encode(q), 1)
}

// ---------------------------------------------------------------- files

/// Whether a path found by the file index is worth showing: inside `home`, no hidden or build directories.
pub fn file_ok(path: &str, home: &str) -> bool {
    let Some(rest) = path.strip_prefix(home).and_then(|r| r.strip_prefix('/')) else { return false };
    !rest.split('/').any(|c| c.starts_with('.') || matches!(c, "node_modules" | "target" | "__pycache__" | "site-packages"))
}

/// Ranks raw index hits for `q` by file name match and depth; returns the best few.
pub fn rank_files(q: &str, hits: Vec<String>, home: &str) -> Vec<String> {
    let mut scored: Vec<(i32, String)> = hits
        .into_iter()
        .filter(|p| file_ok(p, home))
        .filter_map(|p| {
            let name = p.rsplit('/').next().unwrap_or(&p);
            let s = fuzzy::score(q, name).filter(|s| *s >= 450)?;
            let depth = p.matches('/').count() as i32;
            Some((s - depth * 6, p))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
    scored.into_iter().take(MAX_FILES).map(|(_, p)| p).collect()
}

fn run_capture(prog: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(prog).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Index lookup with `plocate`, else a bounded `fd` walk of `$HOME`; empty when neither is installed.
fn find_files(q: &str) -> Vec<String> {
    let Ok(home) = std::env::var("HOME") else { return Vec::new() };
    let mut hits = Vec::new();
    if in_path("plocate") {
        if let Some(out) = run_capture("plocate", &["-i", "-b", "-l", "400", "--", q]) {
            hits = out.lines().map(str::to_string).collect();
        }
    }
    if hits.is_empty() && in_path("fd") {
        if let Some(out) = run_capture("fd", &["-a", "-i", "--max-results", "200", "--", q, &home]) {
            hits = out.lines().map(str::to_string).collect();
        }
    }
    rank_files(q, hits, &home)
}

fn file_item(path: &str, home: &str) -> Item {
    let is_dir = std::path::Path::new(path).is_dir();
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let dir = match dir.strip_prefix(home) {
        Some(r) => format!("~{r}"),
        None => dir.to_string(),
    };
    let folder = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    Item {
        alt: Some(Act::Reveal(if is_dir { path.to_string() } else { folder })),
        ..Item::new(
            name.to_string(),
            format!("{dir}  ·  Ctrl+Enter shows the folder"),
            if is_dir { "Folder" } else { "File" },
            Icon::Badge(if is_dir { "folder" } else { "doc" }, BLUE),
            Act::Open(path.to_string()),
        )
    }
}

/// Whether `q` should also search for files.
fn wants_files(q: &str) -> bool {
    let q = q.trim();
    q.chars().count() >= 3 && !q.starts_with('>') && !q.starts_with('$') && package_verb(q).is_none() && calc::eval(q).is_none() && units::convert(q).is_none()
}

// ---------------------------------------------------------------- actions

/// Opens `shell_cmd` in a terminal window, keeping it open on failure so errors can be read.
fn run_in_terminal(shell_cmd: &str) {
    let script = format!("{shell_cmd}; s=$?; if [ $s -ne 0 ]; then echo; read -rn1 -p \"Failed ($s). Press any key to close.\"; fi");
    let term = std::env::var("TERMINAL").unwrap_or_default();
    let mut attempts: Vec<Vec<String>> = Vec::new();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if in_path("xdg-terminal-exec") {
        attempts.push(s(&["xdg-terminal-exec", "--app-id=org.omarchy.terminal", "--title=Omarchy", "-e", "bash", "-c", &script]));
    }
    if !term.is_empty() && in_path(&term) {
        attempts.push(s(&[&term, "-e", "bash", "-c", &script]));
    }
    for t in ["alacritty", "kitty", "foot", "ghostty", "xterm", "st", "urxvt"] {
        if in_path(t) {
            attempts.push(s(&[t, "-e", "bash", "-c", &script]));
        }
    }
    for a in attempts {
        if Command::new("setsid").args(&a).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(reap).is_ok() {
            return;
        }
    }
}

fn reap(mut c: std::process::Child) {
    std::thread::spawn(move || {
        let _ = c.wait();
    });
}

fn detached(prog: &str, args: &[String]) {
    let _ = Command::new("setsid").arg(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(reap);
}

fn run(act: &Act) {
    match act {
        Act::Exec(e) => {
            launch::spawn_exec(e);
        }
        Act::App(a) => {
            launch::spawn_app(a);
        }
        Act::Argv(a) => {
            launch::spawn_argv(a);
        }
        Act::Terminal(c) => run_in_terminal(c),
        Act::Shell(c) => detached("sh", &["-c".into(), c.clone()]),
        Act::Copy(t) => {
            crate::clipboard::copy(t);
        }
        Act::Url(u) => detached("xdg-open", std::slice::from_ref(u)),
        Act::Open(p) => detached("xdg-open", std::slice::from_ref(p)),
        Act::Reveal(p) => detached("xdg-open", std::slice::from_ref(p)),
        Act::DarkMode => crate::controlcenter::sys::toggle_appearance(),
        Act::SetQuery(_) => {}
    }
}

/// Results the background threads hand to the UI thread.
#[derive(Default)]
struct Shared {
    apps: Mutex<Option<Vec<AppEntry>>>,
    files: Mutex<Option<(u64, Vec<String>)>>,
}

pub struct Spotlight {
    apps: Vec<AppEntry>,
    hist: History,
    field: TextField,
    /// Search results without files.
    base: Vec<Item>,
    files: Vec<Item>,
    /// What is shown: `base` with `files` placed before the web-search fallback.
    items: Vec<Item>,
    sel: usize,
    scroll: usize,
    confirm: Option<usize>,
    size: (u32, u32),
    origin: (f32, f32),
    overlay: bool,
    popup: bool,
    rounded: bool,
    fonts: Option<Fonts>,
    icons: IconCache,
    symbols: SymbolCache,
    shared: Arc<Shared>,
    gen: Arc<AtomicU64>,
    waker: Option<Waker>,
    /// Look up files with plocate/fd while typing.
    search_files: bool,
}

impl Spotlight {
    /// The app list comes from the on-disk cache (a rescan in the background refreshes it).
    pub fn new() -> Self {
        let mut apps = apps::load_cache();
        let cached = !apps.is_empty();
        if !cached {
            apps = apps::list();
            apps::save_cache(&apps);
        }
        let mut s = Self::with_apps(apps);
        s.hist = History::load();
        s.search_files = true;
        s.requery();
        s
    }

    pub fn with_apps(apps: Vec<AppEntry>) -> Self {
        Self {
            apps,
            hist: History::default(),
            field: TextField::default(),
            base: Vec::new(),
            files: Vec::new(),
            items: Vec::new(),
            sel: 0,
            scroll: 0,
            confirm: None,
            size: (CARD_W, HEADER_H as u32),
            origin: (0.0, 0.0),
            overlay: false,
            popup: false,
            rounded: true,
            fonts: Fonts::load(),
            icons: IconCache::default(),
            symbols: SymbolCache::default(),
            shared: Arc::new(Shared::default()),
            gen: Arc::new(AtomicU64::new(0)),
            waker: None,
            search_files: false,
        }
    }

    /// Rescans the `.desktop` files off the UI thread; a changed list replaces the cache and the shown apps.
    fn refresh_apps_in_background(&self) {
        let shared = self.shared.clone();
        let waker = self.waker.clone();
        let current = self.apps.clone();
        std::thread::spawn(move || {
            let fresh = apps::list();
            if fresh != current {
                apps::save_cache(&fresh);
                *shared.apps.lock().unwrap_or_else(|e| e.into_inner()) = Some(fresh);
                if let Some(w) = waker {
                    w();
                }
            }
        });
    }

    fn visible_rows(&self) -> usize {
        if self.overlay || self.popup {
            MAX_ROWS
        } else {
            (((self.size.1 as f32 - HEADER_H) / ROW_H).floor() as usize).max(1)
        }
    }

    fn shown(&self) -> usize {
        self.items.len().min(self.visible_rows())
    }

    /// Height of the card for the current results (the whole window when it is neither an overlay nor a popup).
    fn card_h(&self) -> f32 {
        if self.overlay || self.popup {
            HEADER_H + if self.items.is_empty() { 0.0 } else { self.shown() as f32 * ROW_H + PAD }
        } else {
            self.size.1 as f32
        }
    }

    fn card_w(&self) -> f32 {
        self.size.0 as f32
    }

    fn compose(&mut self) {
        let keep = self.items.get(self.sel).map(|i| (i.kind, i.title.clone()));
        let mut items = self.base.clone();
        let at = items.iter().position(|i| i.kind == "Web").unwrap_or(items.len());
        items.splice(at..at, self.files.iter().cloned());
        self.items = items;
        if let Some(k) = keep {
            if let Some(p) = self.items.iter().position(|i| (i.kind, i.title.clone()) == k) {
                self.sel = p;
            }
        }
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
        let rows = self.visible_rows();
        if self.sel >= self.scroll + rows {
            self.scroll = self.sel + 1 - rows;
        }
        self.scroll = self.scroll.min(self.sel);
    }

    fn requery(&mut self) {
        let q = self.field.text().to_string();
        self.base = search_with(&q, &self.apps, &self.hist, crate::fuzzy::now());
        self.files.clear();
        self.sel = 0;
        self.scroll = 0;
        self.confirm = None;
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        self.compose();
        if self.search_files && wants_files(&q) {
            let (cur, shared, waker) = (self.gen.clone(), self.shared.clone(), self.waker.clone());
            std::thread::spawn(move || {
                // Debounce: only the latest keystroke's query gets searched.
                std::thread::sleep(std::time::Duration::from_millis(150));
                if cur.load(Ordering::SeqCst) != gen {
                    return;
                }
                let files = find_files(q.trim());
                if cur.load(Ordering::SeqCst) != gen || files.is_empty() {
                    return;
                }
                *shared.files.lock().unwrap_or_else(|e| e.into_inner()) = Some((gen, files));
                if let Some(w) = waker {
                    w();
                }
            });
        }
    }

    fn move_sel(&mut self, delta: isize) -> Response {
        if self.items.is_empty() {
            return Response::NONE;
        }
        let n = self.items.len() as isize;
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        self.confirm = None;
        let rows = self.visible_rows();
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + rows {
            self.scroll = self.sel + 1 - rows;
        }
        Response::REDRAW
    }

    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < 0.0 || x >= self.card_w() || y < HEADER_H {
            return None;
        }
        let i = ((y - HEADER_H) / ROW_H) as usize + self.scroll;
        (i < self.items.len() && i < self.scroll + self.visible_rows()).then_some(i)
    }

    fn activate(&mut self, alt: bool) -> Response {
        let Some(item) = self.items.get(self.sel).cloned() else { return Response::NONE };
        if item.danger && self.confirm != Some(self.sel) {
            self.confirm = Some(self.sel);
            return Response::REDRAW;
        }
        if let Act::SetQuery(q) = &item.act {
            self.field.set(q);
            self.requery();
            return Response::REDRAW;
        }
        if let Some(k) = &item.key {
            self.hist.bump(k, crate::fuzzy::now());
            if self.search_files {
                self.hist.save();
            }
        }
        let act = if alt { item.alt.as_ref().unwrap_or(&item.act) } else { &item.act };
        run(act);
        Response::act(Action::Close)
    }

    fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, w))
    }

    fn fit(&self, s: &str, px: f32, max_w: f32) -> String {
        if self.measure(s, px, Weight::Regular) <= max_w {
            return s.to_string();
        }
        let mut t = s.to_string();
        while !t.is_empty() && self.measure(&format!("{t}…"), px, Weight::Regular) > max_w {
            t.pop();
        }
        format!("{}…", t.trim_end())
    }

    #[allow(clippy::too_many_arguments)] // a drawing helper: target, text, position, font size, weight, colour
    fn text_at(&mut self, pm: &mut Pixmap, s: &str, x: f32, cy: f32, px: f32, weight: Weight, c: Color) {
        if let Some(f) = &mut self.fonts {
            f.draw(pm, s, x, cy - px * 0.62, px, weight, c);
        }
    }

    fn draw_card(&mut self, pm: &mut Pixmap) {
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        pm.fill(Color::TRANSPARENT);
        let radius = if self.rounded || self.overlay { crate::theme::th().r(1.6) } else { 0.0 };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, crate::theme::th().overlay_c());

        // Search line.
        if let Some(sym) = self.symbols.get("magnifyingglass", crate::theme::th().text_faint.t(), 44) {
            blit_scaled(pm, sym, PAD + 10.0, HEADER_H / 2.0 - 11.0, 22.0, 22.0, 1.0);
        }
        let tx = PAD + 46.0;
        let cy = HEADER_H / 2.0;
        if self.field.text().is_empty() {
            self.text_at(pm, "Spotlight Search", tx, cy, 20.0, Weight::Regular, subtext_color());
        } else {
            let t = self.field.text().to_string();
            self.text_at(pm, &t, tx, cy, 20.0, Weight::Regular, text_color());
            let cx = (tx + self.measure(self.field.before_cursor(), 20.0, Weight::Regular)).round();
            fill_rect(pm, cx, cy - 12.0, 1.5, 24.0, text_color());
        }
        if !self.items.is_empty() {
            fill_rect(pm, 0.0, HEADER_H - 1.0, w, 1.0, crate::theme::th().ink(0.10));
        }

        let end = (self.scroll + self.visible_rows()).min(self.items.len());
        for pos in self.scroll..end {
            let y = HEADER_H + PAD / 2.0 + (pos - self.scroll) as f32 * ROW_H - PAD / 2.0;
            let item = self.items[pos].clone();
            let selected = pos == self.sel;
            if selected {
                fill_rounded(pm, 8.0, y + 2.0, w - 16.0, ROW_H - 4.0, crate::theme::th().r(0.9), crate::theme::th().accent_c());
            }
            let ix = 20.0;
            let iy = y + (ROW_H - ICON as f32) / 2.0;
            match &item.icon {
                Icon::App(id) => {
                    let icon = self.icons.get(id, ICON);
                    blit_scaled(pm, icon, ix, iy, ICON as f32, ICON as f32, 1.0);
                }
                Icon::Badge(sym, (r, g, b)) => {
                    fill_rounded(pm, ix, iy, ICON as f32, ICON as f32, 8.0, color(*r, *g, *b, 1.0));
                    if let Some(s) = self.symbols.get(sym, (255, 255, 255), 40) {
                        blit_scaled(pm, s, ix + 6.0, iy + 6.0, 20.0, 20.0, 1.0);
                    }
                }
            }
            let kind_w = self.measure(item.kind, 12.0, Weight::Regular);
            let text_x = ix + ICON as f32 + 14.0;
            let max_w = w - text_x - kind_w - 40.0;
            let danger_armed = self.confirm == Some(pos);
            let (main, sub) = if selected { (crate::theme::th().accent_text_c(), crate::theme::th().accent_text.color(0.78)) } else { (text_color(), subtext_color()) };
            let sub_text = if item.danger && !danger_armed { String::new() } else { item.subtitle.clone() };
            let sub_color = if danger_armed && !selected { crate::theme::th().danger.over(crate::theme::th().text, 0.8).color(1.0) } else { sub };
            if sub_text.is_empty() {
                let title = self.fit(&item.title, 15.0, max_w);
                self.text_at(pm, &title, text_x, y + ROW_H / 2.0, 15.0, Weight::Regular, main);
            } else {
                let title = self.fit(&item.title, 15.0, max_w);
                let subt = self.fit(&sub_text, 11.5, max_w);
                self.text_at(pm, &title, text_x, y + ROW_H / 2.0 - 8.0, 15.0, Weight::Regular, main);
                self.text_at(pm, &subt, text_x, y + ROW_H / 2.0 + 10.0, 11.5, Weight::Regular, sub_color);
            }
            self.text_at(pm, item.kind, w - 20.0 - kind_w, y + ROW_H / 2.0, 12.0, Weight::Regular, if selected { crate::theme::th().accent_text.color(0.8) } else { subtext_color() });
        }
        if !self.overlay || !self.items.is_empty() || !self.field.text().is_empty() {
            stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), crate::theme::th().outline_c(), 1.0);
        }
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }
}

impl Default for Spotlight {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowApp for Spotlight {
    fn title(&self) -> String {
        "Spotlight".into()
    }

    fn app_id(&self) -> &'static str {
        "decspot"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn set_overlay(&mut self, overlay: bool) {
        self.overlay = overlay;
    }

    fn initial_size(&self) -> (u32, u32) {
        (CARD_W, HEADER_H as u32 + MAX_ROWS as u32 * ROW_H as u32 / 2)
    }

    fn set_popup(&mut self, popup: bool) {
        self.popup = popup;
    }

    fn content_size(&self) -> Option<(u32, u32)> {
        self.popup.then(|| (CARD_W, self.card_h().ceil() as u32))
    }

    fn set_waker(&mut self, waker: Waker) {
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w());
        self.waker = Some(waker);
        if self.search_files {
            self.refresh_apps_in_background();
        }
    }

    fn set_screen(&mut self, _: u32, _: u32) {}

    fn resize(&mut self, w: u32, h: u32) {
        if self.popup {
            self.size = (w, h);
            self.origin = (0.0, 0.0);
        } else if self.overlay {
            let cw = CARD_W.min(w.saturating_sub(40)).max(300);
            self.size = (cw, h);
            self.origin = ((w as f32 - cw as f32) / 2.0, (h as f32 * 0.2).round());
        } else {
            self.size = (w, h);
            self.origin = (0.0, 0.0);
        }
    }

    fn set_rounded(&mut self, rounded: bool) {
        self.rounded = rounded;
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        if !self.overlay {
            self.draw_card(pm);
            return;
        }
        pm.fill(color(0, 0, 0, 0.18));
        if let Some(mut card) = Pixmap::new(self.size.0, self.card_h().ceil() as u32) {
            self.draw_card(&mut card);
            pm.draw_pixmap(self.origin.0 as i32, self.origin.1 as i32, card.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        match self.row_at(x - self.origin.0, y - self.origin.1) {
            Some(i) if i != self.sel => {
                self.sel = i;
                self.confirm = None;
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed || button != Button::Left {
            return Response::NONE;
        }
        let (px, py) = (x - self.origin.0, y - self.origin.1);
        if self.overlay && (px < 0.0 || py < 0.0 || px >= self.card_w() || py >= self.card_h()) {
            return Response::act(Action::Close);
        }
        match self.row_at(px, py) {
            Some(i) => {
                self.sel = i;
                self.activate(false)
            }
            None => Response::NONE,
        }
    }

    fn scroll(&mut self, _: f32, _: f32, dy: f32) -> Response {
        if dy > 0.0 {
            self.move_sel(1)
        } else if dy < 0.0 {
            self.move_sel(-1)
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Escape => Response::act(Action::Close),
            Key::Enter => self.activate(ev.ctrl || ev.alt),
            Key::Down | Key::Tab => self.move_sel(1),
            Key::Up => self.move_sel(-1),
            Key::PageDown => self.move_sel(MAX_ROWS as isize),
            Key::PageUp => self.move_sel(-(MAX_ROWS as isize)),
            Key::Left => {
                self.field.left();
                Response::REDRAW
            }
            Key::Right => {
                self.field.right();
                Response::REDRAW
            }
            Key::Home => {
                self.field.home();
                Response::REDRAW
            }
            Key::End => {
                self.field.end();
                Response::REDRAW
            }
            Key::Backspace => {
                self.field.backspace();
                self.requery();
                Response::REDRAW
            }
            Key::Delete => {
                self.field.delete();
                self.requery();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && ev.ch == Some('u') => {
                self.field.clear();
                self.requery();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && ev.ch == Some('a') => {
                self.field.home();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && ev.ch == Some('e') => {
                self.field.end();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && matches!(ev.ch, Some('n' | 'j')) => self.move_sel(1),
            Key::Char if ev.ctrl && matches!(ev.ch, Some('p' | 'k')) => self.move_sel(-1),
            Key::Char if !ev.ctrl && !ev.alt && !ev.text.is_empty() => {
                // The hotkey's own space (auto-repeat or a late release) must not start the query.
                if self.field.text().is_empty() && ev.text.trim().is_empty() {
                    return Response::NONE;
                }
                self.field.insert(&ev.text);
                self.requery();
                Response::REDRAW
            }
            Key::Char => Response::NONE,
        }
    }

    fn tick(&mut self) -> Response {
        let mut changed = crate::theme::take_changed();
        if let Some(fresh) = self.shared.apps.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.apps = fresh;
            self.base = search_with(self.field.text(), &self.apps, &self.hist, crate::fuzzy::now());
            changed = true;
        }
        let files = self.shared.files.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some((gen, paths)) = files {
            if gen == self.gen.load(Ordering::SeqCst) {
                let home = std::env::var("HOME").unwrap_or_default();
                self.files = paths.iter().map(|p| file_item(p, &home)).collect();
                changed = true;
            }
        }
        if changed {
            self.compose();
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppEntry {
        AppEntry { name: name.into(), exec: format!("{}-bin", name.to_lowercase()), ..Default::default() }
    }

    fn key(k: Key, text: &str) -> KeyEvent {
        KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift: false, alt: false }
    }

    fn spot() -> Spotlight {
        let mut s = Spotlight::with_apps(vec![app("Firefox"), app("Files"), app("Terminal")]);
        s.fonts = None;
        s
    }

    fn type_text(s: &mut Spotlight, t: &str) {
        for c in t.chars() {
            s.key(&key(Key::Char, &c.to_string()));
        }
    }

    #[test]
    fn empty_query_shows_nothing() {
        assert!(search("  ", &[app("Firefox")]).is_empty());
    }

    #[test]
    fn apps_and_commands_match() {
        let r = search("fire", &[app("Firefox"), app("Terminal")]);
        assert_eq!(r[0].title, "Firefox");
        let r = search("lock", &[]);
        assert_eq!(r[0].title, "Lock Screen");
        assert!(search("uninstall", &[]).iter().any(|i| i.title == "Remove Packages"), "extra search words");
    }

    #[test]
    fn command_title_prefix_beats_apps() {
        let r = search("shut", &[app("Shutter")]);
        assert_eq!(r[0].title, "Shut Down");
        assert_eq!(r[1].title, "Shutter");
    }

    #[test]
    fn calculator_comes_first_and_copies() {
        let r = search("2+3*4", &[]);
        assert_eq!(r[0].kind, "Calculator");
        assert_eq!(r[0].act, Act::Copy("14".into()));
    }

    #[test]
    fn unknown_text_offers_packages_and_web() {
        let r = search("zzqq", &[app("Firefox")]);
        assert_eq!(r[0].act, Act::SetQuery("install zzqq".into()));
        assert!(matches!(r.last().unwrap().act, Act::Url(ref u) if u.ends_with("zzqq")));
    }

    #[test]
    fn parses_pacman_output() {
        let out = "extra/firefox 131.0-1 [installed]\n    Fast, Private & Safe Web Browser\nextra/firefox-i18n-de 131.0-1\n    German language pack\nlocal/vim 9.1-1\n    Vi Improved\n";
        let p = parse_pacman(out);
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].name.as_str(), p[0].installed, p[0].desc.as_str()), ("firefox", true, "Fast, Private & Safe Web Browser"));
        assert!(!p[1].installed);
        assert!(p[2].installed, "local repo means installed");
    }

    #[test]
    fn install_and_remove_items_build_pacman_commands() {
        let pkgs = vec![
            Package { name: "htop".into(), repo: "extra".into(), version: "3.3".into(), installed: false, desc: "Monitor".into() },
            Package { name: "bad;rm".into(), repo: "extra".into(), version: "1".into(), installed: false, desc: String::new() },
        ];
        let i = pacman_items("install", pkgs.clone());
        assert_eq!(i.len(), 1, "unsafe names are dropped");
        assert_eq!(i[0].act, Act::Terminal("sudo pacman -S --needed htop".into()));
        assert_eq!(pacman_items("remove", pkgs)[0].act, Act::Terminal("sudo pacman -Rns htop".into()));
    }

    #[test]
    fn package_verbs() {
        assert_eq!(package_verb("install htop"), Some(("install", "htop")));
        assert_eq!(package_verb("Uninstall   htop "), Some(("remove", "htop")));
        assert_eq!(package_verb("installer"), None);
    }

    #[test]
    fn dangerous_commands_need_two_enters() {
        let mut s = spot();
        type_text(&mut s, "shut");
        assert_eq!(s.items[0].title, "Shut Down");
        let r = s.key(&key(Key::Enter, ""));
        assert_eq!(r.action, None, "first Enter only arms it");
        assert_eq!(s.confirm, Some(0));
        s.key(&key(Key::Down, ""));
        assert_eq!(s.confirm, None, "moving disarms");
    }

    #[test]
    fn suggestion_rewrites_the_query() {
        let mut s = spot();
        type_text(&mut s, "zzqq");
        let r = s.key(&key(Key::Enter, ""));
        assert_eq!(r.action, None);
        assert_eq!(s.field.text(), "install zzqq");
    }

    #[test]
    fn selection_wraps_and_scrolls() {
        let mut s = spot();
        type_text(&mut s, "a");
        let n = s.items.len();
        s.key(&key(Key::Up, ""));
        assert_eq!(s.sel, n - 1);
        s.key(&key(Key::Down, ""));
        assert_eq!(s.sel, 0);
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn overlay_card_grows_with_results_and_background_closes() {
        let mut s = spot();
        s.set_overlay(true);
        s.resize(3440, 1440);
        let empty = s.card_h();
        type_text(&mut s, "fi");
        assert!(s.card_h() > empty);
        assert_eq!(s.pointer_button(5.0, 5.0, Button::Left, true).action, Some(Action::Close));
        let mut pm = Pixmap::new(3440, 1440).unwrap();
        s.draw(&mut pm);
        assert!(pm.pixel(5, 5).unwrap().alpha() > 0);
    }

    #[test]
    fn fuzzy_apps_and_history_ranking() {
        let apps = [app("Visual Studio Code"), app("Vivaldi"), app("Videos Player Pro")];
        assert_eq!(search("vsc", &apps)[0].title, "Visual Studio Code");
        let mut h = History::default();
        for _ in 0..5 {
            h.bump("app:Videos Player Pro", 1000);
        }
        let r = search_with("vi", &apps, &h, 1000);
        assert_eq!(r[0].title, "Videos Player Pro", "frequently used app wins among equal matches");
        assert!(search_with("vi", &apps, &History::default(), 0)[0].title != "Videos Player Pro");
    }

    #[test]
    fn empty_query_lists_recent_apps() {
        let apps = [app("Firefox"), app("Files")];
        let mut h = History::default();
        h.bump("app:Files", 5);
        h.bump("app:Gone", 9);
        let r = search_with("", &apps, &h, 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Files");
    }

    #[test]
    fn conversions_and_shell_commands() {
        let r = search("5 km to mi", &[]);
        assert_eq!(r[0].kind, "Conversion");
        assert_eq!(r[0].act, Act::Copy("3.106856".into()));
        assert!(r.iter().all(|i| i.kind != "Web"));
        let r = search("> ls -l", &[]);
        assert_eq!(r[0].act, Act::Terminal("ls -l".into()));
        assert_eq!(r[0].alt, Some(Act::Shell("ls -l".into())));
    }

    #[test]
    fn alt_actions() {
        let r = search("2+2", &[]);
        assert_eq!(r[0].alt, Some(Act::Copy("2+2 = 4".into())));
        let r = search("fire", &[AppEntry { name: "Firefox".into(), exec: "firefox --new-window %U".into(), ..Default::default() }]);
        assert_eq!(r[0].alt, Some(Act::Terminal("firefox --new-window".into())));
        assert_eq!(r[0].key.as_deref(), Some("app:Firefox"));
    }

    #[test]
    fn file_results_are_filtered_and_ranked() {
        let home = "/home/u";
        let hits = vec![
            "/home/u/.cache/report.txt".to_string(),
            "/home/u/proj/node_modules/report.js".to_string(),
            "/usr/share/report.md".to_string(),
            "/home/u/a/b/c/report.txt".to_string(),
            "/home/u/report.txt".to_string(),
            "/home/u/notes.txt".to_string(),
        ];
        assert_eq!(rank_files("report", hits, home), vec!["/home/u/report.txt", "/home/u/a/b/c/report.txt"]);
    }

    #[test]
    fn files_arrive_before_the_web_fallback_and_keep_selection() {
        let mut s = spot();
        type_text(&mut s, "zzqq");
        s.files = vec![file_item("/home/u/zzqq.txt", "/home/u")];
        s.compose();
        assert_eq!(s.items.iter().map(|i| i.kind).collect::<Vec<_>>(), vec!["Packages", "File", "Web"]);
        s.sel = 2;
        s.files.clear();
        s.compose();
        assert_eq!(s.items[s.sel].kind, "Web");
        assert_eq!(file_item("/home/u/d/x.txt", "/home/u").subtitle.split("  ").next(), Some("~/d"));
    }

    #[test]
    fn leading_space_is_ignored_and_ctrl_enter_uses_alt() {
        let mut s = spot();
        s.key(&key(Key::Char, " "));
        assert_eq!(s.field.text(), "");
        type_text(&mut s, "2+2");
        assert!(s.items[0].alt.is_some());
        let mut ev = key(Key::Enter, "");
        ev.ctrl = true;
        assert_eq!(s.key(&ev).action, Some(Action::Close));
    }

    #[test]
    fn popup_sizes_itself_to_the_results() {
        let mut s = spot();
        s.set_popup(true);
        let (w, h0) = s.content_size().unwrap();
        assert_eq!((w, h0), (CARD_W, HEADER_H as u32));
        type_text(&mut s, "fi");
        assert!(s.content_size().unwrap().1 > h0);
        s.resize(w, s.content_size().unwrap().1);
        let mut pm = Pixmap::new(w, s.content_size().unwrap().1).unwrap();
        s.draw(&mut pm);
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0, "corner is transparent (clipped by the shape)");
        assert!(pm.pixel(w / 2, 10).unwrap().alpha() > 200);
    }

    #[test]
    fn escape_closes() {
        assert_eq!(spot().key(&key(Key::Escape, "")).action, Some(Action::Close));
    }
}
