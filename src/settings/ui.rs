//! A tiny retained-description / immediate-draw widget layer for the settings window.
//!
//! Panes describe themselves as [`Item`]s (cards of [`Row`]s holding [`Ctl`]s). [`draw_items`]
//! lays them out, paints them with tiny-skia + fontdue and records a [`Frame`] of hit rectangles
//! that the event handlers use until the next paint. Controls have stable string ids so keyboard
//! focus and in-progress edits survive a rebuild (which happens whenever the file changes).

use crate::gfx::{color, fill_circle, fill_rounded, stroke_rounded};
use crate::text::{Fonts, Role, Weight};
use tiny_skia::{Color, Pixmap};

pub type Rect = (f32, f32, f32, f32);

pub fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

/// Axis-aligned rectangle snapped to whole pixels (at least 1x1). tiny-skia's anti-aliased path for
/// thin fractional rectangles asserts, so carets and separators go through here.
pub fn fill_px(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    let (x0, y0) = (x.round(), y.round());
    let (w, h) = ((x + w).round().max(x0 + 1.0) - x0, (y + h).round().max(y0 + 1.0) - y0);
    crate::gfx::fill_rect(pm, x0, y0, w, h, c);
}

// ---------------------------------------------------------------- theme

/// Palette: every colour comes from the shared theme ([`crate::theme`]), read at paint time so a
/// theme change shows on the next frame.
pub mod pal {
    use super::*;
    fn th() -> std::sync::Arc<crate::theme::Theme> {
        crate::theme::th()
    }
    pub fn window() -> Color {
        th().background.color(1.0)
    }
    pub fn sidebar() -> Color {
        th().sidebar.color(1.0)
    }
    pub fn card() -> Color {
        th().surface.color(1.0)
    }
    pub fn text() -> Color {
        th().text_c()
    }
    pub fn sub() -> Color {
        th().sub()
    }
    pub fn faint() -> Color {
        th().faint()
    }
    pub fn sep() -> Color {
        th().ink(0.10)
    }
    pub fn field() -> Color {
        th().ink(0.09)
    }
    pub fn field_hover() -> Color {
        th().ink(0.15)
    }
    pub fn accent() -> Color {
        th().accent_c()
    }
    pub fn accent_text() -> Color {
        th().accent_text_c()
    }
    pub fn green() -> Color {
        th().ok.color(1.0)
    }
    pub fn red() -> Color {
        th().danger.color(1.0)
    }
    pub fn orange() -> Color {
        th().warn.color(1.0)
    }
    /// Opaque tonal accent: the theme accent blended over `base` (selected rows, chips, segments).
    pub fn tonal(base: crate::theme::Rgb, a: f32) -> Color {
        th().accent.over(base, a).color(1.0)
    }
    /// Opaque text-ink blended over `base` (hover fills that glyphs must punch holes in).
    pub fn ink_over(base: crate::theme::Rgb, a: f32) -> Color {
        th().text.over(base, a).color(1.0)
    }
}

/// The one keyboard focus outline: a 2px accent ring 2px outside `r`.
pub fn focus_ring(pm: &mut Pixmap, r: Rect, radius: f32, s: f32) {
    stroke_rounded(pm, r.0 - 2.0 * s, r.1 - 2.0 * s, r.2 + 4.0 * s, r.3 + 4.0 * s, radius + 2.0 * s, crate::theme::th().accent.color(0.85), 2.0 * s);
}

// ---------------------------------------------------------------- description

/// A choice: (label, value).
pub type Opt = (String, String);

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Toggle(bool),
    Slider {
        v: f64,
        min: f64,
        max: f64,
        step: f64,
        unit: String,
    },
    Stepper {
        v: f64,
        min: f64,
        max: f64,
        step: f64,
        unit: String,
    },
    Seg(Vec<Opt>, Option<usize>),
    Popup(Vec<Opt>, Option<usize>),
    /// Hex colour text (`#rrggbb`), may be empty (unset).
    Color(String),
    Text {
        val: String,
        hint: String,
    },
    /// Shortcut display / recorder.
    Key(String),
    Button {
        label: String,
        danger: bool,
    },
    /// A button that stays highlighted while `on` (modifier toggles of the shortcut recorder).
    Chip {
        label: String,
        on: bool,
    },
    Label(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ctl {
    pub id: String,
    pub kind: Kind,
    pub enabled: bool,
}

impl Ctl {
    pub fn new(id: impl Into<String>, kind: Kind) -> Ctl {
        Ctl { id: id.into(), kind, enabled: true }
    }
    pub fn focusable(&self) -> bool {
        self.enabled && !matches!(self.kind, Kind::Label(_))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub label: String,
    pub detail: String,
    /// Controls, drawn right to left in this order's reverse (first = leftmost).
    pub ctls: Vec<Ctl>,
    /// Optional leading colour chip (a small rounded square).
    pub chip: Option<Color4>,
    /// Row is highlighted (selected list entry).
    pub selected: bool,
}

pub type Color4 = (u8, u8, u8);

impl Row {
    pub fn new(label: impl Into<String>) -> Row {
        Row { label: label.into(), ..Row::default() }
    }
    pub fn detail(mut self, d: impl Into<String>) -> Row {
        self.detail = d.into();
        self
    }
    pub fn ctl(mut self, c: Ctl) -> Row {
        self.ctls.push(c);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Small caption above a card.
    Header(String),
    /// Page navigation drawn as a flat strip (`secondary`: chips, otherwise underlined tabs). Same control as a segmented
    /// choice (`Kind::Seg`, one part per option) but it wraps onto more lines instead of truncating labels.
    Nav {
        id: String,
        opts: Vec<Opt>,
        sel: Option<usize>,
        secondary: bool,
    },
    Card(Vec<Row>),
    /// Wrapped explanatory text under a card (or an inline error when `warn`).
    Note {
        text: String,
        warn: bool,
    },
    /// Thumbnail grid: (id, caption, selected). Pixmaps come from the app's thumbnail cache by id.
    Thumbs(Vec<Thumb>),
    /// Layout pictogram grid.
    Layouts(Vec<LayoutTile>),
    /// Theme gallery: miniature desktops painted in each theme's own colours.
    Themes(Vec<ThemeTile>),
    /// Row of round colour swatches (accent picker).
    Swatches(Vec<Swatch>),
    /// A strip of labelled colour chips, not clickable (the "Match wallpaper" palette preview).
    Palette(Vec<Chip>),
    /// Large preview of the current wallpaper (id of a thumbnail) with caption.
    Preview {
        id: String,
        caption: String,
    },
    /// Font preview: one line per role, drawn in that role's font.
    FontSamples(Vec<FontSample>),
    /// Icon theme list (src/settings/iconspane.rs).
    IconThemes(Vec<super::iconspane::ThemeRow>),
    /// Theme package cards with Apply / Preview / Duplicate / Export / Delete (src/settings/themecards.rs).
    ThemeCards(Vec<super::themecards::Card>),
    /// Preview grid of sample icons of one theme (src/settings/iconspane.rs).
    IconGrid(super::iconspane::Grid),
    Space(f32),
    /// A widget definition (TOML) drawn by the desktop widget painter with sample data, and a caption (see `widgetbuilder.rs`).
    WidgetPreview {
        toml: String,
        caption: String,
    },
    /// The multi-line text editor of the Configuration pane (see `editor.rs`).
    Editor(super::editor::View),
}

/// A line of the font preview card.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSample {
    pub label: String,
    pub role: Role,
    pub text: String,
    pub px: f32,
    pub weight: Weight,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Thumb {
    pub id: String,
    pub caption: String,
    pub selected: bool,
}

/// One gallery entry. `id` is the control id (`l:theme:<theme id>`); colours are the resolved variant.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeTile {
    pub id: String,
    pub name: String,
    pub selected: bool,
    pub error: bool,
    /// background, bar, surface, titlebar, text, accent, dock
    pub colors: [Color4; 7],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub name: String,
    pub color: Color4,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Swatch {
    pub id: String,
    pub name: String,
    pub color: Color4,
    pub selected: bool,
    /// The "no override" swatch: drawn with a diagonal stroke.
    pub unset: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutTile {
    pub id: String,
    pub name: String,
    pub symbol: String,
    pub selected: bool,
}

/// The page header: title and an optional one-line context under it.
#[derive(Debug, Clone, Copy)]
pub struct Head<'a> {
    pub title: &'a str,
    pub context: &'a str,
}

impl<'a> Head<'a> {
    pub fn new(title: &'a str) -> Self {
        Head { title, context: "" }
    }
}

// ---------------------------------------------------------------- text helper

pub struct Painter<'a> {
    pub fonts: Option<&'a mut Fonts>,
    pub s: f32,
}

impl Painter<'_> {
    pub fn u(&self, v: f32) -> f32 {
        v * self.s
    }

    pub fn measure(&self, t: &str, px: f32, w: Weight) -> f32 {
        match &self.fonts {
            Some(f) => f.measure(t, px, w),
            None => t.chars().count() as f32 * px * 0.55,
        }
    }

    /// Text with its visual vertical centre at `cy`.
    pub fn text(&mut self, pm: &mut Pixmap, t: &str, x: f32, cy: f32, px: f32, w: Weight, c: Color) -> f32 {
        self.text_as(Role::Ui, pm, t, x, cy, px, w, c)
    }

    /// [`Self::text`] in the font of `role`.
    #[allow(clippy::too_many_arguments)]
    pub fn text_as(&mut self, role: Role, pm: &mut Pixmap, t: &str, x: f32, cy: f32, px: f32, w: Weight, c: Color) -> f32 {
        match &mut self.fonts {
            Some(f) => {
                let top = cy + 0.36 * px * crate::theme::live::font_scale() - f.ascent_as(role, px, w);
                f.draw_as(role, pm, t, x, top, px, w, c)
            }
            None => 0.0,
        }
    }

    pub fn measure_as(&self, role: Role, t: &str, px: f32, w: Weight) -> f32 {
        match &self.fonts {
            Some(f) => f.measure_as(role, t, px, w),
            None => t.chars().count() as f32 * px * 0.55,
        }
    }

    pub fn text_right(&mut self, pm: &mut Pixmap, t: &str, right: f32, cy: f32, px: f32, w: Weight, c: Color) {
        let m = self.measure(t, px, w);
        self.text(pm, t, right - m, cy, px, w, c);
    }

    pub fn text_center(&mut self, pm: &mut Pixmap, t: &str, cx: f32, cy: f32, px: f32, w: Weight, c: Color) {
        let m = self.measure(t, px, w);
        self.text(pm, t, cx - m / 2.0, cy, px, w, c);
    }

    /// Shortens `t` with an ellipsis to fit `max` pixels.
    pub fn fit(&self, t: &str, max: f32, px: f32, w: Weight) -> String {
        if self.measure(t, px, w) <= max {
            return t.to_string();
        }
        let mut out: Vec<char> = t.chars().collect();
        while !out.is_empty() {
            out.pop();
            let cand: String = out.iter().collect::<String>() + "…";
            if self.measure(&cand, px, w) <= max {
                return cand;
            }
        }
        String::new()
    }

    /// Greedy word wrap.
    pub fn wrap(&self, t: &str, max: f32, px: f32, w: Weight) -> Vec<String> {
        let mut lines = Vec::new();
        for para in t.split('\n') {
            let mut cur = String::new();
            for word in para.split_whitespace() {
                let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
                if !cur.is_empty() && self.measure(&cand, px, w) > max {
                    lines.push(std::mem::take(&mut cur));
                    cur = word.to_string();
                } else {
                    cur = cand;
                }
            }
            lines.push(cur);
        }
        lines
    }
}

// ---------------------------------------------------------------- interaction state used while drawing

#[derive(Debug, Clone, Default)]
pub struct Interact {
    pub hover: Option<String>,
    /// Which part of the hovered control the pointer is over (navigation strips highlight one option).
    pub hover_part: Option<usize>,
    pub focus: Option<String>,
    /// Slider being dragged: id and the value shown meanwhile.
    pub drag: Option<(String, f64)>,
    /// Text control being edited: id, text, cursor (chars).
    pub edit: Option<(String, String, usize)>,
    /// Selected range (chars) of the text being edited, if any.
    pub edit_sel: Option<(usize, usize)>,
    /// Key recorder: id and the modifier chips toggled so far.
    pub recording: Option<(String, Vec<&'static str>)>,
    /// Show the keyboard focus ring (only after keyboard use).
    pub ring: bool,
}

// ---------------------------------------------------------------- frame (hit map)

#[derive(Debug, Clone)]
pub struct Placed {
    pub rect: Rect,
    pub ctl: Ctl,
    /// Sub-rects of the control: segment/stepper button rectangles.
    pub parts: Vec<Rect>,
    pub row_rect: Rect,
    pub label: String,
}

#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// Gallery images actually painted in this frame (native debug evidence).
    pub theme_previews_painted: usize,
    pub placed: Vec<Placed>,
    pub thumbs: Vec<(Rect, String)>,
    pub layouts: Vec<(Rect, String)>,
    /// Total height of the laid-out content.
    pub height: f32,
    /// Rows hit rectangles (for right-click reset): row rect and first control id.
    pub rows: Vec<(Rect, String)>,
    /// Text editors on screen: control rect and what the painter measured.
    pub editors: Vec<(Rect, super::editor::Geo)>,
    /// Per drawn item, in order: its area and the range of `placed` entries it produced (the accessibility tree).
    pub spans: Vec<(Rect, std::ops::Range<usize>)>,
    /// The page title's area.
    pub head: Rect,
}

impl Frame {
    pub fn find(&self, id: &str) -> Option<&Placed> {
        self.placed.iter().find(|p| p.ctl.id == id)
    }
    pub fn at(&self, x: f32, y: f32) -> Option<&Placed> {
        self.placed.iter().rev().find(|p| p.ctl.enabled && inside(p.rect, x, y))
    }
    pub fn focus_order(&self) -> Vec<String> {
        self.placed.iter().filter(|p| p.ctl.focusable()).map(|p| p.ctl.id.clone()).collect()
    }
}

// ---------------------------------------------------------------- metrics

pub const ROW_H: f32 = 48.0;
pub const ROW_H_DETAIL: f32 = 60.0;
pub const CARD_R: f32 = 10.0;
pub const CARD_PAD: f32 = 14.0;
pub const TH_W: f32 = 132.0;
pub const TH_H: f32 = 84.0;

pub fn row_h(r: &Row) -> f32 {
    if r.detail.is_empty() {
        ROW_H
    } else {
        ROW_H_DETAIL
    }
}

/// Geometry scales independently of glyph sizes: Fonts already applies the live font scale.
#[derive(Clone, Copy)]
struct TextLayout(f32);

impl TextLayout {
    fn live() -> Self {
        Self(crate::theme::live::font_scale().max(1.0))
    }
    fn extra(self, px: f32) -> f32 {
        px * (self.0 - 1.0)
    }
    fn field(self) -> f32 {
        26.0 + self.extra(20.0)
    }
    fn row(self, r: &Row) -> f32 {
        row_h(r) + self.extra(if r.detail.is_empty() { 20.0 } else { 36.0 })
    }
    fn label_offsets(self, detail: bool) -> (f32, f32) {
        if detail {
            (-4.0 - self.extra(8.0), 12.0 + self.extra(8.0))
        } else {
            (0.0, 0.0)
        }
    }
}

/// Width in px a control wants (before scaling by `s`, measured with the painter).
fn ctl_width(p: &Painter, c: &Ctl) -> f32 {
    let u = |v| p.u(v);
    match &c.kind {
        Kind::Toggle(_) => u(40.0),
        Kind::Slider { .. } => u(230.0),
        Kind::Stepper { v, step, unit, .. } => {
            u(120.0).max(p.measure(&format!("{}{}", format_num(*v, *step), unit), u(13.0), Weight::Regular) + 2.0 * u(26.0).max(p.measure("−", u(15.0), Weight::Medium) + u(8.0)) + u(14.0))
        }
        Kind::Seg(o, _) => o.iter().map(|(l, _)| p.measure(l, u(12.0), Weight::Regular) + u(22.0)).sum::<f32>() + u(4.0),
        Kind::Popup(o, _) => o.iter().map(|(l, _)| p.measure(l, u(13.0), Weight::Regular)).fold(0.0, f32::max).max(u(60.0)) + u(40.0),
        Kind::Color(_) => u(96.0),
        Kind::Text { .. } => u(210.0),
        Kind::Key(k) => (p.measure(k, u(12.0), Weight::Medium) + u(28.0)).max(u(96.0)),
        Kind::Button { label, .. } | Kind::Chip { label, .. } => p.measure(label, u(12.5), Weight::Regular) + u(26.0),
        Kind::Label(t) => p.measure(t, u(13.0), Weight::Regular),
    }
}

pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() < 6 || !h.is_ascii() {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((v(0)?, v(2)?, v(4)?))
}

pub fn format_num(v: f64, step: f64) -> String {
    if step >= 1.0 || (v - v.round()).abs() < 1e-9 && step.fract() == 0.0 {
        format!("{}", v.round() as i64)
    } else if step >= 0.1 {
        format!("{v:.1}")
    } else if step >= 0.01 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

// ---------------------------------------------------------------- drawing controls

/// Draws one control inside `r` (its slot, vertically centred) and returns its [`Placed`] entry.
#[allow(clippy::too_many_arguments)]
fn draw_ctl(pm: &mut Pixmap, p: &mut Painter, c: &Ctl, right: f32, cy: f32, it: &Interact, row_rect: Rect, max_width: f32) -> Placed {
    let s = p.s;
    let u = |v: f32| v * s;
    let w = ctl_width(p, c).min(max_width).max(u(24.0));
    let x = right - w;
    let hover = it.hover.as_deref() == Some(c.id.as_str());
    let focus = it.focus.as_deref() == Some(c.id.as_str());
    let dim = if c.enabled { 1.0 } else { 0.4 };
    let mut parts = Vec::new();
    let h_field = u(TextLayout::live().field());
    let mut rect: Rect = (x, cy - h_field / 2.0, w, h_field);
    let a = |col: Color| {
        let mut col = col;
        col.apply_opacity(dim);
        col
    };
    match &c.kind {
        Kind::Toggle(on) => {
            let (tw, th) = (u(40.0), u(24.0));
            rect = (right - tw, cy - th / 2.0, tw, th);
            fill_rounded(pm, rect.0, rect.1, tw, th, th / 2.0, a(if *on { pal::accent() } else { crate::theme::th().ink(0.18) }));
            let kx = if *on { rect.0 + tw - th / 2.0 } else { rect.0 + th / 2.0 };
            fill_circle(pm, kx, cy + u(0.6), th / 2.0 - u(2.0), color(0, 0, 0, 0.25 * dim));
            fill_circle(pm, kx, cy, th / 2.0 - u(2.0), a(color(255, 255, 255, 1.0)));
        }
        Kind::Slider { v, min, max, unit, step } => {
            let shown = it.drag.as_ref().filter(|(id, _)| id == &c.id).map_or(*v, |(_, d)| *d);
            let label = format!("{}{}", format_num(shown, *step), unit);
            let natural_lw = u(58.0).max(p.measure(&label, u(12.5), Weight::Regular));
            // Keep ordinary rows unchanged. In a squeezed row, the old minimum label + track
            // widths pushed the pointer target beyond the window; budget the target padding too.
            let squeezed = natural_lw + u(12.0) + u(20.0) > w;
            let pad = if squeezed { u(8.0).min(w / 4.0) } else { u(8.0) };
            let gap = if squeezed { u(12.0).min(w / 8.0) } else { u(12.0) };
            let lw = if squeezed { (w - gap - pad - u(20.0).min(w / 2.0)).max(0.0) } else { natural_lw };
            let visible_label = if squeezed { p.fit(&label, lw, u(12.5), Weight::Regular) } else { label };
            p.text_right(pm, &visible_label, x + lw, cy, u(12.5), Weight::Regular, a(pal::sub()));
            let tx = x + lw + gap;
            let tw = if squeezed { (right - pad - tx).max(0.0) } else { (right - tx).max(u(20.0)) };
            let t = if max > min { ((shown - min) / (max - min)).clamp(0.0, 1.0) as f32 } else { 0.0 };
            fill_rounded(pm, tx, cy - u(2.0), tw, u(4.0), u(2.0), a(crate::theme::th().ink(0.18)));
            fill_rounded(pm, tx, cy - u(2.0), tw * t, u(4.0), u(2.0), a(pal::accent()));
            let kx = tx + tw * t;
            fill_circle(pm, kx, cy + u(1.0), if squeezed { u(9.0).min(pad) } else { u(9.0) }, color(0, 0, 0, 0.3 * dim));
            fill_circle(pm, kx, cy, if squeezed { u(8.5).min(pad) } else { u(8.5) }, a(color(250, 250, 250, 1.0)));
            rect = (tx - pad, cy - u(12.0), tw + 2.0 * pad, u(24.0));
            parts.push((tx, cy - u(12.0), tw, u(24.0)));
        }
        Kind::Stepper { v, step, unit, .. } => {
            let bw = u(26.0).max(p.measure("−", u(15.0), Weight::Medium) + u(8.0));
            let label = format!("{}{}", format_num(*v, *step), unit);
            let minus = (right - 2.0 * bw - u(4.0), cy - h_field / 2.0, bw, h_field);
            let plus = (right - bw, cy - h_field / 2.0, bw, h_field);
            p.text_right(pm, &label, minus.0 - u(10.0), cy, u(13.0), Weight::Regular, a(pal::text()));
            for (r, glyph) in [(minus, "−"), (plus, "+")] {
                fill_rounded(pm, r.0, r.1, r.2, r.3, u(6.0), a(pal::field()));
                p.text_center(pm, glyph, r.0 + r.2 / 2.0, cy, u(15.0), Weight::Medium, a(pal::text()));
                parts.push(r);
            }
            rect = (x, rect.1, w, h_field);
        }
        Kind::Seg(opts, sel) => {
            fill_rounded(pm, x, rect.1, w, h_field, u(7.0), a(pal::field()));
            let mut sx = x + u(2.0);
            for (i, (l, _)) in opts.iter().enumerate() {
                let sw = (p.measure(l, u(12.0), Weight::Regular) + u(22.0)).min((w - u(4.0)) / opts.len().max(1) as f32);
                let r = (sx, rect.1 + u(2.0), sw, h_field - u(4.0));
                if *sel == Some(i) {
                    fill_rounded(pm, r.0, r.1, r.2, r.3, u(5.5), a(pal::tonal(crate::theme::th().surface, 0.30)));
                    stroke_rounded(pm, r.0 + u(0.5), r.1 + u(0.5), r.2 - u(1.0), r.3 - u(1.0), u(5.0), a(crate::theme::th().accent.color(0.55)), u(1.0));
                }
                let text_size = u(if ctl_width(p, c) > max_width { 11.0 } else { 12.0 });
                let label = p.fit(l, (sw - u(4.0)).max(0.0), text_size, Weight::Regular);
                p.text_center(pm, &label, r.0 + r.2 / 2.0, cy, text_size, if *sel == Some(i) { Weight::Medium } else { Weight::Regular }, a(pal::text()));
                parts.push(r);
                sx += sw;
            }
        }
        Kind::Popup(opts, sel) => {
            fill_rounded(pm, x, rect.1, w, h_field, u(6.0), a(if hover { pal::field_hover() } else { pal::field() }));
            let label = sel.and_then(|i| opts.get(i)).map_or("Choose…", |o| o.0.as_str());
            let label = p.fit(label, w - u(36.0), u(13.0), Weight::Regular);
            p.text(pm, &label, x + u(10.0), cy, u(13.0), Weight::Regular, a(pal::text()));
            chevrons(pm, x + w - u(14.0), cy, u(3.2), a(pal::sub()));
        }
        Kind::Color(hex) => {
            let sw = u(24.0);
            let col = parse_hex(hex);
            let r = (right - sw, cy - sw / 2.0, sw, sw);
            match col {
                Some((cr, cg, cb)) => fill_circle(pm, r.0 + sw / 2.0, cy, sw / 2.0, a(color(cr, cg, cb, 1.0))),
                None => {
                    fill_circle(pm, r.0 + sw / 2.0, cy, sw / 2.0, a(crate::theme::th().ink(0.1)));
                    p.text_center(pm, "·", r.0 + sw / 2.0, cy, u(14.0), Weight::Bold, a(pal::sub()));
                }
            }
            stroke_rounded(pm, r.0, r.1, sw, sw, sw / 2.0, crate::theme::th().ink(0.35 * dim), u(1.0));
            let t = if hex.is_empty() { "default".to_string() } else { hex.clone() };
            p.text_right(pm, &t, r.0 - u(10.0), cy, u(12.5), Weight::Regular, a(pal::sub()));
            rect = (x, rect.1, w, h_field);
        }
        Kind::Text { val, hint } => {
            let editing = it.edit.as_ref().filter(|(id, _, _)| id == &c.id);
            let bg = if editing.is_some() {
                pal::window()
            } else if hover {
                pal::field_hover()
            } else {
                pal::field()
            };
            fill_rounded(pm, x, rect.1, w, h_field, u(6.0), a(bg));
            if editing.is_some() {
                stroke_rounded(pm, x, rect.1, w, h_field, u(6.0), pal::accent(), u(1.5));
            }
            let (shown, cur) = match editing {
                Some((_, t, cur)) => (t.clone(), Some(*cur)),
                None => (val.clone(), None),
            };
            // password fields (id contains ":secret") show bullets
            let shown = if c.id.contains(":secret") { "•".repeat(shown.chars().count()) } else { shown };
            let inner = w - u(16.0);
            if shown.is_empty() && cur.is_none() {
                p.text(pm, &p.fit(hint, inner, u(12.5), Weight::Regular), x + u(8.0), cy, u(12.5), Weight::Regular, a(pal::faint()));
            } else if let Some(cur) = cur {
                // keep the cursor visible: show the tail that fits
                let chars: Vec<char> = shown.chars().collect();
                let mut start = 0;
                while start < cur && p.measure(&chars[start..cur].iter().collect::<String>(), u(12.5), Weight::Regular) > inner - u(4.0) {
                    start += 1;
                }
                let vis: String = chars[start..].iter().collect();
                let vis = p.fit(&vis, inner, u(12.5), Weight::Regular);
                if let Some((a, b)) = it.edit_sel.filter(|(a, b)| a < b) {
                    let at = |i: usize| x + u(8.0) + p.measure(&chars[start..i.clamp(start, chars.len())].iter().collect::<String>(), u(12.5), Weight::Regular);
                    let (sx, ex) = (at(a), at(b).min(x + w - u(8.0)));
                    if ex > sx {
                        fill_px(pm, sx, cy - u(9.0), ex - sx, u(18.0), crate::theme::th().accent.color(0.35));
                    }
                }
                p.text(pm, &vis, x + u(8.0), cy, u(12.5), Weight::Regular, pal::text());
                let cx = x + u(8.0) + p.measure(&chars[start..cur].iter().collect::<String>(), u(12.5), Weight::Regular);
                fill_px(pm, cx, cy - u(8.0), u(1.2), u(16.0), pal::accent());
            } else {
                p.text(pm, &p.fit(&shown, inner, u(12.5), Weight::Regular), x + u(8.0), cy, u(12.5), Weight::Regular, a(pal::text()));
            }
        }
        Kind::Key(k) => {
            let rec = it.recording.as_ref().filter(|(id, _)| id == &c.id);
            fill_rounded(
                pm,
                x,
                rect.1,
                w,
                h_field,
                u(6.0),
                if rec.is_some() {
                    crate::theme::th().accent.color(0.28)
                } else if hover {
                    pal::field_hover()
                } else {
                    pal::field()
                },
            );
            if rec.is_some() {
                stroke_rounded(pm, x, rect.1, w, h_field, u(6.0), pal::accent(), u(1.5));
                let mods = rec.map(|(_, m)| m.join("+")).unwrap_or_default();
                let t = if mods.is_empty() { "Type shortcut…".to_string() } else { format!("{mods}+…") };
                p.text_center(pm, &t, x + w / 2.0, cy, u(12.0), Weight::Medium, pal::text());
            } else {
                p.text_center(pm, &p.fit(k, w - u(12.0), u(12.0), Weight::Medium), x + w / 2.0, cy, u(12.0), Weight::Medium, a(pal::text()));
            }
        }
        Kind::Button { label, danger } => {
            fill_rounded(pm, x, rect.1, w, h_field, u(6.0), a(if hover { pal::field_hover() } else { pal::field() }));
            p.text_center(pm, &p.fit(label, w - u(12.0), u(12.5), Weight::Regular), x + w / 2.0, cy, u(12.5), Weight::Regular, a(if *danger { pal::red() } else { pal::text() }));
        }
        Kind::Chip { label, on } => {
            fill_rounded(
                pm,
                x,
                rect.1,
                w,
                h_field,
                u(6.0),
                a(if *on {
                    pal::accent()
                } else if hover {
                    pal::field_hover()
                } else {
                    pal::field()
                }),
            );
            p.text_center(
                pm,
                &p.fit(label, w - u(12.0), u(12.5), Weight::Regular),
                x + w / 2.0,
                cy,
                u(12.5),
                if *on { Weight::Medium } else { Weight::Regular },
                a(if *on { pal::accent_text() } else { pal::text() }),
            );
        }
        Kind::Label(t) => {
            p.text_right(pm, t, right, cy, u(13.0), Weight::Regular, pal::sub());
        }
    }
    if focus && it.ring && c.enabled {
        focus_ring(pm, rect, u(6.0), s);
    }
    Placed { rect, ctl: c.clone(), parts, row_rect, label: String::new() }
}

fn chevrons(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: Color) {
    use tiny_skia::{Paint, PathBuilder, Stroke, Transform};
    let mut pb = PathBuilder::new();
    pb.move_to(cx - r, cy - r * 0.9);
    pb.line_to(cx, cy - r * 1.9);
    pb.line_to(cx + r, cy - r * 0.9);
    pb.move_to(cx - r, cy + r * 0.9);
    pb.line_to(cx, cy + r * 1.9);
    pb.line_to(cx + r, cy + r * 0.9);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        pm.stroke_path(&path, &paint, &Stroke { width: r * 0.5, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() }, Transform::identity(), None);
    }
}

// ---------------------------------------------------------------- items

/// Lays out and paints `items` into `pm`, in the column starting at x = `ox` and `width` wide.
/// `scroll` is subtracted from y; the frame's rectangles are in `pm` coordinates. Thumbnails are looked up by `thumb`.
#[allow(clippy::too_many_arguments)]
pub fn draw_items(pm: &mut Pixmap, p: &mut Painter, head: Head, items: &[Item], scroll: f32, ox: f32, width: f32, it: &Interact, thumb: &dyn Fn(&str) -> Option<std::rc::Rc<Pixmap>>) -> Frame {
    let s = p.s;
    let u = |v: f32| v * s;
    let mut fr = Frame::default();
    let side = u(if width < u(400.0) { 16.0 } else { 32.0 });
    let cw = (width - 2.0 * side).min(u(700.0));
    let x0 = ox + ((width - cw) / 2.0).max(side.min(width / 2.0));
    let metrics = TextLayout::live();
    let mut y = u(28.0) - scroll;
    // The title wraps (a very narrow page with enlarged text) rather than being cut at the edge.
    let title_lines = wrap_title(p, head.title, cw, u(25.0));
    let title_lh = u(30.0 * metrics.0);
    for (i, l) in title_lines.iter().enumerate() {
        p.text_as(Role::Title, pm, l, x0, y + u(18.0 + metrics.extra(14.0)) + title_lh * i as f32, u(25.0), Weight::SemiBold, pal::text());
    }
    let title_extra = title_lh * (title_lines.len() - 1) as f32;
    let page_top = y;
    fr.head = (x0, y, cw, title_lh * title_lines.len() as f32 + u(8.0));
    y += u(62.0 + metrics.extra(30.0)) + title_extra;
    if !head.context.is_empty() {
        // One quiet line of orientation under the title; it wraps rather than truncates.
        let lines = p.wrap(head.context, cw, u(12.5), Weight::Regular);
        let lh = u(17.0 * metrics.0);
        let first = page_top + u(48.0 + metrics.extra(40.0)) + title_extra;
        for (i, l) in lines.iter().enumerate() {
            p.text(pm, l, x0, first + lh * i as f32, u(12.5), Weight::Regular, pal::sub());
        }
        y = y.max(first + lh * (lines.len() as f32 - 0.5) + u(16.0));
    }
    for item in items {
        let (span_y, span_p) = (y, fr.placed.len());
        match item {
            Item::Space(h) => y += u(*h),
            Item::WidgetPreview { toml, caption } => y = super::widgetbuilder::draw_preview(pm, p, toml, caption, x0, y, cw),
            Item::Editor(v) => {
                let r = (x0, y, cw, u(v.height));
                let hover = it.hover.as_deref() == Some(v.id.as_str());
                let (mut placed, geo) = super::editor::draw(pm, p, v, r, hover);
                if it.focus.as_deref() == Some(v.id.as_str()) && it.ring && !v.active {
                    stroke_rounded(pm, r.0 - u(2.0), r.1 - u(2.0), r.2 + u(4.0), r.3 + u(4.0), u(10.0), crate::theme::th().accent.color(0.85), u(2.0));
                }
                placed.row_rect = r;
                fr.placed.push(placed);
                fr.editors.push((r, geo));
                y += r.3 + u(14.0);
            }
            Item::IconThemes(rows) => y = super::iconspane::draw_list(pm, p, rows, x0, y, cw, it, &mut fr),
            Item::IconGrid(g) => y = super::iconspane::draw_grid(pm, p, g, x0, y, cw),
            Item::ThemeCards(cards) => y = super::themecards::draw(pm, p, cards, x0, y, cw, it, &mut fr),
            Item::Nav { id, opts, sel, secondary } => y = draw_nav(pm, p, id, opts, *sel, *secondary, x0, y, cw, it, &mut fr),
            Item::Header(t) => {
                y += u(6.0);
                p.text(pm, t, x0, y + u(8.0 + metrics.extra(7.0)), u(12.0), Weight::SemiBold, pal::text());
                y += u(22.0 + metrics.extra(16.0));
            }
            Item::Note { text, warn } => {
                let lines = p.wrap(text, cw - u(28.0), u(12.0), Weight::Regular);
                let lh = u(16.0 * metrics.0);
                if *warn {
                    let h = lines.len() as f32 * lh + u(16.0);
                    fill_rounded(pm, x0, y, cw, h, u(8.0), crate::theme::th().danger.color(0.16));
                    for (i, l) in lines.iter().enumerate() {
                        p.text(pm, l, x0 + u(14.0), y + u(8.0) + lh * (i as f32 + 0.5), u(12.0), Weight::Regular, crate::theme::th().danger.over(crate::theme::th().text, 0.75).color(1.0));
                    }
                    y += h + u(10.0);
                } else {
                    for (i, l) in lines.iter().enumerate() {
                        p.text(pm, l, x0 + u(14.0), y + u(4.0) + lh * (i as f32 + 0.5), u(12.0), Weight::Regular, pal::faint());
                    }
                    y += lines.len() as f32 * lh + u(12.0);
                }
            }
            Item::Card(rows) => {
                // A row whose controls do not fit beside its label moves them onto a line of their own (Dux's compact
                // button-group row) instead of shrinking or truncating them; narrow cards and enlarged text do it for any control.
                let narrow = cw < u(380.0);
                let stack: Vec<bool> = rows
                    .iter()
                    .map(|r| {
                        if r.label.is_empty() {
                            return false;
                        }
                        let controls: f32 = r.ctls.iter().map(|c| ctl_width(p, c) + u(10.0)).sum();
                        let tight = controls + p.measure(&r.label, u(13.0), Weight::Regular) + u(2.0 * CARD_PAD) > cw;
                        narrow || (tight && (metrics.0 > 1.0 || r.ctls.iter().any(|c| matches!(c.kind, Kind::Seg(..)))))
                    })
                    .collect();
                // A detail that does not fit beside the controls wraps (and its row grows) rather than ending in an ellipsis.
                let detail_lines: Vec<Vec<String>> = rows
                    .iter()
                    .zip(&stack)
                    .map(|(r, st)| {
                        if r.detail.is_empty() {
                            return Vec::new();
                        }
                        let controls: f32 = if *st { 0.0 } else { r.ctls.iter().map(|c| ctl_width(p, c).min(cw) + u(10.0)).sum() };
                        let avail = (cw - u(2.0 * CARD_PAD) - if r.chip.is_some() { u(28.0) } else { 0.0 } - controls).max(u(80.0));
                        if p.measure(&r.detail, u(11.0), Weight::Regular) <= avail {
                            Vec::new()
                        } else {
                            p.wrap(&r.detail, avail, u(11.0), Weight::Regular)
                        }
                    })
                    .collect();
                let detail_lh = u(14.0 * metrics.0);
                let heights: Vec<f32> = rows
                    .iter()
                    .zip(&stack)
                    .zip(&detail_lines)
                    .map(|((r, st), dl)| u(metrics.row(r) + if *st { (32.0 + metrics.extra(20.0)) * r.ctls.len() as f32 } else { 0.0 }) + dl.len().saturating_sub(1) as f32 * detail_lh)
                    .collect();
                let h: f32 = heights.iter().sum();
                fill_rounded(pm, x0, y, cw, h, u(CARD_R), pal::card());
                if cw > u(1.0) && h > u(1.0) {
                    stroke_rounded(pm, x0 + u(0.5), y + u(0.5), cw - u(1.0), h - u(1.0), u(CARD_R), pal::sep(), u(1.0));
                }
                let mut ry = y;
                for (i, r) in rows.iter().enumerate() {
                    let rh = heights[i];
                    let stacked = stack[i];
                    let cy = if stacked && !r.ctls.is_empty() { ry + rh - u(22.0 + metrics.extra(10.0)) } else { ry + rh / 2.0 };
                    let row_rect = (x0, ry, cw, rh);
                    if r.selected {
                        fill_px(pm, x0 + u(2.0), ry, cw - u(4.0), rh, crate::theme::th().accent.color(0.22));
                    }
                    if i > 0 {
                        fill_px(pm, x0 + u(CARD_PAD), ry, cw - u(2.0 * CARD_PAD), u(1.0).max(1.0), pal::sep());
                    }
                    let mut lx = x0 + u(CARD_PAD);
                    if let Some((cr, cg, cb)) = r.chip {
                        fill_rounded(pm, lx, cy - u(9.0), u(18.0), u(18.0), u(5.0), color(cr, cg, cb, 1.0));
                        lx += u(28.0);
                    }
                    // controls, right to left
                    let mut right = x0 + cw - u(CARD_PAD);
                    let mut first_id = None;
                    let mut control_cy = cy;
                    for c in r.ctls.iter().rev() {
                        let mut placed = draw_ctl(pm, p, c, right, control_cy, it, row_rect, (right - x0 - u(CARD_PAD)).max(u(24.0)));
                        placed.label = r.label.clone();
                        if stacked {
                            control_cy -= u(32.0 + metrics.extra(20.0));
                        } else {
                            right = placed.rect.0 - u(10.0);
                        }
                        if c.focusable() {
                            first_id = Some(c.id.clone());
                        }
                        fr.placed.push(placed);
                    }
                    // restore left-to-right order for focus: reverse the slice we just pushed
                    let n = r.ctls.len();
                    let len = fr.placed.len();
                    fr.placed[len - n..].reverse();
                    if let Some(id) = first_id {
                        fr.rows.push((row_rect, id));
                    }
                    let avail = (if stacked { x0 + cw - u(CARD_PAD) - lx } else { right - lx }).max(u(40.0));
                    let label_cy = if stacked && !r.ctls.is_empty() { ry + u(20.0 + metrics.extra(if r.detail.is_empty() { 10.0 } else { 18.0 })) } else { cy };
                    let (label_offset, detail_offset) = metrics.label_offsets(!r.detail.is_empty());
                    // Side-by-side rows keep their wrapped text centred; stacked rows pin the label to the top.
                    let dl = &detail_lines[i];
                    let shift = if stacked { 0.0 } else { dl.len().saturating_sub(1) as f32 * detail_lh / 2.0 };
                    let ly = label_cy + u(label_offset) - shift;
                    let label = p.fit(&r.label, avail, u(13.0), Weight::Regular);
                    p.text(pm, &label, lx, ly, u(13.0), Weight::Regular, pal::text());
                    if dl.is_empty() {
                        if !r.detail.is_empty() {
                            let d = p.fit(&r.detail, avail, u(11.0), Weight::Regular);
                            p.text(pm, &d, lx, label_cy + u(detail_offset), u(11.0), Weight::Regular, pal::sub());
                        }
                    } else {
                        for (k, l) in dl.iter().enumerate() {
                            p.text(pm, l, lx, label_cy + u(detail_offset) - shift + detail_lh * k as f32, u(11.0), Weight::Regular, pal::sub());
                        }
                    }
                    ry += rh;
                }
                y += h + u(18.0);
            }
            Item::Preview { id, caption } => {
                let ph = if id == "lw:motion" { cw * 9.0 / 16.0 } else { cw * 0.38 };
                let pw = if id == "lw:motion" { cw } else { ph * 16.0 / 10.0 };
                let px0 = x0 + (cw - pw) / 2.0;
                fill_rounded(pm, px0 - u(2.0), y - u(2.0), pw + u(4.0), ph + u(4.0), u(10.0), crate::theme::th().ink(0.10));
                match if y + ph > 0.0 && y < pm.height() as f32 { thumb(id) } else { None } {
                    Some(t) => draw_cover(pm, &t, px0, y, pw, ph, u(8.0)),
                    None => {
                        fill_rounded(pm, px0, y, pw, ph, u(8.0), pal::card());
                        p.text_center(pm, if id.is_empty() { "No wallpaper image" } else { "Loading…" }, px0 + pw / 2.0, y + ph / 2.0, u(12.0), Weight::Regular, pal::faint());
                    }
                }
                y += ph + u(14.0 + metrics.extra(8.0));
                p.text_center(pm, caption, x0 + cw / 2.0, y + u(2.0), u(12.0), Weight::Regular, pal::sub());
                y += u(30.0 + metrics.extra(16.0));
            }
            Item::Palette(list) => {
                let gap = u(8.0);
                let cols = list.len().clamp(1, (((cw + gap) / (u(60.0) + gap)).floor() as usize).max(1));
                let w = (cw - gap * (cols as f32 - 1.0)) / cols as f32;
                let (h, row_h) = (u(40.0), u(70.0 + metrics.extra(28.0)));
                for (i, chip) in list.iter().enumerate() {
                    let r = (x0 + (i % cols) as f32 * (w + gap), y + (i / cols) as f32 * row_h, w, h);
                    let (cr, cg, cb) = chip.color;
                    fill_rounded(pm, r.0, r.1, r.2, r.3, u(8.0), color(cr, cg, cb, 1.0));
                    stroke_rounded(pm, r.0, r.1, r.2, r.3, u(8.0), crate::theme::th().ink(0.22), u(1.0));
                    let name = p.fit(&chip.name, w + gap, u(10.5), Weight::Regular);
                    p.text_center(pm, &name, r.0 + w / 2.0, r.1 + h + u(11.0 + metrics.extra(6.0)), u(10.5), Weight::Regular, pal::sub());
                    p.text_center(pm, &format!("#{cr:02x}{cg:02x}{cb:02x}"), r.0 + w / 2.0, r.1 + h + u(24.0 + metrics.extra(19.0)), u(9.5), Weight::Regular, pal::faint());
                }
                y += list.len().div_ceil(cols) as f32 * row_h + u(4.0);
            }
            Item::Thumbs(list) => {
                let gap = u(14.0);
                let tw0 = u(TH_W);
                let cols = (((cw + gap) / (tw0 + gap)).floor() as usize).max(1);
                let tw = (cw - gap * (cols as f32 - 1.0)) / cols as f32;
                let th = tw * TH_H / TH_W;
                for (i, t) in list.iter().enumerate() {
                    let (cx, cyy) = ((i % cols) as f32, (i / cols) as f32);
                    let r = (x0 + cx * (tw + gap), y + cyy * (th + u(34.0 + metrics.extra(16.0))), tw, th);
                    let cid = format!("w:{}", t.id);
                    if r.1 + r.3 + u(34.0 + metrics.extra(16.0)) > 0.0 && r.1 < pm.height() as f32 {
                        let hover = it.hover.as_deref() == Some(cid.as_str());
                        if t.selected {
                            stroke_rounded(pm, r.0 - u(3.0), r.1 - u(3.0), r.2 + u(6.0), r.3 + u(6.0), u(10.0), pal::accent(), u(2.5));
                        } else if hover {
                            stroke_rounded(pm, r.0 - u(3.0), r.1 - u(3.0), r.2 + u(6.0), r.3 + u(6.0), u(10.0), crate::theme::th().ink(0.3), u(2.0));
                        }
                        match thumb(&t.id) {
                            Some(img) => draw_cover(pm, &img, r.0, r.1, r.2, r.3, u(7.0)),
                            None => fill_rounded(pm, r.0, r.1, r.2, r.3, u(7.0), crate::theme::th().ink(0.07)),
                        }
                        if it.focus.as_deref() == Some(cid.as_str()) && it.ring {
                            stroke_rounded(pm, r.0 - u(4.0), r.1 - u(4.0), r.2 + u(8.0), r.3 + u(8.0), u(11.0), crate::theme::th().accent.color(0.85), u(2.0));
                        }
                        let cap = p.fit(&t.caption, tw, u(11.0), Weight::Regular);
                        p.text_center(pm, &cap, r.0 + tw / 2.0, r.1 + th + u(14.0 + metrics.extra(7.0)), u(11.0), Weight::Regular, pal::sub());
                    }
                    fr.thumbs.push((r, t.id.clone()));
                    fr.placed.push(Placed {
                        rect: r,
                        ctl: Ctl::new(cid, Kind::Button { label: t.caption.clone(), danger: false }),
                        parts: vec![],
                        row_rect: (x0, r.1, cw, r.3),
                        label: t.caption.clone(),
                    });
                }
                let rows = list.len().div_ceil(cols);
                y += rows as f32 * (th + u(34.0 + metrics.extra(16.0))) + u(8.0);
            }
            Item::Layouts(list) => {
                let gap = u(12.0);
                let tw0 = u(96.0);
                let cols = (((cw + gap) / (tw0 + gap)).floor() as usize).max(1);
                let tw = (cw - gap * (cols as f32 - 1.0)) / cols as f32;
                let th = tw * 0.78;
                for (i, t) in list.iter().enumerate() {
                    let (cx, cyy) = ((i % cols) as f32, (i / cols) as f32);
                    let r = (x0 + cx * (tw + gap), y + cyy * (th + u(34.0 + metrics.extra(16.0))), tw, th);
                    let hover = it.hover.as_deref() == Some(t.id.as_str());
                    fill_rounded(pm, r.0, r.1, r.2, r.3, u(9.0), if hover { crate::theme::th().ink(0.14) } else { pal::card() });
                    if t.selected {
                        stroke_rounded(pm, r.0, r.1, r.2, r.3, u(9.0), pal::accent(), u(2.5));
                    }
                    super::icons::layout_icon(pm, &t.name, (r.0 + u(10.0), r.1 + u(9.0), r.2 - u(20.0), r.3 - u(18.0)), s, if t.selected { pal::accent() } else { crate::theme::th().ink(0.55) });
                    if it.focus.as_deref() == Some(t.id.as_str()) && it.ring {
                        stroke_rounded(pm, r.0 - u(2.0), r.1 - u(2.0), r.2 + u(4.0), r.3 + u(4.0), u(10.0), crate::theme::th().accent.color(0.85), u(2.0));
                    }
                    p.text_center(pm, &t.name, r.0 + tw / 2.0, r.1 + th + u(13.0 + metrics.extra(7.0)), u(11.0), Weight::Regular, if t.selected { pal::text() } else { pal::sub() });
                    fr.layouts.push((r, t.id.clone()));
                    fr.placed.push(Placed {
                        rect: r,
                        ctl: Ctl::new(t.id.clone(), Kind::Button { label: t.name.clone(), danger: false }),
                        parts: vec![],
                        row_rect: (x0, r.1, cw, r.3),
                        label: t.name.clone(),
                    });
                }
                let rows = list.len().div_ceil(cols);
                y += rows as f32 * (th + u(34.0 + metrics.extra(16.0))) + u(8.0);
            }
            Item::FontSamples(lines) => {
                let lh = u(46.0 + metrics.extra(36.0));
                let h = lines.len() as f32 * lh;
                fill_rounded(pm, x0, y, cw, h, u(CARD_R), pal::card());
                if cw > u(1.0) && h > u(1.0) {
                    stroke_rounded(pm, x0 + u(0.5), y + u(0.5), cw - u(1.0), h - u(1.0), u(CARD_R), pal::sep(), u(1.0));
                }
                for (i, l) in lines.iter().enumerate() {
                    let ry = y + i as f32 * lh;
                    if i > 0 {
                        fill_px(pm, x0 + u(CARD_PAD), ry, cw - u(2.0 * CARD_PAD), u(1.0).max(1.0), pal::sep());
                    }
                    p.text(pm, &l.label, x0 + u(CARD_PAD), ry + u(13.0 + metrics.extra(6.0)), u(11.0), Weight::Regular, pal::faint());
                    // the sample is cut at the card's right edge instead of wrapping
                    let avail = cw - 2.0 * u(CARD_PAD);
                    let mut shown = l.text.clone();
                    while shown.chars().count() > 1 && p.measure_as(l.role, &shown, u(l.px), l.weight) > avail {
                        shown.pop();
                    }
                    p.text_as(l.role, pm, &shown, x0 + u(CARD_PAD), ry + u(31.0 + metrics.extra(24.0)), u(l.px), l.weight, pal::text());
                }
                y += h + u(18.0);
            }
            Item::Themes(list) => {
                let gap = u(14.0);
                let tw0 = u(150.0);
                let cols = (((cw + gap) / (tw0 + gap)).floor() as usize).max(1);
                let tw = (cw - gap * (cols as f32 - 1.0)) / cols as f32;
                let th = tw * 0.66;
                for (i, t) in list.iter().enumerate() {
                    let (cx, cyy) = ((i % cols) as f32, (i / cols) as f32);
                    let r = (x0 + cx * (tw + gap), y + cyy * (th + u(36.0 + metrics.extra(16.0))), tw, th);
                    let hover = it.hover.as_deref() == Some(t.id.as_str());
                    let on = r.1 + r.3 + u(36.0) >= 0.0 && r.1 <= pm.height() as f32; // off-screen tiles are laid out, not painted
                    if on {
                        draw_theme_preview(pm, r, s, t);
                    }
                    if on && t.selected {
                        stroke_rounded(pm, r.0 - u(3.0), r.1 - u(3.0), r.2 + u(6.0), r.3 + u(6.0), u(11.0), pal::accent(), u(2.5));
                    } else if on && hover {
                        stroke_rounded(pm, r.0 - u(3.0), r.1 - u(3.0), r.2 + u(6.0), r.3 + u(6.0), u(11.0), crate::theme::th().ink(0.3), u(2.0));
                    }
                    if it.focus.as_deref() == Some(t.id.as_str()) && it.ring {
                        stroke_rounded(pm, r.0 - u(5.0), r.1 - u(5.0), r.2 + u(10.0), r.3 + u(10.0), u(12.0), crate::theme::th().accent.color(0.85), u(2.0));
                    }
                    let name = if on { p.fit(&t.name, tw, u(12.0), Weight::Regular) } else { String::new() };
                    p.text_center(
                        pm,
                        &name,
                        r.0 + tw / 2.0,
                        r.1 + th + u(15.0 + metrics.extra(7.0)),
                        u(12.0),
                        if t.selected { Weight::Medium } else { Weight::Regular },
                        if t.error {
                            pal::red()
                        } else if t.selected {
                            pal::text()
                        } else {
                            pal::sub()
                        },
                    );
                    fr.layouts.push((r, t.id.clone()));
                    fr.placed.push(Placed {
                        rect: r,
                        ctl: Ctl::new(t.id.clone(), Kind::Button { label: t.name.clone(), danger: false }),
                        parts: vec![],
                        row_rect: (x0, r.1, cw, r.3),
                        label: t.name.clone(),
                    });
                }
                let rows = list.len().div_ceil(cols);
                y += rows as f32 * (th + u(36.0 + metrics.extra(16.0))) + u(6.0);
            }
            Item::Swatches(list) => {
                let d = u(26.0);
                let gap = u(12.0);
                let row_h = d + u(16.0);
                let mut x = x0 + u(4.0);
                let mut yy = y + u(6.0);
                for sw in list {
                    if x + d > x0 + cw {
                        x = x0 + u(4.0);
                        yy += d + gap;
                    }
                    let r = (x, yy, d, d);
                    let hover = it.hover.as_deref() == Some(sw.id.as_str());
                    let (cr, cg, cb) = sw.color;
                    fill_circle(pm, x + d / 2.0, yy + d / 2.0, d / 2.0, color(cr, cg, cb, 1.0));
                    stroke_rounded(pm, x, yy, d, d, d / 2.0, crate::theme::th().ink(0.25), u(1.0));
                    if sw.unset {
                        let ink = crate::theme::th().text_c();
                        super::app::stroke_line(pm, x + d * 0.25, yy + d * 0.75, x + d * 0.75, yy + d * 0.25, ink, u(1.8));
                    }
                    if sw.selected {
                        stroke_rounded(pm, x - u(3.5), yy - u(3.5), d + u(7.0), d + u(7.0), (d + u(7.0)) / 2.0, crate::theme::th().text_c(), u(2.0));
                    } else if hover {
                        stroke_rounded(pm, x - u(3.0), yy - u(3.0), d + u(6.0), d + u(6.0), (d + u(6.0)) / 2.0, crate::theme::th().ink(0.4), u(1.5));
                    }
                    if it.focus.as_deref() == Some(sw.id.as_str()) && it.ring {
                        stroke_rounded(pm, x - u(5.5), yy - u(5.5), d + u(11.0), d + u(11.0), (d + u(11.0)) / 2.0, crate::theme::th().accent.color(0.85), u(2.0));
                    }
                    fr.layouts.push((r, sw.id.clone()));
                    fr.placed.push(Placed {
                        rect: r,
                        ctl: Ctl::new(sw.id.clone(), Kind::Button { label: sw.name.clone(), danger: false }),
                        parts: vec![],
                        row_rect: (x0, yy, cw, d),
                        label: sw.name.clone(),
                    });
                    x += d + gap;
                }
                y = yy + row_h + u(6.0);
            }
        }
        fr.spans.push(((x0, span_y, cw, (y - span_y).max(0.0)), span_p..fr.placed.len()));
    }
    fr.height = y + scroll + u(24.0);
    fr
}

/// Greedy word wrap of the page title in its own font; a single word wider than `max` is split by characters.
fn wrap_title(p: &Painter, title: &str, max: f32, px: f32) -> Vec<String> {
    let fits = |t: &str| p.measure_as(Role::Title, t, px, Weight::SemiBold) <= max;
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in title.split_whitespace() {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if fits(&cand) {
            cur = cand;
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        for ch in word.chars() {
            let mut cand = cur.clone();
            cand.push(ch);
            if !cur.is_empty() && !fits(&cand) {
                lines.push(std::mem::take(&mut cur));
                cur.push(ch);
            } else {
                cur = cand;
            }
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Flat page navigation (see [`Item::Nav`]): underlined tabs or chips, wrapped onto as many lines as the width needs so every
/// label stays complete. One `Placed` with one part per option keeps the control id, mouse hits and Left/Right keys of a segmented choice.
#[allow(clippy::too_many_arguments)]
fn draw_nav(pm: &mut Pixmap, p: &mut Painter, id: &str, opts: &[Opt], sel: Option<usize>, secondary: bool, x0: f32, y: f32, cw: f32, it: &Interact, fr: &mut Frame) -> f32 {
    let s = p.s;
    let u = |v: f32| v * s;
    let m = TextLayout::live();
    let (th, pad, gap, px) = if secondary { (u(28.0 + m.extra(16.0)), u(14.0), u(8.0), u(12.5)) } else { (u(36.0 + m.extra(20.0)), u(14.0), 0.0, u(13.0)) };
    let step = th + if secondary { gap } else { 0.0 };
    let mut parts: Vec<Rect> = Vec::new();
    let (mut x, mut line) = (x0, 0usize);
    for (l, _) in opts {
        // Always measured in the selected weight so a choice never moves its neighbours.
        let w = (p.measure(l, px, Weight::Medium) + 2.0 * pad).min(cw);
        if x > x0 && x + w > x0 + cw + 0.5 {
            x = x0;
            line += 1;
        }
        parts.push((x, y + line as f32 * step, w, th));
        x += w + gap;
    }
    let lines = line + 1;
    let hover_id = it.hover.as_deref() == Some(id);
    let hover_part = if hover_id { it.hover_part } else { None };
    let th_ = crate::theme::th();
    if !secondary {
        for n in 0..lines {
            fill_px(pm, x0, y + (n as f32 + 1.0) * step - 1.0, cw, 1.0, pal::sep());
        }
    }
    for (i, ((l, _), r)) in opts.iter().zip(&parts).enumerate() {
        let on = sel == Some(i);
        let hot = hover_part == Some(i);
        let label = p.fit(l, r.2 - 2.0 * pad + u(4.0), px, Weight::Medium);
        if secondary {
            if on {
                fill_rounded(pm, r.0, r.1, r.2, r.3, r.3 / 2.0, pal::tonal(th_.background, 0.20));
                stroke_rounded(pm, r.0 + u(0.5), r.1 + u(0.5), r.2 - u(1.0), r.3 - u(1.0), r.3 / 2.0, th_.accent.color(0.55), u(1.0));
            } else {
                fill_rounded(pm, r.0, r.1, r.2, r.3, r.3 / 2.0, if hot { th_.ink(0.08) } else { th_.ink(0.04) });
            }
        } else {
            if hot && !on {
                fill_rounded(pm, r.0 + u(2.0), r.1 + u(4.0), r.2 - u(4.0), r.3 - u(8.0), u(7.0), th_.ink(0.06));
            }
            if on {
                fill_rounded(pm, r.0 + pad, r.1 + r.3 - u(3.0), r.2 - 2.0 * pad, u(3.0), u(1.5), pal::accent());
            }
        }
        p.text_center(
            pm,
            &label,
            r.0 + r.2 / 2.0,
            r.1 + r.3 / 2.0 - if secondary { 0.0 } else { u(1.0) },
            px,
            if on { Weight::Medium } else { Weight::Regular },
            if on { pal::text() } else { pal::sub() },
        );
    }
    let rect = (x0, y, cw, lines as f32 * step - if secondary { gap } else { 0.0 });
    if it.focus.as_deref() == Some(id) && it.ring {
        let r = sel.and_then(|i| parts.get(i)).or(parts.first()).copied().unwrap_or(rect);
        focus_ring(pm, r, if secondary { r.3 / 2.0 } else { u(7.0) }, s);
    }
    fr.placed.push(Placed { rect, ctl: Ctl::new(id, Kind::Seg(opts.to_vec(), sel)), parts, row_rect: rect, label: String::new() });
    y + rect.3 + u(if secondary { 12.0 } else { 14.0 })
}

/// Palette tiles show semantic swatches only. Desktop designs use generated native previews.
pub(super) fn draw_theme_preview(pm: &mut Pixmap, r: Rect, s: f32, t: &ThemeTile) {
    let (x, y, w, h) = r;
    fill_rounded(pm, x, y, w, h, 8.0 * s, color(t.colors[0].0, t.colors[0].1, t.colors[0].2, 1.0));
    let gap = 4.0 * s;
    let count = t.colors.len() as f32;
    let sw = (w - gap * (count + 1.0)) / count;
    for (i, c) in t.colors.iter().enumerate() {
        fill_rounded(pm, x + gap + i as f32 * (sw + gap), y + gap, sw, (h - 2.0 * gap).max(1.0), 3.0 * s, color(c.0, c.1, c.2, 1.0));
    }
}

/// Draws `img` scaled to cover the rounded rect (centre crop).
pub fn draw_cover(pm: &mut Pixmap, img: &Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32) {
    use tiny_skia::{FillRule, FilterQuality, Paint, Pattern, SpreadMode, Transform};
    let sc = (w / img.width() as f32).max(h / img.height() as f32);
    let tx = x + (w - img.width() as f32 * sc) / 2.0;
    let ty = y + (h - img.height() as f32 * sc) / 2.0;
    let pattern = Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, Transform::from_scale(sc, sc).post_translate(tx, ty));
    let paint = Paint { shader: pattern, anti_alias: true, ..Paint::default() };
    if let Some(path) = crate::gfx::rounded_rect(x, y, w, h, r) {
        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

// ---------------------------------------------------------------- control logic (pure, testable)

/// New value after a click at `x` on a slider track `(tx, tw)`.
pub fn slider_value(x: f32, track: (f32, f32, f32, f32), min: f64, max: f64, step: f64) -> f64 {
    let t = ((x - track.0) / track.2).clamp(0.0, 1.0) as f64;
    snap(min + t * (max - min), min, max, step)
}

pub fn snap(v: f64, min: f64, max: f64, step: f64) -> f64 {
    let v = if step > 0.0 { (v / step).round() * step } else { v };
    // avoid 0.30000000000000004
    ((v.clamp(min, max)) * 1e6).round() / 1e6
}

/// Which option a click at `x` hit in a segmented control (its parts are the segment rects).
pub fn part_at(parts: &[Rect], x: f32, y: f32) -> Option<usize> {
    parts.iter().position(|r| inside(*r, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squeezed_slider_keeps_its_entire_pointer_range_inside_the_budget() {
        for scale in [1.0, 2.0] {
            for width in [24.0, 40.0, 64.0, 96.0, 128.0] {
                let mut pm = Pixmap::new(800, 200).unwrap();
                let mut p = Painter { fonts: None, s: scale };
                let c = Ctl::new("travel", Kind::Slider { v: 120.0, min: 40.0, max: 2000.0, step: 1.0, unit: " logical pixels of travel".into() });
                let right = 700.0;
                let available = width * scale;
                let placed = draw_ctl(&mut pm, &mut p, &c, right, 100.0, &Interact::default(), (right - available, 0.0, available, 200.0), available);
                assert!(placed.rect.0 >= right - available - 0.01, "left: scale {scale}, width {width}");
                assert!(placed.rect.0 + placed.rect.2 <= right + 0.01, "right: scale {scale}, width {width}");
                let track = placed.parts[0];
                assert!(track.2 > 0.0 && track.0 >= placed.rect.0 && track.0 + track.2 <= placed.rect.0 + placed.rect.2);
                assert_eq!(slider_value(track.0, track, 40.0, 2000.0, 1.0), 40.0);
                assert_eq!(slider_value(track.0 + track.2, track, 40.0, 2000.0, 1.0), 2000.0);
                assert!(matches!(&placed.ctl.kind, Kind::Slider { v, unit, .. } if *v == 120.0 && unit == " logical pixels of travel"));
            }
        }
    }

    #[test]
    fn ordinary_slider_retains_its_existing_track_geometry() {
        let mut pm = Pixmap::new(500, 200).unwrap();
        let mut p = Painter { fonts: None, s: 1.0 };
        let c = Ctl::new("level", Kind::Slider { v: 1.0, min: 0.0, max: 10.0, step: 1.0, unit: String::new() });
        let placed = draw_ctl(&mut pm, &mut p, &c, 400.0, 100.0, &Interact::default(), (0.0, 0.0, 500.0, 200.0), 230.0);
        assert_eq!(placed.parts[0], (240.0, 88.0, 160.0, 24.0));
        assert_eq!(placed.rect, (232.0, 88.0, 176.0, 24.0));
    }

    // Use isolated processes because live font preferences are process-wide atomics.
    #[test]
    fn accessible_text_geometry() {
        if std::env::var_os("DECEBAL_UI_GEOMETRY_CHILD").is_none() {
            for scale in ["1", "2"] {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "settings::ui::tests::accessible_text_geometry", "--nocapture"])
                    .env("DECEBAL_UI_GEOMETRY_CHILD", "1")
                    .env("DECEBAL_FONT_SCALE", scale)
                    .status()
                    .unwrap();
                assert!(status.success());
            }
            return;
        }
        let Some(mut fonts) = Fonts::load() else { return };
        let m = TextLayout::live();
        assert_eq!(m.0, std::env::var("DECEBAL_FONT_SCALE").unwrap().parse::<f32>().unwrap());
        let row = Row::new("Readable setting").detail("Explanation with descenders");
        let (label, detail) = m.label_offsets(true);
        // Render each line independently, then compare actual painted ink bounds.
        let mut bounds = |text: &str, px: f32, cy: f32| {
            let mut pm = Pixmap::new(600, 200).unwrap();
            let mut p = Painter { fonts: Some(&mut fonts), s: 1.0 };
            p.text(&mut pm, text, 10.0, cy, px, Weight::Regular, Color::WHITE);
            let ys: Vec<usize> = pm.data().chunks_exact(4).enumerate().filter(|(_, p)| p[3] > 0).map(|(i, _)| i / 600).collect();
            (*ys.iter().min().unwrap() as f32, *ys.iter().max().unwrap() as f32)
        };
        let center = m.row(&row) / 2.0;
        let a = bounds(&row.label, 13.0, center + label);
        let b = bounds(&row.detail, 11.0, center + detail);
        assert!(a.1 < b.0, "label and detail ink overlap: {a:?} {b:?}");
        assert!(a.0 >= 0.0 && b.1 < m.row(&row));
        for (text, px) in [("Apply changes", 12.5), ("− +", 15.0), ("Choose setting", 13.0)] {
            let ink = bounds(text, px, 80.0);
            assert!(ink.0 >= 80.0 - m.field() / 2.0 && ink.1 < 80.0 + m.field() / 2.0, "control clips {text}: {ink:?}");
        }
    }

    #[test]
    fn slider_maps_and_snaps() {
        let track = (100.0, 0.0, 200.0, 10.0);
        assert_eq!(slider_value(100.0, track, 0.0, 10.0, 1.0), 0.0);
        assert_eq!(slider_value(300.0, track, 0.0, 10.0, 1.0), 10.0);
        assert_eq!(slider_value(200.0, track, 0.0, 10.0, 1.0), 5.0);
        assert_eq!(slider_value(-50.0, track, 2.0, 10.0, 1.0), 2.0, "clamped");
        assert_eq!(slider_value(9999.0, track, 2.0, 10.0, 1.0), 10.0);
        let v = slider_value(152.0, track, 0.0, 1.0, 0.05);
        assert!((v * 20.0 - (v * 20.0).round()).abs() < 1e-6, "{v}");
    }

    #[test]
    fn snap_has_no_float_noise() {
        assert_eq!(snap(0.1 + 0.2, 0.0, 1.0, 0.05), 0.3);
        assert_eq!(snap(7.4, 0.0, 10.0, 1.0), 7.0);
        assert_eq!(snap(-1.0, 0.0, 10.0, 1.0), 0.0);
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#ff8000"), Some((255, 128, 0)));
        assert_eq!(parse_hex("ff8000aa"), Some((255, 128, 0)));
        assert_eq!(parse_hex("#12"), None);
        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("#gg0000"), None);
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_num(5.0, 1.0), "5");
        assert_eq!(format_num(0.55, 0.05), "0.55");
        assert_eq!(format_num(1.4, 0.1), "1.4");
    }

    #[test]
    fn frame_focus_order_skips_labels_and_disabled() {
        let mut f = Frame::default();
        let mk =
            |id: &str, kind: Kind, en: bool| Placed { rect: (0.0, 0.0, 1.0, 1.0), ctl: Ctl { id: id.into(), kind, enabled: en }, parts: vec![], row_rect: (0.0, 0.0, 1.0, 1.0), label: String::new() };
        f.placed.push(mk("a", Kind::Toggle(true), true));
        f.placed.push(mk("b", Kind::Label("x".into()), true));
        f.placed.push(mk("c", Kind::Toggle(true), false));
        f.placed.push(mk("d", Kind::Toggle(true), true));
        assert_eq!(f.focus_order(), ["a", "d"]);
    }

    #[test]
    fn compact_rows_keep_controls_inside_cards_at_multiple_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let mut pm = Pixmap::new((480.0 * scale) as u32, (600.0 * scale) as u32).unwrap();
            let mut p = Painter { fonts: None, s: scale };
            let items = vec![Item::Card(vec![
                Row::new("Interface scale").detail("Adjust interface size").ctl(Ctl::new("s", Kind::Slider { v: 1.0, min: 0.5, max: 2.0, step: 0.1, unit: "×".into() })),
                Row::new("Display name").ctl(Ctl::new("n", Kind::Text { val: "Display".into(), hint: String::new() })),
                Row::new("Mode").ctl(Ctl::new("m", Kind::Seg(vec![("First".into(), "a".into()), ("Second".into(), "b".into())], Some(0)))),
                Row::new("Actions").ctl(Ctl::new("a", Kind::Button { label: "Reset".into(), danger: false })).ctl(Ctl::new("b", Kind::Button { label: "Apply".into(), danger: false })),
            ])];
            let sidebar = 164.0 * scale;
            let fr = draw_items(&mut pm, &mut p, Head::new("Settings"), &items, 0.0, sidebar, 316.0 * scale, &Interact::default(), &|_| None);
            assert_eq!(fr.focus_order(), ["s", "n", "m", "a", "b"]);
            for control in &fr.placed {
                assert!(control.rect.0 >= sidebar, "{} escaped the content area", control.ctl.id);
                assert!(control.rect.0 + control.rect.2 <= 480.0 * scale);
                assert!(control.rect.1 >= control.row_rect.1);
                assert!(control.rect.1 + control.rect.3 <= control.row_rect.1 + control.row_rect.3);
                assert!(fr.at(control.rect.0 + control.rect.2 / 2.0, control.rect.1 + control.rect.3 / 2.0).is_some());
            }
        }
    }

    #[test]
    fn navigation_wraps_keeps_every_label_and_stays_one_segmented_control() {
        let opts: Vec<Opt> = ["Themes", "Palettes", "Customise", "All apps", "Window style", "Dock & Bar"].iter().map(|l| (l.to_string(), l.to_lowercase())).collect();
        for secondary in [false, true] {
            for (cw, scale) in [(900.0, 1.0), (300.0, 1.0), (300.0, 2.0), (180.0, 1.0)] {
                let mut pm = Pixmap::new((cw * scale + 120.0) as u32, 600).unwrap();
                let mut p = Painter { fonts: None, s: scale };
                let items = vec![Item::Nav { id: "nav".into(), opts: opts.clone(), sel: Some(2), secondary }];
                let width = cw * scale;
                let fr = draw_items(&mut pm, &mut p, Head::new("T"), &items, 0.0, 60.0, width + 64.0 * scale, &Interact::default(), &|_| None);
                let nav = fr.find("nav").expect("one control");
                assert!(matches!(&nav.ctl.kind, Kind::Seg(o, Some(2)) if *o == opts));
                assert_eq!(nav.parts.len(), opts.len());
                assert_eq!(fr.focus_order(), ["nav"]);
                for (i, a) in nav.parts.iter().enumerate() {
                    assert!(a.0 >= nav.rect.0 - 0.5 && a.0 + a.2 <= nav.rect.0 + nav.rect.2 + 0.5, "part {i} escapes the strip (cw {cw}, {secondary})");
                    for b in &nav.parts[i + 1..] {
                        let overlap = a.0 < b.0 + b.2 - 0.5 && b.0 < a.0 + a.2 - 0.5 && a.1 < b.1 + b.3 - 0.5 && b.1 < a.1 + a.3 - 0.5;
                        assert!(!overlap, "parts overlap (cw {cw}, {secondary})");
                    }
                    assert!(fr.at(a.0 + a.2 / 2.0, a.1 + a.3 / 2.0).is_some_and(|p| p.ctl.id == "nav"));
                }
                if cw < 400.0 {
                    assert!(nav.parts.iter().any(|r| r.1 > nav.parts[0].1), "narrow strips wrap instead of truncating (cw {cw})");
                }
            }
        }
    }

    #[test]
    fn long_details_wrap_and_grow_their_row_instead_of_ending_in_an_ellipsis() {
        let mut pm = Pixmap::new(700, 700).unwrap();
        let mut p = Painter { fonts: None, s: 1.0 };
        let detail = "A rather long explanation of what this setting does that cannot possibly fit beside its control on one line of the card";
        let short = vec![Item::Card(vec![Row::new("Short").detail("fits").ctl(Ctl::new("t", Kind::Toggle(true)))])];
        let long = vec![Item::Card(vec![Row::new("Long").detail(detail).ctl(Ctl::new("t", Kind::Toggle(true)))])];
        let a = draw_items(&mut pm, &mut p, Head::new("T"), &short, 0.0, 0.0, 440.0, &Interact::default(), &|_| None);
        let b = draw_items(&mut pm, &mut p, Head::new("T"), &long, 0.0, 0.0, 440.0, &Interact::default(), &|_| None);
        assert!(b.height > a.height + 10.0, "{} vs {}", b.height, a.height);
        let t = b.find("t").unwrap();
        assert!(t.rect.1 >= t.row_rect.1 && t.rect.1 + t.rect.3 <= t.row_rect.1 + t.row_rect.3);
    }

    #[test]
    fn long_titles_wrap_and_never_lose_characters() {
        let p = Painter { fonts: None, s: 1.0 };
        for max in [400.0, 120.0, 60.0] {
            let lines = wrap_title(&p, "Appearance and Windows", max, 25.0);
            assert_eq!(lines.concat().replace(' ', ""), "AppearanceandWindows");
            assert!(lines.iter().all(|l| !l.is_empty()));
        }
        assert_eq!(wrap_title(&p, "Sound", 400.0, 25.0), ["Sound"]);
        assert_eq!(wrap_title(&p, "", 400.0, 25.0), [""]);
    }

    #[test]
    fn page_header_context_wraps_below_the_title() {
        let mut pm = Pixmap::new(700, 500).unwrap();
        let mut p = Painter { fonts: None, s: 1.0 };
        let items = vec![Item::Card(vec![Row::new("A").ctl(Ctl::new("t", Kind::Toggle(true)))])];
        let plain = draw_items(&mut pm, &mut p, Head::new("Title"), &items, 0.0, 0.0, 700.0, &Interact::default(), &|_| None);
        let ctx = Head { title: "Title", context: "One quiet line of orientation that is long enough to need wrapping when the page is narrow, so the next control moves down" };
        let with = draw_items(&mut pm, &mut p, ctx, &items, 0.0, 0.0, 300.0, &Interact::default(), &|_| None);
        assert!(with.find("t").unwrap().rect.1 > plain.find("t").unwrap().rect.1 + 20.0);
    }

    #[test]
    fn items_draw_and_record_hits() {
        let mut pm = Pixmap::new(700, 600).unwrap();
        let mut p = Painter { fonts: None, s: 1.0 };
        let items = vec![Item::Card(vec![
            Row::new("A").ctl(Ctl::new("t", Kind::Toggle(true))),
            Row::new("B").detail("d").ctl(Ctl::new("s", Kind::Slider { v: 3.0, min: 0.0, max: 10.0, step: 1.0, unit: "px".into() })),
        ])];
        let fr = draw_items(&mut pm, &mut p, Head::new("T"), &items, 0.0, 0.0, 700.0, &Interact::default(), &|_| None);
        assert_eq!(fr.placed.len(), 2);
        let t = fr.find("t").unwrap();
        assert!(fr.at(t.rect.0 + 3.0, t.rect.1 + 3.0).is_some_and(|p| p.ctl.id == "t"));
        assert!(fr.height > 100.0);
        assert!(pm.data().chunks_exact(4).any(|px| px[3] > 0));
    }
}
