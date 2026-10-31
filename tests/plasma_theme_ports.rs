//! Offline contracts of the KDE Plasma theme ports (`themes/plasma-*`). No display, network, live config or process.
//!
//! The upstream numbers below are copied from the pinned sources named in `docs/plasma-theme-ports.md`; a test that
//! recomputes every colour token from them keeps the manifests from drifting into "Breeze-ish" approximations.
use decebal::settings::io::Paths;
use decebal::theme::{
    engine::{
        apply,
        archive::Limits,
        package,
        schema::{self, Status},
    },
    live::Source,
    model::{contrast, Def, Rgb, Selection},
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

const PORTS: &[&str] = &["plasma-breeze", "plasma-layan"];

fn root(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("themes").join(id)
}

/// True when this build's engine has the universal-panel manifest contract (`[bar] applets`, `[branding]`; shared commit
/// 71e3434 and later). The packages require it (`min_shell_version = "0.3.0"`); an older engine rejects exactly those keys.
fn contract() -> bool {
    schema::field("bar.applets").is_some() && schema::field("branding.logo").is_some()
}

/// The manifest as an engine without the contract can parse it: the keys that engine rejects are removed and the version floor
/// lowered. Only used when `contract()` is false; with the contract the manifest is read verbatim.
fn compat(text: &str) -> String {
    let mut current = String::new();
    let mut out = vec![];
    for l in text.lines() {
        if l.starts_with('[') {
            current = l.to_string();
        }
        if current == "[branding]" || current == "[taskbar]" || l.starts_with("applets = ") {
            continue;
        }
        out.push(if l.starts_with("min_shell_version") { "min_shell_version = \"0.2.0\"".to_string() } else { l.to_string() });
    }
    out.join("\n")
}

fn manifest_text(id: &str) -> String {
    fs::read_to_string(root(id).join("theme.toml")).unwrap()
}

fn def(id: &str) -> Def {
    let text = manifest_text(id);
    Def::parse(&if contract() { text } else { compat(&text) }).unwrap_or_else(|e| panic!("{id}: {e}"))
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
fn section(id: &str, name: &str) -> Value {
    let doc = manifest_text(id).parse::<toml_edit::DocumentMut>().unwrap();
    doc.get(name).map(to_json).unwrap_or(Value::Null)
}

type C = (u8, u8, u8);

/// KColorUtils::mix as used by Breeze's `Helper::separatorColor` (linear in sRGB, rounded).
fn mix(a: C, b: C, t: f32) -> C {
    let f = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

fn rgb(c: C) -> Rgb {
    Rgb(c.0, c.1, c.2)
}

/// One upstream colour scheme reduced to the entries the manifests are derived from.
struct Scheme {
    window: C,
    window_alt: C,
    button: C,
    tooltip: C,
    selection: C,
    selection_fg: C,
    fg: C,
    inactive_fg: C,
    wm_active: C,
    wm_inactive: C,
    negative: C,
    positive: C,
    neutral: C,
}

// breeze.git 75561b7f32f3121603a0f2c8ea929b20a0d6c311 colors/BreezeLight.colors, BreezeDark.colors
const BREEZE_LIGHT: Scheme = Scheme {
    window: (239, 240, 241),
    window_alt: (227, 229, 231),
    button: (252, 252, 252),
    tooltip: (247, 247, 247),
    selection: (61, 174, 233),
    selection_fg: (255, 255, 255),
    fg: (35, 38, 41),
    inactive_fg: (112, 125, 138),
    wm_active: (227, 229, 231),
    wm_inactive: (239, 240, 241),
    negative: (218, 68, 83),
    positive: (39, 174, 96),
    neutral: (246, 116, 0),
};
const BREEZE_DARK: Scheme = Scheme {
    window: (32, 35, 38),
    window_alt: (41, 44, 48),
    button: (41, 44, 48),
    tooltip: (41, 44, 48),
    selection: (61, 174, 233),
    selection_fg: (252, 252, 252),
    fg: (252, 252, 252),
    inactive_fg: (161, 169, 177),
    wm_active: (39, 44, 49),
    wm_inactive: (32, 36, 40),
    negative: (218, 68, 83),
    positive: (39, 174, 96),
    neutral: (246, 116, 0),
};
// Layan-kde a0b6a4956022276aee33309b2e07d1d0ef3db30c color-schemes/Layan.colors, LayanLight.colors
// (LayanLight inactive foreground is 51,51,51 at alpha 150, flattened on Window)
const LAYAN_DARK: Scheme = Scheme {
    window: (49, 49, 58),
    window_alt: (54, 56, 62),
    button: (68, 69, 75),
    tooltip: (33, 35, 41),
    selection: (86, 87, 245),
    selection_fg: (255, 255, 255),
    fg: (234, 234, 234),
    inactive_fg: (153, 153, 153),
    wm_active: (54, 56, 62),
    wm_inactive: (62, 65, 71),
    negative: (191, 3, 3),
    positive: (0, 110, 40),
    neutral: (176, 128, 0),
};
const LAYAN_LIGHT: Scheme = Scheme {
    window: (245, 245, 245),
    window_alt: (245, 245, 245),
    button: (252, 252, 252),
    tooltip: (33, 35, 41),
    selection: (86, 87, 245),
    selection_fg: (255, 255, 255),
    fg: (51, 51, 51),
    inactive_fg: (131, 131, 131),
    wm_active: (238, 238, 238),
    wm_inactive: (240, 240, 240),
    negative: (191, 3, 3),
    positive: (0, 110, 40),
    neutral: (176, 128, 0),
};

#[test]
fn colour_tokens_are_recomputed_from_the_pinned_upstream_schemes() {
    for (id, dark, s) in [("plasma-breeze", true, &BREEZE_DARK), ("plasma-breeze", false, &BREEZE_LIGHT), ("plasma-layan", true, &LAYAN_DARK), ("plasma-layan", false, &LAYAN_LIGHT)] {
        let t = def(id).resolve(id, dark, &Selection::default());
        let what = format!("{id} dark={dark}");
        assert_eq!(t.background, rgb(s.window), "{what} background");
        assert_eq!(t.surface, rgb(s.button), "{what} surface");
        assert_eq!(t.overlay, rgb(s.tooltip), "{what} overlay");
        assert_eq!(t.accent, rgb(s.selection), "{what} accent");
        assert_eq!(t.accent_text, rgb(s.selection_fg), "{what} accent_text");
        assert_eq!(t.text, rgb(s.fg), "{what} text");
        assert_eq!(t.bar_text, rgb(s.fg), "{what} bar_text");
        assert_eq!(t.text_secondary, rgb(s.inactive_fg), "{what} text_secondary");
        assert_eq!(t.titlebar, rgb(s.wm_active), "{what} titlebar");
        assert_eq!(t.titlebar_inactive, rgb(s.wm_inactive), "{what} titlebar_inactive");
        assert_eq!((t.danger, t.ok, t.warn), (rgb(s.negative), rgb(s.positive), rgb(s.neutral)), "{what} status colours");
        // panel, popup and dock surfaces are the Window colour: stock Plasma panels are not tinted
        assert_eq!((t.bar, t.popup, t.dock), (rgb(s.window), rgb(s.window), rgb(s.window)), "{what} panel surfaces");
        // derived exactly as documented in the manifests
        assert_eq!(t.border, rgb(mix(s.window, s.fg, 0.2)), "{what} border = Breeze separatorColor at frameContrast 0.2");
        assert_eq!(t.text_faint, rgb(mix(s.inactive_fg, s.window, 0.35)), "{what} text_faint");
        if id == "plasma-breeze" {
            assert_eq!(t.sidebar, rgb(s.window_alt), "{what} sidebar");
        }
    }
}

#[test]
fn readable_where_upstream_allows_and_known_low_contrast_is_explicit() {
    for id in PORTS {
        for dark in [true, false] {
            let t = def(id).resolve(id, dark, &Selection::default());
            let what = format!("{id} dark={dark}");
            for (name, bg) in [("background", t.background), ("surface", t.surface), ("popup", t.popup), ("sidebar", t.sidebar)] {
                assert!(contrast(t.text, bg) >= 4.5, "{what}: text on {name}");
            }
            assert!(contrast(t.bar_text, t.bar) >= 4.5, "{what}: bar text");
            assert_eq!(t.background.luminance() > 0.3, !dark, "{what}: variant brightness");
            // Breeze's own white-on-#3daee9 selection pair is 2.5:1. It is kept faithfully; Layan's indigo pair is not low.
            let floor = if *id == "plasma-breeze" { 2.4 } else { 4.5 };
            assert!(contrast(t.accent_text, t.accent) >= floor, "{what}: accent text {:.2}", contrast(t.accent_text, t.accent));
        }
    }
}

#[test]
fn packages_are_valid_reproducible_and_carry_attribution_but_no_hardcoded_art() {
    for id in PORTS {
        let dir = root(id);
        let entries = package::read_dir(&dir, &Limits::default()).unwrap();
        let report = package::check(&entries, &Limits::default());
        if contract() {
            assert!(report.ok(), "{id}: {:?}", report.errors());
            assert_eq!(report.id.as_deref(), Some(*id));
            let (a, _) = package::pack_dir(&dir, &Limits::default()).unwrap();
            assert_eq!(a, package::pack_dir(&dir, &Limits::default()).unwrap().0, "{id}: reproducible pack");
        } else {
            // An engine without the contract may reject only the new keys; nothing else may be wrong with the package.
            let errors = report.errors().iter().map(|e| format!("{e:?}")).collect::<Vec<_>>();
            assert!(errors.iter().all(|e| e.contains("applets") || e.contains("branding") || e.contains("min_shell_version")), "{id}: {errors:?}");
        }
        // previews are generated from the package, wallpapers come from the curated catalogue: no art, widgets or shots
        for e in &entries {
            assert!(!e.path.starts_with("wallpapers/") && !e.path.starts_with("screenshots/") && !e.path.starts_with("fonts/"), "{id}: unexpected asset {}", e.path);
            assert!(!e.path.starts_with("widgets/") || ["widgets/plasma-analog-clock.toml", "widgets/plasma-digital-clock.toml"].contains(&e.path.as_str()), "{id}: unexpected widget {}", e.path);
        }
        let text = fs::read_to_string(dir.join("theme.toml")).unwrap();
        for forbidden in ["\nscreenshots", "[wallpaper", "extends"] {
            assert!(!text.contains(forbidden), "{id}: {forbidden}");
        }
        assert!(text.contains("min_shell_version = \"0.3.0\""), "{id}: [branding] and [bar] applets need the 0.3.0 framework");
        for file in ["README.md", "LICENSE.txt"] {
            assert!(dir.join(file).is_file(), "{id}: {file}");
        }
        assert!(section(id, "desktop").is_null(), "{id}: [desktop] widgets = none cannot coexist with saved-off widgets");
        let readme = fs::read_to_string(dir.join("README.md")).unwrap();
        let sha = if *id == "plasma-breeze" { "75561b7f32f3121603a0f2c8ea929b20a0d6c311" } else { "a0b6a4956022276aee33309b2e07d1d0ef3db30c" };
        assert!(readme.contains(sha), "{id}: README pins the upstream commit");
        assert!(text.contains(&sha[..7]), "{id}: manifest comment pins the upstream commit");
    }
    assert!(root("plasma-layan").join("AUTHORS.txt").is_file());
    let license = |id: &str| fs::read_to_string(root(id).join("LICENSE.txt")).unwrap();
    assert!(license("plasma-layan").contains("GNU GENERAL PUBLIC LICENSE") && license("plasma-layan").contains("Version 3"));
    assert!(license("plasma-breeze").contains("GNU LIBRARY GENERAL PUBLIC LICENSE") || license("plasma-breeze").contains("GNU LESSER GENERAL PUBLIC LICENSE"));
}

const EDGES: [&str; 4] = ["top", "bottom", "left", "right"];

/// The applet ids of the committed universal-panel registry (`docs/universal-panels.md` at shared commit 71e3434).
const REGISTRY: &[&str] = &[
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

/// Stock applet sets, derived from the pinned Plasma sources (docs/plasma-theme-ports.md, applet inventory).
const BREEZE_APPLETS: &[&str] = &["launcher", "workspaces", "tasks", "tray", "volume", "bluetooth", "brightness", "network", "battery", "notifications", "clock", "show_desktop"];
const LAYAN_APPLETS: &[&str] = &["launcher", "window_title", "tray", "network", "volume", "clipboard", "notifications", "clock", "search"];

#[test]
fn panel_declarations_use_the_shared_contract_and_match_the_researched_layouts() {
    // Breeze: one Plasma panel. The bar is "auto" (hidden under the taskbar) and the taskbar carries the stock applets.
    let (bar, dock, brand) = (section("plasma-breeze", "bar"), section("plasma-breeze", "dock"), section("plasma-breeze", "branding"));
    assert_eq!((&bar["enabled"], &bar["position"]), (&Value::from("auto"), &Value::from("bottom")));
    assert_eq!(bar["applets"], Value::from(BREEZE_APPLETS.to_vec()));
    assert_eq!((&dock["style"], &dock["position"], &dock["align"], &dock["enabled"]), (&Value::from("windows"), &Value::from("bottom"), &Value::from("left"), &Value::from(true)));
    // Layan: 32 px top bar (1.78 grid units in its layout script) plus the preview's companion dock
    let (lbar, ldock, lbrand) = (section("plasma-layan", "bar"), section("plasma-layan", "dock"), section("plasma-layan", "branding"));
    assert_eq!((&lbar["style"], &lbar["position"], &lbar["height"], &lbar["enabled"]), (&Value::from("flat"), &Value::from("top"), &Value::from(32), &Value::from(true)));
    assert_eq!(lbar["applets"], Value::from(LAYAN_APPLETS.to_vec()));
    assert_eq!((&ldock["style"], &ldock["position"]), (&Value::from("mac"), &Value::from("bottom")));
    // every declared applet is a registry key; foreign defaults (search box, layout, weather, Control Center button) are not declared
    for (set, foreign) in [(BREEZE_APPLETS, ["search", "layout", "weather", "control_center", "dashboard"]), (LAYAN_APPLETS, ["workspaces", "layout", "weather", "control_center", "tasks"])] {
        assert!(set.iter().all(|a| REGISTRY.contains(a)));
        assert!(!set.iter().any(|a| foreign.contains(a)), "{set:?}");
    }
    // branding: Kickoff's icon by icon-theme name, tinted like a symbolic icon, no foreign Windows/Apple/Finder defaults
    for b in [&brand, &lbrand] {
        assert_eq!((&b["logo"], &b["logo_tint"], &b["desktop_label"]), (&Value::from("icon:start-here-kde-symbolic"), &Value::from(true), &Value::from("Desktop")));
        assert!(b.get("launcher_label").is_none(), "Kickoff's menuLabel default is empty");
    }
    // Kickoff is the launcher: the anchored "kde" style opens from the applet towards the screen on every edge; the user's Settings choice still wins
    for id in PORTS {
        assert_eq!(section(id, "launcher")["style"], "kde", "{id}");
    }
    assert_eq!(section("plasma-breeze", "decoration")["style"], "breeze");
    assert_eq!(section("plasma-breeze", "icons")["theme"], "breeze");
    assert_eq!(section("plasma-layan", "icons")["theme"], "Tela");
    // translucency from the SVGs: Breeze panel .85, Layan panel .6 / dialog .8
    let t = def("plasma-breeze").resolve("plasma-breeze", true, &Selection::default());
    assert!((t.dock_alpha - 0.85).abs() < 1e-6 && (t.popup_alpha - 0.85).abs() < 1e-6);
    let t = def("plasma-layan").resolve("plasma-layan", true, &Selection::default());
    assert!((t.bar_alpha - 0.6).abs() < 1e-6 && (t.popup_alpha - 0.8).abs() < 1e-6);
}

/// The user's tiling policy is global: a theme must not carry layouts, master fraction, master count or smart gaps, so applying
/// or switching a theme can never overwrite custom layout proportions. Visual gaps and borders are the theme's.
#[test]
fn tiling_policy_is_global_and_never_set_by_the_packages() {
    for id in PORTS {
        let rdwm = section(id, "rdwm");
        for key in ["layouts", "mfact", "nmaster", "smart_gaps"] {
            assert!(rdwm.get(key).is_none(), "{id}: [rdwm] {key} would overwrite the user's tiling policy");
        }
        assert_eq!((&rdwm["gap_inner"], &rdwm["gap_outer"], &rdwm["border_px"]), (&Value::from(8), &Value::from(4), &Value::from(1)), "{id}: KWin tile padding 4");
        let text = fs::read_to_string(root(id).join("theme.toml")).unwrap();
        assert!(text.contains("global behaviour") || text.contains("global behavior"), "{id}: manifest explains why the policy is omitted");
    }
}

/// Applies a private copy of a package whose [bar] and [dock] edges were rewritten to `edge` (control_center.position keeps its own
/// vocabulary). On an engine without the contract the copy also loses the keys that engine rejects, so the old plan still runs.
fn edge_copy(tag: &str, id: &str, edge: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("plasma-edge-{}-{tag}-{id}-{edge}", std::process::id()));
    let pkg = dir.join("themes").join(id);
    fs::create_dir_all(&pkg).unwrap();
    let text = if contract() { manifest_text(id) } else { compat(&manifest_text(id)) };
    let mut current = String::new();
    let mut out = vec![];
    for l in text.lines() {
        if l.starts_with('[') {
            current = l.to_string();
        }
        if l.starts_with("position = \"") && matches!(current.as_str(), "[bar]" | "[dock]") {
            out.push(format!("position = \"{edge}\""));
        } else {
            out.push(l.to_string());
        }
    }
    fs::write(pkg.join("theme.toml"), out.join("\n")).unwrap();
    fs::create_dir_all(dir.join("shell")).unwrap();
    dir
}

fn plan_for_edge(tag: &str, id: &str, edge: &str, user_rdwm: &str) -> (apply::Plan, Paths, PathBuf) {
    let dir = edge_copy(tag, id, edge);
    let paths = Paths { shell_dir: dir.join("shell"), rdwm_config: dir.join("config.toml"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: dir.join("no.sock") };
    fs::write(&paths.rdwm_config, user_rdwm).unwrap();
    let src = Source { shell_json: paths.shell_json(), themes_dir: dir.join("themes") };
    let plan = apply::plan(&src, &paths, id, &Selection::default()).unwrap_or_else(|e| panic!("{id} {edge}: {e}"));
    (plan, paths, dir)
}

/// Native preview of a private edge copy with the production painter (needs no display).
fn render_png(id: &str, edge: &str, dir: &Path) -> Vec<u8> {
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let png = dir.join("out.png");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_decthemectl"))
        .args(["theme", "render", id, "--mode", "dark", "--png"])
        .arg(&png)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("DECEBAL_THEMES", dir.join("themes"))
        .output()
        .unwrap();
    assert!(status.status.success(), "{id} {edge}: {}", String::from_utf8_lossy(&status.stderr));
    fs::read(png).unwrap()
}

fn render_hash(id: &str, edge: &str, dir: &Path) -> String {
    decebal::theme::engine::sha256::digest_hex(&render_png(id, edge, dir))
}

const USER_RDWM: &str = "[general]\nmfact = 0.73\nnmaster = 2\nsmart_gaps = true\nlayouts = [\"monocle\", \"tile\"]\ngap_inner = 1\n";

/// Applying either package keeps the user's tiling proportions byte for byte and only sets the visual gaps and borders.
#[test]
fn apply_plans_keep_custom_tiling_proportions_wallpaper_and_the_declared_panel() {
    for id in PORTS {
        let (plan, paths, dir) =
            plan_for_edge("apply", id, if *id == "plasma-breeze" { "bottom" } else { "top" }, &format!("{USER_RDWM}[wallpaper]\nimage = \"/private/photo.jpg\"\nmode = \"center\"\n"));
        let config = &plan.writes.iter().find(|w| w.path == paths.rdwm_config).unwrap().new;
        assert!(config.contains("/private/photo.jpg"), "{id}: wallpaper preserved");
        for kept in ["mfact = 0.73", "nmaster = 2", "smart_gaps = true", "layouts = [\"monocle\", \"tile\"]"] {
            assert!(config.contains(kept), "{id}: lost the user's `{kept}`:\n{config}");
        }
        assert!(config.contains("gap_inner = 8") && config.contains("gap_outer = 4"), "{id}: visual gaps applied");
        let shell: Value = serde_json::from_str(&plan.writes.iter().find(|w| w.path == paths.shell_json()).unwrap().new).unwrap();
        assert_eq!(shell["theme"]["name"], *id);
        assert!(shell.get("theme_wallpaper").is_none_or(|w| w.is_null()), "{id}: package owns no wallpaper");
        // the theme's panel edge and applets are defaults read from the package, never copied over the user's choices
        assert!(shell["bar"].get("edge").is_none() && shell["bar"].get("applets").is_none(), "{id}: theme defaults must not become user choices");
        let _ = fs::remove_dir_all(dir);
    }
}

/// While an engine cannot draw a left or right panel, the real apply plan must say so and not write it; once it can, the same test
/// asserts every edge renders (four distinct native previews) and nothing is dropped.
#[test]
fn panel_edges_follow_the_engine_capability() {
    let wired = contract() && !matches!(schema::field("dock.position").expect("dock.position").status, Status::Pending(_));
    for id in PORTS {
        let mut hashes = std::collections::BTreeSet::new();
        for edge in EDGES {
            let (plan, _paths, dir) = plan_for_edge("cap", id, edge, "");
            let dropped = plan.notes.iter().any(|n| n.contains("dock.position") && (n.contains("only top and bottom") || n.contains("left out")));
            if matches!(edge, "top" | "bottom") || wired {
                assert!(!dropped, "{id} {edge}: {:?}", plan.notes);
            } else {
                assert!(dropped, "{id} {edge}: an unsupported edge must be reported: {:?}", plan.notes);
            }
            if wired {
                hashes.insert(render_hash(id, edge, &dir));
            }
            let _ = fs::remove_dir_all(dir);
        }
        if wired {
            assert_eq!(hashes.len(), 4, "{id}: four edges must render four distinct native previews");
        }
    }
}

/// Plasma's application menu shows the focused application's own menus over DBus. Decebal has no exporter service, so a menu bar that
/// draws fixed `File Edit View` titles would be a fake: `app_menu` stays out of the default set (it would also repeat the logo beside
/// `launcher`), the focused window's real title stands in, and the doc names the missing service and the API it needs.
#[test]
fn layan_ships_no_fake_global_menu_and_a_single_logo_source() {
    let applets = section("plasma-layan", "bar")["applets"].clone();
    let list: Vec<&str> = applets.as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(!list.contains(&"app_menu"), "app_menu would draw fixed placeholder menus and a second logo");
    assert!(list.contains(&"launcher") && list.contains(&"window_title"));
    assert!(section("plasma-breeze", "bar")["applets"].as_array().unwrap().iter().all(|v| v != "app_menu"));
    let manifest = manifest_text("plasma-layan");
    assert!(manifest.contains("app_menu") && manifest.contains("NOT in the"), "the manifest explains the omission");
    let doc = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/plasma-theme-ports.md")).unwrap();
    for must in ["AppMenu.Registrar", "GetMenuForWindow", "exporter"] {
        assert!(doc.contains(must), "doc must state the missing service API: {must}");
    }
}

/// Fraction of a straight line (distance `d` from the screen edge) whose pixels differ from the desktop colour by a hairline-sized
/// amount, counted along the edge between `margin` and `len - margin`. Breeze's panel fill equals the desktop background, so the
/// panel's outline is what shows a strip in a preview.
fn line_coverage(img: &image::RgbaImage, edge: &str, d: u32, desktop_h: u32, margin: u32) -> f32 {
    let (w, h) = (img.width(), desktop_h);
    let bg = img.get_pixel(w / 2, h / 2).0;
    let differs = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        (0..3).map(|i| (i32::from(p[i]) - i32::from(bg[i])).abs()).max().unwrap() >= 12
    };
    let len = if matches!(edge, "top" | "bottom") { w } else { h };
    let at = |i: u32| match edge {
        "top" => (i, d),
        "bottom" => (i, h - 1 - d),
        "left" => (d, i),
        _ => (w - 1 - d, i),
    };
    let n = len - 2 * margin;
    (margin..len - margin)
        .filter(|i| {
            let (x, y) = at(*i);
            differs(x, y)
        })
        .count() as f32
        / n as f32
}

/// R4, now a normal gate: the Breeze taskbar is a full-length, Plasma-sized, floating strip on all four edges. The package declares
/// `[taskbar] thickness = 44`, `float_gap = 8`: the strip's two outlines sit 8 px and 8 + 43 px from the screen edge and run the whole
/// edge minus the gap at both ends; nothing is drawn inside the gap or beyond the strip; Kickoff's logo is the first item.
#[test]
fn breeze_taskbar_is_a_full_length_floating_native_strip_on_every_edge() {
    assert!(contract(), "needs the shared panel contract (min_shell_version 0.3.0 engine)");
    let style = section("plasma-breeze", "taskbar");
    let (thick, gap) = (style["thickness"].as_u64().unwrap() as u32, style["float_gap"].as_u64().unwrap() as u32);
    assert_eq!((thick, gap), (44, 8));
    for edge in EDGES {
        let dir = edge_copy("strip", "plasma-breeze", edge);
        let png = render_png("plasma-breeze", edge, &dir);
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (1920, 1200), "preview geometry");
        let desktop_h = 1080;
        let margin = gap + 8; // the rounded corners at both ends
        let near = line_coverage(&img, edge, gap, desktop_h, margin);
        let far = line_coverage(&img, edge, gap + thick - 1, desktop_h, margin);
        assert!(near >= 0.9 && far >= 0.9, "{edge}: the strip's outlines cover {:.0}% and {:.0}% of the edge (need at least 90%)", near * 100.0, far * 100.0);
        // Plasma's thickness: nothing beyond the strip and nothing inside the floating gap
        for d in [gap - 4, gap + thick + 2, gap + thick + 12] {
            let stray = line_coverage(&img, edge, d, desktop_h, margin);
            assert!(stray <= 0.05, "{edge}: {:.0}% of the line {d} px from the edge is painted (the strip is {thick} px thick after a {gap} px gap)", stray * 100.0);
        }
        // Kickoff's logo is the first item: blue-ish (accent) pixels near the start of the strip
        let (xr, yr) = match edge {
            "top" => (0..160u32, gap..gap + thick),
            "bottom" => (0..160u32, desktop_h - gap - thick..desktop_h - gap),
            "left" => (gap..gap + thick, 0..160u32),
            _ => (img.width() - gap - thick..img.width() - gap, 0..160u32),
        };
        let ink = xr
            .flat_map(|x| yr.clone().map(move |y| (x, y)))
            .filter(|(x, y)| {
                let p = img.get_pixel(*x, *y).0;
                i32::from(p[2]) > i32::from(p[0]) + 30
            })
            .count();
        assert!(ink >= 8, "{edge}: the Kickoff logo is not at the start of the strip ({ink} accent pixels)");
        let _ = fs::remove_dir_all(dir);
    }
}

fn hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn safe_rel(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.contains('\\') && p.split('/').all(|x| !x.is_empty() && x != "." && x != "..") && !p.chars().any(char::is_control)
}

fn only_keys(v: &Value, allowed: &[&str], what: &str) {
    for k in v.as_object().unwrap_or_else(|| panic!("{what} is a table")).keys() {
        assert!(allowed.contains(&k.as_str()), "{what}: unknown key `{k}` (the contract rejects unknown keys)");
    }
}

/// The Plasma catalogue follows the rules of docs/desktop-asset-packs.md (asset-owner commit 76b9285) and agrees with the receipts
/// produced by tools/plasma-asset-receipt.py. The contract's own validator was not committed, so these rules are checked here.
#[test]
fn asset_catalogue_follows_the_documented_pack_contract_and_matches_its_receipts() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = fs::read_to_string(base.join("assets/desktop-ports/plasma/asset-packs.toml")).unwrap().parse::<toml_edit::DocumentMut>().unwrap();
    let cat = to_json(doc.as_item());
    only_keys(&cat, &["format", "family", "source", "pack", "recommend"], "catalogue");
    assert_eq!((&cat["format"], &cat["family"]), (&Value::from(1), &Value::from("plasma")));
    let mut sources = std::collections::BTreeMap::new();
    for s in cat["source"].as_array().unwrap() {
        only_keys(s, &["id", "repo", "revision", "commit", "license", "license_files", "archive_url", "archive_sha256", "strip_components"], "source");
        let id = s["id"].as_str().unwrap();
        assert!(sources.insert(id, s).is_none(), "{id}: duplicate source");
        assert!(s["repo"].as_str().unwrap().starts_with("https://") && s["archive_url"].as_str().unwrap().starts_with("https://"));
        assert!(hex(s["commit"].as_str().unwrap(), 40) && hex(s["archive_sha256"].as_str().unwrap(), 64), "{id}: pins");
        assert!(!s["license"].as_str().unwrap().is_empty() && !s["license_files"].as_array().unwrap().is_empty());
        assert!(s["license_files"].as_array().unwrap().iter().all(|f| safe_rel(f.as_str().unwrap())));
    }
    let receipts: Value = serde_json::from_str(&fs::read_to_string(base.join("docs/plasma-theme-ports/asset-receipts.json")).unwrap()).unwrap();
    let mut ids = std::collections::BTreeSet::new();
    for p in cat["pack"].as_array().unwrap() {
        only_keys(p, &["id", "name", "provides", "source", "path", "tree", "install_name", "license", "license_files", "link_sources", "description"], "pack");
        let id = p["id"].as_str().unwrap();
        assert!(ids.insert(id), "{id}: duplicate pack id");
        assert!(id.starts_with("plasma-"), "{id}: pack ids are prefixed with the family");
        assert!(sources.contains_key(p["source"].as_str().unwrap()));
        assert!(safe_rel(p["path"].as_str().unwrap()) && hex(p["tree"].as_str().unwrap(), 40));
        let install = p["install_name"].as_str().unwrap();
        assert!(install.len() <= 64 && install.chars().next().unwrap().is_ascii_alphanumeric() && install.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)), "{id}: install_name");
        assert!(p["provides"].as_array().unwrap().iter().all(|k| k == "icons" || k == "cursors"));
        // receipts: the tree computed by git from the downloaded archive equals the pin, the archive hash matches, the commit matches
        let r = &receipts["cursor_packs"]["packs"][id];
        assert_eq!(r["tree"]["computed"], p["tree"], "{id}: receipt tree");
        assert_eq!(r["tree"]["matches"], true);
        assert_eq!(r["install_name"], install);
        assert!(r["content_hash"].as_str().unwrap().starts_with("sha256:") && !r["core_pointer"].as_array().unwrap().is_empty(), "{id}: core pointer and content hash");
        let src = &receipts["cursor_packs"]["sources"][p["source"].as_str().unwrap()];
        assert_eq!((&src["archive_sha256_matches_catalogue"], &src["pax_commit_matches_catalogue"]), (&Value::from(true), &Value::from(true)));
        assert_eq!(src["archive_sha256"], sources[p["source"].as_str().unwrap()]["archive_sha256"]);
        // optional local re-check against the pinned clone (git read-only): skipped when the clone is not on this machine
        let refs = std::env::var_os("PLASMA_REFS").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/decebal-plasma-refs")));
        let clone = refs.and_then(|d| {
            let name = match p["source"].as_str().unwrap() {
                "kde-breeze" => "breeze",
                _ => "layan-cursors",
            };
            d.join(name).join(".git").is_dir().then(|| d.join(name))
        });
        if let Some(clone) = clone {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&clone)
                .args(["rev-parse", &format!("{}:{}", sources[p["source"].as_str().unwrap()]["commit"].as_str().unwrap(), p["path"].as_str().unwrap())])
                .output()
                .unwrap();
            if out.status.success() {
                assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), p["tree"].as_str().unwrap(), "{id}: tree in the pinned clone");
            }
        }
    }
    for r in cat["recommend"].as_array().unwrap() {
        only_keys(r, &["theme", "icons", "cursors"], "recommend");
        assert!(PORTS.contains(&r["theme"].as_str().unwrap()), "recommended theme exists in this port");
        let c = r["cursors"].as_str().unwrap();
        assert!(cat["pack"].as_array().unwrap().iter().any(|p| p["id"] == c && p["provides"].as_array().unwrap().iter().any(|k| k == "cursors")));
    }
    // icon themes are receipts of a native build, not contract packs (they need a generation step)
    assert!(cat["pack"].as_array().unwrap().iter().all(|p| !p["provides"].as_array().unwrap().iter().any(|k| k == "icons")));
    let icons = &receipts["icon_themes_built_natively"]["themes"];
    assert!(
        icons["breeze"]["compared_with_installed_package"]["identical_to_installed_package"].as_u64().unwrap() >= 5600 && icons["breeze-dark"]["content_hash"].as_str().unwrap().starts_with("sha256:")
    );
}

// ---- numeric references from the pinned Plasma sources (docs/plasma-theme-ports/engine-requests.md) ----
const GRID_UNIT: u32 = 18; // kirigami units.cpp gridUnit default
const SMALL_SPACING: u32 = 4;
const CORNER_RADIUS: u32 = 5; // kirigami cornerRadius, breezemetrics.h Frame_FrameRadius
const FLOATING_GAP_PX: u32 = 8; // desktoppackage/contents/views/Panel.qml fallback when the Plasma theme has no `floating-*` prefix
const PANEL_FILL_ALPHA: f32 = 0.85; // libplasma breeze translucent widgets/panel-background.svg
const PAGER_SPACING_PX: f32 = 1.0; // applets/pager/qml/main.qml pagerItemGrid spacing

/// layout.js: panel.height = 2 * Math.ceil(gridUnit * 2.5 / 2)
fn plasma_panel_thickness(grid_unit: u32) -> u32 {
    2 * ((grid_unit as f32 * 2.5 / 2.0).ceil() as u32)
}

/// Kirigami IconSizes.roundedIconSize
fn rounded_icon_size(px: u32) -> u32 {
    match px {
        0..=15 => px,
        16..=21 => 16,
        22..=31 => 22,
        32..=47 => 32,
        48..=63 => 48,
        _ => px,
    }
}

/// Panel.qml: spacingAtMinSize = floor(max(1, thickness - iconSizes.smallMedium(22)) / 2); padding = round(min(margin + smallSpacing, spacingAtMinSize))
fn plasma_panel_padding(thickness: u32, svg_margin: u32) -> u32 {
    let at_min = (thickness.saturating_sub(22).max(1)) / 2;
    (svg_margin + SMALL_SPACING).min(at_min)
}

/// applets/pager/qml/main.qml aspectRatio and the horizontal minimum width (`Math.floor(height * aspectRatio)`)
fn pager_aspect(cell_w: f32, cell_h: f32, cols: f32, rows: f32) -> f32 {
    (cell_w * cols + (cols * PAGER_SPACING_PX - PAGER_SPACING_PX)) / (cell_h * rows + (rows * PAGER_SPACING_PX - PAGER_SPACING_PX))
}

fn pinned_source(rel: &str) -> Option<String> {
    let refs = std::env::var_os("PLASMA_REFS").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/decebal-plasma-refs")))?;
    fs::read_to_string(refs.join(rel)).ok()
}

/// Source-backed Plasma numbers as executable references: they pin the formulas the engine requests rely on, check the packages'
/// declared values against them, and, when the pinned clones are on this machine, re-read the numbers from the actual source files.
#[test]
fn plasma_numeric_references_hold_for_the_packages_and_the_pinned_sources() {
    // panel thickness: 46 at gridUnit 18 (the Breeze preview image shows 44, i.e. gridUnit 17)
    assert_eq!((plasma_panel_thickness(18), plasma_panel_thickness(17)), (46, 44));
    // icon snapping (kirigami)
    for (px, want) in [(15, 15), (16, 16), (21, 16), (22, 22), (31, 22), (32, 32), (47, 32), (48, 48), (63, 48), (64, 64)] {
        assert_eq!(rounded_icon_size(px), want, "roundedIconSize({px})");
    }
    // the task icon fills the panel minus its two paddings: 44 - 2*8 = 28 px, which is also what the upstream preview image shows
    // (about 28 px icons in a 44 px panel); Kirigami's snapped size (22 here) only picks the source image
    let icon = section("plasma-breeze", "dock")["size"].as_u64().unwrap() as u32;
    assert_eq!(icon, 44 - 2 * plasma_panel_padding(44, 4));
    assert_eq!(rounded_icon_size(icon), 22);
    // inner padding at the Breeze numbers: svg margin 4 + smallSpacing 4 = 8, capped by (44-22)/2 = 11
    assert_eq!((plasma_panel_padding(44, 4), plasma_panel_padding(30, 4), plasma_panel_padding(24, 4)), (8, 4, 1));
    // pager: 4 desktops in one row on a 16:9 screen (cell 192x108 px, spacing 1): aspect 771/108 = 7.1389, a 30 px high applet is 214 px wide
    let aspect = pager_aspect(192.0, 108.0, 4.0, 1.0);
    assert!((aspect - 771.0 / 108.0).abs() < 1e-4 && (30.0 * aspect).floor() as u32 == 214);
    // vertical panel: minimumHeight = floor(width / aspect)
    assert_eq!((44.0 / aspect).floor() as u32, 6);
    // 2 rows x 2 columns of the same cells: nearly square
    assert!((pager_aspect(192.0, 108.0, 2.0, 2.0) - 385.0 / 217.0).abs() < 1e-4);
    // what the packages declare
    let breeze = def("plasma-breeze").resolve("plasma-breeze", true, &Selection::default());
    let layan = def("plasma-layan").resolve("plasma-layan", true, &Selection::default());
    assert_eq!((breeze.radius as u32, (breeze.dock_alpha - PANEL_FILL_ALPHA).abs() < 1e-6), (CORNER_RADIUS, true));
    assert_eq!(section("plasma-breeze", "decoration")["radius"], CORNER_RADIUS);
    assert!((layan.bar_alpha - 0.6).abs() < 1e-6 && (layan.popup_alpha - 0.8).abs() < 1e-6);
    // saved-off desktop widgets keep Plasma's own sizes: analog clock gridUnit*15 square, digital clock gridUnit*10 x *4
    for id in PORTS {
        for (file, w, h) in [("plasma-analog-clock", 15 * GRID_UNIT, 15 * GRID_UNIT), ("plasma-digital-clock", 10 * GRID_UNIT, 4 * GRID_UNIT)] {
            let text = fs::read_to_string(root(id).join("widgets").join(format!("{file}.toml"))).unwrap().parse::<toml_edit::DocumentMut>().unwrap();
            let size = to_json(&text["size"]);
            assert_eq!((size[0].as_u64().unwrap() as u32, size[1].as_u64().unwrap() as u32), (w, h), "{id}/{file}");
        }
        let widgets = section(id, "widgets");
        let list = widgets.as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|w| w["enabled"] == false && w["place"] == "desktop"), "{id}: Plasma places no desktop widgets by default, so both are saved off");
    }
    // the engine's own formulas (src/panel/plasma_style.rs) agree with these independent references
    {
        use decebal::panel::plasma_style as ps;
        for (t, m) in [(24, 4), (30, 4), (44, 4), (46, 4), (60, 6)] {
            assert_eq!(ps::inner_padding(t, m), plasma_panel_padding(t, m));
            assert_eq!(ps::icon_size(t, m), t - 2 * plasma_panel_padding(t, m));
        }
        assert!((ps::pager_aspect(192.0, 108.0, 4.0, 1.0) - pager_aspect(192.0, 108.0, 4.0, 1.0)).abs() < 1e-6);
        assert_eq!((ps::SMALL_SPACING, ps::PAGER_SPACING), (SMALL_SPACING, PAGER_SPACING_PX));
        // the Breeze package's [taskbar] is exactly Plasma's numbers, parsed by the engine's own reader
        let style = ps::TaskbarStyle::from_extra(&def("plasma-breeze").extra);
        assert_eq!((style.thickness, style.icon, style.cell_width, style.gap, style.inset, style.float_gap), (Some(44.0), Some(28.0), Some(50.0), Some(0.0), Some(0.0), 8.0));
        assert_eq!((style.thickness, style.icon, style.float_gap), (Some(plasma_panel_thickness(17) as f32), Some(ps::icon_size(44, ps::SVG_MARGIN) as f32), FLOATING_GAP_PX as f32));
        assert_eq!((style.indicator, style.desktop, style.workspaces), (ps::Indicator::Frame, ps::DesktopForm::Icon, ps::WorkspaceForm::Cells));
        // Layan keeps its own bar and dock: no [taskbar], so every renderer value is the style's own
        assert!(ps::TaskbarStyle::from_extra(&def("plasma-layan").extra).is_default());
        // the indicator numbers of tasks.svg
        let a = ps::task_indicator(ps::TaskState::Active, decebal::panel::Edge::Bottom).unwrap();
        assert_eq!((a.fill_alpha, a.line_alpha, a.strong_alpha, a.line_px, a.strong_side), (0.45, 0.45, 0.90, 3.0, decebal::panel::Edge::Top));
    }
    // the floating gap has no manifest key; it is a documented engine request with this exact number
    let req = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/plasma-theme-ports/engine-requests.md")).unwrap();
    assert!(req.contains(&format!("{FLOATING_GAP_PX} px")) && req.contains("2*ceil(gridUnit*2.5/2)"));
    // re-read the numbers from the pinned sources when they are available
    if let Some(panel) = pinned_source("plasma-desktop/desktoppackage/contents/views/Panel.qml") {
        assert!(panel.contains("floatingPrefix ? floatingPanelSvg.fixedMargins.bottom : 8"), "Panel.qml floating gap fallback");
        assert!(
            panel.contains("spacingAtMinSize: Math.floor(Math.max(1, panel.thickness - Kirigami.Units.iconSizes.smallMedium)/2)")
                || panel.contains("spacingAtMinSize = Math.floor(Math.max(1, panel.thickness - Kirigami.Units.iconSizes.smallMedium)/2)")
                || panel.contains("Math.floor(Math.max(1, panel.thickness - Kirigami.Units.iconSizes.smallMedium)/2)")
        );
    }
    if let Some(layout) = pinned_source("plasma-desktop/layout-templates/org.kde.plasma.desktop.defaultPanel/contents/layout.js") {
        assert!(layout.contains("panel.height = 2 * Math.ceil(gridUnit * 2.5 / 2)"));
    }
    if let Some(units) = pinned_source("kirigami/src/platform/units.cpp") {
        assert!(units.contains("gridUnit(18)") && units.contains("smallSpacing(4)") && units.contains("cornerRadius(5)"));
    }
    if let Some(pager) = pinned_source("plasma-desktop/applets/pager/qml/main.qml") {
        assert!(pager.contains("spacing: 1") && pager.contains("(pagerItemGrid.effectiveColumns * pagerItemGrid.spacing) - pagerItemGrid.spacing"));
    }
    if let Some(svg) = pinned_source("libplasma/src/desktoptheme/breeze/translucent/widgets/panel-background.svg") {
        assert!(svg.contains("opacity:0.85;fill:currentColor"), "panel fill alpha .85");
    }
    if let Some(tasks) = pinned_source("libplasma/src/desktoptheme/breeze/widgets/tasks.svg") {
        // task indicator: focus frame .45 with a .90 line on the interior side; hover .34; minimised .08; attention .35 (engine-requests R1)
        for needle in ["id=\"focus-center\" class=\"ColorScheme-ButtonFocus\"", "opacity=\".45\"", "opacity=\".90\"", "opacity=\".34\"", "opacity=\".08\"", "opacity=\".35\""] {
            assert!(tasks.contains(needle), "tasks.svg lacks {needle}");
        }
    }
    // decoration metadata (docs/plasma-theme-ports/decoration-metadata.md)
    if let Some(kcfg) = pinned_source("breeze/kdecoration/breezesettingsdata.kcfg") {
        assert!(kcfg.contains("<default>AlignCenterFullWidth</default>") && kcfg.contains("<default>ShadowLarge</default>") && kcfg.contains("<default>255</default>"));
    }
    if let Some(h) = pinned_source("breeze/kdecoration/breeze.h") {
        assert!(h.contains("Frame_FrameRadius = 2.5") && h.contains("TitleBar_TopMargin = 2"));
    }
    if let Some(rc) = pinned_source("layan-kde/aurorae/themes/Layan/Layanrc") {
        assert!(rc.contains("ButtonHeight=16") && rc.contains("ButtonSpacing=12") && rc.contains("TitleHeight=15") && rc.contains("TitleAlignment=Center"));
    }
    if let Some(d) = pinned_source("layan-kde/plasma/look-and-feel/com.github.vinceliuice.Layan/contents/defaults") {
        assert!(d.contains("ButtonsOnRight=IAX") && d.contains("Layan-white-cursors"));
    }
    let deco = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/plasma-theme-ports/decoration-metadata.md")).unwrap();
    assert!(deco.contains("Frame_FrameRadius = 2.5") && deco.contains("ButtonsOnRight=IAX"));
    assert_eq!((section("plasma-layan", "decoration")["style"].as_str(), section("plasma-layan", "decoration")["title_align"].as_str()), (Some("adwaita"), Some("center")));
    if let Some(ts) = pinned_source("plasma-desktop/applets/taskmanager/qml/code/TaskTools.js") {
        for edge in ["\"west-\"", "\"north-\"", "\"east-\"", "\"south-\""] {
            assert!(ts.contains(edge), "TaskTools taskPrefix {edge}");
        }
    }
}

/// Human acceptance gate. Capability tests never fail because the framework improved, but the packages stay NOT READY until
/// native four-edge, all-applet and launcher-fidelity evidence is accepted; then this test is deleted with the wording.
#[test]
fn packages_stay_not_ready_until_acceptance_evidence_is_recorded() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = fs::read_to_string(base.join("docs/plasma-theme-ports.md")).unwrap();
    assert!(doc.contains("NOT READY") && doc.contains("branding-contract.md") && doc.contains("71e343413fdafcc6fde638d701b63c183512f1c8") && doc.contains("b28cca5fb2e81b84d2cfcd65e9fa978391041e50"));
    for id in PORTS {
        assert!(fs::read_to_string(root(id).join("README.md")).unwrap().contains("NOT READY"), "{id}");
    }
    let branding = fs::read_to_string(base.join("docs/plasma-theme-ports/branding-contract.md")).unwrap();
    for must in ["start-here-kde-symbolic", "org.kde.milou", "org.kde.plasma.appmenu", "neutral \"Desktop\"", "AppMenu.Registrar"] {
        assert!(branding.contains(must), "branding contract lacks {must}");
    }
}

#[test]
fn plasma_wallpaper_records_merge_into_the_shipped_catalogue_and_stay_honest() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut catalogue: Value = serde_json::from_str(&fs::read_to_string(base.join("data/wallhaven-curated.json")).unwrap()).unwrap();
    let records: Value = serde_json::from_str(&fs::read_to_string(base.join("docs/plasma-theme-ports/wallhaven-plasma-records.json")).unwrap()).unwrap();
    for (key, section) in [("images", "images"), ("themes", "themes")] {
        for (id, v) in records[section].as_object().unwrap() {
            assert!(catalogue[key].as_object_mut().unwrap().insert(id.clone(), v.clone()).is_none(), "{id} already in the shared catalogue: update the records file");
        }
    }
    // coverage: exactly the shipped non-legacy packages
    let shipped: std::collections::BTreeSet<String> = fs::read_dir(base.join("themes"))
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let doc = fs::read_to_string(e.path().join("theme.toml")).ok()?.parse::<toml_edit::DocumentMut>().ok()?;
            let id = doc["id"].as_str()?.to_string();
            (!decebal::theme::palette::legacy_theme(&id)).then_some(id)
        })
        .collect();
    let keys: std::collections::BTreeSet<String> = catalogue["themes"].as_object().unwrap().keys().cloned().collect();
    let missing: Vec<_> = shipped.difference(&keys).collect();
    // other lanes' new packages are not ours to cover; ours must be covered after the merge
    assert!(!missing.iter().any(|id| id.starts_with("plasma-")), "{missing:?}");
    let mut used = std::collections::BTreeSet::new();
    for theme in PORTS {
        let record = &catalogue["themes"][*theme];
        assert!(!record["placement"].as_str().unwrap().is_empty());
        for mode in ["dark", "light"] {
            let id = record[mode]["image_id"].as_str().unwrap();
            assert!(used.insert(id), "distinct images: {id}");
            let image = &catalogue["images"][id];
            assert_eq!(image["id"], id);
            assert_eq!(image["page_url"], format!("https://wallhaven.cc/w/{id}"));
            assert!(image["full_url"].as_str().unwrap().starts_with(&format!("https://w.wallhaven.cc/full/{}/wallhaven-{id}.", &id[..2])));
            assert!(image["thumbnail_url"].as_str().unwrap().starts_with(&format!("https://th.wallhaven.cc/lg/{}/{id}.", &id[..2])));
            assert_eq!(image["purity"], "sfw");
            assert_eq!(image["ai_review"]["flagged_ai"], false);
            // an image another lane already shipped in the catalogue keeps that lane's (equally strict, differently shaped) record; only
            // the facts every record must hold are asserted for it, the port's own records keep the full schema
            let ours = records["images"].get(id).is_some();
            assert!(!ours || image["ai_review"]["returned_ai_boolean"].is_null());
            let tags = image["tags"].as_array().unwrap();
            assert!(tags.iter().all(|t| t["id"] != 133451));
            assert!(!decebal::settings::wallhaven::curated::rejected_tags(tags.iter().filter_map(|t| t["name"].as_str())), "{id}");
            assert!(!ours || (image["uploader"]["is_author"].is_null() && image["author"]["name"].is_null()), "{id}: uploader is not author");
            assert!(image["license"]["identifier"].is_null(), "{id}: no licence is established");
            let full = &image["full_resolution_review"];
            let sha = full["sha256"].as_str().unwrap();
            assert!(sha.len() == 64 && sha.bytes().all(|c| c.is_ascii_hexdigit()));
            assert!(full["bytes"].as_u64().unwrap() <= 32 << 20);
            if ours {
                assert_eq!(full["dimensions_verified"], true);
                assert_eq!(full["fresh_detail_check"]["ai_tag_133451_present"], false);
                assert_eq!(full["fresh_detail_check"]["purity"], "sfw");
                assert!(full["provenance_limit"].as_str().unwrap().contains("unverified"));
            }
            // the private persistent cache, when present on this machine, must still match the recorded original
            let cache = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/decebal-plasma-ports/wall/full/img"));
            if let Some(cache) = cache {
                for ext in ["png", "jpg"] {
                    let f = cache.join(format!("wallhaven-{id}.{ext}"));
                    if f.is_file() {
                        assert_eq!(decebal::theme::engine::sha256::digest_hex(&fs::read(&f).unwrap()), sha, "{id}: cached original changed");
                    }
                }
            }
            // measured legibility of the declared panel over this photograph: every edge at four viewports including portrait
            let key = format!("{theme}:{mode}:{id}");
            let review = records["legibility_review"][&key]["min_and_mean_wcag_contrast_by_viewport_and_edge"].as_object().unwrap_or_else(|| panic!("{key}"));
            assert!(review.contains_key("1080x1920") && review.contains_key("2560x1080"), "{key}: ultrawide and portrait were measured");
            let default_edge = if *theme == "plasma-breeze" { "bottom" } else { "top" };
            for (vp, edges) in review {
                for edge in EDGES {
                    let min = edges[edge][0].as_f64().unwrap();
                    assert!(min >= 4.0, "{key} {vp} {edge}: panel text contrast {min}");
                    if edge == default_edge {
                        assert!(min >= 4.5, "{key} {vp}: the package's own panel edge must reach 4.5:1, got {min}");
                    }
                }
            }
            let crops_recorded = if ours { full["crops_checked"]["21:9"].is_string() && full["crops_checked"]["9:16"].is_string() } else { image["crop_review"]["ultrawide"].is_string() && image["crop_review"]["portrait"].is_string() };
            assert!(crops_recorded, "{id}: ultrawide and portrait crops recorded");
        }
    }
}
