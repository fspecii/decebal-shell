//! The panes: what each one shows (as [`Item`]s built from the model) and how the special,
//! non-schema controls (lists, wallpaper picks, shortcuts, buttons) change files.

use super::binds::{self, BindRow, Src};
use super::icons::Glyph;
use super::model::{cap, rid, CtlVal, Model};
use super::system::glyphs;
use super::tomlcfg::Val;
use super::ui::{Ctl, Item, Kind, LayoutTile, Opt, Row, Thumb};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct PaneDef {
    pub id: &'static str,
    pub title: &'static str,
    pub glyph: Glyph,
    pub color: (u8, u8, u8),
    /// Words the sidebar search also matches.
    pub keywords: &'static str,
}

pub const PANES: &[PaneDef] = &[
    PaneDef { id: "general", title: "General", glyph: Glyph::Gear, color: (142, 142, 147), keywords: "about version config file reload editor backup restore tags chord timeout" },
    PaneDef {
        id: "appearance",
        title: "Appearance",
        glyph: Glyph::Brush,
        color: (88, 86, 214),
        keywords: "titlebar decoration style classic macos height radius border colors traffic lights zoom button double click font bar",
    },
    PaneDef {
        id: "desktop",
        title: "Desktop & Dock",
        glyph: Glyph::Dock,
        color: (10, 132, 255),
        keywords: "dock magnification size spacing transparency roundness pinned apps menu bar workspaces macdock macbar",
    },
    PaneDef { id: "wallpaper", title: "Wallpaper", glyph: Glyph::Photo, color: (50, 173, 230), keywords: "background image fill fit stretch center tile color picture" },
    PaneDef {
        id: "tiling",
        title: "Tiling & Layouts",
        glyph: Glyph::Tiles,
        color: (255, 159, 10),
        keywords: "layout tile master stack nmaster mfact gaps smart borders bsp spiral grid monocle floating per tag",
    },
    PaneDef {
        id: "windows",
        title: "Windows & Snapping",
        glyph: Glyph::Windows,
        color: (48, 209, 88),
        keywords: "focus follows mouse snap zones activate pertag size hints fullscreen preview retile shift",
    },
    PaneDef { id: "rules", title: "Rules", glyph: Glyph::Rules, color: (255, 69, 58), keywords: "window rules class instance title floating tag monitor sticky decorate" },
    PaneDef { id: "keyboard", title: "Shortcuts", glyph: Glyph::Keyboard, color: (99, 99, 102), keywords: "shortcuts keys bindings hotkeys modes chords record conflicts bind" },
    PaneDef { id: "autostart", title: "Autostart & Plugins", glyph: Glyph::Rocket, color: (191, 90, 242), keywords: "autostart startup plugins scratchpad commands programs" },
    PaneDef { id: "displays", title: "Displays", glyph: Glyph::Monitor, color: (100, 210, 255), keywords: "monitors outputs screens resolution scale per output wallpaper" },
    PaneDef {
        id: "theme",
        title: "Theme",
        glyph: Glyph::Brush,
        color: (255, 55, 95),
        keywords: "theme appearance dark light auto mode accent colour color swatches transparency translucency corner radius font text size dracula nord solarized windows graphite high contrast",
    },
    // system panes (src/settings/system); the sidebar groups come from system::registry::SECTIONS
    PaneDef { id: "sound", title: "Sound", glyph: Glyph::Sys(glyphs::SPEAKER), color: (255, 69, 58), keywords: "audio speakers microphone volume output input mute pipewire pulseaudio" },
    PaneDef { id: "network", title: "Network", glyph: Glyph::Sys(glyphs::WIFI), color: (10, 132, 255), keywords: "wifi wi-fi wireless ethernet vpn internet ip dns networkmanager nmcli" },
    PaneDef { id: "bluetooth", title: "Bluetooth", glyph: Glyph::Sys(glyphs::BLUETOOTH), color: (10, 132, 255), keywords: "pair devices headset mouse keyboard discoverable bluez" },
    PaneDef { id: "notifications", title: "Notifications", glyph: Glyph::Sys(glyphs::BELL), color: (255, 59, 48), keywords: "do not disturb dnd banner mute alerts macnotify" },
    PaneDef { id: "inputdev", title: "Keyboard & Input", glyph: Glyph::Sys(glyphs::MOUSE), color: (99, 99, 102), keywords: "layout repeat caps lock pointer mouse touchpad scrolling tap speed xkb" },
    PaneDef { id: "power", title: "Power & Battery", glyph: Glyph::Sys(glyphs::BATTERY), color: (48, 209, 88), keywords: "battery profile performance saver screen blank sleep dpms lid suspend" },
    PaneDef { id: "defaults", title: "Default Apps", glyph: Glyph::Sys(glyphs::APPS), color: (255, 159, 10), keywords: "browser mail terminal file manager media player editor pdf xdg" },
    PaneDef { id: "datetime", title: "Date & Time", glyph: Glyph::Sys(glyphs::CLOCK), color: (142, 142, 147), keywords: "timezone clock 24 hour ntp format menu bar" },
    PaneDef { id: "users", title: "Users & Login", glyph: Glyph::Sys(glyphs::USER), color: (100, 210, 255), keywords: "account avatar picture shell groups login screen" },
    PaneDef { id: "privacy", title: "Privacy & Security", glyph: Glyph::Sys(glyphs::LOCK), color: (88, 86, 214), keywords: "lock screen locker timeout suspend encryption" },
    PaneDef { id: "storage", title: "About & Storage", glyph: Glyph::Sys(glyphs::INFO), color: (142, 142, 147), keywords: "system info cpu gpu ram disk usage versions kernel distro check config" },
    PaneDef { id: "accessibility", title: "Accessibility", glyph: Glyph::Sys(glyphs::ACCESS), color: (10, 132, 255), keywords: "cursor size reduce motion text size contrast font large" },
];

pub fn pane_matches(p: &PaneDef, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    q.is_empty() || p.title.to_lowercase().contains(&q) || p.keywords.contains(&q)
}

// ---------------------------------------------------------------- generic table editor

#[derive(Clone)]
pub enum Ft {
    Text,
    /// Comma separated list of strings.
    List,
    /// Integer shown as `stored + offset`; shown 0 = unset.
    Int {
        min: i64,
        max: i64,
        offset: i64,
    },
    Float {
        min: f64,
        max: f64,
        step: f64,
    },
    /// true / false / unset
    Bool3,
    Bool,
    Choice(Vec<Opt>),
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub ft: Ft,
}

pub struct TableSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub noun: &'static str,
    pub fields: Vec<Field>,
    pub new: Vec<(&'static str, Val)>,
    pub footer: &'static str,
}

fn f(key: &'static str, label: &'static str, ft: Ft) -> Field {
    Field { key, label, ft }
}

fn button(id: String, label: &str, danger: bool, enabled: bool) -> Ctl {
    let mut c = Ctl::new(id, Kind::Button { label: label.into(), danger });
    c.enabled = enabled;
    c
}

pub fn expand_tilde(p: &str) -> String {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) => format!("{}/{rest}", h.to_string_lossy().trim_end_matches('/')),
        _ => p.to_string(),
    }
}

/// What a hot corner can do: label in the dropdown, action name in `macshell.json`.
const HOT_ACTIONS: [(&str, &str); 9] = [
    ("None", "none"),
    ("Dashboard", "dashboard"),
    ("Spotlight", "spotlight"),
    ("Launchpad", "launchpad"),
    ("Overview", "overview"),
    ("Session menu", "session"),
    ("Lock screen", "lock"),
    ("Tile all windows", "tile-all"),
    ("Notification history", "notifications"),
];

/// Under the corner rows: whether the daemon that applies them is running.
fn hot_corner_note() -> String {
    let pid = std::fs::read_to_string(crate::single::runtime_file(&format!("{}.lock", crate::hot::LOCK_NAME))).ok().and_then(|t| t.trim().parse::<u32>().ok());
    if pid.is_some_and(|p| std::path::Path::new(&format!("/proc/{p}")).exists()) {
        "Changes apply at once. Nothing fires while a mouse button is held, so dragging a window to a corner is safe.".into()
    } else {
        "machot is not running (X11 only): start it with `machot daemon` or enable its autostart entry. Overview needs macexpose.".into()
    }
}

fn opts(list: &[(&str, &str)]) -> Vec<Opt> {
    list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

impl Model {
    fn layout_choices(&self) -> Vec<Opt> {
        let mut v = vec![("(unchanged)".to_string(), String::new())];
        v.extend(self.schema.layouts.iter().map(|(n, _)| (n.clone(), n.clone())));
        v
    }

    pub fn table_spec(&self, name: &str) -> Option<TableSpec> {
        Some(match name {
            "rule" => TableSpec {
                name: "rule",
                title: "Window rules",
                noun: "rule",
                fields: vec![
                    f("class", "Class contains", Ft::Text),
                    f("instance", "Instance contains", Ft::Text),
                    f("title", "Title contains", Ft::Text),
                    f("tag", "Open on tag", Ft::Int { min: 0, max: 31, offset: 0 }),
                    f("floating", "Floating", Ft::Bool3),
                    f("fullscreen", "Fullscreen", Ft::Bool3),
                    f("sticky", "On all tags", Ft::Bool3),
                    f("decorate", "Titlebar", Ft::Bool3),
                    f("monitor", "Monitor (1 = first)", Ft::Int { min: 0, max: 8, offset: 1 }),
                    f("layout", "Switch tag to layout", Ft::Choice(self.layout_choices())),
                ],
                new: vec![("class", Val::Str("NewApp".into()))],
                footer: "A window matches when every filled-in field is contained in its class, instance or title. Matching rules accumulate.",
            },
            "autostart" => TableSpec {
                name: "autostart",
                title: "Start at login",
                noun: "program",
                fields: vec![f("command", "Command", Ft::Text), f("args", "Arguments (comma separated)", Ft::List)],
                new: vec![("command", Val::Str("my-program".into()))],
                footer: "Without arguments the command runs through the shell (so `macbar &` works); with arguments it is started directly. Runs once when rdwm starts.",
            },
            "plugin" => TableSpec {
                name: "plugin",
                title: "Plugins",
                noun: "plugin",
                fields: vec![f("command", "Command", Ft::Text), f("args", "Arguments (comma separated)", Ft::List), f("restart", "Restart when it exits", Ft::Bool)],
                new: vec![("command", Val::Str("my-plugin".into()))],
                footer: "Plugins talk to rdwm over its IPC socket and are restarted with back-off when they crash.",
            },
            "scratchpad" => TableSpec {
                name: "scratchpad",
                title: "Scratchpads",
                noun: "scratchpad",
                fields: vec![
                    f("name", "Name", Ft::Text),
                    f("command", "Command", Ft::Text),
                    f("class", "Window class", Ft::Text),
                    f("instance", "Window instance", Ft::Text),
                    f("title", "Window title", Ft::Text),
                ],
                new: vec![("name", Val::Str("term".into())), ("command", Val::Str("foot".into())), ("class", Val::Str("foot".into()))],
                footer: "Toggle with the togglescratch action and the scratchpad's name.",
            },
            "output" => TableSpec {
                name: "output",
                title: "Outputs (Wayland)",
                noun: "output",
                fields: vec![
                    f("name", "Connector", Ft::Text),
                    f("mode", "Mode (e.g. 2560x1600@60)", Ft::Text),
                    f("scale", "Scale", Ft::Float { min: 0.25, max: 8.0, step: 0.25 }),
                    f("transform", "Transform", Ft::Choice(opts(&[("(normal)", ""), ("90", "90"), ("180", "180"), ("270", "270"), ("Flipped", "flipped")]))),
                    f("enabled", "Enabled", Ft::Bool),
                ],
                new: vec![("name", Val::Str("DP-1".into()))],
                footer: "Only used by rdwm's Wayland backend.",
            },
            "wallpaper.output" => TableSpec {
                name: "wallpaper.output",
                title: "Per-output wallpapers",
                noun: "output wallpaper",
                fields: vec![
                    f("name", "Connector name", Ft::Text),
                    f("match", "or match size (WxH or *)", Ft::Text),
                    f("image", "Image", Ft::Text),
                    f("mode", "Mode", Ft::Choice(opts(&[("(default)", ""), ("Fill", "fill"), ("Fit", "fit"), ("Stretch", "stretch"), ("Center", "center"), ("Tile", "tile")]))),
                    f("color", "Background colour", Ft::Text),
                ],
                new: vec![("match", Val::Str("*".into()))],
                footer: "Later entries override earlier ones; an empty image means a plain colour on that output.",
            },
            _ => return None,
        })
    }

    fn entry_summary(&self, spec: &TableSpec, i: usize) -> (String, String) {
        let g = |k: &str| self.store.cfg.array_get(spec.name, i, k);
        let s = |k: &str| g(k).and_then(|v| v.as_str().map(String::from)).filter(|x| !x.is_empty());
        let label = ["name", "class", "instance", "title", "match", "command"]
            .iter()
            .find_map(|k| s(k).map(|v| if *k == "command" { v } else { format!("{k}: {v}") }))
            .unwrap_or_else(|| format!("({} {})", spec.noun, i + 1));
        let mut eff = Vec::new();
        for fld in &spec.fields {
            if let Some(v) = g(fld.key) {
                if matches!(fld.key, "class" | "instance" | "title" | "name" | "match" | "command") {
                    continue;
                }
                eff.push(match v {
                    Val::Bool(b) => format!("{} {}", fld.label.to_lowercase(), if b { "on" } else { "off" }),
                    Val::Int(n) => format!("{} {}", fld.label.to_lowercase(), n + if let Ft::Int { offset, .. } = fld.ft { offset } else { 0 }),
                    Val::Float(n) => format!("{} {n}", fld.label.to_lowercase()),
                    Val::Str(t) => format!("{}: {t}", fld.label.to_lowercase()),
                    Val::List(l) => format!("{}: {}", fld.label.to_lowercase(), l.join(" ")),
                });
            }
        }
        (label, eff.join(" · "))
    }

    fn field_ctl(&self, spec: &TableSpec, i: usize, fld: &Field) -> Ctl {
        let id = format!("t:{}:{}:{}", spec.name, i, fld.key);
        let cur = self.store.cfg.array_get(spec.name, i, fld.key);
        let kind = match &fld.ft {
            Ft::Text => Kind::Text { val: cur.and_then(|v| v.as_str().map(String::from)).unwrap_or_default(), hint: String::new() },
            Ft::List => Kind::Text { val: if let Some(Val::List(l)) = cur { l.join(", ") } else { String::new() }, hint: "a, b".into() },
            Ft::Int { min, max, offset } => {
                let v = cur.and_then(|v| v.as_i64()).map(|n| n + offset).unwrap_or(0);
                Kind::Stepper { v: v as f64, min: *min as f64, max: *max as f64, step: 1.0, unit: if v == 0 { " (any)".into() } else { String::new() } }
            }
            Ft::Float { min, max, step } => Kind::Stepper { v: cur.and_then(|v| v.as_f64()).unwrap_or(1.0), min: *min, max: *max, step: *step, unit: String::new() },
            Ft::Bool3 => {
                let o = opts(&[("Unchanged", ""), ("Yes", "yes"), ("No", "no")]);
                let sel = match cur.and_then(|v| v.as_bool()) {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                };
                Kind::Seg(o, Some(sel))
            }
            Ft::Bool => Kind::Toggle(cur.and_then(|v| v.as_bool()).unwrap_or(spec.name == "plugin" || spec.name == "output")),
            Ft::Choice(o) => {
                let c = cur.and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
                Kind::Popup(o.clone(), o.iter().position(|x| x.1 == c))
            }
        };
        Ctl::new(id, kind)
    }

    /// Header, list of entries with the selected one expanded, and an add button.
    pub fn table_items(&self, spec: &TableSpec, extra_add: Option<Ctl>) -> Vec<Item> {
        let n = self.store.cfg.array_len(spec.name);
        let sel = self.sel.table.get(spec.name).copied().filter(|s| *s < n);
        let mut rows = Vec::new();
        for i in 0..n {
            let (label, detail) = self.entry_summary(spec, i);
            let open = sel == Some(i);
            let mut r = Row::new(label).detail(detail).ctl(Ctl::new(format!("t:{}:{}:@sel", spec.name, i), Kind::Button { label: if open { "Done".into() } else { "Edit".into() }, danger: false }));
            r.selected = open;
            rows.push(r);
            if open {
                for fld in &spec.fields {
                    rows.push(Row::new(fld.label).ctl(self.field_ctl(spec, i, fld)));
                }
                let mut tools = Row::new("");
                tools = tools.ctl(button(format!("t:{}:{}:@up", spec.name, i), "Move up", false, i > 0));
                tools = tools.ctl(button(format!("t:{}:{}:@down", spec.name, i), "Move down", false, i + 1 < n));
                tools = tools.ctl(button(format!("t:{}:{}:@del", spec.name, i), &format!("Delete {}", spec.noun), true, true));
                rows.push(tools);
            }
        }
        let mut add = Row::new(if n == 0 { format!("No {}s yet", spec.noun) } else { String::new() })
            .ctl(Ctl::new(format!("t:{}:@add", spec.name), Kind::Button { label: format!("Add {}", spec.noun), danger: false }));
        if let Some(e) = extra_add {
            add = add.ctl(e);
        }
        rows.push(add);
        vec![Item::Header(spec.title.into()), Item::Card(rows), Item::Note { text: spec.footer.into(), warn: false }]
    }

    // ---------------------------------------------------------------- pane builders

    /// Lazy data the panes need (called before painting a pane).
    pub fn prepare(&mut self, pane: usize) {
        if let Some(p) = PANES.get(pane) {
            self.sys_prepare(p.id);
        }
        match PANES.get(pane).map(|p| p.id) {
            Some("desktop") if self.apps.is_none() => self.apps = Some(installed_apps()),
            Some("wallpaper") if self.walls.is_none() => self.walls = Some(list_images(&self.paths.backgrounds(), 3, 400)),
            _ => {}
        }
    }

    fn banners(&self) -> Vec<Item> {
        let mut v = Vec::new();
        if let Some(e) = &self.file_error {
            v.push(Item::Note { text: format!("config.toml is not valid right now (edited elsewhere?): {e}. Showing the last good version; changes here are paused until it parses."), warn: true });
        }
        if let Some(n) = self.notice.as_ref().filter(|n| n.error) {
            v.push(Item::Note { text: n.text.clone(), warn: true });
        }
        if let Some(e) = &self.rt.error {
            v.push(Item::Note { text: format!("rdwm kept its previous configuration because the last reload failed: {e}"), warn: true });
        }
        v
    }

    pub fn items(&self, pane: usize) -> Vec<Item> {
        let mut items = self.banners();
        let id = PANES.get(pane).map(|p| p.id).unwrap_or("general");
        if let Some(v) = self.sys_items(id) {
            items.extend(v);
            return items;
        }
        items.extend(match id {
            "theme" => super::themepane::items(self),
            "general" => self.pane_general(),
            "appearance" => self.pane_appearance(),
            "desktop" => self.pane_desktop(),
            "wallpaper" => self.pane_wallpaper(),
            "tiling" => self.pane_tiling(),
            "windows" => self.pane_windows(),
            "rules" => self.pane_rules(),
            "keyboard" => self.pane_keyboard(),
            "autostart" => self.pane_autostart(),
            "displays" => {
                let mut v = self.sys_display_items();
                v.extend(self.pane_displays());
                v
            }
            _ => self.pane_displays(),
        });
        items
    }

    fn pane_general(&self) -> Vec<Item> {
        let btn = |id: &str, label: &str, danger: bool| Ctl::new(id, Kind::Button { label: label.into(), danger });
        let running = self.rt.running;
        let bak = self.store.path.with_file_name(format!("{}.{}", self.store.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), super::io::BACKUP_SUFFIX));
        let mut about = vec![
            Row::new("macsettings").ctl(Ctl::new("lbl:v", Kind::Label(env!("CARGO_PKG_VERSION").into()))),
            Row::new("Window manager")
                .detail(if running { "rdwm is running and follows every change instantly" } else { "rdwm is not running: changes are saved to the file and apply when it starts" })
                .ctl(Ctl::new("lbl:rdwm", Kind::Label(if running { format!("rdwm {}", self.rt.version.clone().unwrap_or_default()) } else { "not running".into() }))),
            Row::new("Configuration file").detail(self.store.path.display().to_string()),
            Row::new("Key table")
                .detail("Defaults and ranges are read from rdwm's parser")
                .ctl(Ctl::new("lbl:schema", Kind::Label(if self.schema.live { "from rdwmctl".into() } else { "bundled copy".into() }))),
        ];
        about.retain(|r| !r.label.is_empty());
        let mut act = vec![
            Row::new("Reload configuration").detail("Ask rdwm to re-read the file now").ctl(btn("a:reload", "Reload", false)),
            Row::new("Edit the file by hand").detail("Opens config.toml with your default editor; this window follows your edits").ctl(btn("a:open", "Open in editor", false)),
        ];
        if bak.exists() {
            act.push(Row::new("Restore original").detail("Put back the file as it was before macsettings first changed it").ctl(btn("a:restore", "Restore", true)));
        }
        let mut beh: Vec<Row> = Vec::new();
        beh.extend(self.r_toggle("general", "default_binds", "Built-in key bindings", "Off: only the bindings in your file exist"));
        beh.extend(self.r_stepper("general", "chord_timeout_ms", "Key sequence timeout", "How long a multi-key shortcut waits for its next key", 100.0, " ms", None));
        beh.extend(self.r_text("general", "tags", "Tag names", "Comma separated, 1 to 31 names", "1, 2, 3"));
        vec![Item::Header("About".into()), Item::Card(about), Item::Header("Configuration".into()), Item::Card(act), Item::Header("Behaviour".into()), Item::Card(beh)]
    }

    fn pane_appearance(&self) -> Vec<Item> {
        let mut deco = Vec::new();
        deco.extend(self.r_toggle("decoration", "enabled", "Window titlebars", "Draw server-side titlebars with traffic-light buttons"));
        deco.extend(self.r_seg("decoration", "style", "Style", "macOS: rounded, buttons on the left. Classic: flat dwm strip", Some(&[("macOS", "macos"), ("Classic", "classic")])));
        deco.extend(self.r_slider("decoration", "height", "Titlebar height", "", 1.0, " px", Some(64.0)));
        deco.extend(self.r_slider("decoration", "radius", "Corner radius", "macOS style only", 1.0, " px", Some(30.0)));
        deco.extend(self.r_seg("decoration", "title_align", "Title alignment", "", Some(&[("Center", "center"), ("Left", "left")])));
        deco.extend(self.r_stepper("decoration", "font_size", "Title font size", "", 0.5, " px", None));
        deco.extend(self.r_toggle("decoration", "buttons", "Window buttons", "Close, minimise and zoom"));
        deco.extend(self.r_popup(
            "decoration",
            "zoom_button",
            "Green button",
            "",
            Some(&[("Fullscreen", "fullscreen"), ("Maximize", "maximize"), ("Monocle", "monocle"), ("Float", "float"), ("Nothing", "none")]),
            None,
        ));
        deco.extend(self.r_popup(
            "decoration",
            "double_click",
            "Double-click on title",
            "",
            Some(&[("Maximize", "maximize"), ("Fullscreen", "fullscreen"), ("Monocle", "monocle"), ("Float", "float"), ("Nothing", "none")]),
            None,
        ));
        deco.extend(self.r_toggle("decoration", "middle_click_close", "Middle click closes", "Middle click on a title closes the window"));
        deco.extend(self.r_toggle("decoration", "in_monocle", "Titlebars in monocle", ""));
        let mut border = Vec::new();
        border.extend(self.r_slider("general", "border_px", "Window border", "Width around every window", 1.0, " px", Some(16.0)));
        border.extend(self.r_color("colors", "focused", "Focused border", ""));
        border.extend(self.r_color("colors", "normal", "Unfocused border", ""));
        border.extend(self.r_color("colors", "urgent", "Urgent border", ""));
        border.extend(self.r_slider("decoration", "border_px", "Titlebar border", "", 1.0, " px", Some(8.0)));
        let mut cols = Vec::new();
        cols.extend(self.r_color("decoration", "close_color", "Close button", ""));
        cols.extend(self.r_color("decoration", "minimize_color", "Minimise button", ""));
        cols.extend(self.r_color("decoration", "zoom_color", "Zoom button", ""));
        cols.extend(self.r_color("decoration", "inactive_button_color", "Buttons of unfocused windows", ""));
        cols.extend(self.r_color("decoration", "focused_bg", "Focused titlebar", "Empty = style default"));
        cols.extend(self.r_color("decoration", "focused_fg", "Focused title text", ""));
        cols.extend(self.r_color("decoration", "unfocused_bg", "Unfocused titlebar", ""));
        cols.extend(self.r_color("decoration", "unfocused_fg", "Unfocused title text", ""));
        let mut bar = Vec::new();
        bar.extend(self.r_toggle("bar", "show", "rdwm's own bar", "dwm-style bar drawn by the window manager. Leave off while macbar runs"));
        bar.extend(self.r_seg("bar", "top", "Position", "", None).map(|mut r| {
            if let Some(Ctl { kind: Kind::Seg(o, s), .. }) = r.ctls.first_mut() {
                *o = opts(&[("Top", "true"), ("Bottom", "false")]);
                *s = Some(if self.flag("bar", "top") { 0 } else { 1 });
            }
            r
        }));
        bar.extend(self.r_stepper("bar", "height", "Height", "Also the smallest a window can get", 1.0, " px", Some(80.0)));
        vec![
            Item::Header("Titlebars".into()),
            Item::Card(deco),
            Item::Header("Borders".into()),
            Item::Card(border),
            Item::Header("Colours".into()),
            Item::Card(cols),
            Item::Note { text: "Right-click a control to reset it to its default.".into(), warn: false },
            Item::Header("Built-in bar".into()),
            Item::Card(bar),
        ]
    }

    pub fn dock_row(&self, key: &str, label: &str, detail: &str, min: f64, max: f64, step: f64, def: f64) -> Row {
        let v = self.dock.get(key).and_then(Value::as_f64).unwrap_or(def).clamp(min, max);
        Row::new(label).detail(detail).ctl(Ctl::new(format!("j:dock:{key}"), Kind::Slider { v, min, max, step, unit: String::new() }))
    }

    fn pinned_list(&self) -> Vec<String> {
        let p: crate::config::Pinned = serde_json::from_value(self.pinned.clone()).unwrap_or_default();
        p.or_default(&|id| crate::launch::find_exec(id).is_some())
    }

    fn app_name(&self, id: &str) -> String {
        match id {
            "launchpad" => "Launchpad".into(),
            "trash" => "Trash".into(),
            _ => self.apps.as_ref().and_then(|a| a.iter().find(|x| x.0 == id)).map(|x| x.1.clone()).unwrap_or_else(|| id.to_string()),
        }
    }

    /// Four dropdowns (one per corner) and the dwell time: stored in `macshell.json` under `hot_corners`, picked up by `machot`.
    pub(super) fn hot_corner_rows(&self) -> Vec<Row> {
        use crate::hot::{Action, Corner, HotConfig};
        let cfg = HotConfig::from_json(&self.shell);
        let mut rows = Vec::new();
        for c in Corner::ALL {
            let cur = cfg.action(c).name();
            let mut choices: Vec<Opt> = HOT_ACTIONS.iter().map(|(label, name)| (label.to_string(), name.to_string())).collect();
            if let Action::Custom(cmd) = cfg.action(c) {
                choices.push((format!("Custom: {cmd}"), cur.clone()));
            }
            let sel = choices.iter().position(|o| o.1 == cur);
            rows.push(Row::new(c.label()).ctl(Ctl::new(format!("hc:{}", c.key()), Kind::Popup(choices, sel))));
        }
        rows.push(
            Row::new("Dwell time")
                .detail("How long the pointer rests in a corner before it acts")
                .ctl(Ctl::new("hc:dwell_ms", Kind::Slider { v: cfg.dwell_ms.min(1000) as f64, min: 0.0, max: 1000.0, step: 25.0, unit: " ms".into() })),
        );
        rows
    }

    fn pane_desktop(&self) -> Vec<Item> {
        let style = match self.dock.get("style").and_then(Value::as_str) {
            Some("windows") => 1,
            _ => 0,
        };
        let dock = vec![
            Row::new("Style")
                .detail("macOS dock, or a Windows 10/11 taskbar. Applies at once")
                .ctl(Ctl::new("j:dock:style", Kind::Seg(opts(&[("macOS", "mac"), ("Windows", "windows")]), Some(style)))),
            self.dock_row("appSpacing", "Icon spacing", "", 0.0, 24.0, 1.0, 6.0),
            self.dock_row("magnification", "Magnification", "1.0 = off", 1.0, 2.5, 0.05, 1.4),
            self.dock_row("transparency", "Transparency", "", 0.0, 1.0, 0.05, 0.4),
            self.dock_row("roundness", "Roundness", "", 0.0, 1.0, 0.05, 0.45),
            Row::new("Apply").detail("macdock reads these settings when it starts").ctl(Ctl::new("a:restart-dock", Kind::Button { label: "Restart Dock".into(), danger: false })),
        ];
        let ws = match self.shell.get("workspaces") {
            Some(Value::Bool(true)) => 1,
            Some(Value::Bool(false)) => 2,
            Some(Value::String(s)) if s == "on" => 1,
            Some(Value::String(s)) if s == "off" => 2,
            _ => 0,
        };
        let bar = vec![
            Row::new("Workspace dots")
                .detail("Show rdwm's tags in the menu bar. Auto shows them only under rdwm")
                .ctl(Ctl::new("j:shell:workspaces", Kind::Seg(opts(&[("Auto", "auto"), ("On", "on"), ("Off", "off")]), Some(ws)))),
            Row::new("Apply").detail("macbar reads this when it starts").ctl(Ctl::new("a:restart-bar", Kind::Button { label: "Restart Menu Bar".into(), danger: false })),
        ];
        let pins = self.pinned_list();
        let mut rows: Vec<Row> = Vec::new();
        for (i, id) in pins.iter().enumerate() {
            let mut r = Row::new(self.app_name(id)).detail(id.clone());
            r = r.ctl(button(format!("p:up:{i}"), "Up", false, i > 0));
            r = r.ctl(button(format!("p:down:{i}"), "Down", false, i + 1 < pins.len()));
            r = r.ctl(button(format!("p:del:{i}"), "Remove", true, true));
            rows.push(r);
        }
        let choices: Vec<Opt> = self.apps.as_ref().map(|a| a.iter().filter(|x| !pins.contains(&x.0)).map(|x| (x.1.clone(), x.0.clone())).collect()).unwrap_or_default();
        rows.push(Row::new("Add an app").ctl(Ctl::new("p:add", Kind::Popup(choices, None))));
        vec![
            Item::Header("Dock".into()),
            Item::Card(dock),
            Item::Header("Menu bar".into()),
            Item::Card(bar),
            Item::Header("Hot corners".into()),
            Item::Card(self.hot_corner_rows()),
            Item::Note { text: hot_corner_note(), warn: false },
            Item::Header("Pinned apps".into()),
            Item::Card(rows),
            Item::Note { text: "Settings are stored in the omarchy config folder next to dock-settings.json; unknown keys in those files are left alone.".into(), warn: false },
        ]
    }

    /// What is on screen: rdwm's live wallpaper when it runs (it may differ from the file after
    /// `rdwmctl wallpaper`), else the file's.
    pub fn current_wallpaper(&self) -> Option<String> {
        let live = if self.rt.running { self.rt.state["wallpaper"]["image"].as_str().map(String::from) } else { None };
        live.or_else(|| self.string("wallpaper", "image").filter(|s| !s.is_empty())).map(|p| expand_tilde(&p))
    }

    fn pane_wallpaper(&self) -> Vec<Item> {
        let cur = self.current_wallpaper();
        let mode = self.string("wallpaper", "mode").unwrap_or_else(|| "fill".into());
        let mut rows = Vec::new();
        rows.extend(self.r_seg("wallpaper", "mode", "Scaling", "", Some(&[("Fill", "fill"), ("Fit", "fit"), ("Stretch", "stretch"), ("Center", "center"), ("Tile", "tile")])));
        rows.extend(self.r_color("wallpaper", "color", "Background colour", "Behind the image: letterbox of Fit, Center and Tile"));
        rows.extend(self.r_text("wallpaper", "image", "Image file", "Type or paste a path to a PNG, JPEG or WebP file", "/path/to/image.png"));
        if cur.is_some() {
            rows.push(Row::new("Remove wallpaper").ctl(Ctl::new("w:clear", Kind::Button { label: "Remove".into(), danger: true })));
        }
        let mut items = vec![
            Item::Preview {
                id: cur.as_ref().map(|c| format!("{c}#big")).unwrap_or_default(),
                caption: cur
                    .as_deref()
                    .map(|p| format!("{} · {}", Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), cap(&mode)))
                    .unwrap_or_else(|| "No image set".into()),
            },
            Item::Space(6.0),
            Item::Card(rows),
        ];
        items.extend(super::wallpapercolors::wallpaper_pane_note(self));
        // changed through `rdwmctl wallpaper` since the last reload
        let live = self.rt.state["wallpaper"]["image"].as_str().map(expand_tilde);
        let filev = self.string("wallpaper", "image").map(|p| expand_tilde(&p));
        if self.rt.running && live.is_some() && live != filev {
            let live = live.as_deref();
            items.push(Item::Note { text: format!("rdwm currently shows {} (set with rdwmctl). The next reload goes back to the file.", live.unwrap_or_default()), warn: false });
            items.push(Item::Card(vec![Row::new("Keep this wallpaper").ctl(Ctl::new("w:save-live", Kind::Button { label: "Save to config".into(), danger: false }))]));
        }
        let walls = self.walls.clone().unwrap_or_default();
        let mut thumbs: Vec<Thumb> = Vec::new();
        if let Some(c) = &cur {
            if !walls.iter().any(|w| w.to_string_lossy() == c.as_str()) {
                thumbs.push(Thumb { id: c.clone(), caption: Path::new(c).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), selected: true });
            }
        }
        thumbs.extend(walls.iter().map(|w| {
            let id = w.to_string_lossy().into_owned();
            Thumb { selected: cur.as_deref() == Some(id.as_str()), caption: w.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), id }
        }));
        items.push(Item::Header(format!("Backgrounds ({})", thumbs.len())));
        if thumbs.is_empty() {
            items.push(Item::Note { text: format!("No pictures found in {}. Put images there, or type a path above.", self.paths.backgrounds().display()), warn: false });
        } else {
            items.push(Item::Thumbs(thumbs));
        }
        items
    }

    fn pane_tiling(&self) -> Vec<Item> {
        let order = self.list("general", "layouts");
        let default = order.first().cloned().unwrap_or_else(|| "tile".into());
        let tiles: Vec<LayoutTile> = self.schema.layouts.iter().map(|(n, s)| LayoutTile { id: format!("l:set:{n}"), name: n.clone(), symbol: s.clone(), selected: *n == default }).collect();
        self.used.borrow_mut().push(("general".into(), "layouts".into()));
        let mut master = Vec::new();
        master.extend(self.r_slider("general", "mfact", "Master area", "Share of the screen the master column gets", 0.01, "", None));
        master.extend(self.r_stepper("general", "nmaster", "Windows in master", "", 1.0, "", Some(12.0)));
        master.extend(self.r_popup(
            "general",
            "master_side",
            "Master side",
            "Restyles the plain tile layout",
            Some(&[("Left", "left"), ("Right", "right"), ("Top", "top"), ("Bottom", "bottom")]),
            Some("Default (left)"),
        ));
        master.extend(self.r_popup(
            "general",
            "stack_style",
            "Stack style",
            "",
            Some(&[("Vertical", "vertical"), ("Horizontal", "horizontal"), ("Deck", "deck"), ("Tabbed", "tabbed")]),
            Some("Default (vertical)"),
        ));
        let mut gaps = Vec::new();
        gaps.extend(self.r_slider("general", "gap_inner", "Gap between windows", "", 1.0, " px", Some(64.0)));
        gaps.extend(self.r_slider("general", "gap_outer", "Gap around the screen", "", 1.0, " px", Some(64.0)));
        gaps.extend(self.r_toggle("general", "smart_gaps", "Smart gaps", "No gaps around a lone window"));
        gaps.extend(self.r_toggle("general", "smart_borders", "Smart borders", "No border on a lone tiled window"));
        gaps.extend(self.r_toggle("general", "pertag", "Layout per tag", "Each tag remembers its own layout, master size and count"));
        // order list
        let mut ord_rows = Vec::new();
        for (i, n) in order.iter().enumerate() {
            let sym = self.schema.layouts.iter().find(|l| &l.0 == n).map(|l| l.1.clone()).unwrap_or_else(|| "plugin".into());
            let mut r = Row::new(n.clone()).detail(if i == 0 { format!("{sym}  ·  default") } else { sym });
            r = r.ctl(button(format!("l:up:{i}"), "Up", false, i > 0));
            r = r.ctl(button(format!("l:down:{i}"), "Down", false, i + 1 < order.len()));
            r = r.ctl(button(format!("l:del:{i}"), "Remove", true, order.len() > 1));
            ord_rows.push(r);
        }
        let addable: Vec<Opt> = self.schema.layouts.iter().filter(|l| !order.contains(&l.0)).map(|l| (format!("{}  {}", l.0, l.1), l.0.clone())).collect();
        ord_rows.push(Row::new("Add a layout to the cycle").ctl(Ctl::new("l:add", Kind::Popup(addable, None))));
        // per tag
        let tags = self.list("general", "tags");
        let mut tag_rows = Vec::new();
        let mut lopts: Vec<Opt> = vec![("Default".into(), String::new())];
        lopts.extend(self.schema.layouts.iter().map(|l| (l.0.clone(), l.0.clone())));
        for (i, name) in tags.iter().enumerate().take(12) {
            let cur = (0..self.store.cfg.array_len("tag_layout"))
                .find(|&j| self.store.cfg.array_get("tag_layout", j, "tag").and_then(|v| v.as_i64()) == Some(i as i64 + 1))
                .and_then(|j| self.store.cfg.array_get("tag_layout", j, "layout"))
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            let sel = lopts.iter().position(|o| o.1 == cur);
            tag_rows.push(Row::new(format!("Tag {name}")).ctl(Ctl::new(format!("l:tag:{}", i + 1), Kind::Popup(lopts.clone(), sel))));
        }
        // per-layout options
        let target = self.sel.layout_opt.clone().unwrap_or_else(|| default.clone());
        let lo = |k: &str| self.store.cfg.get(&format!("layout.{target}"), k);
        let side_opts = opts(&[("(inherit)", ""), ("Left", "left"), ("Right", "right"), ("Top", "top"), ("Bottom", "bottom")]);
        let stack_opts = opts(&[("(inherit)", ""), ("Vertical", "vertical"), ("Horizontal", "horizontal"), ("Deck", "deck"), ("Tabbed", "tabbed")]);
        let pos = |o: &[Opt], v: Option<Val>| {
            let s = v.and_then(|x| x.as_str().map(String::from)).unwrap_or_default();
            o.iter().position(|x| x.1 == s)
        };
        let all: Vec<Opt> = self.schema.layouts.iter().map(|l| (l.0.clone(), l.0.clone())).collect();
        let lorows = vec![
            Row::new("Layout").ctl(Ctl::new("lo:@pick", Kind::Popup(all.clone(), all.iter().position(|o| o.1 == target)))),
            Row::new("Master side").ctl(Ctl::new(format!("lo:{target}:master_side"), Kind::Popup(side_opts.clone(), pos(&side_opts, lo("master_side"))))),
            Row::new("Stack style").ctl(Ctl::new(format!("lo:{target}:stack_style"), Kind::Popup(stack_opts.clone(), pos(&stack_opts, lo("stack_style"))))),
            Row::new("Inner gap")
                .detail("Overrides the general gap for this layout; 0 = inherit")
                .ctl(Ctl::new(format!("lo:{target}:gap_inner"), Kind::Stepper { v: lo("gap_inner").and_then(|v| v.as_f64()).unwrap_or(0.0), min: 0.0, max: 64.0, step: 1.0, unit: " px".into() })),
        ];
        vec![
            Item::Header("Default layout".into()),
            Item::Layouts(tiles),
            Item::Note { text: "Click a layout to make it the default for new tags. The first entry of the cycle list below is the default.".into(), warn: false },
            Item::Header("Master and stack".into()),
            Item::Card(master),
            Item::Header("Gaps and borders".into()),
            Item::Card(gaps),
            Item::Header("Cycle order".into()),
            Item::Card(ord_rows),
            Item::Header("Layout per tag".into()),
            Item::Card(tag_rows),
            Item::Header("Options of one layout".into()),
            Item::Card(lorows),
        ]
    }

    fn pane_windows(&self) -> Vec<Item> {
        let mut focus = Vec::new();
        focus.extend(self.r_toggle("general", "focus_follows_mouse", "Focus follows mouse", "Hovering a window focuses it"));
        focus.extend(self.r_toggle("general", "activate_focuses", "Activation focuses", "A window asking for attention is focused instead of only marked urgent"));
        focus.extend(self.r_toggle("general", "lock_fullscreen", "Keep fullscreen focus", "Focus keys do nothing while a window is fullscreen"));
        focus.extend(self.r_toggle("general", "resize_hints", "Respect size hints", "Tiled windows keep the size they ask for (terminals snap to whole characters)"));
        let mut snap = Vec::new();
        snap.extend(self.r_toggle("snap", "enabled", "Window snapping", "Snap zones, edge snapping and previews while dragging. Follows titlebars unless set"));
        if self.schema.has("snap", "activation") {
            snap.extend(self.r_seg("snap", "activation", "Snap windows", "", Some(&[("Always", "always"), ("Only while holding Shift", "shift")])));
        }
        snap.extend(self.r_toggle("snap", "zones", "Screen-edge zones", "Drag to an edge for halves, to a corner for quarters, to the top to maximise"));
        snap.extend(self.r_popup("snap", "top", "Top edge", "", Some(&[("Maximize", "maximize"), ("Nothing", "none")]), None));
        snap.extend(self.r_toggle("snap", "windows", "Snap to other windows", "Edges align with neighbouring windows"));
        snap.extend(self.r_slider("snap", "distance", "Window snap distance", "", 1.0, " px", Some(64.0)));
        snap.extend(self.r_toggle("snap", "gaps", "Keep gaps when snapping", ""));
        snap.extend(self.r_slider("snap", "edge", "Edge zone thickness", "", 1.0, " px", Some(32.0)));
        snap.extend(self.r_slider("snap", "corner", "Corner zone length", "", 4.0, " px", Some(256.0)));
        snap.extend(self.r_toggle("snap", "restore", "Restore size when dragged away", ""));
        snap.extend(self.r_popup(
            "snap",
            "retile_on_drop",
            "Dropping a tiled window",
            "Where a tiled window goes when released over the tiles",
            Some(&[("Swap with window", "swap"), ("Insert", "insert"), ("Stay floating", "float"), ("Off", "off")]),
            None,
        ));
        let mut prev = Vec::new();
        prev.extend(self.r_toggle("snap", "preview", "Zone preview", "Translucent outline while dragging"));
        prev.extend(self.r_color("snap", "preview_color", "Preview colour", ""));
        prev.extend(self.r_slider("snap", "preview_border", "Preview outline", "", 1.0, " px", None));
        let mut edge = Vec::new();
        edge.extend(self.r_stepper("general", "snap", "Edge snap while dragging", "Distance at which windows stick to the screen edge or float", 4.0, " px", Some(256.0)));
        vec![
            Item::Header("Focus".into()),
            Item::Card(focus),
            Item::Header("Snapping".into()),
            Item::Card(snap),
            Item::Header("Preview".into()),
            Item::Card(prev),
            Item::Header("Dragging".into()),
            Item::Card(edge),
        ]
    }

    fn pane_rules(&self) -> Vec<Item> {
        let spec = self.table_spec("rule").expect("rule spec");
        let mut classes: Vec<String> = self.rt.state["windows"].as_array().map(|w| w.iter().filter_map(|x| x["class"].as_str().map(String::from)).collect()).unwrap_or_default();
        classes.sort();
        classes.dedup();
        let extra = (!classes.is_empty()).then(|| Ctl::new("t:rule:@addwin", Kind::Popup(classes.into_iter().map(|c| (c.clone(), c)).collect(), None)));
        let mut items = self.table_items(&spec, extra);
        items.push(Item::Note { text: "Tip: \"Add rule\" creates a rule you can edit; the menu next to it starts from a window that is open right now.".into(), warn: false });
        items
    }

    fn pane_autostart(&self) -> Vec<Item> {
        let mut v = Vec::new();
        for n in ["autostart", "plugin", "scratchpad"] {
            v.extend(self.table_items(&self.table_spec(n).expect("spec"), None));
        }
        v
    }

    fn pane_displays(&self) -> Vec<Item> {
        let mut items = Vec::new();
        let mons = self.rt.state["monitors"].as_array().cloned().unwrap_or_default();
        if !self.rt.running {
            items.push(Item::Note { text: "rdwm is not running, so no live display information is available.".into(), warn: false });
        }
        let mut rows = Vec::new();
        for m in &mons {
            let wa = &m["workarea"];
            let size = format!("{} × {}", wa["w"], wa["h"]);
            let at = format!("at {}, {}", wa["x"], wa["y"]);
            let layout = m["layout"].as_str().unwrap_or("?");
            rows.push(
                Row::new(format!("Display {}", m["id"]))
                    .detail(format!("{at} · layout {layout} {}", m["symbol"].as_str().unwrap_or("")))
                    .ctl(Ctl::new(format!("lbl:mon{}", m["id"]), Kind::Label(size))),
            );
        }
        if !rows.is_empty() {
            items.push(Item::Header("Connected displays".into()));
            items.push(Item::Card(rows));
        }
        if let Some(w) = self.rt.state["windows"].as_array() {
            items.push(Item::Card(vec![Row::new("Open windows").ctl(Ctl::new("lbl:win", Kind::Label(w.len().to_string())))]));
        }
        let wp = &self.rt.state["wallpaper"];
        if wp.is_object() {
            items.push(Item::Header("Wallpaper right now".into()));
            items.push(Item::Card(vec![Row::new(
                wp["image"].as_str().map(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or_else(|| "No image".into()),
            )
            .detail(format!("mode {} · colour {}", wp["mode"].as_str().unwrap_or("fill"), wp["color"].as_str().unwrap_or("")))]));
        }
        items.extend(self.table_items(&self.table_spec("wallpaper.output").expect("spec"), None));
        items.extend(self.table_items(&self.table_spec("output").expect("spec"), None));
        items
    }

    // ---------------------------------------------------------------- keyboard

    pub fn bind_rows(&self) -> Vec<BindRow> {
        binds::collect(&self.store.cfg, &self.rt.binds)
    }

    fn pane_keyboard(&self) -> Vec<Item> {
        let rows = self.bind_rows();
        let filter = self.sel.bind_filter.to_lowercase();
        let mut cards = Vec::new();
        let conflicts = binds::conflicts(&rows);
        let mut items = Vec::new();
        if !conflicts.is_empty() {
            items.push(Item::Note { text: format!("Conflicts: {}", conflicts.join("; ")), warn: true });
        }
        cards.push(Row::new("Search shortcuts").ctl(Ctl::new("k:filter", Kind::Text { val: self.sel.bind_filter.clone(), hint: "action or keys".into() })));
        let rec = self.sel.recording.as_deref();
        if rec.is_some() {
            let on = |m: &str| self.sel.rec_mods.contains(&m);
            let mut r = Row::new("Modifiers").detail("Toggle modifiers, then press the key. Esc cancels.");
            for m in ["super", "ctrl", "alt", "shift"] {
                r = r.ctl(Ctl::new(format!("mod:{m}"), Kind::Chip { label: cap(m), on: on(m) }));
            }
            cards.push(r);
        }
        let mut shown = Vec::new();
        let mut last_mode = String::new();
        for (n, b) in rows.iter().enumerate() {
            let text = format!("{} {} {} {}", b.keys, b.action, b.args.join(" "), b.mode).to_lowercase();
            if !filter.is_empty() && !text.contains(&filter) {
                continue;
            }
            if b.mode != last_mode {
                if !shown.is_empty() {
                    items.push(Item::Card(std::mem::take(&mut shown)));
                    items.push(Item::Header(format!("Mode: {}", b.mode)));
                }
                last_mode = b.mode.clone();
            }
            let label = if b.args.is_empty() { b.action.clone() } else { format!("{} {}", b.action, b.args.join(" ")) };
            let detail = match &b.src {
                Src::Default => "built-in".to_string(),
                Src::File(_) => "your config".into(),
                Src::Ipc => "registered by a program (temporary)".into(),
                Src::Mode => format!("mode {}", b.mode),
            };
            let mut r = Row::new(label).detail(detail);
            let editable = matches!(b.src, Src::File(_) | Src::Default);
            let kid = format!("k:{n}");
            let mut kc = Ctl::new(kid, Kind::Key(b.keys.clone()));
            kc.enabled = editable;
            r = r.ctl(kc);
            if editable {
                r = r.ctl(Ctl::new(format!("k:del:{n}"), Kind::Button { label: "Remove".into(), danger: true }));
            }
            shown.push(r);
        }
        if shown.is_empty() {
            shown.push(Row::new(if rows.is_empty() { "No shortcuts known (start rdwm to see the built-in ones)" } else { "No shortcut matches" }));
        }
        let mut head = vec![Item::Card(cards)];
        head.append(&mut items);
        let mut out = head;
        out.push(Item::Header("Shortcuts".into()));
        out.push(Item::Card(shown));
        // add new
        let nb = &self.sel.new_bind;
        let action_opts: Vec<Opt> = self.schema.actions.iter().map(|a| (a.clone(), a.clone())).collect();
        let asel = action_opts.iter().position(|o| o.1 == nb.action);
        out.push(Item::Header("New shortcut".into()));
        out.push(Item::Card(vec![
            Row::new("Keys").ctl(Ctl::new("k:new:keys", Kind::Key(if nb.keys.is_empty() { "Record…".into() } else { binds::nice(&nb.keys) }))),
            Row::new("Action").ctl(Ctl::new("k:new:action", Kind::Popup(action_opts, asel))),
            Row::new("Arguments").detail("Separated by spaces, e.g. foot -e htop for spawn").ctl(Ctl::new("k:new:args", Kind::Text { val: nb.args.clone(), hint: "optional".into() })),
            Row::new("").ctl(Ctl::new("k:new:add", Kind::Button { label: "Add shortcut".into(), danger: false })),
        ]));
        out.push(Item::Note {
            text: "Changes apply immediately. Chord sequences (Super+x then r) and mode bindings from your file are listed too; sequences and modes are edited in the file for now.".into(),
            warn: false,
        });
        out
    }
}

// ---------------------------------------------------------------- file helpers

pub fn list_images(dir: &Path, depth: usize, limit: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, limit: usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= limit {
                return;
            }
            let p = e.path();
            let Ok(ft) = std::fs::metadata(&p).map(|m| m.file_type()) else { continue };
            if ft.is_dir() && depth > 0 {
                walk(&p, depth - 1, out, limit);
            } else if ft.is_file() && p.extension().and_then(|x| x.to_str()).is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, depth, &mut out, limit);
    out
}

/// (desktop file id, name) of every visible application.
pub fn installed_apps() -> Vec<(String, String)> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(h).join("applications"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|s| !s.is_empty()).map(|d| PathBuf::from(d).join("applications")));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let id = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(entry) = std::fs::read_to_string(&p).ok().and_then(|t| crate::apps::parse_entry(&t)) {
                out.push((id, entry.name));
            }
        }
    }
    out.sort_by_key(|a| a.1.to_lowercase());
    out
}

/// SIGTERM every process of ours called `name`, then start it again.
fn restart_process(name: &str) -> bool {
    let me = unsafe { libc::getuid() };
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
            let comm = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default();
            let uid = std::fs::metadata(e.path()).map(|m| std::os::unix::fs::MetadataExt::uid(&m)).unwrap_or(u32::MAX);
            if comm.trim() == name && uid == me && pid > 1 {
                // SAFETY: plain kill(2) on a process we own, matched by exact name.
                unsafe { libc::kill(pid, libc::SIGTERM) };
            }
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(250));
    super::io::spawn_detached(name, &[])
}

// ---------------------------------------------------------------- special controls

fn parse_tid(id: &str) -> Option<(String, String, String)> {
    // t:<name>:<i|@op>:<key>   (name may contain a dot but never a colon)
    let mut it = id.splitn(4, ':');
    it.next()?;
    Some((it.next()?.to_string(), it.next()?.to_string(), it.next().unwrap_or("").to_string()))
}

/// A hot corner dropdown or the dwell slider: writes `macshell.json`; the running machot notices the file at once.
fn apply_hot_corner(m: &mut Model, key: &str, v: CtlVal) {
    let value = match v {
        CtlVal::Str(s) => s,
        CtlVal::Num(n) => format!("{}", n.round().max(0.0) as u64),
        _ => return,
    };
    match crate::hot::set_in(&m.paths.shell_json(), key, &value) {
        Ok(_) => {
            m.shell = super::io::json_load(&m.paths.shell_json());
            m.say("Hot corners saved", false);
        }
        Err(e) => m.say(format!("Not saved: {e}"), true),
    }
    m.dirty = true;
}

pub fn apply_special(m: &mut Model, id: &str, v: CtlVal) {
    let parts: Vec<&str> = id.split(':').collect();
    if matches!(parts.as_slice(), ["sy", ..]) {
        return super::systempane::apply(m, &parts, v);
    }
    if matches!(parts.as_slice(), ["th", ..] | ["l", "theme" | "accent", _]) {
        return super::themepane::apply(m, &parts, v);
    }
    match parts.as_slice() {
        ["a", "reload"] => {
            m.on_files_changed();
            m.reload_rdwm();
            m.say(if m.rt.running { "Reload requested" } else { "rdwm is not running" }, false);
        }
        ["a", "open"] => {
            let p = m.store.path.display().to_string();
            if !m.store.path.exists() {
                let _ = super::io::write_atomic(&m.store.path, "");
            }
            if !super::io::spawn_detached("xdg-open", &[&p]) {
                m.say("Could not start xdg-open", true);
            }
        }
        ["a", "restore"] => match m.store.restore_backup() {
            Ok(()) => {
                m.say("Original configuration restored", false);
                m.reload_rdwm();
            }
            Err(e) => m.say(format!("Not restored: {e}"), true),
        },
        ["hc", key] => apply_hot_corner(m, key, v),
        ["a", "restart-dock"] => {
            let ok = restart_process("macdock");
            m.say(if ok { "Dock restarted" } else { "Could not start macdock" }, !ok);
        }
        ["a", "restart-bar"] => {
            let ok = restart_process("macbar");
            m.say(if ok { "Menu bar restarted" } else { "Could not start macbar" }, !ok);
        }
        ["w", "clear"] => {
            m.apply("r:wallpaper:image", CtlVal::Reset);
        }
        ["w", "save-live"] => {
            let img = m.rt.state["wallpaper"]["image"].as_str().map(String::from);
            if let Some(i) = img {
                m.apply("r:wallpaper:image", CtlVal::Str(i.clone()));
                super::themepane::follow_wallpaper(m, &i);
            }
        }
        ["w", ..] => {
            let path = id.strip_prefix("w:").unwrap_or("").to_string();
            m.apply("r:wallpaper:image", CtlVal::Str(path.clone()));
            super::themepane::follow_wallpaper(m, &path);
        }
        ["p", op, i] => pinned_op(m, op, i.parse().unwrap_or(usize::MAX)),
        ["p", "add"] => {
            if let CtlVal::Str(app) = v {
                let mut list = m.pinned_list_pub();
                if !list.contains(&app) {
                    list.push(app);
                    m.write_pinned(list);
                }
            }
        }
        ["l", "set", name] => set_default_layout(m, name),
        ["l", "add"] => {
            if let CtlVal::Str(n) = v {
                let mut o = m.list("general", "layouts");
                if !o.contains(&n) {
                    o.push(n);
                    set_list(m, "layouts", o);
                }
            }
        }
        ["l", op @ ("up" | "down" | "del"), i] => {
            let mut o = m.list("general", "layouts");
            let Ok(i) = i.parse::<usize>() else { return };
            if i < o.len() {
                match *op {
                    "up" if i > 0 => o.swap(i, i - 1),
                    "down" if i + 1 < o.len() => o.swap(i, i + 1),
                    "del" if o.len() > 1 => {
                        o.remove(i);
                    }
                    _ => return,
                }
                set_list(m, "layouts", o);
            }
        }
        ["l", "tag", n] => {
            if let (Ok(n), CtlVal::Str(layout)) = (n.parse::<i64>(), v) {
                set_tag_layout(m, n, &layout);
            }
        }
        ["lo", "@pick"] => {
            if let CtlVal::Str(n) = v {
                m.sel.layout_opt = Some(n);
                m.dirty = true;
            }
        }
        ["lo", name, key] => {
            let (name, key) = (format!("layout.{name}"), key.to_string());
            let val = match (key.as_str(), v) {
                ("gap_inner", CtlVal::Num(n)) => (n > 0.0).then_some(Val::Int(n as i64)),
                (_, CtlVal::Str(s)) if !s.is_empty() => Some(Val::Str(s)),
                _ => None,
            };
            m.edit(|c| match &val {
                Some(v) => c.set(&name, &key, v),
                None => {
                    c.remove(&name, &key);
                    Ok(())
                }
            });
        }
        ["t", ..] => table_op(m, id, v),
        ["k", ..] => super::keys::apply(m, id, v),
        ["mod", name] => {
            let name = ["super", "ctrl", "alt", "shift"].into_iter().find(|x| x == name);
            if let Some(n) = name {
                if let Some(p) = m.sel.rec_mods.iter().position(|x| *x == n) {
                    m.sel.rec_mods.remove(p);
                } else {
                    m.sel.rec_mods.push(n);
                }
                m.dirty = true;
            }
        }
        _ => {}
    }
}

fn set_list(m: &mut Model, key: &str, list: Vec<String>) {
    let key = key.to_string();
    m.edit(|c| c.set("general", &key, &Val::List(list)));
}

fn set_default_layout(m: &mut Model, name: &str) {
    let mut o = m.list("general", "layouts");
    o.retain(|x| x != name);
    o.insert(0, name.to_string());
    // dwm keeps the first two as the initial slots; make sure there is a second one to toggle with
    if o.len() < 2 {
        o.push("floating".into());
    }
    set_list(m, "layouts", o);
}

fn set_tag_layout(m: &mut Model, tag: i64, layout: &str) {
    let found = (0..m.store.cfg.array_len("tag_layout")).find(|&j| m.store.cfg.array_get("tag_layout", j, "tag").and_then(|v| v.as_i64()) == Some(tag));
    let layout = layout.to_string();
    m.edit(|c| {
        match (found, layout.is_empty()) {
            (Some(j), true) => {
                c.array_unset("tag_layout", j, "layout")?;
                // an entry that only names the tag does nothing: drop it
                if c.array_keys("tag_layout", j).iter().all(|k| k == "tag") {
                    c.array_remove("tag_layout", j)?;
                }
            }
            (Some(j), false) => c.array_set("tag_layout", j, "layout", &Val::Str(layout.clone()))?,
            (None, false) => {
                c.array_push("tag_layout", &[("tag", Val::Int(tag)), ("layout", Val::Str(layout.clone()))])?;
            }
            (None, true) => {}
        }
        Ok(())
    });
}

impl Model {
    pub fn pinned_list_pub(&self) -> Vec<String> {
        self.pinned_list()
    }

    pub fn write_pinned(&mut self, list: Vec<String>) {
        let v = json!(list);
        self.edit_json("pinned", "pinned", Some(v.clone()));
        self.edit_json("pinned", "order", Some(v));
        self.say("Saved (restart the dock to apply)", false);
    }
}

fn pinned_op(m: &mut Model, op: &str, i: usize) {
    let mut list = m.pinned_list_pub();
    if i >= list.len() {
        return;
    }
    match op {
        "up" if i > 0 => list.swap(i, i - 1),
        "down" if i + 1 < list.len() => list.swap(i, i + 1),
        "del" => {
            list.remove(i);
        }
        _ => return,
    }
    m.write_pinned(list);
}

fn table_op(m: &mut Model, id: &str, v: CtlVal) {
    let Some((name, which, key)) = parse_tid(id) else { return };
    let Some(spec) = m.table_spec(&name) else { return };
    let n = m.store.cfg.array_len(spec.name);
    match which.as_str() {
        "@add" => {
            let fields: Vec<(&str, Val)> = spec.new.iter().map(|(k, v)| (*k, v.clone())).collect();
            m.edit(|c| c.array_push(spec.name, &fields).map(|_| ()));
            let n2 = m.store.cfg.array_len(spec.name);
            if n2 > n {
                m.sel.table.insert(name, n2 - 1);
            }
        }
        "@addwin" => {
            if let CtlVal::Str(class) = v {
                m.edit(|c| c.array_push("rule", &[("class", Val::Str(class.clone())), ("floating", Val::Bool(true))]).map(|_| ()));
                let n2 = m.store.cfg.array_len("rule");
                if n2 > n {
                    m.sel.table.insert(name, n2 - 1);
                }
            }
        }
        idx => {
            let Ok(i) = idx.parse::<usize>() else { return };
            if i >= n {
                return;
            }
            match key.as_str() {
                "@sel" => {
                    if m.sel.table.get(&name) == Some(&i) {
                        m.sel.table.remove(&name);
                    } else {
                        m.sel.table.insert(name, i);
                    }
                    m.dirty = true;
                }
                "@del" => {
                    if m.edit(|c| c.array_remove(spec.name, i)) {
                        m.sel.table.remove(&name);
                    }
                }
                "@up" if i > 0 => {
                    if m.edit(|c| c.array_move(spec.name, i, i - 1)) {
                        m.sel.table.insert(name, i - 1);
                    }
                }
                "@down" if i + 1 < n => {
                    if m.edit(|c| c.array_move(spec.name, i, i + 1)) {
                        m.sel.table.insert(name, i + 1);
                    }
                }
                k => {
                    let Some(fld) = spec.fields.iter().find(|f| f.key == k) else { return };
                    let val: Result<Option<Val>, String> = match (&fld.ft, v) {
                        (_, CtlVal::Reset) => Ok(None),
                        (Ft::Text, CtlVal::Str(s)) => Ok((!s.trim().is_empty()).then_some(Val::Str(s))),
                        (Ft::List, CtlVal::Str(s)) => {
                            let l: Vec<String> = s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                            Ok((!l.is_empty()).then_some(Val::List(l)))
                        }
                        (Ft::Int { min, max, offset }, CtlVal::Num(x)) => {
                            let shown = x.round() as i64;
                            if shown < *min || shown > *max {
                                Err(format!("{} must be {min} to {max}", fld.label))
                            } else if shown == 0 {
                                Ok(None)
                            } else {
                                Ok(Some(Val::Int(shown - offset)))
                            }
                        }
                        (Ft::Float { min, max, .. }, CtlVal::Num(x)) => {
                            if x < *min || x > *max {
                                Err(format!("{} must be {min} to {max}", fld.label))
                            } else {
                                Ok(Some(Val::Float(x)))
                            }
                        }
                        (Ft::Bool3, CtlVal::Str(s)) => Ok(match s.as_str() {
                            "yes" => Some(Val::Bool(true)),
                            "no" => Some(Val::Bool(false)),
                            _ => None,
                        }),
                        (Ft::Bool, CtlVal::Bool(b)) => Ok(Some(Val::Bool(b))),
                        (Ft::Choice(_), CtlVal::Str(s)) => Ok((!s.is_empty()).then_some(Val::Str(s))),
                        _ => Err("unexpected value".into()),
                    };
                    match val {
                        Ok(Some(val)) => {
                            m.edit(|c| c.array_set(spec.name, i, k, &val));
                        }
                        Ok(None) => {
                            m.edit(|c| c.array_unset(spec.name, i, k));
                        }
                        Err(e) => m.say(e, true),
                    }
                }
            }
        }
    }
}

#[allow(dead_code)]
fn _use(_: &dyn Fn(&str, &str) -> String) {
    let _ = rid;
}

#[cfg(test)]
mod tests {
    use super::super::model::tests_support::model_with;
    use super::*;
    use std::collections::HashSet;

    const BIG: &str = "# c\n[general]\nlayouts = [\"bsp\", \"tile\"]\ntags = [\"a\", \"b\"]\n[[tag_layout]]\ntag = 2\nlayout = \"grid\"\n[[bind]]\nmods = [\"super\"]\nkey = \"Return\"\naction = \"spawn\"\n[[bind]]\nkeys = [\"super+x\", \"r\"]\naction = \"reload\"\n[[rule]]\nclass = \"mpv\"\nfloating = true\n[[autostart]]\ncommand = \"macbar &\"\n[[plugin]]\ncommand = \"p\"\n[[scratchpad]]\nname = \"t\"\ncommand = \"foot\"\n[[wallpaper.output]]\nmatch = \"*\"\n[[output]]\nname = \"DP-1\"\n[[mode]]\nname = \"resize\"\n[[mode.bind]]\nkeys = \"h\"\naction = \"setmfact\"\n";

    #[test]
    fn every_key_the_panes_use_exists_in_rdwms_key_table() {
        let (mut m, _) = model_with(BIG);
        for i in 0..PANES.len() {
            m.prepare(i);
            let _ = m.items(i);
        }
        let missing = m.missing.borrow();
        assert!(missing.is_empty(), "panes use keys rdwm does not know: {missing:?}");
        let used: HashSet<_> = m.used.borrow().iter().cloned().collect();
        assert!(used.len() > 45, "only {} keys used", used.len());
        // and each of them really parses in rdwm's config (checked by rdwm's own test when rdwmctl is around)
        for (s, k) in &used {
            assert!(m.schema.has(s, k), "{s}.{k}");
        }
    }

    #[test]
    fn control_ids_are_unique_within_a_pane_and_every_pane_builds_with_rdwm_down() {
        let (mut m, _) = model_with(BIG);
        m.sel.table.insert("rule".into(), 0);
        for i in 0..PANES.len() {
            m.prepare(i);
            let items = m.items(i);
            assert!(!items.is_empty(), "{}", PANES[i].id);
            let mut ids = HashSet::new();
            for it in &items {
                let rows: Vec<&Row> = if let Item::Card(r) = it { r.iter().collect() } else { vec![] };
                for r in rows {
                    for c in &r.ctls {
                        if matches!(c.kind, Kind::Label(_)) {
                            continue;
                        }
                        assert!(ids.insert(c.id.clone()), "duplicate control id {} in {}", c.id, PANES[i].id);
                    }
                }
            }
        }
    }

    #[test]
    fn search_finds_panes_by_title_and_keyword() {
        let find = |q: &str| PANES.iter().filter(|p| pane_matches(p, q)).map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(find("wallp"), ["wallpaper", "displays"]);
        assert_eq!(find("gap"), ["tiling"]);
        assert!(find("snap").contains(&"windows"));
        assert_eq!(find("").len(), PANES.len());
        assert!(find("zzzz").is_empty());
    }

    #[test]
    fn tiling_pane_edits_layouts_and_per_tag_setup() {
        let (mut m, cfg) = model_with(BIG);
        apply_special(&mut m, "l:set:grid", CtlVal::Click);
        assert_eq!(m.list("general", "layouts"), ["grid", "bsp", "tile"]);
        apply_special(&mut m, "l:tag:1", CtlVal::Str("monocle".into()));
        apply_special(&mut m, "l:tag:2", CtlVal::Str(String::new()));
        let t = std::fs::read_to_string(&cfg).unwrap();
        assert!(t.contains("layout = \"monocle\"") && !t.contains("layout = \"grid\"\n[[bind]]"), "{t}");
        assert!(!t.contains("tag = 2"), "an empty per-tag entry is removed: {t}");
        apply_special(&mut m, "lo:@pick", CtlVal::Str("rtile".into()));
        apply_special(&mut m, "lo:rtile:master_side", CtlVal::Str("right".into()));
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("[layout.rtile]\nmaster_side = \"right\""));
        assert!(super::super::tomlcfg::Cfg::parse(&std::fs::read_to_string(&cfg).unwrap()).is_ok());
    }

    #[test]
    fn rules_add_edit_move_delete() {
        let (mut m, cfg) = model_with(BIG);
        apply_special(&mut m, "t:rule:@add", CtlVal::Click);
        assert_eq!(m.sel.table.get("rule"), Some(&1));
        apply_special(&mut m, "t:rule:1:class", CtlVal::Str("firefox".into()));
        apply_special(&mut m, "t:rule:1:tag", CtlVal::Num(3.0));
        apply_special(&mut m, "t:rule:1:floating", CtlVal::Str("no".into()));
        apply_special(&mut m, "t:rule:1:monitor", CtlVal::Num(2.0));
        let t = std::fs::read_to_string(&cfg).unwrap();
        assert!(t.contains("class = \"firefox\"") && t.contains("tag = 3") && t.contains("floating = false") && t.contains("monitor = 1"), "{t}");
        apply_special(&mut m, "t:rule:1:tag", CtlVal::Num(0.0));
        assert!(!std::fs::read_to_string(&cfg).unwrap().contains("tag = 3"));
        apply_special(&mut m, "t:rule:1:@up", CtlVal::Click);
        assert_eq!(m.store.cfg.array_get("rule", 0, "class"), Some(Val::Str("firefox".into())));
        apply_special(&mut m, "t:rule:0:@del", CtlVal::Click);
        apply_special(&mut m, "t:rule:0:@del", CtlVal::Click);
        assert_eq!(m.store.cfg.array_len("rule"), 0);
        apply_special(&mut m, "t:rule:9:@del", CtlVal::Click); // out of range: ignored
        apply_special(&mut m, "t:nosuch:0:@del", CtlVal::Click);
    }

    #[test]
    fn pinned_apps_edit_the_json_and_keep_other_keys() {
        let (mut m, _) = model_with("");
        std::fs::write(m.paths.dock_pinned(), "{\"pinned\":[\"a\",\"b\",\"c\"],\"order\":[\"a\",\"b\",\"c\"],\"extra\":1}").unwrap();
        m.pinned = super::super::io::json_load(&m.paths.dock_pinned());
        apply_special(&mut m, "p:down:0", CtlVal::Click);
        apply_special(&mut m, "p:del:2", CtlVal::Click);
        apply_special(&mut m, "p:add", CtlVal::Str("firefox".into()));
        let v = super::super::io::json_load(&m.paths.dock_pinned());
        assert_eq!(v["order"], json!(["b", "a", "firefox"]));
        assert_eq!(v["pinned"], v["order"]);
        assert_eq!(v["extra"], 1);
    }

    #[test]
    fn current_wallpaper_prefers_what_rdwm_shows() {
        let (mut m, _) = model_with("[wallpaper]\nimage = \"/a.png\"\n");
        assert_eq!(m.current_wallpaper().as_deref(), Some("/a.png"));
        m.rt.running = true;
        m.rt.state = json!({"wallpaper": {"image": "/b.png"}});
        assert_eq!(m.current_wallpaper().as_deref(), Some("/b.png"));
        let items = m.items(3);
        assert!(items.iter().any(|i| matches!(i, Item::Note { text, .. } if text.contains("/b.png"))));
    }

    #[test]
    fn schema_activation_key_is_shown_only_when_rdwm_has_it() {
        let (mut m, _) = model_with("");
        let has = |m: &Model| m.items(5).iter().any(|i| matches!(i, Item::Card(rows) if rows.iter().any(|r| r.ctls.iter().any(|c| c.id == "r:snap:activation"))));
        assert!(!has(&m));
        m.schema.keys.push(super::super::schema::KeyMeta {
            section: "snap".into(),
            key: "activation".into(),
            ty: "enum".into(),
            default: json!("always"),
            min: None,
            max: None,
            choices: vec!["always".into(), "shift".into()],
            desc: String::new(),
        });
        assert!(has(&m));
    }
}
