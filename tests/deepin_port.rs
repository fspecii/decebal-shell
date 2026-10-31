//! Offline contracts of the Deepin desktop port: `themes/deepin`, `assets/desktop-ports/deepin`, `docs/desktop-ports/deepin`.
//! No display, network, live config or process. The upstream values below are the ones read from the revisions in
//! `docs/desktop-ports/deepin/sources.json`; recomputing every colour token from them keeps the manifest from drifting.
use decebal::theme::engine::{archive::Limits, package, schema};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn assets() -> PathBuf {
    repo().join("assets/desktop-ports/deepin")
}

fn manifest() -> toml_edit::DocumentMut {
    fs::read_to_string(repo().join("themes/deepin/theme.toml")).unwrap().parse().unwrap()
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// True when this engine has the universal-panel manifest contract the package needs (`min_shell_version = "0.3.0"`).
fn contract() -> bool {
    schema::field("bar.applets").is_some() && schema::field("branding.logo").is_some()
}

/// True when this engine has the mode-specific window-style contract (`decoration.style_light`, commit ca418f2 of the Lingmo branch). Until the
/// root merges it, its validator rejects the new keys as unknown, so everything that asks the engine about the manifest is gated on this.
fn mode_styles() -> bool {
    schema::field("decoration.style_light").is_some()
}

fn rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    [0, 2, 4].map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f32)
}

/// `fg` at `alpha` over `bg`, rounded, as `#rrggbb`.
fn over(fg: &str, alpha: f32, bg: &str) -> String {
    let (f, b) = (rgb(fg), rgb(bg));
    let c: Vec<String> = (0..3).map(|i| format!("{:02x}", (f[i] * alpha + b[i] * (1.0 - alpha)).round() as u8)).collect();
    format!("#{}", c.concat())
}

fn token(doc: &toml_edit::DocumentMut, mode: &str, key: &str) -> String {
    doc["colors"][mode][key].as_str().unwrap_or_else(|| panic!("colors.{mode}.{key}")).to_ascii_lowercase()
}

#[test]
fn manifest_is_a_deterministic_valid_package() {
    if !contract() || !mode_styles() {
        eprintln!("skipped: this engine predates the universal-panel / mode-specific window-style manifest contract");
        return;
    }
    let dir = repo().join("themes/deepin");
    let (a, report) = package::pack_dir(&dir, &Limits::default()).unwrap();
    assert!(report.ok(), "{:?}", report.errors());
    assert_eq!(a, package::pack_dir(&dir, &Limits::default()).unwrap().0);
}

#[test]
fn colour_tokens_are_recomputed_from_the_pinned_dtk_palette() {
    let doc = manifest();
    // dtkgui src/kernel/dguiapplicationhelper.cpp standardPalette(): Window, Base; DPalette TextTitle 0.85, QPalette WindowText 0.7,
    // TextTips 0.6, FrameBorder (0.05 / 0.1), ItemBackground (0.03 / 0.05); TextWarning.
    for (mode, window, base, ink, border_ink, border_alpha, item_alpha, warning) in
        [("light", "#f8f8f8", "#ffffff", "#000000", "#000000", 0.05, 0.03, "#ff5736"), ("dark", "#252525", "#282828", "#ffffff", "#ffffff", 0.1, 0.05, "#e43f2e")]
    {
        assert_eq!(token(&doc, mode, "background"), window, "{mode} Window");
        assert_eq!(token(&doc, mode, "surface"), base, "{mode} Base");
        assert_eq!(token(&doc, mode, "text"), over(ink, 0.85, window), "{mode} TextTitle");
        assert_eq!(token(&doc, mode, "text_secondary"), over(ink, 0.7, window), "{mode} WindowText");
        assert_eq!(token(&doc, mode, "text_faint"), over(ink, 0.6, window), "{mode} TextTips");
        assert_eq!(token(&doc, mode, "sidebar"), over(ink, item_alpha, window), "{mode} ItemBackground");
        assert_eq!(token(&doc, mode, "danger"), warning, "{mode} TextWarning");
        if mode == "dark" {
            assert_eq!(token(&doc, mode, "border"), over(border_ink, border_alpha, window), "dark FrameBorder");
        } else {
            assert_eq!(token(&doc, mode, "border"), "#e0e0e0", "Chameleon light window border");
        }
        assert_eq!(token(&doc, mode, "accent"), "#0081ff");
    }
    // dde-shell dock blend colours and Chameleon titlebar backgrounds.
    assert_eq!((token(&doc, "light", "dock"), token(&doc, "dark", "dock")), ("#ebebeb".into(), "#0a0a0a".into()));
    assert_eq!((token(&doc, "light", "titlebar"), token(&doc, "dark", "titlebar")), ("#ffffff".into(), "#101010".into()));
    // Text must stay readable: body text on its window, bar text on its bar.
    for mode in ["light", "dark"] {
        let c = |a: &str, b: &str| {
            decebal::theme::model::contrast(
                decebal::theme::model::Rgb(rgb(a)[0] as u8, rgb(a)[1] as u8, rgb(a)[2] as u8),
                decebal::theme::model::Rgb(rgb(b)[0] as u8, rgb(b)[1] as u8, rgb(b)[2] as u8),
            )
        };
        assert!(c(&token(&doc, mode, "text"), &token(&doc, mode, "background")) >= 7.0, "{mode} text");
        assert!(c(&token(&doc, mode, "bar_text"), &token(&doc, mode, "bar")) >= 7.0, "{mode} bar text");
        assert!(c(&token(&doc, mode, "text_faint"), &token(&doc, mode, "background")) >= 4.5, "{mode} faint text");
    }
}

#[test]
fn metrics_and_panel_follow_the_measured_dock() {
    let doc = manifest();
    assert_eq!(doc["min_shell_version"].as_str(), Some("0.3.0"));
    assert_eq!(doc["metrics"]["radius"].as_integer(), Some(14), "floating panel radius");
    assert_eq!(doc["metrics"]["dock_alpha"].as_float(), Some(0.4), "appearance Opacity");
    // dock size 56 * 9 / 14 icon size
    assert_eq!(doc["dock"]["size"].as_integer(), Some(36));
    assert_eq!(doc["dock"]["style"].as_str(), Some("windows"));
    assert_eq!(doc["dock"]["position"].as_str(), Some("bottom"));
    assert_eq!(doc["bar"]["enabled"].as_str(), Some("auto"), "one panel process");

    let known: BTreeSet<&str> = [
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
    ]
    .into();
    let applets: Vec<&str> = doc["bar"]["applets"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(applets.iter().all(|a| known.contains(a)), "{applets:?}");
    assert_eq!(applets.iter().collect::<BTreeSet<_>>().len(), applets.len(), "no duplicate applet");
    for must in ["launcher", "tasks", "tray", "clock", "show_desktop"] {
        assert!(applets.contains(&must), "{must}");
    }
}

#[test]
fn package_never_writes_global_or_user_owned_choices() {
    let text = fs::read_to_string(repo().join("themes/deepin/theme.toml")).unwrap();
    let doc = manifest();
    for key in ["layouts", "mfact", "nmaster", "smart_gaps"] {
        assert!(doc["rdwm"].get(key).is_none(), "global tiling key {key}");
    }
    for section in ["wallpaper", "widgets", "hot_corners", "launcher"] {
        assert!(doc.get(section).is_none(), "section [{section}] would override a user choice");
    }
    assert!(!text.lines().any(|l| l.trim_start().to_ascii_lowercase().starts_with("cursor")), "cursor choice belongs to Settings");
    // `theme apply` copies [icons] over the user's icon theme (measured), so the package must not carry one; the packs are only recommended
    assert!(doc.get("icons").is_none(), "an [icons] section would overwrite the user's icon theme on apply");
    assert_eq!(doc["desktop"]["widgets"].as_str(), Some("none"));
    let files: Vec<_> = fs::read_dir(repo().join("themes/deepin")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(files.iter().all(|f| matches!(f.as_str(), "theme.toml" | "README.md" | "LICENSE.txt" | "widgets")), "package carries only data and notices: {files:?}");
    // The widget is offered, never placed: the user's layout is untouched without an Ask/Keep/Replace decision.
    assert!(doc.get("widgets").is_none());
    let widget = fs::read_to_string(repo().join("themes/deepin/widgets/deepin-memory.toml")).unwrap();
    assert!(widget.contains("size = [170, 170]") && widget.contains("#00c5c0") && widget.contains("#fedf19"), "dde-widgets Memory Monitor small size and ring colours");
    assert!(!widget.contains("exec"), "theme widgets never carry commands");
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        let ft = e.file_type().unwrap();
        assert!(!ft.is_symlink(), "symlink {}", p.display());
        if ft.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn asset_catalogue_pins_the_upstream_and_recommends_without_selecting() {
    let text = fs::read_to_string(assets().join("asset-packs.toml")).unwrap();
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    assert_eq!(doc["format"].as_integer(), Some(1));
    assert_eq!(doc["family"].as_str(), Some("deepin"));
    let sources = json(&repo().join("docs/desktop-ports/deepin/sources.json"));
    let hex40 = |v: &str| v.len() == 40 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let src = doc["source"].as_array_of_tables().unwrap();
    assert_eq!(src.len(), 1);
    let s0 = src.get(0).unwrap();
    assert_eq!(s0["commit"].as_str(), sources["repos"]["deepin-icon-theme"]["revision"].as_str(), "the catalogue and sources.json pin the same commit");
    assert_eq!(s0["license"].as_str(), Some("GPL-3.0-or-later"));
    assert!(s0["repo"].as_str().unwrap().starts_with("https://github.com/linuxdeepin/"));
    assert!(s0["archive_url"].as_str().unwrap().starts_with("https://") && s0["archive_url"].as_str().unwrap().contains(s0["commit"].as_str().unwrap()));
    assert!(s0.get("archive_sha256").is_none(), "GitHub archives are not byte-stable; the tree ids pin the content");
    let lic: Vec<&str> = s0["license_files"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(lic.contains(&"LICENSES/GPL-3.0-or-later.txt"), "{lic:?}");
    // The licence texts sit outside the pack trees and a GitHub archive has no stable hash, so each is pinned by its Git blob id
    // (`git rev-parse 5d11ba4:<file>`, taken from the local clone), in the order of `license_files`, or the installer marks it unverified.
    let oids: Vec<&str> = s0["license_oids"].as_array().expect("license_oids").iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(oids.len(), lic.len(), "one blob id per licence file, same order");
    assert!(oids.iter().all(|o| o.len() == 40 && o.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))));
    let expected = [
        ("LICENSE", "94a9ed024d3859793618152ea559a168bbcbb5e2"),
        ("LICENSES/GPL-3.0-or-later.txt", "d41c0bd98feb776ef7d81efbc1b5447e0284a187"),
        (".reuse/dep5", "acc35823e0f2b0632abd5ed4aa2b046833cf1a52"),
    ];
    assert_eq!(lic.iter().copied().zip(oids.iter().copied()).collect::<Vec<_>>(), expected, "the blobs read from 5d11ba4");
    assert!(hex40(s0["commit"].as_str().unwrap()));
    let packs = doc["pack"].as_array_of_tables().unwrap();
    let ids: Vec<&str> = packs.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["deepin-bloom", "deepin-bloom-dark"]);
    for p in packs.iter() {
        assert!(hex40(p["tree"].as_str().unwrap()), "tree id of {}", p["id"]);
        assert_eq!(p["source"].as_str(), s0["id"].as_str());
        let provides: Vec<&str> = p["provides"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(provides, ["icons", "cursors"]);
    }
    // the link to the theme: one recommendation, for a theme that exists here, naming packs that exist
    let rec = doc["recommend"].as_array_of_tables().unwrap();
    assert_eq!(rec.len(), 1);
    let r = rec.get(0).unwrap();
    assert_eq!(r["theme"].as_str(), Some("deepin"));
    assert!(repo().join("themes").join(r["theme"].as_str().unwrap()).join("theme.toml").is_file());
    for k in ["icons", "cursors"] {
        assert!(ids.contains(&r[k].as_str().unwrap()), "recommended {k} pack exists");
    }
    // no URL that is not https, no command-like key, nothing but data
    // data only: https URLs and a closed set of keys (the installer's validator rejects any other; a command-like key would never load)
    assert!(!text.lines().filter(|l| !l.trim_start().starts_with('#')).any(|l| l.contains("http://")));
    let allowed = [
        "id",
        "repo",
        "revision",
        "commit",
        "license",
        "license_files",
        "license_oids",
        "archive_url",
        "strip_components",
        "name",
        "provides",
        "source",
        "path",
        "tree",
        "description",
        "theme",
        "icons",
        "cursors",
    ];
    for t in [src.iter().collect::<Vec<_>>(), packs.iter().collect::<Vec<_>>(), rec.iter().collect::<Vec<_>>()].concat() {
        for (k, _) in t.iter() {
            assert!(allowed.contains(&k), "unexpected key {k}");
        }
    }
}

#[test]
fn no_licensed_artwork_is_stored_in_the_port() {
    let mut all = vec![];
    walk(&assets(), &mut all);
    for f in &all {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        assert!(matches!(ext, "toml" | "json" | "md" | "py" | "sh"), "unexpected file in the asset folder (artwork belongs to the pinned upstream): {}", f.display());
    }
    let bytes: u64 = all.iter().map(|f| fs::metadata(f).unwrap().len()).sum();
    assert!(bytes < 1_500_000, "the asset folder is declarative data and tools, {bytes} bytes");
}

/// What the engine's `decoration_compat::wanted` computes from the manifest's `[decoration]` table (rules of the contract document: desired =
/// `style_<mode>` else `style`, fallback = `fallback_style_<mode>` else `fallback_style`). The real function is called in
/// `tests/deepin_decoration_modes.rs`, which needs the engine merge; this mirror runs everywhere.
fn wanted(doc: &toml_edit::DocumentMut, dark: bool) -> (String, Option<String>) {
    let d = &doc["decoration"];
    let mode = if dark { "dark" } else { "light" };
    let get = |k: &str| d.get(k).and_then(|v| v.as_str()).map(String::from);
    (get(&format!("style_{mode}")).or_else(|| get("style")).unwrap(), get(&format!("fallback_style_{mode}")).or_else(|| get("fallback_style")))
}

#[test]
fn window_style_follows_the_colour_mode_with_a_proven_fallback_and_native_geometry() {
    let doc = manifest();
    assert_eq!(doc["min_shell_version"].as_str(), Some("0.3.0"));
    assert_eq!(wanted(&doc, false), ("deepin".into(), Some("adwaita-light".into())));
    assert_eq!(wanted(&doc, true), ("deepin-dark".into(), Some("adwaita".into())));
    assert_eq!(doc["decoration"]["style"].as_str(), Some("adwaita"), "the older key is what engines without the contract read");
    // a theme value would override the native geometry (deepin: 50 px / radius 8) of whichever style ends up in use
    for k in ["height", "radius", "title_align", "buttons", "button_order"] {
        assert!(doc["decoration"].get(k).is_none(), "decoration.{k} would override the native style");
    }
    if mode_styles() {
        let (_, report) = package::pack_dir(&repo().join("themes/deepin"), &Limits::default()).unwrap();
        assert!(report.ok(), "{:?}", report.errors());
        for k in ["style_light", "style_dark", "fallback_style_light", "fallback_style_dark"] {
            assert!(schema::field(&format!("decoration.{k}")).is_some(), "{k}");
        }
    } else {
        eprintln!("engine checks skipped: no mode-specific window-style contract in this engine yet");
    }
}

#[test]
fn wallpaper_fragment_follows_the_root_catalogue_contract() {
    // The assertions of tests/curated_theme.rs `catalogue_covers_shipped_designs_with_reviewed_sfw_provenance`, applied to this fragment,
    // plus: ids disjoint from the committed catalogue, photographs only, and the old CGI picks kept as rejected.
    let mut all = vec![];
    walk(&assets(), &mut all);
    for f in &all {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        assert!(!matches!(ext, "jpg" | "jpeg" | "webp" | "png"), "image committed: {}", f.display());
    }
    let doc = json(&assets().join("wallpapers/wallhaven-deepin.json"));
    assert_eq!(doc["format"], 1);
    let images = doc["images"].as_object().unwrap();
    let t = &doc["themes"]["deepin"];
    assert!(!t["placement"].as_str().unwrap().is_empty());
    assert_eq!(t["same_image_both_modes"], false);
    let ids: Vec<&str> = ["dark", "light"].iter().map(|m| t[*m]["image_id"].as_str().unwrap()).collect();
    assert_ne!(ids[0], ids[1]);
    assert_eq!(images.keys().map(String::as_str).collect::<BTreeSet<_>>(), ids.iter().copied().collect::<BTreeSet<_>>(), "exactly the two chosen photographs");
    let central = json(&repo().join("data/wallhaven-curated.json"));
    for id in &ids {
        // disjoint from the root's photographs; after the root merges this fragment the same record may be there, identical, and nothing else
        assert!(central["images"].get(*id).is_none() || central["images"][*id] == images[*id], "{id} is already in the root catalogue with different content");
        let image = &images[*id];
        assert_eq!(image["id"], *id);
        assert_eq!(image["page_url"], format!("https://wallhaven.cc/w/{id}"));
        assert!(image["full_url"].as_str().unwrap().starts_with("https://w.wallhaven.cc/full/"));
        assert!(image["thumbnail_url"].as_str().unwrap().starts_with("https://th.wallhaven.cc/lg/"));
        assert_eq!(image["purity"], "sfw");
        assert_eq!(image["ai_review"]["flagged_ai"], false);
        assert!(image["ai_review"]["returned_ai_boolean"].is_null() || image["ai_review"]["returned_ai_boolean"] == false);
        let res = image["resolution"].as_array().unwrap();
        assert!(res[0].as_u64().unwrap() >= 3840 && res[1].as_u64().unwrap() >= 2160, "{id} full resolution");
        let tags: Vec<String> = image["tags"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_lowercase()).collect();
        for tag in image["tags"].as_array().unwrap() {
            assert_ne!(tag["id"], 133451);
        }
        for banned in [
            "ai art",
            "ai-generated",
            "midjourney",
            "stable diffusion",
            "dall-e",
            "digital art",
            "illustration",
            "space art",
            "3d abstract",
            "cgi",
            "windows 11",
            "microsoft windows",
            "macos",
            "apple inc.",
            "dell",
            "linux",
            "openkylin",
            "deepin",
        ] {
            assert!(!tags.iter().any(|t| t == banned), "{id} carries tag {banned}");
        }
        assert!(tags.iter().any(|t| t == "photography" || t == "nature" || t == "landscape"), "{id}: a photograph by its tags, got {tags:?}");
        assert!(image["uploader"]["is_author"].is_null());
        assert!(image["author"]["name"].is_null(), "unverified candidates must not become credited authors");
        assert!(image["license"]["identifier"].is_null());
        assert!(image["source_url"].is_null() || image["source_url"].as_str().unwrap().starts_with("https://"));
        let full = &image["full_resolution_review"];
        assert_eq!(full["dimensions_verified"], true);
        assert!(full["bytes"].as_u64().unwrap() <= 32 << 20);
        assert_eq!(full["sha256"].as_str().unwrap().len(), 64);
        assert!(!full["notes"].as_str().unwrap().is_empty());
        for key in ["detail_sha256", "thumbnail_sha256"] {
            let digest = image["evidence"][key].as_str().unwrap();
            assert!(digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        // the reviewed crops (16:9, ultrawide, portrait), each hashed and judged
        for crop in ["16x9", "ultrawide", "portrait"] {
            assert_eq!(image["crop_review"][crop]["sha256"].as_str().unwrap().len(), 64, "{id} {crop}");
            assert!(!image["crop_review"][crop]["result"].as_str().unwrap().is_empty(), "{id} {crop}");
        }
    }
    // The earlier picks were CGI renders: recorded as rejected, with a replacement, and kept as history rather than silently dropped.
    let rejected = doc["rejected_recommendations"].as_array().unwrap();
    let ids_rejected: Vec<(&str, &str, &str)> = rejected.iter().map(|r| (r["mode"].as_str().unwrap(), r["id"].as_str().unwrap(), r["replacement_id"].as_str().unwrap())).collect();
    assert_eq!(ids_rejected, [("dark", "7j2wl9", "ex5m3w"), ("light", "3zg3xd", "r2k6km")]);
    for r in rejected {
        assert_eq!(r["theme_id"], "deepin");
        assert_eq!(r["sha256"].as_str().unwrap().len(), 64);
        assert!(r["reason"].as_str().unwrap().contains("digital art"));
    }
    assert!(doc["historical_candidates"]["images"].get("7j2wl9").is_some() && doc["historical_candidates"]["images"].get("3zg3xd").is_some());
}

#[test]
fn docs_pin_every_repository_and_name_the_gaps() {
    let sources = json(&repo().join("docs/desktop-ports/deepin/sources.json"));
    for (name, r) in sources["repos"].as_object().unwrap() {
        let rev = r["revision"].as_str().unwrap();
        assert!(rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()), "{name}");
        assert!(r["url"].as_str().unwrap().starts_with("https://github.com/linuxdeepin/"));
    }
    let readme = fs::read_to_string(repo().join("docs/desktop-ports/deepin/README.md")).unwrap();
    assert!(readme.contains("Remaining gaps"));
    assert!(fs::metadata(repo().join("docs/desktop-ports/deepin/DECORATION-CONTRACT.md")).is_ok());
}

#[test]
fn nirvana_conversion_is_pinned_verified_against_git_objects_and_licensed() {
    let sources = json(&repo().join("docs/desktop-ports/deepin/sources.json"));
    let pinned = sources["repos"]["deepin-desktop-theme"]["revision"].as_str().unwrap();
    let tool = fs::read_to_string(assets().join("tools/build_nirvana.py")).unwrap();
    assert!(tool.contains(&format!("PINNED = \"{pinned}\"")), "the converter must refuse any revision but the pinned one");
    assert!(tool.contains("refusing to overwrite"), "the converter never overwrites");
    assert!(tool.contains("--allow-working-tree") && tool.contains("NOT revision-verified"), "an unverified input is labelled, never passed off");
    let m = json(&assets().join("nirvana/MANIFEST.json"));
    assert_eq!(m["format"], 2);
    assert_eq!(m["revision"].as_str(), Some(pinned));
    assert_eq!(m["license"], "CC-BY-4.0");
    // provenance: the input was hashed against the revision's Git objects, and the tree id is the one the installer's catalogue would pin
    let iv = &m["input_verification"];
    assert_eq!(iv["status"], "git-objects", "the committed manifest must come from a revision-verified run");
    assert_eq!(iv["problem_count"], 0);
    assert_eq!(iv["tree"], "fb56b39daf1364bc2c62baa13a5b672292845725");
    assert_eq!(iv["licence_blobs"]["LICENSES/CC-BY-4.0.txt"], "13ca539f377dc705af32b8d2ce89262298ea2f06");
    let icons = m["icons"].as_object().unwrap();
    assert_eq!(icons.len(), 1435);
    let hex = |v: &Value, n: usize| v.as_str().is_some_and(|s| s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    for (name, i) in icons {
        assert!(hex(&i["git_blob"], 40) && hex(&i["source_sha256"], 64) && hex(&i["png_sha256"], 64), "{name}: provenance fields");
        let layer = i["layer"].as_str().unwrap();
        assert!(layer.ends_with("/normal.light/3/1.webp"), "{name}: only a plain `<prior>.webp` layer is converted, got {layer}");
    }
    for must in ["deepin-launcher", "dde-file-manager", "deepin-terminal", "google-chrome", "folder", "user-trash"] {
        assert_eq!(icons[must]["layer"], "256/normal.light/3/1.webp", "{must}: one plain 768 px layer");
    }
    // the converted PNGs are generated outside the repository, never committed here
    let mut all = vec![];
    walk(&assets(), &mut all);
    assert!(!all.iter().any(|p| p.starts_with(assets().join("nirvana")) && p.extension().is_some_and(|e| e == "png")), "no converted PNG committed");
}

#[test]
fn nirvana_reader_refuses_hostile_containers_and_explains_every_refusal() {
    let m = json(&assets().join("nirvana/MANIFEST.json"));
    let skipped = m["skipped"].as_object().unwrap();
    assert_eq!(skipped.keys().map(String::as_str).collect::<Vec<_>>(), ["deepin-virtualkeyboard", "deepin-xdgicon-convert", "network-display-failed", "network-display-succeed"]);
    assert!(skipped["deepin-xdgicon-convert"].as_str().unwrap().contains("declares 31888 bytes but only 21522 remain"));
    for palette in ["deepin-virtualkeyboard", "network-display-failed", "network-display-succeed"] {
        let why = skipped[palette].as_str().unwrap();
        assert!(why.contains("'1.0.webp'") && why.contains("not a plain"), "{palette}: {why}");
    }
    // The stdlib self-test of the reader and of the provenance checks (a throwaway Git repository, no network, no upstream checkout, no ImageMagick).
    // Needs python3 and git on PATH; skipped on a machine without python3.
    let path = std::env::var_os("PATH").unwrap_or_default();
    let py = std::process::Command::new("python3").arg("-I").arg(assets().join("tools/test_build_nirvana.py")).env_clear().env("PATH", path).output();
    match py {
        Ok(out) => assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr)),
        Err(e) => eprintln!("skipped: no python3 ({e})"),
    }
}

#[test]
fn every_surface_is_readable_in_both_modes() {
    // What the shared showcase test checks for the other packages (it cannot cover this one: with the mode-specific window-style contract an apply
    // stops when no rdwm answers, and that test's sandbox has none).
    use decebal::theme::model::{contrast, Def, Selection};
    let def = Def::parse(&fs::read_to_string(repo().join("themes/deepin/theme.toml")).unwrap()).unwrap();
    assert!(def.has_dark() && def.has_light());
    for dark in [true, false] {
        let t = def.resolve("deepin", dark, &Selection::default());
        for (what, bg) in [("background", t.background), ("surface", t.surface), ("popup", t.popup), ("titlebar", t.titlebar), ("sidebar", t.sidebar)] {
            assert!(contrast(t.text, bg) >= 4.5, "dark={dark}: text on {what} {:.2}", contrast(t.text, bg));
        }
        assert!(contrast(t.bar_text, t.bar) >= 4.5, "dark={dark}: bar text");
        assert!(contrast(t.accent_text, t.accent) >= 3.0, "dark={dark}: accent text (white on #0081ff, as DTK)");
        assert_eq!(t.background.luminance() > 0.3, !dark, "dark={dark}: variant brightness");
    }
}
