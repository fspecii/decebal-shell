//! `decthemectl profile ...`: the command-line side of the Configuration pane.

use super::manifest::Origin;
use super::parts::{Part, PartSet};
use super::store::{self, CaptureOpts, Env, Preset, Status};
use crate::settings::io::Paths;

pub const HELP: &str = "decthemectl profile list [--json]
decthemectl profile show NAME                 files, parts and the commands a preset would run
decthemectl profile save NAME [--parts LIST|all] [--description TEXT] [--embed-wallpaper] [--overwrite]
decthemectl profile apply NAME [--dry-run] [--allow-commands]
decthemectl profile undo NAME                 put back what applying NAME replaced (the restore point before-NAME)
decthemectl profile duplicate NAME [NEW]      editable copy (also of a built-in preset)
decthemectl profile rename NAME NEW | delete NAME
decthemectl profile export NAME FILE          a shareable FILE.decebal-preset
decthemectl profile import FILE [--strip-commands | --allow-commands] [--apply]
decthemectl profile parts                     the parts a preset can contain

Presets live in ~/.config/decebal/presets/NAME/ (DECEBAL_PRESETS). Built-in presets are read-only.
LIST is comma separated: shell,fonts,icons,hotcorners,dock,pinned,rdwm,keys,autostart,wallpaper
(default: everything except pinned, autostart and icons). Applying always saves a restore point first and validates every
file before writing. A preset that would run commands (autostart, plugins, scratchpads, spawn shortcuts, custom hot
corners) is refused unless you pass --allow-commands; import with --strip-commands removes them instead.";

fn parse_parts(s: &str) -> Result<PartSet, String> {
    if s == "all" {
        return Ok(PartSet::new(Part::ALL));
    }
    let mut v = Vec::new();
    for w in s.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        v.push(Part::parse(w).ok_or_else(|| format!("unknown part `{w}` (decthemectl profile parts)"))?);
    }
    if v.is_empty() {
        return Err("--parts needs at least one part".into());
    }
    Ok(PartSet::new(v))
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// Positional arguments: everything that is not a flag or a flag's value.
fn positional(args: &[String]) -> Vec<String> {
    let with_value = ["--parts", "--description"];
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if with_value.contains(&a.as_str()) {
            skip = true;
        } else if !a.starts_with("--") {
            out.push(a.clone());
        }
    }
    out
}

fn print_commands(p: &Preset) {
    for c in p.commands() {
        println!("  runs: {}  ({})", c.command, c.source);
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let env = Env::discover();
    let paths = Paths::discover();
    let pos = positional(args.get(1..).unwrap_or(&[]));
    let name = |i: usize, what: &str| pos.get(i).cloned().ok_or_else(|| format!("{what} needs a preset name\n\n{HELP}"));
    match args.first().map(String::as_str) {
        Some("list") => {
            let (all, bad) = store::list(&env);
            if flag(args, "--json") {
                let v: Vec<_> = all
                    .iter()
                    .map(|p| serde_json::json!({"id": p.id, "name": p.manifest.name, "description": p.manifest.description, "parts": p.manifest.parts.ids(), "origin": p.manifest.origin.as_str(), "commands": p.commands().len()}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                for p in &all {
                    let kind = match p.manifest.origin {
                        Origin::Builtin => "built-in",
                        Origin::Imported => "imported",
                        Origin::Backup => "restore point",
                        Origin::Local => "yours",
                    };
                    println!("{:<28} {:<13} {}", p.manifest.name, kind, p.manifest.parts.ids().join(","));
                }
                for (n, e) in bad {
                    println!("{n:<28} ERROR: {e}");
                }
            }
            Ok(())
        }
        Some("parts") => {
            for p in Part::ALL {
                println!("{:<11} {}{}", p.id(), p.detail(), if p.default_on() { "" } else { "  (off by default)" });
            }
            Ok(())
        }
        Some("show") => {
            let p = store::find(&env, &name(0, "show")?).ok_or("no such preset")?;
            println!("{}  [{}]\n{}", p.manifest.name, p.manifest.origin.as_str(), p.manifest.description);
            println!("parts: {}  ({})", p.manifest.parts.ids().join(", "), if p.manifest.merge { "merge" } else { "replace" });
            for (f, d) in &p.files {
                println!("  {f}  {} bytes", d.len());
            }
            print_commands(&p);
            let pl = store::plan(&env, &p);
            for c in &pl.changes {
                println!("would change: {} ({:?}, {} key differences)", c.label, c.status, c.total);
            }
            Ok(())
        }
        Some("save") => {
            let parts = value(args, "--parts").map(|s| parse_parts(&s)).transpose()?.unwrap_or_else(PartSet::defaults);
            let n = name(0, "save")?;
            let opts = CaptureOpts { name: n.clone(), description: value(args, "--description").unwrap_or_default(), author: String::new(), parts, embed_wallpaper: flag(args, "--embed-wallpaper") };
            let (mut p, notes) = store::capture(&env, &opts)?;
            p.id = if flag(args, "--overwrite") { p.id } else { store::unique_id(&env, &p.manifest.name) };
            if !flag(args, "--overwrite") && env.presets_dir.join(super::manifest::slug(&n)).exists() {
                return Err(format!("a preset called `{n}` already exists (use --overwrite)"));
            }
            store::save(&env, &p, flag(args, "--overwrite"))?;
            for note in notes {
                eprintln!("note: {note}");
            }
            println!("saved preset `{}` ({})", p.manifest.name, p.manifest.parts.ids().join(", "));
            Ok(())
        }
        Some("apply") => {
            let p = store::find(&env, &name(0, "apply")?).ok_or("no such preset (decthemectl profile list)")?;
            apply_preset(&env, &paths, &p, flag(args, "--dry-run"), flag(args, "--allow-commands"), true)
        }
        Some("undo") => {
            let n = name(0, "undo")?;
            let rp = store::find(&env, &format!("before-{}", super::manifest::slug(&n))).ok_or("there is no restore point for that preset")?;
            apply_preset(&env, &paths, &rp, false, true, false)
        }
        Some("duplicate") => {
            let p = store::duplicate(&env, &name(0, "duplicate")?, pos.get(1).map(String::as_str))?;
            println!("created `{}`", p.manifest.name);
            Ok(())
        }
        Some("rename") => {
            let p = store::rename(&env, &name(0, "rename")?, pos.get(1).ok_or("rename needs the new name")?)?;
            println!("renamed to `{}`", p.manifest.name);
            Ok(())
        }
        Some("delete") => {
            let p = store::find(&env, &name(0, "delete")?).ok_or("no such preset")?;
            store::delete(&env, &p.id)?;
            println!("deleted `{}`", p.manifest.name);
            Ok(())
        }
        Some("export") => {
            let p = store::find(&env, &name(0, "export")?).ok_or("no such preset")?;
            let mut file = pos.get(1).cloned().ok_or("export needs a file name")?;
            if !file.ends_with(&format!(".{}", super::EXT)) {
                file.push_str(&format!(".{}", super::EXT));
            }
            let n = store::export_to(&p, std::path::Path::new(&file))?;
            println!("wrote {file} ({n} bytes)");
            Ok(())
        }
        Some("import") => {
            let file = pos.first().ok_or("import needs a file")?;
            let p = store::read_bundle_file(std::path::Path::new(file))?;
            let cmds = p.commands();
            let (strip, allow) = (flag(args, "--strip-commands"), flag(args, "--allow-commands"));
            if !cmds.is_empty() && !strip && !allow {
                println!("This preset would make your desktop run {} command(s):", cmds.len());
                print_commands(&p);
                return Err("not imported: pass --strip-commands to drop them or --allow-commands to keep them".into());
            }
            let installed = store::install(&env, p, strip)?;
            println!("imported `{}`{}", installed.manifest.name, if strip && !cmds.is_empty() { format!(" ({} command(s) removed)", cmds.len()) } else { String::new() });
            if flag(args, "--apply") {
                return apply_preset(&env, &paths, &installed, false, allow, true);
            }
            Ok(())
        }
        _ => Err(format!("unknown profile command\n\n{HELP}")),
    }
}

fn apply_preset(env: &Env, paths: &Paths, p: &Preset, dry: bool, allow: bool, restore_point: bool) -> Result<(), String> {
    let pl = store::plan(env, p);
    if !pl.errors.is_empty() {
        return Err(pl.errors.join("; "));
    }
    for c in &pl.changes {
        let what = match c.status {
            Status::New => "create",
            Status::Changed => "change",
            Status::Same => "keep ",
        };
        println!("{what} {}", c.label);
        for l in &c.lines {
            println!("    {l}");
        }
        if c.total > c.lines.len() {
            println!("    ... {} more", c.total - c.lines.len());
        }
    }
    for n in &pl.notes {
        println!("note: {n}");
    }
    if !pl.new_commands.is_empty() {
        println!("this preset would make your desktop run:");
        for c in &pl.new_commands {
            println!("  {}  ({})", c.command, c.source);
        }
        if !allow && !dry {
            return Err("not applied: pass --allow-commands if you trust these commands".into());
        }
    }
    if dry {
        println!("(dry run: nothing written)");
        return Ok(());
    }
    let r = store::apply(env, p, restore_point)?;
    for w in &r.warnings {
        eprintln!("note: {w}");
    }
    if r.rdwm_changed {
        match crate::theme::rdwm::reload(paths) {
            Ok(true) => println!("rdwm reloaded"),
            Ok(false) => println!("rdwm is not running; it reads the file when it starts"),
            Err(e) => eprintln!("note: rdwm did not reload: {e}"),
        }
    }
    println!("applied `{}`{}", p.manifest.name, r.restore_point.map(|id| format!(" (undo: decthemectl profile undo {})", id.trim_start_matches("before-"))).unwrap_or_default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_parse() {
        assert_eq!(parse_parts("shell,dock").unwrap().ids(), ["shell", "dock"]);
        assert_eq!(parse_parts("all").unwrap().0.len(), Part::ALL.len());
        assert!(parse_parts("shell,nope").is_err());
        assert!(parse_parts(" , ").is_err());
    }

    #[test]
    fn positional_skips_flag_values() {
        let a: Vec<String> = ["My Preset", "--parts", "shell,dock", "--embed-wallpaper", "out.file"].iter().map(|s| s.to_string()).collect();
        assert_eq!(positional(&a), ["My Preset", "out.file"]);
        assert_eq!(value(&a, "--parts").as_deref(), Some("shell,dock"));
        assert!(flag(&a, "--embed-wallpaper") && !flag(&a, "--dry-run"));
    }
}
