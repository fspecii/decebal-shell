//! `decthemectl theme ...`: the command line of the theme engine. [`run`] returns the text to print so tests can drive it.

use super::archive::Limits;
use super::manifest;
use super::package::{self, Report};
use super::{apply, capture, schema, store};
use crate::settings::io::Paths;
use crate::theme::live::Source;
use std::path::{Path, PathBuf};

pub const HELP: &str = "decthemectl theme list [--json]                 installed themes (built-in, user, system) with version and author
decthemectl theme render ID --mode dark|light --png FILE [--palette ID]
                                              isolated native preview; never applies to the desktop
decthemectl theme info ID                     everything about one theme
decthemectl theme schema [--json]             every key a theme.toml can set, its type and range, and whether the shell applies it yet
decthemectl theme validate PATH               check a theme folder, a .dtheme file or a single theme.toml (line numbers for every mistake)
decthemectl theme new ID [--name N] [--from BASE]
                                              start a theme in ~/.config/decebal/themes/ID/ (BASE: a theme to copy, default macos)
decthemectl theme install PATH [--force] [--apply]
                                              install a folder or .dtheme (validated; packages are data only)
decthemectl theme uninstall ID                remove an installed theme
decthemectl theme pack DIR [-o FILE]          make a .dtheme (writes the content hash into the manifest)
decthemectl theme unpack FILE [DIR]           extract a .dtheme into a new folder to edit it
decthemectl theme hash DIR [--write]          print the content hash (and store it in theme.toml with --write)
decthemectl theme apply ID [--preview SECS] [--no-rdwm] [--keep-widgets|--replace-widgets]
                                              set colours, fonts, titlebars, bar, dock, wallpaper, widgets... in one step; --preview
                                              puts everything back after SECS seconds (default 10)
decthemectl theme undo                        restore the files as they were before the last apply
decthemectl theme duplicate ID NEWID [--name N]
                                              an editable copy of any theme
decthemectl theme export ID [FILE]            write theme ID as a .dtheme (default ./ID.dtheme)
decthemectl theme snapshot ID [--name N]      create a theme from the current settings (colours, titlebars, gaps, wallpaper, dock, widgets)

Themes live in ~/.config/decebal/themes/ID/ (override with $DECEBAL_THEMES) and /usr/share/decebal/themes/. See docs/theme-engine.md.";

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// Positional arguments (not options or their values).
fn pos<'a>(args: &'a [String], valued: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if valued.contains(&args[i].as_str()) {
            i += 2;
        } else if args[i].starts_with('-') && args[i] != "-" {
            i += 1;
        } else {
            out.push(args[i].as_str());
            i += 1;
        }
    }
    out
}

fn report_text(r: &Report, what: &str) -> String {
    let mut s = String::new();
    for w in r.all_warnings() {
        s.push_str(&format!("warning: {w}\n"));
    }
    if r.ok() {
        let def = r.analysis.def.as_ref();
        let meta = |k: &str| def.and_then(|d| d.extra.get("meta")).and_then(|m| m.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
        s.push_str(&format!(
            "ok: {} {} ({}), {what}, {} files, {} KiB, {}\n",
            def.map(|d| d.name.as_str()).unwrap_or(""),
            meta("version"),
            r.id.as_deref().unwrap_or("?"),
            r.files,
            r.total_bytes.div_ceil(1024),
            r.content_hash
        ));
    }
    s
}

fn paths_note(a: &apply::Applied) -> String {
    let mut s =
        format!("applied {} ({}); changed: {}", a.theme_id, if a.dark { "dark" } else { "light" }, if a.files.is_empty() { "nothing (already applied)".to_string() } else { a.files.join(", ") });
    if a.reloaded {
        s.push_str("; rdwm reloaded");
    }
    s.push('\n');
    for n in &a.notes {
        s.push_str(&format!("  note: {n}\n"));
    }
    if !a.undo_id.is_empty() {
        s.push_str("  undo with: decthemectl theme undo\n");
    }
    s
}

fn json_list(infos: &[store::Info], current: &str) -> String {
    let v: Vec<_> = infos
        .iter()
        .map(|i| {
            serde_json::json!({"id": i.id, "name": i.name, "version": i.version, "author": i.author, "license": i.license, "description": i.description, "tags": i.tags,
            "origin": i.origin.as_str(), "dir": i.dir, "dark": i.has_dark, "light": i.has_light, "error": i.error, "widgets": i.widgets, "current": i.id == current,
            "integrity": match i.hash { store::HashState::None => "none", store::HashState::Ok => "ok", store::HashState::Mismatch => "MISMATCH" }})
        })
        .collect();
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

/// Runs `decthemectl theme <args>`.
pub fn run(args: &[String], src: &Source, paths: &Paths) -> Result<String, String> {
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => Ok(format!("{HELP}\n")),
        Some("list") => {
            let sel = src.selection();
            let cur = src.find(&sel.name).map(|f| f.0).unwrap_or_default();
            let infos = store::list(src);
            if flag(rest, "--json") {
                return Ok(json_list(&infos, &cur));
            }
            let mut s = String::new();
            for i in &infos {
                let var = match (i.has_dark, i.has_light) {
                    (true, true) => "dark+light",
                    (true, false) => "dark",
                    (false, true) => "light",
                    _ => "-",
                };
                let err = i.error.as_deref().map(|e| format!("  ERROR: {e}")).unwrap_or_default();
                let seal = match i.hash {
                    store::HashState::Mismatch => "  [HASH MISMATCH]",
                    _ => "",
                };
                s.push_str(&format!("{} {:<16} {:<18} {:<7} {:<11} {:<9} {}{seal}{err}\n", if i.id == cur { "*" } else { " " }, i.id, i.name, i.version, var, i.origin.as_str(), i.author));
            }
            Ok(s)
        }
        Some("render") => {
            let id = rest.first().filter(|s| !s.starts_with("--")).ok_or("render needs a theme id")?;
            let mut sel = crate::theme::Selection { name: id.clone(), mode: crate::theme::Mode::Dark, ..Default::default() };
            let mut out = None;
            let mut args = rest[1..].iter();
            while let Some(key) = args.next() {
                let val = args.next().ok_or("render option needs a value")?;
                match key.as_str() {
                    "--mode" => {
                        sel.mode = crate::theme::Mode::parse(val).filter(|m| *m != crate::theme::Mode::Auto).ok_or("render mode is dark or light")?;
                    }
                    "--palette" => sel.palette = Some(val.clone()),
                    "--png" => out = Some(std::path::PathBuf::from(val)),
                    _ => return Err(format!("unknown render option {key}")),
                }
            }
            let out = out.ok_or("render needs --png FILE")?;
            let path = super::preview::generate(src, &sel)?;
            std::fs::copy(&path, &out).map_err(|e| e.to_string())?;
            Ok(format!("native preview: {} (cache {})\n", out.display(), path.display()))
        }
        Some("info") => {
            let id = pos(rest, &[]).first().copied().ok_or("info needs a theme id")?;
            let (id, _) = src.find(id).ok_or_else(|| format!("no theme `{id}`"))?;
            let i = store::info_for(src, &id);
            let def = src.load_def(&id)?;
            let mut s = format!(
                "{} ({})\n  id {}  version {}  author {}  license {}\n  origin {}{}\n  {}\n",
                i.name,
                if i.has_dark && i.has_light {
                    "dark+light"
                } else if i.has_dark {
                    "dark"
                } else {
                    "light"
                },
                i.id,
                i.version,
                i.author,
                i.license,
                i.origin.as_str(),
                i.dir.as_ref().map(|d| format!(" ({})", d.display())).unwrap_or_default(),
                i.description
            );
            if !i.tags.is_empty() {
                s.push_str(&format!("  tags: {}\n", i.tags.join(", ")));
            }
            for sec in ["font", "wallpaper", "decoration", "bar", "dock", "branding", "taskbar", "icons", "hot_corners", "control_center", "rdwm", "widgets"] {
                if let Some(v) = def.extra.get(sec) {
                    s.push_str(&format!("  [{sec}] {}\n", v.to_string().replace("\"", "")));
                }
            }
            if let Some(e) = &i.error {
                s.push_str(&format!("  ERROR: {e}\n"));
            }
            Ok(s)
        }
        Some("schema") => {
            let rows = schema::rows();
            if flag(rest, "--json") {
                let v: Vec<_> = rows.iter().map(|r| serde_json::json!({"key": r.path, "type": r.ty, "description": r.desc, "applied": matches!(r.status, schema::Status::Wired), "note": match r.status { schema::Status::Pending(n) => n, _ => "" }})).collect();
                return Ok(serde_json::to_string_pretty(&v).unwrap_or_default() + "\n");
            }
            let mut s = format!("theme.toml format {} - keys marked [pending] are accepted and stored but the shell does not act on them yet\n\n", schema::FORMAT);
            for r in rows {
                let tag = match r.status {
                    schema::Status::Wired => String::new(),
                    schema::Status::Pending(n) => format!("  [pending: {n}]"),
                };
                s.push_str(&format!("{:<34} {:<34} {}{tag}\n", r.path, r.ty, r.desc));
            }
            Ok(s)
        }
        Some("validate") => {
            let p = *pos(rest, &[]).first().ok_or("validate needs a folder, a .dtheme or a theme.toml")?;
            let path = Path::new(p);
            if path.is_file() && path.extension().is_some_and(|e| e == "toml") {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{p}: {e}"))?;
                let a = if manifest::is_v1(&text) {
                    manifest::analyze(&text)
                } else {
                    return crate::theme::model::Def::parse(&text).map(|d| format!("ok: {} (legacy theme file; `theme duplicate` upgrades it to format 1)\n", d.name)).map_err(|e| format!("{p}: {e}"));
                };
                let mut s = String::new();
                for d in &a.diags {
                    s.push_str(&format!("{p}: {d}\n"));
                }
                return if a.ok() { Ok(format!("{s}ok: {}\n", a.def.map(|d| d.name).unwrap_or_default())) } else { Err(s.trim_end().to_string()) };
            }
            let lim = Limits::default();
            let entries = package::read_any(path, &lim)?;
            let r = package::check(&entries, &lim);
            if r.ok() {
                Ok(report_text(&r, if path.is_dir() { "folder" } else { "archive" }))
            } else {
                Err(format!("{}{}", report_text(&r, ""), r.errors().join("\n")))
            }
        }
        Some("new") => {
            let id = *pos(rest, &["--name", "--from"]).first().ok_or("new needs a theme id")?;
            let dir = store::scaffold(src, id, opt(rest, "--name"), opt(rest, "--from"))?;
            Ok(format!("created {}\nedit theme.toml, then: decthemectl theme validate {} && decthemectl theme apply {id}\n", dir.display(), dir.display()))
        }
        Some("install") => {
            let p = *pos(rest, &[]).first().ok_or("install needs a folder or .dtheme")?;
            let (inst, rep) = store::install(src, Path::new(p), flag(rest, "--force"))?;
            let mut s = report_text(&rep, "installed");
            s.push_str(&format!("installed {} -> {}{}\n", inst.id, inst.dir.display(), if inst.replaced { " (replaced the old one)" } else { "" }));
            if flag(rest, "--apply") {
                s.push_str(&paths_note(&apply::apply(src, paths, &inst.id, &src.selection())?));
            }
            Ok(s)
        }
        Some("uninstall") => {
            let id = *pos(rest, &[]).first().ok_or("uninstall needs a theme id")?;
            Ok(format!("removed {}\n", store::uninstall(src, id)?.display()))
        }
        Some("pack") => {
            let dir = *pos(rest, &["-o"]).first().ok_or("pack needs a theme folder")?;
            let (bytes, rep) = package::pack_dir(Path::new(dir), &Limits::default())?;
            let out = opt(rest, "-o").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("{}.dtheme", rep.id.clone().unwrap_or_else(|| "theme".into()))));
            std::fs::write(&out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(format!("{}{} ({} KiB, {})\n", report_text(&rep, "packed"), out.display(), bytes.len().div_ceil(1024), rep.content_hash))
        }
        Some("unpack") => {
            let p = *pos(rest, &[]).first().ok_or("unpack needs a .dtheme file")?;
            let lim = Limits::default();
            let entries = package::read_archive_file(Path::new(p), &lim)?;
            let r = package::check(&entries, &lim);
            if !r.ok() {
                return Err(r.errors().join("\n"));
            }
            let dest = pos(rest, &[]).get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r.id.clone().unwrap_or_else(|| "theme".into())));
            package::unpack_to(&entries, &dest)?;
            Ok(format!("unpacked {} files to {}\n", entries.len(), dest.display()))
        }
        Some("hash") => {
            let dir = *pos(rest, &[]).first().ok_or("hash needs a theme folder")?;
            let mut entries = package::read_dir(Path::new(dir), &Limits::default())?;
            let h = package::content_hash(&entries);
            if flag(rest, "--write") {
                let m = entries.iter_mut().find(|e| e.path == package::MANIFEST).ok_or("no theme.toml")?;
                let text = package::with_hash(&String::from_utf8_lossy(&m.data), &h)?;
                std::fs::write(Path::new(dir).join(package::MANIFEST), text).map_err(|e| e.to_string())?;
            }
            Ok(format!("{h}\n"))
        }
        Some("apply") => {
            let id = *pos(rest, &["--preview"]).first().ok_or("apply needs a theme id")?;
            let mut sel = src.selection();
            if flag(rest, "--no-rdwm") {
                sel.sync_rdwm = false;
            }
            if flag(rest, "--preview") || opt(rest, "--preview").is_some() {
                let secs: u64 = opt(rest, "--preview").and_then(|v| v.parse().ok()).unwrap_or(10).clamp(1, 600);
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let mut p = apply::plan_with_widgets(src, paths, id, &sel, apply::WidgetChoice::Keep)?;
                p.follow_wallpaper = false;
                let a = apply::execute(src, paths, p)?;
                eprintln!("{}previewing for {secs} s (Ctrl-C keeps the preview off and restores now)...", paths_note(&a));
                if !a.undo_id.is_empty() {
                    install_interrupt();
                    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
                    while std::time::Instant::now() < end && !INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed) && !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    apply::undo(src, paths, Some(&a.undo_id))?;
                }
                return Ok("preview over, everything restored\n".into());
            }
            if flag(rest, "--keep-widgets") && flag(rest, "--replace-widgets") {
                return Err("Choose only one widget option".into());
            }
            let choice = if flag(rest, "--replace-widgets") {
                apply::WidgetChoice::Replace
            } else if flag(rest, "--keep-widgets") {
                apply::WidgetChoice::Keep
            } else {
                apply::WidgetChoice::Ask
            };
            let plan = apply::plan_with_widgets(src, paths, id, &sel, choice)?;
            Ok(paths_note(&apply::execute(src, paths, plan)?))
        }
        Some("undo") => Ok(format!("restored the files from before `{}`\n", apply::undo(src, paths, None)?)),
        Some("duplicate") => {
            let p = pos(rest, &["--name"]);
            let (id, new) = (p.first().ok_or("duplicate needs ID NEWID")?, p.get(1).ok_or("duplicate needs ID NEWID")?);
            Ok(format!("created {}\n", store::duplicate(src, id, new, opt(rest, "--name"))?.display()))
        }
        Some("export") => {
            let p = pos(rest, &[]);
            let id = p.first().ok_or("export needs a theme id")?;
            let (id, _) = src.find(id).ok_or_else(|| format!("no theme `{id}`"))?;
            let out = p.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("{id}.dtheme")));
            let r = store::export(src, &id, &out)?;
            Ok(format!("{}{}\n", report_text(&r, "exported"), out.display()))
        }
        Some("snapshot") => {
            let id = *pos(rest, &["--name"]).first().ok_or("snapshot needs a new theme id")?;
            let name = opt(rest, "--name").map(String::from).unwrap_or_else(|| id.to_string());
            let c = capture::from_current(src, paths, id, &name)?;
            let mut s = format!("created {}\n", c.dir.display());
            for n in c.notes {
                s.push_str(&format!("  note: {n}\n"));
            }
            Ok(s)
        }
        Some(other) => Err(format!("unknown theme command `{other}`\n{HELP}")),
    }
}

static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn install_interrupt() {
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as usize);
        libc::signal(libc::SIGTERM, on_signal as *const () as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::engine::apply::tests::{install_sunset, sandbox};

    fn run_ok(args: &[&str], src: &Source, paths: &Paths) -> String {
        run(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), src, paths).unwrap_or_else(|e| panic!("{args:?}: {e}"))
    }
    fn run_err(args: &[&str], src: &Source, paths: &Paths) -> String {
        run(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), src, paths).unwrap_err()
    }

    #[test]
    fn the_whole_workflow_from_new_to_apply_and_undo() {
        let (d, src, paths) = sandbox("cli");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        let out = run_ok(&["new", "mine", "--from", "nord"], &src, &paths);
        assert!(out.contains("created"));
        let dir = src.themes_dir.join("mine");
        // An unset USER leaves the optional author empty and emits a warning before success.
        let validation = run_ok(&["validate", dir.to_str().unwrap()], &src, &paths);
        assert!(validation.lines().any(|line| line.starts_with("ok: Mine")), "{validation}");
        let list = run_ok(&["list"], &src, &paths);
        assert!(list.contains("mine") && list.contains("nord") && list.contains("built-in") && list.contains("user"), "{list}");
        let j: serde_json::Value = serde_json::from_str(&run_ok(&["list", "--json"], &src, &paths)).unwrap();
        assert!(j.as_array().unwrap().iter().any(|t| t["id"] == "mine" && t["origin"] == "user"));

        let packed = d.join("mine.dtheme");
        let out = run_ok(&["pack", dir.to_str().unwrap(), "-o", packed.to_str().unwrap()], &src, &paths);
        assert!(out.contains("sha256:"));
        let validation = run_ok(&["validate", packed.to_str().unwrap()], &src, &paths);
        assert!(validation.lines().any(|line| line.starts_with("ok:")), "{validation}");
        run_ok(&["uninstall", "mine"], &src, &paths);
        assert!(!dir.exists());
        let out = run_ok(&["install", packed.to_str().unwrap(), "--apply"], &src, &paths);
        assert!(out.contains("installed mine") && out.contains("applied mine"), "{out}");
        assert_eq!(src.selection().name, "mine");
        assert!(run_err(&["install", packed.to_str().unwrap()], &src, &paths).contains("already installed"));
        run_ok(&["undo"], &src, &paths);
        assert_eq!(src.selection().name, "macos");
        let un = d.join("unpacked");
        run_ok(&["unpack", packed.to_str().unwrap(), un.to_str().unwrap()], &src, &paths);
        assert!(un.join("theme.toml").is_file());
        let h = run_ok(&["hash", un.to_str().unwrap()], &src, &paths);
        assert!(h.starts_with("sha256:"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn validation_reports_every_problem_with_its_line() {
        let (d, src, paths) = sandbox("cli-bad");
        let f = d.join("theme.toml");
        std::fs::write(&f, "format = 1\nname = \"x\"\n[colors.dark]\nbackground = \"red\"\n[decoration]\nheight = 99\n").unwrap();
        let e = run_err(&["validate", f.to_str().unwrap()], &src, &paths);
        assert!(e.contains("line 4") && e.contains("line 6"), "{e}");
        assert!(run_err(&["validate", d.join("missing").to_str().unwrap()], &src, &paths).contains("missing"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn schema_lists_keys_and_marks_the_ones_not_applied() {
        let (d, src, paths) = sandbox("cli-schema");
        let s = run_ok(&["schema"], &src, &paths);
        assert!(s.contains("colors.dark.accent") && s.contains("decoration.style") && s.contains("[pending:"), "{s}");
        let j: serde_json::Value = serde_json::from_str(&run_ok(&["schema", "--json"], &src, &paths)).unwrap();
        assert!(j.as_array().unwrap().iter().any(|r| r["key"] == "font.files" && r["applied"] == true));
        assert!(run_err(&["bogus"], &src, &paths).contains("unknown theme command"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn apply_with_preview_and_snapshot_through_the_cli() {
        let (d, src, paths) = sandbox("cli-snap");
        std::fs::write(&paths.rdwm_config, "[general]\n").unwrap();
        install_sunset(&src);
        let out = run_ok(&["apply", "sunset", "--no-rdwm"], &src, &paths);
        assert!(out.contains("applied sunset"), "{out}");
        assert_eq!(std::fs::read_to_string(&paths.rdwm_config).unwrap(), "[general]\n", "--no-rdwm leaves rdwm alone");
        let out = run_ok(&["snapshot", "snap1", "--name", "Snap One"], &src, &paths);
        assert!(out.contains("created"), "{out}");
        assert!(store::info_for(&src, "snap1").error.is_none());
        let _ = std::fs::remove_dir_all(d);
    }
}
