//! "Apply to all apps": the optional pipeline that carries the decebal theme to the rest of the desktop.
//!
//! One palette ([`Palette`], taken from the resolved [`Theme`]) is rendered by small targets:
//!
//! | target | what it changes |
//! |---|---|
//! | `omarchy` | a complete Omarchy theme `~/.config/omarchy/themes/decebal` (`colors.toml`; Omarchy's own template engine makes alacritty/foot/kitty/ghostty/btop/helix/neovim/... from it), `omarchy-theme-set`, live reload of terminals and btop |
//! | `browser` | Chromium/Chrome/Edge/Brave theme colour through Omarchy's browser policy helper |
//! | `editors` | VS Code / Cursor / Obsidian through Omarchy's own sync helpers |
//! | `hooks` | the user's `~/.config/omarchy/hooks/theme-set.d` |
//! | `gtk` | `gtk-3.0` / `gtk-4.0` `gtk.css` (libadwaita named colours), `settings.ini`, gsettings (dark/light, font, button layout) |
//! | `qt` | qt5ct / qt6ct colour scheme and config |
//! | `kvantum` | a Kvantum colour theme |
//! | `x11` | an Xresources file included from `~/.Xresources` and merged with `xrdb` (st, xterm, urxvt) |
//!
//! Every file is changed atomically, the first version of each is saved, and `off` restores them ([`files`]).
//! Nothing here runs unless the user switched it on (`decthemectl system on`, or the Theme pane).

pub mod cli;
pub mod files;
pub mod gtk;
pub mod omarchy;
pub mod qt;
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
    pub font_pt: f32,
    pub radius: f32,
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
            font_pt: (11.0 * t.font_scale * 2.0).round() / 2.0,
            radius: t.radius,
            classic_buttons: t.titlebar_style == "classic",
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

    /// `c` pushed toward white by `t` (for the bright terminal colours).
    pub fn lighten(c: Rgb, t: f32) -> Rgb {
        Rgb(255, 255, 255).over(c, t)
    }
}

// ---------------------------------------------------------------- config

/// `"system"` in decebal.json.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    /// The master switch ("Apply to all apps"). Off until the user turns it on.
    pub enabled: bool,
    /// Per-target overrides of the defaults.
    pub targets: BTreeMap<String, bool>,
    /// Optional: names of the icon and cursor theme to set everywhere. Unset leaves them alone.
    pub icon_theme: Option<String>,
    pub cursor_theme: Option<String>,
    /// Also send the reload signals Omarchy uses for kitty (SIGUSR1), ghostty/btop (SIGUSR2) and helix (SIGUSR1).
    pub signals: bool,
}

impl Config {
    pub fn from_json(v: &Value) -> Config {
        let mut c = Config::default();
        let Some(o) = v.as_object() else { return c };
        c.enabled = o.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        if let Some(t) = o.get("targets").and_then(Value::as_object) {
            c.targets = t.iter().filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b))).collect();
        }
        let s = |k: &str| o.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
        c.icon_theme = s("iconTheme");
        c.cursor_theme = s("cursorTheme");
        c.signals = o.get("signals").and_then(Value::as_bool).unwrap_or(false);
        c
    }

    pub fn to_json(&self) -> Value {
        let mut o = serde_json::Map::new();
        o.insert("enabled".into(), json!(self.enabled));
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
    ]
}

// ---------------------------------------------------------------- gsettings / xrdb bookkeeping

fn dconf_path(schema: &str, key: &str) -> String {
    format!("/{}/{}", schema.replace('.', "/"), key)
}

/// Sets gsettings keys, remembering what they held (once per key) for [`gsettings_restore`].
pub fn gsettings_set(cx: &mut Ctx, owner: &str, entries: &[(String, String, String)]) {
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
    let mut st = State::load(&env.state_dir);
    let mut report = Report::default();
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

/// `decthemectl apply-system`.
pub fn apply_now(src: &Source, opts: &Opts) -> Result<Report, String> {
    let (p, _) = current_palette(src)?;
    let cfg = Config::load(&src.shell_json);
    apply(&Env::discover(), &cfg, &p, opts)
}

fn pending_file(env: &Env) -> PathBuf {
    env.state_dir.join("pending")
}

/// Called after any theme / style / font / wallpaper-colour change: when "Apply to all apps" is on, asks for one apply
/// shortly (several changes in a row make one run). Never blocks and never fails the caller.
/// `DECEBAL_SYSTEM_SYNC=1` runs it inline instead (scripts and tests).
pub fn request(shell_json: &Path) {
    let cfg = Config::load(shell_json);
    if !cfg.enabled {
        return;
    }
    if std::env::var_os("DECEBAL_SYSTEM_SYNC").is_some_and(|v| v == "1") {
        let src = Source { shell_json: shell_json.to_path_buf(), themes_dir: Source::discover().themes_dir };
        let _ = apply_now(&src, &Opts::default());
        return;
    }
    if cfg!(test) {
        return;
    }
    let env = Env::discover();
    let nonce = format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    if std::fs::create_dir_all(&env.state_dir).is_err() || std::fs::write(pending_file(&env), &nonce).is_err() {
        return;
    }
    spawn_ctl(&["apply-system", "--debounced", &nonce]);
}

/// Starts `decthemectl ARGS` detached (it lives next to the running program, else on PATH). Does nothing in unit tests.
pub fn spawn_ctl(args: &[&str]) {
    if cfg!(test) {
        return;
    }
    let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("decthemectl"))).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("decthemectl"));
    let _ = std::process::Command::new(exe).args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

/// The child started by [`request`]: waits a moment and applies only if no newer request arrived.
pub fn run_debounced(src: &Source, nonce: &str) -> Result<Option<Report>, String> {
    let env = Env::discover();
    std::thread::sleep(Duration::from_millis(700));
    if std::fs::read_to_string(pending_file(&env)).ok().as_deref() != Some(nonce) {
        return Ok(None);
    }
    let r = apply_now(src, &Opts::default())?;
    let _ = std::fs::remove_file(pending_file(&env));
    Ok(Some(r))
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
    fn config_round_trips_and_defaults_to_off() {
        assert!(!Config::default().enabled);
        let mut c = Config { enabled: true, icon_theme: Some("Papirus".into()), signals: true, ..Config::default() };
        c.targets.insert("gtk".into(), false);
        assert_eq!(Config::from_json(&c.to_json()), c);
        let d = tmp("cfg");
        let f = d.join("decebal.json");
        std::fs::write(&f, "{\"bar\":{\"enabled\":true}}\n").unwrap();
        c.save(&f).unwrap();
        assert_eq!(Config::load(&f), c);
        assert!(std::fs::read_to_string(&f).unwrap().contains("\"bar\""), "other keys kept");
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

    #[test]
    fn dry_run_writes_nothing() {
        let d = tmp("dry");
        let (env, _) = env(&d, &[]);
        let cfg = Config { enabled: true, ..Config::default() };
        let r = apply(&env, &cfg, &palette(false), &Opts { dry_run: true, only: None }).unwrap();
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
