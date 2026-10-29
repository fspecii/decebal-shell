//! "Match wallpaper": an optional layer that recolours any theme from the wallpaper.
//!
//! ```text
//!   wallpaper (rdwm state / Omarchy background / a file / a colour)
//!        | extract::load        (decode + 96 px thumbnail + k-means, cached by path + mtime + size)
//!        v
//!   Dynamic.colors  --stored in decebal.json under "theme"."dynamic"--  (a few words: seed, accents, tone)
//!        | apply_to             (pure maths, microseconds, in every program that draws)
//!        v
//!   the resolved Theme: accent always; surfaces tinted or replaced, by strength
//! ```
//!
//! Only the program that *notices a wallpaper change* decodes an image: `decthemectl watch` (one tiny daemon), the
//! Theme and Wallpaper panes of decsettings, dechaven, and `decthemectl dynamic refresh`. Everyone else reads the
//! stored colours and re-derives within the usual 500 ms theme poll.
//!
//! Inspired by Omarchy (every theme is a `colors.toml` of a handful of colours, turned into app configs by
//! templates; the theme picker shows a preview per theme; a theme carries a light/dark marker) and by matugen /
//! pywal / wallust: the schemes are Material You's (tonal spot, vibrant, expressive, monochrome, neutral, fidelity)
//! plus a pywal-like `palette` that uses the wallpaper's own distinct colours.

use super::extract::{self, Extracted};
use super::live::Source;
use super::material::{self, fit_oklch, oklch, tone_of, CHROMA_SCALE};
use super::model::{contrast, Dynamic, Origin, Rgb, Selection, Surfaces, Theme};
use super::rdwm;
use crate::settings::io::{self, Paths};
use crate::settings::tomlcfg::{Cfg, Val};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Pictures with a mean CIE L* at or above this count as "light" when the light/dark mode follows the wallpaper.
pub const LIGHT_TONE: f32 = 55.0;

fn expand(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) => PathBuf::from(h).join(rest),
        _ => PathBuf::from(p),
    }
}

// ---------------------------------------------------------------- finding the wallpaper

/// Omarchy's `current/background` link: `$XDG_STATE_HOME/omarchy/current/background`, else `~/.local/state/...`
/// (what `omarchy theme bg set` and `omarchy-theme-set` repoint).
pub fn omarchy_state_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|s| !s.is_empty()).map(|h| PathBuf::from(h).join(".local/state")))
        .map(|d| d.join("omarchy"))
}

/// The wallpaper on screen: rdwm's live state (`wallpaper.image`), else the `[wallpaper] image` of rdwm's config,
/// else Omarchy's `current/background` link.
pub fn find_wallpaper(paths: &Paths) -> Option<PathBuf> {
    find_wallpaper_in(paths, omarchy_state_dir().as_deref())
}

/// [`find_wallpaper`] with the Omarchy state folder spelled out (`state/current/background`).
pub fn find_wallpaper_in(paths: &Paths, omarchy_state: Option<&Path>) -> Option<PathBuf> {
    // rdwm answers one request at a time; a busy moment is retried a couple of times before falling back to files
    let ask = || io::ipc_call(&paths.socket, &serde_json::json!({"cmd": "get_state"})).ok().and_then(|v| v["state"]["wallpaper"]["image"].as_str().map(String::from));
    let live = if paths.socket.exists() {
        (0..3).find_map(|i| {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(40));
            }
            ask()
        })
    } else {
        None
    };
    let file = || {
        let text = std::fs::read_to_string(&paths.rdwm_config).ok()?;
        match Cfg::parse(&text).ok()?.get("wallpaper", "image")? {
            Val::Str(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    };
    live.filter(|s| !s.is_empty())
        .or_else(file)
        .map(|s| expand(&s))
        .filter(|p| p.is_file())
        .or_else(|| omarchy_state.map(|d| d.join("current/background")).filter(|p| p.is_file()))
        .or_else(|| Some(paths.shell_dir.join("current/background")).filter(|p| p.is_file()))
}

/// What a [`Dynamic`] looks at right now.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Image(PathBuf),
    Color(Rgb),
}

/// The input of `d`; `wall` stands in for "the wallpaper" when the caller already knows it.
pub fn input_of(d: &Dynamic, paths: &Paths, wall: Option<&Path>) -> Result<Input, String> {
    match &d.source {
        Origin::Color(c) => Ok(Input::Color(*c)),
        Origin::File(p) => {
            let p = expand(p);
            if p.is_file() {
                Ok(Input::Image(p))
            } else {
                Err(format!("{}: no such image", p.display()))
            }
        }
        Origin::Wallpaper => wall.map(Path::to_path_buf).or_else(|| find_wallpaper(paths)).map(Input::Image).ok_or_else(|| "no wallpaper found (set one, or choose an image file)".to_string()),
    }
}

/// The colours of `input` (images through the on-disk cache).
pub fn read(input: &Input) -> Result<(Extracted, Option<String>), String> {
    match input {
        Input::Color(c) => Ok((Extracted::from_color(*c), None)),
        Input::Image(p) => Ok((extract::load(p, extract::default_cache().as_deref())?, Some(p.to_string_lossy().into_owned()))),
    }
}

// ---------------------------------------------------------------- changing the selection

/// `d` with freshly read colours.
fn with_colors(d: &Dynamic, x: Extracted, image: Option<String>) -> Dynamic {
    Dynamic { colors: x, image, ..d.clone() }
}

/// `sel` with "match wallpaper" turned on (settings kept from before) and the colours read; `wall` stands in for
/// "the wallpaper" when the caller knows it.
pub fn enable(sel: &Selection, paths: &Paths, wall: Option<&Path>) -> Result<Selection, String> {
    update_with(sel, paths, wall, |d| d.enabled = true)
}

/// `sel` with "match wallpaper" off. Its settings and last colours stay for the next time.
pub fn disable(sel: &Selection) -> Selection {
    Selection { dynamic: sel.dynamic.clone().map(|d| Dynamic { enabled: false, ..d }), ..sel.clone() }
}

/// [`update_with`] without a known wallpaper.
pub fn update(sel: &Selection, paths: &Paths, f: impl FnOnce(&mut Dynamic)) -> Result<Selection, String> {
    update_with(sel, paths, None, f)
}

/// `sel` with the settings changed by `f`; the picture is read again when the source changed or the feature was just
/// switched on. The first time, Caelestia (whose own palette is Material You) starts with the full palette and every
/// other theme with a 50 % tint.
pub fn update_with(sel: &Selection, paths: &Paths, wall: Option<&Path>, f: impl FnOnce(&mut Dynamic)) -> Result<Selection, String> {
    let mut old = sel.dynamic.clone().unwrap_or_default();
    if sel.dynamic.is_none() && crate::theme::builtin::norm(&sel.name) == crate::theme::builtin::DYNAMIC_THEME {
        old.surfaces = Surfaces::Full;
        old.strength = 1.0;
    }
    let mut d = old.clone();
    f(&mut d);
    d.strength = d.strength.clamp(0.0, 1.0);
    d.contrast = d.contrast.clamp(-1.0, 1.0);
    if d.enabled && (d.source != old.source || !old.enabled) {
        let (x, image) = read(&input_of(&d, paths, wall)?)?;
        d = with_colors(&d, x, image);
    }
    let palette = if d.enabled { None } else { sel.palette.clone() };
    Ok(Selection { dynamic: Some(d), palette, ..sel.clone() })
}

/// `sel` with the picture read again (`wall` stands in for "the wallpaper"); `sel` itself when match wallpaper is off.
pub fn reread(sel: &Selection, paths: &Paths, wall: Option<&Path>) -> Result<Selection, String> {
    let Some(d) = sel.dynamic.as_ref().filter(|d| d.enabled) else { return Ok(sel.clone()) };
    let (x, image) = read(&input_of(d, paths, wall)?)?;
    Ok(Selection { dynamic: Some(with_colors(d, x, image)), ..sel.clone() })
}

/// What a refresh did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// Match wallpaper is off.
    Off,
    Unchanged,
    Updated,
}

/// Re-reads the picture when "match wallpaper" is on and applies the colours when they changed (`path` stands in for
/// "the wallpaper" when the caller knows it). Cheap no-op when the feature is off or the source is a fixed colour.
pub fn refresh(src: &Source, paths: &Paths, path: Option<&Path>) -> Result<Refresh, String> {
    let sel = src.selection();
    let Some(cur) = sel.dynamic.clone().filter(|d| d.enabled) else { return Ok(Refresh::Off) };
    if matches!(cur.source, Origin::Color(_)) {
        return Ok(Refresh::Unchanged);
    }
    let (x, image) = read(&input_of(&cur, paths, path)?)?;
    if x == cur.colors && image == cur.image {
        return Ok(Refresh::Unchanged);
    }
    rdwm::apply(src, paths, &Selection { dynamic: Some(with_colors(&cur, x, image)), ..sel })?;
    Ok(Refresh::Updated)
}

/// Hook for the programs that change the wallpaper (dechaven, the settings Wallpaper pane): follows it when
/// "match wallpaper" is on, ignores every failure.
pub fn on_wallpaper_set(path: Option<&Path>) {
    let (src, paths) = (Source::discover(), Paths::discover());
    // nothing to do (and no image to decode) unless the user turned the feature on
    if src.selection().dynamic.is_some_and(|d| d.enabled) {
        let _ = refresh(&src, &paths, path);
    }
}

// ---------------------------------------------------------------- the watcher (`decthemectl watch`)

/// What [`watch_tick`] remembers between two looks.
#[derive(Debug, Default)]
pub struct WatchState {
    sel_stamp: Option<crate::config::Stamp>,
    source: Option<Origin>,
    seen: Option<(PathBuf, Option<(u64, u64)>)>,
}

/// Modification time and size of a file: the identity [`extract::load`] caches by.
fn fingerprint(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos() as u64, m.len()))
}

/// One look: when "match wallpaper" follows a picture and that picture (its path, or its contents) changed since the
/// last look, re-reads and applies it. Costs one `stat`, plus one request to rdwm when following the wallpaper.
pub fn watch_tick(src: &Source, paths: &Paths, st: &mut WatchState) -> Option<Result<Refresh, String>> {
    let stamp = crate::config::Stamp::of(&src.shell_json);
    if st.sel_stamp != Some(stamp) {
        st.sel_stamp = Some(stamp);
        st.source = src.selection().dynamic.filter(|d| d.enabled).map(|d| d.source);
        if st.source.is_none() {
            st.seen = None;
        }
    }
    let probe = Dynamic { source: st.source.clone()?, ..Dynamic::default() };
    let Ok(Input::Image(img)) = input_of(&probe, paths, None) else { return None };
    let now = (img.clone(), fingerprint(&img));
    if st.seen.as_ref() == Some(&now) {
        return None;
    }
    st.seen = Some(now);
    Some(refresh(src, paths, Some(&img)))
}

/// Runs [`watch_tick`] every `interval` (three times slower while the feature is off). Never returns.
pub fn watch(src: &Source, paths: &Paths, interval: Duration, mut log: impl FnMut(&str)) -> ! {
    let mut st = WatchState::default();
    loop {
        match watch_tick(src, paths, &mut st) {
            Some(Ok(Refresh::Updated)) => log("wallpaper colours updated"),
            Some(Err(e)) => log(&format!("not updated: {e}")),
            _ => {}
        }
        std::thread::sleep(if st.source.is_some() { interval } else { interval * 3 });
    }
}

/// A single-instance lock for `decthemectl watch` (`$XDG_RUNTIME_DIR/decthemectl-watch.lock`). `None` when another
/// watcher holds it. Keep the returned file alive for as long as the watcher runs.
pub fn watch_lock() -> Option<std::fs::File> {
    lock_file(&crate::single::runtime_file("decthemectl-watch.lock"))
}

fn lock_file(path: &Path) -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).ok()?;
    // SAFETY: flock on a descriptor we own; it only takes or refuses an advisory lock.
    (unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(file)
}

// ---------------------------------------------------------------- recolouring a theme

/// OKLCH chroma that a surface of each kind gets at full tint strength (before the scheme's own character scales it).
fn tint_goal(name: &str) -> f64 {
    match name {
        "background" | "sidebar" | "titlebar_inactive" => 0.030,
        "surface" | "bar" | "dock" => 0.034,
        "popup" | "overlay" | "titlebar" => 0.036,
        "border" => 0.040,
        "text_faint" => 0.028,
        "text_secondary" => 0.018,
        _ => 0.010, // text, bar_text
    }
}

/// Every colour the layer may touch, by the token name `Roles::tokens` uses.
fn field<'a>(t: &'a mut Theme, name: &str) -> Option<&'a mut Rgb> {
    Some(match name {
        "background" => &mut t.background,
        "sidebar" => &mut t.sidebar,
        "surface" => &mut t.surface,
        "bar" => &mut t.bar,
        "bar_text" => &mut t.bar_text,
        "popup" => &mut t.popup,
        "dock" => &mut t.dock,
        "overlay" => &mut t.overlay,
        "titlebar" => &mut t.titlebar,
        "titlebar_inactive" => &mut t.titlebar_inactive,
        "border" => &mut t.border,
        "text" => &mut t.text,
        "text_secondary" => &mut t.text_secondary,
        "text_faint" => &mut t.text_faint,
        "accent" => &mut t.accent,
        "accent_text" => &mut t.accent_text,
        "danger" => &mut t.danger,
        "ok" => &mut t.ok,
        "warn" => &mut t.warn,
        _ => return None,
    })
}

fn get(t: &Theme, name: &str) -> Rgb {
    match name {
        "background" => t.background,
        "sidebar" => t.sidebar,
        "surface" => t.surface,
        "bar" => t.bar,
        "bar_text" => t.bar_text,
        "popup" => t.popup,
        "dock" => t.dock,
        "overlay" => t.overlay,
        "titlebar" => t.titlebar,
        "titlebar_inactive" => t.titlebar_inactive,
        "border" => t.border,
        "text_secondary" => t.text_secondary,
        "text_faint" => t.text_faint,
        "accent" => t.accent,
        "accent_text" => t.accent_text,
        "danger" => t.danger,
        "ok" => t.ok,
        "warn" => t.warn,
        _ => t.text,
    }
}

const TINTED: [&str; 14] = ["background", "sidebar", "surface", "bar", "bar_text", "popup", "dock", "overlay", "titlebar", "titlebar_inactive", "border", "text", "text_secondary", "text_faint"];
/// Text and the surface it sits on: readable before, readable after.
const TEXT_ON: [(&str, &str); 9] = [
    ("text", "background"),
    ("text", "surface"),
    ("text", "popup"),
    ("text", "sidebar"),
    ("text", "titlebar"),
    ("bar_text", "bar"),
    ("text_secondary", "background"),
    ("text_secondary", "popup"),
    ("accent_text", "accent"),
];

/// `fg` moved along the lightness scale, away from `bg`, until it reaches `floor`:1 (keeping hue and, as far as it fits, chroma).
fn push_contrast(fg: Rgb, bg: Rgb, floor: f32) -> Rgb {
    let (l, c, h) = oklch(fg);
    let dir = if tone_of(bg) < 50.0 { 1.0 } else { -1.0 };
    for i in 1..=100 {
        let cand = fit_oklch(l + dir * 0.01 * f64::from(i), c, h);
        if contrast(cand, bg) >= floor {
            return cand;
        }
    }
    bg.readable_on()
}

/// Applies "match wallpaper" to a resolved theme: the accent always, surfaces by [`Surfaces`] and strength.
/// `keep_accent` leaves the accent alone (the user picked one by hand). Returns `t` unchanged when the picture has no
/// colour to match. Text keeps at least the contrast it had (or 4.5:1, whichever is lower) on every surface it sits on.
pub fn apply_to(t: Theme, d: &Dynamic, keep_accent: bool) -> Theme {
    if !d.enabled || !d.has_colour() {
        return t;
    }
    let roles = d.roles(t.dark);
    let before = t.clone();
    let mut t = t;
    if !keep_accent {
        t.accent = roles.primary;
        t.accent_text = roles.on_primary;
    }
    let s = f64::from(d.strength.clamp(0.0, 1.0));
    match d.surfaces {
        Surfaces::Accent => {}
        Surfaces::Tint => {
            let neutral = material::palettes_with(d.colors.seed, &d.colors.others, d.scheme).neutral;
            let character = (neutral.chroma / 6.0).clamp(0.0, 2.0);
            for name in TINTED {
                let Some(f) = field(&mut t, name) else { continue };
                let (l, _, _) = oklch(*f);
                // chroma has little room near black and white
                let room = (4.0 * l * (1.0 - l)).sqrt().clamp(0.2, 1.0);
                *f = material::retint(*f, neutral.hue, tint_goal(name) * room * character, s);
            }
        }
        Surfaces::Full => {
            for (name, v) in roles.tokens() {
                if keep_accent && (name == "accent" || name == "accent_text") {
                    continue;
                }
                if let (Some(f), super::model::V::S(hex)) = (field(&mut t, &name), v) {
                    if let Some(role) = Rgb::parse(&hex) {
                        *f = role.over(*f, s as f32);
                    }
                }
            }
        }
    }
    // text keeps its contrast on whatever it sits on
    for (fg, bg) in TEXT_ON {
        if keep_accent && fg == "accent_text" {
            continue;
        }
        let (c0, c1) = (contrast(get(&before, fg), get(&before, bg)), contrast(get(&t, fg), get(&t, bg)));
        let floor = c0.min(4.5);
        if c1 < floor {
            let fixed = push_contrast(get(&t, fg), get(&t, bg), floor);
            if let Some(f) = field(&mut t, fg) {
                *f = fixed;
            }
        }
    }
    t
}

/// Labelled colours for the preview strip: what the picture gave, then what the theme looks like now.
pub fn preview(d: &Dynamic, now: &Theme) -> Vec<(&'static str, Rgb)> {
    let mut v = Vec::new();
    if d.has_colour() {
        let roles = d.roles(now.dark);
        v.push(("Seed", d.colors.seed));
        v.push(("Primary", roles.primary));
        v.push(("Secondary", roles.secondary));
        v.push(("Tertiary", roles.tertiary));
    }
    v.extend([("Accent", now.accent), ("Background", now.background), ("Surface", now.surface), ("Popup", now.popup), ("Border", now.border), ("Text", now.text)]);
    v
}

/// The OKLCH hue of a colour, for status output and tests.
pub fn hue_of(c: Rgb) -> f64 {
    oklch(c).2
}

/// OKLCH chroma on the HCT-ish scale the palette code uses (36 = tonal spot's primary).
pub fn chroma_of(c: Rgb) -> f64 {
    oklch(c).1 / CHROMA_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::material::Variant;
    use crate::theme::model::Mode;

    fn sandbox(name: &str) -> (PathBuf, Source, Paths) {
        let d = std::env::temp_dir().join(format!("decebal-dyn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: d.join("themes") };
        (d, src, paths)
    }

    fn wall(dir: &Path, name: &str, rgb: [u8; 3]) -> PathBuf {
        let p = dir.join(name);
        image::RgbImage::from_pixel(64, 40, image::Rgb(rgb)).save(&p).unwrap();
        p
    }

    fn on(sel: &Selection, paths: &Paths, w: &Path) -> Selection {
        enable(sel, paths, Some(w)).unwrap()
    }

    fn hue_gap(a: Rgb, b: Rgb) -> f64 {
        material::hue_turn(hue_of(a), hue_of(b)).abs()
    }

    // ------------------------------------------------ finding the wallpaper

    #[test]
    fn the_wallpaper_is_found_in_rdwms_config_then_omarchys_link() {
        let (d, _src, paths) = sandbox("find");
        let state = d.join("state/omarchy");
        assert_eq!(find_wallpaper_in(&paths, Some(&state)), None);
        let w = wall(&d, "a.png", [10, 20, 30]);
        // Omarchy's link lives in the XDG state folder
        std::fs::create_dir_all(state.join("current")).unwrap();
        std::os::unix::fs::symlink(&w, state.join("current/background")).unwrap();
        assert_eq!(find_wallpaper_in(&paths, Some(&state)), Some(state.join("current/background")));
        // the older location next to the shell config still works
        let old = wall(&d, "old.png", [1, 2, 3]);
        std::fs::create_dir_all(paths.shell_dir.join("current")).unwrap();
        std::fs::copy(&old, paths.shell_dir.join("current/background")).unwrap();
        assert_eq!(find_wallpaper_in(&paths, None), Some(paths.shell_dir.join("current/background")));
        // rdwm's own config beats both
        std::fs::write(&paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", w.display())).unwrap();
        assert_eq!(find_wallpaper_in(&paths, Some(&state)), Some(w));
    }

    // ------------------------------------------------ the layer is optional

    #[test]
    fn off_by_default_and_when_switched_off_the_theme_is_untouched() {
        let (d, src, paths) = sandbox("off");
        let w = wall(&d, "red.png", [200, 30, 40]);
        let plain = src.build(&Selection { name: "nord".into(), ..Selection::default() }, true).0;
        assert_eq!(plain.accent, Rgb(0x88, 0xc0, 0xd0));
        let sel = on(&Selection { name: "nord".into(), ..Selection::default() }, &paths, &w);
        assert_ne!(src.build(&sel, true).0.accent, plain.accent);
        // off keeps the settings and colours but changes nothing
        let off = disable(&sel);
        assert!(!off.dynamic.as_ref().unwrap().enabled);
        assert_eq!(off.dynamic.as_ref().unwrap().colors, sel.dynamic.as_ref().unwrap().colors);
        assert_eq!(src.build(&off, true).0, plain);
        // refresh does nothing while off
        src.write_selection(&off).unwrap();
        assert_eq!(refresh(&src, &paths, Some(&w)), Ok(Refresh::Off));
    }

    #[test]
    fn it_works_on_any_base_theme_and_changes_only_colours() {
        let (d, src, paths) = sandbox("any");
        let w = wall(&d, "teal.png", [30, 157, 143]);
        for id in ["macos", "windows11", "caelestia", "nord", "dracula", "solarized", "graphite", "high-contrast"] {
            for mode in [Mode::Dark, Mode::Light] {
                let base = Selection { name: id.into(), mode, ..Selection::default() };
                let sel = on(&base, &paths, &w);
                let (plain, t) = (src.build(&base, true).0, src.build(&sel, true).0);
                assert_eq!(t.dark, plain.dark, "{id}: the wallpaper does not flip the variant unless asked");
                assert_ne!(t.accent, plain.accent, "{id} {mode:?}: accent follows the wallpaper");
                let a = hue_of(t.accent);
                assert!((150.0..210.0).contains(&a) || chroma_of(t.accent) < 3.0, "{id}: teal wallpaper gave accent {} (hue {a})", t.accent.hex());
                // shape is untouched
                assert_eq!((t.radius, t.dock_roundness, t.bar_style.clone(), t.font_scale), (plain.radius, plain.dock_roundness, plain.bar_style.clone(), plain.font_scale));
                // and the text still reads
                for (fg, bg) in TEXT_ON {
                    let floor = contrast(get(&plain, fg), get(&plain, bg)).min(4.5);
                    assert!(
                        contrast(get(&t, fg), get(&t, bg)) >= floor - 0.01,
                        "{id} {mode:?}: {fg} on {bg} fell from {:.2} to {:.2}",
                        contrast(get(&plain, fg), get(&plain, bg)),
                        contrast(get(&t, fg), get(&t, bg))
                    );
                }
            }
        }
    }

    #[test]
    fn accent_only_leaves_every_surface_alone() {
        let (d, src, paths) = sandbox("accent");
        let w = wall(&d, "red.png", [200, 30, 40]);
        let base = Selection { name: "windows11".into(), ..Selection::default() };
        let sel = update(&on(&base, &paths, &w), &paths, |d| d.surfaces = Surfaces::Accent).unwrap();
        let (plain, t) = (src.build(&base, true).0, src.build(&sel, true).0);
        assert_ne!(t.accent, plain.accent);
        assert!(hue_gap(t.accent, Rgb(220, 60, 70)) < 25.0);
        for n in TINTED {
            assert_eq!(get(&t, n), get(&plain, n), "{n}");
        }
        assert_eq!(t.accent_text, material::generate(d_seed(&sel), Variant::TonalSpot, true).on_primary);
    }

    fn d_seed(sel: &Selection) -> Rgb {
        sel.dynamic.as_ref().unwrap().colors.seed
    }

    #[test]
    fn tint_keeps_the_themes_tones_and_pulls_the_hue() {
        let (d, src, paths) = sandbox("tint");
        let w = wall(&d, "blue.png", [30, 70, 220]);
        let base = Selection { name: "graphite".into(), ..Selection::default() };
        let weak = update(&on(&base, &paths, &w), &paths, |d| d.strength = 0.25).unwrap();
        let strong = update(&weak, &paths, |d| d.strength = 1.0).unwrap();
        let (plain, a, b) = (src.build(&base, true).0, src.build(&weak, true).0, src.build(&strong, true).0);
        // lightness stays where the theme put it ...
        for n in ["background", "surface", "popup"] {
            let (lp, lb) = (oklch(get(&plain, n)).0, oklch(get(&b, n)).0);
            assert!((lp - lb).abs() < 0.03, "{n}: L {lp:.3} -> {lb:.3}");
        }
        // ... the cast grows with the strength and points to the wallpaper's blue
        let (c0, c1, c2) = (chroma_of(plain.background), chroma_of(a.background), chroma_of(b.background));
        assert!(c0 < c1 && c1 < c2, "chroma {c0:.1} < {c1:.1} < {c2:.1}");
        assert!((230.0..290.0).contains(&hue_of(b.background)), "background hue {}", hue_of(b.background));
        // strength 0 is the theme with just the accent
        let zero = update(&weak, &paths, |d| d.strength = 0.0).unwrap();
        let z = src.build(&zero, true).0;
        for n in TINTED {
            assert_eq!(get(&z, n), get(&plain, n), "{n}");
        }
    }

    #[test]
    fn full_at_full_strength_is_the_material_palette_and_blends_below() {
        let (d, src, paths) = sandbox("full");
        let w = wall(&d, "green.png", [30, 160, 80]);
        let base = Selection { name: "caelestia".into(), ..Selection::default() };
        let sel = on(&base, &paths, &w); // Caelestia starts with the full palette
        let dy = sel.dynamic.clone().unwrap();
        assert_eq!((dy.surfaces, dy.strength), (Surfaces::Full, 1.0));
        let t = src.build(&sel, true).0;
        let roles = material::generate(dy.colors.seed, Variant::TonalSpot, true);
        assert_eq!((t.accent, t.background, t.surface, t.popup, t.text), (roles.primary, roles.surface, roles.surface_container, roles.surface_container_high, roles.on_surface));
        // a different theme at half strength sits between its own colour and the palette
        let half = update(&on(&Selection { name: "nord".into(), ..Selection::default() }, &paths, &w), &paths, |d| {
            d.surfaces = Surfaces::Full;
            d.strength = 0.5;
        })
        .unwrap();
        let (nord, h) = (src.build(&Selection { name: "nord".into(), ..Selection::default() }, true).0, src.build(&half, true).0);
        let (lo, hi) = (tone_of(nord.background).min(tone_of(roles.surface)), tone_of(nord.background).max(tone_of(roles.surface)));
        assert!((lo - 1.0..=hi + 1.0).contains(&tone_of(h.background)));
        assert_ne!(h.background, nord.background);
        assert_ne!(h.background, roles.surface);
    }

    #[test]
    fn a_hand_picked_accent_wins() {
        let (d, src, paths) = sandbox("keep");
        let w = wall(&d, "red.png", [200, 30, 40]);
        let sel = Selection { accent: Rgb::parse("#00ff00"), ..on(&Selection { name: "nord".into(), ..Selection::default() }, &paths, &w) };
        let t = src.build(&sel, true).0;
        assert_eq!(t.accent, Rgb(0, 255, 0));
        assert_eq!(t.accent_text, Rgb(0, 0, 0));
        assert_ne!(t.background, src.build(&Selection { name: "nord".into(), ..Selection::default() }, true).0.background, "surfaces still tint");
    }

    // ------------------------------------------------ schemes

    #[test]
    fn schemes_give_different_looks_for_one_wallpaper() {
        let (d, src, paths) = sandbox("schemes");
        let w = wall(&d, "orange.png", [230, 120, 20]);
        let base = on(&Selection { name: "macos".into(), ..Selection::default() }, &paths, &w);
        let accents: Vec<(Variant, Rgb)> = Variant::ALL
            .iter()
            .map(|v| {
                let s = update(&base, &paths, |d| {
                    d.scheme = *v;
                    d.surfaces = Surfaces::Accent;
                })
                .unwrap();
                (*v, src.build(&s, true).0.accent)
            })
            .collect();
        let get = |v: Variant| accents.iter().find(|a| a.0 == v).unwrap().1;
        assert!(chroma_of(get(Variant::Vibrant)) > chroma_of(get(Variant::TonalSpot)));
        assert!(chroma_of(get(Variant::Neutral)) < chroma_of(get(Variant::TonalSpot)));
        assert!(chroma_of(get(Variant::Monochrome)) < 1.5, "monochrome has no colour: {}", get(Variant::Monochrome).hex());
        assert!(hue_gap(get(Variant::Expressive), get(Variant::TonalSpot)) > 90.0, "expressive turns the hue");
        assert!(hue_gap(get(Variant::TonalSpot), Rgb(230, 120, 20)) < 20.0);
        let distinct: std::collections::HashSet<Rgb> = accents.iter().map(|a| a.1).collect();
        assert!(distinct.len() >= 4, "{accents:?}");
    }

    #[test]
    fn palette_scheme_uses_the_wallpapers_own_companion_colours() {
        let (d, _src, paths) = sandbox("palette");
        let p = d.join("tri.png");
        image::RgbImage::from_fn(90, 30, |x, _| {
            if x < 40 {
                image::Rgb([40, 90, 210])
            } else if x < 65 {
                image::Rgb([220, 120, 20])
            } else {
                image::Rgb([30, 170, 90])
            }
        })
        .save(&p)
        .unwrap();
        let sel = update(&on(&Selection::default(), &paths, &p), &paths, |d| d.scheme = Variant::Palette).unwrap();
        let dy = sel.dynamic.unwrap();
        assert_eq!(dy.colors.others.len(), 2);
        let r = dy.roles(true);
        for (role, want) in [(r.primary, dy.colors.seed), (r.secondary, dy.colors.others[0]), (r.tertiary, dy.colors.others[1])] {
            assert!(hue_gap(role, want) < 18.0, "{} vs {}", role.hex(), want.hex());
        }
        // the three roles are three different hues, unlike tonal spot's single-hue companions
        assert!(hue_gap(r.primary, r.secondary) > 30.0 && hue_gap(r.primary, r.tertiary) > 30.0);
    }

    // ------------------------------------------------ contrast

    #[test]
    fn contrast_control_widens_or_narrows_the_palette_and_text_stays_readable() {
        let seed = Rgb(30, 120, 200);
        for v in Variant::ALL {
            for dark in [true, false] {
                let mut last = 0.0f32;
                for c in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                    let r = material::generate_with(seed, &[Rgb(220, 120, 20), Rgb(30, 170, 90)], v, dark, c);
                    for (what, fg, bg) in r.text_pairs() {
                        assert!(contrast(fg, bg) >= 4.5, "{} {dark} c={c}: {what} {:.2}", v.as_str(), contrast(fg, bg));
                    }
                    let spread = contrast(r.on_surface, r.surface);
                    assert!(spread >= last - 0.2, "{} dark={dark}: text/surface contrast should rise with the level ({last:.2} -> {spread:.2} at {c})", v.as_str());
                    last = spread;
                }
            }
        }
        let low = material::generate_with(seed, &[], Variant::TonalSpot, true, -1.0);
        let high = material::generate_with(seed, &[], Variant::TonalSpot, true, 1.0);
        assert!(contrast(high.on_surface, high.surface) > contrast(low.on_surface, low.surface) + 2.0);
        assert_eq!(material::generate_with(seed, &[], Variant::Vibrant, false, 0.0), material::generate(seed, Variant::Vibrant, false), "level 0 is Material's standard");
    }

    // ------------------------------------------------ graceful fallbacks

    #[test]
    fn grey_dark_and_bright_wallpapers_leave_the_theme_alone() {
        let (d, src, paths) = sandbox("grey");
        let base = Selection { name: "nord".into(), ..Selection::default() };
        let plain = src.build(&base, true).0;
        for (name, rgb) in [("grey", [128u8, 128, 128]), ("black", [0, 0, 0]), ("white", [255, 255, 255]), ("paper", [250, 248, 246])] {
            let sel = on(&base, &paths, &wall(&d, &format!("{name}.png"), rgb));
            assert!(sel.dynamic.as_ref().unwrap().colors.grey, "{name}");
            assert_eq!(src.build(&sel, true).0, plain, "{name}: nothing to match, nothing changes");
        }
        // a very dark but blue picture still gives a (usable) accent
        let navy = on(&base, &paths, &wall(&d, "navy.png", [10, 14, 40]));
        let t = src.build(&navy, true).0;
        assert!(!navy.dynamic.as_ref().unwrap().colors.grey);
        assert!(contrast(t.accent, t.background) >= 4.5, "accent {} on {}", t.accent.hex(), t.background.hex());
        // monochrome does not need colour: it goes grey even for a grey wallpaper
        let mono = update(&on(&base, &paths, &wall(&d, "g2.png", [128, 128, 128])), &paths, |d| d.scheme = Variant::Monochrome).unwrap();
        assert!(chroma_of(src.build(&mono, true).0.accent) < 1.5);
    }

    #[test]
    fn every_wallpaper_hue_keeps_text_readable_in_every_mode() {
        let (_d, src, _paths) = sandbox("sweep");
        for hue in (0..360).step_by(30) {
            let seed = fit_oklch(0.6, 0.13, hue as f64);
            for id in ["macos", "nord", "windows11", "high-contrast"] {
                for mode in [Mode::Dark, Mode::Light] {
                    for surfaces in Surfaces::ALL {
                        for v in [Variant::TonalSpot, Variant::Vibrant, Variant::Palette] {
                            let dy = Dynamic { enabled: true, scheme: v, surfaces, strength: 1.0, colors: Extracted::from_color(seed), ..Dynamic::default() };
                            let sel = Selection { name: id.into(), mode, dynamic: Some(dy), ..Selection::default() };
                            let (t, plain) = (src.build(&sel, true).0, src.build(&Selection { dynamic: None, ..sel.clone() }, true).0);
                            for (fg, bg) in TEXT_ON {
                                let floor = contrast(get(&plain, fg), get(&plain, bg)).min(4.5);
                                assert!(contrast(get(&t, fg), get(&t, bg)) >= floor - 0.01, "hue {hue} {id} {mode:?} {surfaces:?} {}: {fg}/{bg}", v.as_str());
                            }
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------ light / dark follows the picture

    #[test]
    fn light_and_dark_can_follow_the_picture() {
        let (d, src, paths) = sandbox("follow");
        let base = Selection { name: "macos".into(), mode: Mode::Dark, ..Selection::default() };
        let pale = on(&base, &paths, &wall(&d, "pale.png", [200, 220, 245]));
        assert!(src.build(&pale, true).0.dark, "off by default: the mode setting rules");
        let follow = update(&pale, &paths, |d| d.follow_mode = true).unwrap();
        assert!(!src.build(&follow, true).0.dark, "a pale picture gives the light variant");
        let night = wall(&d, "night.png", [10, 20, 60]).to_string_lossy().into_owned();
        let night = update(&follow, &paths, |dy| dy.source = Origin::File(night)).unwrap();
        assert!(src.build(&night, false).0.dark, "a dark picture gives the dark variant, whatever the system says");
        // single-variant themes keep their look
        let dr = Selection { name: "dracula".into(), ..Selection::default() };
        let dr = update(&on(&dr, &paths, &wall(&d, "pale2.png", [220, 230, 245])), &paths, |dy| dy.follow_mode = true).unwrap();
        assert_eq!(src.build(&dr, true).0.dark, src.build(&Selection { dynamic: None, ..dr.clone() }, true).0.dark);
    }

    // ------------------------------------------------ following the wallpaper

    #[test]
    fn refresh_follows_a_changed_wallpaper_and_ignores_it_when_off() {
        let (d, src, paths) = sandbox("refresh");
        let blue = wall(&d, "blue.png", [20, 60, 220]);
        let green = wall(&d, "green.png", [20, 200, 60]);
        assert_eq!(refresh(&src, &paths, Some(&green)), Ok(Refresh::Off));
        assert!(!paths.shell_json().exists(), "nothing written while off");
        let sel = Selection { sync_rdwm: false, ..on(&Selection::default(), &paths, &blue) };
        src.write_selection(&sel).unwrap();
        assert_eq!(refresh(&src, &paths, Some(&blue)), Ok(Refresh::Unchanged), "same picture: no write");
        assert_eq!(refresh(&src, &paths, Some(&green)), Ok(Refresh::Updated));
        let after = src.selection().dynamic.unwrap();
        assert!(hue_gap(after.colors.seed, Rgb(20, 200, 60)) < 8.0);
        assert_eq!(after.image.as_deref(), Some(green.to_str().unwrap()));
        // settings survive the refresh
        assert_eq!((after.scheme, after.surfaces, after.enabled), (sel.dynamic.as_ref().unwrap().scheme, Surfaces::Tint, true));
        // a fixed image is not the wallpaper: the wallpaper changing does not matter
        let pinned = update(&src.selection(), &paths, |d| d.source = Origin::File(blue.to_string_lossy().into_owned())).unwrap();
        src.write_selection(&pinned).unwrap();
        assert!(hue_gap(src.selection().dynamic.unwrap().colors.seed, Rgb(20, 60, 220)) < 8.0);
        assert_eq!(refresh(&src, &paths, Some(&green)), Ok(Refresh::Unchanged));
        // a fixed colour never needs an image
        let colour = update(&pinned, &paths, |d| d.source = Origin::Color(Rgb(255, 0, 0))).unwrap();
        src.write_selection(&colour).unwrap();
        assert_eq!(refresh(&src, &paths, None), Ok(Refresh::Unchanged));
        assert_eq!(src.selection().dynamic.unwrap().colors.seed, Rgb(255, 0, 0));
        // an unreadable image is an error and changes nothing
        src.write_selection(&sel).unwrap();
        assert!(refresh(&src, &paths, Some(&d.join("missing.png"))).is_err());
        assert_eq!(src.selection().dynamic.unwrap().colors, sel.dynamic.unwrap().colors);
    }

    #[test]
    fn the_watcher_applies_a_new_wallpaper_once_and_idles_otherwise() {
        let (d, src, paths) = sandbox("watch");
        let blue = wall(&d, "blue.png", [20, 60, 220]);
        let red = wall(&d, "red.png", [220, 40, 40]);
        std::fs::write(&paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", blue.display())).unwrap();
        let mut st = WatchState::default();
        assert!(watch_tick(&src, &paths, &mut st).is_none(), "off: nothing to do");
        let sel = Selection { sync_rdwm: false, ..on(&Selection::default(), &paths, &blue) };
        src.write_selection(&sel).unwrap();
        // first look after (re)start: the colours are in sync, so nothing changes
        assert_eq!(watch_tick(&src, &paths, &mut st), Some(Ok(Refresh::Unchanged)));
        assert!(watch_tick(&src, &paths, &mut st).is_none(), "same path, same file: not even a decode");
        // rdwm now shows another picture
        std::fs::write(&paths.rdwm_config, format!("[wallpaper]\nimage = \"{}\"\n", red.display())).unwrap();
        assert_eq!(watch_tick(&src, &paths, &mut st), Some(Ok(Refresh::Updated)));
        assert!(hue_gap(src.selection().dynamic.unwrap().colors.seed, Rgb(220, 40, 40)) < 8.0);
        assert!(watch_tick(&src, &paths, &mut st).is_none());
        // the same file name with new contents
        std::thread::sleep(Duration::from_millis(20));
        image::RgbImage::from_pixel(65, 40, image::Rgb([30, 200, 60])).save(&red).unwrap();
        assert_eq!(watch_tick(&src, &paths, &mut st), Some(Ok(Refresh::Updated)));
        assert!(hue_gap(src.selection().dynamic.unwrap().colors.seed, Rgb(30, 200, 60)) < 8.0);
        // switched off again: idle
        src.write_selection(&disable(&src.selection())).unwrap();
        assert!(watch_tick(&src, &paths, &mut st).is_none());
        assert!(st.source.is_none());
    }

    #[test]
    fn the_preview_strip_shows_the_picture_and_the_result() {
        let (d, src, paths) = sandbox("preview");
        let sel = on(&Selection { name: "nord".into(), ..Selection::default() }, &paths, &wall(&d, "c.png", [200, 60, 90]));
        let t = src.build(&sel, true).0;
        let p = preview(sel.dynamic.as_ref().unwrap(), &t);
        assert!(p.len() >= 8 && p[0].0 == "Seed");
        assert_eq!(p.iter().find(|c| c.0 == "Accent").unwrap().1, t.accent);
        // a grey picture has no seed to show
        let grey = on(&Selection::default(), &paths, &wall(&d, "g.png", [128, 128, 128]));
        assert!(preview(grey.dynamic.as_ref().unwrap(), &t).iter().all(|c| c.0 != "Seed"));
    }

    #[test]
    fn two_watchers_cannot_run_at_once() {
        let (d, _, _) = sandbox("lock");
        let a = lock_file(&d.join("w.lock"));
        assert!(a.is_some());
        // flock locks belong to the open file description: a second open conflicts even in one process
        assert!(lock_file(&d.join("w.lock")).is_none());
        drop(a);
        assert!(lock_file(&d.join("w.lock")).is_some(), "released with the first watcher");
    }
}
