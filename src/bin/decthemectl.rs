//! decthemectl: list, inspect and apply decebal themes from scripts.

use decebal::config::BarPosition;
use decebal::settings::io::Paths;
use decebal::theme::live::{system_dark, Source};
use decebal::theme::material::Variant;
use decebal::theme::model::{Def, Dynamic, Mode, Origin, Rgb, Selection, Surfaces};
use decebal::theme::{builtin, dynamic, rdwm, style, system};

const HELP: &str = "decthemectl list [--json]
decthemectl get [--json]
decthemectl apply NAME [--mode light|dark|auto] [--accent #rrggbb|none] [--opacity 0.4-1] [--radius 0-24]
                       [--font NAME|none] [--font-scale 0.8-1.4] [--reset] [--no-rdwm]
decthemectl set KEY VALUE        KEY: mode palette accent opacity radius font font-scale sync-rdwm  (VALUE none = unset)
decthemectl palette [list|status|NAME|none]
                                 colour palettes, separate from themes: a palette recolours whichever theme is active and stays when
                                 you switch themes (catppuccin, rose-pine, gruvbox, tokyo-night, everforest, kanagawa, nord, dracula,
                                 solarized, monochrome, and your own in ~/.config/decebal/palettes/*.toml); `none` = the theme's colours
decthemectl style [macos|windows|caelestia[-left|-right|-top|-bottom|-split]]
                                 the shell style preset: theme + dock style + menu bar together
                                 (windows: Windows 11 theme + taskbar dock; caelestia: Caelestia theme + its pill bar, no dock;
                                 macos: macOS theme + macOS dock). `caelestia` keeps the bar's current position, `caelestia-top`
                                 and the like also set it. Without a name: print the current style.
decthemectl layout [left|right|top|bottom|split]
                                 where the Caelestia pill bar sits (`bar.position` in decebal.json, default left): a vertical bar on
                                 the left or right edge, a horizontal one along the top or bottom edge, or split = a top bar (OS icon,
                                 workspaces, window title) plus a bottom bar (tray, status icons, clock, power). Running bars re-lay out
                                 within about a second; the setting only shows under the Caelestia style. Without a name: print it.
decthemectl panel [--json]       the panels: the bar's renderer and edge, the dock's edge, every applet on or off, and whether each
                                 value is your choice or the theme's (docs/universal-panels.md)
decthemectl panel edge top|bottom|left|right|split|theme
                                 the bar's edge for every bar style (split: the Caelestia rail only); theme = the theme's default
decthemectl panel dock top|bottom|left|right|theme
                                 the dock's or taskbar's edge
decthemectl panel applet KEY on|off|theme
                                 one applet (clock, volume, weather, tray, launcher, tasks, ...); theme = the theme decides
decthemectl panel reset          your panel choices removed: the theme decides everything again
decthemectl dynamic on [OPTIONS]  \"Match wallpaper\": recolour the current theme (any theme) from the wallpaper. Off by default.
decthemectl dynamic off          back to the theme's own colours (the settings are kept)
decthemectl dynamic [status] [--json]
decthemectl dynamic scheme NAME  tonal-spot | vibrant | expressive | monochrome | neutral | content | palette
decthemectl dynamic source wallpaper|PATH|#rrggbb
                                 wallpaper = what rdwm shows (else its config's [wallpaper] image, else Omarchy's
                                 current/background); PATH = a fixed image; #rrggbb = a fixed colour
decthemectl dynamic surfaces accent|tint|full
                                 accent = only the accent; tint = accent + surfaces/text tinted towards the wallpaper's
                                 hue, the theme keeping its own light and dark tones; full = the whole Material palette
decthemectl dynamic strength 0-100   how far surfaces move (tint/full; default 50)
decthemectl dynamic contrast -100..100  Material contrast level of the palette (default 0)
decthemectl dynamic mode keep|wallpaper   wallpaper = light or dark follows the picture's brightness
decthemectl dynamic refresh      re-read the wallpaper now (a no-op while off)
decthemectl dynamic preview [--wallpaper PATH] [--scheme S] [--surfaces K] [--strength N] [--json]
                                 show the palette those settings give, without applying anything
                    OPTIONS (for `on`): --scheme --source --surfaces --strength --contrast --mode --light|--dark --no-rdwm
decthemectl dynamic --wallpaper PATH [--scheme S] [--light|--dark] [--seed #rrggbb]
                                 the older form: the Caelestia theme with a full palette from PATH
decthemectl watch [--interval MS] [--once]
                                 follow wallpaper changes: while match wallpaper is on, re-derives the colours whenever
                                 the wallpaper (path or file) changes. One tiny process; run it from the session
                                 (e.g. `exec-once = decthemectl watch`). Costs a stat per second while idle.
decthemectl font PRESET|list|families|status|set ROLE FAMILY[,FALLBACK...]|scale N|reset
                                 fonts per role (ui title mono display) with fallback chains; PRESET is inter, segoe,
                                 cantarell, rubik, jetbrains, ibm-plex, fira, noto, monospace-ui or system (decthemectl font list)
decthemectl icons list|set NAME|status|reset|scale N|tint on|off|fallback NAME   icon theme settings (decthemectl icons help)
decthemectl wallpaper follow on|off|retry|status [--json]   automatically match curated photos to themes (off by default)
decthemectl wallpaper live set VIDEO | off | status | search WORDS | download ID
                                 Muted video wallpapers, paused behind fullscreen apps (see docs/live-wallpapers.md)
decthemectl wallpaper wallhaven random|search WORDS   a wallpaper from wallhaven.cc (see docs/wallpapers.md)
decthemectl sync                re-apply the current theme to rdwm's config
decthemectl profile ...          presets of the whole setup: list save apply undo export import (decthemectl profile --help)
decthemectl validate FILE        check a theme file
decthemectl export NAME          print a built-in theme file (a starting point for ~/.config/decebal/themes)
decthemectl theme ...            the theme engine: packages (.dtheme), install/pack/validate/new, apply with undo and preview,
                                 snapshot the current look (decthemectl theme help, docs/theme-engine.md)

NAME is a theme id or title (\"nord\", \"Windows 11\"); \"macOS Light\" / \"windows-11-dark\" also pick the mode.
apply writes the selection to decebal.json and the matching colours to rdwm's config.toml
(validated with rdwmctl check-config, comments kept), then reloads rdwm once. Running decbar, decdock,
decspot, declaunch and decsettings follow within half a second.
Environment: DECEBAL_CONFIG (folder of decebal.json), DECEBAL_THEMES, DECSETTINGS_CONFIG / RDWM_CONFIG,
RDWMCTL, RDWM_SOCK.";

/// `decthemectl panel ...`: the same user keys Settings → Panels writes.
fn cmd_panel(src: &Source, args: &[String]) {
    use decebal::panel::{self, AppletId, Edge};
    let paths = Paths::discover();
    let shell_path = paths.shell_json();
    let dock_path = paths.dock_settings();
    let mut shell = decebal::settings::io::json_load(&shell_path);
    let write_bar = |shell: &serde_json::Value| decebal::settings::io::json_set(&shell_path, "bar", shell.get("bar").cloned()).unwrap_or_else(|e| die(&e));
    match args.first().map(String::as_str) {
        None | Some("--json") | Some("status") => {
            let th = src.build(&src.selection(), system_dark()).0;
            let user = panel::UserPanel::from_bar_json(&shell["bar"]);
            let legacy = shell["bar"].get("position").and_then(|p| p.as_str()).and_then(BarPosition::parse);
            let i = panel::Inputs {
                renderer: th.bar_renderer(),
                user: &user,
                theme: &th.panel_theme,
                legacy_position: legacy,
                legacy_workspaces: panel::legacy_workspaces(th.panel_workspaces),
                dux: Some(&th.bar_layout),
                dock_user_edge: None,
            };
            let (edge, split, origin) = panel::resolve_edge(&i);
            let dock = decebal::config::dock_settings();
            let dock_edge = if dock.style == decebal::config::DockStyle::Windows { th.taskbar_panel(&dock).edge } else { dock.edge.or(th.panel_theme.dock_edge).unwrap_or(Edge::Bottom) };
            let applets: Vec<(AppletId, bool, panel::Origin)> = AppletId::all()
                .map(|id| {
                    let (on, o) = panel::resolve_applet(&i, id);
                    (id, on, o)
                })
                .collect();
            if args.iter().any(|a| a == "--json") {
                let list: serde_json::Map<String, serde_json::Value> = applets.iter().map(|(id, on, o)| (id.key().to_string(), serde_json::json!({"on": on, "from": o.title()}))).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({"renderer": th.bar_renderer().key(), "edge": edge.as_str(), "split": split, "edge_from": origin.title(), "dock_edge": dock_edge.as_str(), "applets": list, "fingerprint": th.bar_panel().fingerprint()})).unwrap());
                return;
            }
            println!("{}: {} ({})", th.bar_renderer().title(), if split { "top and bottom" } else { edge.as_str() }, origin.title());
            println!("dock: {}", dock_edge.as_str());
            for (id, on, o) in applets {
                println!("  {:<15} {:<3} ({})", id.key(), if on { "on" } else { "off" }, o.title());
            }
        }
        Some("edge") => {
            let word = args.get(1).map(String::as_str).unwrap_or_else(|| die("panel edge top|bottom|left|right|split|theme"));
            match word {
                "theme" => panel::write_edge(&mut shell, None, None),
                "split" => panel::write_edge(&mut shell, Some(Edge::Top), Some(true)),
                w => panel::write_edge(&mut shell, Some(Edge::parse(w).unwrap_or_else(|| die(&format!("unknown edge `{w}`")))), Some(false)),
            }
            write_bar(&shell);
            println!("bar edge: {word}");
        }
        Some("dock") => {
            let word = args.get(1).map(String::as_str).unwrap_or_else(|| die("panel dock top|bottom|left|right|theme"));
            let value = match word {
                "theme" => None,
                w => Some(serde_json::json!(Edge::parse(w).unwrap_or_else(|| die(&format!("unknown edge `{w}`"))).as_str())),
            };
            decebal::settings::io::json_set(&dock_path, "edge", value).unwrap_or_else(|e| die(&e));
            println!("dock edge: {word}");
        }
        Some("applet") => {
            let key = args.get(1).map(String::as_str).unwrap_or_else(|| die("panel applet KEY on|off|theme"));
            let id = AppletId::parse(key).unwrap_or_else(|| die(&format!("unknown applet `{key}` ({})", AppletId::all().map(AppletId::key).collect::<Vec<_>>().join(", "))));
            let on = match args.get(2).map(String::as_str) {
                Some("on") => Some(true),
                Some("off") => Some(false),
                Some("theme") => None,
                _ => die("panel applet KEY on|off|theme"),
            };
            panel::write_applet(&mut shell, id, on);
            write_bar(&shell);
            println!("{}: {}", id.key(), args[2]);
        }
        Some("reset") => {
            panel::reset(&mut shell);
            write_bar(&shell);
            decebal::settings::io::json_set(&dock_path, "edge", None).unwrap_or_else(|e| die(&e));
            println!("panels follow the theme");
        }
        Some(other) => die(&format!("unknown panel command `{other}`")),
    }
}

fn die(msg: &str) -> ! {
    eprintln!("decthemectl: {msg}");
    std::process::exit(1)
}

fn num(s: &str, what: &str) -> f32 {
    s.parse::<f32>().unwrap_or_else(|_| die(&format!("{what} must be a number")))
}

fn set_key(sel: &mut Selection, key: &str, value: &str) {
    let unset = value == "none" || value.is_empty();
    match key {
        "mode" => sel.mode = Mode::parse(value).unwrap_or_else(|| die("mode must be light, dark or auto")),
        "accent" => sel.accent = if unset { None } else { Some(Rgb::parse(value).unwrap_or_else(|| die("accent must look like #0a84ff"))) },
        "palette" => {
            sel.palette = if unset {
                None
            } else {
                Some(
                    decebal::theme::palette::ids()
                        .into_iter()
                        .find(|i| decebal::theme::builtin::norm(i) == decebal::theme::builtin::norm(value))
                        .unwrap_or_else(|| die(&format!("no palette `{value}` (decthemectl palette list)"))),
                )
            };
            *sel = decebal::theme::palette::select(sel, sel.palette.as_deref());
        }
        "opacity" => sel.opacity = (!unset).then(|| num(value, "opacity")),
        "radius" => sel.radius = (!unset).then(|| num(value, "radius")),
        "font" => sel.font = (!unset).then(|| value.to_string()),
        "font-scale" | "font_scale" => sel.font_scale = (!unset).then(|| num(value, "font-scale")),
        "sync-rdwm" | "sync_rdwm" => sel.sync_rdwm = matches!(value, "true" | "on" | "1" | "yes"),
        other => die(&format!("unknown key `{other}`")),
    }
}

fn apply(src: &Source, sel: &Selection) {
    let paths = Paths::discover();
    match rdwm::apply(src, &paths, sel) {
        Ok(r) => {
            let rd = match (r.rdwm, r.reloaded) {
                (None, _) => "rdwm untouched (--no-rdwm)",
                (Some(rdwm::Wrote::Unchanged), _) => "rdwm config already up to date",
                (Some(_), true) => "rdwm config updated and reloaded",
                (Some(_), false) => "rdwm config updated (rdwm is not running)",
            };
            println!("applied {} ({}); {rd}", r.theme_id, if r.dark { "dark" } else { "light" });
        }
        Err(e) => die(&e),
    }
}

// ---------------------------------------------------------------- match wallpaper

/// What the `dynamic` options ask for; `None` leaves a setting as it is.
#[derive(Default)]
struct DynOpts {
    scheme: Option<Variant>,
    source: Option<Origin>,
    surfaces: Option<Surfaces>,
    strength: Option<f32>,
    contrast: Option<f32>,
    follow_mode: Option<bool>,
    mode: Option<Mode>,
    no_rdwm: bool,
    json: bool,
}

impl DynOpts {
    fn apply(&self, d: &mut Dynamic) {
        if let Some(v) = self.scheme {
            d.scheme = v;
        }
        if let Some(v) = &self.source {
            d.source = v.clone();
        }
        if let Some(v) = self.surfaces {
            d.surfaces = v;
        }
        if let Some(v) = self.strength {
            d.strength = v;
        }
        if let Some(v) = self.contrast {
            d.contrast = v;
        }
        if let Some(v) = self.follow_mode {
            d.follow_mode = v;
        }
    }
}

fn percent(s: &str, what: &str, lo: f32, hi: f32) -> f32 {
    let v: f32 = s.trim_end_matches('%').parse().unwrap_or_else(|_| die(&format!("{what} must be a number from {lo} to {hi} (percent)")));
    if !(lo..=hi).contains(&v) {
        die(&format!("{what} must be from {lo} to {hi} (percent)"));
    }
    v / 100.0
}

fn parse_source(v: &str) -> Origin {
    Origin::parse(v).unwrap_or_else(|| die("source must be `wallpaper`, an image path or a colour like #2a9d8f"))
}

fn parse_opts(args: &[String]) -> DynOpts {
    let mut o = DynOpts::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = || {
            i += 1;
            args.get(i).cloned().unwrap_or_else(|| die(&format!("{a} needs a value")))
        };
        match a {
            "--scheme" => o.scheme = Some(Variant::parse(&value()).unwrap_or_else(|| die("scheme must be tonal-spot, vibrant, expressive, monochrome, neutral, content or palette"))),
            "--source" => o.source = Some(parse_source(&value())),
            // the older spellings of --source
            "--wallpaper" => o.source = Some(Origin::File(value())),
            "--seed" => o.source = Some(Origin::Color(Rgb::parse(&value()).unwrap_or_else(|| die("seed must look like #a8627b")))),
            "--surfaces" => o.surfaces = Some(Surfaces::parse(&value()).unwrap_or_else(|| die("surfaces must be accent, tint or full"))),
            "--strength" => o.strength = Some(percent(&value(), "strength", 0.0, 100.0)),
            "--contrast" => o.contrast = Some(percent(&value(), "contrast", -100.0, 100.0)),
            "--mode" => match value().as_str() {
                "wallpaper" | "follow" => o.follow_mode = Some(true),
                "keep" | "fixed" => o.follow_mode = Some(false),
                _ => die("mode must be `keep` (use the Mode setting) or `wallpaper` (light or dark follows the picture)"),
            },
            "--light" => o.mode = Some(Mode::Light),
            "--dark" => o.mode = Some(Mode::Dark),
            "--no-rdwm" => o.no_rdwm = true,
            "--no-follow" => {} // older option: a fixed image is `--source PATH`
            "--json" => o.json = true,
            other => die(&format!("unknown option {other}")),
        }
        i += 1;
    }
    o
}

fn scheme_text(d: &Dynamic) -> String {
    format!("{} / {}{}", d.scheme.as_str(), d.surfaces.as_str(), if d.surfaces == Surfaces::Accent { String::new() } else { format!(" {:.0}%", d.strength * 100.0) })
}

fn print_status(src: &Source, paths: &Paths, json: bool) {
    let sel = src.selection();
    let (t, _) = src.build(&sel, system_dark());
    let d = sel.dynamic.clone();
    if json {
        let on = d.as_ref().is_some_and(|d| d.enabled);
        let wall = dynamic::find_wallpaper(paths).map(|p| p.to_string_lossy().into_owned());
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "enabled": on,
                "settings": d.as_ref().map(|d| d.to_json()),
                "wallpaper": wall,
                "result": {"theme": t.id, "dark": t.dark, "accent": t.accent.hex(), "background": t.background.hex(), "surface": t.surface.hex(), "text": t.text.hex()},
            }))
            .unwrap()
        );
        return;
    }
    let Some(d) = d else {
        println!("match wallpaper: off (never turned on; `decthemectl dynamic on`)");
        return;
    };
    println!("match wallpaper: {}", if d.enabled { "on" } else { "off (settings kept)" });
    println!("  scheme       {}", scheme_text(&d));
    println!("  contrast     {:+.0}%", d.contrast * 100.0);
    println!(
        "  light/dark   {}",
        if d.follow_mode {
            format!("follows the picture (mean tone {:.0}: {})", d.colors.tone, if d.colors.tone >= dynamic::LIGHT_TONE { "light" } else { "dark" })
        } else {
            "the Mode setting rules".into()
        }
    );
    println!(
        "  source       {}{}",
        d.source.to_text(),
        if d.source == Origin::Wallpaper { dynamic::find_wallpaper(paths).map(|p| format!("  ({})", p.display())).unwrap_or_else(|| "  (none found)".into()) } else { String::new() }
    );
    let others: String = d.colors.others.iter().map(|c| format!(" {}", c.hex())).collect();
    println!(
        "  picture      {} seed {}{}, mean tone {:.0}{}",
        d.image.as_deref().unwrap_or("-"),
        d.colors.seed.hex(),
        others,
        d.colors.tone,
        if d.colors.grey { " (no colour: the theme keeps its own)" } else { "" }
    );
    if d.enabled {
        println!("  result       {} {}: accent {} background {} text {}", t.id, if t.dark { "dark" } else { "light" }, t.accent.hex(), t.background.hex(), t.text.hex());
    }
}

fn print_preview(d: &Dynamic, t: &decebal::theme::Theme, json: bool) {
    use std::io::IsTerminal;
    let chips = dynamic::preview(d, t);
    if json {
        let m: serde_json::Map<String, serde_json::Value> = chips.iter().map(|(n, c)| (n.to_lowercase(), c.hex().into())).collect();
        println!("{}", serde_json::to_string_pretty(&m).unwrap());
        return;
    }
    let tty = std::io::stdout().is_terminal();
    for (name, c) in chips {
        let block = if tty { format!("\x1b[48;2;{};{};{}m      \x1b[0m ", c.0, c.1, c.2) } else { String::new() };
        println!("{block}{:<11}{}", name, c.hex());
    }
}

fn cmd_dynamic(src: &Source, args: &[String]) {
    let paths = Paths::discover();
    let sel = src.selection();
    let rest = |n: usize| args.get(n..).unwrap_or(&[]).to_vec();
    // `scheme X`, `strength N` ... are options with a verb in front
    let verb = |flag: &str| {
        let v = args.get(1).unwrap_or_else(|| die(&format!("`dynamic {}` needs a value", args[0])));
        let mut a = vec![flag.to_string(), v.clone()];
        a.extend(rest(2));
        parse_opts(&a)
    };
    let (opts, kind) = match args.first().map(String::as_str) {
        None | Some("status") => {
            print_status(src, &paths, parse_opts(&rest(1)).json);
            return;
        }
        Some("off") => (parse_opts(&rest(1)), "off"),
        Some("on") => (parse_opts(&rest(1)), "on"),
        Some("refresh") => {
            match dynamic::refresh(src, &paths, None) {
                Ok(dynamic::Refresh::Updated) => println!("wallpaper colours updated"),
                Ok(dynamic::Refresh::Unchanged) => println!("wallpaper colours unchanged"),
                Ok(dynamic::Refresh::Off) => println!("match wallpaper is off"),
                Err(e) => die(&e),
            }
            return;
        }
        Some("preview") => {
            let opts = parse_opts(&rest(1));
            let mut d = sel.dynamic.clone().unwrap_or_default();
            d.enabled = true;
            opts.apply(&mut d);
            let (x, image) = dynamic::read(&dynamic::input_of(&d, &paths, None).unwrap_or_else(|e| die(&e))).unwrap_or_else(|e| die(&e));
            d.colors = x;
            d.image = image;
            let (t, _) = src.build(&Selection { dynamic: Some(d.clone()), ..sel }, system_dark());
            print_preview(&d, &t, opts.json);
            return;
        }
        Some("scheme") => (verb("--scheme"), "set"),
        Some("source") => (verb("--source"), "set"),
        Some("surfaces") => (verb("--surfaces"), "set"),
        Some("strength") => (verb("--strength"), "set"),
        Some("contrast") => (verb("--contrast"), "set"),
        Some("mode") => (verb("--mode"), "set"),
        // the older form: `dynamic --wallpaper PATH --scheme vibrant [--light]` = Caelestia with a full palette
        Some(f) if f.starts_with("--") => (parse_opts(args), "legacy"),
        Some(other) => die(&format!("unknown dynamic command `{other}` (on, off, status, scheme, source, surfaces, strength, contrast, mode, refresh, preview)")),
    };
    let mut sel = sel;
    if let Some(m) = opts.mode {
        sel.mode = m;
    }
    if opts.no_rdwm {
        sel.sync_rdwm = false;
    }
    let new = match kind {
        "off" => dynamic::disable(&sel),
        "set" | "on" | "legacy" => {
            if kind == "legacy" {
                sel.name = decebal::theme::builtin::DYNAMIC_THEME.into();
            }
            let was_on = sel.dynamic.as_ref().is_some_and(|d| d.enabled);
            let legacy_defaults = kind == "legacy" && opts.surfaces.is_none();
            dynamic::update(&sel, &paths, |d| {
                // changing a setting does not switch the feature on by itself; `on` and the older form do
                d.enabled = d.enabled || kind != "set";
                if legacy_defaults {
                    d.surfaces = Surfaces::Full;
                    d.strength = 1.0;
                }
                opts.apply(d);
            })
            .inspect(|_| {
                if !was_on && kind == "set" {
                    eprintln!("decthemectl: saved; match wallpaper is still off (`decthemectl dynamic on`)");
                }
            })
            .unwrap_or_else(|e| die(&e))
        }
        _ => unreachable!(),
    };
    if let Some(d) = new.dynamic.as_ref().filter(|d| d.enabled) {
        println!("match wallpaper: {} from {}, seed {}", scheme_text(d), d.image.as_deref().unwrap_or(&d.source.to_text()), d.colors.seed.hex());
        if d.colors.grey {
            println!("  (that picture has no colour to match: the theme keeps its own)");
        }
    }
    apply(src, &new);
}

fn cmd_watch(src: &Source, args: &[String]) {
    let (mut interval, mut once) = (1500u64, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--once" => once = true,
            "--interval" => interval = it.next().and_then(|v| v.parse::<u64>().ok()).filter(|v| (200..=60_000).contains(v)).unwrap_or_else(|| die("--interval needs milliseconds (200-60000)")),
            other => die(&format!("unknown option {other}")),
        }
    }
    let paths = Paths::discover();
    if once {
        match dynamic::watch_tick(src, &paths, &mut dynamic::WatchState::default()) {
            Some(Ok(dynamic::Refresh::Updated)) => println!("wallpaper colours updated"),
            Some(Err(e)) => die(&e),
            _ => println!("nothing to do"),
        }
        return;
    }
    let Some(_lock) = dynamic::watch_lock() else {
        eprintln!("decthemectl: a watcher is already running");
        return;
    };
    dynamic::watch(src, &paths, std::time::Duration::from_millis(interval), |m| eprintln!("decthemectl watch: {m}"));
}

fn main() {
    let early: Vec<String> = std::env::args().skip(1).collect();
    if early.first().map(String::as_str) == Some("--theme-preview-worker") {
        let Some(path) = early.get(1) else { die("preview worker needs a job file") };
        if let Err(e) = decebal::theme::engine::preview::worker(std::path::Path::new(path)) {
            die(&e);
        }
        return;
    }
    if early.first().map(String::as_str) == Some("--import-reviewed-wallpapers") {
        let Some(source) = early.get(1) else { die("reviewed import needs a source directory") };
        if early.len() > 3 || early.get(2).is_some_and(|s| s != "--apply") {
            die("reviewed import accepts only SOURCE [--apply]")
        }
        match decebal::theme::wallpaper_follow::import_reviewed(&Paths::discover(), std::path::Path::new(source), early.get(2).is_some()) {
            Ok(records) => println!("{}", serde_json::to_string_pretty(&records).unwrap()),
            Err(error) => die(&error),
        }
        return;
    }
    if early.first().map(String::as_str) == Some("--wallpaper-follow-worker") {
        let Some(path) = early.get(1) else { die("wallpaper worker needs a request file") };
        if let Err(e) = decebal::theme::wallpaper_follow::worker(std::path::Path::new(path)) {
            die(&e);
        }
        return;
    }
    decebal::compat::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let src = Source::discover();
    let json = args.iter().any(|a| a == "--json");
    match args.first().map(String::as_str) {
        Some("list") => {
            let sel = src.selection();
            let cur = src.find(&sel.name).map(|f| f.0).unwrap_or_default();
            let list = src.list();
            if json {
                let v: Vec<_> = list
                    .iter()
                    .map(|e| serde_json::json!({"id": e.id, "name": e.name, "description": e.description, "builtin": e.builtin, "dark": e.has_dark, "light": e.has_light, "error": e.error, "current": e.id == cur}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
            } else {
                for e in list {
                    let variants = match (e.has_dark, e.has_light) {
                        (true, true) => "dark+light",
                        (true, false) => "dark",
                        (false, true) => "light",
                        _ => "-",
                    };
                    let note = e.error.as_deref().map(|x| format!("  ERROR: {x}")).unwrap_or_default();
                    println!("{} {:<14} {:<14} {:<11} {}{note}", if e.id == cur { "*" } else { " " }, e.id, e.name, variants, if e.builtin { "built-in" } else { "user" });
                }
            }
        }
        Some("profile") => match args.get(1).map(String::as_str) {
            None | Some("-h") | Some("--help") | Some("help") => println!("{}", decebal::preset::cli::HELP),
            Some(_) => {
                if let Err(e) = decebal::preset::cli::run(&args[1..]) {
                    die(&e);
                }
            }
        },
        Some("get") => {
            let sel = src.selection();
            let (t, problem) = src.build(&sel, system_dark());
            if json {
                let colors: serde_json::Map<String, serde_json::Value> = [
                    ("background", t.background),
                    ("surface", t.surface),
                    ("bar", t.bar),
                    ("popup", t.popup),
                    ("dock", t.dock),
                    ("text", t.text),
                    ("text_secondary", t.text_secondary),
                    ("accent", t.accent),
                    ("accent_text", t.accent_text),
                    ("danger", t.danger),
                ]
                .into_iter()
                .map(|(k, c)| (k.to_string(), c.hex().into()))
                .collect();
                let st = style::current(&src, &Paths::discover()).map(|s| s.as_str());
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({"theme": t.id, "name": t.name, "dark": t.dark, "style": st, "selection": sel.to_json(), "problem": problem, "colors": colors, "radius": t.radius, "font_scale": t.font_scale, "bar_style": t.bar_style, "fonts": {"preset": t.fonts.preset, "ui": t.fonts.chain(decebal::theme::Role::Ui), "title": t.fonts.chain(decebal::theme::Role::Title), "mono": t.fonts.chain(decebal::theme::Role::Mono), "display": t.fonts.chain(decebal::theme::Role::Display)}})).unwrap());
            } else {
                println!(
                    "{} ({}) mode={} accent={} radius={} opacity={} font={} preset={} font-scale={}",
                    t.id,
                    if t.dark { "dark" } else { "light" },
                    sel.mode.as_str(),
                    t.accent.hex(),
                    t.radius,
                    sel.opacity.map_or("theme".into(), |o| o.to_string()),
                    if t.font_family.is_empty() { "default" } else { &t.font_family },
                    t.fonts.preset,
                    t.font_scale
                );
                if let Some(p) = problem {
                    println!("problem: {p} (showing the default theme)");
                }
            }
        }
        Some("apply") => {
            let mut sel = src.selection();
            let Some(name) = args.get(1).filter(|a| !a.starts_with("--")) else { die("apply needs a theme name") };
            let (id, implied) = src.find(name).unwrap_or_else(|| die(&format!("no theme `{name}` (decthemectl list)")));
            let entry = src.list().into_iter().find(|e| e.id == id);
            if let Some(e) = entry.filter(|e| e.error.is_some()) {
                die(&format!("theme `{}` is broken: {}", e.id, e.error.unwrap_or_default()));
            }
            sel.name = id;
            sel.palette = None;
            sel.accent = None;
            sel.dynamic = None;
            if let Some(m) = implied {
                sel.mode = m;
            }
            let mut widget_choice = decebal::theme::engine::apply::WidgetChoice::Ask;
            let mut i = 2;
            while i < args.len() {
                let a = args[i].as_str();
                match a {
                    "--reset" => sel = Selection { name: sel.name.clone(), mode: sel.mode, sync_rdwm: sel.sync_rdwm, ..Selection::default() },
                    "--no-rdwm" => sel.sync_rdwm = false,
                    "--keep-widgets" => widget_choice = decebal::theme::engine::apply::WidgetChoice::Keep,
                    "--replace-widgets" => widget_choice = decebal::theme::engine::apply::WidgetChoice::Replace,
                    "--mode" | "--accent" | "--opacity" | "--radius" | "--font" | "--font-scale" => {
                        i += 1;
                        let v = args.get(i).unwrap_or_else(|| die(&format!("{a} needs a value")));
                        set_key(&mut sel, &a[2..], v);
                    }
                    other => die(&format!("unknown option {other}")),
                }
                i += 1;
            }
            let paths = Paths::discover();
            let mut plan = decebal::theme::engine::apply::plan_selection_with_widgets(&src, &paths, &sel, widget_choice).unwrap_or_else(|e| die(&e));
            // This command applies a complete design, including an explicitly
            // enabled matching photo even when reapplying the same design.
            plan.follow_wallpaper = true;
            let changed = plan.rdwm_changed;
            let result = decebal::theme::engine::apply::execute(&src, &paths, plan).unwrap_or_else(|e| die(&e));
            let rd = if !sel.sync_rdwm {
                "rdwm untouched (--no-rdwm)"
            } else if !changed {
                "rdwm config already up to date"
            } else if result.reloaded {
                "rdwm config updated and reloaded"
            } else {
                "rdwm config updated (rdwm is not running)"
            };
            println!("applied {} ({}); {rd}", result.theme_id, if result.dark { "dark" } else { "light" });
        }

        Some("set") => {
            let (Some(k), Some(v)) = (args.get(1), args.get(2)) else { die("set needs KEY VALUE") };
            let mut sel = src.selection();
            set_key(&mut sel, k, v);
            apply(&src, &sel);
        }
        Some("palette") => match args.get(1).map(String::as_str) {
            None | Some("status") => {
                let sel = src.selection();
                println!("{}", sel.palette.as_deref().unwrap_or("none (the theme's own colours)"));
            }
            Some("list") => {
                let cur = src.selection().palette;
                for p in decebal::theme::palette::list() {
                    let variants = match (p.has_dark, p.has_light) {
                        (true, true) => "dark+light",
                        (true, false) => "dark",
                        _ => "light",
                    };
                    println!("{}{:<16} {:<10} {}  {}", if cur.as_deref() == Some(p.id.as_str()) { "* " } else { "  " }, p.id, variants, if p.builtin { "built-in" } else { "yours   " }, p.description);
                }
            }
            Some(name) => {
                let mut sel = src.selection();
                set_key(&mut sel, "palette", name);
                apply(&src, &sel);
            }
        },
        Some("style") => {
            let paths = Paths::discover();
            let Some(name) = args.get(1).filter(|a| !a.starts_with("--")) else {
                match style::current(&src, &paths) {
                    Some(s) => println!("{}", s.as_str()),
                    None => println!("custom"),
                }
                return;
            };
            let (st, pos) = style::parse_with_layout(name).unwrap_or_else(|| die(&format!("unknown style `{name}` (macos, windows, caelestia or caelestia-left|right|top|bottom|split)")));
            let mut sel = src.selection();
            for a in &args[2..] {
                match a.as_str() {
                    "--no-rdwm" => sel.sync_rdwm = false,
                    other => die(&format!("unknown option {other}")),
                }
            }
            match style::apply_with(&src, &paths, st, &sel, pos) {
                Ok(r) => {
                    let layout = style::position_of(&decebal::settings::io::json_load(&paths.shell_json()));
                    println!(
                        "style {}: theme {} ({}), dock {}, menu bar {}",
                        st.as_str(),
                        r.theme_id,
                        if r.dark { "dark" } else { "light" },
                        match st {
                            style::Style::Windows => "windows taskbar",
                            style::Style::Caelestia => "hidden (Caelestia has no dock)",
                            style::Style::Macos => "macOS dock",
                        },
                        match st {
                            style::Style::Windows => "hidden while the taskbar is up".to_string(),
                            style::Style::Caelestia => layout_phrase(layout).to_string(),
                            style::Style::Macos => "shown".to_string(),
                        }
                    );
                }
                Err(e) => die(&e),
            }
        }
        Some("layout") => {
            let paths = Paths::discover();
            let Some(name) = args.get(1).filter(|a| !a.starts_with("--")) else {
                println!("{}", style::position_of(&decebal::settings::io::json_load(&paths.shell_json())).as_str());
                return;
            };
            let pos = BarPosition::parse(name).unwrap_or_else(|| die(&format!("unknown layout `{name}` (left, right, top, bottom or split)")));
            if let Some(extra) = args.get(2) {
                die(&format!("unexpected argument {extra}"));
            }
            if let Err(e) = style::apply_layout(&paths, pos) {
                die(&e);
            }
            let active = src.build(&src.selection(), system_dark()).0.pill_bar().is_some();
            println!("layout {}: {}{}", pos.as_str(), layout_phrase(pos), if active { "" } else { " (shows once the Caelestia style is on: decthemectl style caelestia)" });
        }
        Some("panel") => cmd_panel(&src, &args[1..]),
        Some("dynamic") => cmd_dynamic(&src, &args[1..]),
        Some("watch") => cmd_watch(&src, &args[1..]),
        Some("font") => {
            let cmd = decebal::theme::fonts::parse_cmd(&args[1..]).unwrap_or_else(|e| die(&format!("{e}\n{}", decebal::theme::fonts::HELP)));
            let db = decebal::theme::fontdb::get();
            let mut sel = src.selection();
            match &cmd {
                decebal::theme::fonts::Cmd::List => print!("{}", decebal::theme::fonts::render_list(&db, &src.build(&sel, system_dark()).0.fonts.preset)),
                decebal::theme::fonts::Cmd::Families => {
                    for f in db.picker_families() {
                        println!("{f}{}", if db.is_mono(&f) { "  (mono)" } else { "" });
                    }
                }
                decebal::theme::fonts::Cmd::Status => print!("{}", decebal::theme::fonts::render_status(&src.build(&sel, system_dark()).0, &db)),
                changing => {
                    if let decebal::theme::fonts::Cmd::Set(_, Some(chain)) = changing {
                        for f in chain.iter().filter(|f| !db.has_family(f)) {
                            eprintln!("decthemectl: note: `{f}` is not installed (kept; it is skipped until it is)");
                        }
                    }
                    decebal::theme::fonts::apply_cmd(&mut sel, changing);
                    apply(&src, &sel);
                    print!("{}", decebal::theme::fonts::render_status(&src.build(&sel, system_dark()).0, &db));
                }
            }
        }
        Some("wallpaper") => match if args.get(1).map(String::as_str) == Some("follow") {
            decebal::theme::wallpaper_follow::cli(&args[2..])
        } else if args.get(1).map(String::as_str) == Some("live") {
            decebal::livewall::cli(&args[2..])
        } else {
            decebal::settings::wallhaven::cli::run(&args[1..])
        } {
            Ok(line) => println!("{line}"),
            Err(e) => die(&e),
        },
        Some("icons") => match decebal::icons::ctl::run(&decebal::icons::config::shell_json(), &decebal::icons::resolve::Roots::from_env(), &args[1..]) {
            Ok(out) => println!("{out}"),
            Err(e) => die(&e),
        },
        Some("sync") => {
            let sel = src.selection();
            apply(&src, &Selection { sync_rdwm: true, ..sel });
        }
        Some("validate") => {
            let Some(file) = args.get(1) else { die("validate needs a file") };
            let text = std::fs::read_to_string(file).unwrap_or_else(|e| die(&format!("{file}: {e}")));
            match Def::parse(&text) {
                Ok(d) => println!(
                    "ok: {} ({})",
                    d.name,
                    match (d.has_dark(), d.has_light()) {
                        (true, true) => "dark+light",
                        (true, false) => "dark",
                        _ => "light",
                    }
                ),
                Err(e) => die(&format!("{file}: {e}")),
            }
        }
        Some("export") => {
            let Some(name) = args.get(1) else { die("export needs a theme name") };
            let id = src.find(name).map(|f| f.0).unwrap_or_else(|| name.clone());
            print!("{}", builtin::source(&id).unwrap_or_else(|| die(&format!("`{name}` is not a built-in theme"))));
        }
        Some("theme") => match decebal::theme::engine::cli::run(&args[1..], &src, &Paths::discover()) {
            Ok(text) => print!("{text}"),
            Err(e) => die(&e),
        },
        Some("apply-system") | Some("system") => {
            if let Err(e) = system::cli::run(&args, &src) {
                die(&e);
            }
        }
        Some("log") => std::process::exit(decebal::dlog::cli(&args[1..])),
        Some("-h") | Some("--help") | Some("help") | None => println!("{HELP}\n\n{}\n\n{}", system::cli::HELP, decebal::dlog::HELP),
        Some(other) => die(&format!("unknown command `{other}`\n{HELP}\n\n{}", system::cli::HELP)),
    }
}

/// What the Caelestia bar looks like at `pos`, for the messages.
fn layout_phrase(pos: BarPosition) -> &'static str {
    match pos {
        BarPosition::Left => "vertical bar on the left edge",
        BarPosition::Right => "vertical bar on the right edge",
        BarPosition::Top => "horizontal bar along the top edge",
        BarPosition::Bottom => "horizontal bar along the bottom edge",
        BarPosition::Split => "split: a bar on top (launcher, workspaces, window) and one at the bottom (tray, status, clock, power)",
    }
}
