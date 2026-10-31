//! Clipped, scrollable native Control Center viewport. Canonical targets stay in
//! content coordinates; only a cached display list is translated for painting.
use crate::controlcenter::{self, settings::Style, Item, Target, R};
use crate::menubar::symbols::SymbolCache;
use crate::text::Fonts;
use tiny_skia::{Mask, Pixmap};

pub(super) const SCROLL_STEP: f32 = 40.0;

#[derive(Default)]
pub(super) struct Viewport {
    pub scroll: f32,
    pub focus: Option<Target>,
    buffer: Option<Pixmap>,
    mask: Option<Mask>,
    source: Vec<Item>,
    translated: Vec<Item>,
    stamp: (f32, u32),
}

/// A target can occur on its tile, leading icon and slider fill. Keyboard
/// traversal visits it once and uses the first (outer) target rectangle.
pub(super) fn targets(items: &[Item]) -> Vec<(Target, R)> {
    let mut result = Vec::new();
    for item in items {
        if let Some(target) = item.target {
            if !result.iter().any(|(seen, _)| *seen == target) {
                result.push((target, item.r));
            }
        }
    }
    result
}

pub(super) fn height(content: f32, origin_y: f32, surface_height: f32) -> f32 {
    content.min((surface_height - origin_y - 8.0).max(1.0))
}

impl Viewport {
    pub fn clamp(&mut self, content: f32, height: f32) {
        self.scroll = self.scroll.clamp(0.0, (content - height).max(0.0));
    }
    pub fn reveal(&mut self, target: R, content: f32, height: f32) {
        if target.y < self.scroll {
            self.scroll = target.y;
        } else if target.y + target.h > self.scroll + height {
            self.scroll = (target.y + target.h - height).min(target.y);
        }
        self.clamp(content, height);
    }
    pub fn point(&self, x: f32, y: f32, origin: (f32, f32), width: f32, height: f32) -> Option<(f32, f32)> {
        (x >= origin.0 && x < origin.0 + width && y >= origin.1 && y < origin.1 + height).then_some((x - origin.0, y - origin.1 + self.scroll))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, pm: &mut Pixmap, origin: (f32, f32), width: f32, height: f32, items: &[Item], hover: Option<Target>, fonts: &mut Fonts, symbols: &mut SymbolCache) {
        let (width, height) = (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32);
        if self.buffer.as_ref().is_none_or(|buffer| buffer.width() != width || buffer.height() != height) {
            self.buffer = Pixmap::new(width, height);
            self.mask = Mask::new(width, height).and_then(|mut mask| {
                let path = crate::gfx::rounded_rect(0.0, 0.0, width as f32, height as f32, 28.0)?;
                mask.fill_path(&path, tiny_skia::FillRule::Winding, true, crate::gfx::transform());
                Some(mask)
            });
        }
        if self.source != items || self.stamp != (self.scroll, height) {
            self.source.clear();
            self.source.extend_from_slice(items);
            self.translated.clear();
            let scale = crate::theme::live::font_scale().max(1.0);
            self.translated.extend(items.iter().filter(|item| item.r.y + item.r.h * scale >= self.scroll && item.r.y <= self.scroll + height as f32).cloned().map(|mut item| {
                item.r.y -= self.scroll;
                item
            }));
            self.stamp = (self.scroll, height);
        }
        let Some(buffer) = self.buffer.as_mut() else { return };
        crate::gfx::clear(buffer);
        controlcenter::draw(buffer, &controlcenter::View { origin: (0.0, 0.0), items: &self.translated, height: height as f32, hover, style: Style::Dux }, fonts, symbols);
        if let Some((_, focus)) = targets(items).into_iter().find(|(target, _)| Some(*target) == self.focus) {
            crate::gfx::stroke_rounded(buffer, focus.x + 1.0, focus.y - self.scroll + 1.0, focus.w - 2.0, focus.h - 2.0, 12.0, crate::theme::th().accent_c(), 2.0);
        }
        if !items.is_empty() && height as f32 > 32.0 {
            let content = items.iter().map(|item| item.r.y + item.r.h).fold(0.0f32, f32::max) + 16.0;
            if content > height as f32 {
                let track = height as f32 - 32.0;
                let thumb = (track * height as f32 / content).max(24.0);
                let y = 16.0 + (track - thumb) * self.scroll / (content - height as f32);
                crate::gfx::fill_rounded(buffer, width as f32 - 7.0, y, 3.0, thumb, 1.5, crate::theme::th().text_secondary.color(0.5));
            }
        }
        if let Some(mask) = &self.mask {
            buffer.apply_mask(mask);
        }
        crate::gfx::blit_scaled(pm, buffer, origin.0, origin.1, width as f32, height as f32, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_tile_and_icon_targets_are_one_keyboard_stop() {
        let make = |target, y| Item { r: R { x: 16.0, y, w: 250.0, h: 64.0 }, draw: controlcenter::Draw::Tile, target: Some(target) };
        let items = [make(Target::DarkMode, 16.0), make(Target::DarkMode, 24.0), make(Target::Settings, 800.0)];
        let stops = targets(&items);
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].1.y, 16.0);
        let mut view = Viewport { focus: Some(Target::Settings), ..Viewport::default() };
        view.reveal(stops[1].1, 900.0, 600.0);
        assert_eq!(view.focus, Some(Target::Settings));
        assert!(view.scroll > 0.0);
        assert!(view.point(20.0, 20.0, (0.0, 40.0), 600.0, 600.0).is_none());
    }

    #[test]
    fn clip_scroll_and_reveal_share_one_canonical_transform() {
        let mut view = Viewport::default();
        let visible = height(900.0, 50.0, 720.0);
        assert_eq!(visible, 662.0);
        assert!(view.point(610.0, 713.0, (10.0, 50.0), 600.0, visible).is_none());
        let target = R { x: 16.0, y: 820.0, w: 250.0, h: 64.0 };
        view.reveal(target, 900.0, visible);
        assert_eq!(view.scroll, 222.0);
        assert_eq!(view.point(30.0, 650.0, (10.0, 50.0), 600.0, visible), Some((20.0, 822.0)));
        view.clamp(300.0, visible);
        assert_eq!(view.scroll, 0.0);
    }
}
