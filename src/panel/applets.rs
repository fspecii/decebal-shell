//! The applet registry: every control a Decebal panel can show, under one stable key, with what it does, what it needs
//! and which background work it keeps alive. Renderers, Settings, theme packages and `decthemectl panel` all read this
//! table; nothing else names applets. See `docs/universal-panels.md` for the contract.

/// One applet. The discriminant orders the bit set and the Settings list; keys are the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AppletId {
    Launcher,
    AppMenu,
    Workspaces,
    WindowTitle,
    Tasks,
    Layout,
    Media,
    Clock,
    Weather,
    Tray,
    Clipboard,
    Cpu,
    Memory,
    Notifications,
    StayAwake,
    Agents,
    LockKeys,
    Volume,
    Network,
    Bluetooth,
    Battery,
    Brightness,
    Search,
    Dashboard,
    ControlCenter,
    Power,
    ShowDesktop,
}

/// What a pointer or keyboard can do with an applet besides its primary action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    /// Opens a popup / popout of its own (Enter or a click; Escape closes it).
    Popup,
    /// The wheel changes its value (volume, brightness) or cycles it (layout, workspaces).
    Scroll,
    /// Middle click has a meaning (mute).
    Middle,
    /// Right click offers more (timed Stay Awake, application menus of tray items).
    Secondary,
    /// Launches another Decebal program (it opens beside the applet when the program takes a position).
    Launch,
}

/// What must exist for the applet to show anything. An enabled applet whose need is missing is still listed in Settings,
/// with [`Need::explain`] saying why the panel does not show it right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Nothing,
    /// A window manager that reports workspaces (EWMH desktops or rdwm tags).
    WindowManager,
    /// A window manager that publishes the window list and can show the desktop (EWMH / foreign-toplevel).
    WindowList,
    Rdwm,
    /// An MPRIS player on the session bus.
    MediaPlayer,
    /// A cached or fetched weather reading (network access to wttr.in).
    WeatherData,
    /// A StatusNotifierItem application.
    TrayApps,
    Battery,
    Bluetooth,
    AudioSink,
    NetworkManager,
    Backlight,
    /// A sibling program (`decmon`, `decclip`, `decnotify`...).
    Program(&'static str),
    /// Only shown while its state is on (Stay Awake, a lock key).
    ActiveState,
}

impl Need {
    pub fn explain(self) -> &'static str {
        match self {
            Need::Nothing => "always available",
            Need::WindowManager => "shown when the window manager reports workspaces",
            Need::WindowList => "needs a window manager that publishes its window list (EWMH or wlr-foreign-toplevel)",
            Need::Rdwm => "needs rdwm (layout IPC)",
            Need::MediaPlayer => "shown while an MPRIS media player is running",
            Need::WeatherData => "needs a weather reading (fetched from wttr.in, cached for 30 minutes)",
            Need::TrayApps => "shows applications that publish a StatusNotifierItem",
            Need::Battery => "shown when a battery is detected",
            Need::Bluetooth => "shown when a Bluetooth adapter is detected (bluetoothctl)",
            Need::AudioSink => "shown when PipeWire/PulseAudio reports an output (wpctl or pactl)",
            Need::NetworkManager => "shown when a network interface is detected (nmcli for Wi-Fi lists)",
            Need::Backlight => "shown when a backlight is detected (brightnessctl or /sys/class/backlight)",
            Need::Program(p) => match p {
                "decmon" => "needs decmon (the process monitor) installed",
                "decclip" => "needs decclip (clipboard history) installed",
                "decnotify" => "needs decnotify (notification history) installed",
                "decsession" => "needs decsession (session menu) installed",
                "decdash" => "needs decdash (dashboard) installed",
                "decspot" => "needs decspot (search) installed",
                _ => "needs a Decebal program installed",
            },
            Need::ActiveState => "by default shown only while it is on; turned on here it stays as a control",
        }
    }
}

/// Background work an applet keeps alive. A renderer starts a worker only while some enabled applet (or a shared
/// consumer such as the Control Center) needs it, see [`super::workers_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Worker {
    /// The 30-minute weather fetch.
    Weather,
    /// The MPRIS player subscription.
    Mpris,
    /// CPU / memory sampling every 2 s.
    Resources,
    /// Caps / Num Lock state.
    LockKeys,
    /// The StatusNotifierWatcher host (tray applications).
    Indicators,
    /// Volume change subscription (`pactl subscribe` / wpctl).
    VolumeWatch,
    /// rdwm layout and tag subscription.
    LayoutWatch,
}

#[derive(Debug, Clone, Copy)]
pub struct Applet {
    pub id: AppletId,
    /// Stable key in `decebal.json` `bar.applets`, theme `[bar] applets`, and `decthemectl panel`.
    pub key: &'static str,
    /// Older spellings that still parse (the Dux item ids among them).
    pub aliases: &'static [&'static str],
    pub title: &'static str,
    /// What its primary activation (click, Enter, Space) does.
    pub action: &'static str,
    pub caps: &'static [Cap],
    pub need: Need,
    pub workers: &'static [Worker],
}

use AppletId as A;
use Cap::*;
use Worker as Wk;

/// Every applet, in Settings order (which is also the order of the bit set).
pub const REGISTRY: &[Applet] = &[
    Applet {
        id: A::Launcher,
        key: "launcher",
        aliases: &["launcherButton", "logo", "start"],
        title: "Launcher",
        action: "opens the application launcher (Start button, OS logo)",
        caps: &[Launch],
        need: Need::Nothing,
        workers: &[],
    },
    Applet {
        id: A::AppMenu,
        key: "app_menu",
        aliases: &["appMenu", "menus", "global_menu", "window_menu"],
        title: "Application menu",
        action: "opens the system menu and the focused application's menus",
        caps: &[Popup],
        need: Need::Nothing,
        workers: &[],
    },
    Applet {
        id: A::Workspaces,
        key: "workspaces",
        aliases: &["workspaceSwitcher", "tags"],
        title: "Workspaces",
        action: "switches to the clicked workspace",
        caps: &[Scroll],
        need: Need::WindowManager,
        workers: &[Wk::LayoutWatch],
    },
    Applet {
        id: A::WindowTitle,
        key: "window_title",
        aliases: &["focusedWindow", "window"],
        title: "Window title",
        action: "shows the focused window; opens its window menu (close, minimise)",
        caps: &[Popup],
        need: Need::Nothing,
        workers: &[],
    },
    Applet {
        id: A::Tasks,
        key: "tasks",
        aliases: &["taskButtons", "task_manager"],
        title: "Task buttons",
        action: "focuses, minimises or launches pinned and running applications",
        caps: &[Secondary],
        need: Need::WindowList,
        workers: &[],
    },
    Applet {
        id: A::Layout,
        key: "layout",
        aliases: &[],
        title: "Window layout",
        action: "picks the tiling layout; the wheel cycles it",
        caps: &[Popup, Scroll],
        need: Need::Rdwm,
        workers: &[Wk::LayoutWatch],
    },
    Applet {
        id: A::Media,
        key: "media",
        aliases: &["music", "mpris"],
        title: "Media",
        action: "shows the playing track; play/pause, previous and next",
        caps: &[Popup],
        need: Need::MediaPlayer,
        workers: &[Wk::Mpris],
    },
    Applet {
        id: A::Clock,
        key: "clock",
        aliases: &["date", "datetime"],
        title: "Clock and date",
        action: "opens the calendar (the dashboard on rail bars)",
        caps: &[Popup],
        need: Need::Nothing,
        workers: &[],
    },
    Applet { id: A::Weather, key: "weather", aliases: &[], title: "Weather", action: "opens the forecast", caps: &[Popup], need: Need::WeatherData, workers: &[Wk::Weather] },
    Applet {
        id: A::Tray,
        key: "tray",
        aliases: &["systemTray", "indicators", "status_notifier"],
        title: "System tray",
        action: "application status icons and their menus",
        caps: &[Popup, Secondary],
        need: Need::TrayApps,
        workers: &[Wk::Indicators],
    },
    Applet { id: A::Clipboard, key: "clipboard", aliases: &[], title: "Clipboard history", action: "opens clipboard history", caps: &[Launch], need: Need::Program("decclip"), workers: &[] },
    Applet {
        id: A::Cpu,
        key: "cpu",
        aliases: &["cpuUsage"],
        title: "CPU usage",
        action: "shows processor load; opens the process monitor",
        caps: &[Launch],
        need: Need::Nothing,
        workers: &[Wk::Resources],
    },
    Applet {
        id: A::Memory,
        key: "memory",
        aliases: &["memUsage", "ram"],
        title: "Memory usage",
        action: "shows memory use; opens the process monitor",
        caps: &[Launch],
        need: Need::Nothing,
        workers: &[Wk::Resources],
    },
    Applet {
        id: A::Notifications,
        key: "notifications",
        aliases: &["notificationButton"],
        title: "Notifications",
        action: "opens notification history",
        caps: &[Launch],
        need: Need::Program("decnotify"),
        workers: &[],
    },
    Applet {
        id: A::StayAwake,
        key: "stay_awake",
        aliases: &["awake", "caffeine"],
        title: "Stay Awake",
        action: "turns Stay Awake off; right click offers timed sessions",
        caps: &[Secondary],
        need: Need::ActiveState,
        workers: &[],
    },
    Applet {
        id: A::Agents,
        key: "agents",
        aliases: &["process_monitor", "decmon"],
        title: "Process monitor",
        action: "opens the process monitor",
        caps: &[Launch],
        need: Need::Program("decmon"),
        workers: &[],
    },
    Applet {
        id: A::LockKeys,
        key: "lock_keys",
        aliases: &["lock", "capslock"],
        title: "Caps / Num Lock",
        action: "shows Caps Lock and Num Lock while on",
        caps: &[],
        need: Need::ActiveState,
        workers: &[Wk::LockKeys],
    },
    Applet {
        id: A::Volume,
        key: "volume",
        aliases: &["audio", "sound"],
        title: "Volume",
        action: "opens the output slider; wheel changes, middle click mutes",
        caps: &[Popup, Scroll, Middle],
        need: Need::AudioSink,
        workers: &[Wk::VolumeWatch],
    },
    Applet { id: A::Network, key: "network", aliases: &["wifi"], title: "Network", action: "opens Wi-Fi networks and the connection editor", caps: &[Popup], need: Need::NetworkManager, workers: &[] },
    Applet { id: A::Bluetooth, key: "bluetooth", aliases: &[], title: "Bluetooth", action: "opens paired devices", caps: &[Popup], need: Need::Bluetooth, workers: &[] },
    Applet { id: A::Battery, key: "battery", aliases: &["power_status"], title: "Battery", action: "opens charge details", caps: &[Popup], need: Need::Battery, workers: &[] },
    Applet {
        id: A::Brightness,
        key: "brightness",
        aliases: &["backlight", "display"],
        title: "Brightness",
        action: "opens the brightness slider; the wheel changes it",
        caps: &[Popup, Scroll],
        need: Need::Backlight,
        workers: &[],
    },
    Applet { id: A::Search, key: "search", aliases: &["spotlight"], title: "Search", action: "opens search (Spotlight)", caps: &[Launch], need: Need::Program("decspot"), workers: &[] },
    Applet {
        id: A::Dashboard,
        key: "dashboard",
        aliases: &["dash"],
        title: "Dashboard",
        action: "opens the dashboard beside the applet",
        caps: &[Launch],
        need: Need::Program("decdash"),
        workers: &[],
    },
    Applet {
        id: A::ControlCenter,
        key: "control_center",
        aliases: &["controlCenterButton", "control", "quick_settings"],
        title: "Control Center",
        action: "opens quick settings",
        caps: &[Popup],
        need: Need::Nothing,
        workers: &[],
    },
    Applet {
        id: A::Power,
        key: "power",
        aliases: &["session"],
        title: "Power",
        action: "opens the session menu (lock, log out, restart, shut down)",
        caps: &[Launch],
        need: Need::Program("decsession"),
        workers: &[],
    },
    Applet {
        id: A::ShowDesktop,
        key: "show_desktop",
        aliases: &["showDesktop", "peek"],
        title: "Show desktop",
        action: "minimises every window / brings them back",
        caps: &[],
        need: Need::WindowList,
        workers: &[],
    },
];

impl AppletId {
    pub fn info(self) -> &'static Applet {
        let a = &REGISTRY[self as usize];
        debug_assert_eq!(a.id, self);
        a
    }
    pub fn key(self) -> &'static str {
        self.info().key
    }
    pub fn title(self) -> &'static str {
        self.info().title
    }
    /// The key or an alias, case-insensitive with `-` / `_` / space ignored (so `launcherButton`, `app-menu` and
    /// `APP_MENU` all parse).
    pub fn parse(s: &str) -> Option<AppletId> {
        let norm = |t: &str| t.chars().filter(|c| !matches!(c, '-' | '_' | ' ')).flat_map(char::to_lowercase).collect::<String>();
        let want = norm(s);
        REGISTRY.iter().find(|a| norm(a.key) == want || a.aliases.iter().any(|al| norm(al) == want)).map(|a| a.id)
    }
    pub fn all() -> impl Iterator<Item = AppletId> {
        REGISTRY.iter().map(|a| a.id)
    }
    pub fn has(self, cap: Cap) -> bool {
        self.info().caps.contains(&cap)
    }
}

/// A set of applets (one bit each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AppletSet(u64);

impl AppletSet {
    pub const EMPTY: AppletSet = AppletSet(0);
    pub fn all() -> AppletSet {
        AppletSet((1u64 << REGISTRY.len()) - 1)
    }
    pub fn of(ids: &[AppletId]) -> AppletSet {
        let mut s = AppletSet::EMPTY;
        for id in ids {
            s.set(*id, true);
        }
        s
    }
    pub fn contains(self, id: AppletId) -> bool {
        self.0 & (1 << id as u64) != 0
    }
    pub fn set(&mut self, id: AppletId, on: bool) {
        if on {
            self.0 |= 1 << id as u64;
        } else {
            self.0 &= !(1 << id as u64);
        }
    }
    pub fn with(mut self, id: AppletId, on: bool) -> AppletSet {
        self.set(id, on);
        self
    }
    pub fn intersect(self, other: AppletSet) -> AppletSet {
        AppletSet(self.0 & other.0)
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }
    pub fn iter(self) -> impl Iterator<Item = AppletId> {
        AppletId::all().filter(move |id| self.contains(*id))
    }
    /// Raw bits, for cache fingerprints.
    pub fn bits(self) -> u64 {
        self.0
    }
    pub fn keys(self) -> Vec<&'static str> {
        self.iter().map(AppletId::key).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_order_matches_the_enum_and_keys_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for (i, a) in REGISTRY.iter().enumerate() {
            assert_eq!(a.id as usize, i, "{} out of order", a.key);
            assert!(seen.insert(a.key), "duplicate key {}", a.key);
            for al in a.aliases {
                assert_eq!(AppletId::parse(al), Some(a.id), "alias {al}");
            }
            assert_eq!(AppletId::parse(a.key), Some(a.id));
            assert!(a.key.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{} is not snake_case", a.key);
        }
        assert!(REGISTRY.len() < 64);
    }

    #[test]
    fn every_dux_item_id_names_an_applet() {
        for id in crate::config::DUX_BAR_ITEMS {
            assert!(AppletId::parse(id).is_some(), "{id}");
        }
    }

    #[test]
    fn the_contract_document_lists_every_key() {
        let doc = include_str!("../../docs/universal-panels.md");
        for a in REGISTRY {
            assert!(doc.contains(&format!("| `{}` |", a.key)), "docs/universal-panels.md lacks {}", a.key);
        }
    }

    #[test]
    fn parsing_ignores_case_and_separators() {
        assert_eq!(AppletId::parse("App-Menu"), Some(AppletId::AppMenu));
        assert_eq!(AppletId::parse("controlCenterButton"), Some(AppletId::ControlCenter));
        assert_eq!(AppletId::parse("nonsense"), None);
    }

    #[test]
    fn sets_hold_each_applet_independently() {
        let mut s = AppletSet::EMPTY;
        for id in AppletId::all() {
            assert!(!s.contains(id));
            s.set(id, true);
            assert!(s.contains(id));
        }
        assert_eq!(s, AppletSet::all());
        assert_eq!(s.with(AppletId::Clock, false).len(), REGISTRY.len() - 1);
    }
}
