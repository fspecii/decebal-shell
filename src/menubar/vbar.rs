//! The vertical bar: `bar_style = "vertical"` (the Caelestia theme), a bar down the left screen edge.
//!
//! Top to bottom: OS icon (opens the launcher), workspace pills (rdwm tags), the focused window (icon and
//! rotated title), a tray group (layout, stay-awake, agents, quick settings), status icons (lock keys, sound,
//! Wi-Fi / Ethernet, Bluetooth, battery), the clock stacked as `HH` over `MM` (opens the dashboard) and the power
//! button (opens the session menu). Hovering a status icon (or the window) opens the same popup the horizontal
//! bar opens on a click, to the right of the bar; the data comes from the same snapshot and worker threads.
//!
//! The surface is [`SURFACE_W`] wide and as tall as the monitor, but only [`BAR_W`] of it is reserved (the strut)
//! and takes input while nothing is open, exactly like the horizontal bar whose surface is taller than its strip.
//!
//! [`layout`] is pure: it turns the screen height and what exists on this machine into rectangles, which drawing,
//! hit-testing and the tests share.

use super::popup::Popup;
use super::*;
use crate::menubar::rdwm::TagState;
use tiny_skia::{FilterQuality, PixmapPaint, Transform};

/// Width reserved at the screen edge (the strut) and the width of the bar strip.
pub const BAR_W: f32 = 52.0;
/// Room right of the strip for popups and the quick-settings panel (widest: the Control Center, 340 px).
pub const POPOUT_W: f32 = 392.0;
pub const SURFACE_W: u32 = (BAR_W + POPOUT_W) as u32;
const PILL_X: f32 = 6.0;
pub const PILL_W: f32 = BAR_W - 2.0 * PILL_X;
/// Space above the first and below the last pill, and between pills.
const EDGE: f32 = 8.0;
const GAP: f32 = 8.0;
pub const LOGO_H: f32 = PILL_W;
pub const POWER_H: f32 = PILL_W;
pub const CLOCK_H: f32 = 58.0;
/// One workspace cell and the padding above / below the cells inside their pill.
pub const WS_CELL: f32 = 26.0;
const WS_PAD: f32 = 6.0;
/// One icon cell in the tray and status pills (the battery also carries its percentage).
pub const ICON_CELL: f32 = 30.0;
pub const BATT_CELL: f32 = 44.0;
const CELL_PAD: f32 = 6.0;
/// The window pill is left out when less than this is free for it.
const WIN_MIN: f32 = 72.0;
/// Longest title run drawn along the window pill.
const WIN_TEXT_MAX: f32 = 200.0;
const WIN_ICON: f32 = 22.0;
/// Gap between the strip and the popup opened from it.
const POP_GAP: f32 = 6.0;
const POP_MARGIN: f32 = 8.0;
/// Pointer rest time on a status icon before its popup opens (moving between icons switches at once).
pub const HOVER_DELAY: Duration = Duration::from_millis(140);

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl VRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// The rounded containers, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pill {
    Logo,
    Workspaces,
    Window,
    Tray,
    Status,
    Clock,
    Power,
}

/// What the bar shows on this machine right now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Content {
    pub workspaces: usize,
    /// Height the window pill would like; 0 = no focused window.
    pub window: f32,
    pub tray: Vec<Tray>,
    pub status: Vec<Tray>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub pills: Vec<(Pill, VRect)>,
    /// Hit targets: the single-item pills as a whole, the icons of the tray and status pills one by one.
    pub items: Vec<(Tray, VRect)>,
}

impl Layout {
    pub fn hit(&self, x: f32, y: f32) -> Option<(Tray, VRect)> {
        self.items.iter().find(|(_, r)| r.contains(x, y)).copied()
    }
    pub fn item(&self, kind: Tray) -> Option<VRect> {
        self.items.iter().find(|(k, _)| *k == kind).map(|(_, r)| *r)
    }
    pub fn pill(&self, which: Pill) -> Option<VRect> {
        self.pills.iter().find(|(p, _)| *p == which).map(|(_, r)| *r)
    }
}

fn cell_h(kind: Tray) -> f32 {
    if kind == Tray::Battery {
        BATT_CELL
    } else {
        ICON_CELL
    }
}

pub fn group_h(kinds: &[Tray]) -> f32 {
    kinds.iter().map(|k| cell_h(*k)).sum::<f32>() + 2.0 * CELL_PAD
}

pub fn workspaces_h(count: usize) -> f32 {
    count as f32 * WS_CELL + 2.0 * WS_PAD
}

/// Lays the bar out on a screen `height` tall. The logo and workspaces hang from the top, the tray, status icons,
/// clock and power button stand on the bottom, and the focused window takes the room between (centred in it),
/// or is left out when there is less than [`WIN_MIN`].
pub fn layout(height: f32, c: &Content) -> Layout {
    let mut out = Layout::default();
    let mut top = EDGE;
    let logo = VRect { x: PILL_X, y: top, w: PILL_W, h: LOGO_H };
    out.pills.push((Pill::Logo, logo));
    out.items.push((Tray::Logo, logo));
    top += LOGO_H + GAP;
    if c.workspaces > 0 {
        let r = VRect { x: PILL_X, y: top, w: PILL_W, h: workspaces_h(c.workspaces) };
        out.pills.push((Pill::Workspaces, r));
        out.items.push((Tray::Workspaces, r));
        top += r.h + GAP;
    }

    // The bottom stack, built from the screen edge up and then put in reading order.
    let mut bot = height - EDGE;
    let mut bottom: Vec<(Pill, VRect)> = Vec::new();
    let mut bottom_items: Vec<(Tray, VRect)> = Vec::new();
    let mut stand = |pill: Pill, h: f32, bot: &mut f32| -> VRect {
        let r = VRect { x: PILL_X, y: *bot - h, w: PILL_W, h };
        bottom.push((pill, r));
        *bot = r.y - GAP;
        r
    };
    let power = stand(Pill::Power, POWER_H, &mut bot);
    bottom_items.push((Tray::Power, power));
    let clock = stand(Pill::Clock, CLOCK_H, &mut bot);
    bottom_items.push((Tray::Clock, clock));
    for (pill, kinds) in [(Pill::Status, &c.status), (Pill::Tray, &c.tray)] {
        if kinds.is_empty() {
            continue;
        }
        let r = stand(pill, group_h(kinds), &mut bot);
        let mut y = r.y + CELL_PAD;
        for k in kinds {
            let cell = VRect { x: PILL_X, y, w: PILL_W, h: cell_h(*k) };
            bottom_items.push((*k, cell));
            y += cell.h;
        }
    }
    bottom.reverse();
    // `bot` is now the free space's lower boundary (it already includes the gap above the topmost bottom pill).
    let avail = bot - top;
    if c.window > 0.0 && avail >= WIN_MIN {
        let h = c.window.min(avail);
        let r = VRect { x: PILL_X, y: top + (avail - h) / 2.0, w: PILL_W, h };
        out.pills.push((Pill::Window, r));
        out.items.push((Tray::Window, r));
    }
    out.pills.extend(bottom);
    bottom_items.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    out.items.extend(bottom_items);
    out
}

/// Which workspace cell (0-based) lies under `y` inside the workspace pill `pill`.
pub fn workspace_hit(pill: VRect, count: usize, y: f32) -> Option<usize> {
    let rel = y - (pill.y + WS_PAD);
    if rel < 0.0 || y >= pill.bottom() - WS_PAD {
        return None;
    }
    let i = (rel / WS_CELL) as usize;
    (i < count).then_some(i)
}

/// What a left click on `kind` does in the vertical bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// The OS icon: the launcher (macspot).
    Launcher,
    /// A workspace pill: view it.
    Workspace(usize),
    /// A status icon or the window: open its popup (and keep it open).
    Popout(Tray),
    /// The quick-settings tile: toggle the Control Center panel.
    Control,
    /// The clock: the dashboard (macdash).
    Dashboard,
    /// The power button: the session menu (macsession).
    Session,
    Nothing,
}

pub fn click_for(kind: Tray, workspace: Option<usize>) -> Click {
    match kind {
        Tray::Logo => Click::Launcher,
        Tray::Workspaces => workspace.map_or(Click::Nothing, Click::Workspace),
        Tray::Clock => Click::Dashboard,
        Tray::Power => Click::Session,
        Tray::Control => Click::Control,
        k if popout(k) => Click::Popout(k),
        _ => Click::Nothing,
    }
}

/// Items that open a popup when hovered or clicked.
pub fn popout(kind: Tray) -> bool {
    matches!(kind, Tray::Audio | Tray::Network | Tray::Bluetooth | Tray::Battery | Tray::Window | Tray::Layout)
}

/// The rdwm request that views workspace `index` (0-based; rdwm tags are numbered from 1).
pub fn view_request(index: usize) -> String {
    super::rdwm::action_request("view", &[&(index + 1).to_string()])
}

/// The 2-digit stacked clock: (`HH`, `MM`), 12-hour clocks show 01..12.
pub fn clock_parts(hour: u32, min: u32, hour12: bool) -> (String, String) {
    let h = if hour12 {
        match hour % 12 {
            0 => 12,
            h => h,
        }
    } else {
        hour % 24
    };
    (format!("{h:02}"), format!("{:02}", min % 60))
}

/// Nerd Font code point of the distribution logo for `os-release` fields (`ID`, then `ID_LIKE` words).
pub fn os_logo(ids: &[&str]) -> char {
    for id in ids {
        let c = match *id {
            "arch" | "archlinux" | "omarchy" => '\u{f303}',
            "endeavouros" => '\u{f322}',
            "manjaro" => '\u{f312}',
            "ubuntu" => '\u{f31b}',
            "debian" => '\u{f306}',
            "fedora" => '\u{f30a}',
            "nixos" => '\u{f313}',
            "gentoo" => '\u{f30d}',
            "alpine" => '\u{f300}',
            "void" => '\u{f32e}',
            "linuxmint" => '\u{f30e}',
            "pop" | "pop-os" => '\u{f32a}',
            "kali" => '\u{f327}',
            "centos" => '\u{f304}',
            "rhel" => '\u{f316}',
            "opensuse" | "opensuse-tumbleweed" | "opensuse-leap" | "suse" => '\u{f314}',
            _ => continue,
        };
        return c;
    }
    '\u{f17c}'
}

fn os_release_ids() -> Vec<String> {
    let text = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let mut ids = Vec::new();
    for key in ["ID", "ID_LIKE"] {
        for line in text.lines() {
            if let Some(v) = line.strip_prefix(&format!("{key}=")) {
                ids.extend(v.trim_matches(|c| c == '"' || c == '\'').split_whitespace().map(str::to_string));
            }
        }
    }
    ids
}

/// Symbol name of the OS icon: the distribution's Nerd Font logo, else the grid.
pub fn logo_symbol() -> String {
    if super::symbols::agents_available() {
        let ids = os_release_ids();
        format!("nerd:{:x}", os_logo(&ids.iter().map(String::as_str).collect::<Vec<_>>()) as u32)
    } else {
        "grid".to_string()
    }
}

impl MacBar {
    /// True when this bar draws the vertical layout (the theme asks for it; never for an embedded status engine).
    pub(super) fn vertical(&self) -> bool {
        if self.embedded {
            return false;
        }
        #[cfg(test)]
        return TEST_VERTICAL.with(|f| f.get());
        #[cfg(not(test))]
        crate::theme::th().bar_vertical()
    }

    /// Starts the Caps / Num Lock watcher once, when the vertical bar is (about to be) on screen and a waker exists.
    pub(super) fn ensure_lock_watcher(&mut self) {
        if self.lock_watcher || self.embedded || !self.vertical() {
            return;
        }
        let Some(waker) = self.waker.clone() else { return };
        self.lock_watcher = true;
        let changed = Arc::clone(&self.shared);
        lock::spawn_watcher(self.locks.clone(), move || {
            changed.changed.store(true, Ordering::SeqCst);
            waker.wake();
        });
    }

    /// The workspace pills: rdwm's tags of the selected monitor when it reports them, else the EWMH desktops.
    pub(super) fn v_workspaces(&self) -> Vec<Workspace> {
        if !self.show_workspaces {
            return Vec::new();
        }
        match self.layout_state().map(|l| l.tags).filter(|t| !t.is_empty()) {
            Some(tags) => tags.into_iter().map(|TagState { name, active, occupied, urgent }| Workspace { name, active, occupied, urgent }).collect(),
            None => self.workspaces.clone(),
        }
    }

    fn v_content(&self) -> Content {
        let snap = self.snapshot();
        let mut tray = Vec::new();
        if self.layout_state().is_some() {
            tray.push(Tray::Layout);
        }
        if snap.awake {
            tray.push(Tray::Awake);
        }
        if symbols::agents_available() {
            tray.push(Tray::Agents);
        }
        tray.push(Tray::Control);
        let mut status = Vec::new();
        if self.locks.caps() || self.locks.num() {
            status.push(Tray::Lock);
        }
        if snap.volume.is_some() {
            status.push(Tray::Audio);
        }
        if snap.net.is_some() {
            status.push(Tray::Network);
        }
        if snap.bluetooth.is_some() {
            status.push(Tray::Bluetooth);
        }
        if snap.battery.is_some() {
            status.push(Tray::Battery);
        }
        let window = if self.v_win.is_some() { (2.0 * 8.0 + WIN_ICON + 8.0 + self.v_win_len.min(WIN_TEXT_MAX)).ceil() } else { 0.0 };
        Content { workspaces: self.v_workspaces().len(), window, tray, status }
    }

    pub(super) fn v_layout(&self) -> Layout {
        layout(self.height as f32, &self.v_content())
    }

    /// Re-reads the focused window (app id and title) and measures the title; true when something changed.
    pub(super) fn v_refresh_window(&mut self) -> bool {
        let id = self.active.active_app_name();
        let new = id.map(|id| {
            let title = self.active.active_title().unwrap_or_default();
            (id, title)
        });
        if new == self.v_win {
            return false;
        }
        let shown = new.as_ref().map(|(id, t)| if t.is_empty() { active::display_name(id) } else { t.clone() });
        self.v_win_len = match (&shown, &self.fonts) {
            (Some(s), Some(f)) => f.measure(s, TEXT_PX, Weight::Medium),
            _ => 0.0,
        };
        self.v_win = new;
        true
    }

    /// Title run of the window pill: the title, else the app's display name.
    fn v_window_text(&self) -> String {
        match &self.v_win {
            Some((id, t)) if t.is_empty() => active::display_name(id),
            Some((_, t)) => t.clone(),
            None => String::new(),
        }
    }

    // ------------------------------------------------------------ geometry for the backend / input

    pub(super) fn v_spec() -> crate::backend::SurfaceSpec {
        crate::backend::SurfaceSpec { anchor: crate::backend::Anchor::Left, margin: 0, reserve: BAR_W as i32, stretch: true, namespace: "macshell-bar" }
    }

    /// Top-left of the open Control Center panel: right of the strip, centred on its tile, kept on screen.
    pub(super) fn v_panel_origin(&self) -> (f32, f32) {
        let cy = self.v_layout().item(Tray::Control).map_or(self.height as f32 / 2.0, |r| r.cy());
        (BAR_W + POP_GAP, clamp_y(cy - self.cc_height / 2.0, self.cc_height, self.height as f32))
    }

    pub(super) fn v_input_rects(&self) -> Vec<InputRect> {
        let h = self.height.max(1) as i32;
        let mut rects = vec![InputRect { x: 0, y: 0, w: BAR_W as i32, h }];
        match self.open {
            Open::Control => {
                let (ox, oy) = self.v_panel_origin();
                // starts at the strip so the pointer never leaves the surface on its way over
                rects.push(InputRect { x: BAR_W as i32, y: oy.floor() as i32, w: (ox + controlcenter::PANEL_W - BAR_W).ceil() as i32, h: self.cc_height.ceil() as i32 });
            }
            Open::Popup(_) => {
                if let Some(p) = &self.popup {
                    rects.push(InputRect { x: BAR_W as i32, y: p.y.floor() as i32, w: (p.x + p.w - BAR_W).ceil() as i32, h: p.h.ceil() as i32 });
                }
            }
            Open::Menu(_) | Open::None => {}
        }
        rects
    }

    /// Opens the popup of `kind` to the right of its icon (or window pill), vertically centred on it.
    pub(super) fn v_open_popup(&mut self, kind: Tray) {
        self.close();
        let Some(item) = self.v_layout().item(kind) else { return };
        let Some((rows, w, compact)) = self.build_popup(kind) else { return };
        let mut p = Popup::layout(rows, BAR_W + POP_GAP, 0.0, w, compact);
        p.y = clamp_y(item.cy() - p.h / 2.0, p.h, self.height as f32);
        self.popup = Some(p);
        self.open = Open::Popup(kind);
        self.last_full_refresh = Instant::now();
        match kind {
            Tray::Audio => self.spawn_snapshot(true),
            Tray::Network | Tray::Bluetooth => self.spawn_lists(kind),
            Tray::Battery => self.spawn_snapshot(false),
            _ => {}
        }
    }

    /// After a rebuild the popup may have grown: keep it on screen.
    pub(super) fn v_clamp_popup(&mut self) {
        let h = self.height as f32;
        if let Some(p) = self.popup.as_mut() {
            p.y = clamp_y(p.y, p.h, h);
        }
    }

    /// `--png` for the vertical layout: the whole surface (strip and popout room) at the height given by [`MacBar::with_height`].
    /// `open` is `control` or the name of a popup (`audio`, `network`, `bluetooth`, `battery`, `window`, `layout`).
    pub(super) fn v_render_png(&mut self, path: &str, open: Option<&str>) -> Result<(), String> {
        if self.height == 0 {
            self.height = 900;
        }
        self.width = SURFACE_W;
        self.v_refresh_window();
        match open {
            Some("control") => {
                self.open_control();
            }
            Some(name) => {
                let kind = match name {
                    "audio" => Tray::Audio,
                    "network" => Tray::Network,
                    "bluetooth" => Tray::Bluetooth,
                    "battery" => Tray::Battery,
                    "window" => Tray::Window,
                    "layout" => Tray::Layout,
                    other => return Err(format!("unknown popup {other}")),
                };
                self.v_open_popup(kind);
            }
            None => {}
        }
        let mut pm = Pixmap::new(SURFACE_W, self.height.max(1)).ok_or("cannot allocate pixmap")?;
        self.draw(&mut pm);
        pm.save_png(path).map_err(|e| e.to_string())
    }

    // ------------------------------------------------------------------------------- input

    pub(super) fn v_target(&self, x: f32, y: f32) -> Option<(Tray, Option<usize>)> {
        if x < 0.0 || x >= BAR_W {
            return None;
        }
        let lay = self.v_layout();
        let (kind, r) = lay.hit(x, y)?;
        let ws = (kind == Tray::Workspaces).then(|| workspace_hit(r, self.v_workspaces().len(), y)).flatten();
        Some((kind, ws))
    }

    pub(super) fn v_motion(&mut self, x: f32, y: f32) -> bool {
        let mut redraw = false;
        self.leave_at = None;
        if let (Some(t), Open::Control) = (self.cc_drag, self.open) {
            let (ox, _) = self.v_panel_origin();
            if let Some(it) = self.cc_items.iter().find(|i| i.target == Some(t)) {
                let (rx, rw) = (it.r.x, it.r.w);
                self.slide(t, rx, rw, x - ox, false);
                return true;
            }
        }
        if let Some(row) = self.popup_drag {
            self.popup_slide(row, x, false);
            return true;
        }
        if self.open != Open::None {
            if self.over_open_ui(x, y) {
                self.leave_at = None;
            } else if self.leave_at.is_none() {
                self.leave_at = Some(Instant::now());
            }
        }
        let target = self.v_target(x, y);
        let (hover, ws) = (target.map(|t| t.0), target.and_then(|t| t.1));
        if hover != self.v_hover || ws != self.v_hover_ws {
            self.v_hover = hover;
            self.v_hover_ws = ws;
            redraw = true;
            self.v_hover_open = None;
            if let Some(kind) = hover.filter(|k| popout(*k)) {
                match self.open {
                    // sliding from one icon to the next switches at once, like sliding along the macOS menu bar
                    Open::Popup(cur) if cur != kind => {
                        self.v_open_popup(kind);
                    }
                    Open::None => self.v_hover_open = Some((kind, Instant::now())),
                    _ => {}
                }
            } else if hover.is_some() && matches!(self.open, Open::Popup(_)) {
                // onto an item without a popup of its own: the popup was a hover and goes away
                self.close();
            }
        }
        match self.open {
            Open::Control => {
                let (ox, oy) = self.v_panel_origin();
                let hover = controlcenter::hit(&self.cc_items, x - ox, y - oy).map(|h| h.0);
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
            Open::Menu(_) | Open::None => {}
        }
        redraw
    }

    pub(super) fn v_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        self.leave_at = None;
        if self.open != Open::None && !self.over_open_ui(x, y) {
            self.close();
            return true;
        }
        let target = self.v_target(x, y);
        if button == Button::Middle {
            if matches!(target, Some((Tray::Audio, _))) {
                self.toggle_mute_now();
                if matches!(self.open, Open::Popup(Tray::Audio)) {
                    self.rebuild_popup();
                }
                return true;
            }
            return false;
        }
        if button != Button::Left {
            return false;
        }
        if x < BAR_W {
            let Some((kind, ws)) = target else { return self.close() };
            self.v_hover_open = None;
            match click_for(kind, ws) {
                Click::Launcher => {
                    self.close();
                    self.spotlight();
                }
                Click::Workspace(i) => {
                    self.close();
                    self.v_switch_workspace(i);
                }
                Click::Popout(k) => {
                    if self.open != Open::Popup(k) {
                        self.v_open_popup(k);
                    }
                }
                Click::Control => {
                    let was = self.open == Open::Control;
                    self.close();
                    if !was {
                        self.open_control();
                    }
                }
                Click::Dashboard => {
                    self.close();
                    launch_ui(vec![crate::launch::sibling_or_path("macdash"), "--top".into(), "10".into()]);
                }
                Click::Session => {
                    self.close();
                    launch_ui(vec![crate::launch::sibling_or_path("macsession")]);
                }
                Click::Nothing => {
                    return self.close();
                }
            }
            return true;
        }
        match self.open {
            Open::Control => {
                let (ox, oy) = self.v_panel_origin();
                if let Some((t, r)) = controlcenter::hit(&self.cc_items, x - ox, y - oy) {
                    self.activate(t, r.x, r.w, x - ox);
                    return true;
                }
                false
            }
            Open::Popup(_) => self.popup_press(x, y),
            Open::Menu(_) | Open::None => false,
        }
    }

    /// Views workspace `index`: rdwm's `view` over its IPC socket when it reports tags, else EWMH.
    fn v_switch_workspace(&mut self, index: usize) {
        if self.layout_state().is_some_and(|l| !l.tags.is_empty()) {
            rdwm::send_action("view", &[&(index + 1).to_string()]);
        } else {
            self.active.switch_workspace(index);
        }
    }

    /// Wheel on the strip, as in caelestia: over the workspaces it steps through them, elsewhere the upper half of the
    /// screen changes the volume and the lower half the brightness.
    pub(super) fn v_scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if x >= BAR_W {
            return false;
        }
        let target = self.v_target(x, y).map(|t| t.0);
        if target == Some(Tray::Workspaces) {
            rdwm::send_action(if dy > 0.0 { "viewnext" } else { "viewprev" }, &[]);
            return false;
        }
        let up = if dy > 0.0 { -1.0 } else { 1.0 };
        if target == Some(Tray::Audio) || y < self.height as f32 / 2.0 {
            let Some(v) = self.snapshot().volume else { return false };
            let cur = if v.muted { 0.0 } else { v.level };
            self.set_volume_now((cur + up * 0.05).clamp(0.0, 1.0));
            if matches!(self.open, Open::Popup(Tray::Audio)) {
                self.rebuild_popup();
            }
            return true;
        }
        let Some(b) = self.snapshot().brightness else { return false };
        let pct = (b as i32 + (up * 5.0) as i32).clamp(0, 100) as u32;
        self.update_snapshot(|s| s.brightness = Some(pct));
        sys::set_brightness(pct);
        true
    }

    /// Timers of the vertical bar: the hover delay before a popup opens. Returns (redraw, time until the next one).
    pub(super) fn v_tick(&mut self) -> (bool, Option<Duration>) {
        let Some((kind, since)) = self.v_hover_open else { return (false, None) };
        if since.elapsed() < HOVER_DELAY {
            return (false, Some(HOVER_DELAY - since.elapsed()));
        }
        self.v_hover_open = None;
        if self.v_hover == Some(kind) && self.open == Open::None {
            self.v_open_popup(kind);
            return (true, None);
        }
        (false, None)
    }

    // ------------------------------------------------------------------------------ drawing

    #[allow(clippy::too_many_arguments)]
    fn v_text(&mut self, pm: &mut Pixmap, s: &str, cx: f32, cy: f32, px: f32, weight: Weight, rgb: (u8, u8, u8), alpha: f32) {
        let Some(f) = self.fonts.as_mut() else { return };
        let w = f.measure(s, px, weight);
        let top = cy + 0.36 * px * crate::theme::live::font_scale() - f.ascent(px, weight);
        f.draw(pm, s, cx - w / 2.0, top, px, weight, color(rgb.0, rgb.1, rgb.2, alpha));
    }

    fn v_symbol(&mut self, pm: &mut Pixmap, name: &str, size: f32, cx: f32, cy: f32, rgb: (u8, u8, u8)) {
        let px = (size * 2.0).ceil() as u32;
        if let Some(sym) = self.symbols.get(name, rgb, px) {
            crate::gfx::blit_scaled(pm, sym, (cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size, 1.0);
        }
    }

    /// `s` turned 90 degrees clockwise (reading downward), its glyph bodies centred on `cx`, starting at `top`.
    #[allow(clippy::too_many_arguments)]
    fn v_rotated_text(&mut self, pm: &mut Pixmap, s: &str, cx: f32, top: f32, px: f32, weight: Weight, rgb: (u8, u8, u8)) {
        let Some(f) = self.fonts.as_mut() else { return };
        let line = (px * crate::theme::live::font_scale() * 1.5).ceil();
        let Some(mut tmp) = crate::gfx::pixmap(f.measure(s, px, weight).ceil() as u32 + 2, line as u32) else { return };
        f.draw(&mut tmp, s, 0.0, 0.0, px, weight, color(rgb.0, rgb.1, rgb.2, 1.0));
        let body = f.ascent(px, weight) - 0.36 * px * crate::theme::live::font_scale();
        let paint = PixmapPaint { quality: FilterQuality::Nearest, ..PixmapPaint::default() };
        let tx = (cx + body).round();
        pm.draw_pixmap(0, 0, tmp.as_ref(), &paint, Transform::from_row(0.0, 1.0, -1.0, 0.0, tx, top.round()), None);
    }

    pub(super) fn v_draw(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        self.width = pm.width();
        self.height = pm.height();
        let h = pm.height() as f32;
        crate::gfx::fill_rect(pm, 0.0, 0.0, BAR_W, h, th.background.color(th.bar_alpha));
        let lay = self.v_layout();
        let snap = self.snapshot();
        let text = th.text.t();
        let radius = PILL_W / 2.0;
        for (_, r) in &lay.pills {
            fill_rounded(pm, r.x, r.y, r.w, r.h, radius.min(r.h / 2.0), th.bar.color(1.0));
            stroke_rounded(pm, r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, radius.min(r.h / 2.0) - 0.5, th.outline_c(), 1.0);
        }

        for (kind, r) in &lay.items {
            let open = self.open == Open::Popup(*kind) || (*kind == Tray::Control && self.open == Open::Control);
            let hovered = self.v_hover == Some(*kind) && *kind != Tray::Workspaces;
            if open || hovered {
                let a = if open { 0.18 } else { 0.10 };
                let single = matches!(kind, Tray::Logo | Tray::Window | Tray::Clock | Tray::Power);
                if single {
                    fill_rounded(pm, r.x, r.y, r.w, r.h, radius.min(r.h / 2.0), th.ink(a));
                } else {
                    fill_rounded(pm, r.x + 2.0, r.y + 1.0, r.w - 4.0, r.h - 2.0, 12.0, th.ink(a));
                }
            }
            let (cx, cy) = (r.cx(), r.cy());
            match kind {
                Tray::Logo => {
                    let name = self.logo_symbol.clone();
                    self.v_symbol(pm, &name, 22.0, cx, cy, th.accent.t());
                }
                Tray::Workspaces => self.v_draw_workspaces(pm, *r),
                Tray::Window => self.v_draw_window(pm, *r),
                Tray::Layout => {
                    if let Some(l) = self.layout_state() {
                        popup::draw_layout_icon(pm, l.canonical(), cx - 11.0, cy - 7.0, 22.0, 14.0, (text.0, text.1, text.2, 0.95));
                    }
                }
                Tray::Awake => self.v_symbol(pm, "awake", 16.0, cx, cy, th.accent.t()),
                Tray::Agents => self.v_symbol(pm, "agents", 16.0, cx, cy, text),
                Tray::Control => self.v_symbol(pm, "switches", 16.0, cx, cy, text),
                Tray::Lock => {
                    let label = match (self.locks.caps(), self.locks.num()) {
                        (true, true) => "A1",
                        (true, false) => "A",
                        _ => "1",
                    };
                    fill_rounded(pm, cx - 10.0, cy - 8.0, 20.0, 16.0, 5.0, th.accent.color(1.0));
                    self.v_text(pm, label, cx, cy, 10.0, Weight::Bold, th.accent_text.t(), 1.0);
                }
                Tray::Audio => {
                    let icon = snap.volume.as_ref().map(controlcenter::speaker_icon).unwrap_or("speaker-2");
                    self.v_symbol(pm, icon, 17.0, cx, cy, text);
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
                    self.v_symbol(pm, icon, 17.0, cx, cy, text);
                }
                Tray::Bluetooth => {
                    let icon = match &snap.bluetooth {
                        Some(b) if !b.powered => "bluetooth-off",
                        Some(b) if b.connected > 0 => "bluetooth-connected",
                        _ => "bluetooth",
                    };
                    self.v_symbol(pm, icon, 17.0, cx, cy, text);
                }
                Tray::Battery => {
                    if let Some(b) = &snap.battery {
                        self.v_draw_battery(pm, *r, b);
                    }
                }
                Tray::Clock => {
                    let (hh, mm) = self.v_clock();
                    self.v_text(pm, &hh, cx, cy - 11.0, 15.0, Weight::Bold, th.accent.t(), 1.0);
                    self.v_text(pm, &mm, cx, cy + 11.0, 15.0, Weight::Medium, text, 1.0);
                }
                Tray::Power => self.v_symbol(pm, "power", 18.0, cx, cy, th.danger.t()),
                Tray::Weather | Tray::Dash | Tray::Spotlight => {}
            }
        }

        match self.open {
            Open::Control => {
                let (ox, oy) = self.v_panel_origin();
                if let Some(f) = self.fonts.as_mut() {
                    let view = controlcenter::View { origin: (ox, oy), items: &self.cc_items, height: self.cc_height, hover: self.cc_hover };
                    controlcenter::draw(pm, &view, f, &mut self.symbols);
                }
            }
            Open::Popup(_) => {
                if let (Some(f), Some(p)) = (self.fonts.as_mut(), &self.popup) {
                    p.draw(pm, f, &mut self.symbols, self.popup_hover);
                }
            }
            Open::Menu(_) | Open::None => {}
        }
        if let Some((path, last)) = self.debug_tray.as_mut() {
            let text: String = lay.items.iter().map(|(k, r)| format!("{:?} {} {}\n", k, r.cx().round(), r.cy().round())).collect();
            if *last != text {
                let _ = std::fs::write(&*path, &text);
                *last = text;
            }
        }
    }

    /// (`HH`, `MM`) for the stacked clock.
    fn v_clock(&self) -> (String, String) {
        let (h, m) = self.last_minute.unwrap_or((0, 0));
        clock_parts(h, m, crate::config::shell_settings().clock_hour12)
    }

    fn v_draw_workspaces(&mut self, pm: &mut Pixmap, pill: VRect) {
        let th = crate::theme::th();
        let wss = self.v_workspaces();
        for (i, ws) in wss.iter().enumerate() {
            let cy = pill.y + WS_PAD + WS_CELL * (i as f32 + 0.5);
            let cx = pill.cx();
            let label = if ws.name.chars().count() <= 2 && !ws.name.is_empty() { ws.name.clone() } else { (i + 1).to_string() };
            let hovered = self.v_hover_ws == Some(i);
            let (bg, fg): (Option<tiny_skia::Color>, (u8, u8, u8)) = if ws.active {
                (Some(th.accent.color(1.0)), th.accent_text.t())
            } else if ws.urgent {
                (Some(th.danger.color(1.0)), th.danger.readable_on().t())
            } else if ws.occupied {
                (Some(th.ink(if hovered { 0.22 } else { 0.14 })), th.text.t())
            } else {
                (hovered.then(|| th.ink(0.10)), th.text_faint.t())
            };
            if let Some(c) = bg {
                fill_rounded(pm, cx - 15.0, cy - 11.0, 30.0, 22.0, 11.0, c);
            }
            if ws.active || ws.urgent || ws.occupied || hovered {
                self.v_text(pm, &label, cx, cy, 12.0, if ws.active { Weight::Bold } else { Weight::Medium }, fg, 1.0);
            } else {
                crate::gfx::fill_circle(pm, cx, cy, 2.2, th.ink(0.32));
            }
        }
    }

    fn v_draw_window(&mut self, pm: &mut Pixmap, r: VRect) {
        let Some((id, _)) = self.v_win.clone() else { return };
        let th = crate::theme::th();
        let icon = self.v_icons.get(&id, (WIN_ICON * 2.0) as u32).clone();
        crate::gfx::blit_scaled(pm, &icon, r.cx() - WIN_ICON / 2.0, r.y + 8.0, WIN_ICON, WIN_ICON, 1.0);
        let top = r.y + 8.0 + WIN_ICON + 8.0;
        let room = (r.bottom() - 8.0 - top).max(0.0);
        let shown = match &self.fonts {
            Some(f) => controlcenter::fit(&self.v_window_text(), room, TEXT_PX, Weight::Medium, f),
            None => return,
        };
        self.v_rotated_text(pm, &shown, r.cx(), top, TEXT_PX, Weight::Medium, th.text.t());
    }

    fn v_draw_battery(&mut self, pm: &mut Pixmap, r: VRect, b: &sys::Battery) {
        let th = crate::theme::th();
        let (cx, top) = (r.cx(), r.y + 4.0);
        let (bw, bh) = (12.0, 20.0);
        let body = th.text.color(0.75);
        stroke_rounded(pm, cx - bw / 2.0 + 0.5, top + 2.5, bw - 1.0, bh - 1.0, 3.0, body, 1.0);
        crate::gfx::fill_rect(pm, cx - 2.5, top, 5.0, 2.0, body);
        let fill = if b.charging {
            th.ok.t()
        } else if b.percent <= 15 {
            th.danger.t()
        } else {
            th.text.t()
        };
        let fh = ((bh - 5.0) * b.percent.min(100) as f32 / 100.0).max(1.0);
        fill_rounded(pm, cx - bw / 2.0 + 2.0, top + 2.5 + bh - 1.0 - 2.0 - fh, bw - 4.0, fh, 1.5, color(fill.0, fill.1, fill.2, 0.95));
        self.v_text(pm, &format!("{}", b.percent), cx, top + bh + 9.0, 10.0, Weight::Medium, th.text.t(), 0.9);
    }
}

/// Top y that keeps a box `h` tall inside a screen `screen` tall, `POP_MARGIN` from the edges (the margin wins on a short screen).
pub fn clamp_y(y: f32, h: f32, screen: f32) -> f32 {
    y.min(screen - h - POP_MARGIN).max(POP_MARGIN)
}

/// Starts a UI program (dashboard, session menu). In dry-run mode (tests) the command is only recorded.
fn launch_ui(argv: Vec<String>) {
    if sys::is_dry_run() {
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        sys::spawn(&refs);
    } else {
        crate::launch::spawn_argv(&argv);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Content {
        Content { workspaces: 9, window: 140.0, tray: vec![Tray::Layout, Tray::Control], status: vec![Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery] }
    }

    #[test]
    fn the_surface_leaves_room_for_the_widest_panel() {
        const { assert!(POPOUT_W >= POP_GAP + controlcenter::PANEL_W + 8.0) };
        assert_eq!(SURFACE_W as f32, BAR_W + POPOUT_W);
        assert_eq!(PILL_W, 40.0);
    }

    #[test]
    fn every_pill_sits_inside_the_strip_and_the_screen_and_none_overlap() {
        for height in [800.0, 900.0, 1440.0] {
            let lay = layout(height, &full());
            assert_eq!(lay.pills.len(), 7, "{height}");
            let mut last_bottom = 0.0;
            for (_, r) in &lay.pills {
                assert!(r.x >= PILL_X && r.x + r.w <= BAR_W - PILL_X + 0.001, "inside the strip: {r:?}");
                assert!(r.y >= EDGE - 0.001 && r.bottom() <= height - EDGE + 0.001, "{height}: {r:?}");
                assert!(r.y >= last_bottom + GAP - 0.001, "{height}: pills are separated by the gap: {r:?} after {last_bottom}");
                last_bottom = r.bottom();
            }
        }
    }

    #[test]
    fn the_order_is_logo_workspaces_window_tray_status_clock_power() {
        let lay = layout(900.0, &full());
        let order: Vec<Pill> = lay.pills.iter().map(|p| p.0).collect();
        assert_eq!(order, [Pill::Logo, Pill::Workspaces, Pill::Window, Pill::Tray, Pill::Status, Pill::Clock, Pill::Power]);
        let kinds: Vec<Tray> = lay.items.iter().map(|i| i.0).collect();
        assert_eq!(kinds, [Tray::Logo, Tray::Workspaces, Tray::Window, Tray::Layout, Tray::Control, Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery, Tray::Clock, Tray::Power]);
        let ys: Vec<f32> = lay.items.iter().map(|i| i.1.y).collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "items run down the screen: {ys:?}");
    }

    #[test]
    fn the_top_group_hangs_from_the_top_and_the_bottom_group_stands_on_the_bottom() {
        let lay = layout(900.0, &full());
        assert_eq!(lay.pill(Pill::Logo).unwrap().y, EDGE);
        assert_eq!(lay.pill(Pill::Workspaces).unwrap().y, EDGE + LOGO_H + GAP);
        assert_eq!(lay.pill(Pill::Power).unwrap().bottom(), 900.0 - EDGE);
        assert_eq!(lay.pill(Pill::Clock).unwrap().bottom(), lay.pill(Pill::Power).unwrap().y - GAP);
        let ws = lay.pill(Pill::Workspaces).unwrap();
        assert_eq!(ws.h, 9.0 * WS_CELL + 2.0 * WS_PAD);
        // a taller screen moves the bottom group down with it, the top group stays
        let tall = layout(1200.0, &full());
        assert_eq!(tall.pill(Pill::Logo), lay.pill(Pill::Logo));
        assert_eq!(tall.pill(Pill::Power).unwrap().y - lay.pill(Pill::Power).unwrap().y, 300.0);
    }

    #[test]
    fn the_window_pill_is_centred_in_the_free_space_and_capped_by_it() {
        let lay = layout(900.0, &full());
        let (ws, win, tray) = (lay.pill(Pill::Workspaces).unwrap(), lay.pill(Pill::Window).unwrap(), lay.pill(Pill::Tray).unwrap());
        let above = win.y - ws.bottom();
        let below = tray.y - win.bottom();
        assert!((above - below).abs() < 0.01 && above >= GAP - 0.01, "centred: {above} vs {below}");
        assert_eq!(win.h, 140.0);
        // asks for more than there is: takes what is free, between the gaps
        let big = layout(900.0, &Content { window: 2000.0, ..full() });
        let (ws, win, tray) = (big.pill(Pill::Workspaces).unwrap(), big.pill(Pill::Window).unwrap(), big.pill(Pill::Tray).unwrap());
        assert_eq!((win.y, win.bottom()), (ws.bottom() + GAP, tray.y - GAP));
    }

    #[test]
    fn the_window_pill_goes_when_there_is_no_room_or_no_window() {
        assert!(layout(900.0, &Content { window: 0.0, ..full() }).pill(Pill::Window).is_none());
        // a short screen: the stacks fill it, nothing is left for the window
        let c = full();
        let fixed = EDGE + LOGO_H + GAP + workspaces_h(9) + GAP + group_h(&c.tray) + GAP + group_h(&c.status) + GAP + CLOCK_H + GAP + POWER_H + EDGE;
        assert!(layout(fixed + WIN_MIN - 1.0, &c).pill(Pill::Window).is_none());
        assert!(layout(fixed + WIN_MIN + 2.0 * GAP, &c).pill(Pill::Window).is_some());
        // no workspaces (not rdwm): the pill is simply absent
        assert!(layout(900.0, &Content { workspaces: 0, ..full() }).pill(Pill::Workspaces).is_none());
    }

    #[test]
    fn icons_fill_their_pill_exactly_and_the_battery_is_taller() {
        let lay = layout(900.0, &full());
        let status = lay.pill(Pill::Status).unwrap();
        assert_eq!(status.h, 3.0 * ICON_CELL + BATT_CELL + 2.0 * CELL_PAD);
        let cells: Vec<VRect> = lay.items.iter().filter(|i| matches!(i.0, Tray::Audio | Tray::Network | Tray::Bluetooth | Tray::Battery)).map(|i| i.1).collect();
        assert_eq!(cells[0].y, status.y + CELL_PAD);
        for pair in cells.windows(2) {
            assert_eq!(pair[0].bottom(), pair[1].y);
        }
        assert_eq!(cells[3].h, BATT_CELL);
        assert_eq!(cells[3].bottom(), status.bottom() - CELL_PAD);
    }

    #[test]
    fn hit_testing_finds_each_item_and_nothing_in_the_gaps() {
        let lay = layout(900.0, &full());
        for (k, r) in &lay.items {
            assert_eq!(lay.hit(r.cx(), r.cy()).map(|h| h.0), Some(*k));
            assert_eq!(lay.hit(r.x + 1.0, r.y + 1.0).map(|h| h.0), Some(*k), "{k:?} top-left corner");
        }
        let logo = lay.pill(Pill::Logo).unwrap();
        assert_eq!(lay.hit(logo.cx(), logo.bottom() + GAP / 2.0), None, "the gap between pills");
        assert_eq!(lay.hit(2.0, logo.cy()), None, "the strip margin left of the pills");
        assert_eq!(lay.hit(BAR_W + 5.0, logo.cy()), None, "right of the strip");
    }

    #[test]
    fn workspace_pills_hit_test_by_cell() {
        let pill = VRect { x: PILL_X, y: 100.0, w: PILL_W, h: workspaces_h(9) };
        assert_eq!(workspace_hit(pill, 9, 100.0), None, "the pill's top padding");
        assert_eq!(workspace_hit(pill, 9, 100.0 + WS_PAD), Some(0));
        assert_eq!(workspace_hit(pill, 9, 100.0 + WS_PAD + WS_CELL - 0.1), Some(0));
        assert_eq!(workspace_hit(pill, 9, 100.0 + WS_PAD + WS_CELL), Some(1));
        for i in 0..9 {
            assert_eq!(workspace_hit(pill, 9, 100.0 + WS_PAD + WS_CELL * (i as f32 + 0.5)), Some(i));
        }
        assert_eq!(workspace_hit(pill, 9, pill.bottom() - WS_PAD / 2.0), None, "the bottom padding");
        assert_eq!(workspace_hit(pill, 9, pill.bottom() + 1.0), None);
        // fewer cells than the pill is tall for (the list shrank): nothing past the last one
        assert_eq!(workspace_hit(pill, 4, 100.0 + WS_PAD + WS_CELL * 5.5), None);
    }

    #[test]
    fn clicks_map_to_the_widgets() {
        assert_eq!(click_for(Tray::Logo, None), Click::Launcher);
        assert_eq!(click_for(Tray::Clock, None), Click::Dashboard);
        assert_eq!(click_for(Tray::Power, None), Click::Session);
        assert_eq!(click_for(Tray::Control, None), Click::Control);
        assert_eq!(click_for(Tray::Workspaces, Some(4)), Click::Workspace(4));
        assert_eq!(click_for(Tray::Workspaces, None), Click::Nothing);
        for k in [Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery, Tray::Window, Tray::Layout] {
            assert_eq!(click_for(k, None), Click::Popout(k));
        }
        assert_eq!(click_for(Tray::Awake, None), Click::Nothing);
        assert_eq!(view_request(0), r#"{"action":"view","args":["1"]}"#);
        assert_eq!(view_request(8), r#"{"action":"view","args":["9"]}"#);
    }

    #[test]
    fn the_stacked_clock_pads_to_two_digits() {
        assert_eq!(clock_parts(9, 5, false), ("09".into(), "05".into()));
        assert_eq!(clock_parts(23, 59, false), ("23".into(), "59".into()));
        assert_eq!(clock_parts(0, 0, true), ("12".into(), "00".into()));
        assert_eq!(clock_parts(13, 7, true), ("01".into(), "07".into()));
        assert_eq!(clock_parts(12, 30, true), ("12".into(), "30".into()));
    }

    #[test]
    fn the_os_logo_follows_os_release() {
        assert_eq!(os_logo(&["omarchy", "arch"]), '\u{f303}');
        assert_eq!(os_logo(&["somethingnew", "debian"]), '\u{f306}', "ID_LIKE is the fallback");
        assert_eq!(os_logo(&[]), '\u{f17c}');
        assert_eq!(os_logo(&["unknownos"]), '\u{f17c}');
    }

    #[test]
    fn popups_stay_on_screen() {
        assert_eq!(clamp_y(-50.0, 100.0, 900.0), POP_MARGIN);
        assert_eq!(clamp_y(300.0, 100.0, 900.0), 300.0);
        assert_eq!(clamp_y(880.0, 100.0, 900.0), 900.0 - 100.0 - POP_MARGIN);
        assert_eq!(clamp_y(10.0, 2000.0, 900.0), POP_MARGIN, "taller than the screen: the top edge wins");
    }

    // ------------------------------------------------------------------ the bar itself

    fn bar() -> MacBar {
        TEST_VERTICAL.with(|f| f.set(true));
        let mut bar = MacBar::new(Box::new(active::NoActiveWindow));
        bar.width = SURFACE_W;
        bar.height = 900;
        bar.show_workspaces = true;
        bar.workspaces = (0..9).map(|i| Workspace { name: (i + 1).to_string(), active: i == 1, occupied: i == 2, urgent: i == 4 }).collect();
        bar
    }

    #[test]
    fn the_surface_asks_for_the_left_edge_and_a_strut_of_the_bar_width() {
        let b = bar();
        let spec = b.surface_spec().unwrap();
        assert_eq!((spec.anchor, spec.reserve, spec.stretch, spec.margin), (crate::backend::Anchor::Left, BAR_W as i32, true, 0));
        assert_eq!(b.size(), (SURFACE_W, 0), "as wide as the strip plus the popouts, full height");
        TEST_VERTICAL.with(|f| f.set(false));
        let h = MacBar::new(Box::new(active::NoActiveWindow));
        assert_eq!(h.surface_spec().unwrap().anchor, crate::backend::Anchor::Top, "the horizontal bar is unchanged");
        assert_eq!(h.size(), (0, SURFACE_MIN_HEIGHT));
    }

    #[test]
    fn only_the_strip_takes_input_until_something_opens() {
        let b = bar();
        assert_eq!(b.input_rects(), vec![InputRect { x: 0, y: 0, w: BAR_W as i32, h: 900 }]);
        assert!(!b.wants_keyboard() && !b.wants_pointer_grab());
    }

    #[test]
    fn clicking_a_workspace_pill_asks_rdwm_to_view_that_tag() {
        let mut b = bar();
        let _g = crate::menubar::tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let _ = sys::take_log();
        let ws = b.v_layout().pill(Pill::Workspaces).unwrap();
        let (x, y) = (ws.cx(), ws.y + WS_PAD + WS_CELL * 3.5);
        assert_eq!(b.v_target(x, y), Some((Tray::Workspaces, Some(3))));
        // without rdwm tags it falls back to the EWMH tracker (a no-op here); with them it sends `view 4`
        b.v_switch_workspace(3);
        *b.shared.layout.lock().unwrap() = Some(rdwm::LayoutState { name: "tile".into(), symbol: "[]=".into(), tags: vec![TagState::default(); 9] });
        b.v_switch_workspace(3);
        let log = sys::take_log();
        sys::set_dry_run(false);
        assert!(log.iter().any(|l| l.contains("rdwmctl action view 4")), "{log:?}");
    }

    #[test]
    fn rdwm_tags_win_over_the_ewmh_desktops() {
        let b = bar();
        assert_eq!(b.v_workspaces().len(), 9);
        let tags = vec![
            TagState { name: "1".into(), active: false, occupied: true, urgent: false },
            TagState { name: "2".into(), active: true, occupied: false, urgent: false },
            TagState { name: "3".into(), active: false, occupied: false, urgent: true },
        ];
        *b.shared.layout.lock().unwrap() = Some(rdwm::LayoutState { name: "tile".into(), symbol: "[]=".into(), tags });
        let ws = b.v_workspaces();
        assert_eq!(ws.len(), 3);
        assert_eq!((ws[0].occupied, ws[1].active, ws[2].urgent), (true, true, true));
        assert_eq!(b.v_content().workspaces, 3);
    }

    #[test]
    fn the_workspace_pills_are_hidden_when_workspaces_are_off() {
        let mut b = bar();
        b.show_workspaces = false;
        assert!(b.v_workspaces().is_empty());
        assert!(b.v_layout().pill(Pill::Workspaces).is_none());
    }

    #[test]
    fn hovering_a_status_icon_opens_its_popup_after_the_delay_and_sliding_switches() {
        let mut b = bar();
        b.update_snapshot(|s| {
            s.volume = Some(sys::Volume { level: 0.4, muted: false });
            s.net = Some(sys::Net { wifi_enabled: true, wifi_name: "home".into(), wired_name: String::new() });
        });
        let lay = b.v_layout();
        let audio = lay.item(Tray::Audio).unwrap();
        let net = lay.item(Tray::Network).unwrap();
        assert!(b.pointer_motion(audio.cx(), audio.cy()));
        assert_eq!(b.open, Open::None, "not at once");
        assert!(b.tick_in().is_some(), "the delay is scheduled");
        b.v_hover_open = Some((Tray::Audio, Instant::now() - HOVER_DELAY));
        let t = b.tick();
        assert!(t.redraw);
        assert_eq!(b.open, Open::Popup(Tray::Audio));
        let p = b.popup.as_ref().unwrap();
        assert!(p.x >= BAR_W && p.x + p.w <= SURFACE_W as f32 && p.y >= POP_MARGIN && p.y + p.h <= 900.0, "inside the surface, right of the strip");
        assert!(b.wants_pointer_grab());
        // the popup's input region reaches back to the strip, so the pointer can get there
        let rects = b.input_rects();
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[1].x, BAR_W as i32);
        assert!(rects[1].x + rects[1].w >= (p.x + p.w) as i32);
        // sliding to the next icon switches at once
        b.pointer_motion(net.cx(), net.cy());
        assert_eq!(b.open, Open::Popup(Tray::Network));
        // onto the logo (no popup): it closes
        let logo = lay.item(Tray::Logo).unwrap();
        b.pointer_motion(logo.cx(), logo.cy());
        assert_eq!(b.open, Open::None);
    }

    #[test]
    fn leaving_with_a_popup_open_closes_it_after_the_delay() {
        let mut b = bar();
        b.update_snapshot(|s| s.volume = Some(sys::Volume { level: 0.4, muted: false }));
        let audio = b.v_layout().item(Tray::Audio).unwrap();
        b.pointer_button(audio.cx(), audio.cy(), Button::Left);
        assert_eq!(b.open, Open::Popup(Tray::Audio));
        b.pointer_button(audio.cx(), audio.cy(), Button::Left);
        assert_eq!(b.open, Open::Popup(Tray::Audio), "a click keeps the hover popup open");
        b.pointer_motion(SURFACE_W as f32 - 5.0, 20.0);
        assert!(b.leave_at.is_some());
        b.leave_at = Some(Instant::now() - CLOSE_DELAY);
        b.tick();
        assert_eq!(b.open, Open::None);
    }

    #[test]
    fn the_logo_clock_and_power_launch_their_programs() {
        let mut b = bar();
        let _g = crate::menubar::tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        sys::set_dry_run(true);
        let _ = sys::take_log();
        let lay = b.v_layout();
        for k in [Tray::Logo, Tray::Clock, Tray::Power] {
            let r = lay.item(k).unwrap();
            assert!(b.pointer_button(r.cx(), r.cy(), Button::Left), "{k:?}");
        }
        let log = sys::take_log();
        sys::set_dry_run(false);
        let has = |needle: &str| log.iter().any(|l| l.contains(needle));
        assert!(has("macspot") || has("rofi") || has("wofi") || has("fuzzel") || log.is_empty() || !sys::have("macspot"), "launcher: {log:?}");
        assert!(has("macdash --top 10"), "{log:?}");
        assert!(has("macsession"), "{log:?}");
    }

    #[test]
    fn the_control_center_opens_beside_its_tile_and_stays_on_screen() {
        let mut b = bar();
        let tile = b.v_layout().item(Tray::Control).unwrap();
        assert!(b.pointer_button(tile.cx(), tile.cy(), Button::Left));
        assert_eq!(b.open, Open::Control);
        let (ox, oy) = b.v_panel_origin();
        assert_eq!(ox, BAR_W + POP_GAP);
        assert!(oy >= POP_MARGIN && oy + b.cc_height <= 900.0 - POP_MARGIN + 0.01, "{oy} + {}", b.cc_height);
        assert!(b.input_rects().iter().any(|r| r.x == BAR_W as i32 && r.w as f32 >= controlcenter::PANEL_W));
        assert!(b.pointer_button(tile.cx(), tile.cy(), Button::Left));
        assert_eq!(b.open, Open::None, "the tile toggles");
    }

    #[test]
    fn switching_the_theme_layout_closes_whatever_is_open_and_resizes() {
        let mut b = bar();
        b.was_vertical = true;
        b.update_snapshot(|s| s.volume = Some(sys::Volume { level: 0.4, muted: false }));
        let audio = b.v_layout().item(Tray::Audio).unwrap();
        b.pointer_button(audio.cx(), audio.cy(), Button::Left);
        assert_ne!(b.open, Open::None);
        TEST_VERTICAL.with(|f| f.set(false));
        let t = b.tick();
        assert!(t.redraw);
        assert_eq!(b.open, Open::None);
        assert_eq!(b.size(), (0, SURFACE_MIN_HEIGHT));
        assert!(!b.was_vertical);
    }

    #[test]
    fn a_vertical_frame_paints_the_strip_pills_and_leaves_the_popout_area_clear() {
        let mut b = bar();
        let mut pm = crate::gfx::pixmap(SURFACE_W, 900).unwrap();
        b.draw(&mut pm);
        let th = crate::theme::th();
        let lay = b.v_layout();
        let logo = lay.pill(Pill::Logo).unwrap();
        let a = |x: u32, y: u32| pm.pixel(x, y).unwrap().alpha();
        assert_eq!(a(0, 0), 255, "the strip is painted from the corner");
        assert_eq!(a(BAR_W as u32 - 1, 899), 255);
        assert_eq!(a(BAR_W as u32 + 5, 100), 0, "right of the strip stays transparent");
        assert_eq!(a(BAR_W as u32 + 5, 899), 0);
        let strip = pm.pixel(2, 450).unwrap();
        let pill = pm.pixel(logo.cx() as u32, (logo.y + 3.0) as u32).unwrap();
        assert_ne!((strip.red(), strip.green(), strip.blue()), (pill.red(), pill.green(), pill.blue()), "pills are drawn over the strip ({:?})", th.id);
    }
}
