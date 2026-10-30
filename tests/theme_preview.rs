//! Actual headless worker regression: source changes must select new native pixels.
use decebal::theme::{
    engine::{
        preview,
        writer::{self, Header},
    },
    live::Source,
    model::{Mode, Selection},
};
use std::{fs, path::PathBuf, process::Command};

fn fixture() -> (PathBuf, Source) {
    let dir = std::env::temp_dir().join(format!("native-preview-contract-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(dir.join("themes/probe/widgets")).unwrap();
    fs::create_dir_all(dir.join("themes/probe/wallpapers")).unwrap();
    let text = r##"format=1
id="probe"
name="Preview probe"
version="1.0.0"
author="test"
license="MIT"
[colors.dark]
background="#101520"
text="#ffffff"
accent="#ff77aa"
[colors.light]
background="#f4f7ff"
text="#112244"
accent="#774455"
[wallpaper]
dark="wallpapers/a.png"
light="wallpapers/a.png"
[bar]
enabled=false
[dock]
enabled=false
[[widgets]]
use="probe"
place="desktop"
anchor="top-left"
x=70
y=80
"##;
    fs::write(dir.join("themes/probe/theme.toml"), text).unwrap();
    fs::write(dir.join("themes/probe/widgets/probe.toml"), "format=1\nid=\"probe\"\nsize=[300,100]\n[root]\ntype=\"text\"\ntext=\"12:34\"\nsize=48\n").unwrap();
    wall(&dir, [30, 50, 70]);
    let source = Source { shell_json: dir.join("live-sentinel.json"), themes_dir: dir.join("themes") };
    fs::write(&source.shell_json, "{\"do_not_touch\":true}").unwrap();
    (dir, source)
}
fn wall(dir: &std::path::Path, color: [u8; 3]) {
    image::RgbImage::from_pixel(20, 20, image::Rgb(color)).save(dir.join("themes/probe/wallpapers/a.png")).unwrap();
}
fn render(dir: &std::path::Path, src: &Source, mode: &str) -> String {
    render_bin(dir, src, mode, std::path::Path::new(env!("CARGO_BIN_EXE_decthemectl")))
}
fn render_bin(dir: &std::path::Path, src: &Source, mode: &str, bin: &std::path::Path) -> String {
    let out = Command::new(bin)
        .args(["theme", "render", "probe", "--mode", mode, "--png"])
        .arg(dir.join(format!("result-{mode}.png")))
        .env("DECEBAL_CONFIG", dir)
        .env("DECEBAL_THEMES", &src.themes_dir)
        .env("DECEBAL_SYSTEM_THEMES", dir.join("no-system"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("DECEBAL_SYSTEM_AUTOMATIC", "0")
        .env("RDWMCTL", "/nonexistent/rdwmctl")
        .env("DISPLAY", "")
        .env("WAYLAND_DISPLAY", "")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}
#[test]
fn automatic_native_cache_tracks_palette_layout_widget_wallpaper_and_mode() {
    let (dir, src) = fixture();
    let before = fs::read(&src.shell_json).unwrap();
    let start = std::time::Instant::now();
    let first = render(&dir, &src, "dark");
    eprintln!("cold native preview: {:?}; evidence {}", start.elapsed(), dir.display());
    let original = fs::read(dir.join("result-dark.png")).unwrap();
    let warm = std::time::Instant::now();
    assert_eq!(render(&dir, &src, "dark"), first, "unchanged inputs reuse the same cache entry");
    eprintln!("persistent cached native preview: {:?}", warm.elapsed());
    assert!(dir.join("cache/decebal/theme-previews/fontconfig").is_dir(), "font cache outlives individual jobs");
    assert_eq!(fs::read(dir.join("result-dark.png")).unwrap(), original);
    let manifest = dir.join("themes/probe/theme.toml");
    let s = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, s.replace("#ffffff", "#aaff00")).unwrap();
    let palette = render(&dir, &src, "dark");
    assert_ne!(palette, first);
    assert_ne!(fs::read(dir.join("result-dark.png")).unwrap(), original);
    let s = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, s.replace("x=70", "x=270")).unwrap();
    let layout = render(&dir, &src, "dark");
    assert_ne!(layout, palette);
    let widget = dir.join("themes/probe/widgets/probe.toml");
    let s = fs::read_to_string(&widget).unwrap();
    fs::write(widget, s.replace("12:34", "56:78")).unwrap();
    let widgets = render(&dir, &src, "dark");
    assert_ne!(widgets, layout);
    wall(&dir, [95, 32, 14]);
    let wallpaper = render(&dir, &src, "dark");
    assert_ne!(wallpaper, widgets);
    let light = render(&dir, &src, "light");
    assert_ne!(light, wallpaper);
    assert_ne!(fs::read(dir.join("result-light.png")).unwrap(), fs::read(dir.join("result-dark.png")).unwrap());
    assert_eq!(fs::read(&src.shell_json).unwrap(), before, "live sentinel stays unchanged");
    assert!(!dir.join("decebal.json").exists(), "source configuration is never applied");
}

#[test]
fn fingerprint_includes_resolved_palette_and_builtin_widget_sources() {
    let (dir, src) = fixture();
    let dark = Selection { name: "probe".into(), mode: Mode::Dark, ..Default::default() };
    let a = preview::fingerprint(&src, &dark).unwrap();
    let b = preview::fingerprint(&src, &Selection { palette: Some("nord".into()), ..dark.clone() }).unwrap();
    assert_ne!(a, b);
    let mut def = src.load_def("probe").unwrap();
    def.extra.insert("widgets".into(), serde_json::json!([{"use":"builtin:clock-card","anchor":"top-right"}]));
    fs::write(dir.join("themes/probe/theme.toml"), writer::def_to_manifest(&def, &Header { id: "probe".into(), name: "Preview probe".into(), version: Some("1.0.0".into()), ..Default::default() }))
        .unwrap();
    assert_ne!(a, preview::fingerprint(&src, &dark).unwrap());
}

#[test]
fn fresh_process_and_reinstalled_renderer_reuse_stored_png_without_rewriting_it() {
    let (dir, src) = fixture();
    let a = dir.join("first-install/decthemectl");
    let b = dir.join("second-install/decthemectl");
    for executable in [&a, &b] {
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_decthemectl"), executable).unwrap();
    }
    let first = render_bin(&dir, &src, "dark", &a);
    let path = std::path::PathBuf::from(first.split("(cache ").nth(1).unwrap().trim().trim_end_matches(')'));
    let sentinel = std::time::UNIX_EPOCH + std::time::Duration::from_secs(123456);
    fs::File::options().write(true).open(&path).unwrap().set_times(fs::FileTimes::new().set_modified(sentinel)).unwrap();
    fs::File::options().write(true).open(&b).unwrap().set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))).unwrap();
    assert_eq!(render_bin(&dir, &src, "dark", &b), first, "a new process and install path must find the same stored preview");
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), sentinel, "the native worker must not rewrite an unchanged PNG");
    assert_eq!(fs::read_dir(dir.join("cache/decebal/theme-previews/saved")).unwrap().count(), 1, "persistent gallery reference is reused");
    fs::remove_dir_all(dir).unwrap();
}
