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

pub const CARD_H: f32 = 112.0;
const SHOT_W: f32 = 168.0;

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
    let mut y = y;
    for c in cards {
        let ch = u(CARD_H);
        fill_rounded(pm, x0, y, cw, ch, u(12.0), pal::card());
        if c.current {
            stroke_rounded(pm, x0 + 1.0, y + 1.0, cw - 2.0, ch - 2.0, u(12.0), pal::accent(), u(2.0));
        }
        // preview: the package's screenshot, else a miniature desktop painted in the theme's colours
        let (px, py, pw, ph) = (x0 + u(10.0), y + u(10.0), u(SHOT_W), ch - u(20.0));
        match c.screenshot.as_ref().and_then(shot) {
            Some(img) => super::ui::draw_cover(pm, &img, px, py, pw, ph, u(8.0)),
            None => super::ui::draw_theme_preview(pm, (px, py, pw, ph), s, &ThemeTile { id: c.id.clone(), name: c.name.clone(), selected: false, error: c.error.is_some(), colors: c.swatches }),
        }
        let tx = px + pw + u(14.0);
        let tw = x0 + cw - tx - u(12.0);
        let name = p.fit(&c.name, tw - if c.current { u(70.0) } else { 0.0 }, u(15.0), Weight::SemiBold);
        let nw = p.text(pm, &name, tx, y + u(24.0), u(15.0), Weight::SemiBold, if c.error.is_some() { pal::red() } else { pal::text() });
        if c.current {
            p.text(pm, "In use", tx + nw + u(10.0), y + u(24.0), u(11.0), Weight::Medium, pal::accent());
        }
        let meta = p.fit(&c.meta, tw, u(11.0), Weight::Regular);
        p.text(pm, &meta, tx, y + u(42.0), u(11.0), Weight::Regular, pal::faint());
        let desc = match &c.error {
            Some(e) => format!("Cannot be used: {e}"),
            None if c.hash_mismatch => "Warning: the files do not match the package's content hash".to_string(),
            None => c.description.clone(),
        };
        let d = p.fit(&desc, tw, u(12.0), Weight::Regular);
        p.text(pm, &d, tx, y + u(60.0), u(12.0), Weight::Regular, if c.error.is_some() || c.hash_mismatch { pal::red() } else { pal::sub() });
        if !c.tags.is_empty() {
            let t = p.fit(&c.tags, tw, u(10.5), Weight::Regular);
            p.text(pm, &t, tx, y + u(76.0), u(10.5), Weight::Regular, pal::faint());
        }

        // buttons, right to left from the card's right edge
        let mut bx = x0 + cw - u(12.0);
        let by = y + ch - u(32.0);
        let bh = u(24.0);
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
        for (act, label, danger) in buttons.iter() {
            let w = p.measure(label, u(11.5), Weight::Medium) + u(22.0);
            bx -= w;
            let id = format!("l:tcard:{act}:{}", c.id);
            let rect = (bx, by, w, bh);
            let hover = it.hover.as_deref() == Some(id.as_str());
            let primary = *act == "apply";
            let bg = if primary { crate::theme::th().accent.color(if hover { 1.0 } else { 0.9 }) } else if hover { crate::theme::th().ink(0.16) } else { crate::theme::th().ink(0.08) };
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
            fr.layouts.push((rect, id.clone()));
            fr.placed.push(Placed { rect, ctl: Ctl::new(id, CtlKind::Button { label: label.clone(), danger: *danger }), parts: vec![], row_rect: (x0, y, cw, ch), label: format!("{label} {}", c.name) });
            bx -= u(8.0);
        }
        y += ch + u(10.0);
    }
    y + u(8.0)
}
