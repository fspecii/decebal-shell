//! decwidgets: declarative desktop widgets (TOML) drawn on the desktop, styled by the theme. See docs/theme-engine.md.

use decebal::theme::live::Source;
use decebal::widgets::config::{self, Instance, Place, WidgetsConfig};
use decebal::widgets::{data, dsl, samples, Trust};
use std::path::PathBuf;

const HELP: &str = "decwidgets - declarative desktop widgets

Usage:
  decwidgets autostart             start the shared host for this session (Wayland or X11); quietly skip headless sessions
  decwidgets [daemon]              run the widgets listed in decebal.json (\"widgets\" -> \"items\") on the desktop
  decwidgets status [--json]        host readiness, saved desktop widgets and definition errors
  decwidgets list                  shipped samples, your widgets, and what is placed
  decwidgets data                  the data sources a widget can read ({cpu.usage:pct}, {clock:%H:%M}...)
  decwidgets data --json [--group notes|timer]...
                                   one-shot snapshot of the resolved notes.*/timer.* values (schema decebal-widget-data/1;
                                   read-only, not live; includes private note title/excerpt only when notes is requested)
  decwidgets validate FILE [--user] [--allow-exec]
                                   check a widget file (default: as a theme-supplied widget, the strictest rules)
  decwidgets render NAME|FILE --png OUT [--live] [--bg #rrggbb|IMAGE] [--margin PX] [--light]
                                   draw a widget with fixed sample data (or live data) to a PNG
  decwidgets add NAME [--anchor top-left|top|top-right|left|center|right|bottom-left|bottom|bottom-right] [--x PX] [--y PX]
                                   place a widget on the desktop (NAME: a file in ~/.config/decebal/widgets, a shipped sample, or builtin:NAME)
  decwidgets remove NAME           take it off the desktop

Widgets from a theme can only use the allowed click actions. Your own widgets may use `exec` and [commands] only with
\"widgets\": { \"allow_exec\": true } in decebal.json.";

fn die(msg: &str) -> ! {
    eprintln!("decwidgets: {msg}");
    std::process::exit(1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "data") && args.iter().any(|a| a == "--json") {
        // One-shot read-only snapshot: no log file, no old-name data migration, no state writes.
        decebal::compat::apply_env_fallbacks();
    } else {
        decebal::compat::init();
    }
    let src = Source::discover();
    match args.first().map(String::as_str) {
        None | Some("daemon") => daemon(&src, false),
        // a headless session (no usable display) is quietly skipped
        Some("autostart") => daemon(&src, true),
        Some("-h" | "--help" | "help") => println!("{HELP}"),
        Some("-V" | "--version") => println!("decwidgets {}", env!("CARGO_PKG_VERSION")),
        Some("data") if args.iter().any(|a| a == "--json") => {
            let mut groups = Vec::new();
            let mut rest = args[1..].iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--json" => {}
                    "--group" => groups.push(rest.next().cloned().unwrap_or_else(|| die("--group needs notes or timer"))),
                    other => die(&format!("data --json: unexpected argument {other:?}")),
                }
            }
            let groups = decebal::widgets::data_json::parse_groups(&groups).unwrap_or_else(|e| die(&e));
            println!("{}", decebal::widgets::data_json::snapshot(&groups));
        }
        Some("data") => {
            for s in data::SOURCES {
                println!("{:<18} {:<9} {}", s.path, s.ty, s.desc);
            }
            println!("{:<18} {:<9} an option of the widget instance (\"options\" in decebal.json)", "opt.<key>", "any");
            println!("{:<18} {:<9} first line of a [commands.<name>] run (your own widgets with allow_exec only)", "cmd.<name>", "text");
        }
        Some("list") => list(&src),
        Some("status") => status(&src, args.iter().any(|a| a == "--json")),
        Some("validate") => {
            let Some(file) = args.get(1) else { die("validate needs a file") };
            let trust = if args.iter().any(|a| a == "--user") {
                if args.iter().any(|a| a == "--allow-exec") {
                    Trust::UserExec
                } else {
                    Trust::User
                }
            } else {
                Trust::Theme
            };
            let text = std::fs::read_to_string(file).unwrap_or_else(|e| die(&format!("{file}: {e}")));
            match dsl::parse(&text, trust) {
                Ok(w) => println!("ok: {} ({} data sources, {} actions)", if w.name.is_empty() { &w.id } else { &w.name }, w.paths.len(), w.actions.len()),
                Err(e) => die(&format!("{file}:\n{e}")),
            }
        }
        Some("render") => render(&src, &args[1..]),
        Some("add") => edit(&src, &args[1..], true),
        Some("remove") => edit(&src, &args[1..], false),
        Some(other) => die(&format!("unknown command `{other}`\n\n{HELP}")),
    }
}

fn daemon(src: &Source, autostart: bool) {
    decebal::mem::limit_arenas();
    // The host takes the lock of its display server itself (and returns when another host already serves it).
    if let Err(e) = decebal::widgets::host::run(decebal::widgets::host::Paths { shell_json: src.shell_json.clone(), themes_dir: src.themes_dir.clone() }, autostart) {
        die(&e);
    }
}

fn status(src: &Source, json: bool) {
    let cfg = WidgetsConfig::load(&src.shell_json);
    let instances: Vec<_> = cfg
        .items
        .iter()
        .filter(|i| i.place == Place::Desktop)
        .map(|i| {
            let problem = config::resolve(i, &src.themes_dir, cfg.allow_exec).err();
            serde_json::json!({"use":i.use_,"enabled":i.enabled,"source":i.source,"error":problem})
        })
        .collect();
    use decebal::widgets::host::HostState;
    let state = decebal::widgets::host::state(Some(src.shell_json.as_path()));
    let running = state.running();
    if json {
        let (name, tally) = match state {
            HostState::Down => ("down", None),
            HostState::Starting => ("starting", None),
            HostState::Ready(t) => ("ready", Some(t)),
            HostState::Degraded(t) => ("degraded", Some(t)),
            HostState::Legacy(_) => ("legacy", None),
        };
        let shown = tally.map(|t| serde_json::json!({"wanted":t.wanted,"shown":t.shown,"broken":t.broken,"unplaced":t.unplaced}));
        let legacy_pid = match state {
            HostState::Legacy(pid) => Some(pid),
            _ => None,
        };
        println!("{}", serde_json::json!({"running":running,"state":name,"cards":shown,"legacy_pid":legacy_pid,"enabled":cfg.enabled,"config":src.shell_json,"items":instances}));
    } else {
        let host = match state {
            HostState::Down => "not running in this session".to_string(),
            HostState::Starting => "starting (has not shown the current configuration yet)".to_string(),
            HostState::Ready(t) if t.wanted == 0 => "ready (no desktop widget enabled)".to_string(),
            HostState::Ready(t) => format!("ready ({} shown)", t.shown),
            HostState::Degraded(t) => format!("running, but {}", t.problem()),
            HostState::Legacy(pid) => format!("a host from before the update (process {pid}) shows widgets on this display; log out and back in, or end that process, to switch to the updated host"),
        };
        println!("Desktop host: {host}");
        println!("Visibility: {}", if cfg.enabled { "shown" } else { "hidden" });
        println!("Configuration: {}", src.shell_json.display());
        if instances.is_empty() {
            println!("No desktop cards configured. Open System Settings → Widgets and choose Add.");
        }
        for i in instances {
            println!("  {} {}{}", if i["enabled"] == true { "on " } else { "off" }, i["use"].as_str().unwrap_or("?"), i["error"].as_str().map(|e| format!(" — {e}")).unwrap_or_default());
        }
    }
}

fn list(src: &Source) {
    println!("Available on every theme:");
    for e in decebal::widgets::catalog::list(&src.themes_dir, false) {
        println!("  {}  {}{}", e.id, e.name, e.error.map(|s| format!(" (unavailable: {s})")).unwrap_or_default());
    }
    println!("shipped samples:");
    for (id, text) in samples::SAMPLES {
        let w = dsl::parse(text, Trust::Theme).ok();
        println!("  {id:<18} {}", w.map(|w| w.description).unwrap_or_default());
    }
    let dir = config::user_dir();
    println!("your widgets ({}):", dir.display());
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|_| e.path().extension().is_some_and(|x| x == "toml")))
        .collect();
    names.sort();
    for n in names {
        println!("  {n}");
    }
    println!("placed:");
    for i in WidgetsConfig::load(&src.shell_json).items {
        println!("  {} {}{} at {} {},{}", if i.enabled { "on " } else { "off" }, i.use_, i.source.map(|s| format!(" (theme {s})")).unwrap_or_default(), i.anchor, i.x, i.y);
    }
}

fn render(src: &Source, args: &[String]) {
    let Some(name) = args.first().filter(|a| !a.starts_with("--")) else { die("render needs a widget name or file") };
    let (mut png, mut live, mut bg, mut margin, mut light) = (None::<PathBuf>, false, None::<String>, 16u32, false);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--png" | "--bg" | "--margin" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| die("missing value"));
                match args[i - 1].as_str() {
                    "--png" => png = Some(v.into()),
                    "--bg" => bg = Some(v.clone()),
                    _ => margin = v.parse().unwrap_or_else(|_| die("--margin takes pixels")),
                }
            }
            "--live" => live = true,
            "--light" => light = true,
            other => die(&format!("unknown option {other}")),
        }
        i += 1;
    }
    let png = png.unwrap_or_else(|| die("render needs --png OUT"));
    let (def, base) = if std::path::Path::new(name).is_file() {
        let text = std::fs::read_to_string(name).unwrap_or_else(|e| die(&e.to_string()));
        (dsl::parse(&text, Trust::Theme).unwrap_or_else(|e| die(&e)), PathBuf::from(name).parent().map(PathBuf::from).unwrap_or_default())
    } else {
        let r = config::resolve(&Instance::desktop(name, "top-left", 0, 0), &src.themes_dir, false).unwrap_or_else(|e| die(&e));
        (r.def, r.base)
    };
    let mut sel = src.selection();
    if light {
        sel.mode = decebal::theme::Mode::Light;
    }
    let (th, _) = src.build(&sel, decebal::theme::live::system_dark());
    let mut d = if live {
        let mut l = data::Live::new(&def.paths, &[], data::null_waker());
        let mut d = data::Data::empty();
        l.update(&mut d);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        l.update(&mut d);
        d
    } else {
        data::sample()
    };
    d.set("opt.name", data::Val::Text("demo".into()));
    let mut fonts = decebal::text::Fonts::load().unwrap_or_else(|| die("no usable font"));
    let card = decebal::widgets::paint::render(&def, &d, &th, &mut fonts, &base, margin).unwrap_or_else(|| die("empty widget"));
    let mut out = tiny_skia::Pixmap::new(card.width(), card.height()).unwrap_or_else(|| die("pixmap"));
    match bg.as_deref() {
        Some(b) if decebal::theme::Rgb::parse(b).is_some() => {
            let c = decebal::theme::Rgb::parse(b).unwrap_or(decebal::theme::Rgb(0, 0, 0));
            out.fill(c.color(1.0));
        }
        Some(path) => match image::open(path) {
            Ok(img) => {
                let img = image::imageops::resize(&img.to_rgba8(), out.width(), out.height(), image::imageops::FilterType::Triangle);
                for (dst, s) in out.data_mut().chunks_exact_mut(4).zip(img.pixels()) {
                    dst.copy_from_slice(&[s[0], s[1], s[2], 255]);
                }
            }
            Err(e) => die(&format!("{path}: {e}")),
        },
        None => out.fill(th.background.color(1.0)),
    }
    out.draw_pixmap(0, 0, card.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
    out.save_png(&png).unwrap_or_else(|e| die(&e.to_string()));
    println!("{}", png.display());
}

fn edit(src: &Source, args: &[String], add: bool) {
    let Some(name) = args.first().filter(|a| !a.starts_with("--")) else { die("needs a widget name") };
    let root: serde_json::Value = match std::fs::read_to_string(&src.shell_json) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| die(&format!("{}: {e}; configuration was not changed", src.shell_json.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => die(&format!("{}: {e}", src.shell_json.display())),
    };
    if !root.is_object() || root.get("widgets").is_some_and(|w| !w.is_object() || w.get("items").is_some_and(|i| !i.is_array())) {
        die("widgets configuration must be an object with an items array; configuration was not changed");
    }
    let mut cfg = WidgetsConfig::from_json(&root);
    let mut items = root.pointer("/widgets/items").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    if add {
        let mut inst = Instance::desktop(name, "top-left", 40, 60);
        let mut overrides = serde_json::Map::new();
        let mut i = 1;
        while i < args.len() {
            let v = args.get(i + 1).cloned().unwrap_or_else(|| die("missing value"));
            match args[i].as_str() {
                "--anchor" => {
                    inst.anchor = v;
                    overrides.insert("anchor".into(), serde_json::json!(inst.anchor));
                }
                "--x" => {
                    inst.x = v.parse().unwrap_or_else(|_| die("--x takes pixels"));
                    overrides.insert("x".into(), serde_json::json!(inst.x));
                }
                "--y" => {
                    inst.y = v.parse().unwrap_or_else(|_| die("--y takes pixels"));
                    overrides.insert("y".into(), serde_json::json!(inst.y));
                }
                other => die(&format!("unknown option {other}")),
            }
            i += 2;
        }
        if let Err(e) = config::resolve(&inst, &src.themes_dir, cfg.allow_exec) {
            die(&e);
        }
        inst.place = Place::Desktop;
        upsert(&mut items, &inst, overrides);
    } else {
        let before = items.len();
        items.retain(|v| v.get("use").and_then(serde_json::Value::as_str) != Some(name.as_str()));
        if items.len() == before {
            die(&format!("`{name}` is not placed"));
        }
    }
    let mut widgets = root.get("widgets").and_then(serde_json::Value::as_object).cloned().unwrap_or_default();
    if add {
        cfg.enabled = true;
    }
    widgets.insert("items".into(), serde_json::Value::Array(items));
    widgets.insert("enabled".into(), serde_json::json!(cfg.enabled));
    decebal::settings::io::json_set(&src.shell_json, "widgets", Some(serde_json::Value::Object(widgets))).unwrap_or_else(|e| die(&e));
    cfg = WidgetsConfig::load(&src.shell_json);
    if cfg.enabled && cfg.items.iter().any(|i| i.enabled && i.place == Place::Desktop) {
        if let Err(e) = decebal::widgets::host::ensure_running() {
            die(&e);
        }
    }
    println!("{} {name}; decwidgets follows within half a second", if add { "placed" } else { "removed" });
}

fn upsert(items: &mut Vec<serde_json::Value>, inst: &Instance, overrides: serde_json::Map<String, serde_json::Value>) {
    if let Some(raw) = items.iter_mut().find(|v| Instance::from_json(v).is_some_and(|x| x.use_ == inst.use_ && x.source == inst.source)) {
        let object = raw.as_object_mut().unwrap();
        object.extend(overrides);
        object.insert("enabled".into(), serde_json::json!(true));
        object.insert("place".into(), serde_json::json!("desktop"));
    } else {
        items.push(inst.to_json());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn add_preserves_matching_raw_options_and_only_overrides_explicit_position() {
        let original = json!({"use":"clock-card","enabled":false,"x":17,"y":90,"anchor":"bottom-right","monitor":"DP-2","options":{"format":"24h"},"future":3,"from_theme":true});
        let mut items = vec![original.clone(), json!({"unknown":true})];
        let inst = Instance::desktop("clock-card", "top-left", 80, 60);
        let overrides = serde_json::Map::from_iter([("x".into(), json!(80))]);
        upsert(&mut items, &inst, overrides);
        let mut expected = original;
        expected["enabled"] = json!(true);
        expected["place"] = json!("desktop");
        expected["x"] = json!(80);
        assert_eq!(items, vec![expected, json!({"unknown":true})]);
    }
}
