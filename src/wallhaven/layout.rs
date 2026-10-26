//! Thumbnail grid geometry and scroll maths (mirrors the original GridView).

use std::ops::Range;

/// The scrolling viewport of the grid, in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub cols: usize,
    pub cell_w: f32,
    pub cell_h: f32,
}

/// Height reserved after the last row for the "Loading more…" footer.
pub const FOOTER_MORE: f32 = 48.0;
pub const FOOTER_END: f32 = 16.0;
/// Pages to stay ahead of the visible area, in rows.
pub const PREFETCH_ROWS: f32 = 8.0;
/// Rows of thumbnails decoded beyond the viewport.
pub const BUFFER_ROWS: usize = 3;

impl Grid {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        let w = w.max(1.0);
        let cols = ((w / 300.0).floor() as usize).max(2);
        let cell_w = (w / cols as f32).floor().max(1.0);
        let cell_h = (cell_w * 2.0 / 3.0).floor().max(1.0);
        Self { x, y, w, h: h.max(0.0), cols, cell_w, cell_h }
    }

    pub fn rows(&self, n: usize) -> usize {
        n.div_ceil(self.cols)
    }

    pub fn content_height(&self, n: usize, has_more: bool) -> f32 {
        self.rows(n) as f32 * self.cell_h + if has_more { FOOTER_MORE } else { FOOTER_END }
    }

    pub fn max_scroll(&self, n: usize, has_more: bool) -> f32 {
        (self.content_height(n, has_more) - self.h).max(0.0)
    }

    pub fn clamp_scroll(&self, scroll: f32, n: usize, has_more: bool) -> f32 {
        scroll.clamp(0.0, self.max_scroll(n, has_more))
    }

    /// Rows still below the viewport.
    pub fn rows_left(&self, scroll: f32, n: usize, has_more: bool) -> f32 {
        (self.content_height(n, has_more) - scroll - self.h) / self.cell_h
    }

    /// Items whose cells touch the viewport plus `buffer` rows each side.
    pub fn visible(&self, scroll: f32, n: usize, buffer: usize) -> Range<usize> {
        if n == 0 {
            return 0..0;
        }
        let first_row = (scroll / self.cell_h).floor().max(0.0) as usize;
        let last_row = ((scroll + self.h) / self.cell_h).floor().max(0.0) as usize;
        let from = (first_row.saturating_sub(buffer) * self.cols).min(n);
        let to = ((last_row + buffer + 1) * self.cols).min(n);
        from..to.max(from)
    }

    /// Cell rectangle (x, y, w, h) of item `i` in window coordinates.
    pub fn cell_rect(&self, i: usize, scroll: f32) -> (f32, f32, f32, f32) {
        let (row, col) = (i / self.cols, i % self.cols);
        (self.x + col as f32 * self.cell_w, self.y + row as f32 * self.cell_h - scroll, self.cell_w, self.cell_h)
    }

    /// Item under window point (`px`, `py`), if any.
    pub fn hit(&self, px: f32, py: f32, scroll: f32, n: usize) -> Option<usize> {
        if px < self.x || px >= self.x + self.cols as f32 * self.cell_w || py < self.y || py >= self.y + self.h {
            return None;
        }
        let col = ((px - self.x) / self.cell_w) as usize;
        let row = ((py - self.y + scroll) / self.cell_h) as usize;
        let i = row * self.cols + col;
        (i < n).then_some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_width_like_original() {
        assert_eq!(Grid::new(0.0, 0.0, 1256.0, 600.0).cols, 4);
        assert_eq!(Grid::new(0.0, 0.0, 700.0, 600.0).cols, 2);
        assert_eq!(Grid::new(0.0, 0.0, 200.0, 600.0).cols, 2, "never fewer than two");
        let g = Grid::new(0.0, 0.0, 1256.0, 600.0);
        assert_eq!((g.cell_w, g.cell_h), (314.0, 209.0));
    }

    #[test]
    fn content_height_and_clamp() {
        let g = Grid::new(0.0, 0.0, 1256.0, 600.0);
        assert_eq!(g.rows(24), 6);
        assert_eq!(g.content_height(24, true), 6.0 * 209.0 + FOOTER_MORE);
        assert_eq!(g.clamp_scroll(-50.0, 24, true), 0.0);
        assert_eq!(g.clamp_scroll(1e9, 24, false), 6.0 * 209.0 + FOOTER_END - 600.0);
        let short = Grid::new(0.0, 0.0, 1256.0, 2000.0);
        assert_eq!(short.max_scroll(8, false), 0.0);
    }

    #[test]
    fn visible_range_includes_buffer_and_clamps() {
        let g = Grid::new(0.0, 0.0, 1256.0, 600.0);
        assert_eq!(g.visible(0.0, 100, 0), 0..12);
        assert_eq!(g.visible(0.0, 100, 2), 0..20);
        let r = g.visible(5.0 * 209.0, 100, 1);
        assert_eq!(r.start, 4 * 4);
        assert!(r.end <= 100);
        assert_eq!(g.visible(0.0, 0, 6), 0..0);
        let past = g.visible(1e6, 10, 2);
        assert!(past.is_empty(), "scrolled past the end yields an empty range");
    }

    #[test]
    fn hit_testing_accounts_for_scroll_and_edges() {
        let g = Grid::new(12.0, 100.0, 1256.0, 600.0);
        assert_eq!(g.hit(13.0, 101.0, 0.0, 10), Some(0));
        assert_eq!(g.hit(12.0 + 314.0, 101.0, 0.0, 10), Some(1));
        assert_eq!(g.hit(13.0, 101.0 + 209.0, 0.0, 10), Some(4));
        assert_eq!(g.hit(13.0, 101.0, 209.0, 10), Some(4), "scrolled one row");
        assert_eq!(g.hit(13.0, 101.0 + 209.0 * 3.0, 0.0, 10), None, "past the last item");
        assert_eq!(g.hit(5.0, 101.0, 0.0, 10), None, "left margin");
        assert_eq!(g.hit(13.0, 50.0, 0.0, 10), None, "above viewport");
    }

    #[test]
    fn rows_left_drives_prefetch() {
        let g = Grid::new(0.0, 0.0, 1256.0, 600.0);
        assert!(g.rows_left(0.0, 24, true) < PREFETCH_ROWS);
        assert!(g.rows_left(0.0, 200, true) > PREFETCH_ROWS);
    }

    #[test]
    fn cell_rect_moves_with_scroll() {
        let g = Grid::new(12.0, 100.0, 1256.0, 600.0);
        assert_eq!(g.cell_rect(5, 0.0), (12.0 + 314.0, 100.0 + 209.0, 314.0, 209.0));
        assert_eq!(g.cell_rect(5, 50.0).1, 100.0 + 209.0 - 50.0);
    }
}
