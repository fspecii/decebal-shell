//! Users & Login (read-only account info, picture into `~/.face`) and About & Storage (system facts from /proc,
//! /etc/os-release and `lspci`, disk usage, component versions, "Check config").

use super::run::{Cmd, Runner};
use super::{Dom, Done, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Row};
use std::path::{Path, PathBuf};

// ================================================================ users

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Users {
    pub name: String,
    pub real_name: String,
    pub uid: u32,
    pub shell: String,
    pub home: String,
    pub groups: Vec<String>,
    pub face: Option<(PathBuf, u64)>,
    pub display_manager: Option<String>,
}

#[derive(Default)]
pub struct UsersUi {
    pub path: String,
}

/// One /etc/passwd line for `uid`: (name, gid, gecos, home, shell).
pub fn parse_passwd(text: &str, uid: u32) -> Option<(String, u32, String, String, String)> {
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 7 && f[2].parse::<u32>().ok() == Some(uid)).then(|| (f[0].to_string(), f[3].parse().unwrap_or(0), f[4].split(',').next().unwrap_or("").to_string(), f[5].to_string(), f[6].to_string()))
    })
}

/// Group names the user belongs to (primary group first).
pub fn parse_groups(text: &str, user: &str, gid: u32) -> Vec<String> {
    let mut primary = None;
    let mut rest = Vec::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split(':').collect();
        if f.len() < 4 {
            continue;
        }
        if f[2].parse::<u32>().ok() == Some(gid) {
            primary = Some(f[0].to_string());
        } else if f[3].split(',').any(|m| m == user) {
            rest.push(f[0].to_string());
        }
    }
    rest.sort();
    primary.into_iter().chain(rest).collect()
}

pub fn load_users() -> Users {
    let uid = unsafe { libc::getuid() };
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let (name, gid, real, home, shell) = parse_passwd(&passwd, uid).unwrap_or_else(|| (std::env::var("USER").unwrap_or_default(), 0, String::new(), std::env::var("HOME").unwrap_or_default(), std::env::var("SHELL").unwrap_or_default()));
    let groups = parse_groups(&std::fs::read_to_string("/etc/group").unwrap_or_default(), &name, gid);
    let face = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".face")).and_then(|p| std::fs::metadata(&p).ok().filter(|m| m.is_file()).map(|m| (p, m.len())));
    let display_manager = std::fs::read_link("/etc/systemd/system/display-manager.service").ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().trim_end_matches(".service").to_string()));
    Users { name, real_name: real, uid, shell, home, groups, face, display_manager }
}

/// Validates and installs a picture as `~/.face` (256x256 PNG). Blocking: call from a worker.
pub fn install_face(src: &Path, dest: &Path) -> Result<(), String> {
    let meta = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
    if !meta.is_file() {
        return Err("That is not a file".into());
    }
    if meta.len() > 40 << 20 {
        return Err("That picture is too large (limit 40 MB)".into());
    }
    let img = image::ImageReader::open(src).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|_| "That file is not a picture macsettings can read (PNG, JPEG or WebP)".to_string())?;
    let sq = img.resize_to_fill(256, 256, image::imageops::FilterType::Triangle);
    let tmp = dest.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        sq.write_to(&mut f, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub fn users_items(h: &Hub) -> Vec<Item> {
    let Some(u) = &h.users.data else { return vec![Item::Note { text: "Reading account...".into(), warn: false }] };
    let lbl = |id: &str, t: String| Ctl::new(format!("lbl:{id}"), Kind::Label(t));
    let mut v = vec![Item::Header("Account".into())];
    v.push(Item::Card(vec![
        Row::new(if u.real_name.is_empty() { u.name.clone() } else { u.real_name.clone() }).detail(format!("User {} · id {}", u.name, u.uid)),
        Row::new("Login shell").ctl(lbl("shell", u.shell.clone())),
        Row::new("Home folder").ctl(lbl("home", u.home.clone())),
        Row::new("Groups").detail(u.groups.join(", ")),
    ]));
    v.push(Item::Header("Picture".into()));
    v.push(Item::Card(vec![
        Row::new("Current picture").detail(match &u.face {
            Some((p, n)) => format!("{} ({} KB)", p.display(), n / 1024),
            None => "None: ~/.face is not set".into(),
        }),
        Row::new("Use this image file").detail("Path to a PNG, JPEG or WebP; it is cropped to a square and saved as ~/.face").ctl(Ctl::new("s:usr:path:", Kind::Text { val: h.ui_users.path.clone(), hint: "/path/to/picture.png".into() })).ctl(Ctl::new("s:usr:setface:", Kind::Button { label: "Set".into(), danger: false })),
    ]));
    v.push(Item::Header("Login screen".into()));
    v.push(Item::Note {
        text: format!("{} The login screen's theme belongs to the display manager and is not changed here.", match &u.display_manager {
            Some(d) => format!("Your login screen is {d}."),
            None => "No display manager service was found.".to_string(),
        }),
        warn: false,
    });
    v
}

pub fn users_apply(h: &mut Hub, act: &str, val: CtlVal) {
    match (act, &val) {
        ("path", CtlVal::Str(p)) => {
            h.ui_users.path = p.clone();
            h.dirty = true;
        }
        ("setface", _) => {
            let p = h.ui_users.path.trim().to_string();
            if p.is_empty() {
                h.say("Type the path of an image first", true);
                return;
            }
            let Some(dest) = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".face")) else {
                h.say("HOME is not set", true);
                return;
            };
            h.sh.spawn(move |_| match install_face(Path::new(&p), &dest) {
                Ok(()) => Done { msg: Some(("Picture updated".into(), false)), refresh: Some(Dom::Users), ..Done::default() },
                Err(e) => Done { msg: Some((format!("Picture: {e}"), true)), ..Done::default() },
            });
        }
        _ => h.say("Unsupported account change", true),
    }
}

// ================================================================ storage / about

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Disk {
    pub path: String,
    pub total: u64,
    pub free: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Storage {
    pub distro: String,
    pub kernel: String,
    pub host: String,
    pub cpu: String,
    pub cores: usize,
    pub ram_kb: u64,
    pub gpus: Vec<String>,
    pub disks: Vec<Disk>,
    pub tools: Vec<(String, Option<PathBuf>)>,
    pub lspci_missing: bool,
}

pub fn parse_os_release(text: &str) -> String {
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string());
    get("PRETTY_NAME").or_else(|| get("NAME")).unwrap_or_else(|| "Linux".into())
}

/// -> (model name, logical cores).
pub fn parse_cpuinfo(text: &str) -> (String, usize) {
    let name = text.lines().find_map(|l| l.strip_prefix("model name").and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_string())).unwrap_or_default();
    (name, text.lines().filter(|l| l.starts_with("processor")).count())
}

pub fn parse_meminfo_kb(text: &str) -> u64 {
    text.lines().find_map(|l| l.strip_prefix("MemTotal:")?.split_whitespace().next()?.parse().ok()).unwrap_or(0)
}

/// `lspci -mm`: graphics adapters as "Vendor Device".
pub fn parse_lspci_gpus(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let q: Vec<&str> = l.split('"').collect();
            // slot "class" "vendor" "device" ...
            let (class, vendor, dev) = (q.get(1)?, q.get(3)?, q.get(5)?);
            (class.contains("VGA") || class.contains("3D controller") || class.contains("Display controller")).then(|| format!("{} {}", vendor.replace("Corporation", "").replace("[AMD/ATI]", "AMD").replace("Advanced Micro Devices, Inc.", "AMD").trim(), dev))
        })
        .collect()
}

pub fn disk_usage(path: &str) -> Option<Disk> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: statvfs writes into a zeroed struct we own.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut s) };
    (rc == 0).then(|| Disk { path: path.into(), total: s.f_blocks as u64 * s.f_frsize as u64, free: s.f_bavail as u64 * s.f_frsize as u64 })
}

pub fn human(b: u64) -> String {
    let g = b as f64 / (1u64 << 30) as f64;
    if g >= 1000.0 {
        format!("{:.1} TB", g / 1024.0)
    } else if g >= 1.0 {
        format!("{g:.1} GB")
    } else {
        format!("{:.0} MB", b as f64 / (1u64 << 20) as f64)
    }
}

pub const TOOLS: &[&str] = &["rdwm", "rdwmctl", "macdock", "macbar", "machaven", "maclaunch", "macspot", "macsettings", "macwm"];

pub fn load_storage(r: &dyn Runner) -> Storage {
    let rd = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let (cpu, cores) = parse_cpuinfo(&rd("/proc/cpuinfo"));
    let (gpus, lspci_missing) = if r.exists("lspci") {
        let o = r.run(&Cmd::new("lspci", &["-mm"]).timeout(5));
        (parse_lspci_gpus(&o.stdout), !o.ok)
    } else {
        (vec![], true)
    };
    let mut disks = Vec::new();
    disks.extend(disk_usage("/"));
    if let Some(h) = std::env::var("HOME").ok().filter(|h| h != "/") {
        if let Some(d) = disk_usage(&h) {
            if disks.first().is_none_or(|r| (r.total, r.free) != (d.total, d.free)) {
                disks.push(d);
            }
        }
    }
    Storage {
        distro: parse_os_release(&rd("/etc/os-release")),
        kernel: rd("/proc/sys/kernel/osrelease").trim().to_string(),
        host: rd("/proc/sys/kernel/hostname").trim().to_string(),
        cpu,
        cores,
        ram_kb: parse_meminfo_kb(&rd("/proc/meminfo")),
        gpus,
        disks,
        tools: TOOLS.iter().map(|t| (t.to_string(), super::run::which(t))).collect(),
        lspci_missing,
    }
}

pub fn storage_items(h: &Hub, rdwm_version: Option<&str>) -> Vec<Item> {
    let Some(s) = &h.storage.data else { return vec![Item::Note { text: "Reading system information...".into(), warn: false }] };
    let l = |id: &str, t: String| Ctl::new(format!("lbl:{id}"), Kind::Label(t));
    let mut v = vec![Item::Header("This computer".into())];
    let mut rows = vec![
        Row::new("Distribution").ctl(l("distro", s.distro.clone())),
        Row::new("Kernel").ctl(l("kernel", s.kernel.clone())),
        Row::new("Host name").ctl(l("host", s.host.clone())),
        Row::new("Processor").detail(format!("{} logical cores", s.cores)).ctl(l("cpu", s.cpu.clone())),
        Row::new("Memory").ctl(l("ram", format!("{:.1} GB", s.ram_kb as f64 / 1048576.0))),
    ];
    if s.gpus.is_empty() {
        rows.push(Row::new("Graphics").detail(if s.lspci_missing { "Not available: lspci is not installed" } else { "No graphics adapter reported" }));
    }
    for (i, g) in s.gpus.iter().enumerate() {
        rows.push(Row::new(if i == 0 { "Graphics" } else { "" }).ctl(l(&format!("gpu{i}"), g.clone())));
    }
    v.push(Item::Card(rows));
    v.push(Item::Header("Disk usage".into()));
    v.push(Item::Card(
        s.disks
            .iter()
            .map(|d| {
                let used = d.total.saturating_sub(d.free);
                Row::new(d.path.clone()).detail(format!("{} used of {}", human(used), human(d.total))).ctl(Ctl::new(format!("s:sto:disk:{}", d.path), Kind::Slider { v: if d.total > 0 { (used as f64 * 100.0 / d.total as f64).round() } else { 0.0 }, min: 0.0, max: 100.0, step: 1.0, unit: " %".into() }).tap(|c| c.enabled = false))
            })
            .collect(),
    ));
    v.push(Item::Header("Software".into()));
    let mut rows = vec![Row::new("macshell").detail("This build").ctl(l("macshell", env!("CARGO_PKG_VERSION").to_string()))];
    rows.push(Row::new("rdwm").detail("Running window manager").ctl(l("rdwmv", rdwm_version.map(|v| v.to_string()).unwrap_or_else(|| "not running".into()))));
    for (t, p) in &s.tools {
        rows.push(Row::new(format!("  {t}")).ctl(l(&format!("tool:{t}"), p.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not installed".into()))));
    }
    v.push(Item::Card(rows));
    v.push(Item::Header("Configuration".into()));
    v.push(Item::Card(vec![Row::new("Check configuration").detail("Validate rdwm's config.toml with rdwm's own parser").ctl(Ctl::new("s:sto:check:", Kind::Button { label: "Check config".into(), danger: false }))]));
    v
}

trait Tap: Sized {
    fn tap(self, f: impl FnOnce(&mut Self)) -> Self;
}
impl Tap for Ctl {
    fn tap(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::FakeRunner;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\nambsd:x:1000:1000:Amb Sd,,,:/home/ambsd:/usr/bin/fish\n";
    const GROUP: &str = "root:x:0:\nwheel:x:998:ambsd\nambsd:x:1000:\nvideo:x:985:ambsd,other\naudio:x:996:other\n";

    #[test]
    fn passwd_and_groups() {
        let (n, gid, real, home, shell) = parse_passwd(PASSWD, 1000).unwrap();
        assert_eq!((n.as_str(), gid, real.as_str(), home.as_str(), shell.as_str()), ("ambsd", 1000, "Amb Sd", "/home/ambsd", "/usr/bin/fish"));
        assert!(parse_passwd(PASSWD, 4242).is_none());
        assert_eq!(parse_groups(GROUP, "ambsd", 1000), ["ambsd", "video", "wheel"]);
    }

    #[test]
    fn system_facts() {
        assert_eq!(parse_os_release("NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\n"), "Arch Linux");
        assert_eq!(parse_os_release("NAME=Foo\n"), "Foo");
        assert_eq!(parse_os_release(""), "Linux");
        let (m, c) = parse_cpuinfo("processor\t: 0\nmodel name\t: AMD Ryzen 9 5900X\nprocessor\t: 1\nmodel name\t: AMD Ryzen 9 5900X\n");
        assert_eq!((m.as_str(), c), ("AMD Ryzen 9 5900X", 2));
        assert_eq!(parse_meminfo_kb("MemTotal:       32768000 kB\nMemFree: 1 kB\n"), 32768000);
        let g = parse_lspci_gpus("01:00.0 \"VGA compatible controller\" \"NVIDIA Corporation\" \"GA106 [GeForce RTX 3060]\" -ra1 \"Gigabyte\" \"x\"\n00:14.0 \"USB controller\" \"AMD\" \"x\"\n");
        assert_eq!(g, ["NVIDIA GA106 [GeForce RTX 3060]"]);
        assert_eq!(human(500 << 20), "500 MB");
        assert_eq!(human(3 << 30), "3.0 GB");
        assert_eq!(human(2u64 << 40), "2.0 TB");
    }

    #[test]
    fn disk_usage_of_root_works() {
        let d = disk_usage("/").unwrap();
        assert!(d.total > 0 && d.free <= d.total);
        assert!(disk_usage("/definitely/not/here").is_none());
    }

    #[test]
    fn storage_loads_without_lspci() {
        let s = load_storage(&FakeRunner::new());
        assert!(s.lspci_missing && s.gpus.is_empty());
        assert!(!s.distro.is_empty() && !s.kernel.is_empty());
        assert_eq!(s.tools.len(), TOOLS.len());
        let mut h = Hub::with(std::sync::Arc::new(FakeRunner::lenient()), std::env::temp_dir().join("ms-sto-t"), std::sync::Arc::new(|| {}));
        h.storage.data = Some(s);
        let t = format!("{:?}", storage_items(&h, Some("0.9")));
        assert!(t.contains("lspci is not installed") && t.contains("s:sto:check:") && t.contains("0.9"));
    }

    #[test]
    fn face_install_validates_and_scales() {
        let d = std::env::temp_dir().join(format!("ms-face-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let src = d.join("a.png");
        let mut im = image::RgbaImage::new(400, 200);
        im.pixels_mut().for_each(|p| *p = image::Rgba([200, 30, 30, 255]));
        im.save(&src).unwrap();
        let dest = d.join(".face");
        install_face(&src, &dest).unwrap();
        let out = image::ImageReader::open(&dest).unwrap().with_guessed_format().unwrap().decode().unwrap();
        assert_eq!((out.width(), out.height()), (256, 256));
        std::fs::write(d.join("bad.png"), "not an image").unwrap();
        assert!(install_face(&d.join("bad.png"), &dest).unwrap_err().contains("not a picture"));
        assert!(install_face(&d.join("missing.png"), &dest).is_err());
        assert!(install_face(&d, &dest).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn users_items_show_login_note() {
        let mut h = Hub::with(std::sync::Arc::new(FakeRunner::lenient()), std::env::temp_dir().join("ms-usr-t"), std::sync::Arc::new(|| {}));
        h.users.data = Some(Users { name: "a".into(), display_manager: Some("sddm".into()), groups: vec!["wheel".into()], ..Users::default() });
        let t = format!("{:?}", users_items(&h));
        assert!(t.contains("sddm") && t.contains("wheel") && t.contains("s:usr:setface:"));
    }
}
