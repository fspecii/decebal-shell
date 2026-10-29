//! The theme package gallery of the Theme pane: one card per installed theme (screenshot or a miniature desktop in its colours,
//! name, version, author, description) with Apply, Preview, Duplicate, Export and Delete.
//!
//! The data comes from [`crate::theme::engine::store`]; the clicks are handled in `themepane` (`l:tcard:<action>:<id>`).

use super::ui::{pal, Frame, Interact, Kind as CtlKind, Painter, Placed, ThemeTile};
use crate::gfx::{fill_rounded, stroke_rounded};
use crate::settings::ui::Ctl;
use crate::text::Weight;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tiny_skia::Pixmap;

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: String,
    pub name: String,
    /// `1.0.0 · Ada · MIT · user`
    pub meta: String,
    pub description: String,
    pub tags: String,
    pub swatches: [(u8, u8, u8); 7],
    pub screenshot: Option<PathBuf>,
    pub current: bool,
    pub deletable: bool,
    pub error: Option<String>,
    /// The Delete button was pressed once and waits for a second click.
    pub confirm_delete: bool,
    pub hash_mismatch: bool,
}

type ShotCache = HashMap<PathBuf, Option<Pixmap>>;
static SHOTS: Mutex<Option<ShotCache>> = Mutex::new(None);

/// A screenshot decoded once, at card size (the files are validated images of bounded size, but a card only needs a thumbnail).
fn shot(path: &PathBuf) -> Option<Pixmap> {
    let mut g = SHOTS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = g.get_or_insert_with(HashMap::new);
    cache
        .entry(path.clone())
        .or_insert_with(|| {
            let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?.thumbnail(420, 280).to_rgba8();
            let mut pm = Pixmap::new(img.width(), img.height())?;
            for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(img.pixels()) {
                let a = u32::from(s[3]);
                d.copy_from_slice(&[(u32::from(s[0]) * a / 255) as u8, (u32::from(s[1]) * a / 255) as u8, (u32::from(s[2]) * a / 255) as u8, s[3]]);
            }
            Some(pm)
        })
        .clone()
}

/// Draws the cards one under the other; returns the y below them.
pub fn draw(pm: &mut Pixmap, p: &mut Painter, cards: &[Card], x0: f32, y: f32, cw: f32, it: &Interact, fr: &mut Frame) -> f32 {
    let s = p.s;
    let u = |v: f32| v * s;
    let origin_x = x0;
    let origin_y = y;
    let columns = if cw >= u(600.0) { 2 } else { 1 };
    let cw = (cw - u(16.0) * (columns - 1) as f32) / columns as f32;
    let preview_h = if cw >= u(300.0) { 160.0 } else { 120.0 };
    let row_height = u(preview_h + 192.0);
    for (index, c) in cards.iter().enumerate() {
        let x0 = origin_x + (index % columns) as f32 * (cw + u(16.0));
        let y = origin_y + (index / columns) as f32 * row_height;
        let mut buttons: Vec<(&str, String, bool)> = Vec::new();
        if c.error.is_none() {
            if !c.current {
                buttons.push(("apply", "Apply".into(), false));
                buttons.push(("preview", "Preview".into(), false));
            }
            buttons.push(("dup", "Duplicate".into(), false));
            buttons.push(("export", "Export".into(), false));
        }
        if c.deletable && !c.current {
            buttons.push(("delete", if c.confirm_delete { "Really delete?".into() } else { "Delete".into() }, true));
        }
        let ch = row_height - u(16.0);
        // Cards far outside the window are only laid out (their buttons stay reachable by Tab), not painted: a long list
        // re-shaped and re-rasterised every card on every wheel notch.
        let on = y + ch >= 0.0 && y <= pm.height() as f32;
        if on {
            fill_rounded(pm, x0, y, cw, ch, u(12.0), pal::card());
        }
        if c.current && on {
            stroke_rounded(pm, x0 + 1.0, y + 1.0, cw - 2.0, ch - 2.0, u(12.0), pal::accent(), u(2.0));
        }
        if on {
            // preview: the package's screenshot, else a miniature desktop painted in the theme's colours
            let (px, py, pw, ph) = (x0 + u(10.0), y + u(10.0), cw - u(20.0), u(preview_h));
            match c.screenshot.as_ref().and_then(shot) {
                Some(img) => super::ui::draw_cover(pm, &img, px, py, pw, ph, u(8.0)),
                None => super::ui::draw_theme_preview(pm, (px, py, pw, ph), s, &ThemeTile { id: c.id.clone(), name: c.name.clone(), selected: false, error: c.error.is_some(), colors: c.swatches }),
            }
            let tx = x0 + u(12.0);
            let text_y = y + u(preview_h + 8.0);
            let tw = x0 + cw - tx - u(12.0);
            let name = p.fit(&c.name, tw - if c.current { u(70.0) } else { 0.0 }, u(15.0), Weight::SemiBold);
            let nw = p.text(pm, &name, tx, text_y + u(24.0), u(15.0), Weight::SemiBold, if c.error.is_some() { pal::red() } else { pal::text() });
            if c.current {
                p.text(pm, "In use", tx + nw + u(10.0), text_y + u(24.0), u(11.0), Weight::Medium, pal::accent());
            }
            let meta = p.fit(&c.meta, tw, u(11.0), Weight::Regular);
            p.text(pm, &meta, tx, text_y + u(42.0), u(11.0), Weight::Regular, pal::faint());
            let desc = match &c.error {
                Some(e) => format!("Cannot be used: {e}"),
                None if c.hash_mismatch => "Warning: the files do not match the package's content hash".to_string(),
                None => c.description.clone(),
            };
            let d = p.fit(&desc, tw, u(12.0), Weight::Regular);
            p.text(pm, &d, tx, text_y + u(60.0), u(12.0), Weight::Regular, if c.error.is_some() || c.hash_mismatch { pal::red() } else { pal::sub() });
            if !c.tags.is_empty() {
                let t = p.fit(&c.tags, tw, u(10.5), Weight::Regular);
                p.text(pm, &t, tx, text_y + u(76.0), u(10.5), Weight::Regular, pal::faint());
            }
        }

        // buttons, right to left from the card's right edge
        let mut bx = x0 + cw - u(12.0);
        let mut by = y + ch - u(32.0);
        let bh = u(24.0);
        for (act, label, danger) in buttons.iter() {
            let w = p.measure(label, u(11.5), Weight::Medium) + u(22.0);
            if bx - w < x0 + u(12.0) {
                bx = x0 + cw - u(12.0);
                by -= u(32.0);
            }
            bx -= w;
            let id = format!("l:tcard:{act}:{}", c.id);
            let rect = (bx, by, w, bh);
            let hover = it.hover.as_deref() == Some(id.as_str());
            let primary = *act == "apply";
            let bg = if primary {
                crate::theme::th().accent.color(if hover { 1.0 } else { 0.9 })
            } else if hover {
                crate::theme::th().ink(0.16)
            } else {
                crate::theme::th().ink(0.08)
            };
            if on {
                fill_rounded(pm, bx, by, w, bh, bh / 2.0, bg);
                let fg = if primary {
                    crate::theme::th().accent_text_c()
                } else if *danger {
                    pal::red()
                } else {
                    pal::text()
                };
                p.text_center(pm, label, bx + w / 2.0, by + bh / 2.0, u(11.5), Weight::Medium, fg);
                if it.focus.as_deref() == Some(id.as_str()) && it.ring {
                    stroke_rounded(pm, bx - u(2.0), by - u(2.0), w + u(4.0), bh + u(4.0), bh / 2.0 + u(2.0), crate::theme::th().accent.color(0.85), u(2.0));
                }
            }
            fr.layouts.push((rect, id.clone()));
            fr.placed.push(Placed {
                rect,
                ctl: Ctl::new(id, CtlKind::Button { label: label.clone(), danger: *danger }),
                parts: vec![],
                row_rect: (x0, y, cw, ch),
                label: format!("{label} {}", c.name),
            });
            bx -= u(8.0);
        }
    }
    origin_y + cards.len().div_ceil(columns) as f32 * row_height + u(8.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_theme_actions_wrap_inside_the_card_at_multiple_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let mut pm = Pixmap::new((480.0 * scale) as u32, (800.0 * scale) as u32).unwrap();
            let mut painter = Painter { fonts: None, s: scale };
            let card = Card {
                id: "example".into(),
                name: "Example theme".into(),
                meta: "1.0 · Example · MIT".into(),
                description: "A complete desktop design".into(),
                tags: "#example".into(),
                swatches: [(128, 128, 128); 7],
                screenshot: None,
                current: false,
                deletable: true,
                error: None,
                confirm_delete: true,
                hash_mismatch: false,
            };
            let mut wide = Frame::default();
            let mut second = card.clone();
            second.id = "second".into();
            draw(&mut pm, &mut painter, &[card.clone(), second], 0.0, 0.0, 640.0 * scale, &Interact::default(), &mut wide);
            assert_eq!(wide.placed.len(), 10);
            assert_eq!(wide.placed[0].row_rect.1, wide.placed[5].row_rect.1);
            assert!(wide.placed[0].row_rect.0 + wide.placed[0].row_rect.2 < wide.placed[5].row_rect.0);
            let mut frame = Frame::default();
            draw(&mut pm, &mut painter, &[card], 180.0 * scale, 10.0 * scale, 284.0 * scale, &Interact::default(), &mut frame);
            assert_eq!(frame.placed.len(), 5);
            for (i, button) in frame.placed.iter().enumerate() {
                let r = button.rect;
                assert!(r.0 >= 192.0 * scale && r.0 + r.2 <= 452.0 * scale);
                assert!(r.1 >= 210.0 * scale, "button overlaps theme text");
                assert!(frame.at(r.0 + r.2 / 2.0, r.1 + r.3 / 2.0).is_some());
                for other in &frame.placed[..i] {
                    let q = other.rect;
                    assert!(r.0 + r.2 <= q.0 || q.0 + q.2 <= r.0 || r.1 + r.3 <= q.1 || q.1 + q.3 <= r.1);
                }
            }
        }
    }
}
