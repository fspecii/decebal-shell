//! Universal panels: which edge a panel sits on and which applets it shows, for every renderer (the classic menu bar,
//! the floating bar, the Caelestia/Glyph pill rail, the Dux islands and the Windows taskbar), from one typed registry
//! ([`applets`]) and one resolution order ([`resolve`]). Settings, `decthemectl panel`, the theme packages and the
//! native previews all go through here. `docs/universal-panels.md` is the contract.
//!
//! Resolution, first match wins:
//!
//! | what | 1. user (`decebal.json` / `dock-settings.json`) | 2. legacy | 3. theme package | 4. renderer |
//! |---|---|---|---|---|
//! | bar edge | `bar.edge` (+ `bar.split`) | `bar.position` (pill rail only, as before) | `[bar] position` | top (rail: left) |
//! | dock / taskbar edge | `edge` | `windowsPosition` (taskbar) | `[dock] position` | bottom |
//! | each applet | `bar.applets.<key>` | `workspaces` toggle | `[bar] applets` (Dux: `left`/`center`/`right`) | [`Renderer::defaults`] |
//!
//! User values are never written by a theme or palette change; Settings' "Use theme default" removes them.

pub mod applets;
pub mod branding;
pub mod frame;
pub mod plasma_style;

/// The first release with this framework (the 0.3.0 feature batch): packages that use `[bar] applets`, `[branding]`, a
/// non-top edge for a non-rail bar or a side dock declare it as `min_shell_version` (0.2.x rejects `[bar] applets` and
/// `[branding]` as unknown keys and draws those bars only on top).
pub const SINCE: &str = "0.3.0";

/// Whether a theme's extra sections use something only this framework understands (see [`SINCE`]).
pub fn needs_framework(extra: &serde_json::Map<String, Value>) -> bool {
    let bar = extra.get("bar");
    let style = bar.and_then(|b| b.get("style")).and_then(Value::as_str).unwrap_or("flat");
    let edge = bar.and_then(|b| b.get("position")).and_then(Value::as_str).and_then(Edge::parse);
    let dock = extra.get("dock").and_then(|d| d.get("position")).and_then(Value::as_str).and_then(Edge::parse);
    extra.contains_key("branding")
        || extra.contains_key("taskbar")
        || bar.is_some_and(|b| b.get("applets").is_some())
        || (style != "vertical" && edge.is_some_and(|e| e != Edge::Top))
        || dock.is_some_and(Edge::is_vertical)
}

pub use applets::{AppletId, AppletSet, Cap, Need, Worker, REGISTRY};

use crate::config::BarPosition;
use serde_json::Value;
use std::collections::BTreeMap;

/// A screen edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Edge {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right];

    pub fn parse(s: &str) -> Option<Edge> {
        match s.trim().to_ascii_lowercase().as_str() {
            "top" => Some(Edge::Top),
            "bottom" => Some(Edge::Bottom),
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Edge::Top => "Top",
            Edge::Bottom => "Bottom",
            Edge::Left => "Left",
            Edge::Right => "Right",
        }
    }
    /// A side edge: the panel runs down the screen.
    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
    /// The panel sits at the far end of its axis (bottom / right): popups open towards the start.
    pub fn is_far(self) -> bool {
        matches!(self, Edge::Bottom | Edge::Right)
    }
    pub fn anchor(self) -> crate::backend::Anchor {
        use crate::backend::Anchor;
        match self {
            Edge::Top => Anchor::Top,
            Edge::Bottom => Anchor::Bottom,
            Edge::Left => Anchor::Left,
            Edge::Right => Anchor::Right,
        }
    }
    /// The pill rail's word for a single bar on this edge.
    pub fn position(self) -> BarPosition {
        match self {
            Edge::Top => BarPosition::Top,
            Edge::Bottom => BarPosition::Bottom,
            Edge::Left => BarPosition::Left,
            Edge::Right => BarPosition::Right,
        }
    }
    /// (edge, split) of a pill-rail position word.
    pub fn of_position(p: BarPosition) -> (Edge, bool) {
        match p {
            BarPosition::Top => (Edge::Top, false),
            BarPosition::Bottom => (Edge::Bottom, false),
            BarPosition::Left => (Edge::Left, false),
            BarPosition::Right => (Edge::Right, false),
            BarPosition::Split => (Edge::Top, true),
        }
    }
}

/// Which renderer draws a panel. Each keeps its own look and its own default applets; every renderer draws every
/// registered applet when it is enabled (an applet is only absent when the system lacks what it needs, see [`Need`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Renderer {
    /// `bar_style = "flat"`: the macOS-style strip with application menus.
    MenuBar,
    /// `bar_style = "floating"`: the same bar as a pill with a margin.
    Floating,
    /// `bar_style = "vertical"`: the Caelestia / Glyph pill rail (any edge, or split top + bottom).
    Pill,
    /// `bar_style = "dux"`: the Dux band with three islands.
    Dux,
    /// `dock-settings.json` `style = "windows"`: the Windows taskbar (decdock).
    Taskbar,
}

impl Renderer {
    pub const ALL: [Renderer; 5] = [Renderer::MenuBar, Renderer::Floating, Renderer::Pill, Renderer::Dux, Renderer::Taskbar];

    /// The renderer of decbar for a theme's `bar_style` (unknown words draw the classic bar, as before).
    pub fn for_bar_style(style: &str) -> Renderer {
        match style {
            "floating" => Renderer::Floating,
            "vertical" => Renderer::Pill,
            "dux" => Renderer::Dux,
            _ => Renderer::MenuBar,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Renderer::MenuBar => "menubar",
            Renderer::Floating => "floating",
            Renderer::Pill => "rail",
            Renderer::Dux => "dux",
            Renderer::Taskbar => "taskbar",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Renderer::MenuBar => "Menu bar",
            Renderer::Floating => "Floating bar",
            Renderer::Pill => "Pill rail",
            Renderer::Dux => "Dux bar",
            Renderer::Taskbar => "Taskbar",
        }
    }
    pub fn default_edge(self) -> Edge {
        match self {
            Renderer::Pill => Edge::Left,
            Renderer::Taskbar => Edge::Bottom,
            _ => Edge::Top,
        }
    }
    /// Only the pill rail draws a split layout (two bars, top and bottom).
    pub fn supports_split(self) -> bool {
        self == Renderer::Pill
    }

    /// What the renderer showed before applets could be chosen: its authentic default (themes and users change it).
    pub fn defaults(self) -> AppletSet {
        use AppletId as A;
        AppletSet::of(match self {
            Renderer::MenuBar | Renderer::Floating => &[
                A::AppMenu,
                A::Workspaces,
                A::Layout,
                A::Weather,
                A::StayAwake,
                A::Agents,
                A::Tray,
                A::Battery,
                A::Bluetooth,
                A::Volume,
                A::Network,
                A::Dashboard,
                A::Search,
                A::ControlCenter,
                A::Clock,
            ],
            Renderer::Pill => &[
                A::Launcher,
                A::Workspaces,
                A::WindowTitle,
                A::Layout,
                A::StayAwake,
                A::Agents,
                A::ControlCenter,
                A::LockKeys,
                A::Weather,
                A::Volume,
                A::Network,
                A::Bluetooth,
                A::Battery,
                A::Tray,
                A::Clock,
                A::Power,
            ],
            // the Dux default is its item lists (see [`resolve`]); this is BarLayout::default()
            Renderer::Dux => &[A::Launcher, A::Workspaces, A::WindowTitle, A::Media, A::Clock, A::Weather, A::Tray, A::Clipboard, A::Cpu, A::Memory, A::Notifications, A::Battery, A::ControlCenter],
            Renderer::Taskbar => &[A::Launcher, A::Search, A::Tasks, A::Layout, A::Weather, A::Bluetooth, A::Network, A::Volume, A::Battery, A::Tray, A::ControlCenter, A::Clock, A::ShowDesktop],
        })
    }
}

// ------------------------------------------------------------------------------------------------ preferences

/// The user's own choices (`decebal.json` `bar`), kept apart from what themes say.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserPanel {
    pub edge: Option<Edge>,
    /// Pill rail only: two bars, top and bottom.
    pub split: Option<bool>,
    pub applets: BTreeMap<AppletId, bool>,
}

impl UserPanel {
    /// Reads `bar.edge`, `bar.split` and `bar.applets` (`{"weather": false}` or a list of enabled keys). Unknown keys and
    /// values are ignored, never an error.
    pub fn from_bar_json(bar: &Value) -> UserPanel {
        let edge = bar.get("edge").and_then(Value::as_str).and_then(Edge::parse);
        let split = bar.get("split").and_then(Value::as_bool);
        let mut applets = BTreeMap::new();
        match bar.get("applets") {
            Some(Value::Object(m)) => {
                for (k, v) in m {
                    if let (Some(id), Some(on)) = (AppletId::parse(k), v.as_bool()) {
                        applets.insert(id, on);
                    }
                }
            }
            Some(Value::Array(list)) => {
                let on: Vec<AppletId> = list.iter().filter_map(Value::as_str).filter_map(AppletId::parse).collect();
                for id in AppletId::all() {
                    applets.insert(id, on.contains(&id));
                }
            }
            _ => {}
        }
        UserPanel { edge, split, applets }
    }
    pub fn is_empty(&self) -> bool {
        self.edge.is_none() && self.split.is_none() && self.applets.is_empty()
    }
}

/// What a theme package declares (`[bar] position`, `[bar] applets`, `[dock] position`). Read from the package at run
/// time, so a palette or mode change never touches the user's choices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemePanel {
    pub edge: Option<Edge>,
    pub split: bool,
    pub applets: Option<AppletSet>,
    pub dock_edge: Option<Edge>,
}

impl ThemePanel {
    pub fn from_extra(extra: &serde_json::Map<String, Value>) -> ThemePanel {
        let bar = extra.get("bar");
        let pos = bar.and_then(|b| b.get("position")).and_then(Value::as_str);
        let (edge, split) = match pos.and_then(BarPosition::parse) {
            Some(p) => {
                let (e, s) = Edge::of_position(p);
                (Some(e), s)
            }
            None => (None, false),
        };
        let applets = bar.and_then(|b| b.get("applets")).and_then(Value::as_array).map(|list| AppletSet::of(&list.iter().filter_map(Value::as_str).filter_map(AppletId::parse).collect::<Vec<_>>()));
        let dock_edge = extra.get("dock").and_then(|d| d.get("position")).and_then(Value::as_str).and_then(Edge::parse);
        ThemePanel { edge, split, applets, dock_edge }
    }
}

/// Where a resolved value came from (Settings says "theme default" or "your choice").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    User,
    Legacy,
    Theme,
    Renderer,
}

impl Origin {
    pub fn title(self) -> &'static str {
        match self {
            Origin::User => "your choice",
            Origin::Legacy => "earlier setting",
            Origin::Theme => "theme default",
            Origin::Renderer => "style default",
        }
    }
}

/// Everything [`resolve`] reads.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub renderer: Renderer,
    pub user: &'a UserPanel,
    pub theme: &'a ThemePanel,
    /// `decebal.json` `bar.position` (the rail's old key) or, for the taskbar, `windowsPosition`.
    pub legacy_position: Option<BarPosition>,
    /// The old `decebal.json` `workspaces` switch, resolved for this session.
    pub legacy_workspaces: Option<bool>,
    /// The Dux item lists in effect (`bar.left` / `center` / `right`).
    pub dux: Option<&'a crate::config::BarLayout>,
    /// For the taskbar / dock: the user's `dock-settings.json` `edge`.
    pub dock_user_edge: Option<Edge>,
}

/// The effective panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Effective {
    pub renderer: Renderer,
    pub edge: Edge,
    pub split: bool,
    /// Enabled applets.
    pub applets: AppletSet,
    /// Applets the user switched on explicitly. A state indicator (Stay Awake, lock keys) that the user asked for stays
    /// on the panel as a control while its state is off; by default it only appears while on, as before.
    pub explicit: AppletSet,
}

impl Effective {
    pub fn shows(&self, id: AppletId) -> bool {
        self.applets.contains(id)
    }
    /// Stable digest of what changes a panel's picture: the native preview cache keys on it.
    pub fn fingerprint(&self) -> String {
        format!("{}:{}:{}:{:x}:{:x}", self.renderer.key(), self.edge.as_str(), if self.split { "split" } else { "single" }, self.applets.bits(), self.explicit.bits())
    }
}

/// The edge and where it came from.
pub fn resolve_edge(i: &Inputs) -> (Edge, bool, Origin) {
    let split_ok = i.renderer.supports_split();
    if i.renderer == Renderer::Taskbar {
        if let Some(e) = i.dock_user_edge {
            return (e, false, Origin::User);
        }
        if let Some(p) = i.legacy_position {
            return (Edge::of_position(p).0, false, Origin::Legacy);
        }
        return match i.theme.dock_edge {
            Some(e) => (e, false, Origin::Theme),
            None => (Edge::Bottom, false, Origin::Renderer),
        };
    }
    if let Some(e) = i.user.edge {
        return (e, split_ok && i.user.split.unwrap_or(false), Origin::User);
    }
    if split_ok && i.user.split == Some(true) {
        return (Edge::Top, true, Origin::User);
    }
    if i.renderer == Renderer::Pill {
        if let Some(p) = i.legacy_position {
            let (e, s) = Edge::of_position(p);
            return (e, s, Origin::Legacy);
        }
    }
    match i.theme.edge {
        Some(e) => (e, split_ok && i.theme.split, Origin::Theme),
        None => (i.renderer.default_edge(), false, Origin::Renderer),
    }
}

/// Whether one applet is on, and where that came from.
pub fn resolve_applet(i: &Inputs, id: AppletId) -> (bool, Origin) {
    resolve_applet_with(i, id, i.dux.map(dux_items))
}

/// [`resolve_applet`] with the Dux item lists already turned into a set (once per resolution, not per applet).
fn resolve_applet_with(i: &Inputs, id: AppletId, dux: Option<AppletSet>) -> (bool, Origin) {
    if let Some(on) = i.user.applets.get(&id) {
        return (*on, Origin::User);
    }
    if id == AppletId::Workspaces {
        if let Some(on) = i.legacy_workspaces {
            if !on {
                return (false, Origin::Legacy);
            }
        }
    }
    if let Some(set) = i.theme.applets {
        return (set.contains(id), Origin::Theme);
    }
    if i.renderer == Renderer::Dux {
        if let Some(set) = dux {
            return (set.contains(id), Origin::Theme);
        }
    }
    (i.renderer.defaults().contains(id), Origin::Renderer)
}

/// The applets named by Dux item lists.
pub fn dux_items(l: &crate::config::BarLayout) -> AppletSet {
    AppletSet::of(&l.left.iter().chain(&l.center).chain(&l.right).filter_map(|k| AppletId::parse(k)).collect::<Vec<_>>())
}

pub fn resolve(i: &Inputs) -> Effective {
    let (edge, split, _) = resolve_edge(i);
    let mut enabled = AppletSet::EMPTY;
    let dux = if i.renderer == Renderer::Dux { i.dux.map(dux_items) } else { None };
    for id in AppletId::all() {
        enabled.set(id, resolve_applet_with(i, id, dux).0);
    }
    let explicit = AppletSet::of(&i.user.applets.iter().filter(|(_, on)| **on).map(|(id, _)| *id).collect::<Vec<_>>());
    Effective { renderer: i.renderer, edge, split, applets: enabled, explicit }
}

/// The background workers a panel needs for `applets`. `control_center` is true while its button is shown: its sound
/// tile follows volume changes made elsewhere, so the volume subscription stays for it.
pub fn workers_for(applets: AppletSet, control_center: bool) -> Vec<Worker> {
    let mut out: Vec<Worker> = applets.iter().flat_map(|id| id.info().workers.iter().copied()).collect();
    if control_center {
        out.push(Worker::VolumeWatch);
    }
    out.sort();
    out.dedup();
    out
}

/// The old `workspaces` switch for this session: `auto` means "only under rdwm" (asked once per process).
pub fn legacy_workspaces(t: crate::config::Toggle) -> Option<bool> {
    static RDWM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    Some(match t {
        crate::config::Toggle::Word(crate::config::Word::Auto) => {
            if cfg!(test) {
                true
            } else {
                *RDWM.get_or_init(crate::wm::is_rdwm)
            }
        }
        other => other.resolve(true),
    })
}

// ------------------------------------------------------------------------------------------------ writing

/// Sets (`Some`) or clears (`None`, back to the theme's default) the user's bar edge in a `decebal.json` document. A
/// single edge also ends a split layout. The older `bar.position` stays as the theme wrote it, so clearing the user's
/// edge really returns to the theme's (0.2.x readers keep their own layout: they never see the user's edge).
pub fn write_edge(shell: &mut Value, edge: Option<Edge>, split: Option<bool>) {
    let bar = bar_obj(shell);
    match edge {
        Some(e) => {
            bar.insert("edge".into(), Value::from(e.as_str()));
            bar.insert("split".into(), Value::from(split.unwrap_or(false)));
        }
        None => {
            bar.shift_remove("edge");
            bar.shift_remove("split");
        }
    }
}

/// Sets (`Some`) or clears (`None`, back to the theme's default) one applet in a `decebal.json` document.
pub fn write_applet(shell: &mut Value, id: AppletId, on: Option<bool>) {
    let bar = bar_obj(shell);
    let applets = bar.entry("applets").or_insert_with(|| Value::Object(Default::default()));
    if let Value::Array(list) = applets {
        // the list form names every enabled applet: it becomes the map form first, keeping each choice
        let on: Vec<AppletId> = list.iter().filter_map(Value::as_str).filter_map(AppletId::parse).collect();
        *applets = Value::Object(AppletId::all().map(|a| (a.key().to_string(), Value::from(on.contains(&a)))).collect());
    }
    if !applets.is_object() {
        *applets = Value::Object(Default::default());
    }
    let map = applets.as_object_mut().expect("object");
    match on {
        Some(v) => {
            map.insert(id.key().into(), Value::from(v));
        }
        None => {
            map.shift_remove(id.key());
        }
    }
    if map.is_empty() {
        bar.shift_remove("applets");
    }
}

/// "Use theme defaults": removes every user panel value (edge, split, applets) and nothing else.
pub fn reset(shell: &mut Value) {
    let bar = bar_obj(shell);
    for k in ["edge", "split", "applets"] {
        bar.shift_remove(k);
    }
}

fn bar_obj(shell: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !shell.is_object() {
        *shell = Value::Object(Default::default());
    }
    let o = shell.as_object_mut().expect("object");
    let bar = o.entry("bar").or_insert_with(|| Value::Object(Default::default()));
    if !bar.is_object() {
        *bar = Value::Object(Default::default());
    }
    bar.as_object_mut().expect("object")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inputs<'a>(r: Renderer, user: &'a UserPanel, theme: &'a ThemePanel) -> Inputs<'a> {
        Inputs { renderer: r, user, theme, legacy_position: None, legacy_workspaces: None, dux: None, dock_user_edge: None }
    }

    #[test]
    fn old_configs_resolve_exactly_as_before() {
        let (u, t) = (UserPanel::default(), ThemePanel::default());
        // classic / floating / dux: top; the rail: bar.position, left when absent
        for r in [Renderer::MenuBar, Renderer::Floating, Renderer::Dux] {
            assert_eq!(resolve(&inputs(r, &u, &t)).edge, Edge::Top);
            // the rail's old key does not move the other bars
            assert_eq!(resolve(&Inputs { legacy_position: Some(BarPosition::Right), ..inputs(r, &u, &t) }).edge, Edge::Top);
        }
        assert_eq!(resolve(&inputs(Renderer::Pill, &u, &t)).edge, Edge::Left);
        let split = resolve(&Inputs { legacy_position: Some(BarPosition::Split), ..inputs(Renderer::Pill, &u, &t) });
        assert_eq!((split.edge, split.split), (Edge::Top, true));
        assert_eq!(resolve(&Inputs { legacy_position: Some(BarPosition::Top), ..inputs(Renderer::Taskbar, &u, &t) }).edge, Edge::Top);
        assert_eq!(resolve(&inputs(Renderer::Taskbar, &u, &t)).edge, Edge::Bottom);
        // default applet sets are what each renderer always drew
        for r in Renderer::ALL {
            assert_eq!(resolve(&inputs(r, &u, &t)).applets, r.defaults());
        }
    }

    #[test]
    fn user_choice_beats_legacy_beats_theme_beats_renderer() {
        let t = ThemePanel { edge: Some(Edge::Bottom), ..Default::default() };
        let none = UserPanel::default();
        assert_eq!(resolve_edge(&inputs(Renderer::MenuBar, &none, &t)), (Edge::Bottom, false, Origin::Theme));
        assert_eq!(resolve_edge(&Inputs { legacy_position: Some(BarPosition::Right), ..inputs(Renderer::Pill, &none, &t) }), (Edge::Right, false, Origin::Legacy));
        let user = UserPanel { edge: Some(Edge::Left), ..Default::default() };
        for r in [Renderer::MenuBar, Renderer::Floating, Renderer::Pill, Renderer::Dux] {
            assert_eq!(resolve_edge(&Inputs { legacy_position: Some(BarPosition::Right), ..inputs(r, &user, &t) }).0, Edge::Left);
        }
        // split is the rail's alone
        let split = UserPanel { split: Some(true), ..Default::default() };
        assert_eq!(resolve_edge(&inputs(Renderer::Pill, &split, &t)), (Edge::Top, true, Origin::User));
        assert!(!resolve_edge(&inputs(Renderer::MenuBar, &split, &t)).1);
    }

    #[test]
    fn applets_follow_user_then_theme_then_renderer() {
        let mut user = UserPanel::default();
        user.applets.insert(AppletId::Weather, false);
        user.applets.insert(AppletId::Media, true);
        let t = ThemePanel::default();
        let e = resolve(&inputs(Renderer::MenuBar, &user, &t));
        assert!(!e.shows(AppletId::Weather) && e.shows(AppletId::Media) && e.shows(AppletId::Clock));
        let themed = ThemePanel { applets: Some(AppletSet::of(&[AppletId::Clock, AppletId::Volume])), ..Default::default() };
        let e = resolve(&inputs(Renderer::Pill, &UserPanel::default(), &themed));
        assert_eq!(e.applets, AppletSet::of(&[AppletId::Clock, AppletId::Volume]));
        // the workspaces switch of old configs still turns them off
        let e = resolve(&Inputs { legacy_workspaces: Some(false), ..inputs(Renderer::MenuBar, &UserPanel::default(), &t) });
        assert!(!e.shows(AppletId::Workspaces));
    }

    #[test]
    fn dux_defaults_are_its_item_lists() {
        let l = crate::config::BarLayout::default();
        let (u, t) = (UserPanel::default(), ThemePanel::default());
        let e = resolve(&Inputs { dux: Some(&l), ..inputs(Renderer::Dux, &u, &t) });
        assert_eq!(e.applets, dux_items(&l));
        assert_eq!(dux_items(&l), Renderer::Dux.defaults());
    }

    #[test]
    fn every_renderer_can_enable_every_applet() {
        let mut user = UserPanel::default();
        for id in AppletId::all() {
            user.applets.insert(id, true);
        }
        for r in Renderer::ALL {
            assert_eq!(resolve(&inputs(r, &user, &ThemePanel::default())).applets, AppletSet::all(), "{}", r.key());
        }
        for id in AppletId::all() {
            let mut one = UserPanel::default();
            for other in AppletId::all() {
                one.applets.insert(other, other == id);
            }
            for r in Renderer::ALL {
                assert_eq!(resolve(&inputs(r, &one, &ThemePanel::default())).applets, AppletSet::of(&[id]));
            }
        }
    }

    #[test]
    fn user_json_round_trips_and_reset_removes_only_user_keys() {
        let mut doc = json!({"bar": {"enabled": true, "position": "left", "keep": 1}, "theme": {"name": "nord"}});
        write_edge(&mut doc, Some(Edge::Bottom), None);
        write_applet(&mut doc, AppletId::Weather, Some(false));
        write_applet(&mut doc, AppletId::Media, Some(true));
        let u = UserPanel::from_bar_json(&doc["bar"]);
        assert_eq!(u.edge, Some(Edge::Bottom));
        assert_eq!(u.applets.get(&AppletId::Weather), Some(&false));
        assert_eq!(doc["bar"]["position"], "left", "the theme's older key is left alone");
        write_applet(&mut doc, AppletId::Weather, None);
        write_applet(&mut doc, AppletId::Media, None);
        assert!(doc["bar"].get("applets").is_none());
        reset(&mut doc);
        assert_eq!(doc, json!({"bar": {"enabled": true, "position": "left", "keep": 1}, "theme": {"name": "nord"}}));
        // the list form names exactly the enabled ones; junk is ignored
        let u = UserPanel::from_bar_json(&json!({"applets": ["clock", "nope", 3], "edge": "sideways"}));
        assert_eq!(u.edge, None);
        assert_eq!(u.applets.iter().filter(|(_, on)| **on).count(), 1);
    }

    #[test]
    fn a_reset_rail_returns_to_the_themes_edge() {
        // the rail's theme said left (bar.position); the user chose bottom, then reset
        let mut doc = json!({"bar": {"position": "left"}});
        write_edge(&mut doc, Some(Edge::Bottom), None);
        let theme = ThemePanel::default();
        let edge = |doc: &Value| {
            let user = UserPanel::from_bar_json(&doc["bar"]);
            let legacy = doc["bar"]["position"].as_str().and_then(BarPosition::parse);
            resolve_edge(&Inputs { legacy_position: legacy, ..inputs(Renderer::Pill, &user, &theme) }).0
        };
        assert_eq!(edge(&doc), Edge::Bottom);
        reset(&mut doc);
        assert_eq!(edge(&doc), Edge::Left, "reset gives the theme's edge back");
        write_edge(&mut doc, Some(Edge::Right), None);
        write_edge(&mut doc, None, None);
        assert_eq!(edge(&doc), Edge::Left);
    }

    #[test]
    fn editing_one_applet_keeps_the_others_of_a_list_choice() {
        let mut doc = json!({"bar": {"applets": ["clock", "volume", "weather"]}});
        write_applet(&mut doc, AppletId::Weather, Some(false));
        let u = UserPanel::from_bar_json(&doc["bar"]);
        assert_eq!(u.applets.get(&AppletId::Clock), Some(&true));
        assert_eq!(u.applets.get(&AppletId::Volume), Some(&true));
        assert_eq!(u.applets.get(&AppletId::Weather), Some(&false));
        assert_eq!(u.applets.get(&AppletId::Battery), Some(&false), "unlisted ones stay off");
    }

    #[test]
    fn theme_manifest_keys_parse() {
        let extra: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"bar": {"position": "split", "applets": ["clock", "launcherButton", "bogus"]}, "dock": {"position": "left"}})).unwrap();
        let t = ThemePanel::from_extra(&extra);
        assert_eq!((t.edge, t.split, t.dock_edge), (Some(Edge::Top), true, Some(Edge::Left)));
        assert_eq!(t.applets, Some(AppletSet::of(&[AppletId::Clock, AppletId::Launcher])));
    }

    #[test]
    fn workers_follow_enabled_applets() {
        assert!(workers_for(AppletSet::EMPTY, false).is_empty());
        assert_eq!(workers_for(AppletSet::EMPTY, true), vec![Worker::VolumeWatch]);
        let w = workers_for(AppletSet::of(&[AppletId::Weather, AppletId::Cpu, AppletId::Memory, AppletId::Media]), false);
        assert_eq!(w, vec![Worker::Weather, Worker::Mpris, Worker::Resources]);
    }

    #[test]
    fn packages_using_the_framework_require_its_release() {
        let need = |v: serde_json::Value| needs_framework(&serde_json::from_value(v).unwrap());
        assert!(need(json!({"bar": {"applets": ["clock"]}})));
        assert!(need(json!({"branding": {"logo": "builtin:os"}})));
        assert!(need(json!({"bar": {"style": "flat", "position": "bottom"}})));
        assert!(need(json!({"dock": {"position": "left"}})));
        assert!(!need(json!({"bar": {"style": "vertical", "position": "right"}})), "the rail had every edge before");
        assert!(!need(json!({"bar": {"style": "flat", "position": "top"}, "dock": {"position": "bottom"}})));
        assert_eq!(SINCE, "0.3.0");
        assert!(crate::theme::engine::schema::parse_version(SINCE) <= crate::theme::engine::schema::parse_version(env!("CARGO_PKG_VERSION")), "never newer than this build");
    }

    #[test]
    fn fingerprint_changes_with_every_relevant_input() {
        let (u, t) = (UserPanel::default(), ThemePanel::default());
        let base = resolve(&inputs(Renderer::MenuBar, &u, &t));
        let moved = Effective { edge: Edge::Left, ..base };
        let fewer = Effective { applets: base.applets.with(AppletId::Clock, false), ..base };
        let fps = [base.fingerprint(), moved.fingerprint(), fewer.fingerprint()];
        assert!(fps[0] != fps[1] && fps[0] != fps[2] && fps[1] != fps[2]);
    }
}
