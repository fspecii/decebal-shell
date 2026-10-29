//! Native terminal adapters, including unpatched st (which does not read Xresources).
//! Configuration edits join the same backup/transaction as GTK and Qt. Live st
//! updates use OSC on this user's terminal PTYs in this DISPLAY; no signals/restarts.
use super::files::{Comment, Edit};
use super::{x11, Config, Ctx, Env, Palette, Plan, Target};
use std::collections::BTreeSet;
use std::io::Write;
use std::os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt},
};
use std::path::PathBuf;

pub struct Terminals;

pub fn osc(p: &Palette) -> String {
    let mut text = String::new();
    for (i, c) in x11::ansi(p).iter().enumerate() {
        text.push_str(&format!("\x1b]4;{i};{}\x07", c.hex()));
    }
    text.push_str(&format!("\x1b]10;{}\x07\x1b]11;{}\x07\x1b]12;{}\x07", p.text.hex(), p.bg.hex(), p.text.hex()));
    text
}

fn st_ptys(env: &Env) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    let Some(display) = &env.display else { return paths };
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        if entry.file_name().as_bytes().iter().any(|c| !c.is_ascii_digit()) {
            continue;
        }
        if entry.metadata().map_or(true, |m| m.uid() != unsafe { libc::geteuid() }) {
            continue;
        }
        let Ok(vars) = std::fs::read(entry.path().join("environ")) else { continue };
        let vars: Vec<&[u8]> = vars.split(|b| *b == 0).collect();
        let wanted = format!("DISPLAY={display}");
        if !vars.contains(&wanted.as_bytes()) || !vars.iter().any(|v| v.starts_with(b"TERM=st-")) {
            continue;
        }
        if let Ok(tty) = std::fs::read_link(entry.path().join("fd/0")) {
            if tty.parent().is_some_and(|p| p == std::path::Path::new("/dev/pts")) && tty.file_name().is_some_and(|n| n.as_bytes().iter().all(u8::is_ascii_digit)) {
                paths.insert(tty);
            }
        }
    }
    paths
}

fn write_st(env: &Env, text: &str) -> usize {
    st_ptys(env).iter().filter(|tty| std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK).open(tty).is_ok_and(|mut f| f.write_all(text.as_bytes()).is_ok())).count()
}

fn alacritty_colors(p: &Palette, original: &str) -> Result<String, String> {
    let mut doc = original.parse::<toml_edit::DocumentMut>().map_err(|e| e.to_string())?;
    if doc.get("colors").is_some_and(|c| !c.is_table_like()) {
        return Err("colors is not a table".into());
    }
    for group in ["primary", "normal", "bright"] {
        if doc.get("colors").and_then(|c| c.get(group)).is_some_and(|c| !c.is_table_like()) {
            return Err(format!("colors.{group} is not a table"));
        }
    }
    for (key, value) in [("foreground", p.text.hex()), ("background", p.bg.hex())] {
        doc["colors"]["primary"][key] = toml_edit::value(value);
    }
    for (group, offset) in [("normal", 0), ("bright", 8)] {
        for (i, name) in ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"].iter().enumerate() {
            doc["colors"][group][*name] = toml_edit::value(x11::ansi(p)[i + offset].hex());
        }
    }
    Ok(doc.to_string())
}

impl Target for Terminals {
    fn id(&self) -> &'static str {
        "terminals"
    }
    fn title(&self) -> &'static str {
        "Terminal colours"
    }
    fn detail(&self) -> &'static str {
        "st-256color live palette and shell startup; alacritty, kitty, ghostty and foot configuration without requiring Omarchy"
    }
    fn available(&self, env: &Env) -> Result<(), String> {
        if ["st", "st-256color", "kitty", "ghostty", "foot", "alacritty"].iter().any(|t| env.has(t) || env.config_home.join(t).is_dir()) {
            Ok(())
        } else {
            Err("no supported terminal installed".into())
        }
    }
    fn plan(&self, p: &Palette, _cfg: &Config, env: &Env) -> Plan {
        let mut plan = Plan::default();
        // Explicit native colours also cover custom configs that do not include Omarchy’s files.
        if env.has("alacritty") || env.config_home.join("alacritty").is_dir() {
            let path = env.config_home.join("alacritty/alacritty.toml");
            let original = std::fs::read_to_string(&path).unwrap_or_default();
            if let Ok(text) = alacritty_colors(p, &original) {
                plan.edits.push(Edit::Whole { path, text });
            }
        }

        if env.has("kitty") || env.config_home.join("kitty").is_dir() {
            let mut body =
                format!("foreground {}\nbackground {}\ncursor {}\nselection_background {}\nselection_foreground {}\n", p.text.hex(), p.bg.hex(), p.text.hex(), p.accent.hex(), p.accent_text.hex());
            for (i, c) in x11::ansi(p).iter().enumerate() {
                body.push_str(&format!("color{i} {}\n", c.hex()));
            }
            plan.edits.push(Edit::Block { path: env.config_home.join("kitty/kitty.conf"), comment: Comment::Hash, text: body });
        }
        if env.has("ghostty") || env.config_home.join("ghostty").is_dir() {
            let mut body = format!(
                "foreground = {}\nbackground = {}\ncursor-color = {}\nselection-background = {}\nselection-foreground = {}\n",
                p.text.hex(),
                p.bg.hex(),
                p.text.hex(),
                p.accent.hex(),
                p.accent_text.hex()
            );
            for (i, c) in x11::ansi(p).iter().enumerate() {
                body.push_str(&format!("palette = {i}={}\n", c.hex()));
            }
            plan.edits.push(Edit::Block { path: env.config_home.join("ghostty/config"), comment: Comment::Hash, text: body });
        }
        if env.has("foot") || env.config_home.join("foot").is_dir() {
            let mut sets = vec![("colors".into(), "foreground".into(), p.text.hex()[1..].into()), ("colors".into(), "background".into(), p.bg.hex()[1..].into())];
            for (i, c) in x11::ansi(p).iter().enumerate() {
                sets.push(("colors".into(), format!("{}{}", if i < 8 { "regular" } else { "bright" }, i % 8), c.hex()[1..].into()));
            }
            plan.edits.push(Edit::Ini { path: env.config_home.join("foot/foot.ini"), sets });
        }
        if env.has("st") || env.has("st-256color") {
            let palette = env.gen_dir.join("st-colors.sh");
            let escaped = osc(p).replace('\x1b', "\\033").replace('\x07', "\\007");
            plan.edits.push(Edit::Whole { path: palette.clone(), text: format!("# Generated terminal palette; no subprocesses or terminal restart.\nprintf '%b' '{escaped}'\n") });
            // A shell startup hook gives new unpatched st windows the same colours.
            // The st TERM guard prevents changing other terminal emulators.
            let quoted = palette.to_string_lossy().replace('\'', "'\\''");
            let body = format!("case \"${{TERM-}}\" in st-*) [ ! -t 1 ] || [ ! -r '{quoted}' ] || . '{quoted}' ;; esac");
            for name in [".bashrc", ".zshrc"] {
                if name == ".bashrc" || env.home.join(name).exists() || env.has("zsh") {
                    plan.edits.push(Edit::Block { path: env.home.join(name), comment: Comment::Hash, text: body.clone() });
                }
            }
        }
        plan
    }
    fn after(&self, cx: &mut Ctx) {
        let alacritty = cx.env.config_home.join("alacritty/alacritty.toml");
        if alacritty.exists() {
            if let Err(error) = alacritty_colors(cx.p, &std::fs::read_to_string(&alacritty).unwrap_or_default()) {
                cx.report.warnings.push(format!("alacritty: existing configuration preserved: {error}"));
            }
        }
        let count = write_st(cx.env, &osc(cx.p));
        if count > 0 {
            cx.st.extra.insert("stOsc".into(), serde_json::json!(true));
            cx.report.notes.push(format!("st: updated {count} terminal palette(s) without restarting"));
        }
        cx.report.notes.push("Terminal configuration is ready for new windows; st uses OSC live, other terminals use their supported reload mechanism or next launch".into());
    }
    fn undo(&self, cx: &mut Ctx) {
        if cx.st.extra.remove("stOsc").is_some() {
            let _ = write_st(cx.env, "\x1b]104\x07\x1b]110\x07\x1b]111\x07\x1b]112\x07");
            cx.report.notes.push("st: reset live OSC colours to terminal defaults".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    #[test]
    fn alacritty_preserves_comments_and_unknown_options_and_rejects_broken_tables() {
        let original = "# user\n[window]\npadding = { x = 8, y = 8 }\n[colors.primary]\nbackground = '#000000'\n";
        let result = alacritty_colors(&palette(true), original).unwrap();
        assert!(result.contains("# user") && result.contains("padding = { x = 8, y = 8 }"));
        assert!(result.parse::<toml_edit::DocumentMut>().is_ok());
        let inline = alacritty_colors(&palette(true), "colors = { primary = { background = '#000000' } }\n").unwrap();
        assert!(inline.parse::<toml_edit::DocumentMut>().is_ok());
        assert!(alacritty_colors(&palette(true), "colors = 'invalid'\n").is_err());
        assert!(alacritty_colors(&palette(true), "not valid TOML").is_err());
    }

    #[test]
    fn standalone_terminal_edits_preserve_unknown_settings_and_st_uses_osc() {
        let d = tmp("native-terminals");
        let (env, _) = env(&d, &["st", "kitty", "ghostty", "foot"]);
        let plan = Terminals.plan(&palette(true), &Config::default(), &env);
        assert!(plan.edits.iter().any(|e| e.path().ends_with(".bashrc")));
        let kitty = plan.edits.iter().find(|e| e.path().ends_with("kitty.conf")).unwrap().render(Some("font_size 13\n"));
        assert!(kitty.starts_with("font_size 13\n") && kitty.contains("color15 "));
        assert!(osc(&palette(true)).contains("\x1b]11;#1e1e1e\x07"));
    }
}
