//! The "Appearance" pane: the one home of how the desktop looks.
//!
//! Five tabs, in the order a person needs them:
//!
//! * **Themes**: pick the desktop design.
//! * **Palettes**: colours for every theme, light / dark / auto and wallpaper colours.
//! * **Customise**: one section at a time (Colours, Fonts, Window style, Dock & Bar and, once it exists, Control Center),
//!   each with a one-line explanation and a "Reset to theme default". Icons and Wallpaper have their own panes next to
//!   Appearance.
//! * **Wallpaper**: the pictures (My backgrounds and Wallhaven).
//! * **All apps**: one switch that themes GTK, Qt, terminals and the browser; the per-app choices sit under "Advanced".
//!
//! Config & Presets (text editing, presets, import and export) is the separate "Advanced" pane next to it.
//!
//! Nothing here stores anything new: the tabs only arrange the builders of the older panes (themepane, fontpane, iconspane,
//! systempane, wallhaven, the window-style and dock builders in panes.rs). Old pane names (`decsettings theme`, `icons`,
//! `wallpaper`, `desktop`) resolve to a tab through [`resolve`]; `icons` and `wallpaper` open their own panes. The static
//! [`INDEX`] makes every setting findable by search.

use super::model::{CtlVal, Model};
use super::themepane::Ctx;
use super::ui::{Ctl, Item, Kind, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Theme,
    Palettes,
    Customise,
    Wallpaper,
    Apps,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Theme, Tab::Palettes, Tab::Customise, Tab::Apps];

    pub fn id(self) -> &'static str {
        match self {
            Tab::Theme => "theme",
            Tab::Palettes => "palettes",
            Tab::Customise => "customise",
            Tab::Wallpaper => "wallpaper",
            Tab::Apps => "apps",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Theme => "Themes",
            Tab::Palettes => "Palettes",
            Tab::Customise => "Customise",
            Tab::Wallpaper => "Wallpaper",
            Tab::Apps => "All apps",
        }
    }

    /// What the current tab is for: the context line under the page title.
    pub fn blurb(self) -> &'static str {
        match self {
            Tab::Theme => "Pick a desktop design: its bar, dock, window style and fonts.",
            Tab::Palettes => "Recolour every theme: a colour set, light or dark, and your accent.",
            Tab::Customise => "Change single things on top of the theme, one section at a time.",
            Tab::Wallpaper => "Pictures for your desktop.",
            Tab::Apps => "Bring the theme to GTK, Qt, terminals and the browser.",
        }
    }

    pub fn parse(s: &str) -> Option<Tab> {
        if s == "wallpaper" {
            return Some(Tab::Wallpaper);
        }
        Tab::ALL.into_iter().find(|t| t.id() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Colours,
    Fonts,
    WindowStyle,
    DockBar,
    Icons,
    /// Filled in by [`control_center_items`] when the Control Center has settings; hidden until then.
    ControlCenter,
}

impl Section {
    pub const ALL: [Section; 6] = [Section::Colours, Section::Fonts, Section::WindowStyle, Section::DockBar, Section::Icons, Section::ControlCenter];

    pub fn id(self) -> &'static str {
        match self {
            Section::Colours => "colours",
            Section::Fonts => "fonts",
            Section::WindowStyle => "window",
            Section::DockBar => "dock",
            Section::Icons => "icons",
            Section::ControlCenter => "control",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Colours => "Colours",
            Section::Fonts => "Fonts",
            Section::WindowStyle => "Window style",
            Section::DockBar => "Dock & Bar",
            Section::Icons => "Icons",
            Section::ControlCenter => "Control Center",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Section::Colours => "How see-through and round menus and the dock are. Choose colours in Palettes.",
            Section::Fonts => "Which fonts the desktop uses and how big its text is.",
            Section::WindowStyle => "How windows look: title bars, the close / minimise / zoom buttons and borders.",
            Section::DockBar => "The dock, the menu bar, hot corners and which apps are pinned.",
            Section::Icons => "Which icon set the dock, launchers and menus use. Icons has its own pane in the sidebar.",
            Section::ControlCenter => "What the Control Center shows.",
        }
    }

    pub fn parse(s: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|t| t.id() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct View {
    pub tab: Tab,
    pub section: Section,
}

/// Where a pane name given to `decsettings NAME` (or any link) opens. `None` = not an Appearance name.
///
/// Old names keep working: `theme`, `icons`, `wallpaper`, `desktop` (and `dock`, `fonts`, `windowstyle`, `colours`, `allapps`) open
/// the pane / tab / section that now holds them (`icons` and `wallpaper` are panes of their own next to Appearance);
/// `appearance` opens the Theme tab; `appearance/fonts`, `appearance/customise` and so on are the explicit forms.
pub fn resolve(name: &str) -> Option<View> {
    let n = name.trim().to_lowercase();
    let n = n.strip_prefix("appearance/").unwrap_or(&n);
    let v = |tab, section| Some(View { tab, section });
    match n {
        "appearance" | "theme" | "themes" => v(Tab::Theme, Section::Colours),
        "customise" | "customize" | "customization" => v(Tab::Customise, Section::Colours),
        "palettes" | "palette" | "colours" | "colors" | "colour" | "color" | "accent" => v(Tab::Palettes, Section::Colours),
        "fonts" | "font" => v(Tab::Customise, Section::Fonts),
        "window" | "windows-style" | "windowstyle" | "titlebars" | "decoration" => v(Tab::Customise, Section::WindowStyle),
        "dock" | "desktop" | "bar" | "menubar" => v(Tab::Customise, Section::DockBar),
        "icons" | "icon" => v(Tab::Customise, Section::Icons),
        "control" | "controlcenter" | "control-center" => v(Tab::Customise, Section::ControlCenter),
        "wallpaper" | "wallpapers" | "background" | "wallhaven" => v(Tab::Wallpaper, Section::Colours),
        "apps" | "allapps" | "all-apps" | "system" | "system-theme" => v(Tab::Apps, Section::Colours),
        _ => None,
    }
}

impl View {
    /// The sidebar pane that shows this view, when it is not the Appearance pane itself (Wallpaper and Icons panes).
    pub fn pane(self) -> Option<&'static str> {
        match self.tab {
            Tab::Wallpaper => Some("wallpaper"),
            Tab::Customise if self.section == Section::Icons => Some("icons"),
            _ => None,
        }
    }
}

/// Every tab, and every section of Customise (tests and the screenshot tour).
pub fn views() -> Vec<View> {
    let mut v: Vec<View> = Tab::ALL.iter().filter(|t| **t != Tab::Customise).map(|t| View { tab: *t, section: Section::Colours }).collect();
    v.extend(Section::ALL.iter().map(|s| View { tab: Tab::Customise, section: *s }));
    v
}

/// The Control Center's own settings (toggles and tiles). `None` = nothing to show yet, the section stays hidden.
///
/// This is the slot for the Control Center work: return `Some(items)` and the "Control Center" section appears under
/// Appearance > Customise; nothing else needs to change.
pub fn control_center_items(_m: &Model) -> Option<Vec<Item>> {
    None
}

fn sections(m: &Model) -> Vec<Section> {
    // Icons has its own pane next to Appearance; Control Center appears once something fills the slot.
    Section::ALL.into_iter().filter(|s| *s != Section::ControlCenter || control_center_items(m).is_some()).filter(|s| *s != Section::Icons).collect()
}

/// Lazy data for the visible tab (called before every paint).
pub fn prepare(m: &mut Model) {
    match m.sel.tab {
        Tab::Customise if m.sel.section == Section::DockBar && m.apps.is_none() => m.apps = Some(super::panes::installed_apps()),
        Tab::Wallpaper if m.wh.tab == super::wallhaven::Tab::Wallhaven => super::wallhaven::prepare(m),
        Tab::Wallpaper if m.walls.is_none() => m.walls = Some(super::panes::list_images(&m.paths.backgrounds(), 3, 400)),
        _ => {}
    }
}

fn tab_bar(m: &Model) -> Item {
    let opts: Vec<(String, String)> = Tab::ALL.iter().map(|t| (t.title().to_string(), t.id().to_string())).collect();
    let sel = Tab::ALL.iter().position(|t| *t == m.sel.tab);
    Item::Nav { id: "ap:tab".into(), opts, sel, secondary: false }
}

fn section_bar(m: &Model) -> Vec<Item> {
    let secs = sections(m);
    let opts: Vec<(String, String)> = secs.iter().map(|t| (t.title().to_string(), t.id().to_string())).collect();
    let sel = secs.iter().position(|t| *t == m.sel.section);
    vec![Item::Nav { id: "ap:sec".into(), opts, sel, secondary: true }, Item::Note { text: m.sel.section.blurb().into(), warn: false }]
}

fn go_button(id: &str, label: &str) -> Ctl {
    Ctl::new(id, Kind::Button { label: label.into(), danger: false })
}

pub fn items(m: &Model) -> Vec<Item> {
    let mut out = vec![tab_bar(m)];
    match m.sel.tab {
        Tab::Theme => out.extend(theme_tab(m)),
        Tab::Palettes => out.extend(palettes_tab(m)),
        Tab::Customise => out.extend(customise_tab(m)),
        Tab::Wallpaper => out.extend(wallpaper_tab(m)),
        Tab::Apps => out.extend(super::systempane::items(m)),
    }
    out
}

fn theme_tab(m: &Model) -> Vec<Item> {
    let c = Ctx::new(m);
    let mut out = super::themepane::layout_items(&c);
    if let Some(note) = super::wallhaven::follow_theme_notice(m) {
        out.push(note);
    }
    out.extend(super::packages::items(m));
    out.push(Item::Header("More".into()));
    out.push(Item::Card(vec![
        Row::new("Fonts, window style, dock").detail("Change single things on top of the theme").ctl(go_button("ap:go:customise", "Customise")),
        Row::new("Presets, import and export, edit as text").detail("For advanced users").ctl(go_button("ap:go:config", "Config & Presets")),
    ]));
    out
}

fn palettes_tab(m: &Model) -> Vec<Item> {
    let c = Ctx::new(m);
    let mut out = super::themepane::scheme_items(&c);
    out.extend(super::themepane::mode_items(&c));
    out.extend(super::wallpapercolors::items(&c.sel, &c.now));
    out.extend(super::themepane::accent_items(&c));
    out
}

fn reset_card(id: &str, what: &str) -> Item {
    Item::Card(vec![Row::new("Reset to theme default").detail(what.to_string()).ctl(Ctl::new(id, Kind::Button { label: "Reset".into(), danger: false }))])
}

fn customise_tab(m: &Model) -> Vec<Item> {
    let mut out = section_bar(m);
    let c = Ctx::new(m);
    match m.sel.section {
        Section::Colours => {
            out.push(Item::Card(vec![Row::new("Colour palette").detail("Choose colours for every theme in Palettes").ctl(go_button("ap:go:palettes", "Open Palettes"))]));
            out.extend(super::themepane::surface_items(&c));
            out.push(reset_card("ap:reset:colours", "Back to the theme's own palette, accent, translucency and corner radius"));
        }
        Section::Fonts => {
            out.extend(super::themepane::text_size_items(&c));
            out.extend(super::fontpane::items(&c.sel, &c.now));
        }
        Section::WindowStyle => {
            out.extend(super::themepane::frame_sync_rows(&c));
            out.extend(m.window_style_items());
            out.push(reset_card("ap:reset:window", "Window style, title bar, border and button settings go back to their defaults, then the theme colours the windows again"));
        }
        Section::DockBar => {
            out.extend(m.dock_bar_items());
            out.push(reset_card("ap:reset:dock", "Dock spacing, magnification, transparency, roundness and style go back to their defaults"));
        }
        // Icons has its own pane (see `View::pane`); this stays so a legacy `show` of the view still renders the controls.
        Section::Icons => out.extend(super::iconspane::items(m)),
        Section::ControlCenter => out.extend(control_center_items(m).unwrap_or_default()),
    }
    out
}

pub(super) fn wallpaper_tab(m: &Model) -> Vec<Item> {
    let c = Ctx::new(m);
    let mut out = m.pane_wallpaper();
    out.splice(1.min(out.len())..1.min(out.len()), super::wallhaven::follow_theme_items(m));
    // The source switch stays first; colour matching belongs to this pane for every source.
    let colors = super::wallpapercolors::items(&c.sel, &c.now);
    let mut colors = colors;
    colors.push(super::wallpapercolors::save_items(m));
    if m.wh.tab == super::wallhaven::Tab::Live {
        out.extend(colors);
    } else {
        out.splice(1.min(out.len())..1.min(out.len()), colors);
    }
    out
}

// ---------------------------------------------------------------- events

/// Settings a "Reset to theme default" of Window style removes from rdwm's file (the look, not the behaviour).
const WINDOW_STYLE_KEYS: &[(&str, &str)] = &[
    ("decoration", "style"),
    ("decoration", "height"),
    ("decoration", "radius"),
    ("decoration", "title_align"),
    ("decoration", "font_size"),
    ("decoration", "border_px"),
    ("decoration", "close_color"),
    ("decoration", "minimize_color"),
    ("decoration", "zoom_color"),
    ("decoration", "inactive_button_color"),
    ("decoration", "focused_bg"),
    ("decoration", "focused_fg"),
    ("decoration", "unfocused_bg"),
    ("decoration", "unfocused_fg"),
    ("general", "border_px"),
    ("colors", "focused"),
    ("colors", "normal"),
    ("colors", "urgent"),
];

const DOCK_KEYS: &[&str] = &["appSpacing", "magnification", "transparency", "roundness", "style"];

/// Opens `view`: the tab, the section and the top of the page.
pub fn show(m: &mut Model, view: View) {
    if m.sel.tab != view.tab || m.sel.section != view.section {
        m.sel.reset_scroll = true;
    }
    m.sel.tab = view.tab;
    if view.tab == Tab::Customise {
        m.sel.section = view.section;
    }
    m.dirty = true;
}

/// Selects the pane that shows `view` (Appearance, Wallpaper or Icons) on `view`.
pub fn open(m: &mut Model, view: View) {
    if let Some(i) = super::system::registry::pane_index(view.pane().unwrap_or("appearance")) {
        m.sel.goto = Some(i);
    }
    if view.pane().is_none() {
        show(m, view);
    } else {
        m.sel.reset_scroll = true;
        m.dirty = true;
    }
}

pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    match (parts, v) {
        (["ap", "tab"], CtlVal::Str(s)) => {
            if let Some(t) = Tab::parse(&s) {
                if t == Tab::Wallpaper {
                    open(m, View { tab: t, section: m.sel.section });
                } else {
                    show(m, View { tab: t, section: m.sel.section });
                }
            }
        }
        (["ap", "sec"], CtlVal::Str(s)) => {
            if let Some(sec) = Section::parse(&s) {
                show(m, View { tab: Tab::Customise, section: sec });
            }
        }
        (["ap", "adv"], CtlVal::Click) => {
            m.sel.adv = !m.sel.adv;
            m.dirty = true;
        }
        (["ap", "go", "config"], CtlVal::Click) => {
            if let Some(i) = super::system::registry::pane_index("config") {
                m.sel.goto = Some(i);
                m.sel.reset_scroll = true;
                m.dirty = true;
            }
        }
        (["ap", "go", name], CtlVal::Click) => {
            if let Some(view) = resolve(name) {
                open(m, view);
            }
        }
        (["ap", "reset", "colours"], CtlVal::Click) => super::themepane::apply(m, &["th", "reset-colours"], CtlVal::Click),
        (["ap", "reset", "window"], CtlVal::Click) => {
            m.edit(|c| {
                for (sec, key) in WINDOW_STYLE_KEYS {
                    c.remove(sec, key);
                }
                Ok(())
            });
            // the theme paints rdwm's colours again (when "Windows follow the theme" is on)
            let sel = m.theme_selection();
            super::themepane::commit(m, sel);
        }
        (["ap", "reset", "dock"], CtlVal::Click) => {
            for k in DOCK_KEYS {
                if m.dock.get(*k).is_some() {
                    m.edit_json("dock", k, None);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- search

/// Every setting of the tabs, for the search box: (view, label, extra words). The pane test below checks that every row the
/// tabs draw is covered, so a new setting cannot silently be unfindable.
pub const INDEX: &[(Tab, Section, &str, &str)] = &[
    // Theme
    (Tab::Theme, Section::Colours, "Layout", "style macos windows caelestia shell layout bar dock look"),
    (Tab::Theme, Section::Colours, "Themes", "theme desktop design packages cards apply preview export duplicate delete install create macos windows 11 caelestia catalog"),
    (Tab::Palettes, Section::Colours, "Mode", "light dark auto system night day appearance"),
    (Tab::Palettes, Section::Colours, "Match wallpaper colours", "dynamic material you colours from wallpaper recolour scheme strength contrast"),
    (Tab::Theme, Section::Colours, "Get more themes", "market community download"),
    (Tab::Theme, Section::Colours, "Create theme from current settings", "save snapshot new theme package"),
    (Tab::Theme, Section::Colours, "Install a theme file", "import dtheme package folder"),
    // Customise > Colours
    (Tab::Palettes, Section::Colours, "Accent colour", "blue purple pink red orange yellow green graphite highlight swatches hex"),
    (Tab::Palettes, Section::Colours, "Custom accent", "hex colour #rrggbb own accent"),
    (
        Tab::Palettes,
        Section::Colours,
        "Colour palette",
        "colour scheme colors palette catppuccin rose pine gruvbox tokyo night everforest kanagawa nord dracula solarized monochrome recolour theme tiles",
    ),
    (Tab::Customise, Section::Colours, "Translucency", "opacity transparent see through menus dock launchers"),
    (Tab::Customise, Section::Colours, "Corner radius", "rounded corners round menus cards title bars"),
    // Customise > Fonts
    (Tab::Customise, Section::Fonts, "Text size", "font scale larger smaller zoom text"),
    (Tab::Customise, Section::Fonts, "Font preset", "inter sf pro segoe typeface family"),
    (Tab::Customise, Section::Fonts, "Interface font", "ui text typeface family sans"),
    (Tab::Customise, Section::Fonts, "Title font", "titles window titles typeface"),
    (Tab::Customise, Section::Fonts, "Monospace font", "mono code terminal nerd font typeface"),
    (Tab::Customise, Section::Fonts, "Clock and numbers", "display digits typeface clock font"),
    // Customise > Window style
    (Tab::Customise, Section::WindowStyle, "Windows follow the theme", "sync rdwm title bars borders colours theme"),
    (Tab::Customise, Section::WindowStyle, "Window title bars", "titlebar decoration enabled show hide"),
    (
        Tab::Customise,
        Section::WindowStyle,
        "Button and titlebar design",
        "macos classic windows gnome adwaita kde breeze arc material materia nord gruvbox dracula minimal xfce xfwm xfwm4 default daloa kokodi raised bevel titlebar decoration button look",
    ),
    (Tab::Customise, Section::WindowStyle, "Title bar height", "titlebar size thick"),
    (Tab::Customise, Section::WindowStyle, "Window corner radius", "rounded titlebar radius"),
    (Tab::Customise, Section::WindowStyle, "Show window shadows", "shadow shadows whole window effect compositor x11 decoration"),
    (Tab::Customise, Section::WindowStyle, "Shadow softness", "shadow falloff blur radius margin"),
    (Tab::Customise, Section::WindowStyle, "Focused shadow strength", "shadow opacity active window strength"),
    (Tab::Customise, Section::WindowStyle, "Unfocused shadow strength", "shadow opacity inactive window strength"),
    (Tab::Customise, Section::WindowStyle, "Title alignment", "center left title text"),
    (Tab::Customise, Section::WindowStyle, "Title text size", "titlebar font size"),
    (Tab::Customise, Section::WindowStyle, "Window buttons", "close minimise minimize zoom traffic lights"),
    (Tab::Customise, Section::WindowStyle, "Maximise / zoom button", "zoom fullscreen maximize maximise monocle float traffic light"),
    (Tab::Customise, Section::WindowStyle, "Double-click on title", "maximize fullscreen titlebar action"),
    (Tab::Customise, Section::WindowStyle, "Middle click closes", "close title"),
    (Tab::Customise, Section::WindowStyle, "Title bars in full-screen stacking", "monocle titlebars"),
    (Tab::Customise, Section::WindowStyle, "Window border", "border width thickness px"),
    (Tab::Customise, Section::WindowStyle, "Focused border", "active window border colour"),
    (Tab::Customise, Section::WindowStyle, "Unfocused border", "inactive window border colour"),
    (Tab::Customise, Section::WindowStyle, "Urgent border", "attention window border colour"),
    (Tab::Customise, Section::WindowStyle, "Title bar border", "titlebar border width"),
    (Tab::Customise, Section::WindowStyle, "Close button", "red colour traffic light"),
    (Tab::Customise, Section::WindowStyle, "Minimise button", "yellow colour traffic light minimize"),
    (Tab::Customise, Section::WindowStyle, "Zoom button", "green colour traffic light"),
    (Tab::Customise, Section::WindowStyle, "Buttons of unfocused windows", "inactive traffic lights colour grey"),
    (Tab::Customise, Section::WindowStyle, "Focused title bar", "titlebar background colour"),
    (Tab::Customise, Section::WindowStyle, "Focused title text", "titlebar foreground colour"),
    (Tab::Customise, Section::WindowStyle, "Unfocused title bar", "titlebar background colour inactive"),
    (Tab::Customise, Section::WindowStyle, "Unfocused title text", "titlebar foreground colour inactive"),
    // Customise > Dock & Bar
    (Tab::Customise, Section::DockBar, "Dock style", "macos windows taskbar floating dock"),
    (Tab::Customise, Section::DockBar, "Menu bar position", "panel edge top bottom left right placement"),
    (Tab::Customise, Section::DockBar, "Floating bar position", "panel edge top bottom left right placement"),
    (Tab::Customise, Section::DockBar, "Pill rail position", "caelestia panel edge top bottom left right split"),
    (Tab::Customise, Section::DockBar, "Dux bar position", "panel edge top bottom left right placement"),
    (Tab::Customise, Section::DockBar, "Taskbar position", "windows panel edge top bottom left right"),
    (Tab::Customise, Section::DockBar, "Dock position", "dock edge top bottom left right"),
    (Tab::Customise, Section::DockBar, "Show the bar", "bar enabled panel on off"),
    (Tab::Customise, Section::DockBar, "Theme defaults", "reset panel applets position theme default"),
    (Tab::Customise, Section::DockBar, "Applet: Launcher", "panel applet launcher show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Application menu", "panel applet app menu show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Workspaces", "panel applet workspaces show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Window title", "panel applet window title show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Task buttons", "panel applet tasks show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Window layout", "panel applet layout show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Media", "panel applet media show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Clock and date", "panel applet clock show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Weather", "panel applet weather show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: System tray", "panel applet tray show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Clipboard history", "panel applet clipboard show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: CPU usage", "panel applet cpu show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Memory usage", "panel applet memory show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Notifications", "panel applet notifications show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Stay Awake", "panel applet stay awake show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Process monitor", "panel applet agents show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Caps / Num Lock", "panel applet lock keys show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Volume", "panel applet volume show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Network", "panel applet network show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Bluetooth", "panel applet bluetooth show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Battery", "panel applet battery show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Brightness", "panel applet brightness show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Search", "panel applet search show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Dashboard", "panel applet dashboard show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Control Center", "panel applet control center show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Power", "panel applet power show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Applet: Show desktop", "panel applet show desktop show hide enable disable"),
    (Tab::Customise, Section::DockBar, "Icon spacing", "dock gap app spacing"),
    (Tab::Customise, Section::DockBar, "Magnification", "dock zoom hover grow"),
    (Tab::Customise, Section::DockBar, "Transparency", "dock translucent opacity"),
    (Tab::Customise, Section::DockBar, "Roundness", "dock rounded corners"),
    (Tab::Customise, Section::DockBar, "Restart Dock", "apply decdock"),
    (Tab::Customise, Section::DockBar, "Workspace dots", "menu bar workspaces tags indicator decbar"),
    (Tab::Customise, Section::DockBar, "Restart Menu Bar", "apply decbar"),
    (Tab::Customise, Section::DockBar, "Window manager strip", "builtin bar rdwm dwm bar show hide"),
    (Tab::Customise, Section::DockBar, "Strip position", "bar top bottom position"),
    (Tab::Customise, Section::DockBar, "Strip height", "bar height size"),
    (Tab::Customise, Section::DockBar, "Hot corners", "screen corners expose launchpad desktop dwell"),
    (Tab::Customise, Section::DockBar, "Show the Control Center", "control center quick settings panel enable disable tray button"),
    (Tab::Customise, Section::DockBar, "Opens in", "control center position corner where the panel opens"),
    (Tab::Customise, Section::DockBar, "Wi-Fi, Bluetooth, Network", "control center tiles connectivity wifi bluetooth network"),
    (Tab::Customise, Section::DockBar, "Focus, Night Shift, Dark Mode", "control center tiles focus do not disturb night shift dark mode"),
    (Tab::Customise, Section::DockBar, "Sound", "control center tiles volume sound slider output"),
    (Tab::Customise, Section::DockBar, "Now Playing", "control center tiles media music player now playing"),
    (Tab::Customise, Section::DockBar, "Stay Awake, Screenshot, Notifications, Settings", "control center tiles stay awake caffeine screenshot notifications settings"),
    (Tab::Customise, Section::DockBar, "Record, Idle, Game Mode, DND", "control center tiles screen record idle inhibit game mode do not disturb"),
    (Tab::Customise, Section::DockBar, "Top left", "hot corner"),
    (Tab::Customise, Section::DockBar, "Top right", "hot corner"),
    (Tab::Customise, Section::DockBar, "Bottom left", "hot corner"),
    (Tab::Customise, Section::DockBar, "Bottom right", "hot corner"),
    (Tab::Customise, Section::DockBar, "Dwell time", "hot corner delay"),
    (Tab::Customise, Section::DockBar, "Add an app", "pin dock launcher favourites"),
    (Tab::Customise, Section::DockBar, "Pinned apps", "dock favourites favorites add remove reorder launchers"),
    // Customise > Icons (also the Icons pane; `registry::search` sends these to the Icons pane)
    (Tab::Customise, Section::Icons, "Icon theme", "papirus tela whitesur adwaita breeze yaru auto gtk gsettings icon pack"),
    (Tab::Customise, Section::Icons, "Icon size", "scale artwork padding dock"),
    (Tab::Customise, Section::Icons, "Tint symbolic icons", "monochrome text colour"),
    (Tab::Customise, Section::Icons, "Fallback theme", "hicolor missing icons"),
    (Tab::Customise, Section::Icons, "Cursor theme", "mouse pointer cursor bibata adwaita"),
    // Wallpaper
    (Tab::Wallpaper, Section::Colours, "For this theme", "curated recommended photography wallhaven dark light wallpaper"),
    (Tab::Wallpaper, Section::Colours, "Wallpaper", "background picture image desktop wallhaven"),
    (Tab::Wallpaper, Section::Colours, "Scaling", "fill fit stretch center tile wallpaper mode"),
    (Tab::Wallpaper, Section::Colours, "Background colour", "wallpaper letterbox solid colour"),
    (Tab::Wallpaper, Section::Colours, "Image file", "wallpaper path png jpeg webp"),
    (Tab::Wallpaper, Section::Colours, "Wallhaven", "online download search random favourites wallpapers"),
    (Tab::Wallpaper, Section::Colours, "Custom palette name", "wallpaper extract generate save named palette colours"),
    (Tab::Wallpaper, Section::Colours, "Create wallpaper palette", "wallpaper palette create apply saved dark light"),
    (Tab::Wallpaper, Section::Colours, "Live wallpapers", "video animated mp4 webm commons motion fullscreen pause frame rate"),
    // All apps
    (Tab::Apps, Section::Colours, "Apply theme to all apps", "system wide theming gtk qt terminal chrome browser nautilus kvantum switch"),
    (Tab::Apps, Section::Colours, "Apply now", "run again re-apply theme apps"),
    (Tab::Apps, Section::Colours, "Browser colour", "chrome chromium brave theme color toolbar"),
    (Tab::Apps, Section::Colours, "Omarchy theme hooks", "hooks omarchy theme-set scripts"),
    (Tab::Apps, Section::Colours, "Qt apps", "qt5 qt6 qt6ct kvantum"),
    (Tab::Apps, Section::Colours, "Terminals and Omarchy apps", "alacritty kitty foot ghostty waybar mako"),
    (Tab::Apps, Section::Colours, "Theme font in GTK apps", "gtk font"),
    (Tab::Apps, Section::Colours, "X11 terminals", "xterm urxvt xresources"),
    (Tab::Apps, Section::Colours, "Choose what is themed", "advanced targets per app options"),
    (Tab::Apps, Section::Colours, "File manager appearance", "files nautilus explorer windows finder macos profile spacing"),
    (Tab::Apps, Section::Colours, "GTK apps", "gtk3 gtk4 libadwaita nautilus omarchy hooks editors browser chrome qt kvantum x11 per app"),
    (Tab::Apps, Section::Colours, "Icon theme for apps", "gtk qt icons papirus"),
    (Tab::Apps, Section::Colours, "Cursor theme for apps", "xcursor gtk x11 pointer"),
];

/// The view a search hit labelled `label` opens (labels of [`INDEX`] are unique per view).
pub fn view_of_label(label: &str) -> Option<View> {
    INDEX.iter().find(|(_, _, l, _)| l.eq_ignore_ascii_case(label)).map(|(t, s, _, _)| View { tab: *t, section: *s })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;

    #[test]
    fn shadow_search_labels_open_window_style() {
        for label in ["Show window shadows", "Shadow softness", "Focused shadow strength", "Unfocused shadow strength"] {
            let view = view_of_label(label).expect("shadow setting is indexed");
            assert_eq!(view.tab, Tab::Customise);
            assert_eq!(view.section, Section::WindowStyle);
        }
    }

    #[test]
    fn themes_and_palettes_have_separate_controls_and_search_destinations() {
        let (mut m, _) = model_with("[general]\n");
        m.sel.tab = Tab::Theme;
        let themes = items(&m);
        assert!(themes.iter().any(|i| matches!(i, Item::ThemeCards(_))));
        assert!(!themes.iter().any(|i| matches!(i, Item::Themes(_))));
        assert!(!themes.iter().any(|i| matches!(i, Item::Card(rows) if rows.iter().any(|r| r.ctls.iter().any(|c| c.id == "th:style")))));
        m.sel.tab = Tab::Palettes;
        let palettes = items(&m);
        assert!(palettes.iter().any(|i| matches!(i, Item::Themes(_))));
        assert!(!palettes.iter().any(|i| matches!(i, Item::ThemeCards(_))));
        assert_eq!(resolve("appearance/palettes").unwrap().tab, Tab::Palettes);
        assert_eq!(resolve("colours").unwrap().tab, Tab::Palettes);
        for label in ["Colour palette", "Accent colour", "Mode", "Match wallpaper colours"] {
            assert_eq!(INDEX.iter().find(|(_, _, l, _)| *l == label).unwrap().0, Tab::Palettes);
        }
    }

    #[test]
    fn old_pane_names_resolve_to_their_new_home() {
        let v = |n: &str| resolve(n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!(v("theme"), View { tab: Tab::Theme, section: Section::Colours });
        assert_eq!(v("appearance").tab, Tab::Theme);
        assert_eq!(v("icons"), View { tab: Tab::Customise, section: Section::Icons });
        assert_eq!(v("icons").pane(), Some("icons"), "icons opens its own pane");
        assert_eq!(v("icon").pane(), Some("icons"));
        assert_eq!(v("desktop"), View { tab: Tab::Customise, section: Section::DockBar });
        assert_eq!(v("wallpaper").tab, Tab::Wallpaper);
        assert_eq!(v("appearance/fonts"), View { tab: Tab::Customise, section: Section::Fonts });
        assert_eq!(v("system-theme").tab, Tab::Apps);
        assert!(resolve("tiling").is_none() && resolve("zzz").is_none());
    }

    #[test]
    fn every_tab_and_section_builds_and_has_a_tab_bar() {
        let (mut m, _) = model_with("[general]\n");
        for t in Tab::ALL {
            for s in Section::ALL {
                m.sel.tab = t;
                m.sel.section = s;
                let its = items(&m);
                assert!(!its.is_empty(), "{t:?}/{s:?}");
                assert!(matches!(&its[0], Item::Nav { id, secondary: false, .. } if id == "ap:tab"), "the tab bar leads {t:?}/{s:?}");
                if t == Tab::Customise {
                    assert!(matches!(&its[1], Item::Nav { id, secondary: true, .. } if id == "ap:sec"), "the section chips follow {t:?}/{s:?}");
                }
                if t != Tab::Customise {
                    break; // sections only matter in Customise
                }
            }
        }
    }

    #[test]
    fn tabs_and_sections_switch_and_reset_the_scroll() {
        let (mut m, _) = model_with("[general]\n");
        m.apply("ap:tab", CtlVal::Str("customise".into()));
        assert_eq!(m.sel.tab, Tab::Customise);
        assert!(m.sel.reset_scroll);
        m.apply("ap:sec", CtlVal::Str("fonts".into()));
        assert_eq!((m.sel.tab, m.sel.section), (Tab::Customise, Section::Fonts));
        m.apply("ap:go:wallpaper", CtlVal::Click);
        assert_eq!(m.sel.goto, super::super::system::registry::pane_index("wallpaper"));
        m.apply("ap:go:config", CtlVal::Click);
        assert!(m.sel.goto.is_some());
        m.apply("ap:tab", CtlVal::Str("nonsense".into()));
        assert_eq!(m.sel.tab, Tab::Customise);
    }

    #[test]
    fn the_control_center_section_is_hidden_until_something_fills_the_slot() {
        let (m, _) = model_with("[general]\n");
        assert!(!sections(&m).contains(&Section::ControlCenter));
    }

    /// Every row the tabs draw must be findable: its label is an INDEX entry (or a prefix of one).
    #[test]
    fn every_row_of_every_tab_is_in_the_search_index() {
        let (mut m, _) = model_with("[decoration]\nenabled = true\n");
        m.sel.adv = true;
        let mut missing = Vec::new();
        for t in Tab::ALL {
            for s in Section::ALL {
                m.sel.tab = t;
                m.sel.section = s;
                m.apps = Some(vec![]);
                for it in items(&m) {
                    if let Item::Card(rows) = it {
                        for r in rows {
                            let l = r.label.trim().trim_end_matches('…');
                            // tab / section selectors, the "Pictures from" switch, per-device and generated rows
                            let generated = r.ctls.iter().any(|c| c.id.starts_with("p:") || c.id.starts_with("ap:") || c.id.starts_with("sy:t:") || c.id.starts_with("sy:o:"));
                            if l.is_empty() || generated || l.starts_with("Reset") || l == "In use" || ["Customise", "Pictures from"].contains(&l) {
                                continue;
                            }
                            let known = INDEX.iter().any(|(_, _, il, words)| il.eq_ignore_ascii_case(l) || il.to_lowercase().contains(&l.to_lowercase()) || words.contains(&l.to_lowercase()));
                            if !known {
                                missing.push(format!("{t:?}/{s:?}: {l}"));
                            }
                        }
                    }
                }
                if t != Tab::Customise {
                    break;
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "settings missing from appearance::INDEX: {missing:#?}");
    }

    #[test]
    fn index_labels_are_unique_and_found_by_search() {
        let mut seen = std::collections::HashSet::new();
        for (_, _, l, _) in INDEX {
            assert!(seen.insert(l.to_lowercase()), "duplicate label {l}");
        }
        let h = crate::settings::system::registry::search("accent", &[]);
        assert!(h.iter().any(|x| x.label.as_deref() == Some("Accent colour")), "{h:?}");
    }

    #[test]
    fn xfce_styles_are_found_by_search_and_offered_in_the_window_style_picker() {
        for q in ["xfce", "daloa", "kokodi", "xfwm4"] {
            let h = crate::settings::system::registry::search(q, &[]);
            assert!(h.iter().any(|x| x.label.as_deref() == Some("Button and titlebar design")), "{q}: {h:?}");
        }
        let (mut m, _) = model_with("[decoration]\nenabled = true\n");
        m.sel.tab = Tab::Customise;
        m.sel.section = Section::WindowStyle;
        let values: Vec<String> = items(&m)
            .into_iter()
            .filter_map(|i| if let Item::Card(rows) = i { Some(rows) } else { None })
            .flatten()
            .flat_map(|r| r.ctls)
            .filter(|c| c.id == "r:decoration:style")
            .flat_map(|c| if let Kind::Popup(o, _) = c.kind { o.into_iter().map(|(_, v)| v).collect() } else { Vec::new() })
            .collect();
        for id in ["xfce-default", "xfce-daloa", "xfce-kokodi", "adwaita"] {
            assert!(values.iter().any(|v| v == id), "{id} not offered: {values:?}");
        }
        assert!(!values.iter().any(|v| v == "borderonly"));
    }

    #[test]
    fn a_window_style_fallback_warning_comes_from_the_model_snapshot_and_stays_until_cleared() {
        let (mut m, _) = model_with("[decoration]\nenabled = true\n");
        m.sel.tab = Tab::Customise;
        m.sel.section = Section::WindowStyle;
        let notes = |m: &Model| -> Vec<String> { items(m).into_iter().filter_map(|i| if let Item::Note { text, warn: true } = i { Some(text) } else { None }).collect() };
        assert!(!notes(&m).iter().any(|t| t.contains("needs a newer rdwm")));
        // the snapshot of decebal.json decides; nothing is read from disk while the controls are built
        let _ = std::fs::remove_file(m.paths.shell_json());
        m.shell = serde_json::json!({"decoration_compat": {"theme": "x", "mode": "dark", "desired": "lingmo-dark", "applied": "adwaita", "proof": "running",
            "message": "Window style `lingmo-dark` needs a newer rdwm: the running rdwm does not list it, so `adwaita` is used for now."}});
        assert!(notes(&m).iter().any(|t| t.contains("needs a newer rdwm")), "{:?}", notes(&m));
        m.shell = serde_json::json!({});
        assert!(!notes(&m).iter().any(|t| t.contains("needs a newer rdwm")));
    }

    #[test]
    fn window_style_reset_removes_the_look_but_not_the_behaviour() {
        let (mut m, cfg) = model_with("[decoration]\nenabled = true\nheight = 40\nradius = 3\nmiddle_click_close = true\n[general]\nborder_px = 4\n");
        m.apply("ap:reset:window", CtlVal::Click);
        let t = std::fs::read_to_string(&cfg).unwrap();
        // the theme paints rdwm's colours and sizes again, so what the user had typed in is gone
        assert!(!t.contains("height = 40") && !t.contains("radius = 3\n") && !t.contains("border_px = 4"), "{t}");
        assert!(t.contains("middle_click_close") && t.contains("enabled"), "{t}");
    }
}

#[cfg(test)]
mod wallpaper_navigation_tests {
    use super::*;
    use crate::settings::{model::tests_support::model_with, system::registry};
    #[test]
    fn wallpaper_has_its_own_pane_and_leaves_appearance_selection_intact() {
        let (mut m, _) = model_with("[general]\n");
        m.sel.tab = Tab::Palettes;
        m.apply("ap:go:wallpaper", CtlVal::Click);
        assert_eq!(m.sel.goto, registry::pane_index("wallpaper"));
        assert_eq!(m.sel.tab, Tab::Palettes);
        m.sel.pane = registry::pane_index("wallpaper").unwrap();
        assert!(m.wallpaper_open());
        assert!(!Tab::ALL.contains(&Tab::Wallpaper));
        assert!(m.items(m.sel.pane).iter().all(|i| !matches!(i,Item::Card(rows) if rows.iter().any(|r|r.ctls.iter().any(|c|c.id=="ap:tab")))));
    }

    #[test]
    fn icons_have_their_own_pane_and_leave_appearance_selection_intact() {
        let (mut m, _) = model_with("[general]\n");
        m.sel.tab = Tab::Palettes;
        m.apply("ap:go:icons", CtlVal::Click);
        assert_eq!(m.sel.goto, registry::pane_index("icons"));
        assert_eq!(m.sel.tab, Tab::Palettes, "the Icons pane does not change the Appearance tab");
        m.sel.pane = registry::pane_index("icons").unwrap();
        let its = m.items(m.sel.pane);
        assert!(its.iter().any(|i| matches!(i, Item::Card(rows) if rows.iter().any(|r| r.ctls.iter().any(|c| c.id == "ic:use")))), "the Icons pane shows the icon theme chooser");
        assert!(!Tab::ALL.contains(&Tab::Wallpaper));
        // Icons is no longer a visible Customise section: the pane is the one discoverable home
        assert!(!sections(&m).contains(&Section::Icons));
    }
}
