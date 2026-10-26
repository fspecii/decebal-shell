//! The macOS-style menu bar: Apple menu, app name, dropdown menus, status icons, clock and the
//! Control Center. One surface that grows downward while a menu or the panel is open.

pub mod active;
pub mod clock;
pub mod menus;
pub mod symbols;
pub mod weather;

use crate::app::{App, Button, InputRect, Key, Tick, Waker};
use crate::controlcenter::{self, sys, Snapshot, Target};
use crate::gfx::{color, fill_rounded, stroke_rounded};
use crate::text::{Fonts, Weight};
use crate::theme::{BAR_BG, BAR_HEIGHT};
use active::ActiveWindow;
use menus::{Dropdown, Menu, Runner};
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
const BTN_W: f32 = 27.0;
const ICON_PX: f32 = 16.0;
const INK: (u8, u8, u8) = (222, 222, 222);
const MENU_GAP: f32 = 2.0;
const PANEL_GAP: f32 = 4.0;
const BOTTOM_MARGIN: f32 = 8.0;
/// While the Control Center is open its data is refreshed this often.
const PANEL_REFRESH: Duration = Duration::from_secs(4);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tray {
    Weather,
    Awake,
    Bluetooth,
    Audio,
    Network,
    Spotlight,
    Control,
    Clock,
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

/// Window height needed for the bar plus whatever is open below it.
pub fn surface_height(open_bottom: Option<f32>) -> u32 {
    match open_bottom {
        Some(b) => (b + BOTTOM_MARGIN).ceil() as u32,
        None => BAR_HEIGHT as u32,
    }
}

// ------------------------------------------------------------------- state

#[derive(Debug, Clone, Copy, PartialEq)]
enum Open {
    None,
    Menu(usize),
    Control,
}

struct Shared {
    snap: Mutex<Snapshot>,
    weather: Mutex<Option<Weather>>,
    changed: AtomicBool,
    snap_busy: AtomicBool,
    weather_busy: AtomicBool,
}

pub struct MacBar {
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
    cc_hover: Option<Target>,
    cc_drag: Option<Target>,
    cc_last_send: Instant,
    last_full_refresh: Instant,
}

impl MacBar {
    pub fn new(active: Box<dyn ActiveWindow>) -> Self {
        let shared = Arc::new(Shared {
            snap: Mutex::new(Snapshot { light: sys::is_light(), awake: sys::stay_awake(), ..Snapshot::default() }),
            weather: Mutex::new(weather::fresh_cached()),
            changed: AtomicBool::new(false),
            snap_busy: AtomicBool::new(false),
            weather_busy: AtomicBool::new(false),
        });
        let mut bar = MacBar {
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
            height: BAR_HEIGHT as u32,
            clock_text: String::new(),
            last_minute: None,
            shared,
            waker: None,
            cc_items: Vec::new(),
            cc_height: 0.0,
            cc_hover: None,
            cc_drag: None,
            cc_last_send: Instant::now(),
            last_full_refresh: Instant::now(),
        };
        bar.refresh_app_name();
        bar
    }

    /// Renders one frame to a PNG (for testing and screenshots). `open` is a menu key or `control`.
    pub fn render_png(&mut self, path: &str, width: u32, open: Option<&str>) -> Result<(), String> {
        self.width = width;
        self.tick();
        if let Ok(mut s) = self.shared.snap.lock() {
            *s = Snapshot::collect();
        }
        self.shared.weather.lock().map_err(|e| e.to_string())?.get_or_insert_with(|| Weather { temp_c: 9, code: 122, fetched: 0 });
        match open {
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

    fn snapshot(&self) -> Snapshot {
        self.shared.snap.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn update_snapshot(&self, f: impl FnOnce(&mut Snapshot)) {
        if let Ok(mut s) = self.shared.snap.lock() {
            f(&mut s);
        }
    }

    fn refresh_app_name(&mut self) -> bool {
        let name = self.active.active_app_name().map(|id| active::display_name(&id)).unwrap_or_else(|| "Finder".to_string());
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
        title_spans(&self.menus, |m| self.label_w(m))
    }

    fn tray_items(&self) -> Vec<(Tray, f32)> {
        let snap = self.snapshot();
        let mut items = Vec::new();
        let weather = self.shared.weather.lock().ok().and_then(|w| w.clone());
        if let Some(w) = &weather {
            let tw = self.fonts.as_ref().map(|f| f.measure(&format!("{}°", w.temp_c), TEXT_PX, Weight::Regular)).unwrap_or(14.0);
            items.push((Tray::Weather, ICON_PX + 6.0 + tw + 18.0));
        }
        if snap.awake {
            items.push((Tray::Awake, BTN_W));
        }
        if snap.bluetooth.as_ref().map(|b| b.powered && b.connected > 0).unwrap_or(false) {
            items.push((Tray::Bluetooth, BTN_W));
        }
        if snap.volume.is_some() {
            items.push((Tray::Audio, BTN_W));
        }
        if snap.net.is_some() {
            items.push((Tray::Network, BTN_W));
        }
        items.push((Tray::Spotlight, BTN_W));
        items.push((Tray::Control, BTN_W));
        let clock_w = self.fonts.as_ref().map(|f| f.measure(&self.clock_text, TEXT_PX, Weight::Regular)).unwrap_or(100.0) + 17.0;
        items.push((Tray::Clock, clock_w));
        items
    }

    fn tray(&self) -> Vec<(Tray, Span)> {
        tray_spans(self.width as f32, &self.tray_items())
    }

    fn dropdown(&self, idx: usize) -> Option<Dropdown> {
        let menu = self.menus.get(idx)?;
        let span = *self.spans().get(idx)?;
        let f = self.fonts.as_ref();
        let mut d = Dropdown::layout(menu, span.x, BAR_HEIGHT + MENU_GAP, |s| f.map(|f| f.measure(s, TEXT_PX, Weight::Regular)).unwrap_or(7.0 * s.chars().count() as f32));
        // Keep it on screen.
        let over = d.x + d.w - self.width as f32;
        if over > 0.0 && self.width > 0 {
            d.x = (d.x - over).max(0.0);
        }
        Some(d)
    }

    fn panel_origin(&self) -> (f32, f32) {
        let right = self.tray().iter().find(|(k, _)| *k == Tray::Control).map(|(_, s)| s.x + s.w).unwrap_or(self.width as f32);
        ((right - controlcenter::PANEL_W).max(0.0), BAR_HEIGHT + PANEL_GAP)
    }

    fn open_bottom(&self) -> Option<f32> {
        match self.open {
            Open::None => None,
            Open::Menu(i) => self.dropdown(i).map(|d| d.y + d.h),
            Open::Control => Some(BAR_HEIGHT + PANEL_GAP + self.cc_height),
        }
    }

    // --------------------------------------------------------- data workers

    fn spawn_snapshot(&self, full: bool) {
        if self.shared.snap_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let prev = shared.snap.lock().map(|s| s.clone()).unwrap_or_default();
            let next = if full { Snapshot::collect() } else { collect_light(&prev) };
            if let Ok(mut s) = shared.snap.lock() {
                *s = next;
            }
            shared.changed.store(true, Ordering::SeqCst);
            shared.snap_busy.store(false, Ordering::SeqCst);
            if let Some(w) = waker {
                w.wake();
            }
        });
    }

    fn spawn_weather(&self) {
        if self.shared.weather_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            if let Some(w) = weather::fetch() {
                if let Ok(mut slot) = shared.weather.lock() {
                    *slot = Some(w);
                }
                shared.changed.store(true, Ordering::SeqCst);
            }
            shared.weather_busy.store(false, Ordering::SeqCst);
            if let Some(w) = waker {
                w.wake();
            }
        });
    }

    fn weather_stale(&self) -> bool {
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
        was
    }

    fn open_menu(&mut self, idx: usize) {
        self.open = Open::Menu(idx);
        self.hover_row = None;
        self.confirm = None;
    }

    fn open_control(&mut self) {
        let snap = self.snapshot();
        let (items, h) = controlcenter::layout(&snap);
        self.cc_items = items;
        self.cc_height = h;
        self.open = Open::Control;
        self.last_full_refresh = Instant::now();
        self.spawn_snapshot(true);
    }

    fn relayout_control(&mut self) {
        let snap = self.snapshot();
        let (items, h) = controlcenter::layout(&snap);
        self.cc_items = items;
        self.cc_height = h;
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
            eprintln!("macbar: could not run action {}", it.action);
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
                        sys::spawn(&["sh", "-c", &format!("sleep 0.15; {tool}")]);
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
                sys::toggle_stay_awake();
                self.update_snapshot(|s| s.awake = !s.awake);
            }
            Target::Screenshot => {
                self.close();
                let cmd = if sys::have("omarchy") { "omarchy capture screenshot" } else { "grim -g \"$(slurp)\" \"$HOME/screenshot-$(date +%s).png\"" };
                sys::spawn(&["sh", "-c", &format!("sleep 0.15; {cmd}")]);
            }
            Target::Record => {
                self.close();
                if sys::have("omarchy") {
                    sys::spawn(&["sh", "-c", "sleep 0.15; omarchy capture screenrecording"]);
                }
            }
            Target::Settings => {
                self.close();
                if sys::have("omarchy-menu") {
                    sys::spawn(&["sh", "-c", "sleep 0.15; omarchy-menu toggle setup"]);
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
        let cmds: [&[&str]; 4] = [&["omarchy-menu", "toggle"], &["rofi", "-show", "drun"], &["wofi", "--show", "drun"], &["fuzzel"]];
        for c in cmds {
            if sys::have(c[0]) {
                sys::spawn(c);
                return;
            }
        }
    }

    // ---------------------------------------------------------------- draw

    fn text(&mut self, pm: &mut Pixmap, s: &str, x: f32, px: f32, weight: Weight, rgba: (u8, u8, u8, f32)) -> f32 {
        let Some(f) = self.fonts.as_mut() else { return 0.0 };
        let top = BASELINE - f.ascent(px, weight);
        f.draw(pm, s, x, top, px, weight, color(rgba.0, rgba.1, rgba.2, rgba.3))
    }

    fn symbol(&mut self, pm: &mut Pixmap, name: &str, size: f32, cx: f32, cy: f32) {
        let px = (size * 2.0).ceil() as u32;
        if let Some(sym) = self.symbols.get(name, INK, px) {
            crate::gfx::blit_scaled(pm, sym, (cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size, 1.0);
        }
    }

    fn draw_bar(&mut self, pm: &mut Pixmap) {
        let w = pm.width() as f32;
        let (r, g, b) = BAR_BG;
        crate::gfx::fill_rect(pm, 0.0, 0.0, w, BAR_HEIGHT, color(r, g, b, 1.0));

        // Left: titles.
        let spans = self.spans();
        let menus = self.menus.clone();
        for (i, (m, s)) in menus.iter().zip(&spans).enumerate() {
            let active = self.open == Open::Menu(i);
            let hovered = self.hover_title == Some(i);
            let alpha = if active {
                0.18
            } else if hovered {
                0.08
            } else {
                0.0
            };
            if alpha > 0.0 {
                fill_rounded(pm, s.x, 3.0, s.w, BAR_HEIGHT - 6.0, 5.0, color(255, 255, 255, alpha));
            }
            if m.icon {
                self.symbol(pm, "apple", 12.0, s.x + s.w / 2.0, 11.5);
            } else {
                let weight = if m.bold { Weight::Bold } else { Weight::Medium };
                self.text(pm, &m.title, s.x + 9.0, TEXT_PX, weight, (INK.0, INK.1, INK.2, 1.0));
            }
        }

        // Right: tray.
        let snap = self.snapshot();
        let weather = self.shared.weather.lock().ok().and_then(|w| w.clone());
        for (kind, s) in self.tray() {
            let cx = s.x + s.w / 2.0;
            match kind {
                Tray::Weather => {
                    if let Some(wx) = &weather {
                        self.symbol(pm, weather::symbol_for(wx.code), ICON_PX, s.x + 9.0 + ICON_PX / 2.0, 12.5);
                        self.text(pm, &format!("{}°", wx.temp_c), s.x + 9.0 + ICON_PX + 6.0, TEXT_PX, Weight::Regular, (INK.0, INK.1, INK.2, 1.0));
                    }
                }
                Tray::Awake => self.symbol(pm, "cup", 11.0, cx, 12.5),
                Tray::Bluetooth => self.symbol(pm, "bluetooth-connected", ICON_PX, cx, 12.5),
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
                Tray::Spotlight => self.symbol(pm, "magnifyingglass", ICON_PX, cx, 12.5),
                Tray::Control => {
                    if self.open == Open::Control {
                        fill_rounded(pm, s.x, 3.0, s.w, BAR_HEIGHT - 6.0, 5.0, color(255, 255, 255, 0.18));
                    }
                    self.symbol(pm, "switches", ICON_PX, cx, 12.5);
                }
                Tray::Clock => {
                    let t = self.clock_text.clone();
                    self.text(pm, &t, s.x + 8.0, TEXT_PX, Weight::Regular, (INK.0, INK.1, INK.2, 1.0));
                }
            }
        }
    }

    fn draw_dropdown(&mut self, pm: &mut Pixmap, idx: usize) {
        let Some(d) = self.dropdown(idx) else { return };
        let menu = self.menus[idx].clone();
        fill_rounded(pm, d.x, d.y, d.w, d.h, 10.0, color(36, 36, 41, 0.96));
        stroke_rounded(pm, d.x + 0.5, d.y + 0.5, d.w - 1.0, d.h - 1.0, 9.5, color(255, 255, 255, 0.14), 1.0);
        for row in &d.rows {
            let it = &menu.items[row.index];
            if it.separator {
                crate::gfx::fill_rect(pm, d.x + PAD_SEP, row.y + row.h / 2.0, d.w - 2.0 * PAD_SEP, 1.0, color(255, 255, 255, 0.12));
                continue;
            }
            let available = self.runner.available(it.action);
            let hovered = self.hover_row == Some(row.index) && available;
            if hovered {
                fill_rounded(pm, d.x + menus::PAD, row.y, d.w - 2.0 * menus::PAD, row.h, 5.0, color(10, 132, 255, 1.0));
            }
            let alpha = if available { 1.0 } else { 0.35 };
            let (label, red) = if self.confirm == Some(it.action) { (format!("{}?", it.label.trim_end_matches('…')), true) } else { (it.label.clone(), false) };
            let main = if hovered {
                (255, 255, 255, 1.0)
            } else if red {
                (255, 105, 97, 1.0)
            } else {
                (245, 245, 247, alpha)
            };
            let sub = if hovered { (255, 255, 255, 1.0) } else { (255, 255, 255, 0.45 * alpha) };
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

/// A cheap refresh: network, volume, stay-awake, appearance; Bluetooth only if it was seen before.
fn collect_light(prev: &Snapshot) -> Snapshot {
    let mut s = prev.clone();
    s.net = sys::probe_net();
    if let Some((v, name)) = sys::probe_volume() {
        s.volume = Some(v);
        s.sink = name;
    }
    s.awake = sys::stay_awake();
    s.light = sys::is_light();
    if prev.bluetooth.is_some() {
        s.bluetooth = sys::probe_bluetooth();
    }
    s
}

impl App for MacBar {
    fn size(&self) -> (u32, u32) {
        (0, surface_height(self.open_bottom()))
    }

    fn resized(&mut self, w: u32, h: u32) {
        self.width = w;
        self.height = h;
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        crate::gfx::clear(pm);
        self.width = pm.width();
        self.draw_bar(pm);
        match self.open {
            Open::Menu(i) => self.draw_dropdown(pm, i),
            Open::Control => {
                let (ox, oy) = self.panel_origin();
                if let Some(f) = self.fonts.as_mut() {
                    let view = controlcenter::View { origin: (ox, oy), items: &self.cc_items, height: self.cc_height, hover: self.cc_hover };
                    controlcenter::draw(pm, &view, f, &mut self.symbols);
                }
            }
            Open::None => {}
        }
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> bool {
        let mut redraw = false;
        // Dragging a slider keeps tracking even outside its rectangle.
        if let (Some(t), Open::Control) = (self.cc_drag, self.open) {
            let (ox, _) = self.panel_origin();
            if let Some(it) = self.cc_items.iter().find(|i| i.target == Some(t)) {
                let (rx, rw) = (it.r.x, it.r.w);
                self.slide(t, rx, rw, x - ox, false);
                return true;
            }
        }
        let title = if y < BAR_HEIGHT { self.spans().iter().position(|s| x >= s.x && x < s.x + s.w) } else { None };
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
                let (ox, oy) = self.panel_origin();
                let hover = controlcenter::hit(&self.cc_items, x - ox, y - oy).map(|h| h.0);
                if hover != self.cc_hover {
                    self.cc_hover = hover;
                    redraw = true;
                }
            }
            Open::None => {}
        }
        redraw
    }

    fn pointer_leave(&mut self) -> bool {
        let mut redraw = self.hover_title.take().is_some();
        if self.cc_drag.is_some() {
            return redraw;
        }
        redraw |= self.close();
        redraw
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        if button != Button::Left {
            return false;
        }
        if y < BAR_HEIGHT {
            if let Some(i) = self.spans().iter().position(|s| x >= s.x && x < s.x + s.w) {
                if self.open == Open::Menu(i) {
                    self.close();
                } else {
                    self.close();
                    self.open_menu(i);
                }
                return true;
            }
            for (kind, s) in self.tray() {
                if x < s.x || x >= s.x + s.w {
                    continue;
                }
                match kind {
                    Tray::Control => {
                        if self.open == Open::Control {
                            self.close();
                        } else {
                            self.close();
                            self.open_control();
                        }
                        return true;
                    }
                    Tray::Spotlight => {
                        self.close();
                        self.spotlight();
                        return true;
                    }
                    Tray::Weather => {
                        self.spawn_weather();
                        return false;
                    }
                    _ => {}
                }
            }
            return self.close();
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
                let (ox, oy) = self.panel_origin();
                if let Some((t, r)) = controlcenter::hit(&self.cc_items, x - ox, y - oy) {
                    self.activate(t, r.x, r.w, x - ox);
                    return true;
                }
                false
            }
            Open::None => false,
        }
    }

    fn pointer_release(&mut self, x: f32, _y: f32, button: Button) -> bool {
        if button != Button::Left {
            return false;
        }
        if let Some(t) = self.cc_drag.take() {
            let (ox, _) = self.panel_origin();
            if let Some(it) = self.cc_items.iter().find(|i| i.target == Some(t)) {
                let (rx, rw) = (it.r.x, it.r.w);
                self.slide(t, rx, rw, x - ox, true);
            }
            return true;
        }
        false
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.open != Open::Control {
            return false;
        }
        let (ox, oy) = self.panel_origin();
        let Some((t @ (Target::Brightness | Target::Volume), r)) = controlcenter::hit(&self.cc_items, x - ox, y - oy) else { return false };
        let snap = self.snapshot();
        let cur = match t {
            Target::Brightness => snap.brightness.unwrap_or(0) as f32 / 100.0,
            _ => snap.volume.map(|v| v.level).unwrap_or(0.0),
        };
        let next = (cur - dy.signum() * 0.05).clamp(0.0, 1.0);
        self.slide(t, r.x, r.w, r.x + next * r.w, true);
        true
    }

    fn key(&mut self, key: Key) -> bool {
        match key {
            Key::Escape => self.close(),
        }
    }

    fn wants_keyboard(&self) -> bool {
        self.open != Open::None
    }

    fn input_rects(&self) -> Vec<InputRect> {
        let w = self.width.max(1) as i32;
        let mut rects = vec![InputRect { x: 0, y: 0, w, h: BAR_HEIGHT as i32 }];
        match self.open {
            Open::Menu(i) => {
                if let Some(d) = self.dropdown(i) {
                    rects.push(InputRect { x: d.x.floor() as i32, y: d.y.floor() as i32, w: d.w.ceil() as i32, h: d.h.ceil() as i32 });
                }
            }
            Open::Control => {
                let (ox, oy) = self.panel_origin();
                rects.push(InputRect { x: ox.floor() as i32, y: oy.floor() as i32, w: controlcenter::PANEL_W as i32, h: self.cc_height.ceil() as i32 });
            }
            Open::None => {}
        }
        rects
    }

    fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
        // First data: weather (cached or fetched) and a light system snapshot.
        if self.weather_stale() {
            self.spawn_weather();
        }
        self.spawn_snapshot(false);
    }

    fn tick(&mut self) -> Tick {
        let mut redraw = self.shared.changed.swap(false, Ordering::SeqCst);
        if redraw && self.open == Open::Control && self.cc_drag.is_none() {
            self.relayout_control();
        }
        let Some(tm) = clock::now() else { return Tick { redraw, next: Some(Duration::from_secs(30)) } };
        let minute = (tm.hour, tm.min);
        if self.last_minute != Some(minute) {
            self.last_minute = Some(minute);
            self.clock_text = clock::format(&tm);
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
        if self.open == Open::Control && self.cc_drag.is_none() && self.last_full_refresh.elapsed() >= PANEL_REFRESH {
            self.last_full_refresh = Instant::now();
            self.spawn_snapshot(true);
        }
        Tick { redraw, next: Some(next_wake(&tm, self.open == Open::Control)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menus() -> Vec<Menu> {
        menus::build("Foot", false)
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
    fn surface_is_26px_when_closed_and_grows_when_open() {
        assert_eq!(surface_height(None), 26);
        assert_eq!(surface_height(Some(300.0)), 308);
        assert!(surface_height(Some(100.5)) >= 109);
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
        let mut bar = MacBar::new(Box::new(Fixed("definitely-not-installed.app")));
        assert!(bar.menus[1].bold);
        assert_eq!(bar.app_name, "App");
        assert!(!bar.refresh_app_name(), "unchanged name needs no rebuild");
    }

    #[test]
    fn default_active_window_is_finder() {
        let bar = MacBar::new(Box::new(active::NoActiveWindow));
        assert_eq!(bar.app_name, "Finder");
    }

    #[test]
    fn closed_bar_only_takes_input_on_the_strip() {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let r = bar.input_rects();
        assert_eq!(r, vec![InputRect { x: 0, y: 0, w: 1920, h: 26 }]);
        assert!(!bar.wants_keyboard());
    }

    #[test]
    fn clicking_a_title_opens_its_menu_and_escape_closes_it() {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let span = bar.spans()[2];
        assert!(bar.pointer_button(span.x + 5.0, 10.0, Button::Left));
        assert_eq!(bar.open, Open::Menu(2));
        assert!(bar.wants_keyboard());
        assert!(bar.size().1 > 26);
        assert_eq!(bar.input_rects().len(), 2);
        assert!(bar.key(Key::Escape));
        assert_eq!(bar.open, Open::None);
        assert_eq!(bar.size().1, 26);
    }

    #[test]
    fn hovering_another_title_switches_menus_only_while_open() {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
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
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let s = bar.spans()[4];
        bar.pointer_button(s.x + 2.0, 10.0, Button::Left);
        bar.pointer_button(s.x + 2.0, 10.0, Button::Left);
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn leaving_the_surface_closes_a_menu() {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let s = bar.spans()[2];
        bar.pointer_button(s.x + 2.0, 10.0, Button::Left);
        assert!(bar.pointer_leave());
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn dropdown_stays_on_screen() {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 300;
        let d = bar.dropdown(6).unwrap();
        assert!(d.x + d.w <= 300.0 + 0.01 || d.x == 0.0);
    }

    #[test]
    fn right_clicks_do_nothing() {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        let s = bar.spans()[2];
        assert!(!bar.pointer_button(s.x + 2.0, 10.0, Button::Right));
        assert_eq!(bar.open, Open::None);
    }

    #[test]
    fn control_center_opens_from_its_button_and_tracks_input() {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
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

    static LOG_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn control_bar() -> MacBar {
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.update_snapshot(|s| {
            *s = Snapshot {
                net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: String::new() }),
                volume: Some(sys::Volume { level: 0.5, muted: false }),
                brightness: Some(40),
                night_shift: Some(false),
                dnd: Some(false),
                ..Snapshot::default()
            }
        });
        bar.relayout_control();
        bar.open = Open::Control;
        bar
    }

    fn centre_of(bar: &MacBar, t: Target) -> (f32, f32, controlcenter::R) {
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
        assert!(bar.pointer_leave());
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
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
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
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = 1920;
        bar.runner = Runner::without_helper();
        let file = bar.spans()[2];
        bar.pointer_button(file.x + 3.0, 10.0, Button::Left);
        let d = bar.dropdown(2).unwrap();
        // "New Window" has no portable fallback, so the click does nothing and the menu stays.
        assert!(!bar.pointer_button(d.x + 20.0, d.rows[0].y + 5.0, Button::Left));
        assert_eq!(bar.open, Open::Menu(2));
    }
}
