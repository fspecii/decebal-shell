//! Dock model, magnification layout and drawing. Pure logic plus CPU rendering, no windowing.

use crate::config::DockSettings;
use crate::gfx::{self, blit_scaled, color, fill_circle, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::theme::*;
use tiny_skia::Pixmap;

#[derive(Debug, Clone, PartialEq)]
pub struct DockItem {
    pub id: String,
    pub running: bool,
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
    icons: IconCache,
}

/// How far (in px) magnification reaches from the pointer.
const REACH: f32 = 150.0;
/// Extra transparent room above the panel for magnified icons and the launch bounce.
const HEADROOM: f32 = 40.0;
const SIDE_MARGIN: f32 = 24.0;

impl Dock {
    pub fn new(items: Vec<DockItem>, settings: DockSettings) -> Self {
        Self {
            items,
            settings,
            hover_x: None,
            icons: IconCache::default(),
        }
    }

    fn base_cell_w() -> f32 {
        CELL_W
    }

    /// Panel width with no magnification.
    pub fn rest_width(&self) -> f32 {
        let n = self.items.len() as f32;
        if n == 0.0 {
            return 2.0 * PANEL_PAD_X;
        }
        n * Self::base_cell_w() + (n - 1.0) * self.settings.app_spacing + 2.0 * PANEL_PAD_X
    }

    /// Canvas (window) size that always fits the fully magnified dock.
    pub fn canvas_size(&self) -> (u32, u32) {
        let extra = self.items.len() as f32
            * Self::base_cell_w()
            * (self.settings.magnification - 1.0).max(0.0)
            * 0.6;
        let w = self.rest_width() + extra + 2.0 * SIDE_MARGIN;
        let h = PANEL_HEIGHT + HEADROOM;
        (w.ceil() as u32, h.ceil() as u32)
    }

    fn falloff(distance: f32) -> f32 {
        if distance >= REACH {
            0.0
        } else {
            0.5 * (1.0 + (std::f32::consts::PI * distance / REACH).cos())
        }
    }

    /// Computes cell positions for a canvas of width `canvas_w`.
    pub fn layout(&self, canvas_w: f32) -> Layout {
        let sp = self.settings.app_spacing;
        let base = Self::base_cell_w();
        let n = self.items.len();
        let rest_w = self.rest_width();
        let rest_x = (canvas_w - rest_w) / 2.0;
        let mag = (self.settings.magnification - 1.0).max(0.0);

        let scales: Vec<f32> = (0..n)
            .map(|i| match self.hover_x {
                Some(hx) => {
                    let centre =
                        rest_x + PANEL_PAD_X - CONTENT_NUDGE + i as f32 * (base + sp) + base / 2.0;
                    1.0 + mag * Self::falloff((hx - centre).abs())
                }
                None => 1.0,
            })
            .collect();

        let total: f32 =
            scales.iter().map(|s| base * s).sum::<f32>() + sp * n.saturating_sub(1) as f32;
        let panel_w = total + 2.0 * PANEL_PAD_X;
        let panel_x = (canvas_w - panel_w) / 2.0;
        let mut x = panel_x + PANEL_PAD_X - CONTENT_NUDGE;
        let cells = scales
            .iter()
            .map(|&scale| {
                let c = Cell {
                    x,
                    w: base * scale,
                    scale,
                };
                x += c.w + sp;
                c
            })
            .collect();
        Layout {
            panel_x,
            panel_w,
            cells,
        }
    }

    /// Index of the item under canvas point (`x`, `y`), if any.
    pub fn hit_test(&self, canvas_w: f32, canvas_h: f32, x: f32, y: f32) -> Option<usize> {
        let l = self.layout(canvas_w);
        let top = canvas_h - PANEL_HEIGHT - 8.0;
        if y < top || y > canvas_h {
            return None;
        }
        l.cells.iter().position(|c| x >= c.x && x <= c.x + c.w)
    }

    /// Draws the whole dock into `pm`, which should be `canvas_size()` pixels.
    pub fn draw(&mut self, pm: &mut Pixmap) {
        gfx::clear(pm);
        let cw = pm.width() as f32;
        let ch = pm.height() as f32;
        let layout = self.layout(cw);
        let panel_y = ch - PANEL_HEIGHT;

        let radius = layout_radius(layout.panel_w, PANEL_HEIGHT, self.settings.roundness);
        let (r, g, b) = DOCK_BG;
        fill_rounded(
            pm,
            layout.panel_x,
            panel_y,
            layout.panel_w,
            PANEL_HEIGHT,
            radius,
            color(r, g, b, 1.0 - self.settings.transparency),
        );
        stroke_rounded(
            pm,
            layout.panel_x + 0.5,
            panel_y + 0.5,
            layout.panel_w - 1.0,
            PANEL_HEIGHT - 1.0,
            radius - 0.5,
            color(255, 255, 255, DOCK_BORDER_ALPHA),
            1.0,
        );

        let icon_bottom = panel_y + PANEL_PAD_TOP + ICON_SIZE;
        for (item, cell) in self.items.iter().zip(&layout.cells) {
            let size = ICON_SIZE * cell.scale;
            let px = (size.ceil() as u32).max(1) * 2; // rasterise at 2x then scale down for crisp edges
            let icon = self.icons.get(&item.id, px);
            let ix = cell.x + (cell.w - size) / 2.0;
            blit_scaled(pm, icon, ix, icon_bottom - size, size, size, 1.0);
            if item.running {
                fill_circle(
                    pm,
                    cell.x + cell.w / 2.0,
                    icon_bottom + DOT_OFFSET,
                    DOT_RADIUS,
                    color(255, 255, 255, 0.85),
                );
            }
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

    fn dock(n: usize) -> Dock {
        let items = (0..n)
            .map(|i| DockItem {
                id: format!("app{i}"),
                running: i == 0,
            })
            .collect();
        Dock::new(items, DockSettings::default())
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
        assert_eq!(
            d.hit_test(w as f32, h as f32, c.x + 5.0, h as f32 - 20.0),
            Some(3)
        );
        assert_eq!(d.hit_test(w as f32, h as f32, c.x + 5.0, 2.0), None);
    }

    #[test]
    fn empty_dock_is_safe() {
        let mut d = dock(0);
        let (w, h) = d.canvas_size();
        let mut pm = Pixmap::new(w, h).unwrap();
        d.draw(&mut pm);
    }

    #[test]
    fn radius_formula() {
        assert!((layout_radius(355.0, 71.0, 0.45) - 15.975).abs() < 0.01);
    }
}
