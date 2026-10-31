//! Offline contracts of the Lingmo desktop port (`themes/lingmo`, `docs/desktop-ports/lingmo`). No display, network, live
//! config or process. The upstream numbers below are copied from the pinned sources in `sources.json`; recomputing every
//! derived token from them keeps the manifest from drifting into "Lingmo-ish" approximations.
use decebal::settings::io::Paths;
use decebal::theme::{
    engine::{
        apply,
        archive::Limits,
        package,
        schema::{self},
    },
    live::Source,
    model::{contrast, Def, Rgb, Selection},
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

const ID: &str = "lingmo";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("themes").join(ID)
}

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/desktop-ports/lingmo")
}

/// True when this build's engine has the universal-panel manifest contract (`[bar] applets`, `[branding]`; shared commit
/// 71e3434 and later). The package requires it (`min_shell_version = "0.3.0"`); an older engine rejects exactly those keys.
fn contract() -> bool {
    schema::field("bar.applets").is_some() && schema::field("branding.logo").is_some()
}

/// The manifest as an engine without the contract can parse it: the keys that engine rejects are removed and the version floor
/// lowered. Only used when `contract()` is false.
fn compat(text: &str) -> String {
    let mut current = String::new();
    let mut out = vec![];
    for l in text.lines() {
        if l.starts_with('[') {
            current = l.to_string();
        }
        if current == "[branding]" || l.starts_with("applets = ") {
            continue;
        }
        out.push(if l.starts_with("min_shell_version") { "min_shell_version = \"0.2.0\"".to_string() } else { l.to_string() });
    }
    out.join("\n")
}

fn manifest_text() -> String {
    fs::read_to_string(root().join("theme.toml")).unwrap()
}

fn def() -> Def {
    let text = manifest_text();
    Def::parse(&if contract() { text } else { compat(&text) }).unwrap_or_else(|e| panic!("{e}"))
}

fn to_json(item: &toml_edit::Item) -> Value {
    fn val(v: &toml_edit::Value) -> Value {
        match v {
            toml_edit::Value::String(s) => Value::from(s.value().clone()),
            toml_edit::Value::Integer(i) => Value::from(*i.value()),
            toml_edit::Value::Float(f) => Value::from(*f.value()),
            toml_edit::Value::Boolean(b) => Value::from(*b.value()),
            toml_edit::Value::Array(a) => Value::Array(a.iter().map(val).collect()),
            toml_edit::Value::InlineTable(t) => Value::Object(t.iter().map(|(k, v)| (k.to_string(), val(v))).collect()),
            toml_edit::Value::Datetime(_) => Value::Null,
        }
    }
    match item {
        toml_edit::Item::Value(v) => val(v),
        toml_edit::Item::Table(t) => Value::Object(t.iter().map(|(k, v)| (k.to_string(), to_json(v))).collect()),
        toml_edit::Item::ArrayOfTables(a) => Value::Array(a.iter().map(|t| to_json(&toml_edit::Item::Table(t.clone()))).collect()),
        toml_edit::Item::None => Value::Null,
    }
}

/// A section of the manifest exactly as written, independent of what this engine's parser knows.
fn section(name: &str) -> Value {
    let doc = manifest_text().parse::<toml_edit::DocumentMut>().unwrap();
    doc.get(name).map(to_json).unwrap_or(Value::Null)
}

type C = (u8, u8, u8);

fn rgb(c: C) -> Rgb {
    Rgb(c.0, c.1, c.2)
}

/// `fg` at `alpha` over `bg`, rounded per channel: how Lingmo's `withAlpha` recipes land on an opaque backdrop.
fn over(fg: C, bg: C, alpha: f32) -> C {
    let f = |x: u8, y: u8| (x as f32 * alpha + y as f32 * (1.0 - alpha)).round() as u8;
    (f(fg.0, bg.0), f(fg.1, bg.1), f(fg.2, bg.2))
}

/// ui::Theme::light()/dark() (lingmo-sdk 752d2b2 src/ui/theme.cpp), statusSurfaceColor() and the decoration palettes
/// (lingmo-compositor 1cdaf0a), and the recipes in toolkit_colors.cpp / panel_style.cpp.
struct Lingmo {
    background: C,
    second_background: C,
    alternate_background: C,
    text: C,
    disabled_text: C,
    status_surface: C,
    titlebar: C,
    destructive: C,
    /// ColorId::HeaderSeparator = Text at this alpha.
    separator_alpha: f32,
}

const WHITE: C = (255, 255, 255);
const BLACK: C = (0, 0, 0);
const BLUE: C = (0x33, 0x85, 0xff);
const GREEN: C = (0x35, 0xbf, 0x56);
const ORANGE: C = (0xfe, 0xa0, 0x42);

const LIGHT: Lingmo = Lingmo {
    background: (0xf5, 0xf6, 0xfa),
    second_background: WHITE,
    alternate_background: (0xf2, 0xf4, 0xf5),
    text: (0x32, 0x32, 0x38),
    disabled_text: (0x64, 0x64, 0x6e),
    status_surface: (0xfa, 0xfa, 0xfc),
    titlebar: WHITE,
    destructive: (0xd9, 0x2d, 0x20),
    separator_alpha: 0.08,
};

const DARK: Lingmo = Lingmo {
    background: (0x1c, 0x1c, 0x1d),
    second_background: (0x2c, 0x2c, 0x2d),
    alternate_background: (0x3c, 0x3c, 0x3d),
    text: WHITE,
    disabled_text: (0x88, 0x88, 0x88),
    status_surface: (0x24, 0x25, 0x29),
    titlebar: (0x2c, 0x2c, 0x2d),
    destructive: (0xff, 0x45, 0x3a),
    separator_alpha: 0.12,
};

#[test]
fn colour_tokens_are_recomputed_from_the_pinned_upstream_values() {
    for (dark, s) in [(true, &DARK), (false, &LIGHT)] {
        let t = def().resolve(ID, dark, &Selection::default());
        let what = format!("dark={dark}");
        assert_eq!(t.background, rgb(s.background), "{what} background");
        assert_eq!(t.surface, rgb(s.second_background), "{what} surface = secondBackground (cards)");
        assert_eq!(t.overlay, rgb(s.second_background), "{what} overlay");
        assert_eq!(t.popup, rgb(s.status_surface), "{what} popup = statusSurfaceColor");
        assert_eq!(t.text, rgb(s.text), "{what} text");
        assert_eq!(t.accent, rgb(BLUE), "{what} accent = default accent blue");
        assert_eq!(t.accent_text, rgb(WHITE), "{what} accent_text = highlightedText");
        assert_eq!(t.titlebar, rgb(s.titlebar), "{what} titlebar");
        assert_eq!(t.titlebar_inactive, rgb(s.titlebar), "{what}: Lingmo dims the caption, not the bar");
        assert_eq!((t.danger, t.ok, t.warn), (rgb(s.destructive), rgb(GREEN), rgb(ORANGE)), "{what} status colours");
        // derived exactly as documented in the manifest comments
        assert_eq!(t.border, rgb(over(s.text, s.background, s.separator_alpha)), "{what} border = HeaderSeparator");
        assert_eq!(t.text_faint, rgb(over(s.text, s.background, 0.4)), "{what} text_faint = TextFieldPlaceholder");
        let secondary = if dark { over(WHITE, s.background, 0.6) } else { s.disabled_text };
        assert_eq!(t.text_secondary, rgb(secondary), "{what} text_secondary = PanelStyle::secondaryTextColor");
        // BarContext::textColor: black at 8-bit alpha 217 on a light backdrop (so 217/255, not exactly 0.85), white on a dark one
        let bar_text = if dark { WHITE } else { over(BLACK, s.background, 217.0 / 255.0) };
        assert_eq!(t.bar_text, rgb(bar_text), "{what} bar_text = BarContext::textColor over the bar");
    }
    // the sidebar is the toolkit's button/alternate surface in light mode and the card surface in dark
    assert_eq!(def().resolve(ID, false, &Selection::default()).sidebar, rgb(LIGHT.alternate_background));
    assert_eq!(def().resolve(ID, true, &Selection::default()).sidebar, rgb(DARK.second_background));
}

/// The dock tint is upstream's own formula at the default glassTint 0.5 (dock_window.cpp glass()). kPace has 21 entries and
/// kPace[10] is exactly 0.5, so glassAmount(0.5) = 0.5 and glassBlend returns the "default" value of each pair: premultiplied grey
/// value / alpha = 0.110/0.29 (dark, #616161) and 0.150/0.25 (light, #999999). Recomputed here independently of the manifest.
#[test]
fn dock_tint_matches_the_glass_formula_at_the_default_tint() {
    // dock_window.cpp:58-60, verbatim
    const K_PACE: [f64; 21] = [0.000, 0.048, 0.116, 0.174, 0.249, 0.314, 0.361, 0.398, 0.435, 0.469, 0.500, 0.587, 0.630, 0.666, 0.701, 0.739, 0.781, 0.827, 0.879, 0.931, 1.000];
    let last = K_PACE.len() - 1;
    let x = 0.5 * last as f64;
    let i = (x.floor() as usize).min(last - 1);
    let amount = K_PACE[i] + (x - i as f64) * (K_PACE[i + 1] - K_PACE[i]);
    assert_eq!(amount, 0.5, "the default slider position is the 'default' glass");
    let blend = |c: f64, d: f64, t: f64| {
        let (d0, d1) = (2.0 * (d - c), 2.0 * (t - d));
        let middle = if d0 * d1 > 0.0 { 2.0 * d0 * d1 / (d0 + d1) } else { 0.0 };
        let upper = amount > 0.5;
        let x = if upper { 2.0 * amount - 1.0 } else { 2.0 * amount };
        let (y0, y1) = if upper { (d, t) } else { (c, d) };
        let (m0, m1) = (0.5 * if upper { middle } else { d0 }, 0.5 * if upper { d1 } else { middle });
        let (x2, x3) = (x * x, x * x * x);
        (2.0 * x3 - 3.0 * x2 + 1.0) * y0 + (x3 - 2.0 * x2 + x) * m0 + (3.0 * x2 - 2.0 * x3) * y1 + (x3 - x2) * m1
    };
    let mut alphas = vec![];
    for (dark, alpha_range, value_range) in [(true, (0.12, 0.29, 0.50), (0.046, 0.110, 0.190)), (false, (0.16, 0.25, 0.53), (0.060, 0.150, 0.530))] {
        let alpha = blend(alpha_range.0, alpha_range.1, alpha_range.2);
        assert!((alpha - alpha_range.1).abs() < 1e-9, "default blend returns the middle value");
        alphas.push(alpha);
        let grey = (blend(value_range.0, value_range.1, value_range.2) / alpha * 255.0).round() as u8;
        let t = def().resolve(ID, dark, &Selection::default());
        assert_eq!(t.dock, rgb((grey, grey, grey)), "dock tint dark={dark}");
    }
    // one alpha for both modes: between the light (.25) and dark (.29) values
    let t = def().resolve(ID, false, &Selection::default());
    assert!((alphas[1]..=alphas[0]).contains(&(t.dock_alpha as f64)), "dock_alpha {}", t.dock_alpha);
}

#[test]
fn readable_and_each_variant_has_the_right_brightness() {
    for dark in [true, false] {
        let t = def().resolve(ID, dark, &Selection::default());
        let what = format!("dark={dark}");
        for (name, bg) in [("background", t.background), ("surface", t.surface), ("popup", t.popup), ("sidebar", t.sidebar)] {
            assert!(contrast(t.text, bg) >= 4.5, "{what}: text on {name}");
        }
        assert!(contrast(t.bar_text, t.bar) >= 4.5, "{what}: bar text");
        // Lingmo's own white-on-#3385FF pair is the toolkit's selection pair; it is kept faithfully and is not "fixed" here
        assert!(contrast(t.accent_text, t.accent) >= 3.0, "{what}: accent text {:.2}", contrast(t.accent_text, t.accent));
        assert_eq!(t.background.luminance() > 0.3, !dark, "{what}: variant brightness");
    }
}

#[test]
fn metrics_are_the_measured_ones() {
    let (deco, dock, bar, rdwm) = (section("decoration"), section("dock"), section("bar"), section("rdwm"));
    // no height or radius: a theme value would override the native geometry of whichever style is in use (upstream measures 36 / 14, the native
    // `lingmo` style reports 30 / 11: recorded in the manifest, not resolved by a number here)
    assert!(deco.get("height").is_none() && deco.get("radius").is_none());
    assert_eq!(deco["title_align"], "center");
    // DockSettings defaults: icon 54, magnification factor 1.5; DockMetrics: thickness 86 at 54, radius = thickness * 28/86
    assert_eq!((&dock["style"], &dock["position"], &dock["size"], &dock["magnification"]), (&Value::from("mac"), &Value::from("bottom"), &Value::from(54), &Value::from(1.5)));
    // StatusBarWindow::kHeight 32
    assert_eq!((&bar["style"], &bar["position"], &bar["height"]), (&Value::from("flat"), &Value::from("top"), &Value::from(32)));
    let t = def().resolve(ID, true, &Selection::default());
    assert_eq!(t.dock_padding, 16.0, "DockMetrics lengthPadding/outerPadding 16");
    // upstream's corner radius is thickness * 28/86; this engine uses min(w,h)/2 * roundness, so the ratio is what can be kept
    assert!((t.dock_roundness / 2.0 - 28.0 / 86.0).abs() < 0.005, "dock radius ratio {}", t.dock_roundness / 2.0);
    assert_eq!(t.radius, 14.0);
    assert!((t.bar_alpha - 0.55).abs() < 1e-6, "StatusBarSettings::kDefaultOpacity");
    assert!((t.panel_alpha - 0.86).abs() < 1e-6 && (t.popup_alpha - 0.86).abs() < 1e-6, "panelGlass tint alpha");
    assert!((t.blur - 20.0 / 64.0).abs() < 0.01, "bar blur sigma 20 of 64: {}", t.blur);
    // only visual gaps and the border belong to the theme
    assert_eq!((&rdwm["border_px"], &rdwm["gap_inner"], &rdwm["gap_outer"]), (&Value::from(1), &Value::from(8), &Value::from(8)));
}

/// The applets of the real status bar (status_bar_window.cpp): menu bar, tray, volume, network, battery, control centre, clock.
const BAR_APPLETS: &[&str] = &["app_menu", "tray", "volume", "network", "battery", "control_center", "clock"];

#[test]
fn panel_declaration_uses_the_shared_contract_and_the_real_bar_set() {
    let (bar, dock, brand) = (section("bar"), section("dock"), section("branding"));
    assert_eq!(bar["applets"], Value::from(BAR_APPLETS.to_vec()));
    assert_eq!(bar["enabled"], Value::from(true));
    assert_eq!(dock["enabled"], Value::from(true));
    // the real bar has no workspace switcher, weather, search, cpu or media item: none may be declared on by default
    for foreign in ["workspaces", "weather", "search", "cpu", "memory", "media", "layout", "tasks", "launcher", "notifications"] {
        assert!(!BAR_APPLETS.contains(&foreign));
    }
    // not a Dux or pill-rail bar: those layout keys must be absent
    for dux in ["left", "center", "right", "background"] {
        assert!(bar.get(dux).is_none(), "[bar] {dux}");
    }
    // the multicolour mark comes from the icon pack by name, is not tinted, and never falls back to Apple or "Finder"
    assert_eq!((&brand["logo"], &brand["logo_tint"], &brand["desktop_label"]), (&Value::from("icon:systemlogo"), &Value::from(false), &Value::from("Desktop")));
    assert!(brand.get("launcher_label").is_none());
    assert_eq!(section("control_center")["style"], "card");
    // the window style pair and its same-mode fallbacks; `style` is the older key for engines without the mode keys
    let d = section("decoration");
    assert_eq!((&d["style"], &d["style_light"], &d["style_dark"]), (&Value::from("adwaita"), &Value::from("lingmo"), &Value::from("lingmo-dark")));
    assert_eq!((&d["fallback_style_light"], &d["fallback_style_dark"]), (&Value::from("adwaita-light"), &Value::from("adwaita")));
    assert!(d.get("fallback_style").is_none(), "one fallback per mode: a single one cannot serve both light and dark rows");
}

/// The user's tiling policy is global: a theme must not carry layouts, master fraction, master count or smart gaps.
#[test]
fn tiling_policy_is_global_and_never_set_by_the_package() {
    let rdwm = section("rdwm");
    for key in ["layouts", "mfact", "nmaster", "smart_gaps"] {
        assert!(rdwm.get(key).is_none(), "[rdwm] {key} would overwrite the user's tiling policy");
    }
    assert!(manifest_text().contains("global tiling behaviour"));
}

#[test]
fn package_is_valid_has_no_bundled_art_and_declares_the_framework_floor() {
    let dir = root();
    let entries = package::read_dir(&dir, &Limits::default()).unwrap();
    let report = package::check(&entries, &Limits::default());
    if contract() {
        assert!(report.ok(), "{:?}", report.errors());
        assert_eq!(report.id.as_deref(), Some(ID));
        let (a, _) = package::pack_dir(&dir, &Limits::default()).unwrap();
        assert_eq!(a, package::pack_dir(&dir, &Limits::default()).unwrap().0, "reproducible pack");
    } else {
        let errors = report.errors().iter().map(|e| format!("{e:?}")).collect::<Vec<_>>();
        assert!(errors.iter().all(|e| e.contains("applets") || e.contains("branding") || e.contains("min_shell_version")), "{errors:?}");
    }
    // no wallpaper, screenshot, widget, font, icon or cursor ships in the package: the packs are fetch recipes and wallpapers
    // come from the curated catalogue
    let mut names: Vec<_> = entries.iter().map(|e| e.path.clone()).collect();
    names.sort();
    assert_eq!(names, ["NOTICE.md", "README.md", "theme.toml"], "unexpected package contents");
    let text = manifest_text();
    for forbidden in ["\nscreenshots", "[wallpaper", "[[widgets]]", "extends"] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
    assert!(text.contains("min_shell_version = \"0.3.0\""));
    let readme = fs::read_to_string(dir.join("README.md")).unwrap();
    assert!(readme.contains("NOT READY"), "must not claim a finished replica");
    let notice = fs::read_to_string(dir.join("NOTICE.md")).unwrap();
    for sha in ["1cdaf0a86596ef3732e23d2450318728a180877b", "752d2b24b398b11b995cbb82bfba0b8c161df7bd"] {
        assert!(notice.contains(sha), "notice pins {sha}");
        assert!(text.contains(&sha[..7]), "manifest comments pin {}", &sha[..7]);
    }
    assert!(notice.contains("BSD 3-Clause") && notice.contains("GPL-3.0-or-later"), "the upstream licence conflict is stated");
}

#[test]
fn source_pins_and_asset_recipes_are_well_formed_and_honest_about_licences() {
    let sources: Value = serde_json::from_str(&fs::read_to_string(docs().join("sources.json")).unwrap()).unwrap();
    let is_sha = |s: &str| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit());
    for p in sources["primary"].as_array().unwrap().iter().chain(sources["reference_only_previous_generation"].as_array().unwrap()) {
        assert!(is_sha(p["commit"].as_str().unwrap()), "{p}");
    }
    let assets: toml_edit::DocumentMut = fs::read_to_string(docs().join("reference-recipes.toml")).unwrap().parse().unwrap();
    let packs = to_json(assets.get("pack").unwrap());
    let packs = packs.as_array().unwrap();
    assert_eq!(packs.len(), 2);
    for p in packs {
        assert!(is_sha(p["commit"].as_str().unwrap()), "{p}");
        for tree in p["source_trees"].as_object().unwrap().values() {
            assert!(is_sha(tree.as_str().unwrap()), "{p}");
        }
    }
    let by = |id: &str| packs.iter().find(|p| p["id"] == id).unwrap();
    // icons: GPL-3.0-or-later with evidence; a single installed theme, not an invented dark variant
    assert_eq!(by("lingmo-icons")["license"], "GPL-3.0-or-later");
    assert_eq!(by("lingmo-icons")["variant"].as_array().unwrap().len(), 1);
    // cursors: the repository grants no licence and the recipe must not pretend it does
    assert_eq!(by("lingmo-cursors")["license"], "UNDETERMINED");
    assert!(by("lingmo-cursors")["redistribution"].as_str().unwrap().contains("never commit or bundle"));
    // the same pins appear in both files
    let primary = |repo: &str| sources["primary"].as_array().unwrap().iter().find(|p| p["repo"] == repo).unwrap()["commit"].clone();
    assert_eq!(by("lingmo-icons")["commit"], primary("LingmoOS/lingmo_icon_theme"));
    assert_eq!(by("lingmo-cursors")["commit"], primary("LingmoOS/lingmo-cursor-themes"));
}

const USER_RDWM: &str = "[general]\nmfact = 0.73\nnmaster = 2\nsmart_gaps = true\nlayouts = [\"monocle\", \"tile\"]\ngap_inner = 1\n";

/// Applies a private copy of the package over a user's configuration (custom tiling, wallpaper, icon theme).
fn plan(tag: &str, user_rdwm: &str, user_shell: Option<&str>) -> (apply::Plan, Paths, PathBuf) {
    let dir = std::env::temp_dir().join(format!("lingmo-port-{}-{tag}", std::process::id()));
    let pkg = dir.join("themes").join(ID);
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("theme.toml"), if contract() { manifest_text() } else { compat(&manifest_text()) }).unwrap();
    fs::create_dir_all(dir.join("shell")).unwrap();
    // no rdwm runs here: the explicit target CLI is the (labelled) proof that the package's window styles exist
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: fake_ctl(&dir, Some(NEW_STYLES)), socket: dir.join("no.sock") };
    fs::write(&paths.rdwm_config, user_rdwm).unwrap();
    if let Some(shell) = user_shell {
        fs::write(paths.shell_json(), shell).unwrap();
    }
    let src = Source { shell_json: paths.shell_json(), themes_dir: dir.join("themes") };
    let plan = apply::plan(&src, &paths, ID, &Selection::default()).unwrap_or_else(|e| panic!("{e}"));
    (plan, paths, dir)
}

#[test]
fn applying_keeps_tiling_proportions_wallpaper_and_the_users_panel_choices() {
    let (plan, paths, dir) = plan("apply", &format!("{USER_RDWM}[wallpaper]\nimage = \"/private/photo.jpg\"\nmode = \"center\"\n"), Some("{\"bar\":{\"edge\":\"left\",\"applets\":{\"weather\":true}}}"));
    let config = &plan.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new;
    assert!(config.contains("/private/photo.jpg"), "wallpaper preserved");
    for kept in ["mfact = 0.73", "nmaster = 2", "smart_gaps = true", "layouts = [\"monocle\", \"tile\"]"] {
        assert!(config.contains(kept), "lost the user's `{kept}`:\n{config}");
    }
    assert!(config.contains("gap_inner = 8"), "visual gaps applied");
    let shell: Value = serde_json::from_str(&plan.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
    assert_eq!(shell["theme"]["name"], ID);
    assert!(shell.get("theme_wallpaper").is_none_or(|w| w.is_null()), "the package owns no wallpaper");
    // panel defaults come from the package at read time; the user's own edge and applet choices survive the apply
    assert_eq!(shell["bar"]["edge"], "left");
    assert_eq!(shell["bar"]["applets"]["weather"], true);
    let _ = fs::remove_dir_all(dir);
}

/// Icons stay independently selectable: the engine merges a theme's [icons] over the user's choice, so the package declares none
/// and a user's explicit icon theme (and scale) survives applying the theme.
#[test]
fn applying_keeps_a_users_explicit_icon_theme_because_the_package_declares_none() {
    assert!(section("icons").is_null(), "an [icons] section would replace the user's icon pack on apply");
    let (plan, paths, dir) = plan("icons", USER_RDWM, Some("{\"icons\":{\"theme\":\"Papirus\",\"size_scale\":1.1}}"));
    let shell: Value = serde_json::from_str(&plan.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
    assert_eq!(shell["icons"]["theme"], "Papirus", "applying a theme must not replace the user's icon pack: {}", shell["icons"]);
    assert_eq!(shell["icons"]["size_scale"], 1.1);
    let _ = fs::remove_dir_all(dir);
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn wallhaven_records_are_honest_distinct_and_legible_and_no_image_ships() {
    let rec: Value = serde_json::from_str(&fs::read_to_string(docs().join("wallhaven-lingmo-records.json")).unwrap()).unwrap();
    let images = rec["images"].as_object().unwrap();
    assert_eq!(images.len(), 2);
    let theme = &rec["themes"]["lingmo"];
    let (light, dark) = (theme["light"]["image_id"].as_str().unwrap(), theme["dark"]["image_id"].as_str().unwrap());
    assert_eq!((light, dark), ("mldor9", "l8pe5r"));
    assert_ne!(light, dark);
    assert_eq!(theme["same_image_both_modes"], false);
    assert!(images.contains_key(light) && images.contains_key(dark));
    // the policy follows the integration catalogue's: matching photographs are prepared only when the user turned Follow theme on
    let policy = rec["policy"].as_str().unwrap();
    assert!(policy.contains("Follow theme") && policy.contains("otherwise the wallpaper stays independent") && policy.contains("nothing downloads by itself"), "{policy}");
    assert!(!policy.contains("never replaces"), "the stale blanket claim is gone");
    // ids disjoint from every curated image in this checkout (the integration owner checks the rest of the catalogue)
    let shared: Value = serde_json::from_str(&fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/wallhaven-curated.json")).unwrap()).unwrap();
    let hex64 = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    for (id, i) in images {
        assert!(shared["images"].get(id).is_none() || shared["images"][id] == *i, "{id} duplicates a shared catalogue image with different content");
        assert_eq!(i["purity"], "sfw");
        assert_eq!(i["ai_review"]["flagged_ai"], false);
        assert!(i["license"]["status"].as_str().unwrap().starts_with("unknown"), "{id}: licence must stay unknown");
        assert!(i["author"]["name"].is_null() && i["uploader"]["is_author"].is_null(), "{id}: authorship is not asserted");
        assert!(i["full_resolution_review"]["provenance_limit"].as_str().unwrap().contains("unverified"), "{id}");
        assert!(hex64(i["full_resolution_review"]["sha256"].as_str().unwrap()) && i["full_resolution_review"]["dimensions_verified"] == true, "{id}");
        assert!(hex64(i["evidence"]["detail_sha256"].as_str().unwrap()) && hex64(i["evidence"]["thumbnail_sha256"].as_str().unwrap()), "{id}");
        let dims = i["resolution"].as_array().unwrap();
        assert!(dims[0].as_u64().unwrap() >= 3840 && dims[1].as_u64().unwrap() >= 2160 || dims[0].as_u64().unwrap() >= 5120, "{id}: full resolution");
        assert!(!i["tags"].as_array().unwrap().iter().any(|t| matches!(t["name"].as_str(), Some("digital art" | "illustration" | "artwork"))), "{id}: photographs only");
        // all three crops reviewed and hashed, and the parent verified against a fresh API response and a fresh download
        for crop in ["16x9", "ultrawide", "portrait"] {
            assert!(hex64(i["crop_review"][crop]["sha256"].as_str().unwrap()) && i["crop_review"][crop]["result"].as_str().unwrap().starts_with("accepted"), "{id} {crop}");
            assert!(i["readability_metrics"][crop]["panel_p95"].is_number() && i["readability_metrics"][crop]["dock_zone_p95"].is_number(), "{id} {crop}");
        }
        let pv = &i["parent_verification"];
        assert_eq!((&pv["status"], &pv["fresh_api_and_original_sha_match"], &pv["dimensions_match"]), (&Value::from("independently_accepted"), &Value::from(true), &Value::from(true)), "{id}");
        assert_eq!(pv["crops_reviewed"], serde_json::json!(["16x9", "3440x1440", "1080x1920"]), "{id}");
        assert!(pv["remaining"].as_array().unwrap().iter().any(|r| r.as_str().is_some_and(|r| r.contains("licence"))), "{id}: the open licence question stays listed");
    }
    // the package's own bar ink stays legible on every edge and viewport of both images
    for (key, r) in rec["bar_legibility"].as_object().unwrap() {
        let min = r["viewports"].as_object().unwrap().values().flat_map(|e| e.as_object().unwrap().values()).map(|x| x["min_contrast"].as_f64().unwrap()).fold(f64::MAX, f64::min);
        assert!(min >= 4.5, "{key}: bar text contrast {min}");
        assert_eq!(r["bar_alpha"], section("metrics")["bar_alpha"], "{key}: measured with the package's own bar alpha");
    }
    // no raster or vector image, font or archive ships in the package or the asset tools directory
    let mut files = vec![];
    files_under(&root(), &mut files);
    files_under(&Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/desktop-ports/lingmo"), &mut files);
    for f in files {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        assert!(!matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "svg" | "ttf" | "otf" | "xcursor" | "zip" | "gz" | "tar"), "{} must not ship", f.display());
    }
}

/// The catalogue follows the documented shared contract (docs/desktop-asset-packs.md at commit 76b9285 of the asset-pack branch): checked here
/// against the rules the document lists, without the shared validator (its implementation was not committed when this was written).
#[test]
fn asset_pack_catalogue_follows_the_documented_contract_and_matches_the_pins() {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/desktop-ports/lingmo/asset-packs.toml")).unwrap();
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    let cat = to_json(doc.as_item());
    let known_top = ["format", "family", "source", "pack", "recommend"];
    assert!(cat.as_object().unwrap().keys().all(|k| known_top.contains(&k.as_str())), "unknown top-level key");
    assert_eq!((&cat["format"], &cat["family"]), (&Value::from(1), &Value::from("lingmo")));
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let id_ok = |s: &str| !s.is_empty() && s.len() <= 64 && s.as_bytes()[0].is_ascii_alphanumeric() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let rel_ok = |s: &str| !s.is_empty() && !s.starts_with('/') && !s.contains('\\') && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..") && !s.chars().any(|c| c.is_control());
    let sources = cat["source"].as_array().unwrap();
    let packs = cat["pack"].as_array().unwrap();
    for s in sources {
        assert!(id_ok(s["id"].as_str().unwrap()));
        assert!(s["repo"].as_str().unwrap().starts_with("https://") && s["archive_url"].as_str().unwrap().starts_with("https://"));
        assert!(hex(s["commit"].as_str().unwrap(), 40) && hex(s["archive_sha256"].as_str().unwrap(), 64));
        assert!(!s["license"].as_str().unwrap().is_empty() && !s["license_files"].as_array().unwrap().is_empty());
        for f in s["license_files"].as_array().unwrap() {
            assert!(rel_ok(f.as_str().unwrap()));
        }
        assert!(s["archive_url"].as_str().unwrap().contains(s["commit"].as_str().unwrap()), "the archive is the pinned commit's");
    }
    let mut ids = vec![];
    for p in packs {
        let id = p["id"].as_str().unwrap();
        assert!(id_ok(id) && id.starts_with("lingmo-"), "pack ids carry the family prefix");
        assert!(!ids.contains(&id), "unique ids");
        ids.push(id);
        assert!(sources.iter().any(|s| s["id"] == p["source"]), "source exists");
        assert!(rel_ok(p["path"].as_str().unwrap()) && hex(p["tree"].as_str().unwrap(), 40));
        assert!(p["provides"].as_array().unwrap().iter().all(|k| matches!(k.as_str(), Some("icons" | "cursors"))));
        let name = p["install_name"].as_str().unwrap();
        assert!(name.len() <= 64 && name.as_bytes()[0].is_ascii_alphanumeric() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b)));
    }
    // the remaining fallback is disclosed in the catalogue itself, and no link source is claimed for text aliases the installer cannot read
    let description = packs.iter().find(|p| p["id"] == "lingmo-icons").unwrap()["description"].as_str().unwrap();
    let raw_catalogue = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/desktop-ports/lingmo/asset-packs.toml")).unwrap();
    assert!(raw_catalogue.contains("14137 REGULAR git blobs") && raw_catalogue.contains("0 symlinks") && raw_catalogue.contains("only 32 exist"), "the full disclosure sits in the catalogue's comments");
    assert!(description.len() <= 400, "the shared validator limits descriptions to 400 characters");
    for must in ["DISCLOSED", "links/", "not git symlinks", "@2x/@3x", "DEGRADED", "fall back to breeze or hicolor"] {
        assert!(description.contains(must), "the description must say: {must}");
    }
    assert!(packs.iter().all(|p| p.get("link_sources").is_none() && p.get("link_source_trees").is_none()), "text aliases are not git symlinks");
    // the cursor pack is deliberately absent: its repository names no licence file, which the contract requires
    assert!(!packs.iter().any(|p| p["provides"].as_array().unwrap().iter().any(|k| k == "cursors")));
    // every recommendation names an existing theme package and a pack that provides what it is recommended for
    for r in cat["recommend"].as_array().unwrap() {
        assert!(root().join("theme.toml").is_file() && r["theme"] == ID);
        let icons = r["icons"].as_str().unwrap();
        assert!(packs.iter().any(|p| p["id"] == icons && p["provides"].as_array().unwrap().iter().any(|k| k == "icons")));
        assert!(r.get("cursors").is_none());
    }
    // the same pins as the reference recipes and sources.json
    let recipes: toml_edit::DocumentMut = fs::read_to_string(docs().join("reference-recipes.toml")).unwrap().parse().unwrap();
    let recipe = to_json(recipes.get("pack").unwrap());
    let icons = recipe.as_array().unwrap().iter().find(|p| p["id"] == "lingmo-icons").unwrap();
    assert_eq!(icons["commit"], sources[0]["commit"]);
    assert_eq!(icons["source_trees"]["src"], packs[0]["tree"]);
}

// ---------------------------------------------------------------- mode-specific window styles against fake window managers

use decebal::theme::engine::decoration_compat;
use decebal::theme::model::Mode;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;

const OLD_STYLES: &[&str] = &["macos", "macos-light", "classic", "windows", "windows-light", "adwaita", "adwaita-light", "breeze", "arc", "materia", "nord", "gruvbox", "dracula", "minimal", "borderonly", "xfce-default", "xfce-daloa", "xfce-kokodi"];
const NEW_STYLES: &[&str] = &[
    "macos", "macos-light", "classic", "windows", "windows-light", "adwaita", "adwaita-light", "breeze", "arc", "materia", "nord", "gruvbox", "dracula", "minimal", "borderonly", "xfce-default", "xfce-daloa", "xfce-kokodi",
    "openkylin", "openkylin-dark", "deepin", "deepin-dark", "lingmo", "lingmo-dark",
];

fn schema_reply(styles: &[&str]) -> String {
    serde_json::json!({"ok": true, "keys": [{"section": "decoration", "key": "style", "choices": styles}], "decoration_styles": styles.iter().map(|s| serde_json::json!({"name": s})).collect::<Vec<_>>()}).to_string()
}

/// A fake rdwm: exactly one connection (the request itself; the engine makes no separate probe connection), answered with `reply`.
fn fake_wm(sock: &Path, reply: Option<String>) -> std::thread::JoinHandle<()> {
    let listener = UnixListener::bind(sock).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(1) {
            let Ok(mut s) = stream else { continue };
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            if let Some(text) = &reply {
                let _ = writeln!(s, "{text}");
            }
        }
    })
}

fn fake_ctl(dir: &Path, styles: Option<&[&str]>) -> PathBuf {
    let p = dir.join("rdwmctl");
    let body = match styles {
        // `check-config` is accepted like a current rdwmctl does; anything else is an error
        Some(s) => format!("#!/bin/sh\nif [ \"$1 $2\" = \"schema --offline\" ]; then cat <<'EOF'\n{}\nEOF\nelif [ \"$1\" = \"check-config\" ]; then exit 0; else exit 2; fi\n", schema_reply(s)),
        None => "#!/bin/sh\nexit 3\n".to_string(),
    };
    fs::write(&p, body).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

enum Wm {
    /// a running rdwm listing these styles
    Running(&'static [&'static str]),
    /// a running rdwm that answers with something that is not a schema
    Garbage,
    /// nothing running; the target CLI lists these (or fails when `None`)
    NotRunning(Option<&'static [&'static str]>),
}

/// `apply::plan` for the Lingmo package in one mode against a fake window manager; returns the plan result and the private paths.
fn plan_in_mode(tag: &str, mode: Mode, wm: Wm) -> (Result<apply::Plan, String>, Paths, PathBuf) {
    let dir = std::env::temp_dir().join(format!("lingmo-wm-{}-{tag}", std::process::id()));
    let pkg = dir.join("themes").join(ID);
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("theme.toml"), manifest_text()).unwrap();
    fs::create_dir_all(dir.join("shell")).unwrap();
    let sock = dir.join("rdwm.sock");
    let (socket, ctl, handle) = match wm {
        Wm::Running(styles) => (sock.clone(), fake_ctl(&dir, Some(NEW_STYLES)), Some(fake_wm(&sock, Some(schema_reply(styles))))),
        Wm::Garbage => (sock.clone(), fake_ctl(&dir, Some(NEW_STYLES)), Some(fake_wm(&sock, Some("not json".into())))),
        Wm::NotRunning(styles) => (dir.join("none.sock"), fake_ctl(&dir, styles), None),
    };
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: ctl, socket };
    fs::write(&paths.rdwm_config, USER_RDWM).unwrap();
    let src = Source { shell_json: paths.shell_json(), themes_dir: dir.join("themes") };
    let sel = Selection { mode, ..Selection::default() };
    let result = apply::plan(&src, &paths, ID, &sel);
    if let Some(h) = handle {
        h.join().ok();
    }
    (result, paths, dir)
}

fn written(plan: &apply::Plan, path: &Path) -> String {
    plan.writes.iter().find(|w| w.path == path).map(|w| w.new.clone()).unwrap_or_default()
}

#[test]
fn lingmo_wants_the_native_pair_by_mode_with_same_mode_fallbacks() {
    let e = |dark| {
        let doc: toml_edit::DocumentMut = manifest_text().parse().unwrap();
        let mut m = serde_json::Map::new();
        m.insert("decoration".into(), to_json(doc.get("decoration").unwrap()));
        decoration_compat::wanted(&m, dark)
    };
    assert_eq!(e(false), Some(decoration_compat::Wanted { desired: "lingmo".into(), fallback: Some("adwaita-light".into()) }));
    assert_eq!(e(true), Some(decoration_compat::Wanted { desired: "lingmo-dark".into(), fallback: Some("adwaita".into()) }));
    // the fallbacks themselves are styles every rdwm that lists the pair also lists (and old rdwm builds list them too)
    for id in ["adwaita", "adwaita-light"] {
        assert!(OLD_STYLES.contains(&id));
    }
    // a package that uses the mode keys must declare the framework floor, which the writer derives from the keys
    assert!(manifest_text().contains("min_shell_version = \"0.3.0\""));
}

#[test]
fn a_new_running_wm_gets_the_native_style_of_the_mode() {
    for (mode, style) in [(Mode::Dark, "lingmo-dark"), (Mode::Light, "lingmo")] {
        let (plan, paths, dir) = plan_in_mode(&format!("new-{style}"), mode, Wm::Running(NEW_STYLES));
        let plan = plan.unwrap_or_else(|e| panic!("{style}: {e}"));
        let cfg = written(&plan, &paths.rdwm_config);
        assert!(cfg.contains(&format!("style = \"{style}\"")), "{style} in\n{cfg}");
        let shell: Value = serde_json::from_str(&written(&plan, &paths.shell_json())).unwrap();
        assert!(shell.get(decoration_compat::RECORD).is_none(), "no warning while the wanted style applies");
        // the user's tiling policy still survives byte for byte
        for kept in ["mfact = 0.73", "nmaster = 2", "smart_gaps = true"] {
            assert!(cfg.contains(kept), "{kept}");
        }
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn an_old_running_wm_gets_the_proven_fallback_and_a_persistent_warning_not_the_new_style() {
    // the target CLI (the installed one) knows the new styles; the RUNNING rdwm does not: the running answer decides
    for (mode, desired, fallback) in [(Mode::Dark, "lingmo-dark", "adwaita"), (Mode::Light, "lingmo", "adwaita-light")] {
        let (plan, paths, dir) = plan_in_mode(&format!("old-{desired}"), mode, Wm::Running(OLD_STYLES));
        let plan = plan.unwrap_or_else(|e| panic!("{desired}: {e}"));
        let cfg = written(&plan, &paths.rdwm_config);
        assert!(cfg.contains(&format!("style = \"{fallback}\"")) && !cfg.contains(&format!("\"{desired}\"")), "{cfg}");
        let shell: Value = serde_json::from_str(&written(&plan, &paths.shell_json())).unwrap();
        let rec = &shell[decoration_compat::RECORD];
        assert_eq!((rec["desired"].as_str(), rec["applied"].as_str(), rec["proof"].as_str(), rec["theme"].as_str()), (Some(desired), Some(fallback), Some("running"), Some(ID)));
        assert!(decoration_compat::warning(&shell).unwrap().contains("log in again"));
        assert!(plan.notes.iter().any(|n| n.contains("needs a newer rdwm")), "{:?}", plan.notes);
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn without_a_running_wm_only_the_explicit_target_cli_is_proof_and_it_is_labelled() {
    let (plan, paths, dir) = plan_in_mode("off-new", Mode::Dark, Wm::NotRunning(Some(NEW_STYLES)));
    let plan = plan.unwrap();
    assert!(written(&plan, &paths.rdwm_config).contains("style = \"lingmo-dark\""));
    assert!(plan.notes.iter().any(|n| n.contains("not running")), "{:?}", plan.notes);
    let _ = fs::remove_dir_all(dir);
    // an older target CLI: the fallback of the mode, with the warning that says when it can change
    let (plan, paths, dir) = plan_in_mode("off-old", Mode::Light, Wm::NotRunning(Some(OLD_STYLES)));
    let plan = plan.unwrap();
    assert!(written(&plan, &paths.rdwm_config).contains("style = \"adwaita-light\""));
    let shell: Value = serde_json::from_str(&written(&plan, &paths.shell_json())).unwrap();
    assert_eq!(shell[decoration_compat::RECORD]["proof"], "offline");
    assert!(decoration_compat::warning(&shell).unwrap().contains("takes effect when rdwm starts"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_or_transient_capability_stops_before_writing_and_is_not_offline_proof() {
    // a running rdwm that answers garbage: the CLI would say "supported", it must not be believed
    let (plan, _paths, dir) = plan_in_mode("garbage", Mode::Dark, Wm::Garbage);
    let err = plan.err().expect("must stop");
    assert!(err.contains("could not be checked") && err.contains("Nothing was changed"), "{err}");
    let _ = fs::remove_dir_all(dir);
    // no rdwm and no usable target CLI
    let (plan, _paths, dir) = plan_in_mode("nocli", Mode::Dark, Wm::NotRunning(None));
    let err = plan.err().expect("must stop");
    assert!(err.contains("could not list its styles"), "{err}");
    let _ = fs::remove_dir_all(dir);
    // a fallback that the old running rdwm does not list either is not proof: stop, nothing written
    let (plan, _paths, dir) = plan_in_mode("nofallback", Mode::Dark, Wm::Running(&["macos", "classic"]));
    let err = plan.err().expect("must stop");
    assert!(err.contains("fallback `adwaita` is not listed"), "{err}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_later_apply_clears_the_warning_once_the_running_wm_supports_the_style() {
    let (plan, paths, dir) = plan_in_mode("clear-1", Mode::Dark, Wm::Running(OLD_STYLES));
    let plan = plan.unwrap();
    // persist what the first apply wrote, then apply again against an upgraded rdwm
    fs::write(paths.shell_json(), written(&plan, &paths.shell_json())).unwrap();
    assert!(decoration_compat::warning(&serde_json::from_str(&fs::read_to_string(paths.shell_json()).unwrap()).unwrap()).is_some());
    let sock = dir.join("upgraded.sock");
    let handle = fake_wm(&sock, Some(schema_reply(NEW_STYLES)));
    let paths2 = Paths { socket: sock, ..paths.clone() };
    let src = Source { shell_json: paths2.shell_json(), themes_dir: dir.join("themes") };
    let again = apply::plan(&src, &paths2, ID, &Selection { mode: Mode::Dark, ..Selection::default() }).unwrap();
    handle.join().ok();
    let shell: Value = serde_json::from_str(&written(&again, &paths2.shell_json())).unwrap();
    assert!(shell.get(decoration_compat::RECORD).is_none(), "the record goes away when the wanted style applies");
    assert!(written(&again, &paths2.rdwm_config).contains("style = \"lingmo-dark\""), "the wanted style applies once the running rdwm lists it");
    let _ = fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------- the manifest against the pinned original, and the native strip

#[test]
fn manifest_numbers_are_the_pinned_originals_and_the_native_strip_matches_them() {
    let reference: Value = serde_json::from_str(&fs::read_to_string(docs().join("native/reference-numbers.json")).unwrap()).unwrap();
    let sources: Value = serde_json::from_str(&fs::read_to_string(docs().join("sources.json")).unwrap()).unwrap();
    let pinned = sources["primary"].as_array().unwrap().iter().find(|p| p["repo"] == "LingmoOS/lingmo-compositor").unwrap()["commit"].clone();
    assert_eq!(reference["commit"], pinned, "the numbers were extracted at the commit sources.json pins");
    let v = |name: &str| reference["values"][name]["value"].as_f64().unwrap_or_else(|| panic!("{name}"));
    let (bar, dock) = (section("bar"), section("dock"));
    // the declared 32 is StatusBarWindow::kHeight, read from the original, not transcribed
    assert_eq!(bar["height"].as_f64().unwrap(), v("status_bar_height"));
    assert_eq!(dock["size"].as_f64().unwrap(), v("dock_base_icon_size"));
    assert_eq!(dock["magnification"].as_f64().unwrap(), v("dock_magnification_factor"));
    let t = def().resolve(ID, true, &Selection::default());
    assert!(t.bar_height_declared, "the engine records that the theme declared its bar height");
    assert_eq!(t.bar_height as f64, v("status_bar_height"));
    assert_eq!(t.flat_bar_height() as f64, v("status_bar_height"), "and a classic strip along the top or bottom is that tall");
    assert_eq!(t.dock_padding as f64, v("dock_outer_padding"));
    assert!((t.bar_alpha as f64 - v("status_bar_opacity")).abs() < 1e-6);
    assert!((t.blur as f64 - v("status_bar_blur_sigma") / 64.0).abs() < 0.01);
    assert_eq!(t.radius as f64, v("decoration_corner_radius"));
    // real decbar captures: the strip is the declared height times the display scale on top and bottom, and the 40 px rail (never thinner) on the sides
    let measured: Value = serde_json::from_str(&fs::read_to_string(docs().join("native/bar-height/measurements.json")).unwrap()).unwrap();
    let rows = measured["measurements"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    for r in rows {
        let (edge, scale, got) = (r["edge"].as_str().unwrap(), r["scale"].as_f64().unwrap(), r["thickness_px"].as_u64().unwrap());
        let logical = if matches!(edge, "left" | "right") { t.rail_thickness() as f64 } else { v("status_bar_height") };
        assert_eq!(got, (logical * scale).round() as u64, "{} {edge} at {scale}", r["capture"]);
    }
}
