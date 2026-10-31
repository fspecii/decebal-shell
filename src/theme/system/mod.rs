//! "Apply to all apps": the automatic, reversible pipeline that carries the decebal theme to the rest of the desktop.
//!
//! One palette ([`Palette`], taken from the resolved [`Theme`]) is rendered by small targets:
//!
//! | target | what it changes |
//! |---|---|
//! | `omarchy` | a complete Omarchy theme `~/.config/omarchy/themes/decebal` (`colors.toml`; Omarchy's own template engine makes alacritty/foot/kitty/ghostty/btop/helix/neovim/... from it), `omarchy-theme-set`, live reload of terminals and btop |
//! | `browser` | Chromium/Chrome/Edge/Brave theme colour through Omarchy's browser policy helper |
//! | `editors` | VS Code / Cursor / Obsidian through Omarchy's own sync helpers |
//! | `hooks` | the user's `~/.config/omarchy/hooks/theme-set.d` |
//! | `gtk` | `gtk-3.0` / `gtk-4.0` `gtk.css` (libadwaita CSS variables and named colours), `settings.ini`, gsettings (dark/light, font, button layout) |
//! | `qt` | qt5ct / qt6ct colour scheme and config |
//! | `kvantum` | a Kvantum colour theme |
//! | `x11` | an Xresources file included from `~/.Xresources` and merged with `xrdb` (st, xterm, urxvt) |
//!
//! Every file is changed atomically, the first version of each is saved, and `off` restores them ([`files`]).
//! Nothing here runs unless the user switched it on (`decthemectl system on`, or the Theme pane).

pub mod cli;
pub mod file_manager;
pub mod files;
pub mod gtk;
pub mod omarchy;
pub mod qt;
pub mod terminals;
pub mod x11;

use super::live::{system_dark, Source};
use super::model::{contrast, Rgb, Selection, Theme};
use files::{Edit, State};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

// ---------------------------------------------------------------- palette

/// The colours every target renders from.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub id: String,
    pub name: String,
    pub dark: bool,
    pub bg: Rgb,
    pub sidebar: Rgb,
    pub surface: Rgb,
    pub popup: Rgb,
    pub titlebar: Rgb,
    pub titlebar_inactive: Rgb,
    pub border: Rgb,
    pub text: Rgb,
    pub text_secondary: Rgb,
    pub text_faint: Rgb,
    pub accent: Rgb,
    pub accent_text: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub orange: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    /// The theme's UI font, when it names one.
    pub font: Option<String>,
    /// The interface and monospace font chains, first choice first (fallbacks when the first is not installed).
    pub ui_fonts: Vec<String>,
    pub mono_fonts: Vec<String>,
    pub font_pt: f32,
    pub radius: f32,
    /// Menu / popover opacity from the theme (1 = opaque). Only those backgrounds use it, never window content.
    pub popup_alpha: f32,
    /// `classic` titlebars (buttons on the right) or `macos` (on the left).
    pub classic_buttons: bool,
}

fn hue(c: Rgb) -> f32 {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    if d < 0.08 {
        return -1.0;
    }
    let h = if mx == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

impl Palette {
    pub fn from_theme(t: &Theme) -> Palette {
        let (blue, magenta, cyan, orange, yellow) = if t.dark {
            (Rgb(0x0a, 0x84, 0xff), Rgb(0xbf, 0x5a, 0xf2), Rgb(0x64, 0xd2, 0xff), t.warn, Rgb(0xff, 0xd6, 0x0a))
        } else {
            (Rgb(0x00, 0x62, 0xd6), Rgb(0xa5, 0x50, 0xa7), Rgb(0x0a, 0x8f, 0xb5), t.warn, Rgb(0x9a, 0x70, 0x00))
        };
        let h = hue(t.accent);
        let blue = if (200.0..=260.0).contains(&h) && contrast(t.accent, t.background) >= 3.0 { t.accent } else { blue };
        Palette {
            id: t.id.clone(),
            name: t.name.clone(),
            dark: t.dark,
            bg: t.background,
            sidebar: t.sidebar,
            surface: t.surface,
            popup: t.popup,
            titlebar: t.titlebar,
            titlebar_inactive: t.titlebar_inactive,
            border: t.border,
            text: t.text,
            text_secondary: t.text_secondary,
            text_faint: t.text_faint,
            accent: t.accent,
            accent_text: t.accent_text,
            red: t.danger,
            green: t.ok,
            yellow,
            blue,
            orange,
            magenta,
            cyan,
            font: (!t.font_family.is_empty()).then(|| t.font_family.clone()),
            ui_fonts: t.fonts.chain(super::fonts::Role::Ui).to_vec(),
            mono_fonts: t.fonts.chain(super::fonts::Role::Mono).to_vec(),
            font_pt: (11.0 * t.font_scale * 2.0).round() / 2.0,
            radius: t.radius,
            popup_alpha: t.popup_alpha.clamp(0.0, 1.0),
            classic_buttons: t.window_buttons_right,
        }
    }

    /// The accent as text or an icon on the window background (contrast 3:1 at least).
    pub fn accent_on_bg(&self) -> Rgb {
        let mut c = self.accent;
        for i in 0..10 {
            if contrast(c, self.bg) >= 3.0 {
                break;
            }
            c = self.text.over(self.accent, 0.1 * (i + 1) as f32);
        }
        c
    }

    /// Every role as `#rrggbb`, plus the font and button facts (`decthemectl system palette --json`).
    pub fn to_json(&self) -> Value {
        let roles = [
            ("bg", self.bg),
            ("sidebar", self.sidebar),
            ("surface", self.surface),
            ("popup", self.popup),
            ("titlebar", self.titlebar),
            ("titlebar_inactive", self.titlebar_inactive),
            ("border", self.border),
            ("text", self.text),
            ("text_secondary", self.text_secondary),
            ("text_faint", self.text_faint),
            ("accent", self.accent),
            ("accent_text", self.accent_text),
            ("accent_on_bg", self.accent_on_bg()),
            ("red", self.red),
            ("green", self.green),
            ("yellow", self.yellow),
            ("blue", self.blue),
            ("orange", self.orange),
            ("magenta", self.magenta),
            ("cyan", self.cyan),
        ];
        let mut o: serde_json::Map<String, Value> = roles.iter().map(|(k, c)| (k.to_string(), json!(c.hex()))).collect();
        o.insert("id".into(), json!(self.id));
        o.insert("name".into(), json!(self.name));
        o.insert("dark".into(), json!(self.dark));
        o.insert("font".into(), json!(self.font));
        o.insert("font_pt".into(), json!(self.font_pt));
        o.insert("radius".into(), json!(self.radius));
        o.insert("popup_alpha".into(), json!(self.popup_alpha));
        o.insert("ui_fonts".into(), json!(self.ui_fonts));
        o.insert("mono_fonts".into(), json!(self.mono_fonts));
        o.insert("classic_buttons".into(), json!(self.classic_buttons));
        Value::Object(o)
    }

    /// `c` pushed toward white by `t` (for the bright terminal colours).
    pub fn lighten(c: Rgb, t: f32) -> Rgb {
        Rgb(255, 255, 255).over(c, t)
    }
}

// ---------------------------------------------------------------- config

/// `"system"` in decebal.json.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The master switch. Theme propagation is automatic unless explicitly disabled.
    pub enabled: bool,
    /// Per-target overrides of the defaults.
    pub targets: BTreeMap<String, bool>,
    /// Optional: names of the icon and cursor theme to set everywhere. Unset leaves them alone.
    pub icon_theme: Option<String>,
    pub cursor_theme: Option<String>,
    /// Also send the reload signals Omarchy uses for kitty (SIGUSR1), ghostty/btop (SIGUSR2) and helix (SIGUSR1).
    pub signals: bool,
    /// Files follows the desktop family by default; explicit visual profiles survive theme changes.
    pub file_manager_style: file_manager::Style,
    /// Per-target options that differ from their default (see [`OPTIONS`]), `"options"` in the JSON.
    pub options: BTreeMap<String, bool>,
}

/// The per-target on/off options: `(key, default, title, detail)`. Only a value that differs from the default is stored.
pub const OPTIONS: &[(&str, bool, &str, &str)] = &[
    ("gtk.font", true, "Theme font in GTK apps", "Write the theme's interface and monospace fonts (the first installed ones of their chains) to GTK's settings.ini and gsettings; off leaves your GTK fonts alone"),
    (
        "gtk.theme",
        true,
        "Theme GTK 3 and GTK 4 controls",
        "Select a generated Decebal GTK theme so plain GTK 3 / GTK 4 apps get the theme's buttons, fields, header bars and states (libadwaita apps are coloured either way); off keeps your own GTK theme",
    ),
];

impl Default for Config {
    fn default() -> Self {
        Self { enabled: true, targets: BTreeMap::new(), icon_theme: None, cursor_theme: None, signals: false, file_manager_style: file_manager::Style::FollowTheme, options: BTreeMap::new() }
    }
}

impl Config {
    pub fn from_json(v: &Value) -> Config {
        let mut c = Config::default();
        let Some(o) = v.as_object() else { return c };
        c.enabled = o.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        if let Some(t) = o.get("targets").and_then(Value::as_object) {
            c.targets = t.iter().filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b))).collect();
        }
        let s = |k: &str| o.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
        c.icon_theme = s("iconTheme");
        c.cursor_theme = s("cursorTheme");
        c.file_manager_style = o.get("fileManagerStyle").and_then(Value::as_str).and_then(file_manager::Style::parse).unwrap_or_default();
        c.signals = o.get("signals").and_then(Value::as_bool).unwrap_or(false);
        if let Some(t) = o.get("options").and_then(Value::as_object) {
            c.options = t.iter().filter(|(k, _)| OPTIONS.iter().any(|o| o.0 == k.as_str())).filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b))).collect();
        }
        c
    }

    /// The value of a per-target option: the user's, else its default.
    pub fn option(&self, key: &str) -> bool {
        self.options.get(key).copied().unwrap_or_else(|| OPTIONS.iter().find(|o| o.0 == key).is_none_or(|o| o.1))
    }

    /// Sets an option; a value equal to the default is not stored.
    pub fn set_option(&mut self, key: &str, on: bool) {
        let default = OPTIONS.iter().find(|o| o.0 == key).is_none_or(|o| o.1);
        if on == default {
            self.options.remove(key);
        } else {
            self.options.insert(key.to_string(), on);
        }
    }

    pub fn to_json(&self) -> Value {
        let mut o = serde_json::Map::new();
        o.insert("enabled".into(), json!(self.enabled));
        o.insert("fileManagerStyle".into(), json!(self.file_manager_style.id()));
        if !self.targets.is_empty() {
            o.insert("targets".into(), json!(self.targets));
        }
        if let Some(i) = &self.icon_theme {
            o.insert("iconTheme".into(), json!(i));
        }
        if let Some(i) = &self.cursor_theme {
            o.insert("cursorTheme".into(), json!(i));
        }
        if self.signals {
            o.insert("signals".into(), json!(true));
        }
        if !self.options.is_empty() {
            o.insert("options".into(), json!(self.options));
        }
        Value::Object(o)
    }

    pub fn load(shell_json: &Path) -> Config {
        Config::from_json(&crate::settings::io::json_load(shell_json).get("system").cloned().unwrap_or_default())
    }

    pub fn save(&self, shell_json: &Path) -> Result<(), String> {
        crate::settings::io::json_set(shell_json, "system", Some(self.to_json()))
    }

    /// Is `target` switched on: the user's choice, else its default for this machine.
    pub fn is_on(&self, t: &dyn Target, env: &Env) -> bool {
        self.targets.get(t.id()).copied().unwrap_or_else(|| t.default_on(env))
    }
}

// ---------------------------------------------------------------- environment

/// Output of a finished command.
#[derive(Debug, Clone, Default)]
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Where commands go. The real one starts processes; tests record the calls.
pub trait Runner {
    fn which(&self, cmd: &str) -> Option<PathBuf>;
    fn run(&self, cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Out;
}

pub struct SysRunner;

impl Runner for SysRunner {
    fn which(&self, cmd: &str) -> Option<PathBuf> {
        if cmd.contains('/') {
            return Some(PathBuf::from(cmd)).filter(|p| p.exists());
        }
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(cmd)).find(|p| p.is_file()))
    }

    fn run(&self, cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Out {
        use std::process::{Command, Stdio};
        let mut c = Command::new(cmd);
        c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in env {
            c.env(k, v);
        }
        let child = match c.spawn() {
            Ok(c) => c,
            Err(e) => return Out { ok: false, stdout: String::new(), stderr: format!("{cmd}: {e}") },
        };
        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        match rx.recv_timeout(Duration::from_secs(40)) {
            Ok(Ok(o)) => Out { ok: o.status.success(), stdout: String::from_utf8_lossy(&o.stdout).into_owned(), stderr: String::from_utf8_lossy(&o.stderr).into_owned() },
            Ok(Err(e)) => Out { ok: false, stdout: String::new(), stderr: e.to_string() },
            Err(_) => {
                // only the child started here
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                Out { ok: false, stdout: String::new(), stderr: format!("{cmd}: timed out") }
            }
        }
    }
}

/// Everything the targets need to know about this machine.
pub struct Env {
    pub home: PathBuf,
    pub config_home: PathBuf,
    /// `$XDG_DATA_HOME`: where generated toolkit themes (`themes/Decebal*`) live.
    pub data_home: PathBuf,
    pub state_home: PathBuf,
    /// Manifest, backups and the lock.
    pub state_dir: PathBuf,
    /// Files decebal generates for other programs to include.
    pub gen_dir: PathBuf,
    pub omarchy_path: Option<PathBuf>,
    pub display: Option<String>,
    pub session_bus: bool,
    /// `GSETTINGS_BACKEND` is not dconf.
    pub other_backend: bool,
    pub qpa_theme: Option<String>,
    pub runtime_dir: Option<PathBuf>,
    pub run: Box<dyn Runner>,
}

impl Env {
    pub fn discover() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("HOME").unwrap_or_else(|| PathBuf::from("/nonexistent"));
        let config_home = var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let state_home = var("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
        let data_home = var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
        let omarchy_path =
            var("OMARCHY_PATH").filter(|p| p.is_dir()).or_else(|| ["/usr/share/omarchy".into(), home.join(".local/share/omarchy")].into_iter().find(|p: &PathBuf| p.join("default/themed").is_dir()));
        Env {
            state_dir: var("DECEBAL_SYSTEM_STATE").unwrap_or_else(|| state_home.join("decebal/system")),
            gen_dir: config_home.join("decebal/system"),
            omarchy_path,
            display: std::env::var("DISPLAY").ok().filter(|d| !d.is_empty()),
            session_bus: std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty()),
            other_backend: std::env::var("GSETTINGS_BACKEND").is_ok_and(|b| !b.is_empty() && b != "dconf"),
            qpa_theme: std::env::var("QT_QPA_PLATFORMTHEME").ok().filter(|v| !v.is_empty()),
            runtime_dir: var("XDG_RUNTIME_DIR"),
            home,
            config_home,
            data_home,
            state_home,
            run: Box::new(SysRunner),
        }
    }

    pub fn has(&self, cmd: &str) -> bool {
        self.run.which(cmd).is_some()
    }

    /// gsettings can be used: a command and either a session bus or a backend that needs none.
    pub fn gsettings_ready(&self) -> bool {
        self.has("gsettings") && (self.session_bus || self.other_backend)
    }
}

// ---------------------------------------------------------------- targets

/// What a target wants to change.
#[derive(Debug, Default)]
pub struct Plan {
    pub edits: Vec<Edit>,
    /// `(schema, key, GVariant text)`
    pub gsettings: Vec<(String, String, String)>,
}

/// What `apply` did, for the command line and the settings pane.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub applied: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub wrote: Vec<PathBuf>,
    pub unchanged: usize,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct Ctx<'a> {
    pub p: &'a Palette,
    pub cfg: &'a Config,
    pub env: &'a Env,
    pub st: &'a mut State,
    pub report: &'a mut Report,
}

pub trait Target {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn detail(&self) -> &'static str;
    /// `Err(reason)` when this machine has nothing for the target to do (the target is then skipped).
    fn available(&self, env: &Env) -> Result<(), String>;
    fn default_on(&self, env: &Env) -> bool {
        self.available(env).is_ok()
    }
    fn plan(&self, _p: &Palette, _cfg: &Config, _env: &Env) -> Plan {
        Plan::default()
    }
    /// Commands to run once the files are written (reloads, `omarchy-theme-set`, ...).
    fn after(&self, _cx: &mut Ctx) {}
    /// Undo what [`Target::after`] and gsettings did that files cannot (run before the files are restored).
    fn undo(&self, _cx: &mut Ctx) {}
}

pub fn targets() -> Vec<Box<dyn Target>> {
    vec![
        Box::new(omarchy::Omarchy::new(omarchy::Kind::Core)),
        Box::new(omarchy::Omarchy::new(omarchy::Kind::Browser)),
        Box::new(omarchy::Omarchy::new(omarchy::Kind::Editors)),
        Box::new(omarchy::Omarchy::new(omarchy::Kind::Hooks)),
        Box::new(gtk::Gtk),
        Box::new(qt::Qt),
        Box::new(qt::Kvantum),
        Box::new(x11::X11),
        Box::new(terminals::Terminals),
    ]
}

// ---------------------------------------------------------------- gsettings / xrdb bookkeeping

fn dconf_path(schema: &str, key: &str) -> String {
    format!("/{}/{}", schema.replace('.', "/"), key)
}

/// Sets gsettings keys, remembering what they held (once per key) for [`gsettings_restore`].
pub fn gsettings_set(cx: &mut Ctx, owner: &str, entries: &[(String, String, String)]) {
    gsettings_write(cx.env, cx.st, cx.report, owner, entries);
}

/// [`gsettings_set`] without a palette context (the icon / cursor publication has none).
fn gsettings_write(env: &Env, st: &mut State, report: &mut Report, owner: &str, entries: &[(String, String, String)]) {
    struct Cx<'a> {
        env: &'a Env,
        st: &'a mut State,
        report: &'a mut Report,
    }
    let cx = &mut Cx { env, st, report };
    if entries.is_empty() {
        return;
    }
    if !cx.env.gsettings_ready() {
        cx.report.notes.push(format!("{owner}: no gsettings session; settings.ini is enough for GTK apps started from now on"));
        return;
    }
    let mut saved = cx.st.extra.get("gsettings").and_then(Value::as_object).cloned().unwrap_or_default();
    let listed = cx.env.run.run("gsettings", &["list-schemas"], &[]).stdout;
    for (schema, key, val) in entries {
        if !listed.lines().any(|l| l.trim() == schema) {
            continue;
        }
        let id = format!("{schema} {key}");
        let cur = cx.env.run.run("gsettings", &["get", schema, key], &[]);
        if !cur.ok {
            continue;
        }
        let cur = cur.stdout.trim().to_string();
        if !saved.contains_key(&id) {
            let was_default = !cx.env.other_backend && cx.env.has("dconf") && cx.env.run.run("dconf", &["read", &dconf_path(schema, key)], &[]).stdout.trim().is_empty();
            saved.insert(id.clone(), json!({"owner": owner, "old": if was_default { Value::Null } else { json!(cur) }}));
        }
        if cur != *val {
            let r = cx.env.run.run("gsettings", &["set", schema, key, val], &[]);
            if !r.ok {
                cx.report.warnings.push(format!("gsettings set {schema} {key}: {}", r.stderr.trim()));
            }
        }
    }
    cx.st.extra.insert("gsettings".into(), Value::Object(saved));
}

/// Puts the gsettings keys of `owner` back (reset when they were at their default).
pub fn gsettings_restore(cx: &mut Ctx, owner: &str) {
    let Some(mut saved) = cx.st.extra.get("gsettings").and_then(Value::as_object).cloned() else { return };
    let ids: Vec<String> = saved.iter().filter(|(_, v)| v["owner"] == owner).map(|(k, _)| k.clone()).collect();
    for id in ids {
        let (schema, key) = id.split_once(' ').unwrap_or((&id, ""));
        if cx.env.gsettings_ready() {
            let r = match saved[&id]["old"].as_str() {
                Some(old) => cx.env.run.run("gsettings", &["set", schema, key, old], &[]),
                None => cx.env.run.run("gsettings", &["reset", schema, key], &[]),
            };
            if !r.ok {
                cx.report.warnings.push(format!("gsettings restore {schema} {key}: {}", r.stderr.trim()));
            }
        }
        saved.remove(&id);
    }
    if saved.is_empty() {
        cx.st.extra.remove("gsettings");
    } else {
        cx.st.extra.insert("gsettings".into(), Value::Object(saved));
    }
}

// ---------------------------------------------------------------- the pipeline

fn lock(dir: &Path) -> Result<std::fs::File, String> {
    use std::os::fd::AsRawFd;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("lock")).map_err(|e| e.to_string())?;
    unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
    Ok(f)
}

#[derive(Debug, Default, Clone)]
pub struct Opts {
    /// Compute and report, write nothing.
    pub dry_run: bool,
    /// Only these targets (ids).
    pub only: Option<Vec<String>>,
}

/// Why `t` would not run: switched off, missing on this machine, or its Omarchy base is off.
fn skip_reason(t: &dyn Target, cfg: &Config, env: &Env, opts: &Opts) -> Option<String> {
    if let Some(only) = &opts.only {
        if !only.iter().any(|o| o == t.id()) {
            return Some("not selected".into());
        }
    }
    if !cfg.is_on(t, env) {
        return Some("off".into());
    }
    if let Err(e) = t.available(env) {
        return Some(e);
    }
    if matches!(t.id(), "browser" | "editors" | "hooks") {
        let core = omarchy::Omarchy::new(omarchy::Kind::Core);
        if !cfg.is_on(&core, env) || core.available(env).is_err() {
            return Some("needs the Omarchy theme target".into());
        }
    }
    None
}

/// Renders the theme to every enabled target. Files are all computed first and written together (rolled back if one
/// fails); then gsettings and the reload commands run.
pub fn apply(env: &Env, cfg: &Config, p: &Palette, opts: &Opts) -> Result<Report, String> {
    let _lock = if opts.dry_run { None } else { Some(lock(&env.state_dir)?) };
    apply_locked(env, cfg, p, opts)
}

/// [`apply`] for a caller that already holds the system lock (the debounced worker claims its request under it).
fn apply_locked(env: &Env, cfg: &Config, p: &Palette, opts: &Opts) -> Result<Report, String> {
    let mut report = Report::default();
    let mut st = State::load(&env.state_dir);
    let all = targets();
    let mut changes = Vec::new();
    let mut plans: Vec<(&dyn Target, Plan)> = Vec::new();
    for t in &all {
        let off_selected = opts.only.as_ref().is_some_and(|o| !o.iter().any(|x| x == t.id()));
        if let Some(why) = skip_reason(t.as_ref(), cfg, env, opts) {
            if why != "not selected" {
                report.skipped.push((t.id().into(), why));
            }
            // a target switched off (not merely skipped by --only) gives its files back
            if !opts.dry_run && !off_selected && st.files.values().any(|e| e.owner == t.id()) {
                let mut cx = Ctx { p, cfg, env, st: &mut st, report: &mut report };
                t.undo(&mut cx);
                files::restore(&env.state_dir, cx.st, Some(t.id()));
            }
            continue;
        }
        let plan = t.plan(p, cfg, env);
        let mut ch = files::plan(&plan.edits);
        for c in &mut ch {
            c.owner = t.id().into();
        }
        changes.extend(ch);
        plans.push((t.as_ref(), plan));
    }
    report.unchanged = changes.iter().filter(|c| c.is_noop()).count();
    if opts.dry_run {
        for c in changes.iter().filter(|c| !c.is_noop()) {
            report.wrote.push(c.path.clone());
        }
        report.applied = plans.iter().map(|(t, _)| t.id().to_string()).collect();
        return Ok(report);
    }
    report.wrote = files::commit(&changes, &env.state_dir, &mut st)?;
    for (t, plan) in &plans {
        let mut cx = Ctx { p, cfg, env, st: &mut st, report: &mut report };
        gsettings_set(&mut cx, t.id(), &plan.gsettings);
        t.after(&mut cx);
        cx.report.applied.push(t.id().to_string());
    }
    st.extra.insert("theme".into(), json!(p.id));
    st.extra.insert("dark".into(), json!(p.dark));
    st.save(&env.state_dir)?;
    if let Some(q) = &env.qpa_theme {
        if !(q.contains("qt5ct") || q.contains("qt6ct") || q.contains("kvantum")) && plans.iter().any(|(t, _)| t.id() == "qt") {
            report.warnings.push(format!("QT_QPA_PLATFORMTHEME is `{q}`; Qt apps read the qt5ct/qt6ct scheme only with QT_QPA_PLATFORMTHEME=qt6ct (qt5ct for Qt 5)"));
        }
    } else if plans.iter().any(|(t, _)| t.id() == "qt") {
        report.warnings.push("QT_QPA_PLATFORMTHEME is not set; export QT_QPA_PLATFORMTHEME=qt6ct (qt5ct for Qt 5) in your session environment for Qt apps to read the scheme".into());
    }
    let completion = json!({"error": null, "warnings": report.warnings, "applied": report.applied, "theme": p.id});
    files::write_atomic(&env.state_dir.join("last-result.json"), &completion.to_string())?;
    Ok(report)
}

/// Gives everything back: files from their backups, gsettings keys, the previous Omarchy theme.
pub fn off(env: &Env, cfg: &Config, p: &Palette) -> Result<Report, String> {
    let _lock = lock(&env.state_dir)?;
    let mut st = State::load(&env.state_dir);
    let mut report = Report::default();
    for t in targets().iter().rev() {
        let mut cx = Ctx { p, cfg, env, st: &mut st, report: &mut report };
        t.undo(&mut cx);
    }
    for (path, how) in files::restore(&env.state_dir, &mut st, None) {
        match how {
            files::Restored::Exact => report.wrote.push(PathBuf::from(path)),
            files::Restored::BlockRemoved => {
                report.notes.push(format!("{path}: edited since; only the decebal block was removed"));
                report.wrote.push(PathBuf::from(path));
            }
            files::Restored::Kept => report.warnings.push(format!("{path}: edited since decebal wrote it, left as it is")),
        }
    }
    st.extra.clear();
    let _ = std::fs::remove_file(env.state_dir.join("last-result.json"));
    // An explicit "off" drops queued requests: a worker that wakes later finds no ticket and writes nothing.
    let _ = with_queue(env, |q| q.pending = None);
    if st.is_empty() && st.dirs.is_empty() {
        let _ = std::fs::remove_file(env.state_dir.join("manifest.json"));
        let _ = std::fs::remove_dir(env.state_dir.join("backups"));
    } else {
        st.save(&env.state_dir)?;
    }
    Ok(report)
}

/// One line per target for `status` and the settings pane.
#[derive(Debug, Clone)]
pub struct TargetStatus {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub on: bool,
    /// `Some(reason)` when it cannot run here.
    pub unavailable: Option<String>,
    pub files: usize,
}

pub fn status(env: &Env, cfg: &Config) -> Vec<TargetStatus> {
    let st = State::load(&env.state_dir);
    targets()
        .iter()
        .map(|t| TargetStatus {
            id: t.id(),
            title: t.title(),
            detail: t.detail(),
            on: cfg.is_on(t.as_ref(), env),
            unavailable: skip_reason(t.as_ref(), cfg, env, &Opts::default()).filter(|r| r != "off").or_else(|| t.available(env).err()),
            files: st.files.values().filter(|e| e.owner == t.id()).count(),
        })
        .collect()
}

/// The theme decebal last applied to the system, if any.
pub fn applied_theme(env: &Env) -> Option<String> {
    State::load(&env.state_dir).extra.get("theme").and_then(Value::as_str).map(String::from)
}

// ---------------------------------------------------------------- entry points

/// Resolves the current selection into a palette.
pub fn current_palette(src: &Source) -> Result<(Palette, Selection), String> {
    let sel = src.selection();
    let (t, problem) = src.build(&sel, system_dark());
    match problem {
        Some(p) => Err(p),
        None => Ok((Palette::from_theme(&t), sel)),
    }
}

/// The system configuration as the targets see it: an explicit `system.iconTheme` wins, otherwise the theme's shell icon pack
/// (`icons.theme`) is carried to apps too.
fn effective_config(shell_json: &Path) -> Config {
    let mut cfg = Config::load(shell_json);
    if cfg.icon_theme.is_none() {
        cfg.icon_theme = crate::settings::io::json_load(shell_json).get("icons").and_then(|i| i.get("theme")).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
    }
    cfg
}

/// `decthemectl apply-system`.
pub fn apply_now(src: &Source, opts: &Opts) -> Result<Report, String> {
    apply_now_inner(src, opts, false)
}

fn apply_now_inner(src: &Source, opts: &Opts, lock_held: bool) -> Result<Report, String> {
    let cfg = effective_config(&src.shell_json);
    let env = Env::discover();
    let result = current_palette(src).and_then(|(p, _)| if lock_held { apply_locked(&env, &cfg, &p, opts) } else { apply(&env, &cfg, &p, opts) });
    if !opts.dry_run {
        if let Err(error) = &result {
            record_failure(&env, error);
        }
    }
    result
}

#[derive(Debug, Default)]
pub struct Completion {
    pub pending: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// Honest asynchronous status for Settings: queued or running work is not a completed apply.
pub fn completion(env: &Env) -> Completion {
    let value: Value = std::fs::read_to_string(env.state_dir.join("last-result.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let busy = Queue::load(env).busy();
    Completion {
        pending: busy,
        error: if busy { None } else { value["error"].as_str().map(String::from) },
        warnings: if busy { Vec::new() } else { value["warnings"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default() },
    }
}

fn record_failure(env: &Env, error: &str) {
    let _ = files::write_atomic(&env.state_dir.join("last-result.json"), &json!({"error": error, "warnings": []}).to_string());
}

fn automatic_enabled(value: Option<&str>) -> bool {
    value != Some("0")
}

// ---------------------------------------------------------------- request ownership
//
// Producers (a theme change: [`request`]; an accepted icon / cursor selection: [`publish_assets`]) and the debounced worker
// ([`run_debounced`]) share one queue, `requests.json` in the state directory, changed only by a read-modify-write under its own
// lock (`requests.lock`). That lock is held for a few file operations and never during an apply, so a producer on a UI thread never
// waits for one.
//
// * Enqueue: a new ticket replaces the pending one and inherits its intent when wider: `Full` (every enabled target) outranks
//   `Assets` (icon / cursor keys only), so an outstanding theme apply is never narrowed, whatever the order of producers.
// * Claim: a worker takes the pending ticket only if it is still its own, moving it to `running`, and only while holding the apply
//   lock; it reads the selection after claiming, so any later change has queued a newer ticket that will run after it.
// * Finish / abandon: a worker clears `running` only when it is still its own ticket; a failed spawn drops only its own pending one.

/// What a request asks for. Ordered: merging keeps the wider one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Intent {
    /// Publish the icon / cursor selection only ([`ASSET_TARGETS`], icon and cursor keys, nothing else).
    Assets,
    /// Render the theme to every enabled target.
    Full,
}

impl Intent {
    fn id(self) -> &'static str {
        match self {
            Intent::Assets => "assets",
            Intent::Full => "full",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub nonce: String,
    pub intent: Intent,
    /// The worker running it (0 while pending); a running ticket whose process is gone no longer counts as busy.
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Queue {
    pub pending: Option<Ticket>,
    pub running: Option<Ticket>,
}

impl Queue {
    fn path(env: &Env) -> PathBuf {
        env.state_dir.join("requests.json")
    }

    fn load(env: &Env) -> Queue {
        let v: Value = std::fs::read_to_string(Self::path(env)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let ticket = |v: &Value| {
            Some(Ticket {
                nonce: v.get("nonce")?.as_str()?.to_string(),
                intent: if v.get("intent")?.as_str()? == "assets" { Intent::Assets } else { Intent::Full },
                pid: v.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32,
            })
        };
        Queue { pending: ticket(&v["pending"]), running: ticket(&v["running"]) }
    }

    fn save(&self, env: &Env) -> Result<(), String> {
        if self.pending.is_none() && self.running.is_none() {
            return match std::fs::remove_file(Self::path(env)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        let t = |t: &Option<Ticket>| t.as_ref().map(|t| json!({"nonce": t.nonce, "intent": t.intent.id(), "pid": t.pid}));
        files::write_atomic(&Self::path(env), &json!({"pending": t(&self.pending), "running": t(&self.running)}).to_string())
    }

    /// A producer's request: replaces the pending ticket, never narrowing its intent.
    pub fn enqueue(&mut self, nonce: &str, intent: Intent) {
        let intent = self.pending.as_ref().map_or(intent, |p| p.intent.max(intent));
        self.pending = Some(Ticket { nonce: nonce.to_string(), intent, pid: 0 });
    }

    /// Is `nonce` still the newest request?
    pub fn is_pending(&self, nonce: &str) -> bool {
        self.pending.as_ref().is_some_and(|p| p.nonce == nonce)
    }

    /// The worker for `nonce` takes its ticket (only if it is still the newest); `None` means superseded.
    pub fn claim(&mut self, nonce: &str, pid: u32) -> Option<Intent> {
        if !self.is_pending(nonce) {
            return None;
        }
        let mut t = self.pending.take()?;
        t.pid = pid;
        let intent = t.intent;
        self.running = Some(t);
        Some(intent)
    }

    /// The worker for `nonce` is done; a newer running ticket is never cleared.
    pub fn finish(&mut self, nonce: &str) {
        if self.running.as_ref().is_some_and(|r| r.nonce == nonce) {
            self.running = None;
        }
    }

    /// The worker for `nonce` could not be started: drop its ticket if nothing newer replaced it.
    pub fn abandon(&mut self, nonce: &str) {
        if self.is_pending(nonce) {
            self.pending = None;
        }
    }

    /// Work is queued or a live worker is running.
    pub fn busy(&self) -> bool {
        self.pending.is_some() || self.running.as_ref().is_some_and(|r| r.pid == 0 || unsafe { libc::kill(r.pid as i32, 0) } == 0)
    }
}

/// One read-modify-write of the request queue under its own short lock.
fn with_queue<R>(env: &Env, f: impl FnOnce(&mut Queue) -> R) -> Result<R, String> {
    use std::os::fd::AsRawFd;
    std::fs::create_dir_all(&env.state_dir).map_err(|e| format!("{}: {e}", env.state_dir.display()))?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(env.state_dir.join("requests.lock")).map_err(|e| e.to_string())?;
    unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) };
    let mut q = Queue::load(env);
    let r = f(&mut q);
    q.save(env)?;
    Ok(r)
}

fn new_nonce() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// Queues `intent` and starts its worker. Never blocks beyond the queue's few file operations.
fn queue_request(env: &Env, intent: Intent) -> Result<(), String> {
    let nonce = new_nonce();
    with_queue(env, |q| q.enqueue(&nonce, intent))?;
    spawn_ctl(&["apply-system", "--debounced", &nonce]);
    Ok(())
}

/// Called after any theme / style / font / wallpaper-colour change: unless app integration is explicitly off, asks for one apply
/// shortly (several changes in a row make one run). Never blocks and never fails the caller.
/// `DECEBAL_SYSTEM_SYNC=1` runs it inline instead (scripts and tests).
/// `DECEBAL_SYSTEM_AUTOMATIC=0` suppresses automatic requests before any I/O; explicit apply-system still works.
pub fn request(shell_json: &Path) {
    // Unit tests must never discover/apply the developer’s desktop, even if the caller exports SYNC=1.
    if cfg!(test) || !automatic_enabled(std::env::var("DECEBAL_SYSTEM_AUTOMATIC").ok().as_deref()) {
        return;
    }
    let cfg = Config::load(shell_json);
    if !cfg.enabled {
        return;
    }
    if std::env::var_os("DECEBAL_SYSTEM_SYNC").is_some_and(|v| v == "1") {
        let src = Source { shell_json: shell_json.to_path_buf(), themes_dir: Source::discover().themes_dir };
        let _ = apply_now(&src, &Opts::default());
        return;
    }
    let _ = queue_request(&Env::discover(), Intent::Full);
}

/// The targets an icon / cursor publication may touch: never Omarchy, browser, editors, hooks, terminals, Kvantum or X11.
pub const ASSET_TARGETS: &[&str] = &["gtk", "qt"];

/// The answer of [`publish_assets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publication {
    /// A debounced background publication was queued (merged into a pending theme apply when there is one).
    Requested,
    /// Nothing will be published, and why (an explicit opt-out, the targets are off, or automatic propagation is suppressed).
    Off(String),
}

/// The accepted icon / cursor selection in `shell_json` changed (`icons.theme`, `system.iconTheme`, `system.cursorTheme`): publish it to
/// GTK and Qt without running the rest of the pipeline. Never blocks. Writes exactly: GTK `settings.ini` `gtk-icon-theme-name` /
/// `gtk-cursor-theme-name` (gtk-3.0 and gtk-4.0), gsettings `icon-theme` / `cursor-theme`, qt5ct / qt6ct `[Appearance] icon_theme`.
///
/// * Explicit opt-outs win: `system.enabled = false` (unset means on), or the `gtk` and `qt` targets switched off, publish nothing.
///   An explicit `system.iconTheme` keeps winning over the theme's `icons.theme`. A theme that is not installed is not published.
/// * Ordering: see the request ownership protocol above. A pending theme apply keeps its full intent; the worker publishes the
///   selection current when it claims its ticket.
pub fn publish_assets(shell_json: &Path) -> Publication {
    if cfg!(test) || !automatic_enabled(std::env::var("DECEBAL_SYSTEM_AUTOMATIC").ok().as_deref()) {
        return Publication::Off("automatic app integration is suppressed in this process".into());
    }
    let env = Env::discover();
    if let Err(why) = asset_gate(&Config::load(shell_json), &env) {
        return Publication::Off(why);
    }
    match queue_request(&env, Intent::Assets) {
        Ok(()) => Publication::Requested,
        Err(e) => Publication::Off(format!("cannot queue the request: {e}")),
    }
}

/// Why an icon / cursor publication must not write anything, if it must not.
fn asset_gate(cfg: &Config, env: &Env) -> Result<(), String> {
    if !cfg.enabled {
        return Err("app integration is switched off".into());
    }
    if !targets().iter().filter(|t| ASSET_TARGETS.contains(&t.id())).any(|t| cfg.is_on(t.as_ref(), env) && t.available(env).is_ok()) {
        return Err("the GTK and Qt app targets are switched off".into());
    }
    Ok(())
}

/// The icon / cursor publication itself, under the system lock: only the asset keys of the enabled asset targets, through the same
/// file transaction (originals saved, `off` restores) and gsettings bookkeeping as a full apply.
fn publish_assets_locked(env: &Env, cfg: &Config) -> Result<Report, String> {
    let mut report = Report::default();
    if let Err(why) = asset_gate(cfg, env) {
        report.notes.push(format!("icons and cursors not published: {why}"));
        return Ok(report);
    }
    let mut st = State::load(&env.state_dir);
    let mut changes = Vec::new();
    let mut plans = Vec::new();
    for t in targets().iter().filter(|t| ASSET_TARGETS.contains(&t.id())) {
        if let Some(why) = skip_reason(t.as_ref(), cfg, env, &Opts::default()) {
            report.skipped.push((t.id().into(), why));
            continue;
        }
        let plan = match t.id() {
            "gtk" => gtk::asset_plan(cfg, env),
            _ => qt::asset_plan(cfg, env),
        };
        let mut ch = files::plan(&plan.edits);
        for c in &mut ch {
            c.owner = t.id().into();
        }
        changes.extend(ch);
        plans.push((t.id(), plan));
    }
    report.unchanged = changes.iter().filter(|c| c.is_noop()).count();
    report.wrote = files::commit(&changes, &env.state_dir, &mut st)?;
    for (id, plan) in &plans {
        gsettings_write(env, &mut st, &mut report, id, &plan.gsettings);
        report.applied.push(id.to_string());
    }
    report.warnings.extend(gtk::missing_themes(cfg, env));
    st.save(&env.state_dir)?;
    files::write_atomic(&env.state_dir.join("last-result.json"), &json!({"error": null, "warnings": report.warnings, "applied": report.applied, "intent": "assets"}).to_string())?;
    Ok(report)
}

/// Starts `decthemectl ARGS` detached (it lives next to the running program, else on PATH). Does nothing in unit tests.
pub fn spawn_ctl(args: &[&str]) {
    if cfg!(test) {
        return;
    }
    let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("decthemectl"))).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("decthemectl"));
    if let Err(error) = std::process::Command::new(exe).args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        if args.first() == Some(&"apply-system") {
            let env = Env::discover();
            record_failure(&env, &format!("Could not start app theme integration: {error}"));
            if let Some(nonce) = args.get(2) {
                let _ = with_queue(&env, |q| q.abandon(nonce));
            }
        }
    }
}

/// The child started by a producer: waits a moment, then runs its ticket only if it is still the newest (see the protocol above).
pub fn run_debounced(src: &Source, nonce: &str) -> Result<Option<Report>, String> {
    let env = Env::discover();
    std::thread::sleep(Duration::from_millis(700));
    if !with_queue(&env, |q| q.is_pending(nonce))? {
        return Ok(None);
    }
    let _apply = lock(&env.state_dir)?;
    let Some(intent) = with_queue(&env, |q| q.claim(nonce, std::process::id()))? else { return Ok(None) };
    // Everything is read from here on: a change made after this point has queued a newer ticket.
    let result = match intent {
        Intent::Full => apply_now_inner(src, &Opts::default(), true),
        Intent::Assets => publish_assets_locked(&env, &effective_config(&src.shell_json)).inspect_err(|e| record_failure(&env, e)),
    };
    let _ = with_queue(&env, |q| q.finish(nonce));
    result.map(Some)
}

// ---------------------------------------------------------------- test support

#[cfg(test)]
pub mod testutil {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Records every command; `present` are the programs `which` finds, `answers` map "cmd args" prefixes to stdout.
    #[derive(Clone, Default)]
    pub struct Fake {
        pub present: Vec<String>,
        pub calls: Rc<RefCell<Vec<String>>>,
        pub answers: Rc<RefCell<Vec<(String, String)>>>,
    }

    impl Runner for Fake {
        fn which(&self, cmd: &str) -> Option<PathBuf> {
            self.present.iter().any(|p| p == cmd).then(|| PathBuf::from("/fake").join(cmd))
        }
        fn run(&self, cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Out {
            let line = format!("{cmd} {}", args.join(" "));
            let envs: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            self.calls.borrow_mut().push(if envs.is_empty() { line.trim().to_string() } else { format!("{} [{}]", line.trim(), envs.join(" ")) });
            let ans = self.answers.borrow().iter().find(|(p, _)| line.starts_with(p.as_str())).map(|(_, a)| a.clone());
            Out { ok: true, stdout: ans.unwrap_or_default(), stderr: String::new() }
        }
    }

    /// A temp directory removed when dropped.
    pub struct TmpDir(PathBuf);

    impl std::ops::Deref for TmpDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for TmpDir {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn tmp(name: &str) -> TmpDir {
        let d = std::env::temp_dir().join(format!("decebal-system-{name}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }

    /// A private machine under `root` with the given programs "installed".
    pub fn env(root: &Path, present: &[&str]) -> (Env, Fake) {
        let fake = Fake { present: present.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        let home = root.join("home");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        let e = Env {
            config_home: home.join(".config"),
            data_home: home.join(".local/share"),
            state_home: home.join(".local/state"),
            state_dir: root.join("state"),
            gen_dir: home.join(".config/decebal/system"),
            omarchy_path: None,
            display: None,
            session_bus: false,
            other_backend: false,
            qpa_theme: None,
            runtime_dir: None,
            home,
            run: Box::new(fake.clone()),
        };
        (e, fake)
    }

    /// A fixed palette (the macOS dark theme) so renderers have golden outputs.
    pub fn palette(dark: bool) -> Palette {
        let d = crate::theme::builtin::def("macos").unwrap();
        Palette::from_theme(&d.resolve("macos", dark, &Selection::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn palette_has_every_role_for_both_modes() {
        for dark in [true, false] {
            let p = palette(dark);
            assert_eq!(p.dark, dark);
            assert!(contrast(p.text, p.bg) >= 4.5);
            assert!(contrast(p.accent_on_bg(), p.bg) >= 3.0);
            for c in [p.red, p.green, p.yellow, p.blue, p.magenta, p.cyan] {
                assert!(contrast(c, p.bg) >= 2.0, "{} on {}", c.hex(), p.bg.hex());
            }
        }
    }

    #[test]
    fn automatic_override_is_explicit_and_does_not_change_the_default() {
        assert!(automatic_enabled(None));
        assert!(automatic_enabled(Some("1")));
        assert!(!automatic_enabled(Some("0")));
        assert!(Config::default().enabled);
    }

    #[test]
    fn unit_test_request_never_discovers_or_writes_desktop_environment() {
        let d = tmp("request-noop");
        request(&d.join("decebal.json"));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    }

    #[test]
    fn completion_distinguishes_pending_failure_and_success() {
        let d = tmp("completion");
        let (env, _) = env(&d, &[]);
        with_queue(&env, |q| q.enqueue("nonce", Intent::Full)).unwrap();
        record_failure(&env, "write failed");
        let result = completion(&env);
        assert!(result.pending);
        assert!(result.error.is_none(), "a retry hides stale errors while pending");
        with_queue(&env, |q| assert_eq!(q.claim("nonce", std::process::id()), Some(Intent::Full))).unwrap();
        assert!(completion(&env).pending, "a live running worker is still pending work");
        with_queue(&env, |q| q.finish("nonce")).unwrap();
        assert!(!Queue::path(&env).exists(), "an empty queue leaves no file");
        assert_eq!(completion(&env).error.as_deref(), Some("write failed"));
        with_queue(&env, |q| {
            q.enqueue("crashed", Intent::Full);
            q.claim("crashed", u32::MAX - 1);
        })
        .unwrap();
        assert!(!completion(&env).pending, "a running ticket whose worker died is not shown as pending forever");
        with_queue(&env, |q| q.finish("crashed")).unwrap();
        files::write_atomic(&env.state_dir.join("last-result.json"), "{\"warnings\":[\"reopen GTK apps\"]}").unwrap();
        assert!(!completion(&env).pending);
        assert!(completion(&env).error.is_none());
        assert_eq!(completion(&env).warnings, ["reopen GTK apps"]);
    }

    #[test]
    fn config_round_trips_and_defaults_to_automatic_unless_explicitly_disabled() {
        assert!(Config::default().enabled);
        assert!(Config::from_json(&json!({})).enabled);
        assert!(!Config::from_json(&json!({"enabled": false})).enabled);
        let mut c = Config { enabled: true, icon_theme: Some("Papirus".into()), signals: true, ..Config::default() };
        c.targets.insert("gtk".into(), false);
        assert!(c.option("gtk.font"), "the theme font is carried to GTK unless the user says no");
        c.set_option("gtk.font", false);
        assert!(!c.option("gtk.font"));
        assert_eq!(Config::from_json(&c.to_json()), c);
        assert_eq!(Config::from_json(&json!({"options": {"nonsense": false, "gtk.font": false}})).options.len(), 1, "unknown options are dropped");
        let d = tmp("cfg");
        let f = d.join("decebal.json");
        std::fs::write(&f, "{\"bar\":{\"enabled\":true}}\n").unwrap();
        c.save(&f).unwrap();
        assert_eq!(Config::load(&f), c);
        assert!(std::fs::read_to_string(&f).unwrap().contains("\"bar\""), "other keys kept");
    }

    #[test]
    fn file_manager_profile_round_trip_and_invalid_fallback() {
        for style in ["theme", "native", "explorer", "finder"] {
            let c = Config::from_json(&json!({"fileManagerStyle": style}));
            assert_eq!(c.file_manager_style.id(), style);
            assert_eq!(Config::from_json(&c.to_json()), c);
        }
        for value in [json!(null), json!(42), json!("unknown"), json!("Finder")] {
            assert_eq!(Config::from_json(&json!({"fileManagerStyle": value})).file_manager_style, file_manager::Style::FollowTheme);
        }
    }

    #[test]
    fn absent_file_manager_preference_follows_theme_without_rewriting_explicit_profiles() {
        assert_eq!(Config::default().file_manager_style, file_manager::Style::FollowTheme);
        assert_eq!(Config::from_json(&json!({})).file_manager_style, file_manager::Style::FollowTheme);
        assert_eq!(Config::from_json(&json!({"fileManagerStyle":"follow-theme"})).to_json()["fileManagerStyle"], "theme");
        for style in ["native", "explorer", "finder"] {
            let cfg = Config::from_json(&json!({"fileManagerStyle":style}));
            assert_eq!(cfg.to_json()["fileManagerStyle"], style);
        }
    }

    #[test]
    fn missing_apps_are_skipped_and_reported() {
        let d = tmp("skip");
        let (env, _) = env(&d, &[]);
        let cfg = Config { enabled: true, ..Config::default() };
        let r = apply(&env, &cfg, &palette(true), &Opts::default()).unwrap();
        let skipped: Vec<&str> = r.skipped.iter().map(|(t, _)| t.as_str()).collect();
        for t in ["omarchy", "qt", "x11"] {
            assert!(skipped.contains(&t), "{t} should be skipped without its program: {skipped:?}");
        }
        assert!(r.applied.contains(&"gtk".to_string()), "gtk files need no program");
    }

    #[test]
    fn apply_off_round_trip_restores_everything_and_apply_is_idempotent() {
        let d = tmp("roundtrip");
        let (env, fake) = env(&d, &["qt6ct", "xrdb"]);
        let gtk4 = env.config_home.join("gtk-4.0/gtk.css");
        std::fs::create_dir_all(gtk4.parent().unwrap()).unwrap();
        std::fs::write(&gtk4, "/* mine */\nbutton { color: red; }\n").unwrap();
        let settings = env.config_home.join("gtk-3.0/settings.ini");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "[Settings]\ngtk-font-name=Old 10\n").unwrap();
        let before: Vec<_> = [&gtk4, &settings].iter().map(|p| std::fs::read(p).unwrap()).collect();
        let cfg = Config { enabled: true, ..Config::default() };
        let p = palette(true);
        let r1 = apply(&env, &cfg, &p, &Opts::default()).unwrap();
        assert!(r1.wrote.len() >= 5, "{:?}", r1.wrote);
        assert!(std::fs::read_to_string(&gtk4).unwrap().contains("window_bg_color"));
        let snapshot: Vec<Vec<u8>> = r1.wrote.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let r2 = apply(&env, &cfg, &p, &Opts::default()).unwrap();
        assert!(r2.wrote.is_empty(), "second apply changes nothing: {:?}", r2.wrote);
        let snapshot2: Vec<Vec<u8>> = r1.wrote.iter().map(|p| std::fs::read(p).unwrap()).collect();
        assert_eq!(snapshot, snapshot2);
        // a different theme rewrites; its originals stay the first ones
        let p2 = {
            let d = crate::theme::builtin::def("nord").unwrap();
            Palette::from_theme(&d.resolve("nord", true, &Selection::default()))
        };
        let r3 = apply(&env, &cfg, &p2, &Opts::default()).unwrap();
        assert!(!r3.wrote.is_empty());
        let _ = fake;
        let r = off(&env, &cfg, &p2).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(std::fs::read(&gtk4).unwrap(), before[0]);
        assert_eq!(std::fs::read(&settings).unwrap(), before[1]);
        for w in &r1.wrote {
            if !before_exists(w, &[&gtk4, &settings]) {
                assert!(!w.exists(), "{} should be gone", w.display());
            }
        }
        assert!(!env.state_dir.join("manifest.json").exists());
    }

    fn before_exists(p: &Path, known: &[&PathBuf]) -> bool {
        known.iter().any(|k| k.as_path() == p)
    }

    #[test]
    fn a_target_switched_off_gives_its_files_back() {
        let d = tmp("pertarget");
        let (env, _) = env(&d, &["qt6ct"]);
        let mut cfg = Config { enabled: true, ..Config::default() };
        let p = palette(true);
        apply(&env, &cfg, &p, &Opts::default()).unwrap();
        let qt = env.config_home.join("qt6ct/colors/decebal.conf");
        let gtk = env.config_home.join("gtk-4.0/gtk.css");
        assert!(qt.exists() && gtk.exists());
        cfg.targets.insert("qt".into(), false);
        let r = apply(&env, &cfg, &p, &Opts::default()).unwrap();
        assert!(!qt.exists(), "qt files removed when the qt box is unchecked");
        assert!(gtk.exists(), "gtk untouched");
        assert!(r.skipped.iter().any(|(t, why)| t == "qt" && why == "off"));
    }

    /// Every order of producers: the pending intent is the widest one asked for since the last claim.
    #[test]
    fn enqueue_never_narrows_a_pending_intent_in_any_order() {
        let seqs: [&[Intent]; 6] = [
            &[Intent::Full, Intent::Assets],
            &[Intent::Assets, Intent::Full],
            &[Intent::Assets, Intent::Assets],
            &[Intent::Full, Intent::Full],
            &[Intent::Assets, Intent::Full, Intent::Assets],
            &[Intent::Full, Intent::Assets, Intent::Assets],
        ];
        for seq in seqs {
            let mut q = Queue::default();
            for (i, intent) in seq.iter().enumerate() {
                q.enqueue(&format!("n{i}"), *intent);
            }
            let want = *seq.iter().max().unwrap();
            assert_eq!(q.pending.as_ref().map(|t| (t.nonce.as_str(), t.intent)), Some((format!("n{}", seq.len() - 1).as_str(), want)), "{seq:?}");
        }
    }

    /// Root's scenario: an asset change and a theme change race; whichever worker survives still runs every target.
    #[test]
    fn a_theme_change_racing_an_asset_publication_keeps_its_full_intent() {
        let mut q = Queue::default();
        q.enqueue("full-1", Intent::Full); // theme change
        q.enqueue("assets-2", Intent::Assets); // asset follow completes right after
        assert_eq!(q.claim("full-1", 10), None, "the older worker is superseded");
        assert_eq!(q.claim("assets-2", 11), Some(Intent::Full), "the surviving worker inherits the full intent");
        // and the other order
        let mut q = Queue::default();
        q.enqueue("assets-1", Intent::Assets);
        q.enqueue("full-2", Intent::Full);
        assert_eq!(q.claim("assets-1", 10), None);
        assert_eq!(q.claim("full-2", 11), Some(Intent::Full));
    }

    /// A request arriving while a worker runs is kept for the next worker; finishing never clears someone else's ticket.
    #[test]
    fn claim_finish_and_abandon_only_touch_their_own_ticket() {
        let mut q = Queue::default();
        q.enqueue("a", Intent::Full);
        assert_eq!(q.claim("a", 1), Some(Intent::Full));
        assert_eq!(q.claim("a", 1), None, "a ticket is claimed once");
        q.enqueue("b", Intent::Assets);
        assert_eq!(q.pending.as_ref().unwrap().intent, Intent::Assets, "the running full apply read the selection before b: an icon-only follow-up is enough");
        q.finish("b");
        assert!(q.running.is_some(), "b is not running: a's ticket stays");
        q.finish("a");
        assert!(q.running.is_none() && q.is_pending("b"), "a's end leaves b queued");
        q.enqueue("c", Intent::Assets);
        q.abandon("b");
        assert!(q.is_pending("c"), "a stale spawn failure never drops a newer request");
        q.abandon("c");
        assert!(q.pending.is_none());
    }

    /// Many producers on threads through the real lock and file: the queue stays valid and the full intent survives.
    #[test]
    fn concurrent_producers_through_the_file_lock_keep_one_widest_ticket() {
        let d = tmp("queue-threads");
        let (env, _) = env(&d, &[]);
        let env = std::sync::Arc::new(env.state_dir.clone());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let (dir, barrier) = (env.clone(), barrier.clone());
                std::thread::spawn(move || {
                    let (e, _) = super::testutil::env(dir.parent().unwrap(), &[]);
                    barrier.wait();
                    with_queue(&e, |q| q.enqueue(&format!("t{i}"), if i == 3 { Intent::Full } else { Intent::Assets })).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let (e, _) = super::testutil::env(&d, &[]);
        let q = Queue::load(&e);
        assert_eq!(q.pending.map(|t| t.intent), Some(Intent::Full), "one full producer among eight: the queue keeps full");
    }

    #[test]
    fn asset_publication_respects_explicit_opt_outs() {
        let d = tmp("assetgate");
        let (env, _) = env(&d, &["qt6ct"]);
        let mut cfg = Config::default();
        assert_eq!(asset_gate(&cfg, &env), Ok(()), "unset master switch means on");
        cfg.targets.insert("gtk".into(), false);
        assert_eq!(asset_gate(&cfg, &env), Ok(()), "qt is still on when installed");
        cfg.targets.insert("qt".into(), false);
        assert!(asset_gate(&cfg, &env).is_err(), "both asset targets off: nothing");
        cfg.targets.clear();
        cfg.enabled = false;
        assert!(asset_gate(&cfg, &env).is_err(), "explicit opt-out");
        let r = publish_assets_locked(&env, &cfg).unwrap();
        assert!(r.wrote.is_empty() && !env.config_home.join("gtk-3.0").exists(), "an opted-out publication writes nothing");
        assert_eq!(publish_assets(&d.join("decebal.json")), Publication::Off("automatic app integration is suppressed in this process".into()));
        assert!(!ASSET_TARGETS.iter().any(|t| ["omarchy", "browser", "editors", "hooks", "terminals", "kvantum", "x11"].contains(t)));
    }

    /// The icon / cursor publication changes those keys and nothing else: custom CSS, fonts, GTK theme and Qt palette stay.
    #[test]
    fn an_icons_only_publication_writes_only_icon_and_cursor_keys() {
        let d = tmp("assetkeys");
        let (mut env, fake) = env(&d, &["qt6ct", "xrdb", "gsettings"]);
        env.other_backend = true;
        fake.answers.borrow_mut().push(("gsettings list-schemas".into(), "org.gnome.desktop.interface\n".into()));
        let icons = env.data_home.join("icons/Papirus");
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(icons.join("index.theme"), "[Icon Theme]\nName=Papirus\n").unwrap();
        std::fs::create_dir_all(env.home.join(".icons/Bibata/cursors")).unwrap();
        let css = env.config_home.join("gtk-4.0/gtk.css");
        let ini = env.config_home.join("gtk-3.0/settings.ini");
        let qt = env.config_home.join("qt6ct/qt6ct.conf");
        for f in [&css, &ini, &qt] {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        }
        std::fs::write(&css, "/* mine */\nwindow { background: red; }\n").unwrap();
        std::fs::write(&ini, "[Settings]\ngtk-theme-name=Orchis\ngtk-font-name=My Font 12\ngtk-icon-theme-name=Old\n").unwrap();
        std::fs::write(&qt, "[Appearance]\ncolor_scheme_path=/mine.conf\nstyle=kvantum\n").unwrap();
        let cfg = Config { icon_theme: Some("Papirus".into()), cursor_theme: Some("Bibata".into()), ..Config::default() };
        let r = publish_assets_locked(&env, &cfg).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(std::fs::read_to_string(&css).unwrap(), "/* mine */\nwindow { background: red; }\n", "user CSS untouched");
        assert_eq!(std::fs::read_to_string(&ini).unwrap(), "[Settings]\ngtk-theme-name=Orchis\ngtk-font-name=My Font 12\ngtk-icon-theme-name=Papirus\ngtk-cursor-theme-name=Bibata\n");
        assert_eq!(std::fs::read_to_string(&qt).unwrap(), "[Appearance]\ncolor_scheme_path=/mine.conf\nstyle=kvantum\nicon_theme=Papirus\n");
        assert!(!env.data_home.join("themes").exists() && !env.config_home.join("qt6ct/colors").exists(), "no generated theme or colour scheme");
        let calls = fake.calls.borrow().join("\n");
        assert!(calls.contains("gsettings set org.gnome.desktop.interface icon-theme 'Papirus'") && calls.contains("cursor-theme 'Bibata'"), "{calls}");
        for other in ["gtk-theme", "color-scheme", "font-name", "button-layout", "xrdb"] {
            assert!(!calls.contains(other), "{other} must not be touched:\n{calls}");
        }
        let r = off(&env, &cfg, &palette(true)).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(std::fs::read_to_string(&ini).unwrap().contains("gtk-icon-theme-name=Old"), "off restores the original keys");
    }

    #[test]
    fn dry_run_writes_nothing() {
        let d = tmp("dry");
        let (env, _) = env(&d, &[]);
        let cfg = Config { enabled: true, ..Config::default() };
        let r = apply(&env, &cfg, &palette(false), &Opts { dry_run: true, ..Default::default() }).unwrap();
        assert!(!r.wrote.is_empty());
        assert!(!env.config_home.join("gtk-4.0").exists() && !env.state_dir.exists());
    }

    #[test]
    fn gsettings_originals_are_saved_once_and_restored() {
        let d = tmp("gsettings");
        let (mut env, fake) = env(&d, &["gsettings"]);
        env.other_backend = true;
        fake.answers.borrow_mut().push(("gsettings list-schemas".into(), "org.gnome.desktop.interface\norg.gnome.desktop.wm.preferences\n".into()));
        fake.answers.borrow_mut().push(("gsettings get org.gnome.desktop.interface color-scheme".into(), "'default'\n".into()));
        let cfg = Config { enabled: true, ..Config::default() };
        apply(&env, &cfg, &palette(true), &Opts::default()).unwrap();
        apply(&env, &cfg, &palette(true), &Opts::default()).unwrap();
        let calls = fake.calls.borrow().join("\n");
        assert!(calls.contains("gsettings set org.gnome.desktop.interface color-scheme 'prefer-dark'"), "{calls}");
        off(&env, &cfg, &palette(true)).unwrap();
        let calls = fake.calls.borrow().join("\n");
        assert!(calls.contains("gsettings set org.gnome.desktop.interface color-scheme 'default'"), "the original value goes back:\n{calls}");
    }
}
