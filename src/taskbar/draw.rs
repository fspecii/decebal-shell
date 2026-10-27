//! Painting of the Windows-style taskbar into a [`Pixmap`]. All colours come from [`Colors`], which is
//! built from the shared theme ([`Colors::from_theme`]); [`Colors::dark`]/[`Colors::light`] are the
//! Windows 11 palettes. Nothing here reads files or the display.

use super::{Hit, Layout, Metrics, Rect, TaskButton};
use crate::controlcenter::{self, Snapshot};
use crate::dock_menu::truncate;
use crate::gfx::{blit_scaled, color, fill_rect, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::menubar::popup::draw_layout_icon;
use crate::menubar::symbols::SymbolCache;
use crate::menubar::Tray;
use crate::text::{Fonts, Weight};
use crate::theme::{Selection, Theme};
use tiny_skia::Pixmap;

type Rgb = (u8, u8, u8);

/// Palette of the taskbar. `ink` is text and icons, `dim` secondary text, `*_a` are white or black overlays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub light: bool,
    pub bg: Rgb,
    pub edge: (Rgb, f32),
    pub ink: Rgb,
    pub dim: Rgb,
    /// Overlay colour and alphas for hover, pressed and the focused task.
    pub overlay: Rgb,
    pub hover_a: f32,
    pub press_a: f32,
    pub active_a: f32,
    pub accent: Rgb,
    pub search_bg: (Rgb, f32),
    pub search_ink: Rgb,
    pub search_edge: (Rgb, f32),
}

impl Colors {
    /// The palette of a theme: its dock colour as the bar, its text colours as ink, its accent for the
    /// start logo and running lines, hover/press overlays in the text colour.
    pub fn from_theme(t: &Theme) -> Colors {
        let light = !t.dark;
        Colors {
            light,
            bg: t.dock.t(),
            edge: (t.text.t(), t.outline.clamp(0.05, 0.14)),
            ink: t.text.t(),
            dim: t.text_secondary.t(),
            overlay: t.text.t(),
            hover_a: if light { 0.05 } else { 0.07 },
            press_a: if light { 0.03 } else { 0.04 },
            active_a: if light { 0.07 } else { 0.10 },
            accent: t.accent.t(),
            search_bg: if light { (t.surface.t(), 0.75) } else { (t.text.t(), 0.07) },
            search_ink: t.text_secondary.t(),
            search_edge: (t.text.t(), 0.09),
        }
    }

    /// The Windows 11 theme in one mode (what `windowsTheme: dark|light` forces when the active theme is the other mode).
    fn windows11(dark: bool) -> Colors {
        let def = crate::theme::builtin::def("windows11").expect("built-in windows11 theme");
        Colors::from_theme(&def.resolve("windows11", dark, &Selection::default()))
    }

    /// Windows 11 dark: charcoal mica, light-blue accent.
    pub fn dark() -> Colors {
        Colors::windows11(true)
    }

    /// Windows 11 light: pale mica, deep blue accent.
    pub fn light() -> Colors {
        Colors::windows11(false)
    }

    /// The active theme when it is already in the wanted mode, else the Windows 11 palette of that mode.
    pub fn pick(light: bool) -> Colors {
        let t = crate::theme::th();
        if t.dark != light {
            Colors::from_theme(&t)
        } else {
            Colors::windows11(!light)
        }
    }
}

/// Owns the font and the icon caches so they survive between frames.
pub struct Painter {
    pub fonts: Option<Fonts>,
    pub icons: IconCache,
    pub symbols: SymbolCache,
}

impl Default for Painter {
    fn default() -> Self {
        Painter { fonts: Fonts::load(), icons: IconCache::default(), symbols: SymbolCache::default() }
    }
}

/// Everything one frame needs.
pub struct Frame<'a> {
    pub m: &'a Metrics,
    pub c: &'a Colors,
    pub layout: &'a Layout,
    pub buttons: &'a [TaskButton],
    pub hover: Option<Hit>,
    pub pressed: Option<Hit>,
    /// The item whose popup is open (kept highlighted).
    pub open: Option<Hit>,
    pub tray: &'a Snapshot,
    pub layout_name: Option<&'a str>,
    pub time: &'a str,
    pub date: &'a str,
    pub show_search_label: bool,
    /// Bar background opacity, 0..=1.
    pub alpha: f32,
    /// The strip hugs the top edge (its hairline goes underneath).
    pub top: bool,
    /// Task being dragged: (button index, pointer x).
    pub drag: Option<(usize, f32)>,
}

/// Symbol name for a status item in the current state.
pub fn tray_symbol(kind: Tray, snap: &Snapshot) -> &'static str {
    match kind {
        Tray::Bluetooth => match &snap.bluetooth {
            Some(b) if !b.powered => "bluetooth-off",
            Some(b) if b.connected > 0 => "bluetooth-connected",
            _ => "bluetooth",
        },
        Tray::Audio => snap.volume.as_ref().map(controlcenter::speaker_icon).unwrap_or("speaker-2"),
        Tray::Network => {
            let net = snap.net.clone().unwrap_or_default();
            if !net.wired_name.is_empty() {
                "ethernet"
            } else if net.wifi_enabled && !net.wifi_name.is_empty() {
                "wifi-3"
            } else {
                "wifi-off"
            }
        }
        _ => "switches",
    }
}

/// The visible plate of a button in slot `r`: (x, y, w, h, corner radius). Square and full height in the
/// classic look, a rounded inset tile otherwise.
fn plate(m: &Metrics, r: Rect) -> (f32, f32, f32, f32, f32) {
    if m.classic {
        (r.x, 0.0, r.w, m.bar_h, 0.0)
    } else {
        (r.x + 2.0 * m.scale, ((m.bar_h - m.btn_h) / 2.0).round(), r.w - 4.0 * m.scale, m.btn_h, m.corner)
    }
}

fn rgba(c: Rgb, a: f32) -> tiny_skia::Color {
    color(c.0, c.1, c.2, a)
}

impl Painter {
    /// Draws `text` vertically centred on `cy` (x-height centred, which looks right for mixed case).
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, pm: &mut Pixmap, s: &str, x: f32, cy: f32, px: f32, weight: Weight, c: tiny_skia::Color) -> f32 {
        let Some(f) = self.fonts.as_mut() else { return 0.0 };
        let top = cy + 0.36 * px - f.ascent(px, weight);
        f.draw(pm, s, x, top, px, weight, c)
    }

    pub fn measure(&self, s: &str, px: f32) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, Weight::Regular))
    }

    fn symbol(&mut self, pm: &mut Pixmap, name: &str, ink: Rgb, size: f32, cx: f32, cy: f32) {
        let px = (size * 2.0).ceil() as u32;
        if let Some(sym) = self.symbols.get(name, ink, px) {
            blit_scaled(pm, sym, (cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size, 1.0);
        }
    }

    /// Draws the whole strip into a fresh pixmap `layout.width` x `bar_h`.
    pub fn draw_strip(&mut self, f: &Frame) -> Option<Pixmap> {
        let (m, c, l) = (f.m, f.c, f.layout);
        let mut pm = Pixmap::new(l.width.ceil().max(1.0) as u32, m.bar_h as u32)?;
        let h = m.bar_h;
        fill_rect(&mut pm, 0.0, 0.0, l.width, h, rgba(c.bg, f.alpha));
        let ey = if f.top { h - 1.0 } else { 0.0 };
        fill_rect(&mut pm, 0.0, ey, l.width, 1.0, rgba(c.edge.0, c.edge.1));

        self.draw_start(&mut pm, f);
        if let Some(r) = l.search {
            self.draw_search(&mut pm, f, r);
        }
        self.draw_tasks(&mut pm, f);
        for (kind, r) in &l.tray {
            self.draw_tray_item(&mut pm, f, *kind, *r);
        }
        self.draw_clock(&mut pm, f);
        self.draw_desktop(&mut pm, f);
        if let Some(fo) = &self.fonts {
            fo.trim();
        }
        Some(pm)
    }

    fn state_fill(&self, pm: &mut Pixmap, f: &Frame, hit: Hit, r: Rect, base: f32) {
        let c = f.c;
        let pressed = f.pressed == Some(hit) && f.hover == Some(hit);
        let a = if pressed {
            c.press_a
        } else if f.open == Some(hit) {
            c.active_a
        } else if f.hover == Some(hit) {
            c.hover_a.max(base)
        } else {
            base
        };
        if a > 0.0 {
            let (x, y, w, h, rad) = plate(f.m, r);
            fill_rounded(pm, x, y, w, h, rad, rgba(c.overlay, a));
        }
    }

    /// Four squares, the way every Windows since 8 draws its Start button.
    fn draw_start(&mut self, pm: &mut Pixmap, f: &Frame) {
        let (m, c) = (f.m, f.c);
        let r = f.layout.start;
        self.state_fill(pm, f, Hit::Start, r, 0.0);
        let q = (7.0 * m.scale).round();
        let g = (2.0 * m.scale).round().max(1.0);
        let total = 2.0 * q + g;
        let (x0, y0) = ((r.cx() - total / 2.0).round(), (r.cy() - total / 2.0).round());
        let rad = if m.classic { 0.0 } else { 1.2 * m.scale };
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            fill_rounded(pm, x0 + dx * (q + g), y0 + dy * (q + g), q, q, rad, rgba(c.accent, 1.0));
        }
    }

    fn draw_search(&mut self, pm: &mut Pixmap, f: &Frame, r: Rect) {
        let (m, c) = (f.m, f.c);
        if !f.show_search_label {
            self.state_fill(pm, f, Hit::Search, r, 0.0);
            self.symbol(pm, "magnifyingglass", c.ink, 16.0 * m.scale, r.cx(), r.cy());
            return;
        }
        let hov = f.hover == Some(Hit::Search);
        let (bg, bga, ink) = if m.classic && !c.light {
            // Windows 10 on a dark taskbar: a pale box with dark text.
            ((236, 236, 236), if hov { 1.0 } else { 0.92 }, (60, 60, 60))
        } else {
            (c.search_bg.0, c.search_bg.1 + if hov { 0.04 } else { 0.0 }, c.search_ink)
        };
        let bh = if m.classic { m.bar_h - 2.0 * m.scale.round() * 5.0 + 2.0 } else { 32.0 * m.scale };
        let bh = bh.round();
        let by = ((m.bar_h - bh) / 2.0).round();
        let rad = if m.classic { 0.0 } else { 16.0 * m.scale };
        let (x, w) = (r.x + if m.classic { 0.0 } else { 2.0 * m.scale }, r.w - if m.classic { 0.0 } else { 4.0 * m.scale });
        fill_rounded(pm, x, by, w, bh, rad, rgba(bg, bga));
        if !(m.classic && !c.light) {
            stroke_rounded(pm, x + 0.5, by + 0.5, w - 1.0, bh - 1.0, (rad - 0.5).max(0.0), rgba(c.search_edge.0, c.search_edge.1), 1.0);
        }
        let ico = 15.0 * m.scale;
        let sym_ink = if m.classic && !c.light { ink } else { c.ink };
        let ix = x + 14.0 * m.scale + ico / 2.0;
        self.symbol(pm, "magnifyingglass", sym_ink, ico, ix, by + bh / 2.0);
        let px = 12.5 * m.scale;
        self.text(pm, "Search", ix + ico / 2.0 + 9.0 * m.scale, by + bh / 2.0, px, Weight::Regular, rgba(ink, 1.0));
    }

    fn draw_tasks(&mut self, pm: &mut Pixmap, f: &Frame) {
        let (m, c, l) = (f.m, f.c, f.layout);
        let (cl, cr) = l.clip;
        let cw = (cr - cl).max(0.0);
        if cw < 1.0 || f.buttons.is_empty() {
            return;
        }
        let Some(mut tp) = Pixmap::new(cw.ceil() as u32, m.bar_h as u32) else { return };
        for (i, b) in f.buttons.iter().enumerate() {
            let Some(mut r) = l.tasks.get(i).copied() else { break };
            if r.x + r.w < cl || r.x > cr || f.drag.is_some_and(|d| d.0 == i) {
                continue;
            }
            r.x -= cl;
            self.draw_task(&mut tp, f, i, b, r, false);
        }
        if let Some((i, px)) = f.drag {
            if let (Some(b), Some(r)) = (f.buttons.get(i), l.tasks.get(i)) {
                let r = Rect { x: px - r.w / 2.0 - cl, ..*r };
                self.draw_task(&mut tp, f, i, b, r, true);
            }
        }
        pm.draw_pixmap(cl.floor() as i32, 0, tp.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        // Fade the clipped edges of a scrolling row.
        if l.scroll_max > 0.0 {
            let edge = 10.0 * m.scale;
            for k in 0..edge as u32 {
                let a = 1.0 - k as f32 / edge;
                if l.tasks[0].x < cl - 0.5 {
                    fill_rect(pm, cl + k as f32, 0.0, 1.0, m.bar_h, rgba(c.bg, a * 0.8));
                }
                if l.tasks.last().is_some_and(|r| r.x + r.w > cr + 0.5) {
                    fill_rect(pm, cr - k as f32 - 1.0, 0.0, 1.0, m.bar_h, rgba(c.bg, a * 0.8));
                }
            }
        }
    }

    fn draw_task(&mut self, tp: &mut Pixmap, f: &Frame, i: usize, b: &TaskButton, r: Rect, dragging: bool) {
        let (m, c) = (f.m, f.c);
        let base = if b.active { c.active_a } else { 0.0 };
        let hit = Hit::Task(i);
        // The button's visible plate, inside its slot.
        let (px, py, pw, _, rad) = plate(m, r);
        let pressed = f.pressed == Some(hit) && f.hover == Some(hit);
        let a = if dragging {
            c.active_a + 0.05
        } else if pressed {
            c.press_a
        } else if f.hover == Some(hit) {
            c.hover_a.max(base)
        } else {
            base
        };
        if a > 0.0 {
            fill_rounded(tp, px, py, pw, m.btn_h, rad, rgba(c.overlay, a));
        }
        let labelled = !b.title.is_empty() || (f.layout.task_w > m.btn_w * 1.5 && !b.app_id.is_empty());
        let show_label = labelled && pw >= m.icon + 2.0 * m.pad + 30.0 * m.scale;
        let icon_x = if show_label { px + m.pad - 2.0 * m.scale } else { r.cx() - m.icon / 2.0 };
        let size = m.icon;
        let iy = ((m.bar_h - size) / 2.0 - if m.classic { 0.0 } else { 1.0 * m.scale }).round();
        let px_size = (size.ceil() as u32).max(1) * 2;
        let icon = self.icons.get(&b.app_id, px_size);
        blit_scaled(tp, icon, icon_x.round(), iy, size, size, if dragging { 0.85 } else { 1.0 });
        if show_label {
            let text = if b.title.trim().is_empty() { crate::wm::display_name(&b.app_id) } else { b.title.trim().to_string() };
            let tx = icon_x + size + 8.0 * m.scale;
            let maxw = (px + pw - tx - 8.0 * m.scale).max(10.0);
            let fpx = 12.0 * m.scale;
            let shown = {
                let fonts = self.fonts.as_ref();
                truncate(&text, maxw, &|s| fonts.map_or(s.chars().count() as f32 * fpx * 0.55, |fo| fo.measure(s, fpx, Weight::Regular)))
            };
            self.text(tp, &shown, tx, m.bar_h / 2.0, fpx, Weight::Regular, rgba(c.ink, 1.0));
        }
        // Running indicator.
        if b.running {
            let t = (3.0 * m.scale).round().max(2.0);
            if m.classic {
                let th = (2.0 * m.scale).round().max(2.0);
                let col = if b.active { c.accent } else { c.dim };
                fill_rect(tp, px, m.bar_h - th, pw, th, rgba(col, if b.active { 1.0 } else { 0.9 }));
            } else {
                let (w, col) = if b.active { (16.0 * m.scale, c.accent) } else { (6.0 * m.scale, c.dim) };
                let y = py + m.btn_h - t - 1.0 * m.scale;
                fill_rounded(tp, (r.cx() - w / 2.0).round(), y.round(), w.round(), t, t / 2.0, rgba(col, 1.0));
            }
            if b.windows > 1 {
                // A second plate peeking out on the right: "several windows".
                let sx = px + pw + if m.classic { -2.0 } else { 0.0 };
                fill_rect(tp, sx.round(), py + 6.0 * m.scale, 1.0, m.btn_h - 12.0 * m.scale, rgba(c.dim, 0.5));
            }
        }
    }

    fn draw_tray_item(&mut self, pm: &mut Pixmap, f: &Frame, kind: Tray, r: Rect) {
        let (m, c) = (f.m, f.c);
        let hit = Hit::Tray(kind);
        self.state_fill_tray(pm, f, hit, r);
        let (cx, cy) = (r.cx(), r.cy());
        match kind {
            Tray::Layout => {
                if let Some(name) = f.layout_name {
                    let (w, h) = (20.0 * m.scale, 13.0 * m.scale);
                    draw_layout_icon(pm, name, (cx - w / 2.0).round(), (cy - h / 2.0).round(), w, h, (c.ink.0, c.ink.1, c.ink.2, 0.95));
                }
            }
            Tray::Battery => draw_battery(pm, f, cx, cy),
            k => {
                let name = tray_symbol(k, f.tray);
                self.symbol(pm, name, c.ink, 16.0 * m.scale, cx, cy);
            }
        }
    }

    fn state_fill_tray(&self, pm: &mut Pixmap, f: &Frame, hit: Hit, r: Rect) {
        let (m, c) = (f.m, f.c);
        let a = if f.pressed == Some(hit) && f.hover == Some(hit) {
            c.press_a
        } else if f.open == Some(hit) {
            c.active_a
        } else if f.hover == Some(hit) {
            c.hover_a
        } else {
            return;
        };
        let (x, y, w, h, rad) = plate(m, r);
        fill_rounded(pm, x, y, w, h, rad, rgba(c.overlay, a));
    }

    fn draw_clock(&mut self, pm: &mut Pixmap, f: &Frame) {
        let (m, c) = (f.m, f.c);
        let r = f.layout.clock;
        self.state_fill_tray(pm, f, Hit::Clock, r);
        let px = 12.0 * m.scale;
        let right = r.x + r.w - m.clock_pad;
        let line_gap = 8.5 * m.scale;
        for (s, cy) in [(f.time, m.bar_h / 2.0 - line_gap), (f.date, m.bar_h / 2.0 + line_gap)] {
            let w = self.measure(s, px);
            self.text(pm, s, right - w, cy, px, Weight::Regular, rgba(c.ink, 1.0));
        }
    }

    fn draw_desktop(&mut self, pm: &mut Pixmap, f: &Frame) {
        let (m, c) = (f.m, f.c);
        let r = f.layout.desktop;
        if f.hover == Some(Hit::Desktop) {
            fill_rect(pm, r.x, 0.0, r.w, m.bar_h, rgba(c.overlay, c.hover_a * 1.6));
        }
        fill_rect(pm, r.x, 0.0, 1.0, m.bar_h, rgba(c.overlay, 0.22));
    }
}

/// Battery glyph: outline, nub and a level fill (green charging, red low).
fn draw_battery(pm: &mut Pixmap, f: &Frame, cx: f32, cy: f32) {
    let (m, c) = (f.m, f.c);
    let Some(b) = &f.tray.battery else { return };
    let (w, h) = ((20.0 * m.scale).round(), (10.0 * m.scale).round());
    let (x, y) = ((cx - w / 2.0 - 1.0).round(), (cy - h / 2.0).round());
    stroke_rounded(pm, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 2.5 * m.scale, rgba(c.ink, 0.8), 1.0);
    fill_rect(pm, x + w + 0.5, y + h * 0.3, 1.5 * m.scale, h * 0.4, rgba(c.ink, 0.8));
    let fill: Rgb = if b.charging {
        (76, 194, 100)
    } else if b.percent <= 15 {
        (255, 69, 58)
    } else {
        c.ink
    };
    let fw = ((w - 4.0) * b.percent.min(100) as f32 / 100.0).max(1.0);
    fill_rounded(pm, x + 2.0, y + 2.0, fw, h - 4.0, 1.0, rgba(fill, 0.95));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WinAlign, WinLabels, WinSearch};
    use crate::taskbar::{layout, Inputs};

    fn frame_pm(align: WinAlign, light: bool, labels: WinLabels, search: WinSearch, n: usize) -> Pixmap {
        let m = Metrics::new(1.0, align);
        let kinds = [Tray::Bluetooth, Tray::Network, Tray::Audio, Tray::Battery];
        let mut p = Painter::default();
        let l = layout(&m, &Inputs { width: 1280.0, align, labels, search, buttons: n, tray: &kinds, clock_text_w: p.measure("27/10/2026", 12.0), scroll: 0.0 });
        let buttons: Vec<TaskButton> = (0..n)
            .map(|i| TaskButton {
                app_id: format!("app{i}"),
                window: Some(i as u64),
                title: format!("Window number {i} with a long title"),
                pinned: true,
                running: i % 2 == 0,
                active: i == 2,
                windows: 1 + i % 3,
            })
            .collect();
        let c = if light { Colors::light() } else { Colors::dark() };
        let snap = Snapshot::default();
        let f = Frame {
            m: &m,
            c: &c,
            layout: &l,
            buttons: &buttons,
            hover: Some(Hit::Task(1)),
            pressed: None,
            open: None,
            tray: &snap,
            layout_name: None,
            time: "14:05",
            date: "27/10/2026",
            show_search_label: search == WinSearch::Box,
            alpha: 0.95,
            top: false,
            drag: Some((0, 300.0)),
        };
        p.draw_strip(&f).unwrap()
    }

    #[test]
    fn every_variant_paints_a_sized_strip_with_content() {
        for align in [WinAlign::Center, WinAlign::Left] {
            for light in [false, true] {
                for (labels, search) in [(WinLabels::Never, WinSearch::Box), (WinLabels::Always, WinSearch::Button), (WinLabels::Always, WinSearch::None)] {
                    for n in [0, 3, 40] {
                        let pm = frame_pm(align, light, labels, search, n);
                        assert_eq!((pm.width(), pm.height()), (1280, Metrics::new(1.0, align).bar_h as u32));
                        assert!(pm.pixel(5, 20).unwrap().alpha() > 200, "opaque-ish background");
                    }
                }
            }
        }
    }

    #[test]
    fn dark_and_light_differ_and_accent_paints_the_start_logo() {
        let d = frame_pm(WinAlign::Center, false, WinLabels::Never, WinSearch::Box, 3);
        let l = frame_pm(WinAlign::Center, true, WinLabels::Never, WinSearch::Box, 3);
        assert!(d.pixel(5, 20).unwrap().red() < l.pixel(5, 20).unwrap().red());
        let accent = Colors::dark().accent;
        assert!(d.data().chunks_exact(4).any(|p| p[2] > 200 && p[0].abs_diff(accent.0) < 12 && p[1].abs_diff(accent.1) < 12), "accent coloured squares exist");
    }

    #[test]
    fn colors_follow_the_theme_and_windows_palettes_are_windows11() {
        let def = crate::theme::builtin::def("macos").unwrap();
        let t = def.resolve("macos", false, &Selection::default());
        let c = Colors::from_theme(&t);
        assert!(c.light && c.bg == t.dock.t() && c.accent == t.accent.t() && c.ink == t.text.t());
        assert_eq!(Colors::dark().bg, (32, 32, 32));
        assert_eq!(Colors::light().accent, (0, 95, 184));
    }

    #[test]
    fn tray_symbols_follow_the_state() {
        let mut s = Snapshot::default();
        assert_eq!(tray_symbol(Tray::Network, &s), "wifi-off");
        s.volume = Some(controlcenter::sys::Volume { level: 0.5, muted: true });
        assert_eq!(tray_symbol(Tray::Audio, &s), "speaker-muted");
        s.volume = Some(controlcenter::sys::Volume { level: 0.9, muted: false });
        assert_eq!(tray_symbol(Tray::Audio, &s), "speaker-3");
    }

    #[test]
    fn palettes() {
        assert!(Colors::pick(true).light && !Colors::pick(false).light);
        assert!(Colors::dark().bg.0 < Colors::light().bg.0);
    }
}
