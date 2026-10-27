//! macthemectl: list, inspect and apply macshell themes from scripts.

use macshell::settings::io::Paths;
use macshell::theme::live::{system_dark, Source};
use macshell::theme::model::{Def, Mode, Rgb, Selection};
use macshell::theme::{builtin, rdwm};

const HELP: &str = "macthemectl list [--json]
macthemectl get [--json]
macthemectl apply NAME [--mode light|dark|auto] [--accent #rrggbb|none] [--opacity 0.4-1] [--radius 0-24]
                       [--font NAME|none] [--font-scale 0.8-1.4] [--reset] [--no-rdwm]
macthemectl set KEY VALUE        KEY: mode accent opacity radius font font-scale sync-rdwm  (VALUE none = unset)
macthemectl sync                 re-apply the current theme to rdwm's config
macthemectl validate FILE        check a theme file
macthemectl export NAME          print a built-in theme file (a starting point for ~/.config/macshell/themes)

NAME is a theme id or title (\"nord\", \"Windows 11\"); \"macOS Light\" / \"windows-11-dark\" also pick the mode.
apply writes the selection to macshell.json and the matching colours to rdwm's config.toml
(validated with rdwmctl check-config, comments kept), then reloads rdwm once. Running macbar, macdock,
macspot, maclaunch and macsettings follow within half a second.
Environment: MACSHELL_CONFIG (folder of macshell.json), MACSHELL_THEMES, MACSETTINGS_CONFIG / RDWM_CONFIG,
RDWMCTL, RDWM_SOCK.";

fn die(msg: &str) -> ! {
    eprintln!("macthemectl: {msg}");
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

fn main() {
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
        Some("get") => {
            let sel = src.selection();
            let (t, problem) = src.build(&sel, system_dark());
            if json {
                let colors: serde_json::Map<String, serde_json::Value> = [("background", t.background), ("surface", t.surface), ("bar", t.bar), ("popup", t.popup), ("dock", t.dock), ("text", t.text), ("text_secondary", t.text_secondary), ("accent", t.accent), ("accent_text", t.accent_text), ("danger", t.danger)]
                    .into_iter()
                    .map(|(k, c)| (k.to_string(), c.hex().into()))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({"theme": t.id, "name": t.name, "dark": t.dark, "selection": sel.to_json(), "problem": problem, "colors": colors, "radius": t.radius, "font_scale": t.font_scale})).unwrap());
            } else {
                println!("{} ({}) mode={} accent={} radius={} opacity={} font={} font-scale={}", t.id, if t.dark { "dark" } else { "light" }, sel.mode.as_str(), t.accent.hex(), t.radius, sel.opacity.map_or("theme".into(), |o| o.to_string()), if t.font_family.is_empty() { "default" } else { &t.font_family }, t.font_scale);
                if let Some(p) = problem {
                    println!("problem: {p} (showing the default theme)");
                }
            }
        }
        Some("apply") => {
            let mut sel = src.selection();
            let Some(name) = args.get(1).filter(|a| !a.starts_with("--")) else { die("apply needs a theme name") };
            let (id, implied) = src.find(name).unwrap_or_else(|| die(&format!("no theme `{name}` (macthemectl list)")));
            let entry = src.list().into_iter().find(|e| e.id == id);
            if let Some(e) = entry.filter(|e| e.error.is_some()) {
                die(&format!("theme `{}` is broken: {}", e.id, e.error.unwrap_or_default()));
            }
            sel.name = id;
            if let Some(m) = implied {
                sel.mode = m;
            }
            let mut i = 2;
            while i < args.len() {
                let a = args[i].as_str();
                match a {
                    "--reset" => sel = Selection { name: sel.name.clone(), mode: sel.mode, sync_rdwm: sel.sync_rdwm, ..Selection::default() },
                    "--no-rdwm" => sel.sync_rdwm = false,
                    "--mode" | "--accent" | "--opacity" | "--radius" | "--font" | "--font-scale" => {
                        i += 1;
                        let v = args.get(i).unwrap_or_else(|| die(&format!("{a} needs a value")));
                        set_key(&mut sel, &a[2..], v);
                    }
                    other => die(&format!("unknown option {other}")),
                }
                i += 1;
            }
            apply(&src, &sel);
        }
        Some("set") => {
            let (Some(k), Some(v)) = (args.get(1), args.get(2)) else { die("set needs KEY VALUE") };
            let mut sel = src.selection();
            set_key(&mut sel, k, v);
            apply(&src, &sel);
        }
        Some("sync") => {
            let sel = src.selection();
            apply(&src, &Selection { sync_rdwm: true, ..sel });
        }
        Some("validate") => {
            let Some(file) = args.get(1) else { die("validate needs a file") };
            let text = std::fs::read_to_string(file).unwrap_or_else(|e| die(&format!("{file}: {e}")));
            match Def::parse(&text) {
                Ok(d) => println!("ok: {} ({})", d.name, match (d.has_dark(), d.has_light()) {
                    (true, true) => "dark+light",
                    (true, false) => "dark",
                    _ => "light",
                }),
                Err(e) => die(&format!("{file}: {e}")),
            }
        }
        Some("export") => {
            let Some(name) = args.get(1) else { die("export needs a theme name") };
            let id = src.find(name).map(|f| f.0).unwrap_or_else(|| name.clone());
            print!("{}", builtin::source(&id).unwrap_or_else(|| die(&format!("`{name}` is not a built-in theme"))));
        }
        Some("-h") | Some("--help") | Some("help") | None => println!("{HELP}"),
        Some(other) => die(&format!("unknown command `{other}`\n{HELP}")),
    }
}
