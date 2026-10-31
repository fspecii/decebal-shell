//! Menu definitions (ported from the original `ambsd.mac-menubar`), dropdown layout and actions.

use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    pub action: &'static str,
    pub shortcut: &'static str,
    pub separator: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub key: &'static str,
    pub title: String,
    /// The Apple menu: drawn as the logo instead of text.
    pub icon: bool,
    pub bold: bool,
    pub items: Vec<Item>,
}

fn item(label: &str, action: &'static str, shortcut: &'static str) -> Item {
    Item { label: label.to_string(), action, shortcut, separator: false }
}

fn sep() -> Item {
    Item { label: String::new(), action: "", shortcut: "", separator: true }
}

/// All menus for the app named `app`.
pub fn build(app: &str, light: bool) -> Vec<Menu> {
    vec![
        Menu {
            key: "apple",
            title: String::new(),
            icon: true,
            bold: false,
            items: vec![
                item("About This Computer", "about", ""),
                sep(),
                item("System Settings…", "settings", ""),
                item("App Store…", "appstore", ""),
                item("Desktop Style…", "style", "⌘⌥D"),
                item("Wallpapers…", "wallhaven", ""),
                item(if light { "Switch to Dark Appearance" } else { "Switch to Light Appearance" }, "appearance", ""),
                sep(),
                item(&format!("Force Quit {app}"), "force-quit", ""),
                sep(),
                item("Session…", "session", ""),
                item("Sleep", "sleep", ""),
                item("Restart…", "restart", ""),
                item("Shut Down…", "shutdown", ""),
                sep(),
                item("Lock Screen", "lock", ""),
                item("Log Out…", "logout", ""),
            ],
        },
        Menu {
            key: "app",
            title: app.to_string(),
            icon: false,
            bold: true,
            items: vec![
                item(&format!("About {app}"), "about", ""),
                sep(),
                item(&format!("Hide {app}"), "minimize", ""),
                item("Show Hidden Windows", "show-minimized", "⌘S"),
                sep(),
                item(&format!("Quit {app}"), "close-window", "⌘W"),
            ],
        },
        Menu {
            key: "file",
            title: "File".into(),
            icon: false,
            bold: false,
            items: vec![
                item("New Window", "new-window", "⌃N"),
                item("New Tab", "new-tab", "⌃T"),
                item("Open…", "open", "⌃O"),
                sep(),
                item("Save", "save", "⌃S"),
                item("Print…", "print", "⌃P"),
                sep(),
                item("Close Window", "close-window", "⌘W"),
            ],
        },
        Menu {
            key: "edit",
            title: "Edit".into(),
            icon: false,
            bold: false,
            items: vec![
                item("Undo", "undo", "⌃Z"),
                item("Redo", "redo", "⌃⇧Z"),
                sep(),
                item("Cut", "cut", "⌘X"),
                item("Copy", "copy", "⌘C"),
                item("Paste", "paste", "⌘V"),
                item("Select All", "select-all", "⌃A"),
                sep(),
                item("Find…", "find", "⌃F"),
                item("Emoji & Symbols", "emoji", ""),
            ],
        },
        Menu {
            key: "view",
            title: "View".into(),
            icon: false,
            bold: false,
            items: vec![
                item("Zoom In", "zoom-in", "⌃+"),
                item("Zoom Out", "zoom-out", "⌃−"),
                item("Actual Size", "zoom-reset", "⌃0"),
                sep(),
                item("Reload", "reload", "⌃R"),
                item("Enter Full Screen", "fullscreen", "⌘F"),
            ],
        },
        Menu {
            key: "window",
            title: "Window".into(),
            icon: false,
            bold: false,
            items: vec![
                item("Minimize", "minimize", "⌘⌥S"),
                item("Zoom", "zoom", "⌘⌥F"),
                item("Float / Tile", "float", "⌘T"),
                item("Tile All Windows", "tile-all", "⌘⇧T"),
                item("Snap Left", "snap-left", ""),
                item("Snap Right", "snap-right", ""),
                item("Snap Top / Fill Screen", "snap-up", ""),
                item("Restore Size", "snap-restore", ""),
                item("Center", "center", ""),
                item("Keep on Top", "pin", ""),
                item("Switch Tiling / Floating Windows", "windows-mode", ""),
                sep(),
                item("Move to Next Desktop", "next-desktop", ""),
                item("Move to Previous Desktop", "prev-desktop", ""),
            ],
        },
        Menu { key: "help", title: "Help".into(), icon: false, bold: false, items: vec![item("Keyboard Shortcuts", "shortcuts", "⌘K"), item("Omarchy Manual", "manual", "")] },
    ]
}

// ------------------------------------------------------------------ layout

pub const ROW_H: f32 = 24.0;
pub const SEP_H: f32 = 11.0;
pub const PAD: f32 = 6.0;
pub const MIN_W: f32 = 220.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub y: f32,
    pub h: f32,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dropdown {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rows: Vec<Row>,
}

impl Dropdown {
    /// Lays out `menu` with its top-left at (`x`, `y`). `text_w` measures a string at the menu font.
    pub fn layout(menu: &Menu, x: f32, y: f32, text_w: impl Fn(&str) -> f32) -> Dropdown {
        let widest = menu.items.iter().filter(|i| !i.separator).map(|i| text_w(&i.label) + text_w(i.shortcut) + 56.0).fold(0.0f32, f32::max);
        let w = MIN_W.max(widest + 2.0 * PAD);
        let mut cy = y + PAD;
        let rows = menu
            .items
            .iter()
            .enumerate()
            .map(|(index, i)| {
                let h = if i.separator { SEP_H } else { ROW_H };
                let row = Row { y: cy, h, index };
                cy += h;
                row
            })
            .collect();
        Dropdown { x, y, w, h: cy + PAD - y, rows }
    }

    /// Moves the dropdown (and its rows) by `dy`: a bottom bar opens it upwards.
    pub fn shift_y(&mut self, dy: f32) {
        self.y += dy;
        for r in &mut self.rows {
            r.y += dy;
        }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// Index of the clickable (non-separator) item under the point.
    pub fn hit(&self, menu: &Menu, px: f32, py: f32) -> Option<usize> {
        if px < self.x + PAD || px >= self.x + self.w - PAD {
            return None;
        }
        self.rows.iter().find(|r| py >= r.y && py < r.y + r.h).map(|r| r.index).filter(|&i| !menu.items[i].separator)
    }
}

// ----------------------------------------------------------------- actions

/// Runs menu actions through the original `mac-menu-action` helper when present, else portable fallbacks.
#[derive(Debug, Clone)]
pub struct Runner {
    helper: Option<PathBuf>,
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file()))
}

/// The rdwm action behind a Window-menu row.
fn rdwm_action(action: &str) -> Option<&'static str> {
    Some(match action {
        "minimize" => "minimize",
        "zoom" => "togglemaximize",
        "fullscreen" => "togglefullscreen",
        "float" => "togglefloating",
        "pin" => "togglesticky",
        "tile-all" => "tileall",
        "snap-left" => "snap_left",
        "snap-right" => "snap_right",
        "snap-up" => "snap_up",
        "snap-restore" => "snap_restore",
        _ => return None,
    })
}

/// Portable command for an action without the helper, if one exists.
pub fn fallback_command(action: &str) -> Option<Vec<String>> {
    let c = |args: &[&str]| Some(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    match action {
        "sleep" => c(&["systemctl", "suspend"]),
        "restart" => c(&["systemctl", "reboot"]),
        "shutdown" => c(&["systemctl", "poweroff"]),
        "lock" => c(&["loginctl", "lock-session"]),
        "logout" => c(&["loginctl", "terminate-user", &std::env::var("USER").unwrap_or_default()]),
        "settings" => c(&["decsettings"]),
        "session" => c(&["decsession"]),
        "manual" => c(&["xdg-open", "https://learn.omacom.io/2/the-omarchy-manual"]),
        _ => None,
    }
}

impl Runner {
    /// Tests: a runner whose every action counts as available (a helper path that is never run by them).
    #[cfg(test)]
    pub fn available_for_tests() -> Self {
        Self { helper: Some(PathBuf::from("/nonexistent/mac-menu-action")) }
    }
    pub fn detect() -> Self {
        let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/mac-menu-action"));
        let helper = home.filter(|p| p.is_file()).or_else(|| find_in_path("mac-menu-action"));
        Self { helper }
    }

    pub fn without_helper() -> Self {
        Self { helper: None }
    }

    /// Whether the action can do anything on this system (otherwise the row is dimmed).
    pub fn available(&self, action: &str) -> bool {
        (rdwm_action(action).is_some() && super::rdwm::socket_path().exists()) || self.helper.is_some() || fallback_command(action).is_some()
    }

    /// Destructive actions ask for a second click when no helper (which confirms itself) is installed.
    pub fn needs_confirm(&self, action: &str) -> bool {
        self.helper.is_none() && matches!(action, "restart" | "shutdown" | "logout")
    }

    /// Starts the action detached. Returns false if nothing could be started.
    pub fn run(&self, action: &str) -> bool {
        // Window-menu rows drive rdwm directly (the helper below speaks Hyprland).
        if let Some(a) = rdwm_action(action).filter(|_| super::rdwm::socket_path().exists()) {
            super::rdwm::send_action(a, &[]);
            return true;
        }
        // "System Settings…" opens decsettings when it is installed (the helper's menu is Omarchy's own).
        if action == "settings" && find_in_path("decsettings").is_some() {
            return spawn_detached(&["decsettings".to_string()]);
        }
        // "Session…" is decebal's own overlay; the helper's menu is Omarchy's.
        if action == "session" {
            return spawn_detached(&[crate::launch::sibling_or_path("decsession")]);
        }
        let argv = match &self.helper {
            Some(h) => vec![h.to_string_lossy().into_owned(), action.to_string()],
            None => match fallback_command(action) {
                Some(a) => a,
                None => return false,
            },
        };
        spawn_detached(&argv)
    }
}

/// Spawns `argv` with no stdio and reaps it on a thread. False if it could not be started.
pub fn spawn_detached(argv: &[String]) -> bool {
    let Some((prog, args)) = argv.split_first() else { return false };
    match Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(s: &str) -> f32 {
        s.chars().count() as f32 * 7.0
    }

    #[test]
    fn menus_match_the_original_titles() {
        let m = build("Foot", false);
        let titles: Vec<&str> = m.iter().map(|m| m.key).collect();
        assert_eq!(titles, ["apple", "app", "file", "edit", "view", "window", "help"]);
        assert!(m[1].bold && m[1].title == "Foot");
        assert!(m[0].icon);
    }

    #[test]
    fn apple_menu_reflects_appearance_and_app() {
        let dark = build("Foot", false);
        let light = build("Foot", true);
        assert!(dark[0].items.iter().any(|i| i.label == "Switch to Light Appearance"));
        assert!(light[0].items.iter().any(|i| i.label == "Switch to Dark Appearance"));
        assert!(dark[0].items.iter().any(|i| i.label == "Force Quit Foot"));
    }

    #[test]
    fn dropdown_has_min_width_and_row_geometry() {
        let m = build("Foot", false);
        let d = Dropdown::layout(&m[2], 100.0, 28.0, width);
        assert!(d.w >= MIN_W);
        assert_eq!(d.rows.len(), m[2].items.len());
        assert_eq!(d.rows[0].y, 28.0 + PAD);
        // 6 rows of 24 + 2 separators of 11 + padding
        assert_eq!(d.h, 6.0 * ROW_H + 2.0 * SEP_H + 2.0 * PAD);
    }

    #[test]
    fn hit_test_finds_items_and_skips_separators() {
        let m = build("Foot", false);
        let menu = &m[2];
        let d = Dropdown::layout(menu, 100.0, 28.0, width);
        assert_eq!(d.hit(menu, 120.0, d.rows[0].y + 5.0), Some(0));
        assert_eq!(d.hit(menu, 120.0, d.rows[3].y + 5.0), None, "separator");
        assert_eq!(d.hit(menu, 101.0, d.rows[0].y + 5.0), None, "inside padding");
        assert_eq!(d.hit(menu, 120.0, 5.0), None, "above");
    }

    #[test]
    fn contains_covers_the_whole_panel() {
        let m = build("Foot", false);
        let d = Dropdown::layout(&m[3], 50.0, 28.0, width);
        assert!(d.contains(55.0, 30.0));
        assert!(!d.contains(49.0, 30.0));
        assert!(!d.contains(55.0, d.y + d.h + 1.0));
    }

    #[test]
    fn wide_labels_widen_the_dropdown() {
        let m = build("Foot", false);
        let d = Dropdown::layout(&m[5], 0.0, 0.0, |s| s.chars().count() as f32 * 12.0);
        assert!(d.w > MIN_W);
    }

    #[test]
    fn fallbacks_exist_for_system_actions_only() {
        assert_eq!(fallback_command("sleep"), Some(vec!["systemctl".into(), "suspend".into()]));
        assert_eq!(fallback_command("settings"), Some(vec!["decsettings".into()]));
        assert!(fallback_command("copy").is_none());
        assert!(fallback_command("nonsense").is_none());
    }

    #[test]
    fn without_helper_destructive_actions_confirm() {
        let r = Runner::without_helper();
        assert!(r.needs_confirm("shutdown") && r.needs_confirm("restart") && r.needs_confirm("logout"));
        assert!(!r.needs_confirm("sleep") && !r.needs_confirm("lock"));
        assert!(r.available("sleep") && !r.available("copy"));
    }

    #[test]
    fn unavailable_action_does_not_run() {
        assert!(!Runner::without_helper().run("copy"));
    }
}
