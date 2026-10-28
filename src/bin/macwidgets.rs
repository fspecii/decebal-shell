//! macwidgets: declarative desktop widgets (TOML) drawn on the desktop, styled by the theme. See docs/theme-engine.md.

use macshell::theme::live::Source;
use macshell::widgets::config::{self, Instance, Place, WidgetsConfig};
use macshell::widgets::{data, dsl, samples, Trust};
use serde_json::Value;
use std::path::PathBuf;

const HELP: &str = "macwidgets - declarative desktop widgets

Usage:
  macwidgets [daemon]              run the widgets listed in macshell.json (\"widgets\" -> \"items\") on the desktop
  macwidgets list                  shipped samples, your widgets, and what is placed
  macwidgets data                  the data sources a widget can read ({cpu.usage:pct}, {clock:%H:%M}...)
  macwidgets validate FILE [--user] [--allow-exec]
                                   check a widget file (default: as a theme-supplied widget, the strictest rules)
  macwidgets render NAME|FILE --png OUT [--live] [--bg #rrggbb|IMAGE] [--margin PX] [--light]
                                   draw a widget with fixed sample data (or live data) to a PNG
  macwidgets add NAME [--anchor top-left|top|top-right|left|center|right|bottom-left|bottom|bottom-right] [--x PX] [--y PX]
                                   place a widget on the desktop (NAME: a file in ~/.config/macshell/widgets, a shipped sample, or builtin:NAME)
  macwidgets remove NAME           take it off the desktop

Widgets from a theme can only use the allowed click actions. Your own widgets may use `exec` and [commands] only with
\"widgets\": { \"allow_exec\": true } in macshell.json.";

fn die(msg: &str) -> ! {
    eprintln!("macwidgets: {msg}");
    std::process::exit(1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let src = Source::discover();
    match args.first().map(String::as_str) {
        None | Some("daemon") => daemon(&src),
        Some("-h" | "--help" | "help") => println!("{HELP}"),
        Some("-V" | "--version") => println!("macwidgets {}", env!("CARGO_PKG_VERSION")),
        Some("data") => {
            for s in data::SOURCES {
                println!("{:<18} {:<9} {}", s.path, s.ty, s.desc);
            }
            println!("{:<18} {:<9} {}", "opt.<key>", "any", "an option of the widget instance (\"options\" in macshell.json)");
            println!("{:<18} {:<9} {}", "cmd.<name>", "text", "first line of a [commands.<name>] run (your own widgets with allow_exec only)");
        }
        Some("list") => list(&src),
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

fn daemon(src: &Source) {
    macshell::mem::limit_arenas();
    let Some(_guard) = macshell::single::acquire("macwidgets") else { return };
    if let Err(e) = macshell::widgets::host::run(macshell::widgets::host::Paths { shell_json: src.shell_json.clone(), themes_dir: src.themes_dir.clone() }) {
        die(&e);
    }
}

fn list(src: &Source) {
    println!("shipped samples:");
    for (id, text) in samples::SAMPLES {
        let w = dsl::parse(text, Trust::Theme).ok();
        println!("  {id:<18} {}", w.map(|w| w.description).unwrap_or_default());
    }
    let dir = config::user_dir();
    println!("your widgets ({}):", dir.display());
    let mut names: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|_| e.path().extension().is_some_and(|x| x == "toml"))).collect();
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
        sel.mode = macshell::theme::Mode::Light;
    }
    let (th, _) = src.build(&sel, macshell::theme::live::system_dark());
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
    let mut fonts = macshell::text::Fonts::load().unwrap_or_else(|| die("no usable font"));
    let card = macshell::widgets::paint::render(&def, &d, &th, &mut fonts, &base, margin).unwrap_or_else(|| die("empty widget"));
    let mut out = tiny_skia::Pixmap::new(card.width(), card.height()).unwrap_or_else(|| die("pixmap"));
    match bg.as_deref() {
        Some(b) if macshell::theme::Rgb::parse(b).is_some() => {
            let c = macshell::theme::Rgb::parse(b).unwrap_or(macshell::theme::Rgb(0, 0, 0));
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
    let mut cfg = WidgetsConfig::load(&src.shell_json);
    if add {
        let mut inst = Instance::desktop(name, "top-left", 40, 60);
        let mut i = 1;
        while i < args.len() {
            let v = args.get(i + 1).cloned().unwrap_or_else(|| die("missing value"));
            match args[i].as_str() {
                "--anchor" => inst.anchor = v,
                "--x" => inst.x = v.parse().unwrap_or_else(|_| die("--x takes pixels")),
                "--y" => inst.y = v.parse().unwrap_or_else(|_| die("--y takes pixels")),
                other => die(&format!("unknown option {other}")),
            }
            i += 2;
        }
        if let Err(e) = config::resolve(&inst, &src.themes_dir, cfg.allow_exec) {
            die(&e);
        }
        inst.place = Place::Desktop;
        cfg.items.retain(|x| !(x.use_ == inst.use_ && x.source == inst.source));
        cfg.items.push(inst);
    } else {
        let before = cfg.items.len();
        cfg.items.retain(|x| x.use_ != *name);
        if cfg.items.len() == before {
            die(&format!("`{name}` is not placed"));
        }
    }
    macshell::settings::io::json_set(&src.shell_json, "widgets", Some(Value::from(cfg.to_json()))).unwrap_or_else(|e| die(&e));
    println!("{} {name}; macwidgets follows within half a second", if add { "placed" } else { "removed" });
}
