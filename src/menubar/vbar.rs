//! The Caelestia pill bar: `bar_style = "vertical"` (the Caelestia theme), a bar along a screen edge.
//!
//! One layout, four edges. In reading order the bar holds the OS icon (opens the launcher), the workspace pills
//! (rdwm tags), the focused window (icon and title), a tray group (layout, stay-awake, agents, quick settings),
//! status icons (lock keys, sound, Wi-Fi / Ethernet, Bluetooth, battery), the clock (opens the dashboard) and the
//! power button (opens the session menu). Hovering a status icon (or the window) opens the same popup the classic
//! menu bar opens on a click, on the side of the bar that faces the screen; the data comes from the same snapshot and
//! worker threads.
//!
//! `bar.position` in `decebal.json` picks the edge ([`BarPosition`]):
//!
//! | position | surface | strut | popouts open |
//! |---|---|---|---|
//! | `left` | [`SURFACE_W`] wide, as tall as the monitor | left | to the right |
//! | `right` | the same, on the right edge | right | to the left |
//! | `top` | as wide as the monitor, [`SURFACE_H`] tall | top | downwards |
//! | `bottom` | the same, on the bottom edge | bottom | upwards |
//! | `split` | a `top` bar with the head (OS icon, workspaces, window) and a second `decbar --role bottom` process with a `bottom` bar holding the tail (tray, status, clock, power) | top and bottom | down / up |
//!
//! The surface is bigger than the strip: 52 px at the sides and 40 px at the top/bottom are reserved (the strut) and take input while nothing is
//! open, exactly like the classic bar whose surface is taller than its strip.
//!
//! The layout is written once, for a bar running down an edge (x across the bar, y along it): [`layout`] computes the
//! rectangles in that frame and [`Geo`] rotates / mirrors them onto the surface of the chosen edge. Drawing,
//! hit-testing, popout placement and the tests all share the result, so a horizontal bar is the same bar turned.

use super::indicators::IndicatorPress;
use super::popup::Popup;
use super::*;
use crate::backend::Anchor;
use crate::config::BarPosition;
use crate::menubar::rdwm::TagState;

/// Thickness of the bar strip, and of the strut it reserves.
pub const THICK: f32 = crate::theme::PILL_BAR_THICKNESS;
pub const HORIZONTAL_THICK: f32 = crate::theme::HORIZONTAL_PILL_BAR_THICKNESS;
/// Room beside a side bar for popups and the quick-settings panel (widest: the Control Center, 340 px).
pub const POPOUT_W: f32 = 392.0;
pub const SURFACE_W: u32 = (THICK + POPOUT_W) as u32;
/// Room under (over) a top (bottom) bar for the same; the tallest thing is the Control Center. A small monitor cuts it.
pub const POPOUT_H: f32 = 720.0;
pub const SURFACE_H: u32 = (HORIZONTAL_THICK + POPOUT_H) as u32;
const PILL_X: f32 = 6.0;
pub const PILL_W: f32 = THICK - 2.0 * PILL_X;
/// Space before the first and after the last pill, and between pills.
const EDGE: f32 = 8.0;
const GAP: f32 = 8.0;
/// Padding before / after the workspace cells inside their pill.
const WS_PAD: f32 = 6.0;
/// One icon cell in the tray and status pills (the battery also carries its percentage).
pub const ICON_CELL: f32 = 30.0;
const CELL_PAD: f32 = 6.0;
const WIN_ICON: f32 = 22.0;
/// Gap between the strip and the popup opened from it.
const POP_GAP: f32 = 6.0;
const POP_MARGIN: f32 = 8.0;
/// Pointer rest time on a status icon before its popup opens (moving between icons switches at once).
pub const HOVER_DELAY: Duration = Duration::from_millis(140);
/// How often the main bar of a split layout checks that its bottom half is running.
pub const COMPANION_POLL: Duration = Duration::from_secs(2);
/// Wait before starting the bottom process again after it ended.
const COMPANION_RETRY: Duration = Duration::from_secs(8);

/// Sizes that differ between a bar down an edge and one along it, so that the same content fits both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dim {
    /// One workspace cell along the bar.
    pub ws_cell: f32,
    pub batt_cell: f32,
    pub clock_len: f32,
    /// Longest title run drawn in the window pill.
    pub win_text_max: f32,
    /// The window pill is left out when less than this is free for it.
    pub win_min: f32,
}

impl Dim {
    pub fn of(horizontal: bool) -> Dim {
        if horizontal {
            Dim { ws_cell: 30.0, batt_cell: 54.0, clock_len: 64.0, win_text_max: 340.0, win_min: 96.0 }
        } else {
            Dim { ws_cell: 26.0, batt_cell: 44.0, clock_len: 58.0, win_text_max: 200.0, win_min: 72.0 }
        }
    }
}

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
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// The rounded containers, in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pill {
    Logo,
    AppMenu,
    Workspaces,
    Tasks,
    Window,
    Tray,
    Status,
    Clock,
    Power,
    ShowDesktop,
}

/// The pill of a single-item head or tail applet.
fn single_pill(kind: Tray) -> Pill {
    match kind {
        Tray::AppMenu => Pill::AppMenu,
        Tray::Clock => Pill::Clock,
        Tray::Power => Pill::Power,
        Tray::ShowDesktop => Pill::ShowDesktop,
        _ => Pill::Logo,
    }
}

/// How a rail is painted: the Caelestia pills, or another bar style turned onto a side edge in its own look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Skin {
    #[default]
    Pill,
    /// The classic menu bar: a flat strip in the bar colour, no pill backgrounds.
    Classic,
    /// The floating bar: one rounded strip held off the screen edge.
    Floating,
    /// The Dux band with a face per item.
    Dux,
}

impl Skin {
    pub fn of(r: crate::panel::Renderer) -> Skin {
        use crate::panel::Renderer as R;
        match r {
            R::Pill => Skin::Pill,
            R::Floating => Skin::Floating,
            R::Dux => Skin::Dux,
            R::MenuBar | R::Taskbar => Skin::Classic,
        }
    }
    /// Thickness of the strip on a side (`vertical`) or a top/bottom edge.
    pub fn thickness(self, vertical: bool) -> f32 {
        match self {
            Skin::Pill => {
                if vertical {
                    THICK
                } else {
                    HORIZONTAL_THICK
                }
            }
            Skin::Classic | Skin::Floating => crate::theme::CLASSIC_RAIL_THICKNESS,
            Skin::Dux => {
                let th = crate::theme::th();
                th.bar_height + 2.0 * th.bar_layout.margin
            }
        }
    }
    /// Distance from the strip's sides to the item cells.
    pub fn inset(self, vertical: bool) -> f32 {
        match self {
            Skin::Pill => {
                if vertical {
                    PILL_X
                } else {
                    4.0
                }
            }
            Skin::Classic => 3.0,
            Skin::Floating => 5.0,
            Skin::Dux => {
                let th = crate::theme::th();
                (th.bar_layout.margin + 5.0).min(self.thickness(vertical) / 2.0 - 8.0).max(2.0)
            }
        }
    }
}

/// Which pills a bar surface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// Everything: a single bar.
    All,
    /// The OS icon, workspaces and window title: the top bar of a split layout.
    Head,
    /// Tray, status icons, clock and power: the bottom bar of a split layout.
    Tail,
}

/// Which decbar process this is. A split layout is two processes, each with its own surface and its own single-instance
/// lock: the main one draws the top bar, `decbar --role bottom` the bottom bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Process {
    #[default]
    Main,
    Bottom,
}

impl Process {
    pub fn parse(s: &str) -> Option<Process> {
        match s {
            "main" | "top" => Some(Process::Main),
            "bottom" => Some(Process::Bottom),
            _ => None,
        }
    }
    /// Name of the single-instance lock.
    pub fn lock_name(self) -> &'static str {
        match self {
            Process::Main => "decbar",
            Process::Bottom => "decbar-bottom",
        }
    }
}

/// Where one bar surface sits and what it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub edge: Anchor,
    pub part: Part,
    pub skin: Skin,
}

impl Place {
    /// The surface `process` draws for `position`; `None` when it has nothing to draw (the bottom process of an unsplit layout).
    pub fn of(position: BarPosition, process: Process) -> Option<Place> {
        let (edge, part) = match (position, process) {
            (BarPosition::Left, Process::Main) => (Anchor::Left, Part::All),
            (BarPosition::Right, Process::Main) => (Anchor::Right, Part::All),
            (BarPosition::Top, Process::Main) => (Anchor::Top, Part::All),
            (BarPosition::Bottom, Process::Main) => (Anchor::Bottom, Part::All),
            (BarPosition::Split, Process::Main) => (Anchor::Top, Part::Head),
            (BarPosition::Split, Process::Bottom) => (Anchor::Bottom, Part::Tail),
            (_, Process::Bottom) => return None,
        };
        Some(Place { edge, part, skin: Skin::Pill })
    }
    pub fn with_skin(self, skin: Skin) -> Place {
        Place { skin, ..self }
    }
    pub fn thickness(self) -> f32 {
        self.skin.thickness(self.edge.is_side())
    }
    /// The surface: the strip plus room for popouts on its screen side.
    pub fn surface_size(self) -> (u32, u32) {
        if self.edge.is_side() {
            ((self.thickness() + POPOUT_W) as u32, 0)
        } else {
            (0, (self.thickness() + POPOUT_H) as u32)
        }
    }
    /// Window name of the surface (what `xdotool search --name` finds).
    pub fn namespace(self) -> &'static str {
        if self.part == Part::Tail {
            "decebal-bar-bottom"
        } else {
            "decebal-bar"
        }
    }
}

/// Where `process` draws, given what the theme says (`Some(position)` while it has the pill bar) and an explicit
/// `--position`, which wins over the setting but never over a theme without the pill bar.
pub fn place_for(theme_says: Option<BarPosition>, over: Option<BarPosition>, process: Process) -> Option<Place> {
    Place::of(theme_says.map(|p| over.unwrap_or(p))?, process)
}

/// [`place_for`] for the theme and settings on disk right now (what a process checks before it starts).
pub fn place_now(process: Process, over: Option<BarPosition>) -> Option<Place> {
    crate::theme::live::reload();
    place_for(crate::theme::th().pill_bar(), over, process)
}

/// The surface and its edge: turns the one layout into rectangles on any of the four edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geo {
    pub edge: Anchor,
    /// Surface size.
    pub w: f32,
    pub h: f32,
    /// The strip's thickness and the inset of its cells (the skin's).
    pub thick: f32,
    pub inset: f32,
}

impl Geo {
    /// The pill rail's geometry.
    pub fn new(edge: Anchor, w: f32, h: f32) -> Geo {
        Geo::skinned(edge, w, h, Skin::Pill)
    }
    pub fn skinned(edge: Anchor, w: f32, h: f32, skin: Skin) -> Geo {
        Geo { edge, w, h, thick: skin.thickness(edge.is_side()), inset: skin.inset(edge.is_side()) }
    }
    /// The bar runs along the top or bottom edge.
    pub fn horizontal(&self) -> bool {
        !self.edge.is_side()
    }
    pub fn thickness(&self) -> f32 {
        self.thick
    }
    fn pill_inset(&self) -> f32 {
        self.inset
    }
    pub(super) fn pill_width(&self) -> f32 {
        self.thickness() - 2.0 * self.pill_inset()
    }
    /// Length of the bar along its edge.
    pub fn len(&self) -> f32 {
        if self.horizontal() {
            self.w
        } else {
            self.h
        }
    }
    /// The coordinate along the bar (x of a horizontal bar, y of a vertical one).
    pub fn along(&self, x: f32, y: f32) -> f32 {
        if self.horizontal() {
            x
        } else {
            y
        }
    }
    /// Surface coordinate where the strip starts across the bar.
    fn base(&self) -> f32 {
        match self.edge {
            Anchor::Left | Anchor::Top => 0.0,
            Anchor::Right => self.w - self.thick,
            Anchor::Bottom => self.h - self.thickness(),
        }
    }
    /// The bar strip on the surface: the part that is reserved and takes input.
    pub fn strip(&self) -> VRect {
        if self.horizontal() {
            VRect { x: 0.0, y: self.base(), w: self.w, h: self.thickness() }
        } else {
            VRect { x: self.base(), y: 0.0, w: self.thick, h: self.h }
        }
    }
    /// A rectangle of the layout frame (x across the bar from its outer edge, y along it) on the surface.
    fn map(&self, r: VRect) -> VRect {
        if self.horizontal() {
            VRect { x: r.y, y: self.base() + r.x, w: r.h, h: r.w }
        } else {
            VRect { x: self.base() + r.x, y: r.y, w: r.w, h: r.h }
        }
    }
    /// Top-left of a box `pw` x `ph` that pops out of `item`: on the screen side of the strip, centred on the item along
    /// the bar, kept on the surface.
    pub fn popout_origin(&self, item: VRect, pw: f32, ph: f32) -> (f32, f32) {
        match self.edge {
            Anchor::Left => (self.thick + POP_GAP, clamp_in(item.cy() - ph / 2.0, ph, self.h)),
            Anchor::Right => (self.w - self.thick - POP_GAP - pw, clamp_in(item.cy() - ph / 2.0, ph, self.h)),
            Anchor::Top => (clamp_in(item.cx() - pw / 2.0, pw, self.w), self.thickness() + POP_GAP),
            Anchor::Bottom => (clamp_in(item.cx() - pw / 2.0, pw, self.w), self.h - self.thickness() - POP_GAP - ph),
        }
    }
    /// Input region for a popout box: the box and the way back to the strip, so the pointer never leaves the surface on its way.
    pub fn reach(&self, x: f32, y: f32, w: f32, h: f32) -> InputRect {
        let s = self.strip();
        let (x0, y0, x1, y1) = match self.edge {
            Anchor::Left => (s.right(), y, x + w, y + h),
            Anchor::Right => (x, y, s.x, y + h),
            Anchor::Top => (x, s.bottom(), x + w, y + h),
            Anchor::Bottom => (x, y, x + w, s.y),
        };
        InputRect { x: x0.floor() as i32, y: y0.floor() as i32, w: (x1 - x0.floor()).ceil() as i32, h: (y1 - y0.floor()).ceil() as i32 }
    }
}

/// What the bar shows on this machine right now.
#[derive(Debug, Clone, PartialEq)]
pub struct Content {
    /// Single pills at the start (the launcher, the folded application menu).
    pub head: Vec<Tray>,
    pub workspaces: usize,
    /// Task button cells (their own pill after the workspaces).
    pub tasks: Vec<Tray>,
    /// Length the window pill would like; 0 = no focused window.
    pub window: f32,
    pub tray: Vec<Tray>,
    pub status: Vec<Tray>,
    /// Single pills at the end, in reading order (the clock, power, show desktop).
    pub tail: Vec<Tray>,
}

impl Default for Content {
    /// The rail as it always was: the OS icon first, the clock and power last.
    fn default() -> Self {
        Content { head: vec![Tray::Logo], workspaces: 0, tasks: Vec::new(), window: 0.0, tray: Vec::new(), status: Vec::new(), tail: vec![Tray::Clock, Tray::Power] }
    }
}

/// Length along the bar of the fixed pills (everything but the window pill), gaps and edges included.
fn fixed_len(geo: &Geo, part: Part, c: &Content) -> f32 {
    let d = Dim::of(geo.horizontal());
    let mut len = 2.0 * EDGE;
    if part != Part::Tail {
        len += c.head.len() as f32 * (geo.pill_width() + GAP);
        if c.workspaces > 0 {
            len += workspaces_len(c.workspaces, geo.horizontal()) + GAP;
        }
        if !c.tasks.is_empty() {
            len += group_len(&c.tasks, geo.horizontal()) + GAP;
        }
    }
    if part != Part::Head {
        for k in &c.tail {
            len += if *k == Tray::Clock { d.clock_len } else { geo.pill_width() } + GAP;
        }
        for kinds in [&c.tray, &c.status] {
            if !kinds.is_empty() {
                len += group_len(kinds, geo.horizontal()) + GAP;
            }
        }
    }
    len
}

/// Moves what does not fit into the overflow cell: task buttons first, then optional detail, then controls; on a rail too
/// short even for those, power, the clock, the window, the workspaces and the logo too. Quick settings and the overflow
/// cell itself always stay. Returns what moved.
pub fn fit_overflow(geo: &Geo, part: Part, c: &mut Content) -> Vec<Tray> {
    let mut hidden = Vec::new();
    let order = [
        Tray::Media,
        Tray::Cpu,
        Tray::Memory,
        Tray::Weather,
        Tray::Clipboard,
        Tray::Notifications,
        Tray::Spotlight,
        Tray::Dash,
        Tray::Brightness,
        Tray::Agents,
        Tray::Lock,
        Tray::Layout,
        Tray::ShowDesktop,
        Tray::AppMenu,
        Tray::Bluetooth,
        Tray::Network,
        Tray::Audio,
        Tray::Awake,
        Tray::Battery,
        // a rail too short even for its essentials (a 1024 x 480 output at 200 % is 240 px tall): they move into the overflow
        // cell too rather than overlap; the Control Center and the overflow cell itself always stay
        Tray::Power,
        Tray::Clock,
        Tray::Window,
        Tray::Workspaces,
        Tray::Logo,
    ];
    let fits = |c: &Content| fixed_len(geo, part, c) <= geo.len();
    if fits(c) {
        return hidden;
    }
    let more = |c: &mut Content| {
        if !c.status.contains(&Tray::More) {
            c.status.push(Tray::More);
        }
    };
    hidden.append(&mut c.tasks);
    more(c);
    for victim in order {
        if fits(c) {
            break;
        }
        if victim == Tray::Workspaces && c.workspaces > 0 {
            // the workspace pill is a count, not an entry of the lists below
            c.workspaces = 0;
            hidden.push(Tray::Workspaces);
            continue;
        }
        for list in [&mut c.tray, &mut c.status, &mut c.head, &mut c.tail] {
            if let Some(i) = list.iter().position(|k| *k == victim) {
                hidden.push(list.remove(i));
            }
        }
    }
    hidden
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub pills: Vec<(Pill, VRect)>,
    /// Hit targets: the single-item pills as a whole, the icons of the tray and status pills one by one.
    pub items: Vec<(Tray, VRect)>,
}

/// Extra application cells that fit without displacing the fixed head/tail pills.
/// The optional focused-window pill gives way first; an overflow cell is counted
/// by the shared indicator selector, so its menu never requires an extra slot.
fn indicator_capacity(geo: &Geo, part: Part, c: &Content) -> usize {
    if part == Part::Head {
        return 0;
    }
    let overhead = if c.status.is_empty() { 2.0 * CELL_PAD + GAP } else { 0.0 };
    ((geo.len() - fixed_len(geo, part, c) - overhead).max(0.0) / ICON_CELL).floor().min(32.0) as usize
}

/// A short rail prefers one reachable app-overflow target to duplicate shortcuts.
/// Informational weather/lock state and optional layout/monitor launchers give
/// way first. Basic sound/connectivity/awake buttons only fold when the existing
/// Control Center exposes that same group. Battery, clock, power and workspace
/// targets remain. Some extreme (<600px) or deliberately disabled Control Center
/// combinations physically cannot fit all fixed targets; never overlap them to
/// pretend that an application icon is reachable.
fn fit_indicator_content(geo: &Geo, part: Part, c: &mut Content, active: bool, control: Option<controlcenter::settings::Tiles>) -> usize {
    if part == Part::Head {
        return 0;
    }
    let mut capacity = indicator_capacity(geo, part, c);
    if !active || capacity > 0 {
        return capacity;
    }
    for kind in [Tray::Weather, Tray::Lock, Tray::Agents, Tray::Layout, Tray::Network, Tray::Bluetooth, Tray::Audio, Tray::Awake] {
        let may_fold = match kind {
            Tray::Network | Tray::Bluetooth => control.is_some_and(|tiles| tiles.connectivity),
            Tray::Audio => control.is_some_and(|tiles| tiles.sound),
            Tray::Awake => control.is_some_and(|tiles| tiles.actions),
            _ => true,
        };
        if !may_fold {
            continue;
        }
        c.status.retain(|current| *current != kind);
        c.tray.retain(|current| *current != kind);
        capacity = indicator_capacity(geo, part, c);
        if capacity > 0 {
            break;
        }
    }
    capacity
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

fn cell_len(kind: Tray, d: &Dim) -> f32 {
    if matches!(kind, Tray::Battery | Tray::Cpu | Tray::Memory) {
        d.batt_cell
    } else {
        ICON_CELL
    }
}

/// Length along the bar of a tray / status group.
pub fn group_len(kinds: &[Tray], horizontal: bool) -> f32 {
    let d = Dim::of(horizontal);
    kinds.iter().map(|k| cell_len(*k, &d)).sum::<f32>() + 2.0 * CELL_PAD
}

/// Length along the bar of the workspace pill.
pub fn workspaces_len(count: usize, horizontal: bool) -> f32 {
    count as f32 * Dim::of(horizontal).ws_cell + 2.0 * WS_PAD
}

/// Lays the bar out on `geo`, showing the pills of `part`. The head (logo, workspaces) hangs from the start of the bar
/// (top / left), the tail (tray, status icons, clock, power) stands on its end (bottom / right), and the focused window
/// takes the room between, centred in it (a head-only bar puts it right after the workspaces); it is left out when
/// there is less than [`Dim::win_min`].
pub fn layout(geo: &Geo, part: Part, c: &Content) -> Layout {
    let horiz = geo.horizontal();
    let d = Dim::of(horiz);
    let len = geo.len();
    // the layout frame: x across the bar, y along it
    let pill_width = geo.pill_width();
    let pill = |u: f32, l: f32| VRect { x: geo.pill_inset(), y: u, w: pill_width, h: l };
    let mut pills: Vec<(Pill, VRect)> = Vec::new();
    let mut items: Vec<(Tray, VRect)> = Vec::new();
    let (head, tail) = (part != Part::Tail, part != Part::Head);

    let mut top = EDGE;
    if head {
        for kind in &c.head {
            let r = pill(top, pill_width);
            pills.push((single_pill(*kind), r));
            items.push((*kind, r));
            top += pill_width + GAP;
        }
        if c.workspaces > 0 {
            let r = pill(top, workspaces_len(c.workspaces, horiz));
            pills.push((Pill::Workspaces, r));
            items.push((Tray::Workspaces, r));
            top += r.h + GAP;
        }
        if !c.tasks.is_empty() {
            let r = pill(top, group_len(&c.tasks, horiz));
            pills.push((Pill::Tasks, r));
            let mut u = r.y + CELL_PAD;
            for k in &c.tasks {
                items.push((*k, pill(u, ICON_CELL)));
                u += ICON_CELL;
            }
            top += r.h + GAP;
        }
    }

    // The tail, built from the end of the bar back and then put in reading order.
    let mut bot = len - EDGE;
    let mut tail_pills: Vec<(Pill, VRect)> = Vec::new();
    let mut tail_items: Vec<(Tray, VRect)> = Vec::new();
    if tail {
        let mut stand = |kind: Pill, l: f32, bot: &mut f32| -> VRect {
            let r = pill(*bot - l, l);
            tail_pills.push((kind, r));
            *bot = r.y - GAP;
            r
        };
        for kind in c.tail.iter().rev() {
            let r = stand(single_pill(*kind), if *kind == Tray::Clock { d.clock_len } else { pill_width }, &mut bot);
            tail_items.push((*kind, r));
        }
        for (kind, trays) in [(Pill::Status, &c.status), (Pill::Tray, &c.tray)] {
            if trays.is_empty() {
                continue;
            }
            let r = stand(kind, group_len(trays, horiz), &mut bot);
            let mut u = r.y + CELL_PAD;
            for k in trays {
                let cell = pill(u, cell_len(*k, &d));
                tail_items.push((*k, cell));
                u += cell.h;
            }
        }
        tail_pills.reverse();
        // with a tail, `bot` is the free space's far boundary (it already includes the gap before the first tail pill)
    }
    let avail = bot - top;
    if head && c.window > 0.0 && avail >= d.win_min {
        let l = c.window.min(avail);
        let u = if tail { top + (avail - l) / 2.0 } else { top };
        let r = pill(u, l);
        pills.push((Pill::Window, r));
        items.push((Tray::Window, r));
    }
    pills.extend(tail_pills);
    tail_items.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    items.extend(tail_items);

    Layout { pills: pills.into_iter().map(|(p, r)| (p, geo.map(r))).collect(), items: items.into_iter().map(|(k, r)| (k, geo.map(r))).collect() }
}

/// Which workspace cell (0-based) lies under (`x`, `y`) inside the workspace pill `pill` of a bar.
pub fn workspace_hit(pill: VRect, horizontal: bool, count: usize, x: f32, y: f32) -> Option<usize> {
    let (start, len, at) = if horizontal { (pill.x, pill.w, x) } else { (pill.y, pill.h, y) };
    let rel = at - (start + WS_PAD);
    if rel < 0.0 || at >= start + len - WS_PAD {
        return None;
    }
    let i = (rel / Dim::of(horizontal).ws_cell) as usize;
    (i < count).then_some(i)
}

/// What a left click on `kind` does in the pill bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// The OS icon: the launcher (decspot).
    Launcher,
    /// A workspace pill: view it.
    Workspace(usize),
    /// A status icon or the window: open its popup (and keep it open).
    Popout(Tray),
    /// The quick-settings tile: toggle the Control Center panel.
    Control,
    /// The clock: the dashboard (decdash).
    Dashboard,
    /// The power button: the session menu (decsession).
    Session,
    /// The coffee cup: turn Stay Awake off (hover shows how long it was on; right click offers timed sessions).
    StayAwakeOff,
    /// The agents glyph: the process manager (decmon).
    Monitor,
    Nothing,
}

pub fn click_for(kind: Tray, workspace: Option<usize>) -> Click {
    match kind {
        Tray::Logo => Click::Launcher,
        Tray::Workspaces => workspace.map_or(Click::Nothing, Click::Workspace),
        Tray::Clock => Click::Dashboard,
        Tray::Power => Click::Session,
        Tray::Control => Click::Control,
        Tray::Awake => Click::StayAwakeOff,
        Tray::Agents => Click::Monitor,
        k if popout(k) => Click::Popout(k),
        _ => Click::Nothing,
    }
}

/// Items that open a popup when hovered or clicked.
pub fn popout(kind: Tray) -> bool {
    matches!(kind, Tray::Audio | Tray::Network | Tray::Bluetooth | Tray::Battery | Tray::Window | Tray::Layout | Tray::Weather | Tray::Media | Tray::Brightness)
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

/// Arguments that start the bottom half of a split layout (after the program name).
pub fn bottom_args() -> [&'static str; 2] {
    ["--role", "bottom"]
}

impl DecBar {
    /// Where this process draws the pill bar, or `None` when it draws the classic menu bar (the theme asks for it; never
    /// for an embedded status engine). `bar.position` comes with the live theme, so a change reaches here like a style switch.
    pub(super) fn place(&self) -> Option<Place> {
        if self.embedded {
            return None;
        }
        let p = self.panel();
        let skin = Skin::of(p.renderer);
        // the pill rail is always a rail; another bar becomes one on a side edge (on top / bottom it is its own strip)
        if skin != Skin::Pill && !p.edge.is_vertical() {
            return None;
        }
        let position = if p.split { BarPosition::Split } else { p.edge.position() };
        Place::of(position, self.process).map(|place| place.with_skin(skin))
    }

    /// True when this bar draws the pill layout.
    pub(super) fn pill_bar(&self) -> bool {
        self.place().is_some()
    }

    /// The surface on its edge (the left edge before there is a place).
    pub(super) fn v_geo(&self) -> Geo {
        let place = self.place();
        Geo::skinned(place.map_or(Anchor::Left, |p| p.edge), self.width as f32, self.height as f32, place.map_or(Skin::Pill, |p| p.skin))
    }

    pub(super) fn v_skin(&self) -> Skin {
        self.place().map_or(Skin::Pill, |p| p.skin)
    }

    fn v_part(&self) -> Part {
        self.place().map_or(Part::All, |p| p.part)
    }

    /// Keeps the Caps / Num Lock watcher running exactly while the lock-keys applet is on.
    pub(super) fn ensure_lock_watcher(&mut self) {
        let wanted = self.shows(crate::panel::AppletId::LockKeys);
        if !wanted {
            self.lock_watcher = None;
            return;
        }
        if self.lock_watcher.is_some() || cfg!(test) {
            return;
        }
        let Some(waker) = self.waker.clone() else { return };
        let changed = Arc::clone(&self.shared);
        self.lock_watcher = lock::spawn_watcher(self.locks.clone(), move || {
            changed.changed.store(true, Ordering::SeqCst);
            waker.wake();
        });
    }

    /// While the layout is split this (main) bar owns the bottom bar's process: starts `decbar --role bottom` when it is not
    /// running, and notices when it ended. The bottom process holds its own lock, so a second one just exits, and it leaves by
    /// itself when the layout stops being split. Returns true while the companion is wanted (the caller keeps polling).
    pub(super) fn ensure_bottom(&mut self) -> bool {
        let wanted = self.companion && self.process == Process::Main && self.place().is_some_and(|p| p.part == Part::Head);
        if let Some(child) = self.bottom_child.as_mut() {
            if !matches!(child.try_wait(), Ok(None)) {
                self.bottom_child = None;
                self.bottom_retry = Instant::now() + COMPANION_RETRY;
            }
        }
        if !wanted || self.bottom_child.is_some() || Instant::now() < self.bottom_retry {
            return wanted;
        }
        self.bottom_retry = Instant::now() + COMPANION_RETRY;
        self.bottom_spawns += 1;
        let exe = self.bottom_exe.clone().unwrap_or_else(|| crate::launch::sibling_or_path("decbar"));
        let child = std::process::Command::new(exe).args(bottom_args()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).spawn();
        self.bottom_child = child.ok();
        wanted
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

    /// The rail's content from the enabled applets, and what moved into the overflow cell.
    pub(super) fn v_fit(&self) -> (Content, Vec<Tray>) {
        use super::applets::{RAIL_HEAD, RAIL_STATUS, RAIL_TAIL, RAIL_TRAY};
        use crate::panel::AppletId as A;
        let geo = self.v_geo();
        let horiz = geo.horizontal();
        let text_max = Dim::of(horiz).win_text_max;
        let window = if self.shows(A::WindowTitle) && self.v_win.is_some() {
            if horiz {
                (2.0 * 8.0 + WIN_ICON + 8.0 + self.v_win_len.min(text_max)).ceil()
            } else {
                // upright: the icon and a short label under it
                8.0 + WIN_ICON + 6.0 + 12.0 + 8.0
            }
        } else {
            0.0
        };
        let workspaces = if self.shows(A::Workspaces) { self.v_workspaces().len() } else { 0 };
        let mut content = Content {
            head: self.applet_items(RAIL_HEAD, 0),
            workspaces,
            tasks: if self.shows(A::Tasks) { self.tasks.iter().map(|t| Tray::Task(t.key())).collect() } else { Vec::new() },
            window,
            tray: self.applet_items(RAIL_TRAY, 0),
            status: self.applet_items(RAIL_STATUS, 0),
            tail: self.applet_items(RAIL_TAIL, 0),
        };
        let controls = crate::theme::th().control_center;
        let active = self.shows(A::Tray) && self.indicators.items.iter().any(|item| item.status != crate::sni::Status::Passive);
        let part = self.v_part();
        let mut hidden = fit_overflow(&geo, part, &mut content);
        if active {
            let before: Vec<Tray> = content.tray.iter().chain(&content.status).copied().collect();
            let capacity = fit_indicator_content(&geo, part, &mut content, true, controls.enabled.then_some(controls.tiles));
            // what gave way for the application cells stays reachable from the overflow cell
            let folded: Vec<Tray> = before.into_iter().filter(|k| !content.tray.contains(k) && !content.status.contains(k)).collect();
            if !folded.is_empty() {
                hidden.extend(folded);
                if !content.status.contains(&Tray::More) {
                    content.status.push(Tray::More);
                }
            }
            let room = capacity.saturating_sub(usize::from(!hidden.is_empty() && !content.status.contains(&Tray::More)));
            if room == 0 {
                // no cell left for applications: the overflow list carries them
                hidden.push(Tray::Indicators);
                if !content.status.contains(&Tray::More) {
                    content.status.push(Tray::More);
                }
            } else {
                content.status.extend(self.indicator_kinds(room));
            }
        }
        // the window pill gives way to everything else; then it is in the list too
        if content.window > 0.0 && part != Part::Tail && geo.len() - fixed_len(&geo, part, &content) < Dim::of(horiz).win_min {
            hidden.push(Tray::Window);
            if !content.status.contains(&Tray::More) && part != Part::Head {
                content.status.push(Tray::More);
            }
        }
        (content, hidden)
    }

    fn v_content(&self) -> Content {
        self.v_fit().0
    }

    pub(super) fn v_hidden(&self) -> Vec<Tray> {
        self.v_fit().1
    }

    pub(super) fn v_layout(&self) -> Layout {
        layout(&self.v_geo(), self.v_part(), &self.v_content())
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
    pub(super) fn v_window_text(&self) -> String {
        match &self.v_win {
            Some((id, t)) if t.is_empty() => active::display_name(id),
            Some((_, t)) => t.clone(),
            None => String::new(),
        }
    }

    // ------------------------------------------------------------ geometry for the backend / input

    pub(super) fn v_spec(place: Place) -> crate::backend::SurfaceSpec {
        crate::backend::SurfaceSpec { anchor: place.edge, margin: 0, reserve: place.thickness() as i32, stretch: true, namespace: place.namespace() }
    }

    /// Top-left of the open Control Center panel: on the screen side of the strip, centred on its tile, kept on screen.
    pub(super) fn v_panel_origin(&self) -> (f32, f32) {
        let geo = self.v_geo();
        let tile = self.v_layout().item(Tray::Control).unwrap_or(VRect { x: geo.w / 2.0, y: geo.h / 2.0, w: 0.0, h: 0.0 });
        geo.popout_origin(tile, self.cc_width(), self.cc_visible_height())
    }

    /// The surface of the rail at `p`: [`Place::surface_size`], wide enough beside a side strip for the Control Center,
    /// whose width follows its look and the text scale (GNOME's at 200 % is wider than the usual popout room).
    pub(super) fn v_surface_size(&self, p: Place) -> (u32, u32) {
        let (w, h) = p.surface_size();
        if !p.edge.is_side() {
            return (w, h);
        }
        // the look's natural width; on a narrower output the backend gives less and the panel is fitted to what it got
        let need = (p.thickness() + POP_GAP + controlcenter::panel_width(self.cc_style()) + POP_MARGIN).ceil() as u32;
        (w.max(need), h)
    }

    /// Height the Control Center can have beside the rail: the surface's length less the margins (a side rail), or the
    /// room the surface keeps for popouts (a top or bottom rail). A taller panel is clipped and scrolls.
    pub(super) fn v_cc_room(&self) -> f32 {
        let geo = self.v_geo();
        match geo.edge {
            Anchor::Left | Anchor::Right => geo.h - 2.0 * POP_MARGIN,
            Anchor::Top | Anchor::Bottom => geo.h - geo.thickness() - POP_GAP - POP_MARGIN,
        }
    }

    /// Width the Control Center can have beside the rail: the rest of the surface on the screen side of a side rail (the
    /// backend keeps the surface on its output), the surface's length less margins along a top or bottom rail.
    pub(super) fn v_cc_room_w(&self) -> f32 {
        let geo = self.v_geo();
        if geo.w <= 0.0 {
            return f32::INFINITY;
        }
        match geo.edge {
            Anchor::Left | Anchor::Right => (geo.w - geo.thick - POP_GAP - POP_MARGIN).max(1.0),
            Anchor::Top | Anchor::Bottom => (geo.w - 2.0 * POP_MARGIN).max(1.0),
        }
    }

    pub(super) fn v_input_rects(&self) -> Vec<InputRect> {
        let geo = self.v_geo();
        let s = geo.strip();
        let mut rects = vec![InputRect { x: s.x as i32, y: s.y as i32, w: s.w.ceil() as i32, h: s.h.ceil() as i32 }];
        match self.open {
            Open::Control => {
                let (ox, oy) = self.v_panel_origin();
                rects.push(geo.reach(ox, oy, self.cc_width(), self.cc_visible_height()));
            }
            Open::Popup(_) => {
                if let Some(p) = &self.popup {
                    rects.push(geo.reach(p.x, p.y, p.w, p.h));
                } else if let Some(c) = &self.calendar {
                    rects.push(geo.reach(c.x, c.y, popup::Calendar::W, popup::Calendar::H));
                }
            }
            Open::Menu(_) | Open::None => {}
        }
        rects
    }

    /// Opens the popup of `kind` beside its icon (or window pill) on the screen side of the strip.
    pub(super) fn v_open_popup(&mut self, kind: Tray) {
        self.close();
        let layout = self.v_layout();
        let Some(item) = layout.item(kind).or_else(|| matches!(kind, Tray::Indicator(_)).then(|| layout.item(Tray::Indicators)).flatten()).or_else(|| layout.item(Tray::More)) else { return };
        if kind == Tray::Clock {
            let Some(today) = clock::today() else { return };
            let (x, y) = self.v_geo().popout_origin(item, popup::Calendar::W, popup::Calendar::H);
            self.calendar = Some(popup::Calendar::new(x, y, today));
            self.open = Open::Popup(kind);
            return;
        }
        let Some((rows, w, compact)) = self.build_popup(kind) else { return };
        let mut p = Popup::layout(rows, 0.0, 0.0, w, compact);
        (p.x, p.y) = self.v_geo().popout_origin(item, p.w, p.h);
        self.popup = Some(p);
        self.open = Open::Popup(kind);
        self.last_full_refresh = Instant::now();
        match kind {
            Tray::Audio => self.spawn_snapshot(true),
            Tray::Network | Tray::Bluetooth => self.spawn_lists(kind),
            Tray::Battery => self.spawn_snapshot(false),
            Tray::Weather if self.weather_stale() => self.spawn_weather(),
            _ => {}
        }
    }

    /// After a rebuild the popup may have grown: put it back beside its icon, on screen (a bottom bar's popup grows upwards).
    pub(super) fn v_clamp_popup(&mut self) {
        let Open::Popup(kind) = self.open else { return };
        let layout = self.v_layout();
        let item = layout.item(kind).or_else(|| matches!(kind, Tray::Indicator(_)).then(|| layout.item(Tray::Indicators)).flatten()).or_else(|| layout.item(Tray::More));
        let geo = self.v_geo();
        if let (Some(p), Some(item)) = (self.popup.as_mut(), item) {
            (p.x, p.y) = geo.popout_origin(item, p.w, p.h);
        }
    }

    /// `--png` for the pill bar: the whole surface (strip and popout room) at the height given by [`DecBar::with_height`] for a
    /// side bar, at `width` for a top / bottom one. `open` is `control` or the name of a popup (`audio`, `network`, `bluetooth`,
    /// `battery`, `window`, `layout`, `weather`).
    pub(super) fn v_render_png(&mut self, path: &str, width: u32, open: Option<&str>) -> Result<(), String> {
        let (sw, sh) = self.place().map_or((SURFACE_W, SURFACE_H), |p| self.v_surface_size(p));
        if self.v_geo().horizontal() {
            self.width = width.max(1);
            self.height = sh;
        } else {
            self.width = sw;
            if self.height == 0 {
                self.height = 900;
            }
        }
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
                    "weather" => Tray::Weather,
                    other => return Err(format!("unknown popup {other}")),
                };
                self.v_open_popup(kind);
            }
            None => {}
        }
        let mut pm = Pixmap::new(self.width, self.height.max(1)).ok_or("cannot allocate pixmap")?;
        self.draw(&mut pm);
        pm.save_png(path).map_err(|e| e.to_string())
    }

    // ------------------------------------------------------------------------------- input

    pub(super) fn v_target(&self, x: f32, y: f32) -> Option<(Tray, Option<usize>)> {
        let geo = self.v_geo();
        if !geo.strip().contains(x, y) {
            return None;
        }
        let lay = self.v_layout();
        let (kind, r) = lay.hit(x, y)?;
        let ws = (kind == Tray::Workspaces).then(|| workspace_hit(r, geo.horizontal(), self.v_workspaces().len(), x, y)).flatten();
        Some((kind, ws))
    }

    pub(super) fn v_motion(&mut self, x: f32, y: f32) -> bool {
        let mut redraw = false;
        self.leave_at = None;
        if let (Some(t), Open::Control) = (self.cc_drag, self.open) {
            let (ox, _) = self.v_panel_origin();
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
            let pill = self.v_skin() == Skin::Pill;
            if let Some(kind) = hover.filter(|k| popout(*k) || (pill && *k == Tray::Awake) || (!pill && matches!(k, Tray::Clock | Tray::AppMenu | Tray::Indicators))) {
                match self.open {
                    // sliding from one icon to the next switches at once, like sliding along the macOS menu bar
                    Open::Popup(cur) if cur != kind => {
                        self.v_open_popup(kind);
                    }
                    // Caelestia opens popouts on hover; the other styles on a click
                    Open::None if pill => self.v_hover_open = Some((kind, Instant::now())),
                    _ => {}
                }
            } else if pill
                && hover.is_some_and(|kind| self.open != Open::Popup(kind) && !(kind == Tray::Indicators && matches!(self.open, Open::Popup(Tray::Indicator(_)))))
                && matches!(self.open, Open::Popup(_))
            {
                // onto an item without a popup of its own: the popup was a hover and goes away
                self.close();
            }
        }
        match self.open {
            Open::Control => {
                let hover = self.cc_point(x, y).and_then(|(px, py)| controlcenter::hit(&self.cc_items, px, py)).map(|h| h.0);
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

    /// A press on a rail item other than the workspaces, the indicators and the special buttons.
    pub(super) fn v_press(&mut self, kind: Tray, ws: Option<usize>, x: f32, y: f32) -> bool {
        let skin = self.v_skin();
        let r = self.v_layout().item(kind).map_or((x, y, 0.0, 0.0), |r| (r.x, r.y, r.w, r.h));
        if skin != Skin::Pill {
            if let Some(i) = ws.filter(|_| kind == Tray::Workspaces) {
                self.close();
                self.v_switch_workspace(i);
                return true;
            }
            return self.applet_press(kind, r, Button::Left);
        }
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
                launch_ui(clock_dash_argv(self.place().unwrap_or(Place { edge: Anchor::Left, part: Part::All, skin: Skin::Pill })));
            }
            Click::Session => {
                self.close();
                launch_ui(vec![crate::launch::sibling_or_path("decsession")]);
            }
            Click::StayAwakeOff => {
                self.close();
                self.awake_off();
            }
            Click::Monitor => {
                self.close();
                let place = self.place().unwrap_or(Place { edge: Anchor::Left, part: Part::All, skin: Skin::Pill });
                self.process_monitor(monitor_placement(place, x, y));
            }
            // the items added with the registry behave alike in every style
            Click::Nothing if kind != Tray::Workspaces => return self.applet_press(kind, r, Button::Left),
            Click::Nothing => return self.close(),
        }
        true
    }

    pub(super) fn v_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        self.leave_at = None;
        if self.open != Open::None && !self.over_open_ui(x, y) {
            self.close();
            return true;
        }
        let target = self.v_target(x, y);
        if let Some((Tray::Indicator(key), _)) = target {
            return match self.indicator_press(key, button, x.round() as i32, y.round() as i32) {
                IndicatorPress::Ignored => false,
                IndicatorPress::Handled => true,
                IndicatorPress::Menu => {
                    self.v_open_popup(Tray::Indicator(key));
                    true
                }
            };
        }
        if matches!(target, Some((Tray::Indicators, _))) && matches!(button, Button::Left | Button::Right) {
            self.v_open_popup(Tray::Indicators);
            return true;
        }
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
        if button == Button::Right {
            if let Some((kind @ (Tray::Awake | Tray::Task(_)), _)) = target {
                self.v_open_popup(kind);
                return true;
            }
        }
        if button != Button::Left {
            return false;
        }
        if self.v_geo().strip().contains(x, y) {
            let Some((kind, ws)) = target else { return self.close() };
            self.v_hover_open = None;
            return self.v_press(kind, ws, x, y);
        }
        match self.open {
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
            Open::Menu(_) | Open::None => false,
        }
    }

    /// Views workspace `index`: rdwm's `view` over its IPC socket when it reports tags, else EWMH.
    pub(super) fn v_switch_workspace(&mut self, index: usize) {
        if self.layout_state().is_some_and(|l| !l.tags.is_empty()) {
            rdwm::send_action("view", &[&(index + 1).to_string()]);
        } else {
            self.active.switch_workspace(index);
        }
    }

    /// Wheel on the strip, as in caelestia: over the workspaces it steps through them, elsewhere the first half of the bar
    /// (upper / left) changes the volume and the second half the brightness.
    pub(super) fn v_scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let geo = self.v_geo();
        if !geo.strip().contains(x, y) {
            return false;
        }
        let target = self.v_target(x, y).map(|t| t.0);
        if let Some(Tray::Indicator(key)) = target {
            return self.indicator_scroll(key, dy);
        }
        // the other styles change what is under the pointer; Caelestia's whole strip is a volume / brightness wheel
        if self.v_skin() != Skin::Pill || matches!(target, Some(Tray::Brightness | Tray::Layout)) {
            return target.is_some_and(|k| self.applet_scroll(k, dy));
        }
        if target == Some(Tray::Workspaces) {
            rdwm::send_action(if dy > 0.0 { "viewnext" } else { "viewprev" }, &[]);
            return false;
        }
        let up = if dy > 0.0 { -1.0 } else { 1.0 };
        if target == Some(Tray::Audio) || geo.along(x, y) < geo.len() / 2.0 {
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

    /// Timers of the pill bar: the hover delay before a popup opens. Returns (redraw, time until the next one).
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

    /// `s` starting at `x`, vertically centred on `cy`.
    #[allow(clippy::too_many_arguments)]
    fn v_text_from(&mut self, pm: &mut Pixmap, s: &str, x: f32, cy: f32, px: f32, weight: Weight, rgb: (u8, u8, u8)) {
        let w = self.fonts.as_ref().map_or(0.0, |f| f.measure(s, px, weight));
        self.v_text(pm, s, x + w / 2.0, cy, px, weight, rgb, 1.0);
    }

    fn v_text_width(&self, s: &str, px: f32, weight: Weight) -> f32 {
        self.fonts.as_ref().map_or(0.0, |f| f.measure(s, px, weight))
    }

    fn v_symbol(&mut self, pm: &mut Pixmap, name: &str, size: f32, cx: f32, cy: f32, rgb: (u8, u8, u8)) {
        let px = (size * 2.0).ceil() as u32;
        if let Some(sym) = self.symbols.get(name, rgb, px) {
            crate::gfx::blit_scaled(pm, sym, (cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size, 1.0);
        }
    }

    /// An item's symbol in the rail's style: Material outlines on the Dux band (the SF-style symbol when the package font
    /// is missing), SF-style symbols otherwise.
    #[allow(clippy::too_many_arguments)]
    fn rail_symbol(&mut self, pm: &mut Pixmap, skin: Skin, kind: Tray, size: f32, cx: f32, cy: f32, rgb: (u8, u8, u8)) {
        if skin == Skin::Dux {
            self.dux_rail_symbol(pm, super::applets::material_symbol(kind), size, cx, cy, rgb);
        } else {
            self.v_symbol(pm, super::applets::classic_symbol(kind), size, cx, cy, rgb);
        }
    }

    fn dux_rail_symbol(&mut self, pm: &mut Pixmap, name: &str, size: f32, cx: f32, cy: f32, rgb: (u8, u8, u8)) {
        let px = size.round() as u32;
        if let Some(icon) = self.symbols.get_material(name, rgb, px) {
            crate::gfx::blit_scaled(pm, icon, (cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size, 1.0);
        } else {
            self.v_symbol(pm, name, size, cx, cy, rgb);
        }
    }

    pub(super) fn v_draw(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        (self.width, self.height) = crate::gfx::logical_size(pm);
        let geo = self.v_geo();
        let horiz = geo.horizontal();
        let strip = geo.strip();
        let skin = self.v_skin();
        let native_glass = th.glass_refraction > 0.0;
        let lay = self.v_layout();
        let snap = self.snapshot();
        // ink of text and symbols: the pill rail paints on its own pills, the other styles on the bar colour
        let text = if skin == Skin::Pill { th.text.t() } else { th.bar_text.t() };
        let radius = geo.pill_width() / 2.0;
        match skin {
            Skin::Pill => {
                if !native_glass {
                    crate::gfx::fill_rect(pm, strip.x, strip.y, strip.w, strip.h, th.background.color(th.bar_alpha));
                }
                for (_, r) in &lay.pills {
                    let rad = radius.min(r.w.min(r.h) / 2.0);
                    fill_rounded(pm, r.x, r.y, r.w, r.h, rad, th.bar.color(if native_glass { th.bar_alpha } else { 1.0 }));
                    stroke_rounded(pm, r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, rad - 0.5, th.outline_c(), 1.0);
                }
            }
            Skin::Classic => crate::gfx::fill_rect(pm, strip.x, strip.y, strip.w, strip.h, th.bar_c()),
            Skin::Floating => {
                let r = rail_body(&geo, 2.0);
                let rad = r.w.min(r.h) / 2.0;
                fill_rounded(pm, r.x, r.y, r.w, r.h, rad, th.bar_c());
                stroke_rounded(pm, r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, rad - 0.5, th.outline_c(), 1.0);
            }
            Skin::Dux => {
                // one rounded band with a tonal face per item, the Dux bar turned onto the side
                let band = rail_body(&geo, th.bar_layout.margin);
                if th.bar_layout.background == "band" {
                    fill_rounded(pm, band.x, band.y, band.w, band.h, th.bar_layout.band_radius.min(band.w.min(band.h) / 2.0), th.background.color(1.0));
                }
                for (_, r) in &lay.pills {
                    fill_rounded(pm, r.x, r.y, r.w, r.h, th.bar_layout.radius.min(r.w.min(r.h) / 2.0), th.bar_c());
                }
            }
        }

        for (kind, r) in &lay.items {
            let open = self.open == Open::Popup(*kind) || (*kind == Tray::Control && self.open == Open::Control);
            let hovered = self.v_hover == Some(*kind) && *kind != Tray::Workspaces;
            if open || hovered {
                let a = if open { 0.18 } else { 0.10 };
                let single = matches!(kind, Tray::Logo | Tray::Window | Tray::Clock | Tray::Power | Tray::AppMenu | Tray::ShowDesktop);
                if skin != Skin::Pill {
                    let ink = if skin == Skin::Pill { th.ink(a) } else { th.bar_text.color(a * 0.8) };
                    let rad = if skin == Skin::Dux { th.bar_layout.radius.min(r.w.min(r.h) / 2.0) } else { 5.0 };
                    fill_rounded(pm, r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, rad, ink);
                } else if single {
                    fill_rounded(pm, r.x, r.y, r.w, r.h, radius.min(r.w.min(r.h) / 2.0), th.ink(a));
                } else if horiz {
                    fill_rounded(pm, r.x + 1.0, r.y + 2.0, r.w - 2.0, r.h - 4.0, 12.0, th.ink(a));
                } else {
                    fill_rounded(pm, r.x + 2.0, r.y + 1.0, r.w - 4.0, r.h - 2.0, 12.0, th.ink(a));
                }
            }
            let (cx, cy) = (r.cx(), r.cy());
            match kind {
                Tray::Logo if self.draw_brand(pm, if skin == Skin::Pill { 22.0 } else { 18.0 }, cx, cy, if skin == Skin::Pill { th.accent.t() } else { text }) => {}
                Tray::Logo if skin != Skin::Pill => self.rail_symbol(pm, skin, *kind, 18.0, cx, cy, text),
                Tray::Logo => {
                    let name = self.logo_symbol.clone();
                    self.v_symbol(pm, &name, 22.0, cx, cy, th.accent.t());
                }
                Tray::AppMenu | Tray::Notifications | Tray::Clipboard | Tray::Brightness | Tray::ShowDesktop | Tray::Spotlight | Tray::Dash => self.rail_symbol(pm, skin, *kind, 17.0, cx, cy, text),
                Tray::Media => {
                    let playing = self.dux_player().is_some_and(|p| p.status == crate::dash::mpris::Status::Playing);
                    let name = if playing { "pause" } else { "play" };
                    if skin == Skin::Dux {
                        self.dux_rail_symbol(pm, name, 17.0, cx, cy, text);
                    } else {
                        self.v_symbol(pm, name, 17.0, cx, cy, text);
                    }
                }
                Tray::Cpu | Tray::Memory => {
                    let perf = self.dux_perf.lock().ok().and_then(|p| *p);
                    let value = perf.and_then(|p| if *kind == Tray::Cpu { p.0 } else { p.1 });
                    let label = if *kind == Tray::Cpu { "CPU" } else { "MEM" };
                    let pct = value.map_or_else(|| "—".to_string(), |v| format!("{:.0}", v * 100.0));
                    if horiz {
                        let lw = self.v_text_width(label, 9.0, Weight::Medium);
                        let vw = self.v_text_width(&pct, 11.0, Weight::Medium);
                        let x0 = cx - (lw + 3.0 + vw) / 2.0;
                        self.v_text(pm, label, x0 + lw / 2.0, cy, 9.0, Weight::Medium, text, 0.6);
                        self.v_text(pm, &pct, x0 + lw + 3.0 + vw / 2.0, cy, 11.0, Weight::Medium, text, 1.0);
                    } else {
                        self.v_text(pm, label, cx, cy - 8.0, 9.0, Weight::Medium, text, 0.6);
                        self.v_text(pm, &pct, cx, cy + 7.0, 11.0, Weight::Medium, text, 1.0);
                    }
                }
                Tray::Task(key) => {
                    let Some(t) = self.task(*key).cloned() else { continue };
                    let icon = self.v_icons.get(&t.app_id, 40).clone();
                    crate::gfx::blit_scaled(pm, &icon, cx - 10.0, cy - 10.0, 20.0, 20.0, 1.0);
                    if t.active() {
                        let (dx, dy, dw, dh) = if horiz { (cx - 4.0, r.bottom() - 4.0, 8.0, 2.0) } else { (r.x + 2.0, cy - 4.0, 2.0, 8.0) };
                        fill_rounded(pm, dx, dy, dw, dh, 1.0, th.accent.color(1.0));
                    }
                }
                Tray::More => {
                    for offset in [-5.0, 0.0, 5.0] {
                        let (dx, dy) = if horiz { (cx + offset, cy) } else { (cx, cy + offset) };
                        fill_rounded(pm, dx - 1.5, dy - 1.5, 3.0, 3.0, 1.5, color(text.0, text.1, text.2, 0.8));
                    }
                }
                Tray::Workspaces => self.v_draw_workspaces(pm, *r, horiz),
                Tray::Window => self.v_draw_window(pm, *r, horiz),
                Tray::Layout => {
                    if let Some(l) = self.layout_state() {
                        popup::draw_layout_icon(pm, l.canonical(), cx - 11.0, cy - 7.0, 22.0, 14.0, (text.0, text.1, text.2, 0.95));
                    }
                }
                Tray::Awake => {
                    let on = snap.awake;
                    self.v_symbol(pm, "awake", 16.0, cx, cy, if on { th.accent.t() } else { text });
                }
                Tray::Agents => self.v_symbol(pm, "agents", 16.0, cx, cy, text),
                Tray::Control => self.rail_symbol(pm, skin, *kind, 16.0, cx, cy, text),
                Tray::Lock => {
                    let on = self.locks.caps() || self.locks.num();
                    let label = match (self.locks.caps(), self.locks.num()) {
                        (true, true) => "A1",
                        (true, false) => "A",
                        (false, true) => "1",
                        // turned on explicitly while both are off: the inactive key
                        _ => "A",
                    };
                    if on {
                        fill_rounded(pm, cx - 10.0, cy - 8.0, 20.0, 16.0, 5.0, th.accent.color(1.0));
                        self.v_text(pm, label, cx, cy, 10.0, Weight::Bold, th.accent_text.t(), 1.0);
                    } else {
                        stroke_rounded(pm, cx - 9.5, cy - 7.5, 19.0, 15.0, 5.0, color(text.0, text.1, text.2, 0.45), 1.0);
                        self.v_text(pm, label, cx, cy, 10.0, Weight::Bold, text, 0.55);
                    }
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
                        self.v_draw_battery(pm, *r, b, horiz);
                    }
                }
                Tray::Clock if skin != Skin::Pill => {
                    // the other styles' clock: their own weight and colour, upright digits stacked on a side edge
                    let (hh, mm) = self.v_clock();
                    self.v_text(pm, &hh, cx, cy - 8.0, 13.0, Weight::Medium, text, 1.0);
                    self.v_text(pm, &mm, cx, cy + 8.0, 13.0, Weight::Regular, text, 1.0);
                }
                Tray::Clock => {
                    let (hh, mm) = self.v_clock();
                    if horiz {
                        // HH:MM on one line, the hour in the accent colour as the stacked clock has it
                        let (wh, wc, wm) = (self.v_text_width(&hh, 15.0, Weight::Bold), self.v_text_width(":", 15.0, Weight::Bold), self.v_text_width(&mm, 15.0, Weight::Medium));
                        let x0 = cx - (wh + wc + wm) / 2.0;
                        self.v_text(pm, &hh, x0 + wh / 2.0, cy, 15.0, Weight::Bold, th.accent.t(), 1.0);
                        self.v_text(pm, ":", x0 + wh + wc / 2.0, cy, 15.0, Weight::Bold, th.accent.t(), 1.0);
                        self.v_text(pm, &mm, x0 + wh + wc + wm / 2.0, cy, 15.0, Weight::Medium, text, 1.0);
                    } else {
                        self.v_text(pm, &hh, cx, cy - 11.0, 15.0, Weight::Bold, th.accent.t(), 1.0);
                        self.v_text(pm, &mm, cx, cy + 11.0, 15.0, Weight::Medium, text, 1.0);
                    }
                }
                Tray::Power if skin != Skin::Pill => self.rail_symbol(pm, skin, *kind, 17.0, cx, cy, text),
                Tray::Power => self.v_symbol(pm, "power", 18.0, cx, cy, th.danger.t()),
                Tray::Weather => {
                    let wx = self.shared.weather.lock().ok().and_then(|w| w.clone());
                    if let Some(w) = wx {
                        self.v_symbol(pm, &format!("wx:{:x}", weather::nerd_codepoint(w.code, w.night)), 17.0, cx, cy, text);
                    }
                }
                Tray::Indicator(key) => self.indicator_draw(pm, *key, cx, cy, 18.0),
                Tray::Indicators => {
                    for offset in [-5.0, 0.0, 5.0] {
                        fill_rounded(pm, cx + offset - 1.5, cy - 1.5, 3.0, 3.0, 1.5, th.text.color(1.0));
                    }
                }
            }
        }

        match self.open {
            Open::Control => {
                let (ox, oy) = self.v_panel_origin();
                self.debug_control_panel(self.width, self.height);
                let style = self.cc_style();
                let (clipped, width, height) = (self.cc_clipped(), self.cc_width(), self.cc_visible_height());
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
            Open::Menu(_) | Open::None => {}
        }
        let indicator_rects: Vec<_> = lay.items.iter().map(|(kind, r)| (*kind, (r.x, r.y, r.w, r.h))).collect();
        self.indicator_debug(&indicator_rects);
        if let Some((path, last)) = self.debug_tray.as_mut() {
            let text: String = lay.items.iter().map(|(k, r)| format!("{:?} {} {}\n", k, r.cx().round(), r.cy().round())).collect();
            if *last != text {
                let _ = std::fs::write(&*path, &text);
                *last = text;
            }
        }
    }

    /// (`HH`, `MM`) for the clock.
    fn v_clock(&self) -> (String, String) {
        let (h, m) = self.last_minute.unwrap_or((0, 0));
        clock_parts(h, m, crate::config::shell_settings().clock_hour12)
    }

    fn v_draw_workspaces(&mut self, pm: &mut Pixmap, pill: VRect, horiz: bool) {
        let th = crate::theme::th();
        let wss = self.v_workspaces();
        let cell = Dim::of(horiz).ws_cell;
        for (i, ws) in wss.iter().enumerate() {
            let along = WS_PAD + cell * (i as f32 + 0.5);
            let (cx, cy) = if horiz { (pill.x + along, pill.cy()) } else { (pill.cx(), pill.y + along) };
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
                // a 30 x 22 chip, its long side across the bar (a vertical bar) or along it (a horizontal one)
                let (cw, ch) = if horiz { (28.0, 22.0) } else { (30.0, 22.0) };
                fill_rounded(pm, cx - cw / 2.0, cy - ch / 2.0, cw, ch, 11.0, c);
            }
            if ws.active || ws.urgent || ws.occupied || hovered {
                self.v_text(pm, &label, cx, cy, 12.0, if ws.active { Weight::Bold } else { Weight::Medium }, fg, 1.0);
            } else {
                crate::gfx::fill_circle(pm, cx, cy, 2.2, th.ink(0.32));
            }
        }
    }

    fn v_draw_window(&mut self, pm: &mut Pixmap, r: VRect, horiz: bool) {
        let Some((id, _)) = self.v_win.clone() else { return };
        let th = crate::theme::th();
        let icon = self.v_icons.get(&id, (WIN_ICON * 2.0) as u32).clone();
        if horiz {
            // icon, then the title read along the bar
            crate::gfx::blit_scaled(pm, &icon, r.x + 8.0, r.cy() - WIN_ICON / 2.0, WIN_ICON, WIN_ICON, 1.0);
            let left = r.x + 8.0 + WIN_ICON + 8.0;
            let room = (r.right() - 8.0 - left).max(0.0);
            let shown = match &self.fonts {
                Some(f) => controlcenter::fit(&self.v_window_text(), room, TEXT_PX, Weight::Medium, f),
                None => return,
            };
            self.v_text_from(pm, &shown, left, r.cy(), TEXT_PX, Weight::Medium, th.text.t());
            return;
        }
        // a side bar keeps text upright: the icon, and under it as much of the app's name as the width takes
        crate::gfx::blit_scaled(pm, &icon, r.cx() - WIN_ICON / 2.0, r.y + 8.0, WIN_ICON, WIN_ICON, 1.0);
        let label = active::display_name(&id);
        let shown = match &self.fonts {
            Some(f) => controlcenter::fit(&label, r.w - 4.0, 10.0, Weight::Medium, f),
            None => return,
        };
        let ink = if self.v_skin() == Skin::Pill { th.text.t() } else { th.bar_text.t() };
        self.v_text(pm, &shown, r.cx(), r.y + 8.0 + WIN_ICON + 6.0 + 6.0, 10.0, Weight::Medium, ink, 0.9);
    }

    fn v_draw_battery(&mut self, pm: &mut Pixmap, r: VRect, b: &sys::Battery, horiz: bool) {
        let th = crate::theme::th();
        let body = th.text.color(0.75);
        let fill = if b.charging {
            th.ok.t()
        } else if b.percent <= 15 {
            th.danger.t()
        } else {
            th.text.t()
        };
        let pct = b.percent.min(100) as f32 / 100.0;
        let label = format!("{}", b.percent);
        if horiz {
            // the battery lying down, its percentage to the right
            let (bw, bh) = (20.0, 11.0);
            let tw = self.v_text_width(&label, 10.0, Weight::Medium);
            let x0 = (r.cx() - (bw + 2.0 + 4.0 + tw) / 2.0).round();
            let cy = r.cy();
            stroke_rounded(pm, x0 + 0.5, cy - bh / 2.0 + 0.5, bw - 1.0, bh - 1.0, 3.0, body, 1.0);
            crate::gfx::fill_rect(pm, x0 + bw, cy - 2.5, 2.0, 5.0, body);
            let fw = ((bw - 5.0) * pct).max(1.0);
            fill_rounded(pm, x0 + 2.0, cy - bh / 2.0 + 2.0, fw, bh - 4.0, 1.5, color(fill.0, fill.1, fill.2, 0.95));
            self.v_text(pm, &label, x0 + bw + 2.0 + 4.0 + tw / 2.0, cy, 10.0, Weight::Medium, th.text.t(), 0.9);
            return;
        }
        let (cx, top) = (r.cx(), r.y + 4.0);
        let (bw, bh) = (12.0, 20.0);
        stroke_rounded(pm, cx - bw / 2.0 + 0.5, top + 2.5, bw - 1.0, bh - 1.0, 3.0, body, 1.0);
        crate::gfx::fill_rect(pm, cx - 2.5, top, 5.0, 2.0, body);
        let fh = ((bh - 5.0) * pct).max(1.0);
        fill_rounded(pm, cx - bw / 2.0 + 2.0, top + 2.5 + bh - 1.0 - 2.0 - fh, bw - 4.0, fh, 1.5, color(fill.0, fill.1, fill.2, 0.95));
        self.v_text(pm, &label, cx, top + bh + 9.0, 10.0, Weight::Medium, th.text.t(), 0.9);
    }
}

/// The strip less `margin` on every side: the floating rail's pill and the Dux band.
fn rail_body(geo: &Geo, margin: f32) -> VRect {
    let s = geo.strip();
    VRect { x: s.x + margin, y: s.y + margin, w: (s.w - 2.0 * margin).max(0.0), h: (s.h - 2.0 * margin).max(0.0) }
}

/// Top y (or x) that keeps a box `size` long inside a screen `screen` long, `POP_MARGIN` from the edges (the margin wins on a short screen).
pub fn clamp_in(pos: f32, size: f32, screen: f32) -> f32 {
    pos.min(screen - size - POP_MARGIN).max(POP_MARGIN)
}

/// True when a bar runs along the top edge: the top bar, or the top half of a split layout (seen from its bottom half).
fn top_bar_exists(place: Place) -> bool {
    place.edge == Anchor::Top || (place.edge == Anchor::Bottom && place.part == Part::Tail)
}

/// `decmon --popup` arguments that hang the process manager from the bar's own edge, next to the item clicked at
/// (`x`, `y`) on the strip: under a bar along the top, over one along the bottom, beside one at a side.
fn monitor_placement(place: Place, x: f32, y: f32) -> Vec<String> {
    let (edge, along) = match place.edge {
        Anchor::Top => ("top", x),
        Anchor::Bottom => ("bottom", x),
        Anchor::Left => ("left", y),
        Anchor::Right => ("right", y),
    };
    let gap = place.thickness() as i32 + 10;
    vec!["--edge".into(), edge.into(), "--top".into(), gap.to_string(), "--x".into(), (along.round() as i32).to_string()]
}

/// `decdash` for the clock: hung below a bar along the top edge, else near the top edge of the screen.
fn clock_dash_argv(place: Place) -> Vec<String> {
    let top = 10 + if top_bar_exists(place) { HORIZONTAL_THICK as i32 } else { 0 };
    vec![crate::launch::sibling_or_path("decdash"), "--top".into(), top.to_string()]
}

#[cfg(test)]
thread_local! {
    /// What a test's presses started (unit tests never start programs).
    pub(super) static LAUNCHED: std::cell::RefCell<Vec<Vec<String>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Starts a UI program (dashboard, session menu). In dry-run mode (tests) the command is only recorded.
pub(super) fn launch_ui(argv: Vec<String>) {
    #[cfg(test)]
    LAUNCHED.with(|l| l.borrow_mut().push(argv.clone()));
    if cfg!(test) && !sys::is_dry_run() {
        return;
    }
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
        Content { workspaces: 9, window: 140.0, tray: vec![Tray::Layout, Tray::Control], status: vec![Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery], ..Content::default() }
    }

    /// The surface the backend gives a bar on `edge` of a 1600 x 900 monitor.
    fn geo_on(edge: Anchor) -> Geo {
        if edge.is_side() {
            Geo::new(edge, SURFACE_W as f32, 900.0)
        } else {
            Geo::new(edge, 1600.0, SURFACE_H as f32)
        }
    }

    const EDGES: [Anchor; 4] = [Anchor::Left, Anchor::Right, Anchor::Top, Anchor::Bottom];

    /// (along, across) of a surface rectangle: the frame the layout is written in, whichever the edge.
    fn along(g: &Geo, r: &VRect) -> (f32, f32) {
        if g.horizontal() {
            (r.x, r.right())
        } else {
            (r.y, r.bottom())
        }
    }

    #[test]
    fn the_surface_leaves_room_for_the_widest_panel() {
        const { assert!(POPOUT_W >= POP_GAP + controlcenter::PANEL_W + 8.0) };
        assert_eq!(SURFACE_W as f32, THICK + POPOUT_W);
        assert_eq!(SURFACE_H as f32, HORIZONTAL_THICK + POPOUT_H);
        assert_eq!(PILL_W, 40.0);
        assert_eq!(THICK, 52.0);
    }

    #[test]
    fn the_process_manager_hangs_from_the_bars_own_edge() {
        let args = |edge, x, y| monitor_placement(Place { edge, part: Part::All, skin: Skin::Pill }, x, y).join(" ");
        let gap = HORIZONTAL_THICK as i32 + 10;
        assert_eq!(args(Anchor::Bottom, 900.0, 1400.0), format!("--edge bottom --top {gap} --x 900"));
        assert_eq!(args(Anchor::Top, 900.0, 20.0), format!("--edge top --top {gap} --x 900"));
        let gap = THICK as i32 + 10;
        assert_eq!(args(Anchor::Left, 20.0, 300.0), format!("--edge left --top {gap} --x 300"), "beside a side bar the position is along y");
        assert_eq!(args(Anchor::Right, 20.0, 300.0), format!("--edge right --top {gap} --x 300"));
    }

    #[test]
    fn places_follow_the_position_and_the_process() {
        use BarPosition as P;
        let at = |p, pr| Place::of(p, pr).map(|p| (p.edge, p.part));
        assert_eq!(at(P::Left, Process::Main), Some((Anchor::Left, Part::All)));
        assert_eq!(at(P::Right, Process::Main), Some((Anchor::Right, Part::All)));
        assert_eq!(at(P::Top, Process::Main), Some((Anchor::Top, Part::All)));
        assert_eq!(at(P::Bottom, Process::Main), Some((Anchor::Bottom, Part::All)));
        assert_eq!(at(P::Split, Process::Main), Some((Anchor::Top, Part::Head)), "the main bar is the top half");
        assert_eq!(at(P::Split, Process::Bottom), Some((Anchor::Bottom, Part::Tail)));
        for p in [P::Left, P::Right, P::Top, P::Bottom] {
            assert_eq!(at(p, Process::Bottom), None, "{p:?}: the bottom process has nothing to draw unless split");
        }
        assert_eq!(Place::of(P::Split, Process::Main).unwrap().namespace(), "decebal-bar");
        assert_eq!(Place::of(P::Split, Process::Bottom).unwrap().namespace(), "decebal-bar-bottom");
        assert_eq!((Process::parse("bottom"), Process::parse("main"), Process::parse("x")), (Some(Process::Bottom), Some(Process::Main), None));
        assert_ne!(Process::Main.lock_name(), Process::Bottom.lock_name(), "one lock per role");
    }

    #[test]
    fn the_strip_hugs_its_edge_and_is_the_only_reserved_part() {
        let s = |e| geo_on(e).strip();
        assert_eq!(s(Anchor::Left), VRect { x: 0.0, y: 0.0, w: THICK, h: 900.0 });
        assert_eq!(s(Anchor::Right), VRect { x: SURFACE_W as f32 - THICK, y: 0.0, w: THICK, h: 900.0 });
        assert_eq!(s(Anchor::Top), VRect { x: 0.0, y: 0.0, w: 1600.0, h: HORIZONTAL_THICK });
        assert_eq!(s(Anchor::Bottom), VRect { x: 0.0, y: SURFACE_H as f32 - HORIZONTAL_THICK, w: 1600.0, h: HORIZONTAL_THICK });
        assert_eq!(geo_on(Anchor::Left).len(), 900.0);
        assert_eq!(geo_on(Anchor::Top).len(), 1600.0);
    }

    #[test]
    fn every_pill_sits_inside_the_strip_and_the_screen_and_none_overlap_for_every_edge() {
        for edge in EDGES {
            let g = geo_on(edge);
            let strip = g.strip();
            let lay = layout(&g, Part::All, &full());
            assert_eq!(lay.pills.len(), 7, "{edge:?}");
            let mut last_end = 0.0;
            for (_, r) in &lay.pills {
                // across: inside the strip, PILL_X from its sides
                let (c0, c1) = if g.horizontal() { (r.y, r.bottom()) } else { (r.x, r.right()) };
                let (s0, s1) = if g.horizontal() { (strip.y, strip.bottom()) } else { (strip.x, strip.right()) };
                assert!(c0 >= s0 + g.pill_inset() - 0.001 && c1 <= s1 - g.pill_inset() + 0.001, "{edge:?}: inside the strip: {r:?}");
                // along: inside the bar, separated by the gap, in reading order
                let (a0, a1) = along(&g, r);
                assert!(a0 >= EDGE - 0.001 && a1 <= g.len() - EDGE + 0.001, "{edge:?}: {r:?}");
                assert!(a0 >= last_end + GAP - 0.001 || last_end == 0.0, "{edge:?}: pills are separated by the gap: {r:?} after {last_end}");
                last_end = a1;
            }
        }
    }

    #[test]
    fn the_order_is_logo_workspaces_window_tray_status_clock_power_on_every_edge() {
        for edge in EDGES {
            let g = geo_on(edge);
            let lay = layout(&g, Part::All, &full());
            let order: Vec<Pill> = lay.pills.iter().map(|p| p.0).collect();
            assert_eq!(order, [Pill::Logo, Pill::Workspaces, Pill::Window, Pill::Tray, Pill::Status, Pill::Clock, Pill::Power], "{edge:?}");
            let kinds: Vec<Tray> = lay.items.iter().map(|i| i.0).collect();
            assert_eq!(
                kinds,
                [Tray::Logo, Tray::Workspaces, Tray::Window, Tray::Layout, Tray::Control, Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery, Tray::Clock, Tray::Power],
                "{edge:?}"
            );
            let starts: Vec<f32> = lay.items.iter().map(|i| along(&g, &i.1).0).collect();
            assert!(starts.windows(2).all(|w| w[0] < w[1]), "{edge:?}: items run down (or across) the screen: {starts:?}");
        }
    }

    #[test]
    fn horizontal_pills_are_slimmer_but_keep_their_content_and_hit_targets() {
        let v = layout(&geo_on(Anchor::Left), Part::All, &full());
        for edge in [Anchor::Top, Anchor::Bottom] {
            let g = geo_on(edge);
            let h = layout(&g, Part::All, &full());
            assert_eq!(g.strip().h, 40.0);
            for (kind, r) in &h.pills {
                assert_eq!(r.h, 32.0, "{edge:?} {kind:?}");
                assert!(r.y >= g.strip().y && r.bottom() <= g.strip().bottom());
            }
            for (kind, r) in &h.items {
                assert_eq!(h.hit(r.cx(), r.cy()).map(|h| h.0), Some(*kind));
            }
            assert_eq!(h.pill(Pill::Clock).unwrap().w, Dim::of(true).clock_len);
        }
        assert_eq!(geo_on(Anchor::Left).strip().w, 52.0);
        assert_eq!(v.pill(Pill::Logo).unwrap().w, 40.0);
    }

    #[test]
    fn the_head_hangs_from_the_start_and_the_tail_stands_on_the_end_for_every_edge() {
        for edge in EDGES {
            let g = geo_on(edge);
            let lay = layout(&g, Part::All, &full());
            let at = |p| along(&g, &lay.pill(p).unwrap());
            assert_eq!(at(Pill::Logo).0, EDGE, "{edge:?}");
            assert_eq!(at(Pill::Workspaces).0, EDGE + g.pill_width() + GAP, "{edge:?}");
            assert_eq!(at(Pill::Power).1, g.len() - EDGE, "{edge:?}");
            assert_eq!(at(Pill::Clock).1, at(Pill::Power).0 - GAP, "{edge:?}");
            assert_eq!(lay.pill(Pill::Workspaces).map(|r| if g.horizontal() { r.w } else { r.h }), Some(workspaces_len(9, g.horizontal())), "{edge:?}");
        }
        // a longer screen moves the tail with it, the head stays
        let (short, long) = (Geo::new(Anchor::Top, 1600.0, SURFACE_H as f32), Geo::new(Anchor::Top, 2000.0, SURFACE_H as f32));
        let (a, b) = (layout(&short, Part::All, &full()), layout(&long, Part::All, &full()));
        assert_eq!(a.pill(Pill::Logo), b.pill(Pill::Logo));
        assert_eq!(b.pill(Pill::Power).unwrap().x - a.pill(Pill::Power).unwrap().x, 400.0);
    }

    #[test]
    fn mirrored_edges_swap_sides_not_order() {
        let (l, r) = (layout(&geo_on(Anchor::Left), Part::All, &full()), layout(&geo_on(Anchor::Right), Part::All, &full()));
        let shift = SURFACE_W as f32 - THICK;
        for ((_, a), (_, b)) in l.items.iter().zip(&r.items) {
            assert_eq!((b.x - a.x, b.y, b.w, b.h), (shift, a.y, a.w, a.h));
        }
        let (t, b) = (layout(&geo_on(Anchor::Top), Part::All, &full()), layout(&geo_on(Anchor::Bottom), Part::All, &full()));
        let shift = SURFACE_H as f32 - HORIZONTAL_THICK;
        for ((_, a), (_, c)) in t.items.iter().zip(&b.items) {
            assert_eq!((c.y - a.y, c.x, c.w, c.h), (shift, a.x, a.w, a.h));
        }
    }

    #[test]
    fn the_window_pill_is_centred_in_the_free_space_and_capped_by_it() {
        for edge in EDGES {
            let g = geo_on(edge);
            let lay = layout(&g, Part::All, &full());
            let (ws, win, tray) = (along(&g, &lay.pill(Pill::Workspaces).unwrap()), along(&g, &lay.pill(Pill::Window).unwrap()), along(&g, &lay.pill(Pill::Tray).unwrap()));
            let (above, below) = (win.0 - ws.1, tray.0 - win.1);
            assert!((above - below).abs() < 0.01 && above >= GAP - 0.01, "{edge:?} centred: {above} vs {below}");
            assert_eq!(win.1 - win.0, 140.0, "{edge:?}");
            // asks for more than there is: takes what is free, between the gaps
            let big = layout(&g, Part::All, &Content { window: 5000.0, ..full() });
            let (ws, win, tray) = (along(&g, &big.pill(Pill::Workspaces).unwrap()), along(&g, &big.pill(Pill::Window).unwrap()), along(&g, &big.pill(Pill::Tray).unwrap()));
            assert_eq!((win.0, win.1), (ws.1 + GAP, tray.0 - GAP), "{edge:?}");
        }
    }

    #[test]
    fn the_window_pill_goes_when_there_is_no_room_or_no_window() {
        for edge in [Anchor::Left, Anchor::Top] {
            let horiz = !edge.is_side();
            let g = |len: f32| if horiz { Geo::new(edge, len, SURFACE_H as f32) } else { Geo::new(edge, SURFACE_W as f32, len) };
            assert!(layout(&g(900.0), Part::All, &Content { window: 0.0, ..full() }).pill(Pill::Window).is_none());
            // a short bar: the stacks fill it, nothing is left for the window
            let c = full();
            let d = Dim::of(horiz);
            let fixed = EDGE
                + g(1000.0).pill_width()
                + GAP
                + workspaces_len(9, horiz)
                + GAP
                + group_len(&c.tray, horiz)
                + GAP
                + group_len(&c.status, horiz)
                + GAP
                + d.clock_len
                + GAP
                + g(1000.0).pill_width()
                + EDGE;
            assert!(layout(&g(fixed + d.win_min - 1.0), Part::All, &c).pill(Pill::Window).is_none(), "{edge:?}");
            assert!(layout(&g(fixed + d.win_min + 2.0 * GAP), Part::All, &c).pill(Pill::Window).is_some(), "{edge:?}");
            // no workspaces (not rdwm): the pill is simply absent
            assert!(layout(&g(900.0), Part::All, &Content { workspaces: 0, ..full() }).pill(Pill::Workspaces).is_none());
        }
    }

    #[test]
    fn a_split_layout_divides_the_pills_between_the_two_bars() {
        let top = Geo::new(Anchor::Top, 1600.0, SURFACE_H as f32);
        let bottom = Geo::new(Anchor::Bottom, 1600.0, SURFACE_H as f32);
        let head = layout(&top, Part::Head, &full());
        let tail = layout(&bottom, Part::Tail, &full());
        let kinds = |l: &Layout| l.pills.iter().map(|p| p.0).collect::<Vec<_>>();
        assert_eq!(kinds(&head), [Pill::Logo, Pill::Workspaces, Pill::Window], "the top bar: OS icon, workspaces, window title");
        assert_eq!(kinds(&tail), [Pill::Tray, Pill::Status, Pill::Clock, Pill::Power], "the bottom bar: tray, status, clock, power");
        // a head-only bar puts the window title right after the workspaces; the tail stands on the right end
        let (ws, win) = (head.pill(Pill::Workspaces).unwrap(), head.pill(Pill::Window).unwrap());
        assert_eq!(win.x, ws.right() + GAP);
        assert_eq!(win.w, 140.0);
        assert_eq!(tail.pill(Pill::Power).unwrap().right(), 1600.0 - EDGE);
        // the tail keeps its place on the strip of the bottom edge
        let strip = bottom.strip();
        for (_, r) in &tail.pills {
            assert!(r.y >= strip.y + bottom.pill_inset() - 0.001 && r.bottom() <= strip.bottom() - bottom.pill_inset() + 0.001);
        }
        // together they are the single bar's items (and a lone bar has no gaps at the join)
        let all = layout(&top, Part::All, &full());
        let mut joined: Vec<Tray> = head.items.iter().chain(&tail.items).map(|i| i.0).collect();
        joined.sort_by_key(|k| format!("{k:?}"));
        let mut every: Vec<Tray> = all.items.iter().map(|i| i.0).collect();
        every.sort_by_key(|k| format!("{k:?}"));
        assert_eq!(joined, every);
        // without a focused window or workspaces the top bar is just the OS icon
        assert_eq!(kinds(&layout(&top, Part::Head, &Content { window: 0.0, workspaces: 0, ..full() })), [Pill::Logo]);
    }

    #[test]
    fn icons_fill_their_pill_exactly_and_the_battery_is_longer() {
        for edge in EDGES {
            let g = geo_on(edge);
            let lay = layout(&g, Part::All, &full());
            let status = along(&g, &lay.pill(Pill::Status).unwrap());
            let d = Dim::of(g.horizontal());
            assert_eq!(status.1 - status.0, 3.0 * ICON_CELL + d.batt_cell + 2.0 * CELL_PAD, "{edge:?}");
            let cells: Vec<(f32, f32)> = lay.items.iter().filter(|i| matches!(i.0, Tray::Audio | Tray::Network | Tray::Bluetooth | Tray::Battery)).map(|i| along(&g, &i.1)).collect();
            assert_eq!(cells[0].0, status.0 + CELL_PAD, "{edge:?}");
            for pair in cells.windows(2) {
                assert_eq!(pair[0].1, pair[1].0, "{edge:?}");
            }
            assert_eq!(cells[3].1 - cells[3].0, d.batt_cell);
            assert_eq!(cells[3].1, status.1 - CELL_PAD);
        }
    }

    #[test]
    fn hit_testing_finds_each_item_and_nothing_in_the_gaps_for_every_edge() {
        for edge in EDGES {
            let g = geo_on(edge);
            let lay = layout(&g, Part::All, &full());
            let strip = g.strip();
            for (k, r) in &lay.items {
                assert_eq!(lay.hit(r.cx(), r.cy()).map(|h| h.0), Some(*k), "{edge:?}");
                assert_eq!(lay.hit(r.x + 1.0, r.y + 1.0).map(|h| h.0), Some(*k), "{edge:?} {k:?} top-left corner");
                assert!(strip.contains(r.cx(), r.cy()), "{edge:?}: {k:?} lies on the strip");
            }
            let logo = lay.pill(Pill::Logo).unwrap();
            let in_gap = if g.horizontal() { (logo.right() + GAP / 2.0, logo.cy()) } else { (logo.cx(), logo.bottom() + GAP / 2.0) };
            assert_eq!(lay.hit(in_gap.0, in_gap.1), None, "{edge:?}: the gap between pills");
            let margin = if g.horizontal() { (logo.cx(), strip.y + 2.0) } else { (strip.x + 2.0, logo.cy()) };
            assert_eq!(lay.hit(margin.0, margin.1), None, "{edge:?}: the strip margin beside the pills");
            let beyond = match edge {
                Anchor::Left => (THICK + 5.0, logo.cy()),
                Anchor::Right => (strip.x - 5.0, logo.cy()),
                Anchor::Top => (logo.cx(), THICK + 5.0),
                Anchor::Bottom => (logo.cx(), strip.y - 5.0),
            };
            assert_eq!(lay.hit(beyond.0, beyond.1), None, "{edge:?}: on the popout side of the strip");
        }
    }

    #[test]
    fn workspace_pills_hit_test_by_cell() {
        for horiz in [false, true] {
            let cell = Dim::of(horiz).ws_cell;
            let len = workspaces_len(9, horiz);
            let pill = if horiz { VRect { x: 100.0, y: PILL_X, w: len, h: PILL_W } } else { VRect { x: PILL_X, y: 100.0, w: PILL_W, h: len } };
            let (start, mid) = (100.0, if horiz { pill.cy() } else { pill.cx() });
            let at = |t: f32| if horiz { workspace_hit(pill, true, 9, t, mid) } else { workspace_hit(pill, false, 9, mid, t) };
            assert_eq!(at(start), None, "{horiz}: the pill's start padding");
            assert_eq!(at(start + WS_PAD), Some(0));
            assert_eq!(at(start + WS_PAD + cell - 0.1), Some(0));
            assert_eq!(at(start + WS_PAD + cell), Some(1));
            for i in 0..9 {
                assert_eq!(at(start + WS_PAD + cell * (i as f32 + 0.5)), Some(i));
            }
            assert_eq!(at(start + len - WS_PAD / 2.0), None, "the end padding");
            assert_eq!(at(start + len + 1.0), None);
            // fewer cells than the pill is long for (the list shrank): nothing past the last one
            let few = |t: f32| if horiz { workspace_hit(pill, true, 4, t, mid) } else { workspace_hit(pill, false, 4, mid, t) };
            assert_eq!(few(start + WS_PAD + cell * 5.5), None);
        }
    }

    #[test]
    fn popouts_open_on_the_screen_side_of_the_strip_and_stay_on_the_surface() {
        let item_on = |g: &Geo| layout(g, Part::All, &full()).item(Tray::Audio).unwrap();
        let (pw, ph) = (220.0, 180.0);
        // left: to the right of the strip, level with the icon
        let g = geo_on(Anchor::Left);
        let it = item_on(&g);
        let (x, y) = g.popout_origin(it, pw, ph);
        assert_eq!(x, THICK + POP_GAP);
        assert!((y + ph / 2.0 - it.cy()).abs() < 0.01);
        // right: leftwards, its right edge a gap from the strip
        let g = geo_on(Anchor::Right);
        let it = item_on(&g);
        let (x, y) = g.popout_origin(it, pw, ph);
        assert_eq!(x + pw, g.strip().x - POP_GAP, "opens leftwards");
        assert!((y + ph / 2.0 - it.cy()).abs() < 0.01);
        // top: downwards, centred on the icon
        let g = geo_on(Anchor::Top);
        let it = item_on(&g);
        let (x, y) = g.popout_origin(it, pw, ph);
        assert_eq!(y, HORIZONTAL_THICK + POP_GAP, "opens downwards");
        assert!((x + pw / 2.0 - it.cx()).abs() < 0.01);
        // bottom: upwards, its bottom edge a gap above the strip
        let g = geo_on(Anchor::Bottom);
        let it = item_on(&g);
        let (x, y) = g.popout_origin(it, pw, ph);
        assert_eq!(y + ph, g.strip().y - POP_GAP, "opens upwards");
        assert!((x + pw / 2.0 - it.cx()).abs() < 0.01);
        // an icon at the very end of the bar: the box is pulled back inside the surface, with the margin
        for edge in EDGES {
            let g = geo_on(edge);
            let power = layout(&g, Part::All, &full()).item(Tray::Power).unwrap();
            let (x, y) = g.popout_origin(power, pw, ph);
            assert!(x >= POP_MARGIN - 0.001 && x + pw <= g.w - POP_MARGIN + 0.001 && y >= POP_MARGIN - 0.001 && y + ph <= g.h - POP_MARGIN + 0.001, "{edge:?}: ({x}, {y})");
        }
    }

    #[test]
    fn the_input_region_of_a_popout_reaches_back_to_the_strip() {
        for edge in EDGES {
            let g = geo_on(edge);
            let (pw, ph) = (300.0, 200.0);
            let it = layout(&g, Part::All, &full()).item(Tray::Network).unwrap();
            let (x, y) = g.popout_origin(it, pw, ph);
            let r = g.reach(x, y, pw, ph);
            let (rx0, ry0, rx1, ry1) = (r.x as f32, r.y as f32, (r.x + r.w) as f32, (r.y + r.h) as f32);
            // covers the box, and touches the strip on its facing side
            assert!(rx0 <= x && ry0 <= y && rx1 >= x + pw && ry1 >= y + ph, "{edge:?}: {r:?}");
            let s = g.strip();
            match edge {
                Anchor::Left => assert_eq!(rx0, s.right()),
                Anchor::Right => assert_eq!(rx1, s.x),
                Anchor::Top => assert_eq!(ry0, s.bottom()),
                Anchor::Bottom => assert_eq!(ry1, s.y),
            }
        }
    }

    #[cfg(feature = "x11")]
    #[test]
    fn the_surfaces_and_struts_of_every_position() {
        use crate::backend::x11::{place, strut_partial, Rect};
        let mon = Rect { x: 0, y: 0, w: 1600, h: 900 };
        let strut = |p: BarPosition, process| {
            let pl = Place::of(p, process).unwrap();
            let spec = DecBar::v_spec(pl);
            let size = if pl.edge.is_side() { (SURFACE_W, 0) } else { (0, SURFACE_H) };
            let win = place(&spec, size, mon);
            (win, strut_partial(&spec, win, mon, (1600, 900)).unwrap())
        };
        let (win, s) = strut(BarPosition::Left, Process::Main);
        assert_eq!((win, s), ((0, 0, SURFACE_W, 900), [52, 0, 0, 0, 0, 899, 0, 0, 0, 0, 0, 0]));
        let (win, s) = strut(BarPosition::Right, Process::Main);
        assert_eq!((win, s), ((1600 - SURFACE_W as i32, 0, SURFACE_W, 900), [0, 52, 0, 0, 0, 0, 0, 899, 0, 0, 0, 0]));
        let (win, s) = strut(BarPosition::Top, Process::Main);
        assert_eq!((win, s), ((0, 0, 1600, SURFACE_H), [0, 0, 40, 0, 0, 0, 0, 0, 0, 1599, 0, 0]));
        let (win, s) = strut(BarPosition::Bottom, Process::Main);
        assert_eq!((win, s), ((0, 900 - SURFACE_H as i32, 1600, SURFACE_H), [0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 1599]));
        // split: two surfaces, two struts, one per half
        assert_eq!(strut(BarPosition::Split, Process::Main).1, [0, 0, 40, 0, 0, 0, 0, 0, 0, 1599, 0, 0]);
        assert_eq!(strut(BarPosition::Split, Process::Bottom).1, [0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 1599]);
        assert_eq!(DecBar::v_spec(Place::of(BarPosition::Split, Process::Bottom).unwrap()).namespace, "decebal-bar-bottom");
    }

    #[test]
    fn clicks_map_to_the_widgets() {
        assert_eq!(click_for(Tray::Logo, None), Click::Launcher);
        assert_eq!(click_for(Tray::Clock, None), Click::Dashboard);
        assert_eq!(click_for(Tray::Power, None), Click::Session);
        assert_eq!(click_for(Tray::Control, None), Click::Control);
        assert_eq!(click_for(Tray::Workspaces, Some(4)), Click::Workspace(4));
        assert_eq!(click_for(Tray::Workspaces, None), Click::Nothing);
        for k in [Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery, Tray::Window, Tray::Layout, Tray::Weather] {
            assert_eq!(click_for(k, None), Click::Popout(k));
        }
        assert_eq!(click_for(Tray::Awake, None), Click::StayAwakeOff);
        assert_eq!(click_for(Tray::Agents, None), Click::Monitor);
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
        assert_eq!(clamp_in(-50.0, 100.0, 900.0), POP_MARGIN);
        assert_eq!(clamp_in(300.0, 100.0, 900.0), 300.0);
        assert_eq!(clamp_in(880.0, 100.0, 900.0), 900.0 - 100.0 - POP_MARGIN);
        assert_eq!(clamp_in(10.0, 2000.0, 900.0), POP_MARGIN, "taller than the screen: the top edge wins");
    }

    // ------------------------------------------------------------------ the bar itself

    /// A pill bar at `position`, sized as the backend would size its surface.
    fn bar_at(position: BarPosition, process: Process) -> DecBar {
        TEST_VERTICAL.with(|f| f.set(true));
        TEST_POSITION.with(|p| p.set(position));
        let mut bar = DecBar::new(Box::new(active::NoActiveWindow)).with_process(process);
        let edge = bar.place().map_or(Anchor::Left, |p| p.edge);
        (bar.width, bar.height) = if edge.is_side() { (SURFACE_W, 900) } else { (1600, SURFACE_H) };
        bar.show_workspaces = true;
        bar.workspaces = (0..9).map(|i| Workspace { name: (i + 1).to_string(), active: i == 1, occupied: i == 2, urgent: i == 4 }).collect();
        bar
    }

    fn bar() -> DecBar {
        bar_at(BarPosition::Left, Process::Main)
    }

    #[test]
    fn a_600px_rail_keeps_application_overflow_reachable_without_hiding_basic_controls() {
        for edge in [Anchor::Left, Anchor::Right] {
            let geo = Geo::new(edge, SURFACE_W as f32, 600.0);
            let base = Content {
                workspaces: 9,
                window: 200.0,
                tray: vec![Tray::Layout, Tray::Awake, Tray::Agents, Tray::Control],
                status: vec![Tray::Lock, Tray::Weather, Tray::Audio, Tray::Network, Tray::Bluetooth, Tray::Battery],
                ..Content::default()
            };
            assert_eq!(indicator_capacity(&geo, Part::All, &base), 0);
            let mut content = base.clone();
            let count = fit_indicator_content(&geo, Part::All, &mut content, true, Some(controlcenter::settings::Tiles::ALL));
            assert!(count >= 1);
            assert_eq!(content.workspaces, 9);
            assert!(content.tray.contains(&Tray::Control));
            assert!(content.status.contains(&Tray::Battery));
            // These controls remain available in their existing Control Center groups.
            let snapshot = Snapshot {
                volume: Some(sys::Volume { level: 0.5, muted: false }),
                net: Some(sys::Net::default()),
                bluetooth: Some(sys::Bluetooth { powered: true, connected: 0 }),
                awake: true,
                brightness: Some(50),
                media: Some(sys::Media { playing: true, title: "Sample".into(), artist: "Artist".into() }),
                util: Some(sys::Utils::default()),
                dnd: Some(false),
                ..Snapshot::default()
            };
            let (controls, control_height) = controlcenter::layout_with(&snapshot, &controlcenter::settings::Tiles::ALL);
            assert!(control_height <= geo.h - 2.0 * POP_MARGIN, "the complete fallback panel fits the same 600px screen");
            for target in [Target::Wifi, Target::Bluetooth, Target::Volume, Target::StayAwake] {
                assert!(controls.iter().any(|item| item.target == Some(target)), "folded {target:?} must remain reachable");
            }
            content.status.push(Tray::Indicators);
            let lay = layout(&geo, Part::All, &content);
            for kind in [Tray::Workspaces, Tray::Clock, Tray::Power, Tray::Control, Tray::Battery, Tray::Indicators] {
                let target = lay.item(kind).unwrap();
                assert_eq!(lay.hit(target.cx(), target.cy()).map(|hit| hit.0), Some(kind), "{edge:?} {kind:?} must be an independent target");
            }
            let mut spans: Vec<_> = lay.pills.iter().map(|(_, r)| along(&geo, r)).collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(spans.windows(2).all(|r| r[0].1 <= r[1].0));
            assert!(spans.iter().all(|r| r.0 >= EDGE && r.1 <= geo.len() - EDGE));
            let mut custom = base.clone();
            fit_indicator_content(&geo, Part::All, &mut custom, true, None);
            assert!(custom.status.contains(&Tray::Audio) && custom.status.contains(&Tray::Network) && custom.status.contains(&Tray::Bluetooth));
            assert!(custom.tray.contains(&Tray::Awake), "disabled controls cannot be used as a hidden fallback");
            let mut no_apps = base.clone();
            fit_indicator_content(&geo, Part::All, &mut no_apps, false, Some(controlcenter::settings::Tiles::ALL));
            assert_eq!(no_apps, base, "without app indicators existing layout is unchanged");
        }
    }

    #[test]
    fn application_cells_fit_the_free_span_and_head_only_never_duplicates_them() {
        for edge in [Anchor::Left, Anchor::Right, Anchor::Top, Anchor::Bottom] {
            let geo = if edge.is_side() { Geo::new(edge, SURFACE_W as f32, 900.0) } else { Geo::new(edge, 1600.0, SURFACE_H as f32) };
            for part in [Part::All, Part::Tail] {
                let mut content = full();
                let count = indicator_capacity(&geo, part, &content);
                assert!(count > 0);
                content.status.extend((0..count).map(|key| Tray::Indicator(key as u64)));
                let lay = layout(&geo, part, &content);
                let mut spans: Vec<_> = lay.pills.iter().map(|(_, r)| along(&geo, r)).collect();
                spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                assert!(spans.windows(2).all(|r| r[0].1 <= r[1].0 + 0.01), "{edge:?}/{part:?}: {spans:?}");
                assert!(spans.iter().all(|r| r.0 >= EDGE && r.1 <= geo.len() - EDGE + 0.01));
            }
            assert_eq!(indicator_capacity(&geo, Part::Head, &full()), 0);
        }
    }

    #[test]
    fn rail_indicators_paint_cached_pixels_and_hidden_menus_anchor_to_overflow() {
        for position in [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom] {
            let mut bar = bar_at(position, Process::Main);
            if bar.v_geo().horizontal() {
                bar.width = 900;
            }
            let mut icon = Pixmap::new(8, 8).unwrap();
            icon.fill(tiny_skia::Color::from_rgba8(240, 60, 90, 255));
            let icon = std::sync::Arc::new(icon);
            bar.indicators.items = (1..=32)
                .map(|key| crate::sni::Item {
                    key,
                    title: format!("App {key}"),
                    tooltip: String::new(),
                    status: if key == 2 { crate::sni::Status::Passive } else { crate::sni::Status::Active },
                    icon_name: String::new(),
                    icon: Some(icon.clone()),
                    item_is_menu: false,
                    menu: None,
                    menu_loading: false,
                    menu_error: None,
                })
                .collect();
            let lay = bar.v_layout();
            assert!(lay.item(Tray::Indicator(2)).is_none());
            let cell = lay.item(Tray::Indicator(1)).expect("first visible application");
            assert!(lay.item(Tray::Indicators).is_some(), "the remaining apps stay reachable");
            let mut pm = Pixmap::new(bar.width, bar.height).unwrap();
            bar.v_draw(&mut pm);
            let pixel = pm.pixel(cell.cx() as u32, cell.cy() as u32).unwrap();
            assert_eq!((pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()), (240, 60, 90, 255));
            assert!(bar.v_button(cell.cx(), cell.cy(), Button::Right));
            bar.v_motion(cell.cx(), cell.cy());
            assert_eq!(bar.open, Open::Popup(Tray::Indicator(1)), "moving within an app icon must not dismiss its own menu");
            bar.v_open_popup(Tray::Indicator(32));
            bar.v_clamp_popup();
            assert_eq!(bar.open, Open::Popup(Tray::Indicator(32)));
            let p = bar.popup.as_ref().unwrap();
            let expected = bar.v_geo().popout_origin(lay.item(Tray::Indicators).unwrap(), p.w, p.h);
            assert_eq!((p.x, p.y), expected);
            TEST_VERTICAL.with(|f| f.set(false));
        }
    }

    #[test]
    fn glass_regions_follow_individual_pills_on_every_edge() {
        for position in [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom] {
            let bar = bar_at(position, Process::Main);
            let layout = bar.v_layout();
            let regions = bar.glass_regions();
            assert_eq!(regions.len(), layout.pills.len());
            for (region, (_, pill)) in regions.iter().zip(&layout.pills) {
                assert_eq!((region.x, region.y, region.w, region.h), (pill.x, pill.y, pill.w, pill.h));
                assert!(region.radius > 0.0 && region.radius <= region.w.min(region.h) / 2.0);
            }
            // No material in the canvas's generous popup space, even though it receives pointer input.
            let strip = bar.v_geo().strip();
            assert!(regions.iter().all(|r| r.x >= strip.x && r.y >= strip.y && r.x + r.w <= strip.right() + 0.01 && r.y + r.h <= strip.bottom() + 0.01));
        }
    }

    #[test]
    fn the_surface_asks_for_its_edge_and_a_strut_of_the_bar_thickness() {
        let b = bar();
        let spec = b.surface_spec().unwrap();
        assert_eq!((spec.anchor, spec.reserve, spec.stretch, spec.margin), (Anchor::Left, THICK as i32, true, 0));
        assert_eq!(b.size(), (SURFACE_W, 0), "as wide as the strip plus the popouts, full height");
        for (pos, edge) in [(BarPosition::Right, Anchor::Right), (BarPosition::Top, Anchor::Top), (BarPosition::Bottom, Anchor::Bottom), (BarPosition::Split, Anchor::Top)] {
            let b = bar_at(pos, Process::Main);
            let spec = b.surface_spec().unwrap();
            assert_eq!((spec.anchor, spec.reserve, spec.stretch), (edge, if edge.is_side() { THICK } else { HORIZONTAL_THICK } as i32, true), "{pos:?}");
            assert_eq!(b.size(), if edge.is_side() { (SURFACE_W, 0) } else { (0, SURFACE_H) }, "{pos:?}");
        }
        let split_bottom = bar_at(BarPosition::Split, Process::Bottom);
        let spec = split_bottom.surface_spec().unwrap();
        assert_eq!((spec.anchor, spec.namespace), (Anchor::Bottom, "decebal-bar-bottom"));
        TEST_VERTICAL.with(|f| f.set(false));
        let h = DecBar::new(Box::new(active::NoActiveWindow));
        assert_eq!(h.surface_spec().unwrap().anchor, Anchor::Top, "the classic bar is unchanged");
        assert_eq!(h.size(), (0, SURFACE_MIN_HEIGHT));
    }

    #[test]
    fn the_bottom_process_has_nothing_to_draw_unless_the_layout_is_split() {
        let mut b = bar_at(BarPosition::Split, Process::Bottom);
        assert!(!b.quit_requested(), "split: it runs");
        TEST_POSITION.with(|p| p.set(BarPosition::Left));
        assert!(b.place().is_none() && b.quit_requested(), "no longer split: it leaves");
        TEST_POSITION.with(|p| p.set(BarPosition::Split));
        assert!(!b.quit_requested());
        TEST_VERTICAL.with(|f| f.set(false));
        assert!(b.quit_requested(), "another theme: it leaves too");
        // the main bar never quits because of the layout
        let m = bar_at(BarPosition::Left, Process::Main);
        TEST_VERTICAL.with(|f| f.set(false));
        assert!(!m.quit_requested());
        let _ = &mut b;
    }

    #[test]
    fn the_weather_icon_appears_with_a_reading_and_a_click_opens_its_popout_on_every_edge() {
        for pos in BarPosition::ALL {
            let mut b = bar_at(pos, Process::Main);
            b.update_snapshot(|s| s.volume = Some(sys::Volume { level: 0.5, muted: false }));
            if b.v_layout().item(Tray::Audio).is_none() {
                continue; // the top half of a split bar carries no status icons
            }
            assert!(b.v_layout().item(Tray::Weather).is_none(), "{pos:?}: nothing to show before the first reading");
            *b.shared.weather.lock().unwrap() = Some(crate::menubar::sample_weather());
            let r = b.v_layout().item(Tray::Weather).expect("weather icon");
            assert!(b.v_button(r.cx(), r.cy(), Button::Left), "{pos:?}");
            assert_eq!(b.open, Open::Popup(Tray::Weather), "{pos:?}");
            let p = b.popup.clone().expect("popup");
            assert!(p.rows.iter().any(|row| row.act == popup::Act::WeatherRefresh));
            // the popup sits on the screen side of the strip, inside the surface
            assert!(p.x >= 0.0 && p.y >= 0.0 && p.x + p.w <= b.width as f32 + 0.5 && p.y + p.h <= b.height as f32 + 0.5, "{pos:?}");
            // and the icon paints without panicking
            let mut pm = Pixmap::new(b.width, b.height).unwrap();
            b.draw(&mut pm);
        }
        TEST_VERTICAL.with(|f| f.set(false));
    }

    #[test]
    fn only_the_strip_takes_input_until_something_opens() {
        for pos in BarPosition::ALL {
            let b = bar_at(pos, Process::Main);
            let s = b.v_geo().strip();
            assert_eq!(b.input_rects(), vec![InputRect { x: s.x as i32, y: s.y as i32, w: s.w as i32, h: s.h as i32 }], "{pos:?}");
            assert!(!b.wants_keyboard() && !b.wants_pointer_grab());
        }
    }

    #[test]
    fn clicking_a_workspace_pill_asks_rdwm_to_view_that_tag_on_every_edge() {
        let _g = crate::menubar::tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        for pos in [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom, BarPosition::Split] {
            let mut b = bar_at(pos, Process::Main);
            sys::set_dry_run(true);
            let _ = sys::take_log();
            let geo = b.v_geo();
            let ws = b.v_layout().pill(Pill::Workspaces).unwrap();
            let cell = Dim::of(geo.horizontal()).ws_cell;
            let (x, y) = if geo.horizontal() { (ws.x + WS_PAD + cell * 3.5, ws.cy()) } else { (ws.cx(), ws.y + WS_PAD + cell * 3.5) };
            assert_eq!(b.v_target(x, y), Some((Tray::Workspaces, Some(3))), "{pos:?}");
            // without rdwm tags it falls back to the EWMH tracker (a no-op here); with them it sends `view 4`
            b.v_switch_workspace(3);
            *b.shared.layout.lock().unwrap() = Some(rdwm::LayoutState { name: "tile".into(), symbol: "[]=".into(), tags: vec![TagState::default(); 9] });
            b.v_switch_workspace(3);
            let log = sys::take_log();
            sys::set_dry_run(false);
            assert!(log.iter().any(|l| l.contains("rdwmctl action view 4")), "{pos:?}: {log:?}");
        }
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
        for pos in [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom] {
            let mut b = bar_at(pos, Process::Main);
            let geo = b.v_geo();
            b.update_snapshot(|s| {
                s.volume = Some(sys::Volume { level: 0.4, muted: false });
                s.net = Some(sys::Net { wifi_enabled: true, wifi_name: "home".into(), wired_name: String::new() });
            });
            let lay = b.v_layout();
            let audio = lay.item(Tray::Audio).unwrap();
            let net = lay.item(Tray::Network).unwrap();
            assert!(b.pointer_motion(audio.cx(), audio.cy()), "{pos:?}");
            assert_eq!(b.open, Open::None, "{pos:?}: not at once");
            assert!(b.tick_in().is_some(), "the delay is scheduled");
            b.v_hover_open = Some((Tray::Audio, Instant::now() - HOVER_DELAY));
            let t = b.tick();
            assert!(t.redraw);
            assert_eq!(b.open, Open::Popup(Tray::Audio), "{pos:?}");
            let p = b.popup.as_ref().unwrap().clone();
            let (sw, sh) = (geo.w, geo.h);
            assert!(p.x >= POP_MARGIN && p.x + p.w <= sw && p.y >= POP_MARGIN && p.y + p.h <= sh, "{pos:?}: inside the surface");
            let s = geo.strip();
            match pos {
                BarPosition::Left => assert!(p.x >= s.right()),
                BarPosition::Right => assert!(p.x + p.w <= s.x),
                BarPosition::Top => assert!(p.y >= s.bottom()),
                _ => assert!(p.y + p.h <= s.y),
            }
            assert!(b.wants_pointer_grab());
            // the popup's input region reaches back to the strip, so the pointer can get there
            let rects = b.input_rects();
            assert_eq!(rects.len(), 2, "{pos:?}");
            assert!(rects[1].x as f32 <= p.x && rects[1].y as f32 <= p.y && (rects[1].x + rects[1].w) as f32 >= p.x + p.w && (rects[1].y + rects[1].h) as f32 >= p.y + p.h);
            // sliding to the next icon switches at once
            b.pointer_motion(net.cx(), net.cy());
            assert_eq!(b.open, Open::Popup(Tray::Network), "{pos:?}");
            // onto the logo (no popup): it closes
            let logo = lay.item(Tray::Logo).unwrap();
            b.pointer_motion(logo.cx(), logo.cy());
            assert_eq!(b.open, Open::None, "{pos:?}");
        }
    }

    #[test]
    fn the_coffee_cup_of_the_pill_bar_shows_since_when_on_hover_and_turns_off_on_click() {
        let (_state, dir) = super::super::awake::tests::private_state("vbar");
        for pos in [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom] {
            std::fs::write(dir.join("omarchy/indicators/stay-awake"), "").unwrap();
            let mut b = bar_at(pos, Process::Main);
            b.update_snapshot(|s| s.awake = true);
            let cup = b.v_layout().item(Tray::Awake).unwrap_or_else(|| panic!("{pos:?}: the cup is in the tray"));
            // hover: the popup after the delay says what it is and for how long it has been on
            b.pointer_motion(cup.cx(), cup.cy());
            b.v_hover_open = Some((Tray::Awake, Instant::now() - HOVER_DELAY));
            assert!(b.tick().redraw);
            assert_eq!(b.open, Open::Popup(Tray::Awake), "{pos:?}");
            let rows = b.popup.as_ref().unwrap().rows.clone();
            assert_eq!(rows[0].label, "Stay Awake");
            assert!(rows[0].detail.starts_with("On for"), "{pos:?}: {}", rows[0].detail);
            // clicking the icon itself turns it off (it does not just keep the popup open)
            assert!(b.pointer_button(cup.cx(), cup.cy(), Button::Left), "{pos:?}");
            assert_eq!(b.open, Open::None);
            assert!(!awake::is_on() && !b.snapshot().awake, "{pos:?}");
            assert!(b.v_layout().item(Tray::Awake).is_none(), "the cup is gone");
            // right click on it opens the options at once
            std::fs::write(dir.join("omarchy/indicators/stay-awake"), "").unwrap();
            b.update_snapshot(|s| s.awake = true);
            let cup = b.v_layout().item(Tray::Awake).unwrap();
            assert!(b.pointer_button(cup.cx(), cup.cy(), Button::Right));
            assert_eq!(b.open, Open::Popup(Tray::Awake), "{pos:?}");
            let _ = std::fs::remove_file(dir.join("omarchy/indicators/stay-awake"));
        }
        *awake::TEST_DIR.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_agents_item_runs_the_monitor() {
        let b = bar();
        if let Some(a) = b.v_layout().item(Tray::Agents) {
            // only with a Nerd Font on the machine; then a click is routed to the monitor
            assert_eq!(click_for(Tray::Agents, None), Click::Monitor);
            assert!(a.w > 0.0);
        }
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
        let _g = crate::menubar::tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        for pos in [BarPosition::Left, BarPosition::Top, BarPosition::Bottom] {
            let mut b = bar_at(pos, Process::Main);
            sys::set_dry_run(true);
            let _ = sys::take_log();
            let lay = b.v_layout();
            for k in [Tray::Logo, Tray::Clock, Tray::Power] {
                let r = lay.item(k).unwrap();
                assert!(b.pointer_button(r.cx(), r.cy(), Button::Left), "{pos:?} {k:?}");
            }
            let log = sys::take_log();
            sys::set_dry_run(false);
            let has = |needle: &str| log.iter().any(|l| l.contains(needle));
            assert!(has("decspot") || has("rofi") || has("wofi") || has("fuzzel") || log.is_empty() || !sys::have("decspot"), "launcher: {log:?}");
            let top = if pos == BarPosition::Top { 50 } else { 10 };
            assert!(has(&format!("decdash --top {top}")), "{pos:?}: below a top bar, near the edge otherwise: {log:?}");
            assert!(has("decsession"), "{log:?}");
        }
    }

    #[test]
    fn the_control_center_opens_beside_its_tile_and_stays_on_screen() {
        for pos in [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom] {
            let mut b = bar_at(pos, Process::Main);
            let geo = b.v_geo();
            let tile = b.v_layout().item(Tray::Control).unwrap();
            assert!(b.pointer_button(tile.cx(), tile.cy(), Button::Left), "{pos:?}");
            assert_eq!(b.open, Open::Control);
            let (ox, oy) = b.v_panel_origin();
            assert!(ox >= POP_MARGIN - 0.01 && ox + controlcenter::PANEL_W <= geo.w - POP_MARGIN + 0.01, "{pos:?}: x {ox}");
            assert!(oy >= POP_MARGIN - 0.01 && oy + b.cc_height <= geo.h - POP_MARGIN + 0.01, "{pos:?}: y {oy} + {}", b.cc_height);
            let s = geo.strip();
            match pos {
                BarPosition::Left => assert_eq!(ox, THICK + POP_GAP),
                BarPosition::Right => assert_eq!(ox + controlcenter::PANEL_W, s.x - POP_GAP, "leftwards"),
                BarPosition::Top => assert_eq!(oy, HORIZONTAL_THICK + POP_GAP, "downwards"),
                _ => assert_eq!(oy + b.cc_height, s.y - POP_GAP, "upwards"),
            }
            assert!(b.input_rects().len() == 2, "{pos:?}");
            assert!(b.pointer_button(tile.cx(), tile.cy(), Button::Left));
            assert_eq!(b.open, Open::None, "{pos:?}: the tile toggles");
        }
    }

    #[test]
    fn the_tallest_panel_fits_the_popout_room_of_a_top_or_bottom_bar() {
        let mut b = bar_at(BarPosition::Top, Process::Main);
        b.open_control();
        assert!(b.cc_height <= POPOUT_H - POP_GAP - POP_MARGIN, "the Control Center is {} tall, the room {}", b.cc_height, POPOUT_H);
    }

    #[test]
    fn switching_the_layout_closes_whatever_is_open_and_resizes() {
        let mut b = bar();
        b.update_snapshot(|s| s.volume = Some(sys::Volume { level: 0.4, muted: false }));
        let audio = b.v_layout().item(Tray::Audio).unwrap();
        b.pointer_button(audio.cx(), audio.cy(), Button::Left);
        assert_ne!(b.open, Open::None);
        // left -> top: the same bar, another edge
        TEST_POSITION.with(|p| p.set(BarPosition::Top));
        let t = b.tick();
        assert!(t.redraw);
        assert_eq!(b.open, Open::None);
        assert_eq!(b.size(), (0, SURFACE_H));
        assert_eq!(b.surface_spec().unwrap().anchor, Anchor::Top);
        assert_eq!(b.was_place.map(|p| p.edge), Some(Anchor::Top));
        // ...to split: only the head stays in this process
        TEST_POSITION.with(|p| p.set(BarPosition::Split));
        b.tick();
        assert_eq!(b.was_place.map(|p| p.part), Some(Part::Head));
        // ...and back to the classic bar with another theme
        TEST_VERTICAL.with(|f| f.set(false));
        b.tick();
        assert_eq!(b.size(), (0, SURFACE_MIN_HEIGHT));
        assert!(b.was_place.is_none());
    }

    #[test]
    fn a_pill_frame_paints_the_strip_pills_and_leaves_the_popout_area_clear_on_every_edge() {
        for pos in [BarPosition::Left, BarPosition::Right, BarPosition::Top, BarPosition::Bottom] {
            let mut b = bar_at(pos, Process::Main);
            let geo = b.v_geo();
            let mut pm = crate::gfx::pixmap(geo.w as u32, geo.h as u32).unwrap();
            b.draw(&mut pm);
            let lay = b.v_layout();
            let logo = lay.pill(Pill::Logo).unwrap();
            let a = |x: f32, y: f32| pm.pixel(x as u32, y as u32).unwrap().alpha();
            let s = geo.strip();
            assert_eq!(a(s.x, s.y), 255, "{pos:?}: the strip is painted from its corner");
            assert_eq!(a(s.right() - 1.0, s.bottom() - 1.0), 255, "{pos:?}");
            // beside the strip, on the popout side, stays transparent
            let clear = match pos {
                BarPosition::Left => (THICK + 5.0, 100.0),
                BarPosition::Right => (s.x - 5.0, 100.0),
                BarPosition::Top => (100.0, THICK + 5.0),
                _ => (100.0, s.y - 5.0),
            };
            assert_eq!(a(clear.0, clear.1), 0, "{pos:?}: the popout room stays transparent");
            let th = crate::theme::th();
            let off = if geo.horizontal() { (logo.cx(), s.y + 2.0) } else { (s.x + 2.0, logo.cy()) };
            let (strip, pill) = (pm.pixel(off.0 as u32, off.1 as u32).unwrap(), pm.pixel(logo.cx() as u32, (logo.y + 3.0) as u32).unwrap());
            assert_ne!((strip.red(), strip.green(), strip.blue()), (pill.red(), pill.green(), pill.blue()), "{pos:?}: pills are drawn over the strip ({:?})", th.id);
        }
    }

    #[test]
    fn the_wheel_changes_the_volume_in_the_first_half_and_the_brightness_in_the_second() {
        let _g = crate::menubar::tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        for pos in [BarPosition::Left, BarPosition::Top] {
            let mut b = bar_at(pos, Process::Main);
            let geo = b.v_geo();
            b.update_snapshot(|s| {
                s.volume = Some(sys::Volume { level: 0.4, muted: false });
                s.brightness = Some(50);
            });
            sys::set_dry_run(true);
            let _ = sys::take_log();
            // on the logo (first half) and the power button (second half)
            let lay = b.v_layout();
            let logo = lay.item(Tray::Logo).unwrap();
            let power = lay.item(Tray::Power).unwrap();
            assert!(b.v_scroll(logo.cx(), logo.cy(), -1.0), "{pos:?}: volume");
            assert_eq!(b.snapshot().volume.map(|v| (v.level * 100.0).round() as i32), Some(45), "{pos:?}");
            assert!(b.v_scroll(power.cx(), power.cy(), -1.0), "{pos:?}: brightness");
            assert_eq!(b.snapshot().brightness, Some(55), "{pos:?}");
            sys::set_dry_run(false);
            let _ = sys::take_log();
            // off the strip: nothing
            assert!(!b.v_scroll(geo.w - 3.0, geo.h - 3.0, 1.0));
        }
    }

    #[test]
    fn the_main_bar_starts_the_bottom_process_only_while_split_and_only_when_live() {
        let mut b = bar_at(BarPosition::Split, Process::Main);
        assert!(!b.ensure_bottom(), "a bar that is not the live one never spawns");
        assert_eq!(b.bottom_spawns, 0);
        b.companion = true;
        b.bottom_exe = Some("/bin/true".into());
        assert!(b.ensure_bottom(), "split: the companion is wanted");
        assert_eq!(b.bottom_spawns, 1, "started");
        // it ended at once (true): not restarted before the retry delay, then restarted
        std::thread::sleep(Duration::from_millis(200));
        b.ensure_bottom();
        assert_eq!(b.bottom_spawns, 1);
        assert!(b.bottom_child.is_none());
        b.bottom_retry = Instant::now() - Duration::from_secs(1);
        b.ensure_bottom();
        assert_eq!(b.bottom_spawns, 2);
        // not split any more: nothing is started, the companion leaves by itself
        TEST_POSITION.with(|p| p.set(BarPosition::Top));
        std::thread::sleep(Duration::from_millis(200));
        b.bottom_retry = Instant::now() - Duration::from_secs(1);
        assert!(!b.ensure_bottom());
        assert_eq!(b.bottom_spawns, 2);
        // the bottom bar never owns a companion
        let mut bottom = bar_at(BarPosition::Split, Process::Bottom);
        bottom.companion = true;
        assert!(!bottom.ensure_bottom());
        assert_eq!(bottom_args(), ["--role", "bottom"]);
    }
}
