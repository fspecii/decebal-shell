//! Dock model, magnification layout and drawing. Pure logic plus CPU rendering, no windowing.

use crate::config::DockSettings;
use crate::gfx::{self, blit_scaled, fill_circle, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::theme::*;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

#[derive(Debug, Clone, PartialEq)]
pub struct DockItem {
    pub id: String,
    pub running: bool,
    /// In the pinned list (kept in the dock); false for an app that is only here because it runs.
    pub pinned: bool,
}

impl DockItem {
    pub fn pinned(id: &str, running: bool) -> Self {
        Self { id: id.to_string(), running, pinned: true }
    }

    /// Launchpad and Trash are fixed ends of the dock: they cannot be dragged or removed.
    pub fn is_builtin(&self) -> bool {
        matches!(self.id.as_str(), "launchpad" | "trash")
    }
}

/// An icon being dragged: its index in [`Dock::items`] and the pointer x (canvas coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub index: usize,
    pub x: f32,
}

/// One laid-out cell: left edge, width and magnification scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub x: f32,
    pub w: f32,
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub panel_x: f32,
    pub panel_w: f32,
    pub cells: Vec<Cell>,
}

pub struct Dock {
    pub items: Vec<DockItem>,
    pub settings: DockSettings,
    /// Pointer x in canvas coordinates while hovering the dock.
    pub hover_x: Option<f32>,
    /// Icon currently being dragged to a new place.
    pub drag: Option<Drag>,
    /// Items that were just launched and bounce until their window appears (or a timeout).
    bounce: Vec<(String, Instant)>,
    /// Smoothed per-item scales; empty until the first animation step.
    eased: Vec<f32>,
    icons: IconCache,
    reduce_motion: bool,
    high_contrast: bool,
    #[cfg(test)]
    dimensions_override: Option<(f32, f32)>,
}

/// How far (in px) magnification reaches from the pointer.
const REACH: f32 = 150.0;
/// Extra transparent room above the panel for magnified icons and the launch bounce.
const HEADROOM: f32 = 40.0;
const SIDE_MARGIN: f32 = 24.0;
/// The canvas is never narrower than this, so a context menu always fits above even a tiny dock.
const MIN_CANVAS_W: f32 = 300.0;
/// Height of one bounce, period of one bounce, and how long an app may bounce while it starts.
const BOUNCE_H: f32 = 16.0;
const BOUNCE_PERIOD: f32 = 0.7;
const BOUNCE_MAX: Duration = Duration::from_secs(8);

impl Dock {
    pub fn new(items: Vec<DockItem>, settings: DockSettings) -> Self {
        Self::with_preferences(items, settings, crate::accessibility::Preferences::from_env())
    }

    pub fn with_preferences(items: Vec<DockItem>, settings: DockSettings, preferences: crate::accessibility::Preferences) -> Self {
        Self {
            reduce_motion: preferences.reduce_motion,
            high_contrast: preferences.high_contrast,
            items,
            settings,
            hover_x: None,
            drag: None,
            bounce: Vec::new(),
            eased: Vec::new(),
            icons: IconCache::default(),
            #[cfg(test)]
            dimensions_override: None,
        }
    }

    fn dimensions(&self) -> (f32, f32) {
        #[cfg(test)]
        if let Some(dimensions) = self.dimensions_override {
            return dimensions;
        }
        let theme = crate::theme::th();
        (theme.dock_icon_size, theme.dock_padding)
    }

    fn icon_scale(&self) -> f32 {
        self.dimensions().0 / ICON_SIZE
    }
    fn base_cell_w(&self) -> f32 {
        CELL_W * self.icon_scale()
    }
    fn padding(&self) -> f32 {
        self.dimensions().1
    }
    /// Visible panel height, excluding magnification and bounce headroom.
    pub fn panel_height(&self) -> f32 {
        PANEL_HEIGHT * self.icon_scale()
    }

    /// Panel width with no magnification.
    pub fn rest_width(&self) -> f32 {
        let n = self.items.len() as f32;
        if n == 0.0 {
            return 2.0 * self.padding();
        }
        n * self.base_cell_w() + (n - 1.0) * self.settings.app_spacing + 2.0 * self.padding()
    }

    /// Canvas (window) size that always fits the fully magnified dock: along the edge x across it (swapped for a side
    /// dock, which runs down the screen).
    pub fn canvas_size(&self) -> (u32, u32) {
        let extra = self.items.len() as f32 * self.base_cell_w() * (self.settings.magnification - 1.0).max(0.0) * 0.6;
        let w = (self.rest_width() + extra + 2.0 * SIDE_MARGIN).max(MIN_CANVAS_W);
        let headroom = (HEADROOM * self.icon_scale()).max(self.dimensions().0 * (self.settings.magnification - 1.0).max(0.0) + BOUNCE_H * self.icon_scale() + 4.0);
        let h = self.panel_height() + headroom;
        if self.edge().is_vertical() {
            (h.ceil() as u32, w.ceil() as u32)
        } else {
            (w.ceil() as u32, h.ceil() as u32)
        }
    }

    /// The screen edge of the dock: the user's `edge`, else the theme's `[dock] position`, else the bottom.
    pub fn edge(&self) -> crate::panel::Edge {
        self.settings.edge.or(crate::theme::th().panel_theme.dock_edge).unwrap_or(crate::panel::Edge::Bottom)
    }

    /// (length along the edge, depth across it) of a canvas `cw` x `ch`.
    fn frame(&self, cw: f32, ch: f32) -> (f32, f32) {
        if self.edge().is_vertical() {
            (ch, cw)
        } else {
            (cw, ch)
        }
    }

    /// A canvas point in the dock's own frame: (along the edge, distance from the screen edge).
    pub fn to_frame(&self, cw: f32, ch: f32, x: f32, y: f32) -> (f32, f32) {
        use crate::panel::Edge;
        match self.edge() {
            Edge::Bottom => (x, ch - y),
            Edge::Top => (x, y),
            Edge::Left => (y, x),
            Edge::Right => (y, cw - x),
        }
    }

    /// A box of the dock's frame (`a`.. along, `c`.. from the screen edge, `la` x `lc`) on the canvas: (x, y, w, h).
    /// Icons are placed, never rotated.
    pub fn map(&self, cw: f32, ch: f32, a: f32, c: f32, la: f32, lc: f32) -> (f32, f32, f32, f32) {
        use crate::panel::Edge;
        match self.edge() {
            Edge::Bottom => (a, ch - c - lc, la, lc),
            Edge::Top => (a, c, la, lc),
            Edge::Left => (c, a, lc, la),
            Edge::Right => (cw - c - lc, a, lc, la),
        }
    }

    /// The visible panel on the canvas: (x, y, w, h).
    pub fn panel_rect(&self, cw: f32, ch: f32) -> (f32, f32, f32, f32) {
        let (len, _) = self.frame(cw, ch);
        let l = self.layout(len);
        self.map(cw, ch, l.panel_x, 0.0, l.panel_w, self.panel_height())
    }

    /// Starts the launch bounce for `id`.
    pub fn start_bounce(&mut self, id: &str) {
        if self.reduce_motion {
            return;
        }
        self.bounce.retain(|(b, _)| b != id);
        self.bounce.push((id.to_string(), Instant::now()));
    }

    /// Drops finished bounces (the app's window appeared, or it took too long). True while any remain.
    pub fn bouncing(&mut self) -> bool {
        let items = &self.items;
        self.bounce.retain(|(id, t)| t.elapsed() < BOUNCE_MAX && !items.iter().any(|i| &i.id == id && i.running));
        !self.bounce.is_empty()
    }

    fn bounce_offset(&self, id: &str) -> f32 {
        match self.bounce.iter().find(|(b, _)| b == id) {
            Some((_, t)) => BOUNCE_H * self.icon_scale() * (t.elapsed().as_secs_f32() * std::f32::consts::PI / BOUNCE_PERIOD).sin().abs(),
            None => 0.0,
        }
    }

    fn falloff(distance: f32) -> f32 {
        if distance >= REACH {
            0.0
        } else {
            0.5 * (1.0 + (std::f32::consts::PI * distance / REACH).cos())
        }
    }

    /// Scales the pointer asks for right now (1.0 everywhere without hover).
    fn target_scales(&self, rest_x: f32, n: usize) -> Vec<f32> {
        let base = self.base_cell_w();
        let sp = self.settings.app_spacing;
        let mag = if self.reduce_motion { 0.0 } else { (self.settings.magnification - 1.0).max(0.0) };
        (0..n)
            .map(|i| match self.hover_x {
                Some(hx) => {
                    let centre = rest_x + self.padding() - CONTENT_NUDGE * self.icon_scale() + i as f32 * (base + sp) + base / 2.0;
                    1.0 + mag * Self::falloff((hx - centre).abs())
                }
                None => 1.0,
            })
            .collect()
    }

    fn current_scales(&self, rest_x: f32, n: usize) -> Vec<f32> {
        if self.eased.len() == n {
            self.eased.clone()
        } else {
            self.target_scales(rest_x, n)
        }
    }

    /// Moves the displayed scales toward the pointer's target (about 110 ms to settle, like the
    /// original). Returns true while still animating so the caller keeps ticking.
    pub fn step(&mut self, canvas_w: f32) -> bool {
        if self.reduce_motion {
            self.bounce.clear();
            self.eased = vec![1.0; self.items.len()];
            return false;
        }
        let bouncing = self.bouncing();
        let n = self.items.len();
        let rest_x = (canvas_w - self.rest_width()) / 2.0;
        let target = self.target_scales(rest_x, n);
        if self.eased.len() != n {
            self.eased = vec![1.0; n];
        }
        let mut moving = false;
        for (e, t) in self.eased.iter_mut().zip(&target) {
            if (*t - *e).abs() < 0.002 {
                *e = *t;
            } else {
                *e += (*t - *e) * 0.35;
                moving = true;
            }
        }
        moving || bouncing
    }

    /// Computes cell positions for a canvas of width `canvas_w`.
    pub fn layout(&self, canvas_w: f32) -> Layout {
        let sp = self.settings.app_spacing;
        let base = self.base_cell_w();
        let n = self.items.len();
        let rest_w = self.rest_width();
        let rest_x = (canvas_w - rest_w) / 2.0;

        let scales = self.current_scales(rest_x, n);

        let total: f32 = scales.iter().map(|s| base * s).sum::<f32>() + sp * n.saturating_sub(1) as f32;
        let panel_w = total + 2.0 * self.padding();
        let panel_x = (canvas_w - panel_w) / 2.0;
        let mut x = panel_x + self.padding() - CONTENT_NUDGE * self.icon_scale();
        let cells = scales
            .iter()
            .map(|&scale| {
                let c = Cell { x, w: base * scale, scale };
                x += c.w + sp;
                c
            })
            .collect();
        Layout { panel_x, panel_w, cells }
    }

    /// Index of the item under canvas point (`x`, `y`), if any.
    pub fn hit_test(&self, canvas_w: f32, canvas_h: f32, x: f32, y: f32) -> Option<usize> {
        let (len, _) = self.frame(canvas_w, canvas_h);
        let l = self.layout(len);
        let (a, c) = self.to_frame(canvas_w, canvas_h, x, y);
        if c > self.panel_height() + 8.0 || c < 0.0 {
            return None;
        }
        l.cells.iter().position(|cell| a >= cell.x && a <= cell.x + cell.w)
    }

    /// Actual rounded material plate, excluding animation headroom and side margins.
    pub fn glass_region(&self, canvas_w: f32, canvas_h: f32) -> crate::glass::Region {
        let (x, y, w, h) = self.panel_rect(canvas_w, canvas_h);
        crate::glass::Region { x, y, w, h, radius: layout_radius(w, h, self.settings.roundness) }
    }

    /// Draws the whole dock into `pm`, which should be `canvas_size()` pixels.
    pub fn draw(&mut self, pm: &mut Pixmap) {
        gfx::clear(pm);
        let (cw, ch) = (pm.width() as f32, pm.height() as f32);
        // the dock is laid out along its edge (x of the bottom dock) and mapped onto the edge it sits on
        let (len, _) = self.frame(cw, ch);
        let layout = self.layout(len);
        let ph = self.panel_height();
        let (px0, py0, pw0, ph0) = self.map(cw, ch, layout.panel_x, 0.0, layout.panel_w, ph);

        let radius = layout_radius(pw0, ph0, self.settings.roundness);
        // Theme colours: panel colour, opacity (`dock_alpha` 0.6 = the original look at default settings) and outline.
        let th = crate::theme::th();
        let alpha = if self.high_contrast { 1.0 } else { ((1.0 - self.settings.transparency) * th.dock_alpha / 0.6).clamp(0.0, 1.0) };
        fill_rounded(pm, px0, py0, pw0, ph0, radius, th.dock.color(alpha));
        stroke_rounded(pm, px0 + 0.5, py0 + 0.5, pw0 - 1.0, ph0 - 1.0, radius - 0.5, th.dock_ink(th.outline * 0.35), 1.0);

        // distance from the screen edge of the icons' base line (the bottom dock's `icon_bottom`) and of the dots
        let icon_size = self.dimensions().0;
        let base = ph - PANEL_PAD_TOP * self.icon_scale() - icon_size;
        let dot = base - DOT_OFFSET;
        let dragged = self.drag.map(|d| d.index);
        let dot_at = |pm: &mut Pixmap, cell: &Cell| {
            let (x, y, _, _) = self.map(cw, ch, cell.x + cell.w / 2.0, dot, 0.0, 0.0);
            fill_circle(pm, x, y, DOT_RADIUS, crate::theme::th().dock_ink(0.85));
        };
        let mut icons = Vec::new();
        for (i, (item, cell)) in self.items.iter().zip(&layout.cells).enumerate() {
            if item.running {
                dot_at(pm, cell);
            }
            if dragged == Some(i) {
                // The slot stays empty; the icon follows the pointer (drawn last).
                continue;
            }
            let size = icon_size * cell.scale;
            let lift = self.bounce_offset(&item.id);
            let ix = cell.x + (cell.w - size) / 2.0;
            icons.push((item.id.clone(), self.map(cw, ch, ix, base + lift, size, size), 1.0));
        }
        if let Some(d) = self.drag {
            if let Some(item) = self.items.get(d.index) {
                let size = icon_size * 1.1;
                icons.push((item.id.clone(), self.map(cw, ch, d.x - size / 2.0, base + 8.0, size, size), 0.9));
            }
        }
        for (id, (x, y, w, h), alpha) in icons {
            let px = (w.ceil() as u32).max(1) * 2; // rasterise at 2x then scale down for crisp edges
            let icon = self.icons.get_for_surface(&id, px, th.dock_foreground().t());
            blit_scaled(pm, icon, x, y, w, h, alpha);
        }
    }
}

/// Corner radius the original uses: half the shorter side times `roundness`.
pub fn layout_radius(w: f32, h: f32, roundness: f32) -> f32 {
    w.min(h) / 2.0 * roundness
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_motion_has_static_layout_and_never_starts_bounce() {
        let mut d = Dock::with_preferences(
            vec![DockItem::pinned("one", false), DockItem::pinned("two", false)],
            DockSettings { magnification: 2.0, ..DockSettings::default() },
            crate::accessibility::Preferences { reduce_motion: true, ..Default::default() },
        );
        d.dimensions_override = Some((48.0, 12.0));
        let (w, _) = d.canvas_size();
        let rest = d.layout(w as f32);
        d.hover_x = Some(rest.cells[0].x + rest.cells[0].w / 2.0);
        d.start_bounce("one");
        assert!(!d.bouncing());
        assert!(!d.step(w as f32));
        assert_eq!(d.layout(w as f32), rest);
        assert_eq!(d.bounce_offset("one"), 0.0);
        assert!(rest.cells.iter().all(|cell| cell.scale == 1.0));
    }

    fn dock(n: usize) -> Dock {
        let items = (0..n).map(|i| DockItem::pinned(&format!("app{i}"), i == 0)).collect();
        Dock::new(items, DockSettings::default())
    }

    #[test]
    fn forty_two_pixel_theme_changes_plate_cells_hit_testing_and_render_canvas() {
        let mut dock = dock(5);
        dock.dimensions_override = Some((42.0, 8.0));
        let (w, h) = dock.canvas_size();
        let layout = dock.layout(w as f32);
        let plate = dock.glass_region(w as f32, h as f32);
        assert!((layout.cells[0].w - 60.0 * 42.0 / 47.0).abs() < 0.01);
        assert!((plate.h - 71.0 * 42.0 / 47.0).abs() < 0.01);
        assert!((plate.w - (5.0 * 60.0 * 42.0 / 47.0 + 4.0 * 6.0 + 16.0)).abs() < 0.01);
        assert!(plate.h < PANEL_HEIGHT && dock.rest_width() < 356.0);
        let cell = layout.cells[2];
        assert_eq!(dock.hit_test(w as f32, h as f32, cell.x + cell.w / 2.0, h as f32 - 20.0), Some(2));
        assert_eq!(dock.hit_test(w as f32, h as f32, cell.x + cell.w / 2.0, 0.0), None);
        let mut pm = Pixmap::new(w, h).unwrap();
        dock.draw(&mut pm);
        let p = ((h as usize - 2) * w as usize + (plate.x + plate.w / 2.0) as usize) * 4;
        assert!(pm.data()[p + 3] > 0);
    }

    #[test]
    fn material_plate_excludes_animation_canvas_and_tracks_magnification() {
        let mut dock = Dock::new(vec![DockItem::pinned("one", false), DockItem::pinned("two", false)], DockSettings::default());
        let (w, h) = dock.canvas_size();
        for hover in [None, Some(w as f32 / 2.0)] {
            dock.hover_x = hover;
            let region = dock.glass_region(w as f32, h as f32);
            let layout = dock.layout(w as f32);
            assert!(region.y > 0.0 && region.x > 0.0);
            assert_eq!((region.x, region.w), (layout.panel_x, layout.panel_w));
            assert_eq!(region.y + region.h, h as f32);
            assert_eq!(region.h, PANEL_HEIGHT);
        }
    }

    #[test]
    fn rest_width_matches_measured_original() {
        // 5 cells of 60 + 4 gaps of 6 + 2 x 16 padding = 356 (original measured ~355).
        assert!((dock(5).rest_width() - 356.0).abs() < 1.0);
    }

    #[test]
    fn no_hover_means_no_magnification() {
        let l = dock(5).layout(500.0);
        assert!(l.cells.iter().all(|c| c.scale == 1.0));
    }

    #[test]
    fn magnification_peaks_under_pointer_and_decays() {
        let mut d = dock(5);
        let rest = d.layout(500.0);
        let mid = rest.cells[2].x + rest.cells[2].w / 2.0;
        d.hover_x = Some(mid);
        let l = d.layout(500.0);
        assert!((l.cells[2].scale - 1.4).abs() < 0.01);
        assert!(l.cells[1].scale < l.cells[2].scale && l.cells[1].scale > 1.0);
        assert!((l.cells[1].scale - l.cells[3].scale).abs() < 0.05);
    }

    #[test]
    fn panel_stays_centred() {
        let mut d = dock(5);
        d.hover_x = Some(200.0);
        let l = d.layout(500.0);
        assert!((l.panel_x + l.panel_w / 2.0 - 250.0).abs() < 0.01);
    }

    #[test]
    fn canvas_fits_magnified_dock() {
        let mut d = dock(5);
        let (w, _) = d.canvas_size();
        d.hover_x = Some(w as f32 / 2.0);
        let l = d.layout(w as f32);
        assert!(l.panel_x >= 0.0 && l.panel_x + l.panel_w <= w as f32);
    }

    #[test]
    fn hit_test_finds_cell() {
        let d = dock(5);
        let (w, h) = d.canvas_size();
        let l = d.layout(w as f32);
        let c = l.cells[3];
        assert_eq!(d.hit_test(w as f32, h as f32, c.x + 5.0, h as f32 - 20.0), Some(3));
        assert_eq!(d.hit_test(w as f32, h as f32, c.x + 5.0, 2.0), None);
    }

    #[test]
    fn easing_converges_to_target_and_back() {
        let mut d = dock(5);
        let w = d.canvas_size().0 as f32;
        d.hover_x = Some(w / 2.0);
        let mut steps = 0;
        while d.step(w) {
            steps += 1;
            assert!(steps < 100, "easing never settled");
        }
        let l = d.layout(w);
        assert!((l.cells[2].scale - 1.4).abs() < 0.01);
        d.hover_x = None;
        while d.step(w) {}
        assert!(d.layout(w).cells.iter().all(|c| c.scale == 1.0));
    }

    #[test]
    fn empty_dock_is_safe() {
        let mut d = dock(0);
        let (w, h) = d.canvas_size();
        let mut pm = Pixmap::new(w, h).unwrap();
        d.draw(&mut pm);
    }

    #[test]
    fn canvas_has_room_for_a_menu_even_with_two_icons() {
        assert!(dock(2).canvas_size().0 >= 300);
    }

    #[test]
    fn bounce_runs_until_the_app_is_running() {
        let mut d = dock(3);
        d.start_bounce("app1");
        assert!(d.bouncing());
        assert!(d.step(400.0), "keeps the animation ticking");
        d.items[1].running = true;
        assert!(!d.bouncing());
        assert_eq!(d.bounce_offset("app1"), 0.0);
        d.start_bounce("app2");
        assert!(d.bounce_offset("app2") >= 0.0);
        d.start_bounce("app2");
        assert_eq!(d.bounce.len(), 1, "restarting does not stack");
    }

    #[test]
    fn dragged_icon_draws() {
        let mut d = dock(4);
        let (w, h) = d.canvas_size();
        d.drag = Some(Drag { index: 1, x: 150.0 });
        let mut pm = Pixmap::new(w, h).unwrap();
        d.draw(&mut pm);
    }

    #[test]
    fn builtin_items_are_fixed() {
        assert!(DockItem::pinned("trash", false).is_builtin() && DockItem::pinned("launchpad", false).is_builtin());
        assert!(!DockItem::pinned("foot", false).is_builtin());
    }

    #[test]
    fn radius_formula() {
        assert!((layout_radius(355.0, 71.0, 0.45) - 15.975).abs() < 0.01);
    }
}
