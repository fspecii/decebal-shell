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

/// Path of `<id>.desktop` (also tried lowercase) in the first applications directory that has it.
pub fn find_desktop_file(id: &str) -> Option<PathBuf> {
    if id.is_empty() || id.contains('/') {
        return None;
    }
    for dir in app_dirs() {
        for name in [format!("{id}.desktop"), format!("{}.desktop", id.to_lowercase())] {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `StartupWMClass=` of the `[Desktop Entry]` group.
pub fn parse_wm_class(desktop: &str) -> Option<String> {
    let mut in_entry = false;
    for line in desktop.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("StartupWMClass=") {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Desktop-file id (`org.gnome.Nautilus`) for a window's app id / WM_CLASS: a file named like it, else one
/// whose `StartupWMClass` or trailing name component matches. `None` for windows without an installed app.
pub fn resolve_desktop_id(app_id: &str) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    if let Some(p) = find_desktop_file(app_id) {
        return p.file_stem().map(|s| s.to_string_lossy().into_owned());
    }
    let want = app_id.to_lowercase();
    for dir in app_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "desktop")).collect();
        files.sort();
        for path in files {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let by_name = stem.to_lowercase().ends_with(&format!(".{want}"));
            let by_class = std::fs::read_to_string(&path).ok().and_then(|t| parse_wm_class(&t)).is_some_and(|c| c.to_lowercase() == want);
            if by_name || by_class {
                return Some(stem);
            }
        }
    }
    None
}

/// Command that opens a file, folder or URL: `$DECEBAL_OPEN` (tests, custom file managers) or `xdg-open`.
pub fn open_argv(target: &str) -> Vec<String> {
    let mut argv = match std::env::var("DECEBAL_OPEN") {
        Ok(c) if !c.trim().is_empty() => exec_argv(&c),
        _ => vec!["xdg-open".to_string()],
    };
    argv.push(target.to_string());
    argv
}

/// Command that empties the trash: `$DECEBAL_TRASH_EMPTY` or `gio trash --empty`.
pub fn empty_trash_argv() -> Vec<String> {
    match std::env::var("DECEBAL_TRASH_EMPTY") {
        Ok(c) if !c.trim().is_empty() => exec_argv(&c),
        _ => vec!["gio".to_string(), "trash".to_string(), "--empty".to_string()],
    }
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

/// Values the field codes of an `Exec=` line expand to.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fields<'a> {
    pub name: &'a str,
    pub icon: &'a str,
    pub file: &'a str,
}

/// Splits an Exec line into argv, dropping `%f %U ...` field codes and honouring simple quotes.
pub fn exec_argv(exec: &str) -> Vec<String> {
    expand_exec(exec, Fields::default())
}

/// Splits an `Exec=` line into argv per the Desktop Entry spec: quotes group (with `\` escapes inside),
/// `%f %F %u %U` (and the deprecated `%d %D %n %N %v %m`) vanish because no files are passed, `%i`
/// becomes `--icon <Icon>`, `%c` the name, `%k` the desktop file, `%%` a percent sign.
pub fn expand_exec(exec: &str, f: Fields) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_q, mut has, mut code) = (false, false, false);
    let mut icon_pending = false;
    let mut chars = exec.chars().peekable();
    let flush = |out: &mut Vec<String>, cur: &mut String, has: &mut bool, code: &mut bool, icon_pending: &mut bool| {
        if *icon_pending {
            if !f.icon.is_empty() {
                out.push("--icon".to_string());
                out.push(f.icon.to_string());
            }
            *icon_pending = false;
        } else if *has || !cur.is_empty() || !*code {
            // A token that was only a field code expanding to nothing is dropped.
            if *has || !cur.is_empty() {
                out.push(std::mem::take(cur));
            }
        }
        cur.clear();
        *has = false;
        *code = false;
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_q = !in_q;
                has = true;
            }
            '\\' if in_q => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '%' => match chars.next() {
                Some('%') => cur.push('%'),
                Some('c') => {
                    cur.push_str(f.name);
                    code = true;
                }
                Some('k') => {
                    cur.push_str(f.file);
                    code = true;
                }
                Some('i') if cur.is_empty() && !has && chars.peek().is_none_or(|n| *n == ' ' || *n == '\t') => icon_pending = true,
                Some(_) => code = true,
                None => cur.push('%'),
            },
            ' ' | '\t' if !in_q => {
                if has || !cur.is_empty() || code || icon_pending {
                    flush(&mut out, &mut cur, &mut has, &mut code, &mut icon_pending);
                }
            }
            _ => cur.push(c),
        }
    }
    if has || !cur.is_empty() || code || icon_pending {
        flush(&mut out, &mut cur, &mut has, &mut code, &mut icon_pending);
    }
    out
}

/// Starts the app with desktop id `id` (or a built-in), detached from us. False if it could not be found or spawned.
pub fn launch(id: &str) -> bool {
    let argv = match id {
        "trash" => open_argv("trash:///"),
        "launchpad" => match std::env::var("DECEBAL_LAUNCHPAD") {
            Ok(cmd) if !cmd.is_empty() => exec_argv(&cmd),
            _ => vec![sibling_or_path("declaunch")],
        },
        _ => {
            let Some(file) = find_desktop_file(id) else { return false };
            let Some((mut app, _)) = std::fs::read_to_string(&file).ok().and_then(|t| crate::apps::parse_any(&t)) else { return false };
            app.file = file.to_string_lossy().into_owned();
            if app.exec.trim().is_empty() {
                // D-Bus activatable only: let GIO do the activation.
                return run(Plan { argv: vec!["gio".into(), "launch".into(), app.file.clone()], label: app.name, ..Plan::default() });
            }
            return spawn_app(&app);
        }
    };
    spawn_argv(&argv)
}

/// `name` next to our own executable when it is there (an uninstalled build), otherwise the bare name for `PATH`.
pub fn sibling_or_path(name: &str) -> String {
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join(name))).filter(|p| p.is_file()).map_or_else(|| name.to_string(), |p| p.to_string_lossy().into_owned())
}

/// Starts an application entry: `Path=` as working directory, `Terminal=true` inside a terminal, `TryExec=` honoured.
pub fn spawn_app(app: &crate::apps::AppEntry) -> bool {
    run(plan_app(app))
}

/// Starts an `Exec=` line detached from us.
pub fn spawn_exec(exec: &str) -> bool {
    spawn_argv(&exec_argv(exec))
}

pub fn spawn_argv(argv: &[String]) -> bool {
    run(Plan { argv: argv.to_vec(), label: argv.first().map(|p| base_name(p)).unwrap_or_default(), ..Plan::default() })
}

// ------------------------------------------------------------------ plan

/// What to start: the program line, a working directory and a name for error messages.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub label: String,
    /// Set when the entry cannot run (e.g. `TryExec` missing); nothing is spawned.
    pub error: Option<String>,
}

fn base_name(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

/// Whether `prog` (a path, or a name looked up in `PATH`) is an executable file.
pub fn program_exists(prog: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let ok = |p: &std::path::Path| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if prog.contains('/') {
        return ok(std::path::Path::new(prog));
    }
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| ok(&d.join(prog))))
}

/// The command line that shows `argv` in a terminal: `xdg-terminal-exec`, else `$TERMINAL`, else `rdwm-terminal`.
pub fn terminal_argv(argv: &[String]) -> Vec<String> {
    let mut v: Vec<String> = if program_exists("xdg-terminal-exec") {
        vec!["xdg-terminal-exec".into()]
    } else if let Some(t) = std::env::var("TERMINAL").ok().filter(|t| !t.is_empty() && program_exists(t)) {
        vec![t, "-e".into()]
    } else {
        vec!["rdwm-terminal".into(), "-e".into()]
    };
    v.extend(argv.iter().cloned());
    v
}

pub fn plan_app(app: &crate::apps::AppEntry) -> Plan {
    let label = if app.name.is_empty() { exec_argv(&app.exec).first().map(|p| base_name(p)).unwrap_or_default() } else { app.name.clone() };
    if !app.try_exec.is_empty() && !program_exists(&app.try_exec) {
        return Plan { label, error: Some(format!("{} is not installed", app.try_exec)), ..Plan::default() };
    }
    let mut argv = expand_exec(&app.exec, Fields { name: &app.name, icon: &app.icon, file: &app.file });
    if argv.is_empty() {
        return Plan { label, error: Some("the desktop entry has no command".into()), ..Plan::default() };
    }
    if app.terminal {
        argv = terminal_argv(&argv);
    }
    let cwd = (!app.path.is_empty()).then(|| PathBuf::from(&app.path)).filter(|p| p.is_dir());
    Plan { argv, cwd, label, error: None }
}

// ------------------------------------------------------------ platform rules

/// Which kind of session apps will run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
}

impl Session {
    /// X11 when there is no Wayland compositor to talk to.
    pub fn detect() -> Session {
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
            Session::Wayland
        } else {
            Session::X11
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    X11,
    Wayland,
    Always,
}

/// Adjusts the command line of programs that match `names` (lower-case file names; a trailing `*` is a prefix).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub names: Vec<String>,
    pub when: When,
    /// Inserted right after the program. `--key=value` replaces an existing `--key=...`.
    pub args: Vec<String>,
    /// Environment to set (`Some`) or remove (`None`).
    pub env: Vec<(String, Option<String>)>,
}

impl Rule {
    fn matches(&self, prog: &str, session: Session) -> bool {
        let ok_session = match self.when {
            When::Always => true,
            When::X11 => session == Session::X11,
            When::Wayland => session == Session::Wayland,
        };
        ok_session && self.names.iter().any(|n| n.strip_suffix('*').map_or(prog == n, |p| prog.starts_with(p)))
    }
}

/// Chromium-family browsers and Electron apps read `~/.config/*-flags.conf` (Omarchy ships `--ozone-platform=wayland`
/// there), which cannot start without a Wayland compositor. A switch given later on the command line wins over the
/// flags file (the wrappers put the file first), so `--ozone-platform=x11` fixes it; the env hint covers Electron
/// builds that choose from `ELECTRON_OZONE_PLATFORM_HINT`.
const CHROMIUM: &[&str] = &["google-chrome*", "chromium*", "chrome", "brave*", "vivaldi*", "opera*", "microsoft-edge*", "thorium*", "ungoogled-chromium*", "helium*"];
const ELECTRON: &[&str] = &[
    "electron*",
    "code",
    "code-oss",
    "code-insiders",
    "codium",
    "vscodium",
    "cursor",
    "discord*",
    "vesktop",
    "slack",
    "signal-desktop",
    "obsidian",
    "element-desktop",
    "teams-for-linux",
    "logseq*",
    "bitwarden",
    "1password",
    "notion-app*",
    "postman",
    "insomnia",
    "zed-electron",
    "t3code",
    "heroic",
    "simplenote",
];

pub fn builtin_rules() -> Vec<Rule> {
    let names = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        // Editor/automation hosts can export this; GUI Electron apps must start as apps, not Node.
        Rule { names: names(ELECTRON), when: When::Always, args: vec![], env: vec![("ELECTRON_RUN_AS_NODE".into(), None)] },
        Rule { names: names(CHROMIUM), when: When::X11, args: vec!["--ozone-platform=x11".into()], env: vec![] },
        Rule { names: names(ELECTRON), when: When::X11, args: vec!["--ozone-platform=x11".into()], env: vec![("ELECTRON_OZONE_PLATFORM_HINT".into(), Some("x11".into()))] },
    ]
}

/// A command line ready to run and the environment changes to apply to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Adjusted {
    pub argv: Vec<String>,
    /// `(name, Some(value))` sets, `(name, None)` removes.
    pub env: Vec<(String, Option<String>)>,
}

/// Index of the real program in `argv`, skipping `env` and `NAME=value` prefixes.
fn program_index(argv: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        let is_assign = a.split_once('=').is_some_and(|(k, _)| !k.is_empty() && !k.starts_with('-') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if base_name(a) == "env" || is_assign || (i > 0 && base_name(&argv[i - 1]) == "env" && a.starts_with('-')) {
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}

/// Applies platform rules and sanitises the environment: `get` reads the current environment.
/// Pure, so it is tested with a fake environment.
pub fn adjust(argv: &[String], session: Session, rules: &[Rule], get: &dyn Fn(&str) -> Option<String>) -> Adjusted {
    let mut argv = argv.to_vec();
    let mut env: Vec<(String, Option<String>)> = Vec::new();
    let set = |env: &mut Vec<(String, Option<String>)>, k: &str, v: Option<&str>| {
        env.retain(|(n, _)| n != k);
        env.push((k.to_string(), v.map(str::to_string)));
    };
    let prog = program_index(&argv).map(|i| (i, base_name(&argv[i]).to_lowercase()));
    // Our own internal variables stay with our own programs only.
    if !prog.as_ref().is_some_and(|(_, p)| crate::compat::is_own_program(p)) {
        for (k, _) in std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v))) {
            // the old MACSHELL_* names are still in the environment when the session predates the rename
            if k.starts_with("DECEBAL_") || k.starts_with("MACSHELL_") {
                set(&mut env, &k, None);
            }
        }
    }
    if session == Session::X11 {
        // Leftovers of a Wayland session make toolkits pick a backend that is not there.
        for k in ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"] {
            if get(k).is_some() {
                set(&mut env, k, None);
            }
        }
        if get("XDG_SESSION_TYPE").is_some_and(|v| v == "wayland") {
            set(&mut env, "XDG_SESSION_TYPE", Some("x11"));
        }
        for k in ["GDK_BACKEND", "QT_QPA_PLATFORM", "SDL_VIDEODRIVER", "CLUTTER_BACKEND", "ELECTRON_OZONE_PLATFORM_HINT", "MOZ_ENABLE_WAYLAND", "OZONE_PLATFORM"] {
            if get(k).is_some_and(|v| v.contains("wayland") || (k == "MOZ_ENABLE_WAYLAND" && v == "1")) {
                set(&mut env, k, None);
            }
        }
    }
    if let Some((idx, name)) = prog {
        let mut at = idx + 1;
        for rule in rules.iter().filter(|r| r.matches(&name, session)) {
            for arg in &rule.args {
                if let Some((key, _)) = arg.split_once('=').filter(|_| arg.starts_with("--")) {
                    let prefix = format!("{key}=");
                    let tail: Vec<String> = argv.split_off(at).into_iter().filter(|a| !a.starts_with(&prefix)).collect();
                    argv.extend(tail);
                } else if argv[at..].contains(arg) {
                    continue;
                }
                argv.insert(at, arg.clone());
                at += 1;
            }
            for (k, v) in &rule.env {
                set(&mut env, k, v.as_deref());
            }
        }
    }
    Adjusted { argv, env }
}

// ----------------------------------------------------------- user config

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct FileCfg {
    launch: LaunchCfg,
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct LaunchCfg {
    /// `false` turns the built-in Chromium/Electron rules off.
    builtin: bool,
    rules: Vec<RuleCfg>,
}

impl Default for LaunchCfg {
    fn default() -> Self {
        LaunchCfg { builtin: true, rules: Vec::new() }
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(serde::Deserialize)]
struct RuleCfg {
    #[serde(rename = "match")]
    names: OneOrMany,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, Option<String>>,
}

/// Rules from the `"launch"` object of `decebal.json`:
/// `{"launch": {"builtin": true, "rules": [{"match": ["myapp", "foo*"], "when": "x11", "args": ["--flag"], "env": {"K": "v", "DROP": null}}]}}`.
/// `when` is `x11` (default), `wayland` or `always`. User rules come after, and so win over, the built-in ones.
pub fn rules_from_json(text: &str) -> Vec<Rule> {
    let cfg: FileCfg = serde_json::from_str(text).unwrap_or_default();
    let mut rules = if cfg.launch.builtin { builtin_rules() } else { Vec::new() };
    for r in cfg.launch.rules {
        let names = match r.names {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        };
        let when = match r.when.as_deref().map(str::to_lowercase).as_deref() {
            Some("wayland") => When::Wayland,
            Some("always" | "any") => When::Always,
            _ => When::X11,
        };
        rules.push(Rule { names: names.into_iter().map(|n| n.to_lowercase()).collect(), when, args: r.args, env: r.env.into_iter().collect() });
    }
    rules
}

fn load_rules() -> Vec<Rule> {
    rules_from_json(&std::fs::read_to_string(crate::config::config_dir().join("decebal.json")).unwrap_or_default())
}

// --------------------------------------------------------------- running

fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("decebal"))
}

/// Appends a line to `~/.local/state/decebal/launch.log` (rotated at 256 KiB).
pub fn log(msg: &str) {
    use std::io::Write;
    let Some(dir) = state_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("launch.log");
    if file.metadata().is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::rename(&file, dir.join("launch.log.1"));
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "[{secs}] {msg}");
    }
}

/// Tells the user something went wrong: `notify-send` (or `$DECEBAL_NOTIFY`), best effort.
pub fn notify_error(label: &str, why: &str) {
    let mut argv = match std::env::var("DECEBAL_NOTIFY") {
        Ok(c) if !c.trim().is_empty() => exec_argv(&c),
        _ => vec!["notify-send".to_string(), "-a".into(), "decebal".into(), "-i".into(), "dialog-error".into(), "-u".into(), "normal".into()],
    };
    argv.push(format!("Couldn't open {label}"));
    argv.push(why.to_string());
    if let Some((p, rest)) = argv.split_first() {
        if let Ok(mut c) = Command::new(p).args(rest).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        }
    }
}

fn fail(label: &str, why: &str) -> bool {
    log(&format!("FAILED {label}: {why}"));
    notify_error(label, why);
    false
}

/// A failed start inside this long after launch is reported (apps that crash on startup).
const EARLY_EXIT: std::time::Duration = std::time::Duration::from_secs(8);

static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn run(plan: Plan) -> bool {
    use std::os::unix::process::CommandExt;
    if let Some(e) = &plan.error {
        return fail(&plan.label, e);
    }
    let get = |k: &str| std::env::var(k).ok();
    let adj = adjust(&plan.argv, Session::detect(), &load_rules(), &get);
    let Some((prog, args)) = adj.argv.split_first() else {
        return fail(&plan.label, "nothing to run");
    };
    let mut cmd = Command::new(prog);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null());
    for (k, v) in &adj.env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    if let Some(cwd) = &plan.cwd {
        cmd.current_dir(cwd);
    }
    // stderr goes to a private file that is deleted at once: we read it only if the app dies early.
    let err_path = state_dir().map(|d| d.join(format!("launch-{}-{}.err", std::process::id(), SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed))));
    let err_file = err_path.as_ref().and_then(|p| {
        std::fs::create_dir_all(p.parent()?).ok()?;
        std::fs::File::create(p).ok()
    });
    match err_file {
        Some(f) => cmd.stderr(f),
        None => cmd.stderr(Stdio::null()),
    };
    // A session of its own: the app survives the panel (or its service) restarting and gets no terminal signals.
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let started = std::time::Instant::now();
    match cmd.spawn() {
        Ok(mut child) => {
            log(&format!("started {} (pid {}): {}", plan.label, child.id(), adj.argv.join(" ")));
            let label = plan.label.clone();
            std::thread::spawn(move || {
                let status = child.wait();
                let tail = err_path
                    .as_ref()
                    .map(|p| {
                        let t = std::fs::read(p).unwrap_or_default();
                        let _ = std::fs::remove_file(p);
                        let t = &t[t.len().saturating_sub(1500)..];
                        String::from_utf8_lossy(t).trim().to_string()
                    })
                    .unwrap_or_default();
                if let Ok(st) = status {
                    if !st.success() && started.elapsed() < EARLY_EXIT {
                        let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(160).collect::<String>();
                        log(&format!("EXITED EARLY {label}: {st}\n{tail}"));
                        notify_error(&label, &format!("It quit right away ({st}). {last}"));
                    }
                }
            });
            true
        }
        Err(e) => {
            if let Some(p) = &err_path {
                let _ = std::fs::remove_file(p);
            }
            fail(&plan.label, &format!("{prog}: {e}"))
        }
    }
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
    fn wm_class_from_desktop_entry() {
        assert_eq!(parse_wm_class("[Desktop Entry]\nName=A\nStartupWMClass=Foo\n").as_deref(), Some("Foo"));
        assert_eq!(parse_wm_class("[Desktop Action x]\nStartupWMClass=No\n"), None);
    }

    #[test]
    fn open_and_trash_commands_can_be_overridden() {
        // Only this test touches these variables.
        std::env::set_var("DECEBAL_OPEN", "my-open --flag");
        assert_eq!(open_argv("/tmp"), vec!["my-open", "--flag", "/tmp"]);
        std::env::remove_var("DECEBAL_OPEN");
        assert_eq!(open_argv("/tmp"), vec!["xdg-open", "/tmp"]);
        std::env::set_var("DECEBAL_TRASH_EMPTY", "true");
        assert_eq!(empty_trash_argv(), vec!["true"]);
        std::env::remove_var("DECEBAL_TRASH_EMPTY");
        assert_eq!(empty_trash_argv(), vec!["gio", "trash", "--empty"]);
    }

    #[test]
    fn path_like_ids_never_resolve() {
        assert_eq!(find_desktop_file("../etc/passwd"), None);
        assert_eq!(resolve_desktop_id(""), None);
    }

    #[test]
    fn unknown_app_does_not_launch() {
        assert!(!launch("definitely-not-installed-app"));
    }

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn noenv(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn field_codes_expand() {
        let f = Fields { name: "My App", icon: "myicon", file: "/x/app.desktop" };
        assert_eq!(expand_exec("app %i %c %k", f), sv(&["app", "--icon", "myicon", "My App", "/x/app.desktop"]));
        assert_eq!(expand_exec("app %i", Fields::default()), sv(&["app"]));
        assert_eq!(expand_exec("app --name=%c %F %u", f), sv(&["app", "--name=My App"]));
        assert_eq!(expand_exec(r#"app "a b" "" %U"#, f), sv(&["app", "a b", ""]));
        assert_eq!(expand_exec(r#"sh -c "echo \"hi\" 100%%""#, f), sv(&["sh", "-c", "echo \"hi\" 100%"]));
        assert_eq!(expand_exec("app --file=%f", f), sv(&["app", "--file="]));
    }

    #[test]
    fn chrome_gets_x11_platform_after_the_program() {
        let a = adjust(&sv(&["/usr/bin/google-chrome-stable", "--profile-directory=Default"]), Session::X11, &builtin_rules(), &noenv);
        assert_eq!(a.argv, sv(&["/usr/bin/google-chrome-stable", "--ozone-platform=x11", "--profile-directory=Default"]));
        for n in ["chromium", "brave", "vivaldi-stable", "opera", "microsoft-edge-stable", "google-chrome"] {
            let a = adjust(&sv(&[n]), Session::X11, &builtin_rules(), &noenv);
            assert_eq!(a.argv, sv(&[n, "--ozone-platform=x11"]), "{n}");
        }
    }

    #[test]
    fn explicit_wayland_switch_is_replaced_and_nothing_is_doubled() {
        let a = adjust(&sv(&["chrome", "--ozone-platform=wayland", "--x"]), Session::X11, &builtin_rules(), &noenv);
        assert_eq!(a.argv, sv(&["chrome", "--ozone-platform=x11", "--x"]));
        let b = adjust(&a.argv, Session::X11, &builtin_rules(), &noenv);
        assert_eq!(b.argv, a.argv);
    }

    #[test]
    fn electron_apps_get_flag_and_env_hint() {
        let a = adjust(&sv(&["electron43", "/usr/lib/obsidian/app.asar"]), Session::X11, &builtin_rules(), &noenv);
        assert_eq!(a.argv, sv(&["electron43", "--ozone-platform=x11", "/usr/lib/obsidian/app.asar"]));
        assert!(a.env.contains(&("ELECTRON_OZONE_PLATFORM_HINT".to_string(), Some("x11".to_string()))));
        assert_eq!(adjust(&sv(&["code", "--unity-launch"]), Session::X11, &builtin_rules(), &noenv).argv[1], "--ozone-platform=x11");
    }

    #[test]
    fn electron_gui_launch_drops_inherited_node_only_mode() {
        let inherited = |key: &str| (key == "ELECTRON_RUN_AS_NODE").then(|| "1".into());
        for session in [Session::X11, Session::Wayland] {
            for app in ["obsidian", "electron43", "code", "discord"] {
                let adjusted = adjust(&sv(&[app]), session, &builtin_rules(), &inherited);
                assert!(adjusted.env.contains(&("ELECTRON_RUN_AS_NODE".into(), None)), "{app}");
            }
        }
        assert!(!adjust(&sv(&["node", "script.js"]), Session::X11, &builtin_rules(), &inherited).env.iter().any(|(key, _)| key == "ELECTRON_RUN_AS_NODE"));
    }

    #[test]
    fn wayland_session_changes_nothing() {
        let a = adjust(&sv(&["google-chrome-stable", "u"]), Session::Wayland, &builtin_rules(), &noenv);
        assert_eq!(a.argv, sv(&["google-chrome-stable", "u"]));
        assert!(a.env.iter().all(|(k, _)| k.starts_with("DECEBAL_") || k.starts_with("MACSHELL_")));
    }

    #[test]
    fn unrelated_programs_and_env_prefixes() {
        assert_eq!(adjust(&sv(&["foot", "-e", "x"]), Session::X11, &builtin_rules(), &noenv).argv, sv(&["foot", "-e", "x"]));
        let a = adjust(&sv(&["env", "FOO=1", "BAR=2", "/usr/bin/chromium", "u"]), Session::X11, &builtin_rules(), &noenv);
        assert_eq!(a.argv, sv(&["env", "FOO=1", "BAR=2", "/usr/bin/chromium", "--ozone-platform=x11", "u"]));
    }

    #[test]
    fn x11_session_drops_wayland_leftovers() {
        let env = |k: &str| match k {
            "WAYLAND_DISPLAY" => Some(String::new()),
            "XDG_SESSION_TYPE" => Some("wayland".into()),
            "GDK_BACKEND" => Some("wayland,x11".into()),
            "QT_QPA_PLATFORM" => Some("xcb".into()),
            "MOZ_ENABLE_WAYLAND" => Some("1".into()),
            _ => None,
        };
        let a = adjust(&sv(&["firefox"]), Session::X11, &[], &env);
        assert!(a.env.contains(&("WAYLAND_DISPLAY".to_string(), None)));
        assert!(a.env.contains(&("XDG_SESSION_TYPE".to_string(), Some("x11".to_string()))));
        assert!(a.env.contains(&("GDK_BACKEND".to_string(), None)));
        assert!(a.env.contains(&("MOZ_ENABLE_WAYLAND".to_string(), None)));
        assert!(!a.env.iter().any(|(k, _)| k == "QT_QPA_PLATFORM"));
    }

    #[test]
    fn user_rules_extend_and_can_disable_builtins() {
        let json = r#"{"launch":{"rules":[{"match":"MyApp*","args":["--a=1"],"env":{"K":"v","DROP":null}},{"match":["w"],"when":"wayland","args":["--w"]}]}}"#;
        let rules = rules_from_json(json);
        let a = adjust(&sv(&["myapp-bin"]), Session::X11, &rules, &noenv);
        assert_eq!(a.argv, sv(&["myapp-bin", "--a=1"]));
        assert!(a.env.contains(&("K".to_string(), Some("v".to_string()))) && a.env.contains(&("DROP".to_string(), None)));
        assert_eq!(adjust(&sv(&["w"]), Session::X11, &rules, &noenv).argv, sv(&["w"]));
        assert_eq!(adjust(&sv(&["w"]), Session::Wayland, &rules, &noenv).argv, sv(&["w", "--w"]));
        assert_eq!(rules.len(), builtin_rules().len() + 2);
        assert!(rules_from_json(r#"{"launch":{"builtin":false}}"#).is_empty());
        assert_eq!(rules_from_json("not json").len(), builtin_rules().len());
    }

    #[test]
    fn decebal_vars_are_not_passed_to_foreign_apps() {
        std::env::set_var("DECEBAL_TEST_INTERNAL", "1");
        let a = adjust(&sv(&["firefox"]), Session::Wayland, &[], &noenv);
        assert!(a.env.contains(&("DECEBAL_TEST_INTERNAL".to_string(), None)));
        let b = adjust(&sv(&["/usr/bin/declaunch"]), Session::Wayland, &[], &noenv);
        assert!(b.env.is_empty());
        std::env::remove_var("DECEBAL_TEST_INTERNAL");
    }

    #[test]
    fn plan_handles_path_terminal_and_tryexec() {
        let mut app = crate::apps::AppEntry { name: "T".into(), exec: "htop %U".into(), terminal: true, path: "/tmp".into(), ..Default::default() };
        let p = plan_app(&app);
        assert!(p.error.is_none());
        assert_eq!(p.argv.last().map(String::as_str), Some("htop"));
        assert!(p.argv.len() >= 2, "wrapped in a terminal: {:?}", p.argv);
        assert_eq!(p.cwd, Some(PathBuf::from("/tmp")));
        app.terminal = false;
        app.path = "/definitely/missing".into();
        let p = plan_app(&app);
        assert_eq!((p.argv.clone(), p.cwd), (sv(&["htop"]), None));
        app.try_exec = "definitely-not-installed-xyz".into();
        assert!(plan_app(&app).error.unwrap().contains("not installed"));
        app.try_exec = "sh".into();
        assert!(plan_app(&app).error.is_none());
    }

    #[test]
    fn terminal_wrapper_orders_choices() {
        std::env::set_var("TERMINAL", "sh");
        let v = terminal_argv(&sv(&["x"]));
        assert_eq!(v.last().map(String::as_str), Some("x"));
        std::env::remove_var("TERMINAL");
    }

    #[test]
    fn spawn_runs_in_workdir_and_reports_early_failure() {
        let dir = std::env::temp_dir().join(format!("decebal-launch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("DECEBAL_NOTIFY", "true");
        std::env::set_var("XDG_STATE_HOME", dir.join("state"));
        let app = crate::apps::AppEntry { name: "Pwd".into(), exec: "sh -c \"pwd > out.txt\"".into(), path: dir.to_string_lossy().into(), ..Default::default() };
        assert!(spawn_app(&app));
        for _ in 0..100 {
            if dir.join("out.txt").exists() && std::fs::read_to_string(dir.join("out.txt")).is_ok_and(|t| !t.is_empty()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let got = std::fs::read_to_string(dir.join("out.txt")).unwrap();
        assert_eq!(std::fs::canonicalize(got.trim()).unwrap(), std::fs::canonicalize(&dir).unwrap());
        assert!(!spawn_argv(&sv(&["/definitely/not/a/program"])));
        std::env::remove_var("DECEBAL_NOTIFY");
        assert!(dir.join("state/decebal/launch.log").is_file());
        std::env::remove_var("XDG_STATE_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
