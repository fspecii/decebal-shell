//! `decthemectl apply-system` and `decthemectl system on|off|status|target|set`.

use super::{apply_now, current_palette, off, run_debounced, status, targets, Config, Env, Opts, Report};
use crate::theme::live::Source;
use crate::theme::model::Selection;

pub const HELP: &str = "decthemectl apply-system [--dry-run] [--only gtk,qt,...] [--json]
                                 carry the current theme to the rest of the desktop (GTK/libadwaita, Qt, terminals via Omarchy, X11, browser colour)
decthemectl system on|off        \"Apply to all apps\": on applies now and after every theme/style/font/wallpaper-colour change;
                                 off puts every changed file and setting back (originals are saved by the first apply)
decthemectl system status [--json]
decthemectl system target NAME on|off|default    NAME: omarchy browser editors hooks gtk qt kvantum x11 terminals
decthemectl system set KEY VALUE KEY: icon-theme cursor-theme signals gtk-font  (VALUE none = unset; signals and gtk-font take on|off:
                                 gtk-font, on by default, writes the theme's font to GTK's settings.ini and gsettings when it names an installed one)";

fn print_report(r: &Report, dry: bool) {
    if !r.applied.is_empty() {
        println!("{}: {}", if dry { "would apply" } else { "applied" }, r.applied.join(", "));
    }
    for (t, why) in &r.skipped {
        println!("skipped {t}: {why}");
    }
    for w in &r.wrote {
        println!("  {} {}", if dry { "would write" } else { "wrote" }, w.display());
    }
    if r.wrote.is_empty() {
        println!("nothing to change ({} files already up to date)", r.unchanged);
    }
    for n in &r.notes {
        println!("note: {n}");
    }
    for w in &r.warnings {
        eprintln!("warning: {w}");
    }
}

fn fallback_palette(src: &Source) -> super::Palette {
    super::Palette::from_theme(&src.build(&Selection::default(), true).0)
}

pub fn run(args: &[String], src: &Source) -> Result<(), String> {
    let json = args.iter().any(|a| a == "--json");
    match args.first().map(String::as_str) {
        Some("apply-system") => {
            let mut opts = Opts::default();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--dry-run" => opts.dry_run = true,
                    "--json" => {}
                    "--only" => {
                        i += 1;
                        opts.only = Some(args.get(i).ok_or("--only needs a list")?.split(',').map(String::from).collect());
                    }
                    "--debounced" => {
                        i += 1;
                        let nonce = args.get(i).ok_or("--debounced needs a token")?;
                        run_debounced(src, nonce)?;
                        return Ok(());
                    }
                    other => return Err(format!("unknown option {other}")),
                }
                i += 1;
            }
            let r = apply_now(src, &opts)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({"applied": r.applied, "skipped": r.skipped, "wrote": r.wrote, "unchanged": r.unchanged, "notes": r.notes, "warnings": r.warnings, "dry_run": opts.dry_run})
                );
            } else {
                print_report(&r, opts.dry_run);
            }
            Ok(())
        }
        Some("system") => system(&args[1..], src, json),
        _ => Err("not a system command".into()),
    }
}

fn system(args: &[String], src: &Source, json: bool) -> Result<(), String> {
    let mut cfg = Config::load(&src.shell_json);
    let env = Env::discover();
    match args.first().map(String::as_str) {
        Some("on") => {
            cfg.enabled = true;
            cfg.save(&src.shell_json)?;
            let r = apply_now(src, &Opts::default())?;
            println!("Apply to all apps: on");
            print_report(&r, false);
            Ok(())
        }
        Some("off") => {
            let p = current_palette(src).map(|x| x.0).unwrap_or_else(|_| fallback_palette(src));
            let r = off(&env, &cfg, &p)?;
            cfg.enabled = false;
            cfg.save(&src.shell_json)?;
            println!("Apply to all apps: off ({} files restored)", r.wrote.len());
            for w in &r.wrote {
                println!("  restored {}", w.display());
            }
            for n in &r.notes {
                println!("note: {n}");
            }
            for w in &r.warnings {
                eprintln!("warning: {w}");
            }
            Ok(())
        }
        Some("status") | None => {
            let st = status(&env, &cfg);
            let applied = super::applied_theme(&env);
            if json {
                let t: Vec<_> = st.iter().map(|s| serde_json::json!({"id": s.id, "title": s.title, "on": s.on, "unavailable": s.unavailable, "files": s.files})).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"enabled": cfg.enabled, "applied_theme": applied, "icon_theme": cfg.icon_theme, "cursor_theme": cfg.cursor_theme, "signals": cfg.signals, "gtk_font": cfg.option("gtk.font"), "targets": t})
                    )
                    .unwrap()
                );
            } else {
                println!("Apply to all apps: {}  (last applied theme: {})", if cfg.enabled { "on" } else { "off" }, applied.as_deref().unwrap_or("never"));
                for s in st {
                    let state = match (&s.unavailable, s.on) {
                        (Some(why), _) => format!("skipped ({why})"),
                        (None, true) => format!("on, {} files", s.files),
                        (None, false) => "off".into(),
                    };
                    println!("  {:<8} {:<26} {}", s.id, s.title, state);
                }
                if let Some(i) = &cfg.icon_theme {
                    println!("  icon theme: {i}");
                }
                if let Some(c) = &cfg.cursor_theme {
                    println!("  cursor theme: {c}");
                }
            }
            Ok(())
        }
        Some("target") => {
            let (Some(name), Some(v)) = (args.get(1), args.get(2)) else { return Err("target needs NAME and on|off|default".into()) };
            if !targets().iter().any(|t| t.id() == name) {
                return Err(format!("unknown target `{name}`"));
            }
            match v.as_str() {
                "on" => cfg.targets.insert(name.clone(), true),
                "off" => cfg.targets.insert(name.clone(), false),
                "default" => cfg.targets.remove(name),
                _ => return Err("value must be on, off or default".into()),
            };
            cfg.save(&src.shell_json)?;
            if cfg.enabled {
                print_report(&apply_now(src, &Opts::default())?, false);
            }
            Ok(())
        }
        Some("set") => {
            let (Some(k), Some(v)) = (args.get(1), args.get(2)) else { return Err("set needs KEY VALUE".into()) };
            let unset = v == "none" || v.is_empty();
            match k.as_str() {
                "icon-theme" => cfg.icon_theme = (!unset).then(|| v.clone()),
                "cursor-theme" => cfg.cursor_theme = (!unset).then(|| v.clone()),
                "signals" => cfg.signals = matches!(v.as_str(), "true" | "on" | "1" | "yes"),
                "gtk-font" => cfg.set_option("gtk.font", matches!(v.as_str(), "true" | "on" | "1" | "yes" | "default")),
                other => return Err(format!("unknown key `{other}`")),
            }
            cfg.save(&src.shell_json)?;
            if cfg.enabled {
                print_report(&apply_now(src, &Opts::default())?, false);
            }
            Ok(())
        }
        Some(other) => Err(format!("unknown system command `{other}`\n{HELP}")),
    }
}
