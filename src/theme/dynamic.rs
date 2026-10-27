//! "Colours from wallpaper": finding the wallpaper, deriving the seed, and following wallpaper changes.
//!
//! The seed colour is taken from the image once and stored in the selection (`macshell.json`,
//! `"theme": {"dynamic": {"seed": ...}}`); every program then derives the palette from the seed
//! ([`super::material::generate`], applied by [`super::model::Def::with_dynamic`]). Decoding an image
//! happens only in `macthemectl dynamic`, the Theme pane and [`on_wallpaper_set`].
//!
//! Wallpaper changes macshell itself makes (machaven, the Wallpaper pane of macsettings) call
//! [`on_wallpaper_set`]. A change made elsewhere (`rdwmctl wallpaper`, `omarchy theme bg set`) is picked
//! up by running `macthemectl dynamic refresh` (a no-op unless wallpaper colours are on).

use super::builtin::DYNAMIC_THEME;
use super::live::Source;
use super::material::{self, Variant};
use super::model::{Dynamic, Selection};
use super::rdwm;
use crate::settings::io::{self, Paths};
use crate::settings::tomlcfg::{Cfg, Val};
use std::path::{Path, PathBuf};

fn expand(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) => PathBuf::from(h).join(rest),
        _ => PathBuf::from(p),
    }
}

/// The wallpaper on screen: rdwm's live state, else the `[wallpaper] image` of rdwm's config, else
/// Omarchy's `current/background` link next to the shell config.
pub fn find_wallpaper(paths: &Paths) -> Option<PathBuf> {
    let live = io::ipc_call(&paths.socket, &serde_json::json!({"cmd": "get_state"})).ok().and_then(|v| v["state"]["wallpaper"]["image"].as_str().map(String::from));
    let file = || {
        let text = std::fs::read_to_string(&paths.rdwm_config).ok()?;
        match Cfg::parse(&text).ok()?.get("wallpaper", "image")? {
            Val::Str(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    };
    live.filter(|s| !s.is_empty()).or_else(file).map(|s| expand(&s)).filter(|p| p.is_file()).or_else(|| Some(paths.shell_dir.join("current/background")).filter(|p| p.is_file()))
}

/// The wallpaper colours of `path`.
pub fn derive(path: &Path, variant: Variant, follow: bool) -> Result<Dynamic, String> {
    let seed = material::seed_from_image(path)?;
    Ok(Dynamic { seed, variant, wallpaper: Some(path.to_string_lossy().into_owned()), follow })
}

/// `sel` switched to the Caelestia theme with wallpaper colours `d`.
pub fn enable(sel: &Selection, d: Dynamic) -> Selection {
    Selection { name: DYNAMIC_THEME.into(), dynamic: Some(d), ..sel.clone() }
}

/// `sel` without wallpaper colours (Caelestia keeps its own palette again).
pub fn disable(sel: &Selection) -> Selection {
    Selection { dynamic: None, ..sel.clone() }
}

/// Re-derives the seed from `path` (or the wallpaper on screen) when wallpaper colours are on and following.
/// Returns `Ok(true)` when the selection changed. Cheap no-op when the feature is off.
pub fn refresh(src: &Source, paths: &Paths, path: Option<&Path>) -> Result<bool, String> {
    let sel = src.selection();
    let Some(cur) = sel.dynamic.clone().filter(|d| d.follow) else { return Ok(false) };
    let Some(wall) = path.map(Path::to_path_buf).or_else(|| find_wallpaper(paths)) else { return Err("no wallpaper found".into()) };
    let new = derive(&wall, cur.variant, true)?;
    if new == cur {
        return Ok(false);
    }
    rdwm::apply(src, paths, &Selection { dynamic: Some(new), ..sel })?;
    Ok(true)
}

/// Hook for the programs that change the wallpaper: follows it when wallpaper colours are on, ignores every failure.
pub fn on_wallpaper_set(path: Option<&Path>) {
    let (src, paths) = (Source::discover(), Paths::discover());
    // nothing to do (and no image to decode) unless the user turned the feature on
    if src.selection().dynamic.is_some_and(|d| d.follow) {
        let _ = refresh(&src, &paths, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::model::Rgb;

    fn sandbox(name: &str) -> (PathBuf, Source, Paths) {
        let d = std::env::temp_dir().join(format!("macshell-dyn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: d.join("themes") };
        (d, src, paths)
    }

    fn near(a: Rgb, b: Rgb) -> bool {
        [(a.0, b.0), (a.1, b.1), (a.2, b.2)].iter().all(|(x, y)| x.abs_diff(*y) <= 2)
    }

    fn wall(dir: &Path, name: &str, rgb: [u8; 3]) -> PathBuf {
        let p = dir.join(name);
        image::RgbImage::from_pixel(64, 40, image::Rgb(rgb)).save(&p).unwrap();
        p
    }

    #[test]
    fn the_wallpaper_is_found_in_rdwms_config_then_omarchys_link() {
        let (d, _src, paths) = sandbox("find");
        assert_eq!(find_wallpaper(&paths), None);
        let w = wall(&d, "a.png", [10, 20, 30]);
        std::fs::create_dir_all(paths.shell_dir.join("current")).unwrap();
        std::fs::copy(&w, paths.shell_dir.join("current/background")).unwrap();
        assert_eq!(find_wallpaper(&paths), Some(paths.shell_dir.join("current/background")));
        std::fs::write(&paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", w.display())).unwrap();
        assert_eq!(find_wallpaper(&paths), Some(w));
    }

    #[test]
    fn enabling_switches_to_caelestia_and_builds_its_palette_from_the_seed() {
        let (d, src, _paths) = sandbox("enable");
        let w = wall(&d, "red.png", [200, 30, 40]);
        let dyn_ = derive(&w, Variant::Vibrant, true).unwrap();
        assert!(near(dyn_.seed, Rgb(200, 30, 40)), "{}", dyn_.seed.hex());
        let sel = enable(&Selection { name: "nord".into(), accent: None, ..Selection::default() }, dyn_);
        src.write_selection(&sel).unwrap();
        assert_eq!(src.selection(), sel);
        let (t, problem) = src.build(&src.selection(), true);
        assert!(problem.is_none());
        assert_eq!(t.id, "caelestia");
        // the plain Caelestia dark accent is #ffb0ca; the red wallpaper moved it
        let plain = src.build(&disable(&sel), true).0;
        assert_eq!(plain.accent, Rgb(0xff, 0xb0, 0xca));
        assert_ne!(t.accent, plain.accent);
        let (hue, plain_hue) = (material::oklch(t.accent).2, material::oklch(plain.accent).2);
        assert!((hue - plain_hue).abs() > 10.0, "hue {hue} vs {plain_hue}");
        // metrics (shape) are untouched by the wallpaper colours
        assert_eq!((t.radius, t.dock_roundness, t.bar_vertical()), (plain.radius, plain.dock_roundness, true));
        // light variant is derived too
        assert!(!src.build(&Selection { mode: crate::theme::model::Mode::Light, ..src.selection() }, true).0.dark);
    }

    #[test]
    fn other_themes_ignore_stored_wallpaper_colours() {
        let (_d, src, _paths) = sandbox("other");
        let d = Dynamic { seed: Rgb(0, 0, 255), variant: Variant::TonalSpot, wallpaper: None, follow: true };
        let sel = Selection { name: "nord".into(), dynamic: Some(d), ..Selection::default() };
        assert_eq!(src.build(&sel, true).0.accent, Rgb(0x88, 0xc0, 0xd0));
    }

    #[test]
    fn refresh_follows_a_changed_wallpaper_and_ignores_it_when_off() {
        let (d, src, paths) = sandbox("refresh");
        let blue = wall(&d, "blue.png", [20, 60, 220]);
        let green = wall(&d, "green.png", [20, 200, 60]);
        // off: nothing happens, even with a wallpaper available
        assert_eq!(refresh(&src, &paths, Some(&green)), Ok(false));
        assert!(!paths.shell_json().exists());
        let sel = Selection { sync_rdwm: false, ..enable(&Selection::default(), derive(&blue, Variant::TonalSpot, true).unwrap()) };
        src.write_selection(&sel).unwrap();
        assert_eq!(refresh(&src, &paths, Some(&blue)), Ok(false), "same wallpaper, same seed: no write");
        assert_eq!(refresh(&src, &paths, Some(&green)), Ok(true));
        let after = src.selection().dynamic.unwrap();
        assert!(near(after.seed, Rgb(20, 200, 60)));
        assert_eq!(after.wallpaper.as_deref(), Some(green.to_str().unwrap()));
        // not following: left alone
        src.write_selection(&Selection { dynamic: Some(Dynamic { follow: false, ..after }), ..src.selection() }).unwrap();
        assert_eq!(refresh(&src, &paths, Some(&blue)), Ok(false));
        assert!(near(src.selection().dynamic.unwrap().seed, Rgb(20, 200, 60)));
        // an unreadable image is an error and changes nothing
        src.write_selection(&Selection { dynamic: Some(Dynamic { follow: true, ..src.selection().dynamic.unwrap() }), ..src.selection() }).unwrap();
        assert!(refresh(&src, &paths, Some(&d.join("missing.png"))).is_err());
        assert!(near(src.selection().dynamic.unwrap().seed, Rgb(20, 200, 60)));
    }
}
