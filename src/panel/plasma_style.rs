//! Typed, theme-neutral taskbar appearance: the optional `[taskbar]` section of a theme package, and the pure geometry
//! behind it (panel thickness, inner padding, icon size, the stateful task indicator, pager cells).
//!
//! Nothing here knows a theme id. A package that writes no `[taskbar]` keys gets [`TaskbarStyle::default`], which changes
//! nothing: the Windows 11 and Windows 10 looks keep their own metrics, indicator, show-desktop sliver and workspace
//! number. A port of another desktop describes its look with the same keys, and the formulas here are the ones the KDE
//! Plasma sources use (plasma-desktop `c68d33a9`, plasma-workspace `dec158c0`, libplasma `6a9ded3b`, kirigami `1ea0abb5`),
//! so a later port can reuse them with its own numbers.
//!
//! The taskbar model ([`crate::taskbar::Metrics::themed`]), its painter ([`crate::taskbar::draw`]), the dock app's strip
//! geometry and the native previews read [`TaskbarStyle`] from [`crate::theme::Theme::taskbar`].

use super::Edge;
use serde_json::Value;

/// How the focused, hovered, minimised or demanding task is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Indicator {
    /// The renderer's own: a rounded plate and a pill (Windows 11) or an underline (Windows 10).
    #[default]
    Auto,
    /// A tinted cell with a strong line on the side that faces the desktop ([`task_indicator`]).
    Frame,
}

/// What the show-desktop applet looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopForm {
    /// The thin sliver at the end of the strip (Windows).
    #[default]
    Auto,
    /// A square cell with a desktop icon.
    Icon,
}

/// How the workspace applet draws its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceForm {
    /// The renderer's own: the current workspace's name in a box.
    #[default]
    Auto,
    /// One miniature cell per workspace, the current one highlighted ([`pager_cells`]).
    Cells,
}

/// What the search applet looks like on a horizontal strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchForm {
    /// The dock setting decides (the Windows search box by default).
    #[default]
    Auto,
    /// A square icon button, whatever the dock setting's default box says (UKUI's `ukui-panel-search` is a 32 px `kylin-search` button).
    Button,
}

/// The `[taskbar]` section. Sizes are pixels at scale 1.0; `None` keeps the renderer's own value.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TaskbarStyle {
    /// Strip thickness (the height of a horizontal strip, the width of a side strip).
    pub thickness: Option<f32>,
    /// Drawn size of a task or Start icon. Without it: `thickness - 2 * inner padding` when a thickness is set.
    pub icon: Option<f32>,
    /// Width of a task or Start cell along the strip.
    pub cell_width: Option<f32>,
    /// Space between cells.
    pub gap: Option<f32>,
    /// How far a cell's plate stays inside the strip across it (0 = the cell fills the strip).
    pub inset: Option<f32>,
    /// Distance between the strip and the screen edge and its two ends (a floating panel); 0 = flush.
    pub float_gap: f32,
    pub indicator: Indicator,
    pub desktop: DesktopForm,
    pub workspaces: WorkspaceForm,
    pub search: SearchForm,
}

fn num(t: &Value, key: &str, lo: f64, hi: f64) -> Option<f32> {
    t.get(key).and_then(Value::as_f64).filter(|v| (lo..=hi).contains(v)).map(|v| v as f32)
}

impl TaskbarStyle {
    /// From a theme's extra sections. Invalid values mean the renderer's own (the manifest validator reports them).
    pub fn from_extra(extra: &serde_json::Map<String, Value>) -> TaskbarStyle {
        let Some(t) = extra.get("taskbar") else { return TaskbarStyle::default() };
        let choice = |key: &str| t.get(key).and_then(Value::as_str);
        TaskbarStyle {
            thickness: num(t, "thickness", 24.0, 96.0),
            icon: num(t, "icon_size", 12.0, 64.0),
            cell_width: num(t, "cell_width", 24.0, 120.0),
            gap: num(t, "gap", 0.0, 16.0),
            inset: num(t, "inset", 0.0, 16.0),
            float_gap: num(t, "float_gap", 0.0, 32.0).unwrap_or(0.0),
            indicator: if choice("indicator") == Some("frame") { Indicator::Frame } else { Indicator::Auto },
            desktop: if choice("desktop") == Some("icon") { DesktopForm::Icon } else { DesktopForm::Auto },
            workspaces: if choice("workspaces") == Some("cells") { WorkspaceForm::Cells } else { WorkspaceForm::Auto },
            search: if choice("search") == Some("button") { SearchForm::Button } else { SearchForm::Auto },
        }
    }

    /// No key set: the renderer looks exactly as it did before `[taskbar]` existed.
    pub fn is_default(&self) -> bool {
        *self == TaskbarStyle::default()
    }

    /// The drawn icon size at scale 1.0 for a strip of `thickness`: the explicit size, else the thickness minus the padding
    /// on both sides (when a thickness is set), else `None`.
    pub fn icon_px(&self) -> Option<f32> {
        self.icon.or_else(|| self.thickness.map(|t| icon_size(t as u32, SVG_MARGIN) as f32))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Panel metrics (plasma-desktop Panel.qml; kirigami units). The thickness itself (layout.js: 2*ceil(gridUnit*2.5/2), 44 at grid unit 17)
// and the floating gap (Panel.qml: 8 px) are numbers a package writes in `[taskbar]`; tests/plasma_theme_ports.rs pins them to the sources.

/// Kirigami's default small spacing, in pixels.
pub const SMALL_SPACING: u32 = 4;
/// Panel SVG margin Plasma's Breeze panel adds to its inner padding (`thickPanelSvg.fixedMargins`, 4 px).
pub const SVG_MARGIN: u32 = 4;
/// `Panel.qml`: `spacingAtMinSize = floor(max(1, thickness - 22) / 2)`; padding = `min(margin + smallSpacing, spacingAtMinSize)`.
pub fn inner_padding(thickness: u32, svg_margin: u32) -> u32 {
    (svg_margin + SMALL_SPACING).min(thickness.saturating_sub(22).max(1) / 2)
}

/// The drawn icon fills the strip minus its two paddings: 44 - 2 x 8 = 28.
pub fn icon_size(thickness: u32, svg_margin: u32) -> u32 {
    thickness.saturating_sub(2 * inner_padding(thickness, svg_margin)).max(1)
}

// ---------------------------------------------------------------------------------------------------------------------
// The stateful task indicator (libplasma breeze widgets/tasks.svg, taskmanager Task.qml and TaskTools.js)

/// The state of a task, in the order Task.qml prefers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Normal,
    Hover,
    Active,
    Minimized,
    Attention,
}

/// Which colour a state is drawn in (the SVG's `ColorScheme-*` classes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// `ColorScheme-ButtonFocus` / `ButtonHover`: the accent.
    Accent,
    /// `ColorScheme-Text`.
    Text,
    /// `ColorScheme-NeutralText`: the warning colour.
    Warn,
}

/// One state's drawing: a fill over the whole cell and a three-pixel line on every side. The side facing the desktop
/// carries the strong line (`TaskTools.taskPrefix`: `north-`, `west-`, `east-`, `south-` sets).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndicatorSpec {
    pub tone: Tone,
    pub fill_alpha: f32,
    /// Alpha of the line on the three sides that do not face the desktop.
    pub line_alpha: f32,
    /// Alpha of the line on the side facing the desktop.
    pub strong_alpha: f32,
    /// Thickness of those lines at scale 1.0 (2 px + 1 px in the SVG).
    pub line_px: f32,
    /// How far the lines stay from the cell's corners (the SVG's 5 px).
    pub corner_inset: f32,
    /// The side of the cell that faces the desktop for this panel edge.
    pub strong_side: Edge,
}

/// The indicator of `state` on a panel at `panel_edge`. `Normal` draws nothing. Numbers are tasks.svg's: focus fill .45 with
/// .45 lines and a .90 line towards the desktop, hover fill .34, minimised text .08, attention warning .35.
pub fn task_indicator(state: TaskState, panel_edge: Edge) -> Option<IndicatorSpec> {
    let strong_side = match panel_edge {
        Edge::Bottom => Edge::Top,
        Edge::Top => Edge::Bottom,
        Edge::Left => Edge::Right,
        Edge::Right => Edge::Left,
    };
    let (tone, fill_alpha, lines) = match state {
        TaskState::Normal => return None,
        TaskState::Active => (Tone::Accent, 0.45, true),
        TaskState::Hover => (Tone::Accent, 0.34, false),
        TaskState::Minimized => (Tone::Text, 0.08, false),
        TaskState::Attention => (Tone::Warn, 0.35, false),
    };
    Some(IndicatorSpec { tone, fill_alpha, line_alpha: if lines { 0.45 } else { 0.0 }, strong_alpha: if lines { 0.90 } else { 0.0 }, line_px: 3.0, corner_inset: 5.0, strong_side })
}

// ---------------------------------------------------------------------------------------------------------------------
// The pager (plasma-desktop applets/pager/qml/main.qml)

/// Spacing between pager cells (`pagerItemGrid.spacing`).
pub const PAGER_SPACING: f32 = 1.0;

/// The screen shape the pager's cells imitate when the window manager does not report the desktop size (Wayland's
/// foreign-toplevel protocol does not): 16:9. A reported size always replaces it, see [`pager_shape`].
pub const PAGER_SCREEN: (f32, f32) = (16.0, 9.0);

/// The shape the pager imitates: the real desktop size when the tracker knows it, else the [`PAGER_SCREEN`] assumption.
pub fn pager_shape(desktop: Option<(u32, u32)>) -> (f32, f32) {
    match desktop {
        Some((w, h)) if w > 0 && h > 0 => (w as f32, h as f32),
        _ => PAGER_SCREEN,
    }
}

/// Length the pager takes along a strip for `count` cells of a `shape`d screen whose drawn height (width on a side strip) is `across`.
pub fn pager_extent(count: usize, across: f32, vertical: bool, shape: (f32, f32)) -> f32 {
    let cells = pager_cells(count, shape.0, shape.1, 1.0e5, across, vertical);
    match (cells.first(), cells.last()) {
        (Some(a), Some(b)) if vertical => b.y + b.h - a.y,
        (Some(a), Some(b)) => b.x + b.w - a.x,
        _ => 0.0,
    }
}

/// The pager's aspect ratio for `cols` x `rows` cells of `cell_w` x `cell_h` (`aspectRatio` in main.qml).
pub fn pager_aspect(cell_w: f32, cell_h: f32, cols: f32, rows: f32) -> f32 {
    (cell_w * cols + (cols * PAGER_SPACING - PAGER_SPACING)) / (cell_h * rows + (rows * PAGER_SPACING - PAGER_SPACING))
}

/// One miniature desktop in the pager, in the applet's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagerCell {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Lays `count` workspace cells out in the applet's `length` x `thickness` box (a horizontal strip: `length` along x). The
/// cells keep the screen's aspect ratio (`screen_w` : `screen_h`); they are placed on one row on a horizontal strip and in one
/// column on a vertical one, `vertical` swapping the axes, and the whole grid is centred. The result never leaves the box.
pub fn pager_cells(count: usize, screen_w: f32, screen_h: f32, length: f32, thickness: f32, vertical: bool) -> Vec<PagerCell> {
    if count == 0 || length <= 0.0 || thickness <= 0.0 || screen_w <= 0.0 || screen_h <= 0.0 {
        return Vec::new();
    }
    let n = count as f32;
    let ratio = screen_w / screen_h;
    // Along a horizontal strip the cells' height is the strip's thickness and their widths add up; on a vertical strip the
    // cells' width is the thickness and their heights add up.
    let (cw, ch) = if !vertical {
        let h = thickness.min((length - (n - 1.0) * PAGER_SPACING) / (n * ratio));
        (h * ratio, h)
    } else {
        let w = thickness.min((length - (n - 1.0) * PAGER_SPACING) * ratio / n);
        (w, w / ratio)
    };
    let (cw, ch) = (cw.floor().max(1.0), ch.floor().max(1.0));
    let total = if !vertical { n * cw + (n - 1.0) * PAGER_SPACING } else { n * ch + (n - 1.0) * PAGER_SPACING };
    let start = (length - total) / 2.0;
    (0..count)
        .map(|i| {
            let k = i as f32 * (if !vertical { cw } else { ch } + PAGER_SPACING);
            if !vertical {
                PagerCell { x: (start + k).round(), y: ((thickness - ch) / 2.0).floor(), w: cw, h: ch }
            } else {
                PagerCell { x: ((thickness - cw) / 2.0).floor(), y: (start + k).round(), w: cw, h: ch }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn extra(v: Value) -> serde_json::Map<String, Value> {
        serde_json::from_value(json!({ "taskbar": v })).unwrap()
    }

    #[test]
    fn search_form_is_a_generic_choice_and_defaults_to_the_dock_setting() {
        assert_eq!(TaskbarStyle::from_extra(&serde_json::Map::new()).search, SearchForm::Auto);
        assert_eq!(TaskbarStyle::from_extra(&extra(json!({"search": "button"}))).search, SearchForm::Button);
        assert_eq!(TaskbarStyle::from_extra(&extra(json!({"search": "box"}))).search, SearchForm::Auto);
        assert_eq!(TaskbarStyle::from_extra(&extra(json!({"search": 3}))).search, SearchForm::Auto);
    }

    #[test]
    fn no_taskbar_section_changes_nothing() {
        assert!(TaskbarStyle::from_extra(&serde_json::Map::new()).is_default());
        assert!(TaskbarStyle::from_extra(&extra(json!({}))).is_default());
        // invalid values mean the renderer's own, never a half-applied style
        assert!(TaskbarStyle::from_extra(&extra(json!({"thickness": 5, "indicator": "glow", "desktop": 3, "float_gap": -1}))).is_default());
    }

    #[test]
    fn keys_parse_into_typed_values() {
        let s = TaskbarStyle::from_extra(&extra(
            json!({"thickness": 44, "icon_size": 28, "cell_width": 50, "gap": 0, "inset": 0, "float_gap": 8, "indicator": "frame", "desktop": "icon", "workspaces": "cells"}),
        ));
        assert_eq!((s.thickness, s.icon, s.cell_width, s.gap, s.inset, s.float_gap), (Some(44.0), Some(28.0), Some(50.0), Some(0.0), Some(0.0), 8.0));
        assert_eq!((s.indicator, s.desktop, s.workspaces), (Indicator::Frame, DesktopForm::Icon, WorkspaceForm::Cells));
        assert!(!s.is_default());
    }

    #[test]
    fn the_icon_follows_the_thickness_unless_one_is_given() {
        assert_eq!(TaskbarStyle::from_extra(&extra(json!({"thickness": 44}))).icon_px(), Some(28.0));
        assert_eq!(TaskbarStyle::from_extra(&extra(json!({"thickness": 44, "icon_size": 24}))).icon_px(), Some(24.0));
        assert_eq!(TaskbarStyle::from_extra(&extra(json!({}))).icon_px(), None);
    }

    #[test]
    fn panel_formulas_reproduce_the_plasma_numbers() {
        assert_eq!((inner_padding(44, SVG_MARGIN), inner_padding(30, SVG_MARGIN), inner_padding(24, SVG_MARGIN)), (8, 4, 1));
        assert_eq!((icon_size(44, SVG_MARGIN), icon_size(46, SVG_MARGIN)), (28, 30));
    }

    #[test]
    fn the_indicator_faces_the_desktop_on_every_edge() {
        for (panel, strong) in [(Edge::Bottom, Edge::Top), (Edge::Top, Edge::Bottom), (Edge::Left, Edge::Right), (Edge::Right, Edge::Left)] {
            let a = task_indicator(TaskState::Active, panel).unwrap();
            assert_eq!((a.strong_side, a.fill_alpha, a.line_alpha, a.strong_alpha, a.line_px), (strong, 0.45, 0.45, 0.90, 3.0));
        }
        assert!(task_indicator(TaskState::Normal, Edge::Bottom).is_none());
        let h = task_indicator(TaskState::Hover, Edge::Bottom).unwrap();
        assert_eq!((h.tone, h.fill_alpha, h.strong_alpha), (Tone::Accent, 0.34, 0.0));
        assert_eq!(task_indicator(TaskState::Minimized, Edge::Left).unwrap().fill_alpha, 0.08);
        assert_eq!((task_indicator(TaskState::Attention, Edge::Left).unwrap().tone, task_indicator(TaskState::Attention, Edge::Left).unwrap().fill_alpha), (Tone::Warn, 0.35));
    }

    #[test]
    fn the_pager_follows_the_real_desktop_shape_and_falls_back_to_16_9() {
        assert_eq!(pager_shape(None), PAGER_SCREEN);
        assert_eq!(pager_shape(Some((0, 1080))), PAGER_SCREEN);
        assert_eq!(pager_shape(Some((3440, 1440))), (3440.0, 1440.0));
        // an ultrawide desktop makes wider cells, a portrait one narrower, in the same strip height
        let wide = pager_extent(4, 28.0, false, pager_shape(Some((3440, 1440))));
        let normal = pager_extent(4, 28.0, false, pager_shape(None));
        let tall = pager_extent(4, 28.0, false, pager_shape(Some((1080, 1920))));
        assert!(wide > normal && normal > tall, "{wide} {normal} {tall}");
        // the side strip is the transpose: the same shape needs the matching length
        assert!(pager_extent(4, 28.0, true, pager_shape(Some((1080, 1920)))) > pager_extent(4, 28.0, true, pager_shape(None)));
    }

    #[test]
    fn pager_cells_keep_the_screen_aspect_and_fit_the_box() {
        let a = pager_aspect(192.0, 108.0, 4.0, 1.0);
        assert!((a - 771.0 / 108.0).abs() < 1e-4 && (30.0 * a).floor() == 214.0);
        // four 16:9 cells on a 28 px high horizontal box
        let cells = pager_cells(4, 1920.0, 1080.0, 400.0, 28.0, false);
        assert_eq!(cells.len(), 4);
        for c in &cells {
            assert!((c.w / c.h - 16.0 / 9.0).abs() < 0.12, "{c:?}");
            assert!(c.y >= 0.0 && c.y + c.h <= 28.0 && c.x >= 0.0 && c.x + c.w <= 400.0);
        }
        assert!(cells.windows(2).all(|p| p[1].x >= p[0].x + p[0].w), "no overlap");
        // the same on a side strip: one column, cells keep their aspect and stay inside
        let side = pager_cells(4, 1920.0, 1080.0, 400.0, 28.0, true);
        assert!(side.iter().all(|c| c.x >= 0.0 && c.x + c.w <= 28.0 && c.y >= 0.0 && c.y + c.h <= 400.0));
        assert!(side.windows(2).all(|p| p[1].y >= p[0].y + p[0].h));
        // a narrow box shrinks the cells instead of overflowing
        let tight = pager_cells(4, 1920.0, 1080.0, 90.0, 28.0, false);
        assert!(tight.last().unwrap().x + tight.last().unwrap().w <= 90.0);
        assert!(pager_cells(0, 1920.0, 1080.0, 90.0, 28.0, false).is_empty());
    }
}
