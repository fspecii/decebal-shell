//! GNOME theme ports (`themes/gnome-*`): package safety, pinned upstream values, declared native behaviour and honest evidence.
//! Isolated: private config/theme directories only, no display, no live settings, no network.
use decebal::{
    config::DUX_BAR_ITEMS,
    settings::{io::Paths, tomlcfg},
    theme::{
        engine::{apply, archive::Limits, manifest, package},
        live::Source,
        model::{contrast, Def, Mode, Rgb, Selection},
    },
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const PORTS: &[&str] = &["gnome-adwaita", "gnome-catppuccin-dock"];

/// The release that ships `control_center.style = "gnome"` (assigned by the release owner); the packages declare it as their floor.
const GATE: (u32, u32, u32) = (0, 3, 0);
const GATE_TEXT: &str = "0.3.0";

fn build_version() -> (u32, u32, u32) {
    decebal::theme::engine::schema::parse_version(env!("CARGO_PKG_VERSION")).unwrap()
}
/// Whether this build is new enough to load the packages as shipped.
fn gate_open() -> bool {
    build_version() >= GATE
}
fn real_themes() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("themes")
}
/// The themes folder the engine is pointed at: the repository's own when the build is >= 0.3.0, otherwise a copy of the two packages whose
/// floor is lowered to this build (only `theme.toml` differs) so apply, preview and render can still be exercised on an older tree.
/// Never used to hide the gate: [`packages_declare_the_gnome_look_behind_the_0_3_0_gate`] checks the real files and the refusal.
fn themes() -> PathBuf {
    use std::sync::OnceLock;
    static LOWERED: OnceLock<PathBuf> = OnceLock::new();
    if gate_open() {
        return real_themes();
    }
    LOWERED
        .get_or_init(|| {
            let dir = scratch("gate-lowered");
            for id in PORTS {
                for entry in package::read_dir(&real_themes().join(id), &Limits::default()).unwrap() {
                    let dest = dir.join(id).join(&entry.path);
                    fs::create_dir_all(dest.parent().unwrap()).unwrap();
                    let data = if entry.path == "theme.toml" {
                        String::from_utf8(entry.data).unwrap().replace(&format!("min_shell_version = \"{GATE_TEXT}\""), &format!("min_shell_version = \"{}\"", env!("CARGO_PKG_VERSION"))).into_bytes()
                    } else {
                        entry.data
                    };
                    fs::write(dest, data).unwrap();
                }
            }
            dir
        })
        .clone()
}
fn root(id: &str) -> PathBuf {
    real_themes().join(id)
}
fn real_text(id: &str) -> String {
    fs::read_to_string(root(id).join("theme.toml")).unwrap()
}
/// The manifest as this build can load it (see [`themes`]).
fn text(id: &str) -> String {
    fs::read_to_string(themes().join(id).join("theme.toml")).unwrap()
}
fn def(id: &str) -> Def {
    Def::parse(&text(id)).unwrap_or_else(|e| panic!("{id}: {e}"))
}
fn rgb(s: &str) -> Rgb {
    Rgb::parse(s).unwrap()
}
fn scratch(tag: &str) -> PathBuf {
    let base = std::env::var_os("DECEBAL_TEST_TMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("gnome-ports-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}
fn json_str<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(*k)).and_then(Value::as_str)
}

/// On a build older than the gate the real package must be refused for exactly that reason and nothing else; from the gate on it must be clean.
fn assert_gate_only(errors: &[String], id: &str) {
    if gate_open() {
        assert!(errors.is_empty(), "{id}: {errors:?}");
    } else {
        assert!(!errors.is_empty() && errors.iter().all(|e| e.contains("min_shell_version") && e.contains(GATE_TEXT)), "{id}: only the {GATE_TEXT} floor may refuse it on this build: {errors:?}");
    }
}

#[test]
fn packages_are_valid_reproducible_data_without_baked_previews_or_wallpapers() {
    for id in PORTS {
        let dir = root(id);
        let entries = package::read_dir(&dir, &Limits::default()).unwrap();
        let report = package::check(&entries, &Limits::default());
        assert_gate_only(&report.errors(), id);
        if gate_open() {
            assert_eq!(report.id.as_deref(), Some(*id), "id is the folder name");
        }
        let loadable = themes().join(id); // the repository's own folder once the build reaches the gate
        assert_eq!(package::pack_dir(&loadable, &Limits::default()).unwrap().0, package::pack_dir(&loadable, &Limits::default()).unwrap().0, "{id}: reproducible pack");
        // No hard-coded preview and no packaged wallpaper: previews are generated natively, wallpapers are recommendations only.
        let names: BTreeSet<_> = entries.iter().map(|e| e.path.clone()).collect();
        for n in &names {
            assert!(!n.starts_with("screenshots/") && !n.starts_with("wallpapers/"), "{id}: {n}");
            assert!(!n.ends_with(".png") && !n.ends_with(".jpg") && !n.ends_with(".webp"), "{id}: {n}");
        }
        let t = text(id);
        assert!(!t.contains("\nscreenshots") && !t.contains("[wallpaper]"), "{id}: independent wallpaper, native previews");
        for need in ["theme.toml", "README.md", "ATTRIBUTION.md", "LICENSE.txt", "fonts/MaterialSymbolsRounded.ttf", "MaterialSymbolsRounded-LICENSE.txt"] {
            assert!(names.contains(need), "{id}: {need}");
        }
        // Only the one licensed icon font is bundled; the GNOME text faces are system dependencies.
        let fonts: Vec<_> = names.iter().filter(|n| n.starts_with("fonts/")).collect();
        assert_eq!(fonts, ["fonts/MaterialSymbolsRounded.ttf"], "{id}");
        // `t` is the manifest as this build can load it (the repository's own once the build reaches the gate)
        let analysis = manifest::analyze(&t);
        assert!(analysis.def.is_some());
        assert!(analysis.diags.is_empty(), "{id}: no pending/unknown key may hide in the manifest: {:?}", analysis.diags);
        assert!(!t.contains("custom:") && !t.contains("exec"), "{id}: data only");
    }
}

#[test]
fn bundled_symbol_font_is_the_exact_licensed_upstream_copy() {
    let reference = fs::read(real_themes().join("dux-lavender/fonts/MaterialSymbolsRounded.ttf")).unwrap();
    let lic = fs::read(real_themes().join("dux-lavender/MaterialSymbolsRounded-LICENSE.txt")).unwrap();
    for id in PORTS {
        assert_eq!(fs::read(root(id).join("fonts/MaterialSymbolsRounded.ttf")).unwrap(), reference, "{id}");
        assert_eq!(fs::read(root(id).join("MaterialSymbolsRounded-LICENSE.txt")).unwrap(), lic, "{id}");
        assert!(String::from_utf8_lossy(&lic).contains("Apache License"));
    }
}

#[test]
fn every_variant_is_readable_and_bright_where_it_claims_to_be() {
    for id in PORTS {
        let d = def(id);
        assert!(d.has_dark() && d.has_light(), "{id}: both variants");
        for dark in [true, false] {
            let t = d.resolve(id, dark, &Selection::default());
            for (what, bg) in [("background", t.background), ("surface", t.surface), ("popup", t.popup), ("titlebar", t.titlebar), ("sidebar", t.sidebar), ("overlay", t.overlay)] {
                assert!(contrast(t.text, bg) >= 4.5, "{id} dark={dark}: text on {what} {:.2}", contrast(t.text, bg));
            }
            assert!(contrast(t.bar_text, t.bar) >= 4.5, "{id} dark={dark}: bar text");
            assert!(contrast(t.accent_text, t.accent) >= 3.0, "{id} dark={dark}: accent text {:.2}", contrast(t.accent_text, t.accent));
            assert!(contrast(t.text_secondary, t.background) >= 3.0, "{id} dark={dark}: secondary text {:.2}", contrast(t.text_secondary, t.background));
            assert_eq!(t.background.luminance() > 0.3, !dark, "{id} dark={dark}: variant brightness");
        }
    }
}

/// The pinned upstream values (libadwaita 1.10.0 c9ab118, gnome-shell 51.1 33da068) in the resolved tokens.
#[test]
fn adwaita_tokens_and_geometry_match_the_pinned_upstream_values() {
    let d = def("gnome-adwaita");
    let light = d.resolve("gnome-adwaita", false, &Selection::default());
    let dark = d.resolve("gnome-adwaita", true, &Selection::default());
    for (name, got, want) in [
        ("light window_bg", light.background, "#fafafb"),
        ("light sidebar", light.sidebar, "#ebebed"),
        ("light card/view", light.surface, "#ffffff"),
        ("light headerbar", light.titlebar, "#ffffff"),
        ("light popover", light.popup, "#ffffff"),
        ("light overview", light.overlay, "#f3f3f5"),
        ("light panel", light.bar, "#fafafb"),
        ("light panel fg", light.bar_text, "#222226"),
        ("accent blue_3", light.accent, "#3584e4"),
        ("dark window_bg", dark.background, "#222226"),
        ("dark sidebar", dark.sidebar, "#2e2e32"),
        ("dark headerbar", dark.titlebar, "#2e2e32"),
        ("dark popover", dark.popup, "#36363a"),
        ("dark overview", dark.overlay, "#28282c"),
        ("dark panel is black", dark.bar, "#000000"),
        ("dash is always dark (light)", light.dock, "#38383b"),
        ("dash is always dark (dark)", dark.dock, "#38383b"),
        ("accent blue_3 (dark)", dark.accent, "#3584e4"),
    ] {
        assert_eq!(got, rgb(want), "{name}");
    }
    assert_eq!(dark.text, rgb("#ffffff"));
    assert_eq!((light.accent_text, dark.accent_text), (rgb("#ffffff"), rgb("#ffffff")));
    // Pre-blended alpha values: window_fg rgb(0 0 6 / 80 %) over #fafafb, border at 15 %.
    let blend = |fg: [f32; 3], a: f32, bg: [f32; 3]| -> [u8; 3] { std::array::from_fn(|i| (fg[i] * a + bg[i] * (1.0 - a)).round() as u8) };
    let b = blend([0.0, 0.0, 6.0], 0.8, [250.0, 250.0, 251.0]);
    assert_eq!(light.text, Rgb(b[0], b[1], b[2]), "light text = window_fg over window_bg");
    let bd = blend([b[0] as f32, b[1] as f32, b[2] as f32], 0.15, [250.0, 250.0, 251.0]);
    assert_eq!(light.border, Rgb(bd[0], bd[1], bd[2]), "light border = 15 % of the foreground");
    assert_eq!(
        dark.surface,
        {
            let c = blend([255.0; 3], 0.08, [34.0, 34.0, 38.0]);
            Rgb(c[0], c[1], c[2])
        },
        "dark card = 8 % white over window_bg"
    );

    let extra = &d_extra("gnome-adwaita");
    assert_eq!(extra["decoration"]["height"], 47, "headerbar min-height");
    assert_eq!(extra["decoration"]["radius"], 15, "--window-radius = 9 + 6");
    assert_eq!(extra["decoration"]["style"], "adwaita");
    assert_eq!(extra["decoration"]["title_align"], "center");
    assert_eq!(extra["bar"]["height"], 32, "#panel 2.2em at 11pt");
    assert_eq!(extra["dock"]["size"], 48);
    assert_eq!(light.radius, 20.0, "popover $modal_radius * 1.25");
    assert_eq!(light.dock_padding, 12.0, "$dash_padding");
    assert_eq!(extra["hot_corners"]["top_left"], "overview");
    assert_eq!(extra["control_center"]["style"], "gnome");
    assert_eq!(extra["desktop"]["widgets"], "none");
    assert_eq!(extra["dock"]["enabled"], false, "stock GNOME only shows the dash in the Overview");
    assert_eq!(extra["rdwm"]["border_px"], 0);
}

#[test]
fn catppuccin_rice_uses_the_published_palette_and_the_documented_extension_defaults() {
    let d = def("gnome-catppuccin-dock");
    let dark = d.resolve("gnome-catppuccin-dock", true, &Selection::default());
    let light = d.resolve("gnome-catppuccin-dock", false, &Selection::default());
    // catppuccin/palette 1.8.0, 07d02aa: mocha / latte.
    for (name, got, want) in [
        ("mocha base", dark.background, "#1e1e2e"),
        ("mocha mantle", dark.sidebar, "#181825"),
        ("mocha surface0", dark.surface, "#313244"),
        ("mocha crust (panel)", dark.bar, "#11111b"),
        ("mocha text", dark.text, "#cdd6f4"),
        ("mocha mauve", dark.accent, "#cba6f7"),
        ("mocha red", dark.danger, "#f38ba8"),
        ("mocha green", dark.ok, "#a6e3a1"),
        ("latte base", light.background, "#eff1f5"),
        ("latte mantle", light.surface, "#e6e9ef"),
        ("latte text", light.text, "#4c4f69"),
        ("latte mauve", light.accent, "#8839ef"),
        ("latte red", light.danger, "#d20f39"),
    ] {
        assert_eq!(got, rgb(want), "{name}");
    }
    let extra = d_extra("gnome-catppuccin-dock");
    assert_eq!(extra["dock"]["enabled"], true);
    assert_eq!(extra["dock"]["size"], 48, "dash-max-icon-size default");
    assert_eq!(dark.dock_alpha, 0.8, "background-opacity default");
    assert!(dark.bar_alpha < 1.0 && dark.blur > 0.0, "Blur my Shell translucency");
    assert_eq!(extra["bar"]["height"], 32);
    assert_eq!(extra["hot_corners"]["top_left"], "overview");
    assert_eq!(extra["icons"]["theme"], "Papirus-Dark");
    assert_eq!(extra["icons"]["fallback"], "Adwaita", "a missing Papirus must fall back, not disappear");
    assert_eq!(extra["rdwm"]["border_px"], 2);
}

fn d_extra(id: &str) -> Value {
    Value::Object(def(id).extra.clone())
}

#[test]
fn panel_items_are_declared_native_unique_and_cover_the_gnome_panel_regions() {
    for id in PORTS {
        let extra = d_extra(id);
        assert_eq!(extra["bar"]["style"], "dux", "{id}: the only bar style that honours item lists");
        assert_eq!(extra["bar"]["position"], "top");
        let mut seen = BTreeSet::new();
        for slot in ["left", "center", "right"] {
            let list = extra["bar"][slot].as_array().unwrap_or_else(|| panic!("{id}: bar.{slot} declared, no silent omission"));
            assert!(!list.is_empty(), "{id}: bar.{slot}");
            for item in list {
                let item = item.as_str().unwrap();
                assert!(DUX_BAR_ITEMS.contains(&item), "{id}: {item} is a native item id");
                assert!(seen.insert(item.to_string()), "{id}: {item} appears once");
            }
        }
        // clock in the middle, quick settings at the right end, workspace indicator at the left: the three GNOME regions.
        assert_eq!(extra["bar"]["center"][0], "clock");
        let right = extra["bar"]["right"].as_array().unwrap();
        assert_eq!(right.last().unwrap(), "controlCenterButton");
        assert!(extra["bar"]["left"].as_array().unwrap().iter().any(|i| i == "workspaceSwitcher"));
        assert_eq!(extra["control_center"]["enabled"], true);
        assert_eq!(extra["control_center"]["position"], "auto", "follows whichever edge the panel is on");
        assert_eq!(extra["control_center"]["tiles"].as_array().unwrap().len(), 7, "every quick-settings group stays on");
        // Square, flat band like the real panel.
        assert_eq!(extra["bar"]["band_radius"], 0);
        assert_eq!(extra["bar"]["margin"], 0);
    }
}

#[test]
fn applying_a_port_keeps_wallpaper_and_user_layouts_and_writes_the_declared_structure() {
    for id in PORTS {
        let dir = scratch(id);
        fs::create_dir_all(dir.join("shell")).unwrap();
        let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("no.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: themes() };
        fs::write(&paths.rdwm_config, "[wallpaper]\nimage = \"/private/photo.jpg\"\nmode = \"center\"\ncolor = \"#123456\"\n").unwrap();
        for mode in [Mode::Dark, Mode::Light] {
            let sel = Selection { mode, ..Selection::default() };
            let p = apply::plan(&src, &paths, id, &sel).unwrap_or_else(|e| panic!("{id}: {e}"));
            let cfg_text = &p.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new;
            let cfg = tomlcfg::Cfg::parse(cfg_text).unwrap();
            assert_eq!(cfg.get("wallpaper", "image"), Some(tomlcfg::Val::Str("/private/photo.jpg".into())), "{id}: wallpaper independent");
            assert_eq!(cfg.get("wallpaper", "mode"), Some(tomlcfg::Val::Str("center".into())));
            assert_eq!(cfg.get("decoration", "style"), Some(tomlcfg::Val::Str("adwaita".into())));
            assert_eq!(cfg.get("decoration", "height"), Some(tomlcfg::Val::Int(47)));
            assert_eq!(cfg.get("decoration", "radius"), Some(tomlcfg::Val::Int(15)));
            let shell: Value = serde_json::from_str(&p.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
            assert_eq!(p.selection.name, *id);
            // The bar shape is a resolved theme token (decebal.json holds only the item lists and geometry).
            assert_eq!(def(id).resolve(id, mode == Mode::Dark, &Selection::default()).bar_style, "dux");
            assert_eq!(shell["bar"]["center"][0], "clock");
            assert_eq!(shell["bar"]["right"].as_array().unwrap().last().unwrap(), "controlCenterButton");
            assert_eq!(json_str(&shell, &["hot_corners", "top_left"]), Some("overview"));
            assert_eq!(shell["control_center"]["enabled"], true);
            assert_eq!(shell["dock"]["enabled"], *id == "gnome-catppuccin-dock");
            assert!(shell.get("theme_wallpaper").is_none(), "{id}: no package wallpaper");
            assert_eq!(p.dark, mode == Mode::Dark);
            assert!(p.notes.iter().all(|n| !n.contains("bar.left") && !n.contains("bar.center")), "{id}: item lists apply: {:?}", p.notes);
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

/// The genuine GNOME quick-settings look (`control_center.style = "gnome"`, `Style::Gnome`) ships with 0.3.0, so both packages declare it and
/// that floor (never 0.2.x capability). A build below the gate must refuse them for exactly that reason; from the gate on they load clean and
/// applying one writes the GNOME look with every tile group on.
#[test]
fn packages_declare_the_gnome_look_behind_the_0_3_0_gate() {
    use decebal::controlcenter::settings::{Style, STYLE_NAMES};
    assert!(STYLE_NAMES.contains(&"gnome") && Style::parse("gnome") == Some(Style::Gnome) && Style::Gnome.as_str() == "gnome");
    for id in PORTS {
        let original = real_text(id);
        assert!(original.contains(&format!("min_shell_version = \"{GATE_TEXT}\"")), "{id}: the floor is the release that ships the look");
        assert!(!original.contains("min_shell_version = \"0.2"), "{id}: no 0.2.x capability is claimed");
        assert_eq!(d_extra(id)["control_center"]["style"], "gnome");
        // the manifest as shipped: refused below the gate (for that reason only), clean at or above it
        let analysis = manifest::analyze(&original);
        let gate_errors: Vec<String> = analysis.diags.iter().filter(|d| d.level == manifest::Level::Error).map(|d| d.to_string()).collect();
        assert_gate_only(&gate_errors, id);
        assert_eq!(analysis.def.is_some(), gate_open());
        // the validator allowlist is strict
        let typo = manifest::analyze(&original.replace("style = \"gnome\"", "style = \"gnomes\""));
        assert!(typo.def.is_none() && typo.diags.iter().any(|d| d.key == "control_center.style"), "{id}");
        // applying writes the GNOME look and all seven groups (the loadable copy on a build below the gate)
        let dir = scratch(&format!("{id}-gate"));
        fs::create_dir_all(dir.join("shell")).unwrap();
        let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("no.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: themes() };
        let plan = apply::plan(&src, &paths, id, &Selection::default()).unwrap_or_else(|e| panic!("{id}: {e}"));
        let shell: Value = serde_json::from_str(&plan.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
        assert_eq!(shell["control_center"]["style"], "gnome", "{id}");
        assert_eq!(shell["control_center"]["tiles"].as_array().unwrap().len(), 7, "{id}: every tile group stays on");
        fs::remove_dir_all(dir).unwrap();
    }
}

/// The applet keys of the universal-panel contract (docs/universal-panels.md at 71e3434); unknown keys inside `[bar] applets` are ignored by a 0.3 engine, but the
/// proposed lists must name real ones.
const APPLET_KEYS: &[&str] = &[
    "launcher",
    "app_menu",
    "workspaces",
    "window_title",
    "tasks",
    "layout",
    "media",
    "clock",
    "weather",
    "tray",
    "clipboard",
    "cpu",
    "memory",
    "notifications",
    "stay_awake",
    "agents",
    "lock_keys",
    "volume",
    "network",
    "bluetooth",
    "battery",
    "brightness",
    "search",
    "dashboard",
    "control_center",
    "power",
    "show_desktop",
];

/// The manifest text as this build can load it: below the gate the 0.3.0 floor is lowered to the build, so only the framework keys can differ.
fn loadable_text(text: &str) -> String {
    if gate_open() {
        text.to_string()
    } else {
        text.replace(&format!("min_shell_version = \"{GATE_TEXT}\""), &format!("min_shell_version = \"{}\"", env!("CARGO_PKG_VERSION")))
    }
}

/// The packages ship the framework defaults rather than leaving them in an unused proposal.
#[test]
fn framework_manifests_ship_the_declared_applets_and_branding() {
    for id in PORTS {
        let proposed_text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/gnome-theme-ports/framework-0.3/{id}.theme.toml"))).unwrap();
        // the framework keys, parsed structurally
        let doc: toml_edit::DocumentMut = proposed_text.parse().unwrap();
        let applets: Vec<String> = doc["bar"]["applets"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(!applets.is_empty() && applets.iter().all(|a| APPLET_KEYS.contains(&a.as_str())), "{id}: {applets:?}");
        let unique: BTreeSet<_> = applets.iter().collect();
        assert_eq!(unique.len(), applets.len(), "{id}: duplicate applets");
        assert!(!applets.iter().any(|a| matches!(a.as_str(), "lock_keys" | "power")), "{id}: lock and power live in the quick-settings system row");
        assert_eq!(doc["bar"]["applets"].as_array().unwrap().iter().filter(|a| a.as_str() == Some("control_center")).count(), 1);
        let branding = doc["branding"].as_table().unwrap();
        assert_eq!(branding["logo"].as_str(), Some("icon:start-here-symbolic"));
        assert_eq!(branding["logo_tint"].as_bool(), Some(true));
        assert_eq!(branding["desktop_label"].as_str(), Some("Desktop"));
        assert_eq!(branding.get("launcher_label").and_then(|v| v.as_str()), if *id == "gnome-adwaita" { Some("Activities") } else { None });
        assert!(doc.get("min_shell_version").is_some() || proposed_text.contains("min_shell_version = \"0.3.0\""));
        let here = manifest::analyze(&loadable_text(&proposed_text));
        assert!(here.def.is_some(), "{id}: {:?}", here.diags);
        assert!(!here.diags.iter().any(|d| d.level == manifest::Level::Error), "{id}: {:?}", here.diags);
        let shipped = manifest::analyze(&text(id));
        assert_eq!(here.def, shipped.def, "{id}: the validated fixture and shipped package must agree");
    }
}

/// The user's explicit global window behaviour (new apps tile first, free drag, Shift-snap, auto-formation) must survive a theme apply:
/// appearance may set decoration, gaps and border, never layouts or tiling parameters.
#[test]
fn themes_never_override_the_global_tiling_preference() {
    for id in PORTS {
        let extra = d_extra(id);
        for key in ["layouts", "mfact", "nmaster", "smart_gaps"] {
            assert!(extra["rdwm"].get(key).is_none(), "{id}: [rdwm] {key} would override the user's window policy");
        }
        let dir = scratch(&format!("{id}-wm"));
        fs::create_dir_all(dir.join("shell")).unwrap();
        let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("no.sock") };
        let src = Source { shell_json: paths.shell_json(), themes_dir: themes() };
        let general = "[general]\nlayouts = [\"tile\", \"monocle\"]\nmfact = 0.61\nnmaster = 2\nsmart_gaps = true\n";
        fs::write(&paths.rdwm_config, general).unwrap();
        for mode in [Mode::Dark, Mode::Light] {
            let p = apply::plan(&src, &paths, id, &Selection { mode, ..Selection::default() }).unwrap();
            let cfg = tomlcfg::Cfg::parse(&p.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new).unwrap();
            assert_eq!(cfg.get("general", "layouts"), Some(tomlcfg::Val::List(vec!["tile".into(), "monocle".into()])), "{id}: layouts kept");
            assert_eq!(cfg.get("general", "mfact"), Some(tomlcfg::Val::Float(0.61)), "{id}: mfact kept");
            assert_eq!(cfg.get("general", "nmaster"), Some(tomlcfg::Val::Int(2)));
            assert_eq!(cfg.get("general", "smart_gaps"), Some(tomlcfg::Val::Bool(true)));
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn wallhaven_fragment_records_provenance_and_claims_no_licence() {
    let frag: Value = serde_json::from_str(&fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gnome-theme-ports/wallhaven-fragment.json")).unwrap()).unwrap();
    let images = frag["images"].as_object().unwrap();
    assert_eq!(images.len(), 5);
    for (id, img) in images {
        assert_eq!(img["purity"], "sfw", "{id}");
        assert!(img["license"]["identifier"].is_null(), "{id}: the API publishes no licence; none is invented");
        assert!(img["uploader"]["is_author"].is_null(), "{id}: uploader is not author");
        // only a primary listing may name an author, and even then identity and capture method stay uncertified
        if let Some(name) = img["author"]["name"].as_str() {
            assert_eq!((id.as_str(), name), ("yx3w57", "Paul Wilson"));
            assert!(img["author"]["verification"].as_str().unwrap().contains("not independently certified"));
        }
        assert!(!img["tags"].as_array().unwrap().iter().any(|t| t["id"] == 133451), "{id}: AI-art tag");
        for hash in [&img["evidence"]["detail_sha256"], &img["evidence"]["thumbnail_sha256"], &img["full_resolution_review"]["sha256"]] {
            assert_eq!(hash.as_str().unwrap().len(), 64, "{id}");
        }
        let (w, h) = (img["resolution"][0].as_u64().unwrap(), img["resolution"][1].as_u64().unwrap());
        assert!(w >= 2560 && h >= 1440, "{id}: at least 2560x1440");
        assert_eq!(img["page_url"], format!("https://wallhaven.cc/w/{id}"));
    }
    for theme in PORTS {
        let t = &frag["themes"][theme];
        for mode in ["dark", "light"] {
            let readme = fs::read_to_string(root(theme).join("README.md")).unwrap();
            match t[mode]["image_id"].as_str() {
                Some(id) => {
                    assert!(images.contains_key(id), "{theme} {mode}");
                    assert!(readme.contains(&format!("https://wallhaven.cc/w/{id}")), "{theme}: README links {id}");
                    let status = images[id]["parent_verification"]["status"].as_str().unwrap();
                    assert!(matches!(status, "independently_accepted" | "owner_reviewed_independent_acceptance_pending"), "{theme} {mode}: only reviewed photos are mapped ({status})");
                    if status != "independently_accepted" {
                        // an owner-only review must show its work: all three crops, the readability numbers under the panel and dock, and what is still open
                        let img = &images[id];
                        for crop in ["16x9", "ultrawide", "portrait"] {
                            assert!(img["crop_review"][crop]["result"].as_str().unwrap().starts_with("accepted"), "{id} {crop}");
                            assert_eq!(img["crop_review"][crop]["sha256"].as_str().unwrap().len(), 64);
                            assert!(img["readability_metrics"][crop]["panel_p95"].as_f64().unwrap() < 0.26, "{id} {crop}: panel readability");
                        }
                        assert!(img["parent_verification"]["remaining"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains("independent acceptance")));
                        assert!(img["license"]["identifier"].is_null() && img["author"]["name"].is_null());
                    }
                }
                None => {
                    // an unaccepted slot stays visibly pending: no unrelated photo is substituted
                    assert_eq!(t[mode]["pending"], true, "{theme} {mode}");
                    let rejected = t[mode]["rejected_candidate"].as_str().unwrap();
                    assert!(images[rejected]["parent_verification"]["status"].as_str().unwrap().starts_with("rejected"));
                    assert!(readme.contains(rejected) && readme.to_lowercase().contains("pending"), "{theme}: README says the {mode} slot is pending");
                }
            }
        }
        assert_ne!(t["dark"]["image_id"], t["light"]["image_id"]);
    }
    assert!(frag["scope"].as_str().unwrap().contains("ignores recommended_mode"));
}

/// The native preview worker draws the actual package; its backdrop pixel must be the variant's own background token.
#[test]
fn asset_pack_catalogue_follows_the_published_contract() {
    // docs/desktop-asset-packs.md (asset owner): data only, https URLs, 40-hex pins, ids unique, references resolve, licence files named
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/desktop-ports/gnome/asset-packs.toml");
    let raw = fs::read_to_string(&path).unwrap();
    let doc: toml_edit::DocumentMut = raw.parse().unwrap();
    assert_eq!(doc["format"].as_integer(), Some(1));
    assert_eq!(doc["family"].as_str(), Some("gnome"));
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let mut sources = BTreeSet::new();
    for s in doc["source"].as_array_of_tables().unwrap() {
        let id = s["id"].as_str().unwrap();
        assert!(sources.insert(id.to_string()), "duplicate source {id}");
        assert!(s["repo"].as_str().unwrap().starts_with("https://") && s["archive_url"].as_str().unwrap().starts_with("https://"));
        let commit = s["commit"].as_str().unwrap();
        assert!(hex(commit, 40) && hex(s["archive_sha256"].as_str().unwrap(), 64), "{id}: pins");
        assert!(s["archive_url"].as_str().unwrap().contains(commit), "{id}: the archive is the pinned commit");
        assert!(!s["license"].as_str().unwrap().is_empty() && !s["license_files"].as_array().unwrap().is_empty());
    }
    let mut packs = BTreeSet::new();
    for p in doc["pack"].as_array_of_tables().unwrap() {
        let id = p["id"].as_str().unwrap();
        assert!(id.starts_with("gnome-") && packs.insert(id.to_string()), "{id}: family prefix, unique");
        assert!(sources.contains(p["source"].as_str().unwrap()), "{id}: source exists");
        assert!(hex(p["tree"].as_str().unwrap(), 40), "{id}: tree OID");
        let path = p["path"].as_str().unwrap();
        assert!(!path.starts_with('/') && !path.contains("..") && !path.contains('\\'), "{id}: path");
        assert!(p["provides"].as_array().unwrap().iter().all(|k| matches!(k.as_str(), Some("icons" | "cursors"))));
    }
    for r in doc["recommend"].as_array_of_tables().unwrap() {
        assert!(PORTS.contains(&r["theme"].as_str().unwrap()), "recommendations name a theme of this port");
        let icons = r["icons"].as_str().unwrap();
        assert!(packs.contains(icons), "{icons} is a pack");
        // the rice's own [icons] theme names what the pack installs
        let pack = doc["pack"].as_array_of_tables().unwrap().iter().find(|p| p["id"].as_str() == Some(icons)).unwrap();
        let theme = real_text(r["theme"].as_str().unwrap());
        assert!(theme.contains(&format!("theme = \"{}\"", pack["install_name"].as_str().unwrap())), "{icons}: install_name is the theme's icon theme");
    }
    // nothing but data: no key that could name a command
    let keys: BTreeSet<&str> = raw.lines().filter(|l| !l.trim_start().starts_with(['#', '[']) && l.contains('=')).map(|l| l.split('=').next().unwrap().trim()).collect();
    let known = [
        "format",
        "family",
        "id",
        "repo",
        "revision",
        "commit",
        "license",
        "license_files",
        "license_oids",
        "archive_url",
        "archive_sha256",
        "strip_components",
        "name",
        "provides",
        "source",
        "path",
        "tree",
        "install_name",
        "link_sources",
        "link_source_trees",
        "description",
        "theme",
        "icons",
        "cursors",
    ];
    assert!(keys.iter().all(|k| known.contains(k)), "unknown keys: {:?}", keys.iter().filter(|k| !known.contains(k)).collect::<Vec<_>>());
}

#[test]
fn native_preview_renders_each_port_in_both_modes() {
    let bin = env!("CARGO_BIN_EXE_decthemectl");
    for id in PORTS {
        let d = def(id);
        for (mode, dark) in [("dark", true), ("light", false)] {
            let dir = scratch(&format!("{id}-{mode}"));
            let out = dir.join("preview.png");
            let status = Command::new(bin)
                .args(["theme", "render", id, "--mode", mode, "--png"])
                .arg(&out)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("HOME", &dir)
                .env("XDG_CONFIG_HOME", &dir)
                .env("XDG_CACHE_HOME", dir.join("cache"))
                .env("XDG_DATA_HOME", &dir)
                .env("XDG_STATE_HOME", &dir)
                .env("XDG_RUNTIME_DIR", &dir)
                .env("DECEBAL_CONFIG", &dir)
                .env("DECEBAL_THEMES", themes())
                .env("DECEBAL_SYSTEM_THEMES", &dir)
                .env("DECEBAL_SYSTEM_AUTOMATIC", "0")
                .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
                .output()
                .unwrap();
            assert!(status.status.success(), "{id} {mode}: {}", String::from_utf8_lossy(&status.stderr));
            let img = image::open(&out).unwrap().to_rgb8();
            assert!(img.width() >= 1280 && img.height() >= 720);
            let want = d.resolve(id, dark, &Selection::default()).background;
            let px = img.get_pixel(img.width() / 2, img.height() / 2).0;
            assert_eq!(px, [want.0, want.1, want.2], "{id} {mode}: backdrop is the package background");
            let distinct: BTreeSet<_> = img.pixels().map(|p| p.0).collect();
            assert!(distinct.len() > 20, "{id} {mode}: bar items and captions were painted");
            if let Some(evidence) = std::env::var_os("DECEBAL_THEME_EVIDENCE") {
                let dest = PathBuf::from(evidence).join(id);
                fs::create_dir_all(&dest).unwrap();
                fs::copy(&out, dest.join(format!("{mode}.png"))).unwrap();
            }
            fs::remove_dir_all(dir).unwrap();
        }
    }
}
