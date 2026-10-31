//! The macOS-style menu bar: Apple menu, app name, dropdown menus, status icons, clock and the
//! Control Center. One surface that grows downward while a menu or the panel is open.

pub mod active;
mod applets;
pub mod awake;
mod ccviewport;
pub mod clock;
mod embed;
pub mod gate;
mod indicators;
pub mod lock;
pub use applets::{classic_symbol, theme_logo, FocusSignal};
pub use embed::GAP as STATUS_GAP;
pub mod menus;
pub mod overlay;
pub mod popup;
pub mod rdwm;
mod segmented;
mod status;
pub mod symbols;
pub mod vbar;
pub mod weather;

use crate::app::{App, Button, InputRect, Key, Tick, Waker};
use crate::controlcenter::{self, sys, Snapshot, Target};
use crate::gfx::{color, fill_rounded, stroke_rounded};
use crate::text::{Fonts, Weight};
use crate::theme::BAR_HEIGHT;
use crate::wm::Workspace;
use active::ActiveWindow;
use menus::{Dropdown, Menu, Runner};
use popup::{Calendar, Popup};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symbols::SymbolCache;
use tiny_skia::Pixmap;
use weather::Weather;

const TEXT_PX: f32 = 12.0;
/// Baseline row of menu-bar text (cap height ends here), measured from the original bar.
const BASELINE: f32 = 17.0;
const LEFT_PAD: f32 = 7.0;
const RIGHT_PAD: f32 = 7.0;
/// Floating bar (`bar_style = "floating"`): gap between the screen edge and the pill, and the extra
/// space the content keeps from the pill's rounded ends.
const PILL_MARGIN_X: f32 = 8.0;
const PILL_MARGIN_Y: f32 = 2.0;
const PILL_INSET: f32 = 10.0;

#[cfg(test)]
thread_local! {
    /// Tests never read the developer's real theme: floating is off unless a test switches it on.
    static TEST_FLOATING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Likewise the vertical layout.
    static TEST_VERTICAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// ...and where the pill bar sits while [`TEST_VERTICAL`] is on.
    static TEST_POSITION: std::cell::Cell<crate::config::BarPosition> = const { std::cell::Cell::new(crate::config::BarPosition::Left) };
}

fn floating() -> bool {
    #[cfg(test)]
    return TEST_FLOATING.with(|f| f.get());
    #[cfg(not(test))]
    crate::theme::th().bar_floating()
}

/// How far the content sits inside the strip: 0 for the flat bar, the pill's end cap for the floating one.
fn inset() -> f32 {
    if floating() {
        PILL_INSET
    } else {
        0.0
    }
}

/// Height of the classic or floating strip along a top or bottom edge: what the theme declares (`[bar] height`), else [`BAR_HEIGHT`].
fn bar_h() -> f32 {
    crate::theme::th().flat_bar_height()
}

/// Corner radius of the hover and open-menu highlights: slightly rounded on the strip, a pill on the floating bar.
fn item_radius() -> f32 {
    if floating() {
        (bar_h() - 6.0) / 2.0
    } else {
        5.0
    }
}
const BTN_W: f32 = 27.0;
const ICON_PX: f32 = 16.0;
/// Menu bar text colour (from the theme).
fn ink() -> (u8, u8, u8) {
    crate::theme::th().bar_text.t()
}
const MENU_GAP: f32 = 2.0;
const PANEL_GAP: f32 = 4.0;
const BOTTOM_MARGIN: f32 = 8.0;
/// While the Control Center is open its data is refreshed this often.
/// How long the pointer may be off the bar and its open menu/popup before that closes (a hand
/// moving from an icon to a slider often clips an edge; coming back in cancels the close).
const CLOSE_DELAY: Duration = Duration::from_millis(300);
const PANEL_REFRESH: Duration = Duration::from_secs(4);
/// How long drawing must stay quiet before the heap is trimmed.
const HEAP_QUIET: Duration = Duration::from_millis(750);
/// A second trim this long after the first one catches memory freed by finishing worker threads.
const HEAP_RECHECK: Duration = Duration::from_secs(4);

// ------------------------------------------------------------------ layout

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub x: f32,
    pub w: f32,
}

/// Horizontal boxes of the left-hand titles: label advance + 18 (22 for the Apple icon).
pub fn title_spans(menus: &[Menu], label_w: impl Fn(&Menu) -> f32) -> Vec<Span> {
    let mut x = LEFT_PAD;
    menus
        .iter()
        .map(|m| {
            let w = label_w(m) + if m.icon { 22.0 } else { 18.0 };
            let s = Span { x, w };
            x += w;
            s
        })
        .collect()
}

/// Command line that opens the dashboard hanging under the bar item `item`.
fn dash_argv(program: &str, item: Span) -> Vec<String> {
    vec![program.to_string(), "--x".to_string(), format!("{}", (item.x + item.w / 2.0).round() as i32)]
}

/// Made-up reading for `--png` screenshots when there is no cached one (shows every row of the popup).
pub(crate) fn sample_weather() -> Weather {
    let day = |date: &str, high_c, low_c, code| weather::Day { date: date.into(), high_c, low_c, code };
    Weather {
        temp_c: 9,
        code: 122,
        feels_c: Some(6),
        humidity: Some(80),
        wind_kmh: Some(14),
        wind_dir: "NW".into(),
        desc: "Overcast".into(),
        location: "Berlin, Germany".into(),
        days: vec![day("2026-10-29", 11, 4, 119), day("2026-10-30", 8, 2, 296), day("2026-10-31", 6, -1, 338)],
        fetched: weather::now(),
        ..Weather::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tray {
    Indicator(u64),
    Indicators,
    Workspaces,
    Layout,
    Weather,
    Awake,
    Agents,
    Battery,
    Bluetooth,
    Audio,
    Network,
    Dash,
    Spotlight,
    Control,
    Clock,
    // The OS icon / launcher, the focused window, the power button, caps/num lock.
    Logo,
    Window,
    Power,
    Lock,
    // Applets every renderer can show (see crate::panel): the folded application menus, media, notification history,
    // clipboard history, CPU / memory load, brightness, one task button per window, show desktop.
    AppMenu,
    Media,
    Notifications,
    Clipboard,
    Cpu,
    Memory,
    Brightness,
    Task(u64),
    ShowDesktop,
    /// The overflow cell: applets that do not fit, in a list.
    More,
}
pub type IndicatorRect = (Tray, (f32, f32, f32, f32));

/// Width of one workspace dot cell and the padding around the group.
const WS_SLOT: f32 = 14.0;
const WS_PAD: f32 = 10.0;

/// Width of the workspace indicator for `count` workspaces.
pub fn workspaces_width(count: usize) -> f32 {
    count as f32 * WS_SLOT + WS_PAD
}

/// Which workspace dot (if any) lies under `x`, given the indicator's span.
pub fn workspace_hit(span: Span, count: usize, x: f32) -> Option<usize> {
    let rel = x - (span.x + WS_PAD / 2.0);
    if rel < 0.0 {
        return None;
    }
    let i = (rel / WS_SLOT) as usize;
    (i < count && x < span.x + span.w).then_some(i)
}

/// Lays the right-hand items out right-to-left from the bar's edge. `items` is in left-to-right order.
pub fn tray_spans(width: f32, items: &[(Tray, f32)]) -> Vec<(Tray, Span)> {
    let mut x = width - RIGHT_PAD;
    let mut out: Vec<(Tray, Span)> = items
        .iter()
        .rev()
        .map(|&(kind, w)| {
            x -= w;
            (kind, Span { x, w })
        })
        .collect();
    out.reverse();
    out
}

/// How long until the next wake-up: the minute boundary, or sooner while the panel needs refreshing.
pub fn next_wake(tm: &clock::Tm, panel_open: bool) -> Duration {
    let minute = clock::until_next_minute(tm);
    if panel_open {
        minute.min(PANEL_REFRESH)
    } else {
        minute
    }
}

/// Height of the bar's surface. It never changes while panels open: resizing a layer surface makes
/// Hyprland animate (stretch) it, which looks glitchy. Only the input region follows what is open.
const SURFACE_MIN_HEIGHT: u32 = 420;

/// Window height needed for the bar plus whatever is open below it (never less than [`SURFACE_MIN_HEIGHT`]).
pub fn surface_height(open_bottom: Option<f32>) -> u32 {
    match open_bottom {
        Some(b) => ((b + BOTTOM_MARGIN).ceil() as u32).max(SURFACE_MIN_HEIGHT),
        None => SURFACE_MIN_HEIGHT,
    }
}

// ------------------------------------------------------------------- state

#[derive(Debug, Clone, Copy, PartialEq)]
enum Open {
    None,
    Menu(usize),
    Control,
    /// A popup under a status item (layouts, sound, Wi-Fi, Bluetooth, battery, calendar).
    Popup(Tray),
}

struct Shared {
    snap: Mutex<Snapshot>,
    weather: Mutex<Option<Weather>>,
    changed: AtomicBool,
    snap_busy: AtomicBool,
    weather_busy: AtomicBool,
    /// The last fetch failed (offline): the popup says its reading is old.
    weather_failed: AtomicBool,
    layout: rdwm::Shared,
    wifi: Mutex<Vec<sys::WifiNet>>,
    bt: Mutex<Vec<sys::BtDevice>>,
    lists_busy: AtomicBool,
    /// When this process last changed the volume itself (probe results right after are stale).
    vol_set_at: Mutex<Option<Instant>>,
}

type ResourceSample = (Option<f32>, Option<f32>);

pub struct DecBar {
    indicator_host: Option<crate::sni::Host>,
    indicators: crate::sni::Snapshot,
    indicator_path: Vec<i32>,
    indicator_page: usize,
    indicator_origin: (i32, i32),
    indicator_anchor: (i32, i32),
    indicator_debug_file: Option<(std::path::PathBuf, String)>,
    fonts: Option<Fonts>,
    symbols: SymbolCache,
    active: Box<dyn ActiveWindow>,
    runner: Runner,
    app_name: String,
    menus: Vec<Menu>,
    open: Open,
    hover_title: Option<usize>,
    hover_row: Option<usize>,
    /// Action awaiting a second click (destructive items when no helper script is installed).
    confirm: Option<&'static str>,
    width: u32,
    height: u32,
    clock_text: String,
    last_minute: Option<(u32, u32)>,
    shared: Arc<Shared>,
    waker: Option<Waker>,
    // Control Center
    cc_items: Vec<controlcenter::Item>,
    cc_height: f32,
    /// Content width of the laid-out panel (before the viewport's zoom), and the host room it was laid out for.
    cc_w: f32,
    cc_room: f32,
    cc_viewport: ccviewport::Viewport,
    cc_hover: Option<Target>,
    cc_drag: Option<Target>,
    cc_last_send: Instant,
    popup: Option<Popup>,
    calendar: Option<Calendar>,
    popup_hover: Option<usize>,
    popup_drag: Option<usize>,
    /// When the pointer left the surface with something open; it closes [`CLOSE_DELAY`] later unless the pointer returns.
    leave_at: Option<Instant>,
    /// Test hook (`DECBAR_DEBUG_TRAY=file`): where the status items' centres are written after each draw.
    debug_tray: Option<(std::path::PathBuf, String)>,
    /// Test hook (`DECBAR_DEBUG_PANEL=file`): the open Control Center's rectangle, scroll, content height, every control's rectangle and the
    /// keyboard focus, rewritten when they change (the same format `overlay.rs` writes, so one driver serves both hosts).
    debug_panel: Option<(std::path::PathBuf, String)>,
    last_full_refresh: Instant,
    // Window-manager workspaces (optional indicator)
    show_workspaces: bool,
    workspaces: Vec<Workspace>,
    /// When the heap was last churned (drawing, font parsing); cleared once it has been trimmed.
    heap_dirty: Option<Instant>,
    heap_rechecked: bool,
    /// Where an embedded host (the Windows taskbar) put the open popup; see [`embed`].
    embed: embed::Place,
    /// Set by [`DecBar::with_gate`]: lets the bar close itself while `bar.enabled` is off.
    gate: Option<gate::Gate>,
    /// A status engine inside another host (the Windows taskbar): never draws the vertical layout.
    embedded: bool,
    // The vertical layout (see [`vbar`]).
    /// Where the pill bar was last seen ([`DecBar::place`]); a change closes what is open and re-lays out.
    was_place: Option<vbar::Place>,
    /// Which decbar process this is: the main one, or the bottom half of a split layout.
    process: vbar::Process,
    /// `decbar --position`: draws the pill bar there whatever `bar.position` says (screenshots).
    position_override: Option<crate::config::BarPosition>,
    /// The live main bar starts the bottom process itself while the layout is split.
    companion: bool,
    bottom_child: Option<std::process::Child>,
    bottom_retry: Instant,
    bottom_spawns: u32,
    /// The program that is the bottom bar (tests); the sibling `decbar` when `None`.
    bottom_exe: Option<String>,
    v_hover: Option<Tray>,
    v_hover_ws: Option<usize>,
    v_hover_open: Option<(Tray, Instant)>,
    v_icons: crate::icons::IconCache,
    /// Focused window: (app id, title), and the width of its title run.
    v_win: Option<(String, String)>,
    v_win_len: f32,
    logo_symbol: String,
    locks: lock::Locks,
    /// The lock-key watcher, while the lock-keys applet is on.
    lock_watcher: Option<lock::Watch>,
    /// The volume subscription, while the volume applet or the Control Center button is on.
    volume_watch: Option<sys::VolumeWatch>,
    /// Ends the rdwm layout/tag subscription, while neither the layout nor the workspaces applet is on.
    layout_stop: Option<Arc<AtomicBool>>,
    dux_mpris: Option<crate::dash::mpris::Mpris>,
    dux_perf: Arc<Mutex<Option<ResourceSample>>>,
    dux_sampler: Arc<Mutex<crate::dash::sysinfo::Sampler>>,
    dux_busy: Arc<AtomicBool>,
    dux_refresh: Instant,
    dux_hover: Option<segmented::Kind>,
    dux_material_generation: u64,
    frame_theme_generation: Option<u64>,
    dux_effect: Option<(segmented::Kind, Instant, bool)>,
    dux_reduce_motion: bool,
    /// Task buttons (one per application) while the task applet is on.
    tasks: Vec<applets::Task>,
    /// Windows show desktop minimised, to bring back on the next press.
    desk_hidden: Vec<u64>,
    /// The folded application menu shows this menu's items (`None`: the list of menus).
    app_menu_page: Option<usize>,
    /// An applet opened from the overflow list (its popup hangs from the overflow cell).
    more_target: Option<Tray>,
    /// Keyboard mode (`decbar --focus`): the applet with the focus ring, by its place in [`DecBar::focus_items`].
    kb_focus: Option<usize>,
    /// Set by the `--focus` signal; the tick turns it into keyboard mode.
    focus_request: Arc<AtomicBool>,
    /// How the signal thread wakes the loop (filled in once the loop has a waker).
    focus_waker: Arc<Mutex<Option<Waker>>>,
    /// The theme's logo, rendered once per size and colour.
    brand_cache: Option<(applets::LogoKey, Option<Pixmap>)>,
    /// The resolved panel and the theme generation it was resolved for (it is asked for many times a frame).
    panel_cache: std::cell::Cell<Option<(u64, crate::panel::Effective)>>,
    /// What an embedding host (the taskbar) had no room for: the overflow cell lists it.
    embed_hidden: std::cell::RefCell<Vec<Tray>>,
}

impl DecBar {
    pub fn new(active: Box<dyn ActiveWindow>) -> Self {
        let shared = Arc::new(Shared {
            snap: Mutex::new(Snapshot { light: sys::is_light(), awake: sys::stay_awake(), ..Snapshot::default() }),
            // unit tests never read the real cache
            weather: Mutex::new(if cfg!(test) { None } else { weather::fresh_cached() }),
            changed: AtomicBool::new(false),
            snap_busy: AtomicBool::new(false),
            weather_busy: AtomicBool::new(false),
            weather_failed: AtomicBool::new(false),
            layout: Arc::new(Mutex::new(None)),
            wifi: Mutex::new(Vec::new()),
            bt: Mutex::new(Vec::new()),
            lists_busy: AtomicBool::new(false),
            vol_set_at: Mutex::new(None),
        });
        let mut bar = DecBar {
            indicator_host: None,
            indicators: crate::sni::Snapshot::default(),
            indicator_path: Vec::new(),
            indicator_page: 0,
            indicator_origin: (0, 0),
            indicator_anchor: (0, 0),
            indicator_debug_file: std::env::var_os("DECBAR_DEBUG_INDICATORS").map(|p| (p.into(), String::new())),
            fonts: Fonts::load(),
            symbols: SymbolCache::default(),
            active,
            runner: Runner::detect(),
            app_name: "Finder".into(),
            menus: Vec::new(),
            open: Open::None,
            hover_title: None,
            hover_row: None,
            confirm: None,
            width: 0,
            height: bar_h().ceil() as u32,
            clock_text: String::new(),
            last_minute: None,
            shared,
            waker: None,
            cc_items: Vec::new(),
            cc_height: 0.0,
            cc_w: 0.0,
            cc_room: 0.0,
            cc_viewport: ccviewport::Viewport::default(),
            cc_hover: None,
            cc_drag: None,
            cc_last_send: Instant::now(),
            popup: None,
            calendar: None,
            popup_hover: None,
            popup_drag: None,
            leave_at: None,
            debug_tray: std::env::var_os("DECBAR_DEBUG_TRAY").map(|p| (std::path::PathBuf::from(p), String::new())),
            debug_panel: std::env::var_os("DECBAR_DEBUG_PANEL").map(|p| (std::path::PathBuf::from(p), String::new())),
            last_full_refresh: Instant::now(),
            show_workspaces: false,
            workspaces: Vec::new(),
            heap_dirty: None,
            heap_rechecked: false,
            embed: embed::Place::default(),
            gate: None,
            embedded: false,
            was_place: None,
            process: vbar::Process::Main,
            position_override: None,
            companion: false,
            bottom_child: None,
            bottom_retry: Instant::now(),
            bottom_spawns: 0,
            bottom_exe: None,
            v_hover: None,
            v_hover_ws: None,
            v_hover_open: None,
            v_icons: crate::icons::IconCache::default(),
            v_win: None,
            v_win_len: 0.0,
            logo_symbol: String::new(),
            locks: lock::Locks::default(),
            lock_watcher: None,
            volume_watch: None,
            layout_stop: None,
            dux_mpris: None,
            dux_perf: Arc::new(Mutex::new(None)),
            dux_sampler: Arc::new(Mutex::new(crate::dash::sysinfo::Sampler::new(crate::dash::sysinfo::Paths::default()))),
            dux_busy: Arc::new(AtomicBool::new(false)),
            dux_refresh: Instant::now() - Duration::from_secs(5),
            dux_hover: None,
            dux_material_generation: 0,
            frame_theme_generation: None,
            dux_effect: None,
            dux_reduce_motion: crate::accessibility::Preferences::from_env().reduce_motion,
            tasks: Vec::new(),
            desk_hidden: Vec::new(),
            app_menu_page: None,
            more_target: None,
            brand_cache: None,
            kb_focus: None,
            focus_request: Arc::new(AtomicBool::new(false)),
            focus_waker: Arc::new(Mutex::new(None)),
            embed_hidden: std::cell::RefCell::new(Vec::new()),
            panel_cache: std::cell::Cell::new(None),
        };
        if bar.dux() || bar.cc_style() == controlcenter::settings::Style::Dux {
            bar.symbols.prepare_material();
        }
        bar.dux_material_generation = crate::theme::generation();
        bar.was_place = bar.place();
        if bar.was_place.is_some() {
            bar.logo_symbol = vbar::logo_symbol();
        }
        bar.refresh_app_name();
        bar
    }

    /// Opt-in for a real bar, never offscreen PNG/preview construction.
    pub fn with_frame_publisher(mut self) -> Self {
        self.frame_theme_generation = Some(crate::theme::generation());
        crate::theme::frame::publish(&crate::theme::th());
        self
    }

    /// Which half of a split layout this process draws (`decbar --role bottom`); the default is the main bar.
    pub fn with_process(mut self, process: vbar::Process) -> Self {
        self.process = process;
        self.panel_cache.set(None);
        self.was_place = self.place();
        if process == vbar::Process::Bottom {
            // two bars must not write the same test file
            if let Some((path, _)) = self.debug_tray.as_mut() {
                let mut name = path.clone().into_os_string();
                name.push(".bottom");
                *path = name.into();
            }
        }
        self
    }

    /// Draws the pill bar at `position` whatever `bar.position` says (`decbar --position`, for screenshots).
    pub fn with_position(mut self, position: Option<crate::config::BarPosition>) -> Self {
        self.position_override = position;
        self.panel_cache.set(None);
        self.was_place = self.place();
        self
    }

    /// The main bar starts the bottom bar's process while the layout is split and keeps it running (the live bar only).
    pub fn with_companion(mut self) -> Self {
        self.companion = true;
        self
    }

    /// Lets the bar close itself (the backend returns) while `bar.enabled` in `decebal.json` says it is off;
    /// see [`gate`]. `closed` is set to true when that happens.
    pub fn with_gate(mut self, closed: Arc<AtomicBool>) -> Self {
        self.gate = Some(gate::Gate::new(closed));
        self
    }

    /// Screen height for `--png` renders of the vertical layout (the live bar takes it from the output).
    pub fn with_height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    /// Show the workspace/tag dots when the window manager reports more than one workspace.
    pub fn with_workspaces(mut self, show: bool) -> Self {
        self.show_workspaces = show;
        self.refresh_workspaces();
        self
    }

    fn refresh_workspaces(&mut self) -> bool {
        if !self.show_workspaces {
            return false;
        }
        let ws = self.active.workspaces();
        if ws == self.workspaces {
            return false;
        }
        self.workspaces = ws;
        true
    }

    /// Renders one frame to a PNG (for testing and screenshots). `open` is a menu key or `control`.
    pub fn render_png(&mut self, path: &str, width: u32, open: Option<&str>) -> Result<(), String> {
        self.width = width;
        self.tick();
        if let Ok(mut s) = self.shared.snap.lock() {
            *s = Snapshot::collect();
        }
        self.shared.weather.lock().map_err(|e| e.to_string())?.get_or_insert_with(sample_weather);
        if self.pill_bar() {
            return self.v_render_png(path, width, open);
        }
        match open {
            Some("calendar") => self.open_popup(Tray::Clock),
            Some("weather") => self.open_popup(Tray::Weather),
            Some("control") => {
                self.relayout_control();
                self.open = Open::Control;
            }
            Some(key) => {
                let idx = self.menus.iter().position(|m| m.key == key).ok_or_else(|| format!("unknown menu {key}"))?;
                self.open_menu(idx);
            }
            None => {}
        }
        let h = self.size().1;
        let mut pm = Pixmap::new(width.max(1), h.max(1)).ok_or("cannot allocate pixmap")?;
        self.draw(&mut pm);
        pm.save_png(path).map_err(|e| e.to_string())
    }

    /// Native headless gallery frame. No tick, probes, watchers, popups or companion process.
    pub fn preview_frame(width: u32, height: u32, bottom: bool) -> Result<Pixmap, String> {
        let mut bar = Self::new(Box::new(active::NoActiveWindow));
        bar.width = width;
        bar.height = height;
        bar.process = if bottom { vbar::Process::Bottom } else { vbar::Process::Main };
        bar.clock_text = "Fri 23 Oct  14:07".into();
        bar.last_minute = Some((14, 7));
        bar.app_name = "Sample application".into();
        bar.show_workspaces = true;
        bar.workspaces = crate::widgets::data::sample().tags.into_iter().map(|t| Workspace { name: t.name, active: t.active, occupied: t.occupied, urgent: t.urgent }).collect();
        // fixed sample data (the gallery says so), so every enabled applet shows; nothing is probed
        bar.fill_sample();
        let mut pm = Pixmap::new(width, height).ok_or("preview canvas")?;
        bar.draw(&mut pm);
        Ok(pm)
    }

    /// Starts the push sources (layout changes from rdwm, volume changes) that wake the loop through `waker`.
    fn start_watchers(&mut self, waker: Waker) {
        self.waker = Some(waker.clone());
        self.ensure_indicator_host();
        let tw = waker;
        crate::theme::live::start_watcher(move || tw.wake());
        self.sync_workers();
    }

    fn snapshot(&self) -> Snapshot {
        self.shared.snap.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn update_snapshot(&self, f: impl FnOnce(&mut Snapshot)) {
        if let Ok(mut s) = self.shared.snap.lock() {
            f(&mut s);
        }
    }

    fn refresh_app_name(&mut self) -> bool {
        let name = self.active.active_app_name().map(|id| active::display_name(&id)).unwrap_or_else(|| applets::branding().desktop_label("Finder").to_string());
        let light = self.snapshot().light;
        if name != self.app_name || self.menus.is_empty() {
            self.app_name = name;
            self.menus = menus::build(&self.app_name, light);
            return true;
        }
        false
    }

    fn label_w(&self, m: &Menu) -> f32 {
        let Some(f) = &self.fonts else { return 40.0 };
        if m.icon {
            return 12.0;
        }
        f.measure(&m.title, TEXT_PX, if m.bold { Weight::Bold } else { Weight::Medium })
    }

    fn spans(&self) -> Vec<Span> {
        if self.dux() || !self.shows(crate::panel::AppletId::AppMenu) {
            return Vec::new();
        }
        let dx = inset() + self.left_items().last().map_or(0.0, |(_, s)| s.x + s.w - LEFT_PAD - inset());
        let mut spans = title_spans(&self.menus, |m| self.label_w(m));
        if dx > 0.0 {
            spans.iter_mut().for_each(|s| s.x += dx);
        }
        spans
    }

    /// Width of an item on the classic strip.
    fn classic_width(&self, kind: Tray, snap: &Snapshot, weather: Option<&Weather>) -> f32 {
        let measure = |s: &str, px: f32, w: Weight| self.fonts.as_ref().map(|f| f.measure(s, px, w)).unwrap_or(s.chars().count() as f32 * px * 0.55);
        match kind {
            Tray::Workspaces => workspaces_width(self.workspaces.len()),
            Tray::Layout => 30.0,
            Tray::Weather => weather.map_or(BTN_W, |w| ICON_PX + 6.0 + measure(&format!("{}°", w.temp_c), TEXT_PX, Weight::Regular) + 18.0),
            Tray::Battery => snap.battery.as_ref().map_or(BTN_W, |b| 30.0 + measure(&format!("{}%", b.percent), 11.0, Weight::Regular) + 6.0),
            Tray::Clock => measure(&self.clock_text, TEXT_PX, Weight::Regular) + 17.0,
            Tray::Window => measure(&self.v_window_text(), TEXT_PX, Weight::Medium).min(220.0) + 16.0,
            Tray::Media => ICON_PX + 6.0 + measure(&self.dux_player().map(|p| p.meta.title).unwrap_or_default(), TEXT_PX, Weight::Regular).min(140.0) + 14.0,
            Tray::Cpu | Tray::Memory => measure("CPU 100%", 11.0, Weight::Regular) + 14.0,
            Tray::Logo => {
                let b = applets::branding();
                match &b.launcher_label {
                    Some(l) => measure(l, TEXT_PX, Weight::Medium) + 18.0 + if matches!(b.logo, crate::panel::branding::Logo::None) { 0.0 } else { ICON_PX + 5.0 },
                    None => BTN_W,
                }
            }
            _ => BTN_W,
        }
    }

    /// The right-hand items and those that did not fit (they go to the overflow cell, which takes their place).
    fn classic_fit(&self) -> (Vec<(Tray, f32)>, Vec<Tray>) {
        let snap = self.snapshot();
        let weather = self.shared.weather.lock().ok().and_then(|w| w.clone());
        let order: Vec<_> = applets::CLASSIC_ORDER.iter().copied().filter(|id| *id != crate::panel::AppletId::Tray).collect();
        let mut kinds = self.applet_items(&order, 0);
        // the tray's application cells go where the menu bar always had them: before the workspaces
        let at = kinds.iter().position(|k| !matches!(k, Tray::Window | Tray::Task(_))).unwrap_or(kinds.len());
        let left_end = self.spans().last().map_or(LEFT_PAD + inset() + if self.shows(crate::panel::AppletId::Launcher) { BTN_W } else { 0.0 }, |s| s.x + s.w);
        let room = (self.width as f32 - inset() - RIGHT_PAD - left_end - 12.0).max(0.0);
        let width = |ks: &[Tray]| ks.iter().map(|k| self.classic_width(*k, &snap, weather.as_ref())).sum::<f32>();
        let mut hidden = Vec::new();
        if self.width > 0 {
            // what gives way first on a narrow screen; the clock and quick settings stay
            for victim in [
                Tray::Window,
                Tray::Media,
                Tray::Cpu,
                Tray::Memory,
                Tray::Weather,
                Tray::Clipboard,
                Tray::Notifications,
                Tray::Agents,
                Tray::Dash,
                Tray::Spotlight,
                Tray::Brightness,
                Tray::Layout,
                Tray::Lock,
                Tray::ShowDesktop,
                Tray::Power,
                Tray::Bluetooth,
                Tray::Battery,
                Tray::Network,
                Tray::Audio,
                Tray::Workspaces,
                Tray::Awake,
            ] {
                let more = if hidden.is_empty() { 0.0 } else { BTN_W };
                if width(&kinds) + more <= room {
                    break;
                }
                let tasks = victim == Tray::Window;
                kinds.retain(|k| {
                    let gone = *k == victim || (tasks && matches!(k, Tray::Task(_)));
                    if gone {
                        hidden.push(*k);
                    }
                    !gone
                });
            }
        }
        let used = width(&kinds) + if hidden.is_empty() { 0.0 } else { BTN_W };
        let at = at.min(kinds.len());
        if self.shows(crate::panel::AppletId::Tray) {
            let limit = ((room - used).max(0.0) / BTN_W).floor() as usize;
            let cells = self.indicator_kinds(limit.clamp(1, 8));
            kinds.splice(at..at, cells);
        }
        if !hidden.is_empty() {
            let clock = kinds.iter().position(|k| *k == Tray::Clock).unwrap_or(kinds.len());
            kinds.insert(clock, Tray::More);
        }
        (kinds.into_iter().map(|k| (k, self.classic_width(k, &snap, weather.as_ref()))).collect(), hidden)
    }

    fn tray_items(&self) -> Vec<(Tray, f32)> {
        self.classic_fit().0
    }

    /// The applets the overflow cell lists, whatever the renderer.
    pub(super) fn hidden_items(&self) -> Vec<Tray> {
        if self.embedded {
            return self.embed_hidden.borrow().clone();
        }
        if self.pill_bar() {
            return self.v_hidden();
        }
        if self.dux() {
            return self.dux_hidden();
        }
        self.classic_fit().1
    }

    /// Items at the left edge, before the menus: the launcher.
    fn left_items(&self) -> Vec<(Tray, Span)> {
        if self.dux() || !self.shows(crate::panel::AppletId::Launcher) {
            return Vec::new();
        }
        let snap = self.snapshot();
        vec![(Tray::Logo, Span { x: LEFT_PAD + inset() - 2.0, w: self.classic_width(Tray::Logo, &snap, None) })]
    }

    fn tray(&self) -> Vec<(Tray, Span)> {
        if self.dux() {
            return self.dux_layout().items.into_iter().filter_map(|i| if let segmented::Kind::Tray(k) = i.kind { Some((k, Span { x: i.x, w: i.w })) } else { None }).collect();
        }
        let mut out = self.left_items();
        out.extend(tray_spans(self.width as f32 - inset(), &self.tray_items()));
        out
    }

    /// Where the right-hand items start (the menus end before it).
    fn right_start(&self) -> f32 {
        self.tray().into_iter().find(|(k, _)| *k != Tray::Logo).map_or(self.width as f32, |(_, s)| s.x - 8.)
    }

    /// The edge of a horizontal bar (top unless the panel is on the bottom edge).
    pub(super) fn bottom(&self) -> bool {
        !self.embedded && !self.pill_bar() && self.panel().edge == crate::panel::Edge::Bottom
    }

    /// Surface y of the strip of a horizontal bar: 0 on top, the surface's last rows at the bottom.
    pub(super) fn strip_top(&self) -> f32 {
        if self.bottom() {
            (self.height as f32 - self.bar_bottom()).max(0.0)
        } else {
            0.0
        }
    }

    /// The point is on the strip of a horizontal bar.
    pub(super) fn in_strip(&self, y: f32) -> bool {
        let top = self.strip_top();
        y >= top && y < top + self.bar_bottom()
    }

    /// Top of a box `h` tall opened from the strip: under a top bar, over a bottom one.
    pub(super) fn open_y(&self, h: f32, gap: f32) -> f32 {
        if self.bottom() {
            (self.strip_top() - gap - h).max(0.0)
        } else {
            self.bar_bottom() + gap
        }
    }

    fn title_hit(&self, x: f32) -> Option<usize> {
        let limit = self.right_start();
        self.spans().iter().position(|s| s.x + s.w <= limit && x >= s.x && x < s.x + s.w)
    }

    fn dropdown(&self, idx: usize) -> Option<Dropdown> {
        let menu = self.menus.get(idx)?;
        let span = *self.spans().get(idx)?;
        let f = self.fonts.as_ref();
        let mut d = Dropdown::layout(menu, span.x, self.bar_bottom() + MENU_GAP, |s| f.map(|f| f.measure(s, TEXT_PX, Weight::Regular)).unwrap_or(7.0 * s.chars().count() as f32));
        if self.bottom() {
            d.shift_y(self.open_y(d.h, MENU_GAP) - d.y);
        }
        // Keep it on screen.
        let over = d.x + d.w - self.width as f32;
        if over > 0.0 && self.width > 0 {
            d.x = (d.x - over).max(0.0);
        }
        Some(d)
    }

    fn panel_origin(&self) -> (f32, f32) {
        if self.embedded {
            return self.embed.origin;
        }
        if self.pill_bar() {
            return self.v_panel_origin();
        }
        let right = self.tray().iter().find(|(k, _)| *k == Tray::Control).map(|(_, s)| s.x + s.w).unwrap_or(self.width as f32);
        let y = if self.bottom() { self.open_y(self.cc_visible_height(), PANEL_GAP) } else { self.bar_bottom() + PANEL_GAP };
        ((right - self.cc_width()).max(0.0), y)
    }

    fn open_bottom(&self) -> Option<f32> {
        if self.bottom() {
            // the surface grows upwards: the strip plus what hangs over it
            let depth = match self.open {
                Open::None => None,
                Open::Menu(i) => self.dropdown(i).map(|d| d.h + MENU_GAP),
                Open::Control => Some(PANEL_GAP + self.cc_height),
                Open::Popup(_) => self.popup.as_ref().map(|p| p.h + MENU_GAP).or(self.calendar.as_ref().map(|_| Calendar::H + MENU_GAP)),
            };
            return depth.map(|d| self.bar_bottom() + d);
        }
        match self.open {
            Open::None => None,
            Open::Menu(i) => self.dropdown(i).map(|d| d.y + d.h),
            Open::Control => Some(self.bar_bottom() + PANEL_GAP + self.cc_height),
            Open::Popup(_) => self.popup.as_ref().map(|p| p.y + p.h).or(self.calendar.as_ref().map(|c| c.y + Calendar::H)),
        }
    }

    fn layout_state(&self) -> Option<rdwm::LayoutState> {
        self.shared.layout.lock().ok().and_then(|l| l.clone())
    }

    // --------------------------------------------------------- data workers

    fn spawn_snapshot(&self, full: bool) {
        // Unit tests set the snapshot by hand; a worker probing the real machine (pactl, nmcli, ...) would overwrite it,
        // and which tools exist differs per machine (CI has none).
        if cfg!(test) {
            return;
        }
        // the open Control Center asks for a full snapshot itself; the minute refresh probes only what applets show
        let want = self.probes();
        if !full && !want.any() {
            return;
        }
        if self.shared.snap_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let prev = shared.snap.lock().map(|s| s.clone()).unwrap_or_default();
            let next = if full { Snapshot::collect() } else { collect_light(&prev, want) };
            // A volume the user just set (the wheel, a slider) is still being applied by the worker: a probe taken
            // meanwhile reads the old value and would undo the next wheel step.
            let just_set = shared.vol_set_at.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < Duration::from_secs(1));
            if let Ok(mut s) = shared.snap.lock() {
                let volume = s.volume.clone();
                *s = next;
                if just_set {
                    s.volume = volume;
                }
            }
            shared.changed.store(true, Ordering::SeqCst);
            shared.snap_busy.store(false, Ordering::SeqCst);
            if let Some(w) = waker {
                w.wake();
            }
        });
    }

    pub(super) fn spawn_weather(&self) {
        // Unit tests never reach the network (or the on-disk cache); with the weather applet off nothing fetches.
        if cfg!(test) || !self.shows(crate::panel::AppletId::Weather) {
            return;
        }
        if self.shared.weather_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let fetched = weather::fetch();
            crate::mem::trim();
            shared.weather_failed.store(fetched.is_none(), Ordering::SeqCst);
            if let Some(w) = fetched {
                if let Ok(mut slot) = shared.weather.lock() {
                    *slot = Some(w);
                }
            }
            // also on failure: an open popup says "couldn't refresh"
            shared.changed.store(true, Ordering::SeqCst);
            shared.weather_busy.store(false, Ordering::SeqCst);
            if let Some(w) = waker {
                w.wake();
            }
        });
    }

    pub(super) fn weather_stale(&self) -> bool {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.shared.weather.lock().ok().and_then(|w| w.clone()).map(|w| !weather::is_fresh(&w, now)).unwrap_or(true)
    }

    // ------------------------------------------------------------- opening

    fn close(&mut self) -> bool {
        let was = self.open != Open::None;
        self.open = Open::None;
        self.hover_row = None;
        self.confirm = None;
        self.cc_hover = None;
        self.cc_drag = None;
        self.popup = None;
        self.calendar = None;
        self.popup_hover = None;
        self.popup_drag = None;
        self.leave_at = None;
        self.v_hover_open = None;
        was
    }

    /// Whether (x, y) is on the bar strip or on what is open below it (menu, panel, popup, and the bridge to it).
    fn over_open_ui(&self, x: f32, y: f32) -> bool {
        self.input_rects().iter().any(|r| x >= r.x as f32 && x < (r.x + r.w) as f32 && y >= r.y as f32 && y < (r.y + r.h) as f32)
    }

    fn open_menu(&mut self, idx: usize) {
        self.open = Open::Menu(idx);
        self.hover_row = None;
        self.confirm = None;
    }

    fn open_control(&mut self) {
        self.cc_viewport = ccviewport::Viewport::default();
        self.open = Open::Control;
        self.relayout_control();
        self.last_full_refresh = Instant::now();
        self.spawn_snapshot(true);
    }

    /// How the panel is painted: the theme's `control_center.style`, `auto` meaning the flyout look under the Windows taskbar.
    fn cc_style(&self) -> crate::controlcenter::settings::Style {
        crate::theme::th().control_center.style.resolve(self.embed.taskbar)
    }

    /// Writes the `DECBAR_DEBUG_PANEL` report for the open Control Center (nothing without the variable).
    pub(super) fn debug_control_panel(&mut self, surface_w: u32, surface_h: u32) {
        if self.debug_panel.is_none() {
            return;
        }
        let (ox, oy) = self.panel_origin();
        let (scroll, z) = (self.cc_viewport.scroll, self.cc_zoom());
        // surface coordinates (logical pixels of this surface): where each control is drawn now, after the scroll and the zoom
        let mut text = format!(
            "panel {} {} {} {}\nsurface {} {}\nscroll {} content {}\nzoom {} width {}\n",
            ox,
            oy,
            self.cc_width(),
            self.cc_visible_height(),
            surface_w,
            surface_h,
            scroll,
            self.cc_height,
            z,
            self.cc_w
        );
        let mut seen = Vec::new();
        for item in &self.cc_items {
            if let Some(target) = item.target.filter(|t| !seen.contains(t)) {
                seen.push(target);
                text += &format!("target {target:?} {} {} {} {}\n", ox + item.r.x * z, oy + (item.r.y - scroll) * z, item.r.w * z, item.r.h * z);
            }
        }
        text += &format!("focus {}\n", self.cc_viewport.focus.map_or("none".to_string(), |t| format!("{t:?}")));
        text += &format!("hover {}\n", self.cc_hover.map_or("none".to_string(), |t| format!("{t:?}")));
        if let Some((path, last)) = self.debug_panel.as_mut() {
            if *last != text {
                let _ = std::fs::write(&*path, &text);
                *last = text;
            }
        }
    }

    /// Whether the open panel is clipped to the room it has and scrolls: every look, in every host (the menu bar, the
    /// overlay, the rail styles and the taskbar's engine).
    fn cc_clipped(&self) -> bool {
        self.cc_style().scrolls()
    }
    /// Width the panel may have in this host (logical pixels; infinite when the host does not know yet).
    pub(super) fn cc_room_w(&self) -> f32 {
        if self.embedded {
            return self.embed_room_w();
        }
        if self.pill_bar() {
            return self.v_cc_room_w();
        }
        if self.width > 0 {
            self.width as f32 - 2.0 * BOTTOM_MARGIN
        } else {
            f32::INFINITY
        }
    }
    fn cc_zoom(&self) -> f32 {
        self.cc_viewport.z()
    }
    /// Height of the visible panel on the surface (logical pixels, after the zoom).
    fn cc_visible_height(&self) -> f32 {
        let full = self.cc_height * self.cc_zoom();
        if !self.cc_clipped() {
            return full;
        }
        if self.embedded {
            return full.min(self.embed_room(self.embed.edge, self.embed.up, self.embed.screen_w).max(0.0));
        }
        if self.pill_bar() {
            return full.min(self.v_cc_room().max(0.0));
        }
        if self.bottom() {
            return full.min((self.strip_top() - PANEL_GAP - BOTTOM_MARGIN).max(0.0));
        }
        if self.height as f32 <= self.panel_origin().1 + 1.0 {
            // the surface has not grown for the panel yet (the backend's resize follows the open): nothing to clip to
            return full;
        }
        ccviewport::height(full, self.panel_origin().1, self.height as f32)
    }
    /// The visible height in the panel's own units (before the zoom): what scrolling and revealing work in.
    fn cc_view_height(&self) -> f32 {
        self.cc_visible_height() / self.cc_zoom()
    }
    fn cc_point(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        self.cc_viewport.point(x, y, self.panel_origin(), self.cc_width(), self.cc_visible_height())
    }
    /// Keeps the open panel fitted to its host: laid out again when the room changed (a resize, another edge), the scroll
    /// clamped, the keyboard focus in view.
    fn cc_sync_viewport(&mut self) {
        if self.open == Open::Control && (self.cc_room_w() - self.cc_room).abs() > 0.5 {
            self.relayout_control();
            return;
        }
        self.cc_viewport.clamp(self.cc_height, self.cc_view_height());
        if let Some((_, rect)) = ccviewport::targets(&self.cc_items).into_iter().find(|(target, _)| Some(*target) == self.cc_viewport.focus) {
            self.cc_viewport.reveal(rect, self.cc_height, self.cc_view_height());
        }
    }

    /// Width of the panel on the surface (after the zoom).
    fn cc_width(&self) -> f32 {
        let w = if self.cc_w > 0.0 { self.cc_w } else { controlcenter::panel_width(self.cc_style()) };
        w * self.cc_zoom()
    }

    fn relayout_control(&mut self) {
        let snap = self.snapshot();
        let room = self.cc_room_w();
        let f = controlcenter::layout_fitted(&snap, &crate::theme::th().control_center.tiles, self.cc_style(), room);
        self.cc_items = f.items;
        self.cc_height = f.height;
        self.cc_w = f.width;
        self.cc_viewport.zoom = f.zoom;
        self.cc_room = room;
        self.cc_viewport.clamp(self.cc_height, self.cc_view_height());
        if let Some((_, rect)) = ccviewport::targets(&self.cc_items).into_iter().find(|(target, _)| Some(*target) == self.cc_viewport.focus) {
            self.cc_viewport.reveal(rect, self.cc_height, self.cc_view_height());
        }
    }

    // ------------------------------------------------------------- actions

    fn run_menu_item(&mut self, menu: usize, item: usize) -> bool {
        let Some(it) = self.menus.get(menu).and_then(|m| m.items.get(item)).cloned() else { return false };
        if it.separator || !self.runner.available(it.action) {
            return false;
        }
        if self.runner.needs_confirm(it.action) && self.confirm != Some(it.action) {
            self.confirm = Some(it.action);
            return true;
        }
        self.close();
        if !self.runner.run(it.action) {
            eprintln!("decbar: could not run action {}", it.action);
        }
        true
    }

    fn activate(&mut self, t: Target, x: f32, w: f32, px: f32) {
        let snap = self.snapshot();
        match t {
            Target::Wifi => {
                let on = snap.net.as_ref().map(|n| n.wifi_enabled).unwrap_or(false);
                sys::set_wifi(!on);
                self.update_snapshot(|s| {
                    if let Some(n) = s.net.as_mut() {
                        n.wifi_enabled = !on;
                    }
                });
            }
            Target::Bluetooth => {
                let on = snap.bluetooth.as_ref().map(|b| b.powered).unwrap_or(false);
                sys::set_bluetooth(!on);
                self.update_snapshot(|s| {
                    if let Some(b) = s.bluetooth.as_mut() {
                        b.powered = !on;
                    }
                });
            }
            Target::Network => {
                for tool in ["nm-connection-editor", "nmtui"] {
                    if sys::have(tool) {
                        self.close();
                        if tool == "nmtui" {
                            continue;
                        }
                        sys::spawn_application(&["sh", "-c", "sleep 0.15; exec \"$@\"", "network-editor", tool]);
                        break;
                    }
                }
            }
            Target::Focus => {
                sys::toggle_dnd();
                self.update_snapshot(|s| s.dnd = s.dnd.map(|d| !d));
            }
            Target::NightShift => {
                sys::toggle_night_shift();
                self.update_snapshot(|s| s.night_shift = s.night_shift.map(|d| !d));
            }
            Target::DarkMode => {
                self.close();
                sys::toggle_appearance();
            }
            Target::Brightness | Target::Volume => {
                self.cc_drag = Some(t);
                self.slide(t, x, w, px, true);
            }
            Target::MediaPrev => sys::media_command("previous"),
            Target::MediaToggle => sys::media_command("play-pause"),
            Target::MediaNext => sys::media_command("next"),
            Target::StayAwake => {
                awake::clear_until();
                sys::toggle_stay_awake();
                self.update_snapshot(|s| s.awake = !s.awake);
            }
            Target::Screenshot => {
                self.close();
                // through the dry-run aware application hook like every other control (this used to call `launch::spawn_argv` directly)
                let argv = crate::capture::command();
                sys::spawn_application(&argv.iter().map(String::as_str).collect::<Vec<_>>());
            }
            Target::Mute => {
                sys::toggle_mute();
                self.update_snapshot(|s| {
                    if let Some(v) = s.volume.as_mut() {
                        v.muted = !v.muted;
                    }
                });
            }
            Target::Lock => {
                self.close();
                // only where a lock program exists; a refused spawn is not reported as success
                if let Some(argv) = sys::lock_command() {
                    sys::spawn_application(&argv.iter().map(String::as_str).collect::<Vec<_>>());
                }
            }
            Target::Power => {
                self.close();
                if let Some(menu) = sys::session_menu() {
                    sys::spawn_application(&[&menu]);
                }
            }
            Target::Record => {
                self.close();
                if let Some(m) = sys::decutil() {
                    // Let the panel disappear before the first frame is grabbed.
                    sys::spawn_application(&["sh", "-c", "sleep 0.25; exec \"$@\"", "screen-recording", &m, "record", "toggle"]);
                } else if sys::have("omarchy") {
                    sys::spawn_application(&["sh", "-c", "sleep 0.15; exec omarchy capture screenrecording"]);
                }
            }
            Target::IdleInhibit => {
                sys::util_toggle("idle-inhibit");
                self.update_snapshot(|s| {
                    if let Some(u) = s.util.as_mut() {
                        u.idle = !u.idle;
                    }
                });
            }
            Target::GameMode => {
                sys::util_toggle("gamemode");
                self.update_snapshot(|s| {
                    if let Some(u) = s.util.as_mut() {
                        u.game = !u.game;
                    }
                });
            }
            Target::Dnd => {
                sys::util_toggle("dnd");
                self.update_snapshot(|s| s.dnd = s.dnd.map(|d| !d));
            }
            Target::Notifications => {
                self.close();
                // The history panel of decnotify (a toggle), once the control center is gone.
                sys::spawn_application(&["sh", "-c", "sleep 0.15; exec decnotify history"]);
            }
            Target::Settings => {
                self.close();
                if let Some(argv) = settings_command(sys::have("decsettings"), sys::have("omarchy-menu")) {
                    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
                    sys::spawn_application(&argv);
                }
            }
        }
        self.relayout_control();
    }

    /// Applies a slider drag at pointer x. `commit` forces sending now (press/release); otherwise throttled.
    fn slide(&mut self, t: Target, x: f32, w: f32, px: f32, commit: bool) {
        let v = controlcenter::slider_value(px, x, w);
        match t {
            Target::Brightness => {
                let pct = (v * 100.0).round() as u32;
                self.update_snapshot(|s| s.brightness = Some(pct));
                if commit || self.cc_last_send.elapsed() > Duration::from_millis(120) {
                    sys::set_brightness(pct);
                    self.cc_last_send = Instant::now();
                }
            }
            Target::Volume => {
                self.update_snapshot(|s| s.volume = Some(sys::Volume { level: v, muted: false }));
                if commit || self.cc_last_send.elapsed() > Duration::from_millis(60) {
                    sys::set_volume(v);
                    self.cc_last_send = Instant::now();
                }
            }
            _ => {}
        }
        self.relayout_control();
    }

    fn spotlight(&self) {
        if cfg!(test) {
            return vbar::launch_ui(vec!["decspot".into()]);
        }
        let decspot = crate::launch::sibling_or_path("decspot");
        let cmds: [&[&str]; 4] = [&[&decspot], &["rofi", "-show", "drun"], &["wofi", "--show", "drun"], &["fuzzel"]];
        for c in cmds {
            if sys::have(c[0]) {
                sys::spawn(c);
                return;
            }
        }
    }

    /// Opens `decdash` centred under the bar item (it toggles itself closed when already open).
    fn dashboard(&self, item: Span) {
        vbar::launch_ui(dash_argv(&crate::launch::sibling_or_path("decdash"), item));
    }

    // ---------------------------------------------------------------- draw

    fn text(&mut self, pm: &mut Pixmap, s: &str, x: f32, px: f32, weight: Weight, rgba: (u8, u8, u8, f32)) -> f32 {
        let Some(f) = self.fonts.as_mut() else { return 0.0 };
        let top = BASELINE - f.ascent(px, weight);
        f.draw(pm, s, x, top, px, weight, color(rgba.0, rgba.1, rgba.2, rgba.3))
    }

    fn symbol(&mut self, pm: &mut Pixmap, name: &str, size: f32, cx: f32, cy: f32) {
        let px = (size * 2.0).ceil() as u32;
        if let Some(sym) = self.symbols.get(name, ink(), px) {
            crate::gfx::blit_scaled(pm, sym, (cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size, 1.0);
        }
    }

    fn draw_bar(&mut self, pm: &mut Pixmap) {
        if self.dux() {
            self.dux_draw(pm);
            return;
        }
        let h = bar_h();
        if (h - BAR_HEIGHT).abs() < 0.01 {
            // the classic strip: drawn exactly as it always was
            self.draw_strip(pm, true);
            return;
        }
        // A declared height. The background and the hover / open plates take the whole strip; the content, laid out for the design's 26 px, goes in
        // its own layer and is centred, so text, icons and glyphs keep their exact metrics and nothing clips.
        let k = crate::gfx::render_scale();
        self.draw_strip_background(pm, h);
        for (x, w, alpha) in self.plates() {
            fill_rounded(pm, x, 3.0, w, h - 6.0, item_radius(), crate::theme::th().ink(alpha));
        }
        if let Some(mut layer) = Pixmap::new(pm.width(), (BAR_HEIGHT * k).ceil().max(1.0) as u32) {
            self.draw_strip(&mut layer, false);
            let dy = ((h - BAR_HEIGHT) / 2.0 * k).round() as i32;
            pm.draw_pixmap(0, dy, layer.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
    }

    /// The strip's own fill (and the floating bar's pill), `h` logical px tall.
    fn draw_strip_background(&self, pm: &mut Pixmap, h: f32) {
        let w = self.width as f32;
        if floating() {
            let th = crate::theme::th();
            let (x, y, pw, ph) = (PILL_MARGIN_X, PILL_MARGIN_Y, w - 2.0 * PILL_MARGIN_X, h - 2.0 * PILL_MARGIN_Y);
            fill_rounded(pm, x, y, pw, ph, ph / 2.0, th.bar_c());
            stroke_rounded(pm, x + 0.5, y + 0.5, pw - 1.0, ph - 1.0, ph / 2.0 - 0.5, th.outline_c(), 1.0);
        } else {
            crate::gfx::fill_rect(pm, 0.0, 0.0, w, h, crate::theme::th().bar_c());
        }
    }

    /// The hover and open highlights of the strip: (x, width, ink alpha). The same states the content pass would highlight inline.
    fn plates(&self) -> Vec<(f32, f32, f32)> {
        let mut plates = Vec::new();
        let (spans, limit) = (self.spans(), self.right_start());
        for (i, s) in spans.iter().enumerate().take(self.menus.len()) {
            if s.x + s.w > limit {
                continue;
            }
            let alpha = if self.open == Open::Menu(i) {
                0.18
            } else if self.hover_title == Some(i) {
                0.08
            } else {
                0.0
            };
            if alpha > 0.0 {
                plates.push((s.x, s.w, alpha));
            }
        }
        for (kind, s) in self.tray() {
            if self.open == Open::Popup(kind) || (kind == Tray::Control && self.open == Open::Control) {
                plates.push((s.x, s.w, 0.18));
            }
        }
        plates
    }

    /// The strip's content laid out for [`BAR_HEIGHT`]. `inline` also draws the background and the plates (the classic strip); without it the
    /// caller drew them at the full declared height.
    fn draw_strip(&mut self, pm: &mut Pixmap, inline: bool) {
        if inline {
            self.draw_strip_background(pm, BAR_HEIGHT);
        }

        // Left: titles.
        let spans = self.spans();
        let title_limit = self.right_start();
        let menus = self.menus.clone();
        for (i, (m, s)) in menus.iter().zip(&spans).enumerate() {
            if s.x + s.w > title_limit {
                continue;
            }
            let active = self.open == Open::Menu(i);
            let hovered = self.hover_title == Some(i);
            let alpha = if active {
                0.18
            } else if hovered {
                0.08
            } else {
                0.0
            };
            if inline && alpha > 0.0 {
                fill_rounded(pm, s.x, 3.0, s.w, BAR_HEIGHT - 6.0, item_radius(), crate::theme::th().ink(alpha));
            }
            if m.icon {
                if !self.draw_brand(pm, 14.0, s.x + s.w / 2.0, 12.5, ink()) {
                    self.symbol(pm, "apple", 12.0, s.x + s.w / 2.0, 11.5);
                }
            } else {
                let weight = if m.bold { Weight::Bold } else { Weight::Medium };
                self.text(pm, &m.title, s.x + 9.0, TEXT_PX, weight, (ink().0, ink().1, ink().2, 1.0));
            }
        }

        // Right: tray.
        let snap = self.snapshot();
        let weather = self.shared.weather.lock().ok().and_then(|w| w.clone());
        for (kind, s) in self.tray() {
            let cx = s.x + s.w / 2.0;
            if inline && self.open == Open::Popup(kind) {
                fill_rounded(pm, s.x, 3.0, s.w, BAR_HEIGHT - 6.0, item_radius(), crate::theme::th().ink(0.18));
            }
            match kind {
                Tray::Indicator(key) => self.indicator_draw(pm, key, cx, 13., 17.),
                Tray::Indicators => self.indicator_overflow_draw(pm, cx, 13., 17.),
                Tray::Layout => self.draw_layout_item(pm, s),
                Tray::Battery => {
                    if let Some(b) = &snap.battery {
                        self.draw_battery(pm, s, b);
                    }
                }
                Tray::Weather => {
                    if let Some(wx) = &weather {
                        let glyph = format!("wx:{:x}", weather::nerd_codepoint(wx.code, wx.night));
                        self.symbol(pm, &glyph, 17.0, s.x + 9.0 + ICON_PX / 2.0, 12.5);
                        self.text(pm, &format!("{}°", wx.temp_c), s.x + 9.0 + ICON_PX + 6.0, TEXT_PX, Weight::Regular, (ink().0, ink().1, ink().2, 1.0));
                    }
                }
                Tray::Workspaces => {
                    for (i, ws) in self.workspaces.iter().enumerate() {
                        let cx = s.x + WS_PAD / 2.0 + WS_SLOT * (i as f32 + 0.5);
                        if ws.active {
                            fill_rounded(pm, cx - 5.0, 10.0, 10.0, 6.0, 3.0, crate::theme::th().bar_text.color(0.95));
                        } else if ws.occupied {
                            crate::gfx::fill_circle(pm, cx, 13.0, 2.5, crate::theme::th().bar_text.color(0.6));
                        } else {
                            crate::gfx::fill_circle(pm, cx, 13.0, 2.0, crate::theme::th().bar_text.color(0.22));
                        }
                    }
                }
                Tray::Awake => self.symbol(pm, "awake", 12.0, cx, 12.5),
                Tray::Agents => self.symbol(pm, "agents", 14.0, cx, 12.5),
                Tray::Bluetooth => {
                    let icon = match &snap.bluetooth {
                        Some(b) if !b.powered => "bluetooth-off",
                        Some(b) if b.connected > 0 => "bluetooth-connected",
                        _ => "bluetooth",
                    };
                    self.symbol(pm, icon, ICON_PX, cx, 12.5)
                }
                Tray::Audio => {
                    let icon = snap.volume.as_ref().map(controlcenter::speaker_icon).unwrap_or("speaker-2");
                    self.symbol(pm, icon, ICON_PX, cx, 12.5);
                }
                Tray::Network => {
                    let net = snap.net.clone().unwrap_or_default();
                    let icon = if !net.wired_name.is_empty() {
                        "ethernet"
                    } else if net.wifi_enabled && !net.wifi_name.is_empty() {
                        "wifi-3"
                    } else {
                        "wifi-off"
                    };
                    self.symbol(pm, icon, ICON_PX, cx, 12.5);
                }
                Tray::Window => {
                    let room = s.w - 16.0;
                    let shown = match &self.fonts {
                        Some(f) => controlcenter::fit(&self.v_window_text(), room, TEXT_PX, Weight::Medium, f),
                        None => String::new(),
                    };
                    self.text(pm, &shown, s.x + 8.0, TEXT_PX, Weight::Medium, (ink().0, ink().1, ink().2, 1.0));
                }
                Tray::Media => {
                    let player = self.dux_player();
                    let playing = player.as_ref().is_some_and(|p| p.status == crate::dash::mpris::Status::Playing);
                    self.symbol(pm, if playing { "pause" } else { "play" }, ICON_PX - 2.0, s.x + 8.0 + ICON_PX / 2.0, 12.5);
                    let title = player.map(|p| p.meta.title).unwrap_or_default();
                    let shown = match &self.fonts {
                        Some(f) => controlcenter::fit(&title, s.w - ICON_PX - 20.0, TEXT_PX, Weight::Regular, f),
                        None => String::new(),
                    };
                    self.text(pm, &shown, s.x + 8.0 + ICON_PX + 6.0, TEXT_PX, Weight::Regular, (ink().0, ink().1, ink().2, 1.0));
                }
                Tray::Cpu | Tray::Memory => {
                    let perf = self.dux_perf.lock().ok().and_then(|p| *p);
                    let value = perf.and_then(|p| if kind == Tray::Cpu { p.0 } else { p.1 });
                    let label = if kind == Tray::Cpu { "CPU" } else { "MEM" };
                    let lw = self.fonts.as_ref().map_or(20.0, |f| f.measure(label, 11.0, Weight::Medium));
                    let faint = crate::theme::th().bar_text.color(0.6);
                    self.text(pm, label, s.x + 7.0, 11.0, Weight::Medium, (ink().0, ink().1, ink().2, faint.alpha()));
                    let text = value.map_or_else(|| "—%".to_string(), |v| format!("{:.0}%", v * 100.0));
                    self.text(pm, &text, s.x + 7.0 + lw + 4.0, 11.0, Weight::Regular, (ink().0, ink().1, ink().2, 1.0));
                }
                Tray::Lock => {
                    let on = self.locks.caps() || self.locks.num();
                    let label = match (self.locks.caps(), self.locks.num()) {
                        (true, true) => "A1",
                        (true, false) => "A",
                        (false, true) => "1",
                        _ => "A",
                    };
                    let th = crate::theme::th();
                    if on {
                        fill_rounded(pm, cx - 9.0, 6.0, 18.0, 14.0, 4.0, th.bar_text.color(0.9));
                    } else {
                        stroke_rounded(pm, cx - 8.5, 6.5, 17.0, 13.0, 4.0, th.bar_text.color(0.45), 1.0);
                    }
                    let fg = if on { th.bar.t() } else { ink() };
                    if let Some(f) = self.fonts.as_mut() {
                        let lw = f.measure(label, 10.0, Weight::Bold);
                        let top = 13.0 + 3.6 - f.ascent(10.0, Weight::Bold);
                        f.draw(pm, label, cx - lw / 2.0, top, 10.0, Weight::Bold, color(fg.0, fg.1, fg.2, if on { 1.0 } else { 0.6 }));
                    }
                }
                Tray::Task(key) => {
                    let Some(t) = self.task(key).cloned() else { continue };
                    let icon = self.v_icons.get(&t.app_id, 32).clone();
                    crate::gfx::blit_scaled(pm, &icon, cx - 8.0, 5.0, 16.0, 16.0, 1.0);
                    if t.active() {
                        fill_rounded(pm, cx - 4.0, 22.0, 8.0, 2.0, 1.0, crate::theme::th().bar_text.color(0.9));
                    }
                }
                Tray::Logo => {
                    let label = applets::branding().launcher_label;
                    let has_logo = !matches!(applets::branding().logo, crate::panel::branding::Logo::None);
                    let icon_x = if label.is_some() { s.x + 8.0 + ICON_PX / 2.0 } else { cx };
                    if has_logo && !self.draw_brand(pm, ICON_PX, icon_x, 12.5, ink()) {
                        self.symbol(pm, applets::classic_symbol(kind), ICON_PX - 1.0, icon_x, 12.5);
                    }
                    if let Some(l) = label {
                        let x = if has_logo { s.x + 8.0 + ICON_PX + 5.0 } else { s.x + 9.0 };
                        self.text(pm, &l, x, TEXT_PX, Weight::Medium, (ink().0, ink().1, ink().2, 1.0));
                    }
                }
                Tray::AppMenu | Tray::Power | Tray::Notifications | Tray::Clipboard | Tray::Brightness | Tray::ShowDesktop | Tray::More => {
                    let name = if kind == Tray::More { "chevron-left" } else { applets::classic_symbol(kind) };
                    self.symbol(pm, name, ICON_PX, cx, 12.5)
                }
                Tray::Dash => self.symbol(pm, "chart", ICON_PX, cx, 12.5),
                Tray::Spotlight => self.symbol(pm, "magnifyingglass", ICON_PX, cx, 12.5),
                Tray::Control => {
                    if inline && self.open == Open::Control {
                        fill_rounded(pm, s.x, 3.0, s.w, BAR_HEIGHT - 6.0, item_radius(), crate::theme::th().ink(0.18));
                    }
                    self.symbol(pm, "switches", ICON_PX, cx, 12.5);
                }
                Tray::Clock => {
                    let t = self.clock_text.clone();
                    self.text(pm, &t, s.x + 8.0, TEXT_PX, Weight::Regular, (ink().0, ink().1, ink().2, 1.0));
                }
            }
        }
    }

    fn draw_dropdown(&mut self, pm: &mut Pixmap, idx: usize) {
        let Some(d) = self.dropdown(idx) else { return };
        let menu = self.menus[idx].clone();
        fill_rounded(pm, d.x, d.y, d.w, d.h, crate::theme::th().radius, crate::theme::th().popup_c());
        stroke_rounded(pm, d.x + 0.5, d.y + 0.5, d.w - 1.0, d.h - 1.0, crate::theme::th().radius - 0.5, crate::theme::th().outline_c(), 1.0);
        for row in &d.rows {
            let it = &menu.items[row.index];
            if it.separator {
                crate::gfx::fill_rect(pm, d.x + PAD_SEP, row.y + row.h / 2.0, d.w - 2.0 * PAD_SEP, 1.0, crate::theme::th().ink(0.12));
                continue;
            }
            let available = self.runner.available(it.action);
            let hovered = self.hover_row == Some(row.index) && available;
            if hovered {
                fill_rounded(pm, d.x + menus::PAD, row.y, d.w - 2.0 * menus::PAD, row.h, crate::theme::th().r(0.5), crate::theme::th().accent_c());
            }
            let alpha = if available { 1.0 } else { 0.35 };
            let (label, red) = if self.confirm == Some(it.action) { (format!("{}?", it.label.trim_end_matches('…')), true) } else { (it.label.clone(), false) };
            let th = crate::theme::th();
            let main = if hovered {
                th.accent_text.a(1.0)
            } else if red {
                th.danger.a(1.0)
            } else {
                th.text.a(alpha)
            };
            let sub = if hovered { th.accent_text.a(1.0) } else { th.text.a(0.45 * alpha) };
            // Vertically centre in the 24px row.
            let Some(f) = self.fonts.as_mut() else { continue };
            let top = row.y + (row.h - TEXT_PX) / 2.0 - 1.0;
            f.draw(pm, &label, d.x + 12.0, top, TEXT_PX, Weight::Regular, color(main.0, main.1, main.2, main.3));
            let sw = f.measure(it.shortcut, TEXT_PX, Weight::Regular);
            f.draw(pm, it.shortcut, d.x + d.w - 12.0 - sw, top, TEXT_PX, Weight::Regular, color(sub.0, sub.1, sub.2, sub.3));
        }
    }
}

const PAD_SEP: f32 = 8.0;

/// Which system probes the minute refresh runs: one per kind of state an enabled applet shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Probes {
    pub net: bool,
    pub volume: bool,
    pub battery: bool,
    pub bluetooth: bool,
    pub brightness: bool,
    pub awake: bool,
    pub light: bool,
}

impl Probes {
    pub fn any(self) -> bool {
        self.net || self.volume || self.battery || self.bluetooth || self.brightness || self.awake || self.light
    }
}

/// A cheap refresh of what `want` names (Bluetooth only if it was seen before or its tool exists); the rest is kept.
fn collect_light(prev: &Snapshot, want: Probes) -> Snapshot {
    let mut s = prev.clone();
    if want.net {
        s.net = sys::probe_net();
    }
    if want.volume {
        if let Some((v, name)) = sys::probe_volume() {
            s.volume = Some(v);
            s.sink = name;
        }
    }
    if want.awake {
        s.awake = awake::refresh();
    }
    if want.light {
        s.light = sys::is_light();
    }
    if want.battery {
        s.battery = sys::probe_battery();
    }
    if want.bluetooth && (prev.bluetooth.is_some() || sys::have("bluetoothctl")) {
        s.bluetooth = sys::probe_bluetooth();
    }
    if want.brightness {
        s.brightness = sys::probe_brightness();
    }
    s
}

impl App for DecBar {
    fn size(&self) -> (u32, u32) {
        if let Some(p) = self.place() {
            // a side bar is as tall as the monitor (0), a top / bottom bar as wide as it; the rest is room for popouts
            return self.v_surface_size(p);
        }
        (0, surface_height(self.open_bottom()))
    }

    fn surface_spec(&self) -> Option<crate::backend::SurfaceSpec> {
        if self.embedded {
            return None;
        }
        Some(if let Some(p) = self.place() {
            DecBar::v_spec(p)
        } else {
            crate::backend::SurfaceSpec {
                anchor: if self.bottom() { crate::backend::Anchor::Bottom } else { crate::backend::Anchor::Top },
                margin: 0,
                reserve: self.bar_bottom().ceil() as i32,
                stretch: true,
                namespace: "decebal-bar",
            }
        })
    }

    fn resized(&mut self, w: u32, h: u32) {
        self.width = w;
        self.height = h;
        if self.bottom() {
            // a bottom bar's surface grew or shrank upwards: what hangs over the strip moves with it
            let top = self.strip_top();
            if let Some(p) = self.popup.as_mut() {
                p.y = (top - MENU_GAP - p.h).max(0.0);
            }
            if let Some(c) = self.calendar.as_mut() {
                c.y = (top - MENU_GAP - Calendar::H).max(0.0);
            }
        }
        self.cc_sync_viewport();
    }

    fn surface_origin(&mut self, x: i32, y: i32) {
        self.indicator_origin = (x, y);
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        crate::gfx::clear(pm);
        if self.pill_bar() {
            self.v_draw(pm);
            self.draw_focus_ring(pm);
            if let Some(f) = &self.fonts {
                f.trim();
            }
            self.heap_dirty = Some(Instant::now());
            self.heap_rechecked = false;
            return;
        }
        (self.width, self.height) = crate::gfx::logical_size(pm);
        self.cc_sync_viewport();
        let top = self.strip_top();
        if top > 0.0 {
            // a bottom bar: the strip is drawn as it is on top, then put on the last rows of the surface
            let k = crate::gfx::render_scale();
            if let Some(mut strip) = Pixmap::new(pm.width(), (self.bar_bottom() * k).ceil().max(1.0) as u32) {
                self.draw_bar(&mut strip);
                pm.draw_pixmap(0, (top * k).round() as i32, strip.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
            }
        } else {
            self.draw_bar(pm);
        }
        let indicator_rects = if self.dux() {
            self.dux_layout().items.into_iter().filter_map(|i| if let segmented::Kind::Tray(k) = i.kind { Some((k, (i.x, i.y + top, i.w, i.h))) } else { None }).collect::<Vec<_>>()
        } else {
            self.tray().into_iter().map(|(k, s)| (k, (s.x, top, s.w, self.bar_bottom()))).collect::<Vec<_>>()
        };
        self.indicator_debug(&indicator_rects);
        if self.debug_tray.is_some() {
            let text: String = self
                .tray()
                .iter()
                .map(
                    |(k, s)| {
                        if top > 0.0 {
                            format!("{:?} {} {}\n", k, (s.x + s.w / 2.0).round(), (top + self.bar_bottom() / 2.0).round())
                        } else {
                            format!("{:?} {}\n", k, (s.x + s.w / 2.0).round())
                        }
                    },
                )
                .collect();
            if let Some((path, last)) = self.debug_tray.as_mut() {
                if *last != text {
                    let _ = std::fs::write(&*path, &text);
                    *last = text;
                }
            }
        }
        match self.open {
            Open::Menu(i) => self.draw_dropdown(pm, i),
            Open::Control => {
                let (ox, oy) = self.panel_origin();
                self.debug_control_panel(pm.width(), pm.height());
                let style = self.cc_style();
                let clipped = self.cc_clipped();
                let width = self.cc_width();
                let height = self.cc_visible_height();
                if let Some(f) = self.fonts.as_mut() {
                    if clipped {
                        self.cc_viewport.draw(pm, (ox, oy), width, height, &self.cc_items, self.cc_hover, style, f, &mut self.symbols);
                    } else {
                        let view = controlcenter::View { origin: (ox, oy), items: &self.cc_items, height: self.cc_height, hover: self.cc_hover, style };
                        controlcenter::draw_focused(pm, &view, self.cc_viewport.focus, f, &mut self.symbols);
                    }
                }
            }
            Open::Popup(_) => {
                if let Some(f) = self.fonts.as_mut() {
                    if let Some(p) = &self.popup {
                        p.draw(pm, f, &mut self.symbols, self.popup_hover);
                    } else if let Some(c) = &self.calendar {
                        c.draw(pm, f);
                    }
                }
            }
            Open::None => {}
        }
        self.draw_focus_ring(pm);
        if let Some(f) = &self.fonts {
            f.trim();
        }
        self.heap_dirty = Some(Instant::now());
        self.heap_rechecked = false;
    }

    fn glass_regions(&self) -> Vec<crate::glass::Region> {
        let mut regions = Vec::new();
        let th = crate::theme::th();
        if !self.embedded {
            if self.pill_bar() {
                let radius = self.v_geo().pill_width() / 2.0;
                regions.extend(self.v_layout().pills.into_iter().map(|(_, r)| crate::glass::Region { x: r.x, y: r.y, w: r.w, h: r.h, radius: radius.min(r.w.min(r.h) / 2.0) }));
            } else if self.dux() {
                let top = self.strip_top();
                regions.extend(self.dux_regions().into_iter().map(|r| crate::glass::Region { y: r.y + top, ..r }));
            } else if floating() {
                let height = self.bar_bottom() - 2.0 * PILL_MARGIN_Y;
                regions.push(crate::glass::Region { x: PILL_MARGIN_X, y: self.strip_top() + PILL_MARGIN_Y, w: self.width as f32 - 2.0 * PILL_MARGIN_X, h: height, radius: height / 2.0 });
            } else {
                regions.push(crate::glass::Region { x: 0.0, y: self.strip_top(), w: self.width as f32, h: self.bar_bottom(), radius: 0.0 });
            }
        }
        match self.open {
            Open::Menu(index) => {
                if let Some(d) = self.dropdown(index) {
                    regions.push(crate::glass::Region { x: d.x, y: d.y, w: d.w, h: d.h, radius: th.radius });
                }
            }
            Open::Control => {
                let (x, y) = self.panel_origin();
                regions.push(crate::glass::Region { x, y, w: self.cc_width(), h: self.cc_visible_height(), radius: controlcenter::look(self.cc_style()).0 });
            }
            Open::Popup(_) => {
                if let Some(p) = &self.popup {
                    regions.push(crate::glass::Region { x: p.x, y: p.y, w: p.w, h: p.h, radius: th.radius });
                } else if let Some(c) = &self.calendar {
                    regions.push(crate::glass::Region { x: c.x, y: c.y, w: Calendar::W, h: Calendar::H, radius: th.r(1.2) });
                }
            }
            Open::None => {}
        }
        regions
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> bool {
        if self.pill_bar() {
            return self.v_motion(x, y);
        }
        let mut redraw = false;
        self.leave_at = None;
        if self.dux() && self.cc_drag.is_none() && self.popup_drag.is_none() && self.in_strip(y) {
            let next = self.dux_layout().hit(x, y - self.strip_top()).map(|i| i.kind);
            let changed = next != self.dux_hover;
            if changed {
                self.dux_effect = next.map(|kind| (kind, Instant::now(), true)).or_else(|| self.dux_hover.map(|kind| (kind, Instant::now(), false)));
            }
            self.dux_hover = next;
            let cc_changed = self.cc_hover.take().is_some();
            let focus_changed = self.cc_viewport.focus.take().is_some();
            return changed || cc_changed || focus_changed;
        }
        if let Some(kind) = self.dux_hover.take() {
            self.dux_effect = Some((kind, Instant::now(), false));
            redraw = true;
        }
        // Dragging a slider keeps tracking even outside its rectangle.
        if let (Some(t), Open::Control) = (self.cc_drag, self.open) {
            let (ox, _) = self.panel_origin();
            if let Some(it) = self.cc_items.iter().find(|i| i.target == Some(t)) {
                let (rx, rw) = (it.r.x, it.r.w);
                self.slide(t, rx, rw, (x - ox) / self.cc_zoom(), false);
                return true;
            }
        }
        if let Some(row) = self.popup_drag {
            self.popup_slide(row, x, false);
            return true;
        }
        // Off the bar and what is open (the pointer is grabbed on X11, so motion keeps coming): the close delay runs,
        // and coming back cancels it.
        if self.open != Open::None {
            if self.over_open_ui(x, y) {
                self.leave_at = None;
            } else if self.leave_at.is_none() {
                self.leave_at = Some(Instant::now());
            }
        }
        let title = if self.in_strip(y) { self.title_hit(x) } else { None };
        if title != self.hover_title {
            self.hover_title = title;
            redraw = true;
        }
        // macOS: with a menu open, sliding along the bar opens the neighbouring menus.
        if let (Some(i), Open::Menu(cur)) = (title, self.open) {
            if i != cur {
                self.open_menu(i);
                redraw = true;
            }
        }
        match self.open {
            Open::Menu(i) => {
                let row = self.dropdown(i).and_then(|d| d.hit(&self.menus[i], x, y));
                if row != self.hover_row {
                    self.hover_row = row;
                    redraw = true;
                }
            }
            Open::Control => {
                let hover = self.cc_point(x, y).and_then(|(px, py)| controlcenter::hit(&self.cc_items, px, py)).map(|h| h.0);
                self.cc_viewport.focus = None;
                if hover != self.cc_hover {
                    self.cc_hover = hover;
                    redraw = true;
                }
            }
            Open::Popup(_) => {
                let hover = self.popup.as_ref().and_then(|p| p.hit(x, y));
                if hover != self.popup_hover {
                    self.popup_hover = hover;
                    redraw = true;
                }
            }
            Open::None => {}
        }
        redraw
    }

    fn pointer_leave(&mut self) -> bool {
        let old = self.dux_hover.take();
        if let Some(kind) = old {
            self.dux_effect = Some((kind, Instant::now(), false));
        }
        let redraw = self.hover_title.take().is_some() || old.is_some();
        let redraw = self.cc_hover.take().is_some() || redraw;
        let redraw = self.v_hover.take().is_some() || redraw;
        self.v_hover_ws = None;
        self.v_hover_open = None;
        if self.cc_drag.is_some() || self.popup_drag.is_some() {
            return redraw;
        }
        // Not at once: see CLOSE_DELAY. `tick` does the closing.
        if self.open != Open::None && self.leave_at.is_none() {
            self.leave_at = Some(Instant::now());
        }
        redraw
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        if self.pill_bar() {
            return self.v_button(x, y, button);
        }
        self.leave_at = None;
        let (top, on_strip) = (self.strip_top(), self.in_strip(y));
        if self.dux() && on_strip {
            if self.dux_layout().hit(x, y - top).is_none() {
                return self.close();
            }
            if self.dux_custom_press(x, y - top, button) {
                return true;
            }
        }
        // A click anywhere else (the pointer is grabbed on X11, so such clicks reach us) closes.
        if self.open != Open::None && !self.over_open_ui(x, y) {
            self.close();
            return true;
        }
        let item = if on_strip { self.tray().into_iter().find(|(_, s)| x >= s.x && x < s.x + s.w) } else { None };
        if let Some((kind, _)) = item {
            match kind {
                Tray::Indicator(key) => {
                    match self.indicator_press(key, button, x as i32, y as i32) {
                        indicators::IndicatorPress::Menu => self.open_popup(kind),
                        indicators::IndicatorPress::Handled => {
                            self.close();
                        }
                        indicators::IndicatorPress::Ignored => return false,
                    }
                    return true;
                }
                Tray::Indicators if button == Button::Left || button == Button::Right => {
                    self.indicator_page = 0;
                    if self.open == Open::Popup(kind) {
                        self.close();
                    } else {
                        self.open_popup(kind);
                    }
                    return true;
                }
                _ => {}
            }
        }
        if let (Some((kind, s)), Button::Middle | Button::Right) = (item, button) {
            return self.applet_press(kind, (s.x, top, s.w, self.bar_bottom()), button);
        }
        if button != Button::Left {
            return false;
        }
        if on_strip {
            if let Some(i) = self.title_hit(x) {
                if self.open == Open::Menu(i) {
                    self.close();
                } else {
                    self.close();
                    self.open_menu(i);
                }
                return true;
            }
            let Some((kind, s)) = item else { return self.close() };
            return match kind {
                Tray::Workspaces => {
                    let scale = if self.dux() { crate::theme::live::font_scale().max(1.0) } else { 1.0 };
                    if let Some(i) = workspace_hit(Span { x: s.x, w: s.w / scale }, self.workspaces.len(), s.x + (x - s.x) / scale) {
                        self.close();
                        self.active.switch_workspace(i);
                    }
                    false
                }
                // the Dux focused-window item opens the dashboard, as upstream
                Tray::Window if self.dux() => {
                    self.close();
                    self.dashboard(s);
                    true
                }
                _ => self.applet_press(kind, (s.x, top, s.w, self.bar_bottom()), Button::Left),
            };
        }
        match self.open {
            Open::Menu(i) => {
                let hit = self.dropdown(i).and_then(|d| d.hit(&self.menus[i], x, y));
                match hit {
                    Some(item) => self.run_menu_item(i, item),
                    None => false,
                }
            }
            Open::Control => {
                if let Some((px, py)) = self.cc_point(x, y) {
                    if let Some((t, r)) = controlcenter::hit(&self.cc_items, px, py) {
                        self.cc_viewport.focus = None;
                        self.activate(t, r.x, r.w, px);
                        return true;
                    }
                }
                false
            }
            Open::Popup(_) => self.popup_press(x, y),
            Open::None => false,
        }
    }

    fn pointer_release(&mut self, x: f32, y: f32, button: Button) -> bool {
        if button != Button::Left {
            return false;
        }
        // A drag that ended off the surface: the usual close delay starts now.
        if (self.popup_drag.is_some() || self.cc_drag.is_some()) && !self.over_open_ui(x, y) {
            self.leave_at = Some(Instant::now());
        }
        if let Some(row) = self.popup_drag.take() {
            self.popup_slide(row, x, true);
            return true;
        }
        if let Some(t) = self.cc_drag.take() {
            let (ox, _) = self.panel_origin();
            if let Some(it) = self.cc_items.iter().find(|i| i.target == Some(t)) {
                let (rx, rw) = (it.r.x, it.r.w);
                self.slide(t, rx, rw, (x - ox) / self.cc_zoom(), true);
            }
            return true;
        }
        false
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.indicator_menu_scroll(x, y, dy) {
            return true;
        }
        if self.pill_bar() && self.v_scroll(x, y, dy) {
            return true;
        }
        if !self.pill_bar() && self.in_strip(y) && self.tray_scroll(x, dy) {
            return true;
        }
        self.cc_scroll(x, y, dy)
    }
    fn key(&mut self, key: Key) -> bool {
        match key {
            Key::Escape => self.close() || self.kb_focus.take().is_some(),
        }
    }

    fn nav_key(&mut self, key: crate::app::NavKey) -> bool {
        // the panel's own keyboard mode (applets, popups, menus); the Control Center keeps its routing below
        if let Some(handled) = self.panel_nav(key) {
            return handled;
        }
        if !self.cc_style().scrolls() || self.open != Open::Control {
            return false;
        }
        let targets = ccviewport::targets(&self.cc_items);
        if targets.is_empty() {
            return false;
        }
        let current = targets.iter().position(|(target, _)| Some(*target) == self.cc_viewport.focus);
        match key {
            crate::app::NavKey::Up | crate::app::NavKey::Down => {
                let step = if key == crate::app::NavKey::Up { -1 } else { 1 };
                let index = current.map_or(0, |index| (index as i32 + step).rem_euclid(targets.len() as i32) as usize);
                let (target, rect) = targets[index];
                self.cc_viewport.focus = Some(target);
                self.cc_hover = Some(target);
                if self.cc_clipped() {
                    self.cc_viewport.reveal(rect, self.cc_height, self.cc_view_height());
                }
                true
            }
            crate::app::NavKey::Enter => {
                let Some(index) = current else { return false };
                let (target, rect) = targets[index];
                // Enter selects a command, not an arbitrary midpoint slider value.
                if matches!(target, Target::Volume | Target::Brightness) {
                    return false;
                }
                self.activate(target, rect.x, rect.w, rect.x + rect.w / 2.0);
                true
            }
            crate::app::NavKey::Left | crate::app::NavKey::Right => {
                // a focused slider steps by 5 %, as the wheel does; nothing else takes Left/Right
                let Some(index) = current else { return false };
                let (target, rect) = targets[index];
                if !matches!(target, Target::Volume | Target::Brightness) {
                    return false;
                }
                let snap = self.snapshot();
                let cur = match target {
                    Target::Brightness => snap.brightness.unwrap_or(0) as f32 / 100.0,
                    _ => snap.volume.map(|v| v.level).unwrap_or(0.0),
                };
                let step = if key == crate::app::NavKey::Right { 0.05 } else { -0.05 };
                self.slide(target, rect.x, rect.w, rect.x + (cur + step).clamp(0.0, 1.0) * rect.w, true);
                true
            }
        }
    }

    fn wants_keyboard(&self) -> bool {
        self.open != Open::None || self.kb_focus.is_some()
    }

    /// While anything is open the pointer is grabbed (X11): the strip's shape has gaps (without a compositor
    /// transparent pixels are cut out of the window), so a pointer on its way from an icon to a popup would
    /// otherwise "leave" and close it, and clicks on other windows would never reach us.
    fn wants_pointer_grab(&self) -> bool {
        self.open != Open::None
    }

    fn input_rects(&self) -> Vec<InputRect> {
        if self.pill_bar() {
            return self.v_input_rects();
        }
        let w = self.width.max(1) as i32;
        let (top, bh) = (self.strip_top(), self.bar_bottom());
        let bottom = self.bottom();
        let mut rects = if self.dux() {
            self.dux_regions().into_iter().map(|g| InputRect { x: g.x.floor() as i32, y: (g.y + top).floor() as i32, w: g.w.ceil() as i32, h: g.h.ceil() as i32 }).collect()
        } else {
            vec![InputRect { x: 0, y: top as i32, w, h: bh as i32 }]
        };
        // A box opened from the strip, with the gap between them: the pointer never leaves the surface on its way there.
        let reach = |x: f32, y: f32, bw: f32, h: f32| {
            if bottom {
                InputRect { x: x.floor() as i32, y: y.floor() as i32, w: bw.ceil() as i32, h: (top - y.floor()).ceil() as i32 }
            } else {
                InputRect { x: x.floor() as i32, y: bh as i32, w: bw.ceil() as i32, h: (y + h - bh).ceil() as i32 }
            }
        };
        match self.open {
            Open::Menu(i) => {
                if let Some(d) = self.dropdown(i) {
                    rects.push(reach(d.x, d.y, d.w, d.h));
                }
            }
            Open::Control => {
                let (ox, oy) = self.panel_origin();
                rects.push(reach(ox, oy, self.cc_width(), self.cc_visible_height()));
            }
            Open::Popup(kind) => {
                // The gap between the item and the popup belongs to the popup (the popup may be clamped away from the item).
                let boxed = self.popup.as_ref().map(|p| (p.x, p.y, p.w, p.h)).or(self.calendar.as_ref().map(|c| (c.x, c.y, Calendar::W, Calendar::H)));
                if let (Some(span), Some((_, py, _, ph))) = (self.tray_span(kind).or_else(|| self.tray_span(Tray::More)), boxed) {
                    rects.push(reach(span.x, py, span.w, ph));
                }
                if let Some((px, py, pw, ph)) = boxed {
                    rects.push(reach(px, py, pw, ph));
                }
            }
            Open::None => {}
        }
        rects
    }

    fn idle(&mut self) -> Option<Duration> {
        // Measuring text for hit-testing and menu layout can parse a whole face (~12 MB); the next draw
        // would drop it, but a pointer event that needs no redraw leaves it (and a fragmented heap) behind.
        if let Some(f) = &self.fonts {
            if f.has_loaded_face() {
                f.trim();
                self.heap_dirty = Some(Instant::now());
            }
        }
        // Once things have been quiet for a moment, hand every freed page back: a trim right after a
        // burst of allocations (face parsing, worker threads finishing) can miss memory freed just after it.
        let since = self.heap_dirty?;
        let wait = if self.heap_rechecked { HEAP_RECHECK } else { HEAP_QUIET };
        match wait.checked_sub(since.elapsed()) {
            Some(left) => Some(left),
            None if !self.heap_rechecked => {
                crate::mem::trim();
                // Look again shortly: worker threads may free memory a moment later.
                self.heap_rechecked = true;
                self.heap_dirty = Some(Instant::now());
                Some(HEAP_RECHECK)
            }
            None => {
                crate::mem::trim();
                self.heap_dirty = None;
                self.heap_rechecked = false;
                None
            }
        }
    }

    fn set_waker(&mut self, waker: Waker) {
        if let Ok(mut slot) = self.focus_waker.lock() {
            *slot = Some(waker.clone());
        }
        self.active.set_waker(waker.clone());
        self.start_watchers(waker);
        // First data: weather (cached or fetched) and a light system snapshot.
        if self.weather_stale() {
            self.spawn_weather();
        }
        self.spawn_snapshot(false);
    }

    fn tick_in(&self) -> Option<Duration> {
        let leave = self.leave_at.map(|t| CLOSE_DELAY.saturating_sub(t.elapsed()));
        let hover = self.v_hover_open.map(|(_, t)| vbar::HOVER_DELAY.saturating_sub(t.elapsed()));
        let hover = if self.dux_animating() { Some(Duration::from_millis(16)) } else { hover };
        match (leave, hover) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn quit_requested(&self) -> bool {
        // the bottom half of a split layout has nothing to draw once the layout is no longer split
        (self.process == vbar::Process::Bottom && self.place().is_none()) || self.gate.as_ref().is_some_and(|g| g.closed())
    }

    fn tick(&mut self) -> Tick {
        let mut t = self.tick_inner();
        if let Some(g) = self.gate.as_mut() {
            g.poll();
            t.next = Some(t.next.map_or(gate::POLL, |n| n.min(gate::POLL)));
        }
        if self.ensure_bottom() {
            t.next = Some(t.next.map_or(vbar::COMPANION_POLL, |n| n.min(vbar::COMPANION_POLL)));
        }
        t
    }
}

impl DecBar {
    /// The wheel over the open Control Center (any host): a slider under the pointer steps, elsewhere a clipped panel
    /// scrolls.
    pub(super) fn cc_scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.open != Open::Control {
            return false;
        }
        let (ox, oy) = self.panel_origin();
        if !(x >= ox && x < ox + self.cc_width() && y >= oy && y < oy + self.cc_visible_height()) {
            return false;
        }
        let Some((px, py)) = self.cc_point(x, y) else {
            return false;
        };
        let cleared_focus = self.cc_viewport.focus.take().is_some();
        let cleared_hover = self.cc_hover.take().is_some();
        let cleared_highlight = cleared_focus || cleared_hover;
        let hit = controlcenter::hit(&self.cc_items, px, py);
        if self.cc_clipped() && !matches!(hit, Some((Target::Brightness | Target::Volume, _))) {
            if self.cc_drag.is_some() {
                return cleared_highlight;
            }
            let old = self.cc_viewport.scroll;
            self.cc_viewport.focus = None;
            self.cc_viewport.scroll += dy * ccviewport::SCROLL_STEP;
            self.cc_viewport.clamp(self.cc_height, self.cc_view_height());
            return old != self.cc_viewport.scroll || cleared_highlight;
        }
        let Some((t @ (Target::Brightness | Target::Volume), r)) = hit else { return cleared_highlight };
        let snap = self.snapshot();
        let cur = match t {
            Target::Brightness => snap.brightness.unwrap_or(0) as f32 / 100.0,
            _ => snap.volume.map(|v| v.level).unwrap_or(0.0),
        };
        let next = (cur - dy.signum() * 0.05).clamp(0.0, 1.0);
        self.slide(t, r.x, r.w, r.x + next * r.w, true);
        true
    }
}

impl DecBar {
    fn tick_inner(&mut self) -> Tick {
        let generation = crate::theme::generation();
        if self.frame_theme_generation.is_some_and(|previous| previous != generation) {
            self.frame_theme_generation = Some(generation);
            crate::theme::frame::publish(&crate::theme::th());
        }
        let mut redraw = self.dux_tick();
        redraw |= self.take_focus_request();
        redraw |= self.shared.changed.swap(false, Ordering::SeqCst);
        redraw |= self.dux_animating();
        redraw |= self.indicator_tick();
        let mut close_in = None;
        let place = self.place();
        let vertical = place.is_some();
        if place != self.was_place {
            // The theme or the bar position switched layouts: what was open belongs to the old one. The backend moves,
            // resizes and re-struts the surface.
            self.was_place = place;
            self.close();
            self.v_hover = None;
            self.v_hover_ws = None;
            if vertical && self.logo_symbol.is_empty() {
                self.logo_symbol = vbar::logo_symbol();
            }
            redraw = true;
        }
        if vertical {
            let (r, hover_in) = self.v_tick();
            redraw |= r;
            close_in = hover_in;
            redraw |= self.v_refresh_window();
        }
        if let Some(t) = self.leave_at {
            if self.cc_drag.is_some() || self.popup_drag.is_some() {
                self.leave_at = None;
            } else if t.elapsed() >= CLOSE_DELAY {
                redraw |= self.close();
            } else {
                let left = CLOSE_DELAY - t.elapsed();
                close_in = Some(close_in.map_or(left, |c| c.min(left)));
            }
        }
        redraw |= crate::theme::take_changed();
        if redraw && self.open == Open::Control && !crate::theme::th().control_center.enabled {
            self.close();
        }
        if redraw && self.open == Open::Control && self.cc_drag.is_none() {
            self.relayout_control();
        }
        if redraw && matches!(self.open, Open::Popup(_)) && self.popup_drag.is_none() {
            self.rebuild_popup();
        }
        self.refresh_open_popup();
        let Some(tm) = clock::now() else { return Tick { redraw, next: Some(close_in.map_or(Duration::from_secs(30), |c| c.min(Duration::from_secs(30)))) } };
        let minute = (tm.hour, tm.min);
        if self.last_minute != Some(minute) {
            self.last_minute = Some(minute);
            let cs = crate::config::shell_settings();
            self.clock_text = if self.dux() { clock::format_dux(&tm, cs.clock_hour12) } else { clock::format_with(&tm, cs.clock_hour12, &cs.clock_date) };
            redraw = true;
            // Once a minute: a cheap data refresh, the active app name, and stale weather.
            self.spawn_snapshot(false);
            if self.weather_stale() {
                self.spawn_weather();
            }
        }
        if self.refresh_app_name() {
            redraw = true;
        }
        if self.refresh_workspaces() {
            redraw = true;
        }
        if self.refresh_tasks() {
            redraw = true;
        }
        self.sync_workers();
        if self.open == Open::Control && self.cc_drag.is_none() && self.last_full_refresh.elapsed() >= PANEL_REFRESH {
            self.last_full_refresh = Instant::now();
            self.spawn_snapshot(true);
        }
        let wake = next_wake(&tm, matches!(self.open, Open::Control | Open::Popup(_)));
        let wake = if self.dux_animating() {
            wake.min(Duration::from_millis(16))
        } else if self.dux_samples_resources() {
            wake.min(Duration::from_secs(2))
        } else {
            wake
        };
        Tick { redraw, next: Some(close_in.map_or(wake, |c| c.min(wake))) }
    }
}

#[cfg(test)]
mod bar_height_tests;
#[cfg(test)]
mod cc_fit_tests;
#[cfg(test)]
mod gnome_cc_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn menus() -> Vec<Menu> {
        menus::build("Foot", false)
    }

    #[test]
    fn frame_theme_publication_requires_runtime_opt_in() {
        let bar = DecBar::new(Box::new(active::NoActiveWindow));
        assert!(bar.frame_theme_generation.is_none(), "PNG/native preview construction must not publish");
        let bar = bar.with_frame_publisher();
        assert!(bar.frame_theme_generation.is_some());
    }

    #[test]
    fn glass_metadata_excludes_popup_canvas_and_pointer_bridge() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.resized(1280, 420);
        let regions = bar.glass_regions();
        assert_eq!(regions.len(), 1);
        assert!(regions.iter().all(|r| r.y + r.h <= BAR_HEIGHT));
        bar.open_popup(Tray::Clock);
        let regions = bar.glass_regions();
        assert_eq!(regions.len(), 2);
        let calendar = bar.calendar.as_ref().unwrap();
        assert_eq!((regions[1].x, regions[1].y, regions[1].w, regions[1].h), (calendar.x, calendar.y, Calendar::W, Calendar::H));
        assert!(regions[1].y > regions[0].y + regions[0].h);
    }

    #[test]
    fn titles_are_adjacent_boxes_starting_at_the_left_pad() {
        let m = menus();
        let spans = title_spans(&m, |m| if m.icon { 12.0 } else { 30.0 });
        assert_eq!(spans[0], Span { x: 7.0, w: 34.0 });
        assert_eq!(spans[1].x, 41.0);
        for pair in spans.windows(2) {
            assert!((pair[0].x + pair[0].w - pair[1].x).abs() < 1e-4);
        }
    }

    #[test]
    fn tray_is_right_aligned_and_in_order() {
        let items = [(Tray::Audio, 27.0), (Tray::Control, 27.0), (Tray::Clock, 120.0)];
        let t = tray_spans(1000.0, &items);
        assert_eq!(t[2], (Tray::Clock, Span { x: 1000.0 - RIGHT_PAD - 120.0, w: 120.0 }));
        assert_eq!(t[1].1.x + t[1].1.w, t[2].1.x);
        assert_eq!(t[0].1.x + t[0].1.w, t[1].1.x);
    }

    #[test]
    fn tray_matches_the_measured_original_positions() {
        // From the 3440px screenshot: control button centre 3294.5, spotlight 3267.5. The clock text is
        // 105px wide there (varies a few px with the digits shown), so allow a few px of slack.
        let clock_w = 105.0 + 17.0;
        let items = [(Tray::Spotlight, BTN_W), (Tray::Control, BTN_W), (Tray::Clock, clock_w)];
        let t = tray_spans(3440.0, &items);
        let centre = |s: &Span| s.x + s.w / 2.0;
        assert!((centre(&t[1].1) - 3294.5).abs() < 3.5, "{}", centre(&t[1].1));
        assert!((centre(&t[0].1) - 3267.5).abs() < 3.5);
    }

    #[test]
    fn surface_has_a_constant_height_unless_a_panel_is_taller() {
        assert_eq!(surface_height(None), SURFACE_MIN_HEIGHT);
        assert_eq!(surface_height(Some(300.0)), SURFACE_MIN_HEIGHT);
        assert_eq!(surface_height(Some(500.0)), 508);
    }

    #[test]
    fn wake_is_at_most_a_minute_and_faster_with_the_panel_open() {
        let tm = clock::Tm { wday: 1, mday: 1, mon: 0, hour: 1, min: 1, sec: 10 };
        assert_eq!(next_wake(&tm, false), Duration::from_secs(50));
        assert_eq!(next_wake(&tm, true), PANEL_REFRESH);
        let tm = clock::Tm { sec: 58, ..tm };
        assert_eq!(next_wake(&tm, true), Duration::from_secs(2));
    }

    struct Fixed(&'static str);
    impl ActiveWindow for Fixed {
        fn active_app_name(&self) -> Option<String> {
            Some(self.0.to_string())
        }
    }

    #[test]
    fn app_name_follows_the_active_window() {
        let mut bar = DecBar::new(Box::new(Fixed("definitely-not-installed.app")));
        assert!(bar.menus[1].bold);
        assert_eq!(bar.app_name, "App");
        assert!(!bar.refresh_app_name(), "unchanged name needs no rebuild");
    }

    #[test]
    fn floating_bar_is_a_pill_with_content_inset_and_the_flat_bar_is_unchanged() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1000;
        let flat_spans = bar.spans();
        let flat_tray = bar.tray();
        let mut pm = crate::gfx::pixmap(1000, 26).unwrap();
        bar.draw_bar(&mut pm);
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 255, "flat: the strip reaches the corner");
        assert_eq!(flat_spans[0].x, LEFT_PAD);

        TEST_FLOATING.with(|f| f.set(true));
        let (spans, tray) = (bar.spans(), bar.tray());
        let mut pill = crate::gfx::pixmap(1000, 26).unwrap();
        bar.draw_bar(&mut pill);
        TEST_FLOATING.with(|f| f.set(false));
        assert_eq!(pill.pixel(0, 0).unwrap().alpha(), 0, "floating: the corner is transparent");
        assert_eq!(pill.pixel(500, 0).unwrap().alpha(), 0, "...and so is the margin above the pill");
        assert_eq!(pill.pixel(500, 13).unwrap().alpha(), 255, "the pill itself is painted");
        assert_eq!(pill.pixel(500, 13), pm.pixel(500, 13), "same colour as the strip");
        assert_eq!(pill.pixel(3, 13).unwrap().alpha(), 0, "margin left of the pill");
        assert_eq!(spans[0].x, LEFT_PAD + PILL_INSET);
        let (last, flat_last) = (tray.last().unwrap().1, flat_tray.last().unwrap().1);
        assert_eq!(last.x + last.w, flat_last.x + flat_last.w - PILL_INSET);
        assert!(spans.iter().zip(&flat_spans).all(|(a, b)| a.w == b.w), "widths do not change");
    }

    #[test]
    fn default_active_window_is_finder() {
        let bar = DecBar::new(Box::new(active::NoActiveWindow));
        assert_eq!(bar.app_name, "Finder");
    }

    #[test]
    fn closed_bar_only_takes_input_on_the_strip() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let r = bar.input_rects();
        assert_eq!(r, vec![InputRect { x: 0, y: 0, w: 1920, h: 26 }]);
        assert!(!bar.wants_keyboard());
    }

    #[test]
    fn clicking_a_title_opens_its_menu_and_escape_closes_it() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let span = bar.spans()[2];
        assert!(bar.pointer_button(span.x + 5.0, 10.0, Button::Left));
        assert_eq!(bar.open, Open::Menu(2));
        assert!(bar.wants_keyboard());
        assert_eq!(bar.size().1, SURFACE_MIN_HEIGHT);
        assert_eq!(bar.input_rects().len(), 2);
        assert!(bar.key(Key::Escape));
        assert_eq!(bar.open, Open::None);
        assert_eq!(bar.size().1, SURFACE_MIN_HEIGHT);
    }

    #[test]
    fn hovering_another_title_switches_menus_only_while_open() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let spans = bar.spans();
        bar.pointer_motion(spans[3].x + 2.0, 10.0);
        assert_eq!(bar.open, Open::None, "hover alone must not open");
        bar.pointer_button(spans[2].x + 2.0, 10.0, Button::Left);
        bar.pointer_motion(spans[3].x + 2.0, 10.0);
        assert_eq!(bar.open, Open::Menu(3));
    }

    #[test]
    fn clicking_the_open_title_again_closes_it() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let s = bar.spans()[4];
        bar.pointer_button(s.x + 2.0, 10.0, Button::Left);
        bar.pointer_button(s.x + 2.0, 10.0, Button::Left);
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn leaving_the_surface_closes_a_menu() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let s = bar.spans()[2];
        bar.pointer_button(s.x + 2.0, 10.0, Button::Left);
        bar.pointer_leave();
        assert_eq!(bar.open, Open::Menu(2), "not at once");
        expire_leave(&mut bar);
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn dropdown_stays_on_screen() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 300;
        let d = bar.dropdown(6).unwrap();
        assert!(d.x + d.w <= 300.0 + 0.01 || d.x == 0.0);
    }

    #[test]
    fn right_clicks_do_nothing() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let s = bar.spans()[2];
        assert!(!bar.pointer_button(s.x + 2.0, 10.0, Button::Right));
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn control_center_opens_from_its_button_and_tracks_input() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.last_minute = None;
        let ctl = bar.tray().into_iter().find(|(k, _)| *k == Tray::Control).unwrap().1;
        assert!(bar.pointer_button(ctl.x + 3.0, 10.0, Button::Left));
        assert_eq!(bar.open, Open::Control);
        let rects = bar.input_rects();
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[1].w, 340);
        assert!(bar.size().1 > 100);
        assert!(bar.key(Key::Escape));
    }
    // ---- Control Center interaction (dry-run: setters are recorded, never executed) ----

    pub(super) static LOG_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn control_bar() -> DecBar {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.update_snapshot(|s| {
            *s = Snapshot {
                net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: String::new() }),
                volume: Some(sys::Volume { level: 0.5, muted: false }),
                brightness: Some(40),
                night_shift: Some(false),
                dnd: Some(false),
                util: Some(sys::Utils::default()),
                ..Snapshot::default()
            }
        });
        bar.relayout_control();
        bar.open = Open::Control;
        bar
    }

    fn centre_of(bar: &DecBar, t: Target) -> (f32, f32, controlcenter::R) {
        let it = bar.cc_items.iter().find(|i| i.target == Some(t)).expect("target present");
        let (ox, oy) = bar.panel_origin();
        (ox + it.r.x + it.r.w / 2.0, oy + it.r.y + it.r.h / 2.0, it.r)
    }

    #[test]
    fn clicking_wifi_toggles_it_and_runs_nmcli() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let _ = sys::take_log();
        let mut bar = control_bar();
        let (x, y, _) = centre_of(&bar, Target::Wifi);
        assert!(bar.pointer_button(x, y, Button::Left));
        assert_eq!(bar.snapshot().net.map(|n| n.wifi_enabled), Some(false), "optimistic update");
        assert!(sys::take_log().iter().any(|l| l == "nmcli radio wifi off"));
        sys::set_dry_run(false);
    }

    #[test]
    fn night_shift_and_focus_toggle_their_state() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = control_bar();
        let (x, y, _) = centre_of(&bar, Target::NightShift);
        bar.pointer_button(x, y, Button::Left);
        assert_eq!(bar.snapshot().night_shift, Some(true));
        let (x, y, _) = centre_of(&bar, Target::Focus);
        bar.pointer_button(x, y, Button::Left);
        assert_eq!(bar.snapshot().dnd, Some(true));
        sys::set_dry_run(false);
    }

    #[test]
    fn utility_tiles_run_decutil_and_flip_their_state() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let _ = sys::take_log();
        let mut bar = control_bar();
        for t in [Target::IdleInhibit, Target::GameMode, Target::Dnd] {
            let (x, y, _) = centre_of(&bar, t);
            assert!(bar.pointer_button(x, y, Button::Left));
        }
        let u = bar.snapshot().util.unwrap();
        assert!(u.idle && u.game, "optimistic update");
        assert_eq!(bar.snapshot().dnd, Some(true));
        let log = sys::take_log();
        for want in ["idle-inhibit toggle", "gamemode toggle", "dnd toggle"] {
            assert!(log.iter().any(|l| l.contains("decutil") && l.ends_with(want)), "{want} missing in {log:?}");
        }
        // Record closes the panel (so it is not in the first frames) and starts decutil a moment later.
        let (x, y, _) = centre_of(&bar, Target::Record);
        bar.pointer_button(x, y, Button::Left);
        assert_eq!(bar.open, Open::None);
        assert!(sys::take_log().iter().any(|l| l.contains("decutil") && l.contains("record toggle")));
        // The notification history tile opens decnotify's panel.
        let mut bar = control_bar();
        let (x, y, _) = centre_of(&bar, Target::Notifications);
        bar.pointer_button(x, y, Button::Left);
        assert!(sys::take_log().iter().any(|l| l.contains("decnotify history")));
        sys::set_dry_run(false);
    }

    #[test]
    fn dragging_the_volume_slider_tracks_the_pointer_and_releases() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let _ = sys::take_log();
        let mut bar = control_bar();
        let (_, y, r) = centre_of(&bar, Target::Volume);
        let (ox, _) = bar.panel_origin();
        // Press at 25%, drag to 80%, release past the right edge.
        assert!(bar.pointer_button(ox + r.x + r.w * 0.25, y, Button::Left));
        assert!((bar.snapshot().volume.unwrap().level - 0.25).abs() < 0.01);
        assert!(bar.pointer_motion(ox + r.x + r.w * 0.8, y + 40.0), "drag continues off the slider");
        assert!((bar.snapshot().volume.unwrap().level - 0.8).abs() < 0.01);
        assert!(bar.pointer_release(ox + r.x + r.w + 100.0, y, Button::Left));
        assert!((bar.snapshot().volume.unwrap().level - 1.0).abs() < 0.01, "clamped at the end");
        assert!(bar.cc_drag.is_none());
        if sys::have("wpctl") {
            assert!(sys::take_log().iter().any(|l| l.starts_with("wpctl set-volume @DEFAULT_AUDIO_SINK@ 1.00")));
        }
        sys::set_dry_run(false);
    }

    #[test]
    fn leaving_the_surface_mid_drag_does_not_close_the_panel() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = control_bar();
        let (x, y, _) = centre_of(&bar, Target::Brightness);
        bar.pointer_button(x, y, Button::Left);
        bar.pointer_leave();
        assert_eq!(bar.open, Open::Control);
        bar.pointer_release(x, y, Button::Left);
        bar.pointer_leave();
        expire_leave(&mut bar);
        assert_eq!(bar.open, Open::None);
        sys::set_dry_run(false);
    }

    #[test]
    fn scrolling_over_a_slider_nudges_it_by_5_percent() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = control_bar();
        let (x, y, _) = centre_of(&bar, Target::Volume);
        assert!(bar.scroll(x, y, -1.0));
        assert!((bar.snapshot().volume.unwrap().level - 0.55).abs() < 0.01);
        assert!(bar.scroll(x, y, 1.0));
        assert!((bar.snapshot().volume.unwrap().level - 0.50).abs() < 0.01);
        assert!(!bar.scroll(2.0, 200.0, 1.0), "not over a slider");
        sys::set_dry_run(false);
    }

    #[test]
    fn control_center_wheel_keeps_highlight_and_repaint_consistent() {
        let mut bar = control_bar();
        let (x, y, _) = centre_of(&bar, Target::DarkMode);
        bar.cc_viewport.focus = Some(Target::DarkMode);
        bar.cc_hover = Some(Target::DarkMode);
        assert!(!bar.scroll(-100.0, -100.0, 1.0));
        assert_eq!(bar.cc_viewport.focus, Some(Target::DarkMode), "unhandled wheel must not change the painted state");
        assert_eq!(bar.cc_hover, Some(Target::DarkMode));
        assert!(bar.scroll(x, y, 1.0), "clearing a visible highlight must request a repaint even without scrolling");
        assert_eq!(bar.cc_viewport.focus, None);
        assert_eq!(bar.cc_hover, None);
    }

    #[test]
    fn hovering_panel_items_highlights_only_interactive_ones() {
        let mut bar = control_bar();
        let (x, y, _) = centre_of(&bar, Target::DarkMode);
        assert!(bar.pointer_motion(x, y));
        assert_eq!(bar.cc_hover, Some(Target::DarkMode));
        assert!(bar.pointer_motion(x, 400.0 + y));
        assert_eq!(bar.cc_hover, None);
    }

    #[test]
    fn destructive_menu_items_need_a_second_click_without_the_helper() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.runner = Runner::without_helper();
        let apple = bar.spans()[0];
        bar.pointer_button(apple.x + 3.0, 10.0, Button::Left);
        let d = bar.dropdown(0).unwrap();
        let idx = bar.menus[0].items.iter().position(|i| i.action == "shutdown").unwrap();
        let row = d.rows[idx].clone();
        // First click arms the confirmation and keeps the menu open.
        assert!(bar.pointer_button(d.x + 20.0, row.y + 5.0, Button::Left));
        assert_eq!(bar.confirm, Some("shutdown"));
        assert_eq!(bar.open, Open::Menu(0));
        // Escape cancels everything.
        bar.key(Key::Escape);
        assert_eq!(bar.confirm, None);
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn unavailable_items_are_inert() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.runner = Runner::without_helper();
        let file = bar.spans()[2];
        bar.pointer_button(file.x + 3.0, 10.0, Button::Left);
        let d = bar.dropdown(2).unwrap();
        // "New Window" has no portable fallback, so the click does nothing and the menu stays.
        assert!(!bar.pointer_button(d.x + 20.0, d.rows[0].y + 5.0, Button::Left));
        assert_eq!(bar.open, Open::Menu(2));
    }

    #[test]
    fn workspace_dots_are_hit_by_cell() {
        let span = Span { x: 100.0, w: workspaces_width(4) };
        assert_eq!(workspace_hit(span, 4, 100.0 + WS_PAD / 2.0 + 1.0), Some(0));
        assert_eq!(workspace_hit(span, 4, 100.0 + WS_PAD / 2.0 + WS_SLOT * 2.5), Some(2));
        assert_eq!(workspace_hit(span, 4, 100.0 + 1.0), None, "left padding");
        assert_eq!(workspace_hit(span, 4, span.x + span.w - 1.0), None, "right padding");
    }

    type Log = std::rc::Rc<std::cell::RefCell<Vec<usize>>>;
    struct Spaces(Log, Vec<Workspace>);
    impl ActiveWindow for Spaces {
        fn active_app_name(&self) -> Option<String> {
            None
        }
        fn workspaces(&self) -> Vec<Workspace> {
            self.1.clone()
        }
        fn switch_workspace(&self, index: usize) {
            self.0.borrow_mut().push(index);
        }
    }

    fn spaces(n: usize) -> Vec<Workspace> {
        (0..n).map(|i| Workspace { name: (i + 1).to_string(), active: i == 1, occupied: i == 2, urgent: false }).collect()
    }

    #[test]
    fn workspaces_are_hidden_unless_enabled() {
        let bar = DecBar::new(Box::new(Spaces(Default::default(), spaces(4))));
        assert!(!bar.tray_items().iter().any(|(k, _)| *k == Tray::Workspaces));
        let bar = DecBar::new(Box::new(Spaces(Default::default(), spaces(4)))).with_workspaces(true);
        let first = bar.tray_items()[0];
        assert_eq!(first, (Tray::Workspaces, workspaces_width(4)));
    }

    #[test]
    fn no_workspaces_means_no_indicator_even_when_enabled() {
        let bar = DecBar::new(Box::new(Spaces(Default::default(), vec![]))).with_workspaces(true);
        assert!(!bar.tray_items().iter().any(|(k, _)| *k == Tray::Workspaces));
    }

    #[test]
    fn clicking_a_workspace_dot_asks_the_wm_to_switch() {
        let log = Log::default();
        let mut bar = DecBar::new(Box::new(Spaces(log.clone(), spaces(4)))).with_workspaces(true);
        bar.width = 1920;
        let (_, span) = bar.tray().into_iter().find(|(k, _)| *k == Tray::Workspaces).unwrap();
        let x = span.x + WS_PAD / 2.0 + WS_SLOT * 3.5;
        assert!(!bar.pointer_button(x, 10.0, Button::Left));
        assert_eq!(*log.borrow(), vec![3]);
    }

    #[test]
    fn workspace_changes_trigger_a_redraw() {
        let mut bar = DecBar::new(Box::new(Spaces(Default::default(), spaces(3)))).with_workspaces(true);
        assert!(!bar.refresh_workspaces(), "already up to date");
        bar.workspaces.clear();
        assert!(bar.refresh_workspaces());
    }

    #[test]
    fn idle_drops_fonts_loaded_by_layout() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        let Some(f) = &bar.fonts else { return };
        f.measure("never drawn \u{2603} string", 12.0, Weight::Bold);
        assert!(f.has_loaded_face());
        assert!(bar.idle().is_some(), "a deferred trim is pending");
        assert!(!bar.fonts.as_ref().unwrap().has_loaded_face());
    }

    // ---- status popups ----

    fn status_bar() -> DecBar {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.clock_text = "Tue 27 Oct  12:00".into();
        bar.update_snapshot(|s| {
            *s = Snapshot {
                net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: String::new() }),
                bluetooth: Some(sys::Bluetooth { powered: true, connected: 0 }),
                volume: Some(sys::Volume { level: 0.5, muted: false }),
                battery: Some(sys::Battery { percent: 80, charging: false, status: "Discharging".into() }),
                ..Snapshot::default()
            }
        });
        *bar.shared.layout.lock().unwrap() = Some(rdwm::LayoutState { name: "tile".into(), symbol: "[]=".into(), tags: Vec::new() });
        bar
    }

    fn click_tray(bar: &mut DecBar, kind: Tray) -> bool {
        let s = bar.tray_span(kind).expect("item visible");
        bar.pointer_button(s.x + s.w / 2.0, 10.0, Button::Left)
    }

    #[test]
    fn dashboard_item_sits_before_spotlight_and_opens_decdash_under_itself() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.update_snapshot(|s| *s = Snapshot::default());
        let kinds: Vec<Tray> = bar.tray().into_iter().map(|t| t.0).collect();
        let at = |k: Tray| kinds.iter().position(|x| *x == k).unwrap();
        assert_eq!(at(Tray::Dash) + 1, at(Tray::Spotlight), "the dashboard glyph is left of the search glyph");
        let span = bar.tray_span(Tray::Dash).unwrap();
        assert_eq!(dash_argv("decdash", span), ["decdash".to_string(), "--x".to_string(), format!("{}", (span.x + span.w / 2.0).round() as i32)]);
        assert_eq!(dash_argv("/opt/decdash", Span { x: 100.0, w: 27.0 })[2], "114", "the middle of the item, rounded");
    }

    #[test]
    fn clicking_the_coffee_cup_turns_stay_awake_off_and_right_click_offers_timers() {
        let (_state, dir) = awake::tests::private_state("bar");
        std::fs::write(dir.join("omarchy/indicators/stay-awake"), "").unwrap();
        let mut bar = status_bar();
        bar.update_snapshot(|s| s.awake = true);
        assert!(bar.tray_span(Tray::Awake).is_some(), "the cup is shown while Stay Awake is on");
        // right click: the options popup, with the explanation and the durations
        let s = bar.tray_span(Tray::Awake).unwrap();
        assert!(bar.pointer_button(s.x + s.w / 2.0, 10.0, Button::Right));
        assert_eq!(bar.open, Open::Popup(Tray::Awake));
        let rows = bar.popup.clone().unwrap().rows;
        assert!(rows[0].label == "Stay Awake" && rows[0].detail.starts_with("On"), "{:?}", rows[0]);
        assert!(rows.iter().any(|r| r.act == popup::Act::AwakeOff));
        for (secs, _) in awake::DURATIONS {
            assert!(rows.iter().any(|r| r.act == popup::Act::AwakeFor(secs)));
        }
        // a timed session is recorded and the popup stays open
        bar.run_act(popup::Act::AwakeFor(Some(3600)));
        assert!(awake::until().is_some());
        assert_eq!(bar.open, Open::Popup(Tray::Awake));
        assert!(bar.popup.as_ref().unwrap().rows.iter().any(|r| r.label.starts_with("Turns off in")));
        bar.close();
        // left click: off, at once, for the whole bar (the icon is gone), and the timer with it
        assert!(click_tray(&mut bar, Tray::Awake));
        assert!(!bar.snapshot().awake);
        assert!(bar.tray_span(Tray::Awake).is_none());
        assert!(!awake::is_on(), "the real switch was flipped, not just the icon");
        assert_eq!(awake::until(), None);
        assert_eq!(bar.open, Open::None);
        // the options popup's "Turn off" row does the same
        std::fs::write(dir.join("omarchy/indicators/stay-awake"), "").unwrap();
        bar.update_snapshot(|s| s.awake = true);
        bar.open_popup(Tray::Awake);
        bar.run_act(popup::Act::AwakeOff);
        assert!(!awake::is_on() && !bar.snapshot().awake && bar.open == Open::None);
        *awake::TEST_DIR.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn wx_bar() -> DecBar {
        let bar = status_bar();
        *bar.shared.weather.lock().unwrap() = Some(sample_weather());
        bar
    }

    #[test]
    fn clicking_the_weather_item_opens_a_popup_and_clicking_again_closes_it() {
        let mut bar = wx_bar();
        let s = bar.tray_span(Tray::Weather).expect("shown once there is a reading");
        // the whole drawn item is the hit area: left and right edges as well as the middle
        for x in [s.x + 1.0, s.x + s.w / 2.0, s.x + s.w - 1.0] {
            assert!(bar.pointer_button(x, 10.0, Button::Left), "x={x}");
            assert_eq!(bar.open, Open::Popup(Tray::Weather));
            assert!(bar.popup.is_some());
            assert!(click_tray(&mut bar, Tray::Weather));
            assert_eq!(bar.open, Open::None);
        }
    }

    #[test]
    fn weather_popup_shows_conditions_location_range_and_forecast() {
        let bar = wx_bar();
        let (rows, w, _) = bar.build_popup(Tray::Weather).unwrap();
        let text: Vec<String> = rows.iter().map(|r| format!("{}|{}", r.label, r.detail)).collect();
        let all = text.join("\n");
        assert!(rows[0].label == "Berlin, Germany" && rows[0].detail == "Overcast", "{all}");
        assert!(all.contains("9°C, feels like 6°"), "{all}");
        assert!(all.contains("Humidity 80%") && all.contains("Wind 14 km/h NW"), "{all}");
        assert!(all.contains("Today: high 11°, low 4°"), "{all}");
        assert!(all.contains("Fri|8° / 2°") && all.contains("Sat|6° / -1°"), "{all}");
        let last = rows.last().unwrap();
        assert!(last.label == "Refresh" && last.act == popup::Act::WeatherRefresh && last.detail.starts_with("Updated"), "{all}");
        assert!(w > 200.0);
    }

    #[test]
    fn weather_popup_without_details_or_data_stays_sensible() {
        let bar = status_bar();
        // no reading at all: an empty state that still offers Refresh
        let (rows, _, _) = bar.build_popup(Tray::Weather).unwrap();
        assert!(rows.iter().any(|r| r.label.starts_with("No weather data")));
        assert!(rows.iter().any(|r| r.act == popup::Act::WeatherRefresh));
        bar.shared.weather_failed.store(true, Ordering::SeqCst);
        let (rows, _, _) = bar.build_popup(Tray::Weather).unwrap();
        assert!(rows.iter().any(|r| r.label.contains("offline")));
        // an old cache entry (temperature and condition only) shows just those, and says it is old when offline
        *bar.shared.weather.lock().unwrap() = Some(Weather { temp_c: 3, code: 296, fetched: weather::now() - 7200, ..Weather::default() });
        let (rows, _, _) = bar.build_popup(Tray::Weather).unwrap();
        assert_eq!(rows[0].label, "Weather");
        assert_eq!(rows[0].detail, "Rain");
        assert!(rows.iter().any(|r| r.label == "3°C"));
        assert!(!rows.iter().any(|r| r.label.starts_with("Humidity") || r.label == "Forecast"));
        let refresh = rows.last().unwrap();
        assert!(refresh.detail.starts_with("Offline") && refresh.detail.contains("2 h ago"), "{refresh:?}");
    }

    #[test]
    fn the_refresh_row_keeps_the_popup_open_and_never_touches_the_network_in_tests() {
        let mut bar = wx_bar();
        click_tray(&mut bar, Tray::Weather);
        let p = bar.popup.clone().unwrap();
        let i = p.rows.iter().position(|r| r.act == popup::Act::WeatherRefresh).unwrap();
        let (ry, rh) = p.row_span(i);
        assert!(bar.pointer_button(p.x + 30.0, p.y + ry + rh / 2.0, Button::Left));
        assert_eq!(bar.open, Open::Popup(Tray::Weather));
        assert!(!bar.shared.weather_busy.load(Ordering::SeqCst), "spawn_weather is a no-op under test");
    }

    #[test]
    fn weather_item_is_hit_where_it_is_drawn() {
        let bar = wx_bar();
        let s = bar.tray_span(Tray::Weather).unwrap();
        let temp_w = bar.fonts.as_ref().map(|f| f.measure("9°", TEXT_PX, Weight::Regular)).unwrap_or(14.0);
        // icon (9 px pad + ICON_PX) and the temperature text both lie inside the span
        assert!(s.w >= 9.0 + ICON_PX + 6.0 + temp_w - 0.5, "span {} too narrow for the drawn icon and text", s.w);
    }

    #[test]
    fn the_agents_item_opens_the_process_monitor_popup() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let _ = sys::take_log();
        let bar = status_bar();
        bar.process_monitor(vec!["--x".into(), "700".into()]);
        assert!(sys::take_log().iter().any(|l| l.contains("decmon --popup --x 700")));
        sys::set_dry_run(false);
    }

    #[test]
    fn items_without_their_tool_are_hidden() {
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.update_snapshot(|s| *s = Snapshot::default());
        let kinds: Vec<Tray> = bar.tray().into_iter().map(|t| t.0).collect();
        for hidden in [Tray::Layout, Tray::Battery, Tray::Bluetooth, Tray::Audio, Tray::Network] {
            assert!(!kinds.contains(&hidden), "{hidden:?} should be hidden");
        }
        assert!(kinds.contains(&Tray::Clock) && kinds.contains(&Tray::Control));
        // Clicking where nothing is does not open anything.
        assert!(bar.tray_span(Tray::Layout).is_none());
    }

    #[test]
    fn layout_item_opens_a_popup_and_picking_a_row_applies_it() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = status_bar();
        assert!(click_tray(&mut bar, Tray::Layout));
        assert_eq!(bar.open, Open::Popup(Tray::Layout));
        let p = bar.popup.clone().expect("popup");
        let rows = &p.rows;
        assert!(rows.iter().filter(|r| r.checked).count() == 1 && rows.iter().any(|r| r.checked && r.label == "Tile"));
        assert_eq!(rows.iter().filter(|r| matches!(r.act, popup::Act::SetLayout(_))).count(), rdwm::LAYOUTS.len());
        assert!(bar.input_rects().len() >= 2);
        let idx = rows.iter().position(|r| r.act == popup::Act::SetLayout("bsp".into())).unwrap();
        let (ry, rh) = p.row_span(idx);
        sys::take_log();
        assert!(bar.pointer_button(p.x + 40.0, p.y + ry + rh / 2.0, Button::Left));
        assert!(sys::take_log().contains(&"rdwmctl action setlayout bsp".to_string()));
        assert_eq!(bar.open, Open::None);
        // The bar shows the new layout at once, before rdwm's event arrives.
        assert_eq!(bar.layout_state().map(|l| l.name), Some("bsp".into()));
        sys::set_dry_run(false);
    }

    #[test]
    fn wheel_over_the_layout_item_cycles_layouts() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = status_bar();
        let s = bar.tray_span(Tray::Layout).unwrap();
        sys::take_log();
        bar.scroll(s.x + 5.0, 10.0, 1.0);
        bar.scroll(s.x + 5.0, 10.0, -1.0);
        assert_eq!(sys::take_log(), vec!["rdwmctl action cyclelayout".to_string(), "rdwmctl action cyclelayout_back".to_string()]);
        sys::set_dry_run(false);
    }

    #[test]
    fn wheel_over_the_sound_icon_steps_five_percent() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = status_bar();
        let s = bar.tray_span(Tray::Audio).unwrap();
        assert!(bar.scroll(s.x + 5.0, 10.0, -1.0));
        assert!((bar.snapshot().volume.unwrap().level - 0.55).abs() < 1e-4);
        assert!(bar.scroll(s.x + 5.0, 10.0, 1.0) && bar.scroll(s.x + 5.0, 10.0, 1.0));
        assert!((bar.snapshot().volume.unwrap().level - 0.45).abs() < 1e-4);
        // Scrolling elsewhere on the bar does nothing.
        assert!(!bar.scroll(5.0, 10.0, 1.0));
        sys::set_dry_run(false);
    }

    #[test]
    fn sound_popup_slider_and_mute() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = status_bar();
        click_tray(&mut bar, Tray::Audio);
        assert_eq!(bar.open, Open::Popup(Tray::Audio));
        let p = bar.popup.clone().unwrap();
        let slider = p.rows.iter().position(|r| matches!(r.kind, popup::Kind::Slider(_))).unwrap();
        let (ry, rh) = p.row_span(slider);
        let (tx, tw) = p.slider_track();
        let y = p.y + ry + rh / 2.0;
        assert!(bar.pointer_button(tx + tw * 0.8, y, Button::Left));
        assert!(bar.popup_drag.is_some());
        bar.pointer_motion(tx + tw * 0.2, y);
        bar.pointer_release(tx + tw * 0.2, y, Button::Left);
        assert!(bar.popup_drag.is_none());
        assert!((bar.snapshot().volume.unwrap().level - 0.2).abs() < 0.01);
        // Leaving the surface mid-drag must not close the popup.
        bar.pointer_button(tx + tw * 0.5, y, Button::Left);
        assert!(!bar.pointer_leave() || bar.open != Open::None);
        bar.pointer_release(tx + tw * 0.5, y, Button::Left);
        let mute = bar.popup.as_ref().unwrap().rows.iter().position(|r| r.act == popup::Act::ToggleMute).unwrap();
        let (my, mh) = bar.popup.as_ref().unwrap().row_span(mute);
        bar.pointer_button(p.x + 30.0, p.y + my + mh / 2.0, Button::Left);
        assert!(bar.snapshot().volume.unwrap().muted);
        assert_eq!(bar.open, Open::Popup(Tray::Audio));
        sys::set_dry_run(false);
    }

    #[test]
    fn network_and_bluetooth_popups_list_what_the_workers_found() {
        let bar = status_bar();
        *bar.shared.wifi.lock().unwrap() = vec![sys::WifiNet { ssid: "Cafe".into(), signal: 60, secure: false, active: false }];
        *bar.shared.bt.lock().unwrap() = vec![sys::BtDevice { mac: "AA".into(), name: "Buds".into(), connected: true }];
        let (rows, _, _) = bar.build_popup(Tray::Network).unwrap();
        assert!(rows.iter().any(|r| r.act == popup::Act::WifiConnect("Cafe".into())));
        assert!(rows.iter().any(|r| r.act == popup::Act::WifiToggle && r.checked));
        let (rows, _, _) = bar.build_popup(Tray::Bluetooth).unwrap();
        assert!(rows.iter().any(|r| r.act == (popup::Act::BtDevice { mac: "AA".into(), connect: false }) && r.checked));
        bar.update_snapshot(|s| s.bluetooth = Some(sys::Bluetooth { powered: false, connected: 0 }));
        let (rows, _, _) = bar.build_popup(Tray::Bluetooth).unwrap();
        assert!(!rows.iter().any(|r| matches!(r.act, popup::Act::BtDevice { .. })));
    }

    #[test]
    fn clock_opens_a_calendar_that_navigates_and_escape_closes() {
        let mut bar = status_bar();
        assert!(click_tray(&mut bar, Tray::Clock));
        assert_eq!(bar.open, Open::Popup(Tray::Clock));
        let c = bar.calendar.clone().expect("calendar");
        assert!(bar.input_rects().len() >= 2);
        assert!(bar.size().1 as f32 >= c.y + Calendar::H);
        let before = (c.year, c.mon0);
        assert!(bar.pointer_button(c.x + Calendar::W - 20.0, c.y + 20.0, Button::Left));
        let after = bar.calendar.as_ref().map(|c| (c.year, c.mon0)).unwrap();
        assert_ne!(before, after);
        assert!(bar.key(Key::Escape));
        assert_eq!(bar.open, Open::None);
        assert!(bar.calendar.is_none() && bar.popup.is_none());
    }

    #[test]
    fn popups_toggle_and_replace_each_other() {
        let mut bar = status_bar();
        click_tray(&mut bar, Tray::Audio);
        click_tray(&mut bar, Tray::Network);
        assert_eq!(bar.open, Open::Popup(Tray::Network));
        click_tray(&mut bar, Tray::Network);
        assert_eq!(bar.open, Open::None);
        // A battery popup mentions the percentage.
        click_tray(&mut bar, Tray::Battery);
        assert!(bar.popup.as_ref().unwrap().rows[0].detail.contains("80%"));
        bar.pointer_leave();
        expire_leave(&mut bar);
        assert_eq!(bar.open, Open::None);
    }

    /// Pretends the close delay has passed and runs the tick that closes.
    fn expire_leave(bar: &mut DecBar) {
        bar.leave_at = bar.leave_at.map(|_| Instant::now() - CLOSE_DELAY - Duration::from_millis(5));
        bar.tick();
    }

    #[test]
    fn popup_survives_a_short_trip_off_the_surface() {
        let mut bar = status_bar();
        click_tray(&mut bar, Tray::Audio);
        let p = bar.popup.clone().unwrap();
        bar.pointer_leave();
        assert!(bar.tick_in().is_some_and(|d| d <= CLOSE_DELAY), "the backend is asked to tick when the delay ends");
        let t = bar.tick();
        assert_eq!(bar.open, Open::Popup(Tray::Audio));
        assert!(t.next.is_some_and(|d| d <= CLOSE_DELAY), "a tick is scheduled to do the closing");
        // The pointer comes back before the delay is up: nothing closes, later or ever.
        bar.pointer_motion(p.x + 40.0, p.y + 30.0);
        assert!(bar.leave_at.is_none());
        expire_leave(&mut bar);
        assert_eq!(bar.open, Open::Popup(Tray::Audio));
        // Wandering off while grabbed (X11 reports motion, not leave) starts the delay too.
        bar.pointer_motion(p.x + p.w + 300.0, p.y + 400.0);
        assert!(bar.leave_at.is_some());
        bar.pointer_motion(p.x + 40.0, p.y + 30.0);
        assert!(bar.leave_at.is_none());
        // Gone for good: closes after the delay.
        bar.pointer_leave();
        expire_leave(&mut bar);
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn the_gap_between_item_and_popup_is_part_of_the_popup() {
        let mut bar = status_bar();
        for kind in [Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Layout, Tray::Battery, Tray::Clock] {
            bar.close();
            click_tray(&mut bar, kind);
            if bar.open != Open::Popup(kind) {
                continue;
            }
            let span = bar.tray_span(kind).unwrap();
            let top = bar.popup.as_ref().map(|p| p.y).or(bar.calendar.as_ref().map(|c| c.y)).unwrap();
            // Straight down from the icon, row by row, never off the interactive area.
            let mut y = 0.0;
            while y < top + 40.0 {
                assert!(bar.over_open_ui(span.x + span.w / 2.0, y), "{kind:?}: pointer at y={y} left the surface");
                y += 1.0;
            }
        }
    }

    #[test]
    fn clicks_outside_close_and_clicks_inside_do_not() {
        let mut bar = status_bar();
        click_tray(&mut bar, Tray::Audio);
        let p = bar.popup.clone().unwrap();
        assert!(bar.wants_pointer_grab(), "the pointer is grabbed while open (X11)");
        // On the popup's header: nothing to do, but it stays open.
        bar.pointer_button(p.x + 20.0, p.y + 12.0, Button::Left);
        assert_eq!(bar.open, Open::Popup(Tray::Audio));
        bar.pointer_button(p.x + p.w + 80.0, p.y + 60.0, Button::Left);
        assert_eq!(bar.open, Open::None);
        assert!(!bar.wants_pointer_grab());
        // Outside our surface altogether (negative coordinates of a grabbed pointer).
        click_tray(&mut bar, Tray::Network);
        bar.pointer_button(-50.0, 300.0, Button::Left);
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn a_slider_drag_keeps_working_outside_the_popup() {
        let _g = LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let mut bar = status_bar();
        click_tray(&mut bar, Tray::Audio);
        let p = bar.popup.clone().unwrap();
        let slider = p.rows.iter().position(|r| matches!(r.kind, popup::Kind::Slider(_))).unwrap();
        let (sy, sh) = p.row_span(slider);
        let (tx, tw) = p.slider_track();
        bar.pointer_button(tx + tw * 0.2, p.y + sy + sh / 2.0, Button::Left);
        bar.pointer_leave();
        bar.pointer_motion(tx + tw * 0.9, p.y + p.h + 200.0);
        expire_leave(&mut bar);
        assert_eq!(bar.open, Open::Popup(Tray::Audio), "dragging holds the popup open");
        let v = bar.snapshot().volume.unwrap().level;
        assert!((v - 0.9).abs() < 0.02, "{v}");
        bar.pointer_release(tx + tw * 0.9, p.y + p.h + 200.0, Button::Left);
        assert_eq!(bar.open, Open::Popup(Tray::Audio));
        expire_leave(&mut bar);
        assert_eq!(bar.open, Open::None, "released outside: closes after the delay");
        sys::set_dry_run(false);
    }
}

/// What the Control Center's Settings tile starts: Decebal's own System Settings first (next to this executable
/// for an uninstalled build), Omarchy's menu only when decsettings is not installed.
fn settings_command(have_decsettings: bool, have_omarchy_menu: bool) -> Option<Vec<String>> {
    let own = crate::launch::sibling_or_path("decsettings");
    if have_decsettings || std::path::Path::new(&own).is_absolute() {
        Some(vec![own])
    } else if have_omarchy_menu {
        Some(["sh", "-c", "sleep 0.15; omarchy-menu toggle setup"].map(String::from).to_vec())
    } else {
        None
    }
}

#[cfg(test)]
mod settings_tile_tests {
    use super::settings_command;

    #[test]
    fn settings_tile_prefers_decsettings_over_omarchy_menu() {
        let argv = settings_command(true, true).unwrap();
        assert_eq!(argv.len(), 1);
        assert!(argv[0].ends_with("decsettings"));
    }

    #[test]
    fn settings_tile_falls_back_to_omarchy_menu() {
        // The test binary sits next to no decsettings, so only the PATH flag decides.
        if let Some(argv) = settings_command(false, true) {
            assert!(argv[0].ends_with("decsettings") || argv[0] == "sh");
        }
        assert!(settings_command(false, false).is_none() || settings_command(false, false).unwrap()[0].ends_with("decsettings"));
    }
}
