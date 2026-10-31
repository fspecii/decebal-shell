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

#[test]
fn session_accessibility_does_not_pollute_cached_theme_samples() {
    let (dir, src) = fixture();
    let manifest = dir.join("themes/probe/theme.toml");
    let text = fs::read_to_string(&manifest).unwrap().replace("[dock]\nenabled=false", "[dock]\nenabled=true");
    fs::write(manifest, text).unwrap();
    let run = |enabled: bool| {
        let out = Command::new(env!("CARGO_BIN_EXE_decthemectl"))
            .args(["theme", "render", "probe", "--mode", "dark", "--png"])
            .arg(dir.join("sample.png"))
            .env("DECEBAL_CONFIG", &dir)
            .env("DECEBAL_THEMES", &src.themes_dir)
            .env("DECEBAL_SYSTEM_THEMES", dir.join("no-system"))
            .env("XDG_CACHE_HOME", dir.join("cache"))
            .env("DECEBAL_SYSTEM_AUTOMATIC", "0")
            .env("RDWMCTL", "/nonexistent/rdwmctl")
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", "")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .env("DECEBAL_FONT_SCALE", if enabled { "2" } else { "1" })
            .env("DECEBAL_HIGH_CONTRAST", if enabled { "1" } else { "0" })
            .env("DECEBAL_REDUCE_MOTION", if enabled { "1" } else { "0" })
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        fs::read(dir.join("sample.png")).unwrap()
    };
    let regular = run(false);
    fs::remove_dir_all(dir.join("cache/decebal/theme-previews")).unwrap();
    assert_eq!(run(true), regular, "cold previews must not cache session-dependent dock opacity or motion");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn official_preview_samples_cached_photography_without_changing_live_wallpaper() {
    let (dir, src) = fixture();
    let before = fs::read(&src.shell_json).unwrap();
    let render_aster = || {
        let out = Command::new(env!("CARGO_BIN_EXE_decthemectl"))
            .args(["theme", "render", "aster", "--mode", "dark", "--png"])
            .arg(dir.join("aster.png"))
            .env("DECEBAL_CONFIG", &dir)
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
        image::open(dir.join("aster.png")).unwrap().to_rgb8()
    };
    let plain = render_aster();
    fs::create_dir_all(dir.join("backgrounds")).unwrap();
    let id = decebal::settings::wallhaven::curated::choices("aster").unwrap().remove(0).wall.id;
    // A non-square test fixture catches wrong stretch/crop handling without provider network.
    let sample = image::RgbImage::from_pixel(800, 200, image::Rgb([191, 55, 87]));
    sample.save(dir.join(format!("backgrounds/wallhaven-{id}.png"))).unwrap();
    let photo = render_aster();
    assert_ne!(plain, photo);
    assert_eq!(photo.get_pixel(10, 540).0, [191, 55, 87]);
    assert_eq!(render_aster(), photo, "the independent sample is persistently cached");
    assert_eq!(fs::read(&src.shell_json).unwrap(), before);
    assert!(!dir.join("decebal.json").exists());
    fs::remove_dir_all(dir).unwrap();
}

/// Two installed roots as `make install` leaves them (`PREFIX=/usr/local` and `/usr`), a user data home with nothing in it, a relative
/// entry that must be ignored, and an empty user theme folder: the child lives in one root, its parent, widget and logo in the other.
struct Installed {
    dir: PathBuf,
    local: PathBuf,
    usr: PathBuf,
}
fn installed_fixture() -> Installed {
    let dir = std::env::temp_dir().join(format!("native-preview-installed-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let (local, usr) = (dir.join("usr-local/share/decebal/themes"), dir.join("usr/share/decebal/themes"));
    fs::create_dir_all(dir.join("themes")).unwrap();
    for pkg in [usr.join("lib-base/widgets"), usr.join("lib-base/icons"), usr.join("lib-base/fonts"), local.join("lib-kid")] {
        fs::create_dir_all(pkg).unwrap();
    }
    let header = |id: &str| format!("format=1\nid=\"{id}\"\nname=\"{id}\"\nversion=\"1.0.0\"\nauthor=\"test\"\nlicense=\"MIT\"\n");
    fs::write(
        usr.join("lib-base/theme.toml"),
        header("lib-base") + "[colors.dark]\nbackground=\"#101520\"\ntext=\"#ffffff\"\naccent=\"#ff77aa\"\n[colors.light]\nbackground=\"#f4f7ff\"\ntext=\"#112244\"\naccent=\"#774455\"\n[branding]\nlogo=\"icons/logo.png\"\n[font]\nfiles=[\"fonts/not-a-font.ttf\"]\n",
    )
    .unwrap();
    fs::write(usr.join("lib-base/widgets/probe.toml"), "format=1\nid=\"probe\"\nsize=[300,100]\n[root]\ntype=\"text\"\ntext=\"12:34\"\nsize=48\n").unwrap();
    image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30])).save(usr.join("lib-base/icons/logo.png")).unwrap();
    fs::write(usr.join("lib-base/fonts/not-a-font.ttf"), b"package font bytes").unwrap();
    fs::write(
        local.join("lib-kid/theme.toml"),
        header("lib-kid") + "extends=\"lib-base\"\n[bar]\nenabled=false\n[dock]\nenabled=false\n[[widgets]]\nuse=\"probe\"\nplace=\"desktop\"\nanchor=\"top-left\"\nx=70\ny=80\n",
    )
    .unwrap();
    Installed { dir, local, usr }
}
impl Installed {
    fn command(&self, themes: &std::path::Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_decthemectl"));
        c.args(["theme", "render", "lib-kid", "--mode", "dark", "--png"])
            .arg(self.dir.join("out.png"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.dir.join("home"))
            .env("XDG_CONFIG_HOME", self.dir.join("home/.config"))
            .env("XDG_CACHE_HOME", self.dir.join("cache"))
            .env("XDG_DATA_HOME", self.dir.join("home/data"))
            .env("XDG_DATA_DIRS", format!("relative/ignored:{}:{}:{}/", self.dir.join("usr-local/share").display(), self.dir.join("usr/share").display(), self.dir.join("usr-local/share").display()))
            .env("XDG_RUNTIME_DIR", self.dir.join("run"))
            .env("DECEBAL_CONFIG", &self.dir)
            .env("DECEBAL_THEMES", themes)
            .env("DECEBAL_SYSTEM_AUTOMATIC", "0")
            .env("RDWMCTL", "/nonexistent/rdwmctl")
            .env("DISPLAY", "")
            .env("WAYLAND_DISPLAY", "")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent");
        c
    }
    fn render(&self) -> String {
        let out = self.command(&self.dir.join("themes")).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }
}
fn cache_path(output: &str) -> PathBuf {
    PathBuf::from(output.split("(cache ").nth(1).unwrap().trim().trim_end_matches(')'))
}

#[test]
fn native_preview_reads_every_installed_root_and_a_fresh_process_reuses_the_cache() {
    let t = installed_fixture();
    let tree = |root: &std::path::Path| -> Vec<(PathBuf, u64)> {
        let mut v = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let m = e.metadata().unwrap();
                if m.is_dir() {
                    stack.push(e.path());
                } else {
                    v.push((e.path(), m.len()));
                }
            }
        }
        v.sort();
        v
    };
    let before = (tree(&t.local), tree(&t.usr));
    let first = t.render();
    let png = cache_path(&first);
    let notes: serde_json::Value = serde_json::from_slice(&fs::read(png.with_extension("json")).unwrap()).unwrap();
    assert_eq!(notes["widgets"], 1, "the parent's widget in the other root was drawn for the child: {notes}");
    assert!(!notes["notes"].to_string().contains("not installed"), "{notes}");
    // the same packages in the user's own folder, with no installed roots at all, draw the same pixels
    let mine = t.dir.join("mine");
    fs::create_dir_all(&mine).unwrap();
    for (root, id) in [(&t.usr, "lib-base"), (&t.local, "lib-kid")] {
        let dest = mine.join(id);
        assert!(Command::new("cp").arg("-r").arg(root.join(id)).arg(&dest).status().unwrap().success());
    }
    let reference = t.dir.join("reference.png");
    let out = {
        let mut c = t.command(&mine);
        c.env("DECEBAL_SYSTEM_THEMES", t.dir.join("no-system")).env("XDG_CACHE_HOME", t.dir.join("cache-reference"));
        c.output().unwrap()
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    fs::copy(t.dir.join("out.png"), &reference).unwrap();
    // (the reference run overwrote out.png; render the installed layout again for a like-for-like file)
    assert_eq!(t.render(), first, "a fresh process finds the same stored preview");
    assert_eq!(fs::read(t.dir.join("out.png")).unwrap(), fs::read(&reference).unwrap(), "installed roots and the user folder give the same native pixels");
    // reuse: nothing is rewritten and no second image appears
    let sentinel = std::time::UNIX_EPOCH + std::time::Duration::from_secs(123456);
    fs::File::options().write(true).open(&png).unwrap().set_times(fs::FileTimes::new().set_modified(sentinel)).unwrap();
    assert_eq!(t.render(), first);
    assert_eq!(fs::metadata(&png).unwrap().modified().unwrap(), sentinel, "an unchanged install is not re-rendered");
    assert_eq!(fs::read_dir(t.dir.join("cache/decebal/theme-previews/saved")).unwrap().count(), 1);
    // dependencies in either root are part of the cache key
    let widget = t.usr.join("lib-base/widgets/probe.toml");
    fs::write(&widget, fs::read_to_string(&widget).unwrap().replace("12:34", "56:78")).unwrap();
    let edited = t.render();
    assert_ne!(edited, first, "an edit in the parent's root regenerates");
    let logo = t.usr.join("lib-base/icons/logo.png");
    image::RgbImage::from_pixel(8, 8, image::Rgb([30, 200, 30])).save(&logo).unwrap();
    let relogo = t.render();
    assert_ne!(relogo, edited, "the parent's logo asset is a dependency");
    // a same-id package in the higher-priority root now wins, and the key says so
    fs::create_dir_all(t.local.join("lib-base")).unwrap();
    fs::write(
        t.local.join("lib-base/theme.toml"),
        fs::read_to_string(t.usr.join("lib-base/theme.toml")).unwrap().replace("#ff77aa", "#00ccff").replace("[branding]\nlogo=\"icons/logo.png\"\n[font]\nfiles=[\"fonts/not-a-font.ttf\"]\n", ""),
    )
    .unwrap();
    let shadow = t.render();
    assert_ne!(shadow, relogo);
    // and the explicit override keeps its single-directory meaning: neither root is searched, so the parent is missing
    let out = t.command(&t.dir.join("themes")).env("DECEBAL_SYSTEM_THEMES", t.dir.join("no-system")).output().unwrap();
    assert!(!out.status.success(), "DECEBAL_SYSTEM_THEMES excludes the XDG roots");
    // nothing but the files this test wrote itself exists in the installed roots
    let mut want = before.0.clone();
    want.push((t.local.join("lib-base/theme.toml"), fs::metadata(t.local.join("lib-base/theme.toml")).unwrap().len()));
    want.sort();
    assert_eq!(tree(&t.local), want);
    assert!(!t.dir.join("decebal.json").exists());
    fs::remove_dir_all(t.dir).unwrap();
}
