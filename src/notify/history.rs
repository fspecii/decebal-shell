//! The history panel, independent of the display server: its layout, focus ring, scrolling, keyboard and click
//! behaviour, and painting. A front end owns the surface, feeds native input to [`HistWin`] and redraws or closes
//! the panel as the returned [`HistOutcome`] says.

use super::model::{ago, unix_now, Config, HistoryItem};
use super::view::{accent_text, draw_icon, ellipsize, ellipsize_role, line_height, Metrics, Palette, Render, HIST_MAX_ROWS, HIST_ROW, HIST_W, R};
use super::Shared;
use crate::gfx::{self, color};
use crate::text::{Fonts, Role, Weight};
use std::collections::HashMap;
use tiny_skia::Pixmap;

// X11 keysym numbers (Wayland keysyms are the same numbers).
pub const KEY_ESCAPE: u32 = 0xff1b;
pub const KEY_TAB: u32 = 0xff09;
pub const KEY_ISO_LEFT_TAB: u32 = 0xfe20;
pub const KEY_RETURN: u32 = 0xff0d;
pub const KEY_KP_ENTER: u32 = 0xff8d;
pub const KEY_SPACE: u32 = 0x20;
pub const KEY_DELETE: u32 = 0xffff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistFocus {
    Clear,
    Dnd,
    Retry,
    Item(u32),
}

impl HistFocus {
    pub fn id(self) -> String {
        match self {
            Self::Clear => "clear".into(),
            Self::Dnd => "dnd".into(),
            Self::Retry => "retry".into(),
            Self::Item(id) => format!("item:{id}"),
        }
    }
}

/// What the front end does after a history input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistOutcome {
    /// Nothing changed.
    Idle,
    /// Paint the panel again.
    Redraw,
    /// Close the panel (the keyboard goes back to whoever had it).
    Close,
}

#[derive(Debug, Clone, Copy)]
struct ErrorLayout {
    card: R,
    retry: R,
    title_y: f32,
    reason_y: f32,
    help_y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct HistoryLayout {
    clear: R,
    dnd: R,
    error: Option<ErrorLayout>,
    title_y: f32,
    header: f32,
    row: f32,
    title_line: f32,
    body_line: f32,
}

impl HistoryLayout {
    pub fn new(fonts: &Fonts, s: f32, w: f32, error: bool) -> Self {
        let clear_h = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s).max(14.0 * s) + 8.0 * s;
        let clear_w = (fonts.measure("Clear", 12.0 * s, Weight::Medium) + 16.0 * s).min((w - 28.0 * s).max(1.0));
        let title_h = line_height(fonts, Role::Title, 16.0 * s, Weight::Bold, s).max(clear_h);
        let clear = R { x: (w - clear_w - 14.0 * s).max(0.0), y: 11.0 * s, w: clear_w, h: clear_h };
        let dnd_h = (line_height(fonts, Role::Ui, 12.5 * s, Weight::Medium, s) + 10.0 * s).max(30.0 * s);
        let dnd = R { x: 12.0 * s, y: 13.0 * s + title_h + 9.0 * s, w: (w - 24.0 * s).max(1.0), h: dnd_h };
        let mut header = dnd.y + dnd.h + 8.0 * s;
        let error = error.then(|| {
            let title_lh = line_height(fonts, Role::Ui, 12.0 * s, Weight::SemiBold, s);
            let reason_lh = line_height(fonts, Role::Ui, 11.0 * s, Weight::Regular, s);
            let help_lh = line_height(fonts, Role::Ui, 10.0 * s, Weight::Regular, s);
            let retry_h = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s) + 10.0 * s;
            let retry_w = (fonts.measure("Retry", 12.0 * s, Weight::Medium) + 24.0 * s).min((w - 44.0 * s).max(1.0));
            let title_y = header + 6.0 * s;
            let reason_y = title_y + title_lh + 3.0 * s;
            let retry = R { x: (w - 22.0 * s - retry_w).max(0.0), y: reason_y, w: retry_w, h: retry_h };
            let help_y = reason_y + reason_lh.max(retry_h) + 3.0 * s;
            let card = R { x: 12.0 * s, y: header, w: (w - 24.0 * s).max(1.0), h: help_y + help_lh + 6.0 * s - header };
            header = card.y + card.h + 6.0 * s;
            ErrorLayout { card, retry, title_y, reason_y, help_y }
        });
        let title_line = line_height(fonts, Role::Ui, 12.5 * s, Weight::SemiBold, s);
        let body_line = line_height(fonts, Role::Ui, 12.0 * s, Weight::Regular, s);
        let app_line = line_height(fonts, Role::Ui, 10.5 * s, Weight::Regular, s);
        let row = (16.0 * s + title_line + body_line + app_line).max(HIST_ROW * s);
        Self { clear, dnd, error, title_y: 13.0 * s, header, row, title_line, body_line }
    }

    pub fn row_rect(self, i: usize, w: f32) -> R {
        R { x: 0.0, y: self.header + i as f32 * self.row, w, h: self.row }
    }
}

pub fn shifted(mut r: R, pan: f32) -> R {
    r.y -= pan;
    r
}

/// How big the panel is for the current history, theme and area.
pub struct HistDims {
    pub w: u32,
    pub h: u32,
    pub rows: usize,
    layout: HistoryLayout,
    pub content_h: f32,
}

/// `fit` snaps a size to what the front end's surfaces can be (identity on X11).
pub fn history_dimensions(fonts: &Fonts, m: &Metrics, sh: &Shared, fit: &dyn Fn(u32, u32) -> (u32, u32)) -> HistDims {
    let s = m.s;
    let w = fit(m.surface_width(HIST_W).round().max(1.0) as u32, 1).0;
    let layout = HistoryLayout::new(fonts, s, w as f32, sh.history.error().is_some());
    let available = m.available_height();
    let rows = ((available - layout.header - 8.0 * s) / layout.row).floor().clamp(1.0, HIST_MAX_ROWS as f32) as usize;
    let count = sh.history.items.len().clamp(1, rows);
    let content = layout.header + count as f32 * layout.row + 8.0 * s;
    let (w, h) = fit(w, content.ceil().min(available).max(1.0) as u32);
    HistDims { w, h, rows, layout, content_h: content }
}

pub struct HistWin<W> {
    pub win: W,
    pub w: u32,
    pub h: u32,
    pub scroll: usize,
    pub rows: usize,
    pub x: i32,
    pub y: i32,
    pub hover: Option<usize>,
    pub focus: Option<HistFocus>,
    pub ring: bool,
    pub pan: f32,
    pub content_h: f32,
    layout: HistoryLayout,
}

impl<W> HistWin<W> {
    pub fn new(win: W, dims: HistDims, x: i32, y: i32) -> HistWin<W> {
        HistWin { win, w: dims.w, h: dims.h, scroll: 0, rows: dims.rows, x, y, hover: None, focus: None, ring: false, pan: 0.0, content_h: dims.content_h, layout: dims.layout }
    }

    /// Takes new dimensions (history, theme, font, scale or area changed) keeping scroll, focus and pan inside
    /// their new bounds. Returns whether the size or position changed (the surface has to follow).
    pub fn relayout(&mut self, dims: HistDims, x: i32, y: i32, sh: &Shared, s: f32) -> bool {
        self.rows = dims.rows;
        self.scroll = self.scroll.min(sh.history.items.len().saturating_sub(dims.rows));
        self.layout = dims.layout;
        self.content_h = dims.content_h;
        self.pan = self.pan.clamp(0.0, (dims.content_h - dims.h as f32).max(0.0));
        if self.focus.is_some_and(|focus| !history_order(sh).contains(&focus)) {
            self.focus = Some(HistFocus::Dnd);
        }
        let changed = (self.w, self.h, self.x, self.y) != (dims.w, dims.h, x, y);
        if changed {
            (self.w, self.h, self.x, self.y) = (dims.w, dims.h, x, y);
        }
        if self.ring {
            reveal_history_focus(self, sh, s);
        }
        changed
    }

    /// Whether (`x`, `y`) in panel pixels is on the panel.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= 0.0 && y >= 0.0 && x < self.w as f32 && y < self.h as f32
    }

    /// The control or row under (`x`, `y`).
    pub fn target_at(&self, x: f32, y: f32, sh: &Shared) -> Option<HistFocus> {
        let (clear, dnd, rows) = hist_rects(self, sh);
        if self.layout.error.is_some_and(|layout| shifted(layout.retry, self.pan).contains(x, y)) {
            Some(HistFocus::Retry)
        } else if clear.contains(x, y) {
            Some(HistFocus::Clear)
        } else if dnd.contains(x, y) {
            Some(HistFocus::Dnd)
        } else {
            rows.iter().position(|r| r.contains(x, y)).and_then(|i| sh.history.items.get(self.scroll + i)).map(|item| HistFocus::Item(item.id))
        }
    }

    /// A left click at (`x`, `y`) on the panel. Removing a row only edits the history: it never replays an action.
    pub fn click(&mut self, x: f32, y: f32, sh: &mut Shared) -> HistOutcome {
        match self.target_at(x, y, sh) {
            Some(target) => {
                self.focus = Some(target);
                self.ring = false;
                self.activate(target, sh)
            }
            None => HistOutcome::Idle,
        }
    }

    /// One wheel step: the panel pans first (when its content is taller than the surface), then the list scrolls a row.
    pub fn scroll_step(&mut self, up: bool, sh: &Shared, s: f32) -> HistOutcome {
        let max_pan = (self.content_h - self.h as f32).max(0.0);
        let old_pan = self.pan;
        self.pan = (self.pan + if up { -30.0 * s } else { 30.0 * s }).clamp(0.0, max_pan);
        if (self.pan - old_pan).abs() < 0.1 {
            let max = sh.history.items.len().saturating_sub(self.rows);
            self.scroll = if up { self.scroll.saturating_sub(1) } else { (self.scroll + 1).min(max) };
        }
        self.ring = false;
        HistOutcome::Redraw
    }

    /// Moves the hover highlight; true when it changed.
    pub fn hover_at(&mut self, x: f32, y: f32, sh: &Shared) -> bool {
        let (_, _, rows) = hist_rects(self, sh);
        let hover = rows.iter().position(|r| r.contains(x, y));
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    /// The pointer left the panel.
    pub fn clear_hover(&mut self) -> bool {
        self.hover.take().is_some()
    }

    pub fn activate(&mut self, target: HistFocus, sh: &mut Shared) -> HistOutcome {
        match target {
            HistFocus::Retry => {
                let _ = sh.history.retry();
            }
            HistFocus::Clear => {
                let _ = sh.history.clear();
                self.scroll = 0;
            }
            HistFocus::Dnd => {
                let on = !sh.cfg.dnd;
                if Config::save_dnd(on).is_ok() {
                    sh.cfg.dnd = on;
                }
            }
            HistFocus::Item(id) => {
                let index = sh.history.items.iter().position(|item| item.id == id);
                sh.history.remove(id);
                self.scroll = self.scroll.min(sh.history.items.len().saturating_sub(self.rows));
                self.focus = index.and_then(|i| sh.history.items.get(i.min(sh.history.items.len().saturating_sub(1)))).map(|item| HistFocus::Item(item.id)).or(Some(HistFocus::Dnd));
            }
        }
        HistOutcome::Redraw
    }

    /// A key press (X11 keysym numbers). Escape closes; Tab / Shift+Tab walk Clear, Do Not Disturb, Retry (when
    /// storage failed) and the rows; Enter or Space activate the focused control; Delete removes the focused row.
    pub fn key(&mut self, sym: u32, shift: bool, sh: &mut Shared, s: f32) -> HistOutcome {
        match sym {
            KEY_ESCAPE => HistOutcome::Close,
            KEY_TAB | KEY_ISO_LEFT_TAB => {
                let order = history_order(sh);
                let backwards = shift || sym == KEY_ISO_LEFT_TAB;
                let index = self.focus.and_then(|focus| order.iter().position(|target| *target == focus));
                let next = match (index, backwards) {
                    (Some(0) | None, true) => order.len() - 1,
                    (Some(i), true) => i - 1,
                    (Some(i), false) => (i + 1) % order.len(),
                    (None, false) => 0,
                };
                self.focus = Some(order[next]);
                self.ring = true;
                if let HistFocus::Item(id) = order[next] {
                    if let Some(index) = sh.history.items.iter().position(|item| item.id == id) {
                        if index < self.scroll {
                            self.scroll = index;
                        }
                        if index >= self.scroll + self.rows {
                            self.scroll = index + 1 - self.rows;
                        }
                    }
                }
                reveal_history_focus(self, sh, s);
                HistOutcome::Redraw
            }
            KEY_RETURN | KEY_KP_ENTER | KEY_SPACE => match self.focus {
                Some(target) => self.activate(target, sh),
                None => HistOutcome::Idle,
            },
            KEY_DELETE => match self.focus {
                Some(target @ HistFocus::Item(_)) => self.activate(target, sh),
                _ => HistOutcome::Idle,
            },
            _ => HistOutcome::Idle,
        }
    }

    /// Opt-in geometry/state evidence (`DECNOTIFY_DEBUG_HISTORY`), without artwork.
    pub fn debug(&self, sh: &Shared, window: u64) {
        let Some(path) = std::env::var_os("DECNOTIFY_DEBUG_HISTORY") else { return };
        let (clear, dnd, rows) = hist_rects(self, sh);
        let rect = |r: R| [r.x, r.y, r.w, r.h];
        let items: Vec<_> = sh.history.items.iter().map(|i| serde_json::json!({"id":i.id,"summary":i.summary})).collect();
        let data = serde_json::json!({"window":window,"origin":[self.x,self.y],"error":sh.history.error(),"pending":sh.history.pending(),"clear_pending":sh.history.clear_pending(),"items":items,"retry_rect":self.layout.error.map(|e|rect(shifted(e.retry,self.pan))),"clear_rect":rect(clear),"dnd_rect":rect(dnd),"row_rect_count":rows.len(),"row_rects":rows.into_iter().map(rect).collect::<Vec<_>>(),"size":[self.w,self.h],"scroll":self.scroll,"pan":self.pan,"focus":self.focus.map(HistFocus::id),"font_scale":crate::theme::live::font_scale()});
        let _ = std::fs::write(path, data.to_string());
    }
}

impl Render {
    /// Makes sure the icons of the history rows that will be drawn are cached.
    pub fn prepare_history(&mut self, sh: &Shared, scroll: usize, capacity: usize) {
        let size = (32.0 * self.s).round() as u32;
        let items: Vec<HistoryItem> = sh.history.items.iter().skip(scroll).take(capacity).cloned().collect();
        for it in &items {
            self.icon_cached(&it.icon, &it.app, &it.entry, size);
        }
    }

    pub fn paint_history<W>(&mut self, sh: &Shared, hw: &HistWin<W>) -> Pixmap {
        paint_history(&mut self.fonts, &self.pal, self.s, sh, Some(hw), &self.icons)
    }
}

pub fn paint_history<W>(fonts: &mut Fonts, pal: &Palette, s: f32, sh: &Shared, hw: Option<&HistWin<W>>, icons: &HashMap<String, Option<Pixmap>>) -> Pixmap {
    let (w, h) = hw.map_or((1, 1), |h| (h.w, h.h));
    let mut pm = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"));
    gfx::fill_rounded(&mut pm, 0.0, 0.0, w as f32, h as f32, 16.0 * s, pal.bg_color());
    gfx::stroke_rounded(&mut pm, 0.5, 0.5, w as f32 - 1.0, h as f32 - 1.0, 16.0 * s, pal.border, 1.0);
    let Some(hw) = hw else { return pm };
    let layout = hw.layout;
    let pan = hw.pan;
    let (clear, dnd, rows) = hist_rects(hw, sh);
    let title = ellipsize_role(fonts, Role::Title, "Notifications", 16.0 * s, Weight::Bold, (clear.x - 24.0 * s).max(1.0));
    fonts.draw_as(Role::Title, &mut pm, &title, 16.0 * s, layout.title_y - pan, 16.0 * s, Weight::Bold, pal.title);
    let clear_line = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s);
    fonts.draw(&mut pm, "Clear", clear.x + 8.0 * s, clear.y + (clear.h - clear_line) / 2.0, 12.0 * s, Weight::Medium, pal.link);
    let (ar, ag, ab) = pal.accent;
    gfx::fill_rounded(&mut pm, dnd.x, dnd.y, dnd.w, dnd.h, 8.0 * s, pal.button);
    let dnd_line = line_height(fonts, Role::Ui, 12.5 * s, Weight::Medium, s);
    let label = ellipsize(fonts, "Do Not Disturb", 12.5 * s, Weight::Medium, (dnd.w - 64.0 * s).max(1.0));
    fonts.draw(&mut pm, &label, dnd.x + 10.0 * s, dnd.y + (dnd.h - dnd_line) / 2.0, 12.5 * s, Weight::Medium, pal.title);
    let (sw, sh_) = (34.0 * s, 20.0 * s);
    let (sx, sy) = (dnd.x + dnd.w - sw - 10.0 * s, dnd.y + (dnd.h - sh_) / 2.0);
    let track = if sh.cfg.dnd { color(ar, ag, ab, 1.0) } else { pal.button_hover };
    gfx::fill_rounded(&mut pm, sx, sy, sw, sh_, sh_ / 2.0, track);
    let knob_x = if sh.cfg.dnd { sx + sw - sh_ + 2.0 * s } else { sx + 2.0 * s };
    gfx::fill_circle(&mut pm, knob_x + (sh_ - 4.0 * s) / 2.0, sy + sh_ / 2.0, (sh_ - 4.0 * s) / 2.0, if sh.cfg.dnd { accent_text(pal) } else { pal.title });

    if let (Some(error), Some(el)) = (sh.history.error(), layout.error) {
        let card = shifted(el.card, pan);
        let retry = shifted(el.retry, pan);
        gfx::fill_rounded(&mut pm, card.x, card.y, card.w, card.h, 8.0 * s, pal.button);
        let title = if sh.history.clear_pending() {
            "Clear is not saved"
        } else if sh.history.pending() {
            "History is not saved"
        } else {
            "History is unavailable"
        };
        fonts.draw(&mut pm, &ellipsize(fonts, title, 12.0 * s, Weight::SemiBold, (card.w - 20.0 * s).max(1.0)), 22.0 * s, el.title_y - pan, 12.0 * s, Weight::SemiBold, pal.title);
        let reason = ellipsize(fonts, error, 11.0 * s, Weight::Regular, (retry.x - 30.0 * s).max(1.0));
        fonts.draw(&mut pm, &reason, 22.0 * s, el.reason_y - pan, 11.0 * s, Weight::Regular, pal.body);
        let help = if sh.history.pending() { "Unsaved changes remain here for this session" } else { "History file preserved; repair the problem and Retry" };
        fonts.draw(&mut pm, &ellipsize(fonts, help, 10.0 * s, Weight::Regular, (card.w - 20.0 * s).max(1.0)), 22.0 * s, el.help_y - pan, 10.0 * s, Weight::Regular, pal.body);
        gfx::fill_rounded(&mut pm, retry.x, retry.y, retry.w, retry.h, 6.0 * s, color(ar, ag, ab, 1.0));
        let retry_line = line_height(fonts, Role::Ui, 12.0 * s, Weight::Medium, s);
        let label = ellipsize(fonts, "Retry", 12.0 * s, Weight::Medium, (retry.w - 12.0 * s).max(1.0));
        let width = fonts.measure(&label, 12.0 * s, Weight::Medium);
        fonts.draw(&mut pm, &label, retry.x + (retry.w - width) / 2.0, retry.y + (retry.h - retry_line) / 2.0, 12.0 * s, Weight::Medium, accent_text(pal));
    }

    if sh.history.items.is_empty() {
        let msg = ellipsize(fonts, "No Notifications", 13.0 * s, Weight::Medium, (w as f32 - 24.0 * s).max(1.0));
        let width = fonts.measure(&msg, 13.0 * s, Weight::Medium);
        fonts.draw(&mut pm, &msg, (w as f32 - width) / 2.0, layout.header + 16.0 * s - pan, 13.0 * s, Weight::Medium, pal.dim);
    } else {
        let now = unix_now();
        for (i, r) in rows.iter().enumerate() {
            let item = &sh.history.items[hw.scroll + i];
            if hw.hover == Some(i) {
                gfx::fill_rounded(&mut pm, 8.0 * s, r.y, w as f32 - 16.0 * s, r.h - 4.0 * s, 10.0 * s, pal.button);
            }
            let size = (32.0 * s).round() as u32;
            let icon = icons.get(&format!("{}|{}|{}|{}", item.icon, item.app, item.entry, size)).and_then(Option::as_ref);
            draw_icon(fonts, &mut pm, icon, &item.app, 18.0 * s, r.y + 11.0 * s, 32.0 * s);
            let tx = 62.0 * s;
            let when = ago(item.time, now);
            let tw = fonts.measure(&when, 11.0 * s, Weight::Regular);
            fonts.draw(&mut pm, &when, w as f32 - 18.0 * s - tw, r.y + 8.0 * s, 11.0 * s, Weight::Regular, pal.dim);
            let title = ellipsize(fonts, &item.summary, 12.5 * s, Weight::SemiBold, (w as f32 - tx - tw - 30.0 * s).max(1.0));
            fonts.draw(&mut pm, &title, tx, r.y + 8.0 * s, 12.5 * s, Weight::SemiBold, pal.title);
            let body = ellipsize(fonts, item.body.lines().next().unwrap_or(""), 12.0 * s, Weight::Regular, (w as f32 - tx - 18.0 * s).max(1.0));
            fonts.draw(&mut pm, &body, tx, r.y + 8.0 * s + layout.title_line, 12.0 * s, Weight::Regular, pal.body);
            let app = ellipsize(fonts, &item.app, 10.5 * s, Weight::Regular, (w as f32 - tx - 18.0 * s).max(1.0));
            fonts.draw(&mut pm, &app, tx, r.y + 8.0 * s + layout.title_line + layout.body_line, 10.5 * s, Weight::Regular, pal.dim);
        }
    }
    if hw.ring {
        if let Some(r) = history_focus_rect(hw, sh) {
            let row = matches!(hw.focus, Some(HistFocus::Item(_)));
            let inset = if row { 6.0 * s } else { -3.0 * s };
            let y_inset = if row { 1.0 * s } else { -3.0 * s };
            gfx::stroke_rounded(&mut pm, r.x + inset, r.y + y_inset, (r.w - inset * 2.0).max(1.0), (r.h - y_inset * 2.0).max(1.0), 7.0 * s, pal.link, 2.0 * s);
        }
    }
    if hw.content_h > h as f32 {
        let thumb = ((h as f32 / hw.content_h) * h as f32).max(10.0 * s);
        let y = (h as f32 - thumb) * hw.pan / (hw.content_h - h as f32).max(1.0);
        gfx::fill_rounded(&mut pm, (w as f32 - 4.0 * s).max(0.0), y, 3.0 * s, thumb, 1.5 * s, pal.dim);
    }
    pm
}

pub fn history_order(sh: &Shared) -> Vec<HistFocus> {
    let mut order = vec![HistFocus::Clear, HistFocus::Dnd];
    if sh.history.error().is_some() {
        order.push(HistFocus::Retry);
    }
    order.extend(sh.history.items.iter().map(|item| HistFocus::Item(item.id)));
    order
}

pub fn hist_rects<W>(hw: &HistWin<W>, sh: &Shared) -> (R, R, Vec<R>) {
    let n = sh.history.items.len().saturating_sub(hw.scroll).min(hw.rows);
    (shifted(hw.layout.clear, hw.pan), shifted(hw.layout.dnd, hw.pan), (0..n).map(|i| shifted(hw.layout.row_rect(i, hw.w as f32), hw.pan)).collect())
}

pub fn reveal_history_focus<W>(hw: &mut HistWin<W>, sh: &Shared, s: f32) {
    if let Some(HistFocus::Item(id)) = hw.focus {
        if let Some(index) = sh.history.items.iter().position(|item| item.id == id) {
            if index < hw.scroll {
                hw.scroll = index;
            }
            if index >= hw.scroll + hw.rows {
                hw.scroll = index + 1 - hw.rows;
            }
        }
    }
    if let Some(r) = history_focus_rect(hw, sh) {
        let margin = (4.0 * s).min(hw.h as f32 / 4.0);
        if r.y < margin {
            hw.pan = (hw.pan + r.y - margin).max(0.0);
        }
        if r.y + r.h > hw.h as f32 - margin {
            hw.pan = (hw.pan + r.y + r.h - hw.h as f32 + margin).min((hw.content_h - hw.h as f32).max(0.0));
        }
    }
}

pub fn history_focus_rect<W>(hw: &HistWin<W>, sh: &Shared) -> Option<R> {
    let r = match hw.focus? {
        HistFocus::Clear => hw.layout.clear,
        HistFocus::Dnd => hw.layout.dnd,
        HistFocus::Retry => hw.layout.error?.retry,
        HistFocus::Item(id) => {
            let i = sh.history.items.iter().position(|item| item.id == id)?;
            if i < hw.scroll || i >= hw.scroll + hw.rows {
                return None;
            }
            hw.layout.row_rect(i - hw.scroll, hw.w as f32)
        }
    };
    Some(shifted(r, hw.pan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::model::History;
    use tiny_skia::Color;

    fn item(id: u32) -> HistoryItem {
        HistoryItem { id, app: "Fixture".into(), icon: String::new(), summary: format!("Item {id}"), body: "Body".into(), entry: String::new(), time: 0 }
    }

    fn shared(count: u32) -> Shared {
        let mut sh = Shared { cfg: Config::default(), history: History::in_memory() };
        for id in 1..=count {
            sh.history.push(item(id));
        }
        sh
    }

    fn panel(fonts: &Fonts, rows: usize, tall: bool) -> HistWin<u32> {
        let layout = HistoryLayout::new(fonts, 1.0, 296.0, false);
        let content_h = layout.header + rows as f32 * layout.row + 8.0;
        HistWin { win: 0, w: 296, h: if tall { content_h.ceil() as u32 } else { 134 }, scroll: 0, rows, x: 0, y: 0, hover: None, focus: None, ring: false, pan: 0.0, content_h, layout }
    }

    #[test]
    fn measured_history_metrics_keep_lines_and_controls_separate() {
        let fonts = Fonts::load().expect("test fonts");
        for (scale, width) in [(1.0, 296.0), (1.0, 380.0), (2.0, 600.0)] {
            let m = HistoryLayout::new(&fonts, scale, width, true);
            assert!(m.dnd.y >= m.clear.y + m.clear.h);
            assert!(m.clear.x >= 0.0 && m.clear.x + m.clear.w <= width);
            let e = m.error.unwrap();
            assert!(e.card.y >= m.dnd.y + m.dnd.h);
            assert!(e.reason_y >= e.title_y + line_height(&fonts, Role::Ui, 12.0 * scale, Weight::SemiBold, scale));
            assert!(e.help_y >= e.retry.y + e.retry.h);
            assert!(e.retry.x >= 0.0 && e.retry.x + e.retry.w <= width);
            assert!(e.retry.h >= line_height(&fonts, Role::Ui, 12.0 * scale, Weight::Medium, scale));
            assert!(m.header >= e.card.y + e.card.h);
            let row = m.row_rect(0, width);
            assert!(row.h >= 16.0 * scale + m.title_line + m.body_line + line_height(&fonts, Role::Ui, 10.5 * scale, Weight::Regular, scale));
            assert!(m.row_rect(1, width).y >= row.y + row.h);
        }
    }

    #[test]
    fn history_focus_uses_notification_ids_and_matches_visible_hit_rows() {
        let fonts = Fonts::load().expect("test fonts");
        let mut sh = shared(5);
        assert_eq!(history_order(&sh), vec![HistFocus::Clear, HistFocus::Dnd, HistFocus::Item(5), HistFocus::Item(4), HistFocus::Item(3), HistFocus::Item(2), HistFocus::Item(1)]);
        let hw = HistWin {
            win: 0,
            w: 296,
            h: 300,
            scroll: 3,
            rows: 2,
            x: 0,
            y: 0,
            hover: None,
            focus: Some(HistFocus::Item(2)),
            ring: true,
            pan: 10.0,
            content_h: 310.0,
            layout: HistoryLayout::new(&fonts, 1.0, 296.0, false),
        };
        let (_, _, rows) = hist_rects(&hw, &sh);
        assert_eq!(history_focus_rect(&hw, &sh), Some(rows[0]));
        assert!(rows[0].contains(rows[0].x + rows[0].w / 2.0, rows[0].y + rows[0].h / 2.0));
        sh.history.remove(2);
        assert!(!history_order(&sh).contains(&HistFocus::Item(2)));
        assert!(history_focus_rect(&hw, &sh).is_none(), "a removed ID cannot target the row that replaces it");
        let mut hw = hw;
        hw.focus = Some(HistFocus::Item(1));
        hw.scroll = 0;
        hw.h = 134;
        hw.content_h = hw.layout.header + hw.rows as f32 * hw.layout.row + 8.0;
        reveal_history_focus(&mut hw, &sh, 1.0);
        let r = history_focus_rect(&hw, &sh).unwrap();
        assert!(r.y >= 4.0 && r.y + r.h <= hw.h as f32 - 4.0, "focused row and ring remain in the viewport: {r:?}");
    }

    #[test]
    fn resolved_dynamic_and_accessibility_palettes_drive_notification_paint() {
        use crate::theme::model::{Dynamic, Rgb, Selection, Surfaces};
        let mut t = crate::theme::builtin::def("macos").unwrap().resolve("macos", true, &Selection::default());
        let before = Palette::from_theme(&t);
        let d = Dynamic { enabled: true, surfaces: Surfaces::Full, strength: 1., colors: crate::theme::extract::Extracted::from_color(Rgb(180, 20, 90)), ..Default::default() };
        t = crate::theme::dynamic::apply_to(t, &d, false);
        let after = Palette::from_theme(&t);
        assert_ne!(before, after);
        assert_eq!(after.bg, t.popup.t());
        assert_eq!(after.accent, t.accent.t());
        let accessible = crate::accessibility::Preferences { high_contrast: true, reduce_motion: true, ..Default::default() }.apply(t);
        let p = Palette::from_theme(&accessible);
        assert_eq!(p.bg, (0, 0, 0));
        assert_eq!(p.title, Color::WHITE);
        assert_eq!(p.accent, (255, 255, 0));
        // Native history renderer uses these exact colours, without reading files.
        let mut fonts = Fonts::load().expect("test fonts");
        let mut cfg = Config::default();
        cfg.dnd = true;
        let sh = Shared { cfg, history: History::default() };
        let hist = HistWin {
            win: 0,
            w: 380,
            h: 160,
            rows: 1,
            scroll: 0,
            x: 0,
            y: 0,
            hover: None,
            focus: None,
            ring: false,
            pan: 0.0,
            content_h: 160.0,
            layout: HistoryLayout::new(&fonts, 1.0, 380.0, false),
        };
        let pm = paint_history(&mut fonts, &p, 1., &sh, Some(&hist), &HashMap::new());
        let pixel = pm.pixel(190, 125).unwrap();
        assert_eq!((pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()), (0, 0, 0, 255));
        let thumb = pm.pixel(348, 59).unwrap();
        assert_eq!((thumb.red(), thumb.green(), thumb.blue()), (0, 0, 0), "yellow high-contrast switch needs a dark thumb");
    }

    #[test]
    fn tab_and_shift_tab_walk_controls_then_rows_and_wrap() {
        let fonts = Fonts::load().expect("test fonts");
        let mut sh = shared(3);
        let mut hw = panel(&fonts, 3, true);
        let mut seen = Vec::new();
        for _ in 0..6 {
            assert_eq!(hw.key(KEY_TAB, false, &mut sh, 1.0), HistOutcome::Redraw);
            seen.push(hw.focus.unwrap());
        }
        assert_eq!(seen, vec![HistFocus::Clear, HistFocus::Dnd, HistFocus::Item(3), HistFocus::Item(2), HistFocus::Item(1), HistFocus::Clear], "Tab wraps from the last row to Clear");
        assert!(hw.ring, "keyboard focus shows the ring");
        // Shift+Tab (also as the ISO_Left_Tab keysym) goes the other way and wraps from the first control
        assert_eq!(hw.key(KEY_TAB, true, &mut sh, 1.0), HistOutcome::Redraw);
        assert_eq!(hw.focus, Some(HistFocus::Item(1)));
        hw.key(KEY_ISO_LEFT_TAB, false, &mut sh, 1.0);
        assert_eq!(hw.focus, Some(HistFocus::Item(2)));
        hw.focus = None;
        hw.key(KEY_TAB, true, &mut sh, 1.0);
        assert_eq!(hw.focus, Some(HistFocus::Item(1)), "Shift+Tab with no focus starts at the end");
    }

    #[test]
    fn escape_closes_and_unrelated_keys_do_nothing() {
        let fonts = Fonts::load().expect("test fonts");
        let mut sh = shared(2);
        let mut hw = panel(&fonts, 2, true);
        assert_eq!(hw.key(KEY_ESCAPE, false, &mut sh, 1.0), HistOutcome::Close);
        for sym in [0x61, 0xff51, 0xff52, 0xff0a] {
            assert_eq!(hw.key(sym, false, &mut sh, 1.0), HistOutcome::Idle, "{sym:#x}");
        }
        assert_eq!(hw.key(KEY_RETURN, false, &mut sh, 1.0), HistOutcome::Idle, "Enter with nothing focused does nothing");
        assert_eq!(hw.key(KEY_DELETE, false, &mut sh, 1.0), HistOutcome::Idle);
        assert_eq!(sh.history.items.len(), 2);
    }

    #[test]
    fn delete_removes_only_the_focused_row_and_focus_moves_to_its_neighbour() {
        let fonts = Fonts::load().expect("test fonts");
        let mut sh = shared(3); // rows are 3, 2, 1
        let mut hw = panel(&fonts, 3, true);
        hw.focus = Some(HistFocus::Clear);
        assert_eq!(hw.key(KEY_DELETE, false, &mut sh, 1.0), HistOutcome::Idle, "Delete on a control does nothing");
        assert_eq!(sh.history.items.len(), 3);
        hw.focus = Some(HistFocus::Item(2));
        assert_eq!(hw.key(KEY_DELETE, false, &mut sh, 1.0), HistOutcome::Redraw);
        assert_eq!(sh.history.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![3, 1]);
        assert_eq!(hw.focus, Some(HistFocus::Item(1)), "focus stays at the same position");
        assert_eq!(hw.key(KEY_DELETE, false, &mut sh, 1.0), HistOutcome::Redraw);
        assert_eq!(hw.focus, Some(HistFocus::Item(3)), "removing the last row moves focus up");
        assert_eq!(hw.key(KEY_DELETE, false, &mut sh, 1.0), HistOutcome::Redraw);
        assert!(sh.history.items.is_empty());
        assert_eq!(hw.focus, Some(HistFocus::Dnd), "an empty list leaves focus on a control");
    }

    #[test]
    fn enter_space_and_click_activate_clear_and_a_row_removal_never_replays_an_action() {
        let fonts = Fonts::load().expect("test fonts");
        let mut sh = shared(3);
        let mut hw = panel(&fonts, 3, true);
        // a click on a row removes it and focuses nothing special: no UiEvent exists on this path at all
        let (_, _, rows) = hist_rects(&hw, &sh);
        let r = rows[1];
        assert_eq!(hw.target_at(r.x + 5.0, r.y + 5.0, &sh), Some(HistFocus::Item(2)));
        assert_eq!(hw.click(r.x + 5.0, r.y + 5.0, &mut sh), HistOutcome::Redraw);
        assert_eq!(sh.history.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![3, 1]);
        assert!(!hw.ring, "a pointer click hides the focus ring");
        // a click on nothing is ignored
        assert_eq!(hw.click(2.0, 2.0, &mut sh), HistOutcome::Idle);
        // Enter and Space on Clear both empty the list and reset the scroll
        for sym in [KEY_RETURN, KEY_SPACE, KEY_KP_ENTER] {
            let mut sh = shared(2);
            let mut hw = panel(&fonts, 2, true);
            hw.focus = Some(HistFocus::Clear);
            hw.scroll = 1;
            assert_eq!(hw.key(sym, false, &mut sh, 1.0), HistOutcome::Redraw, "{sym:#x}");
            assert!(sh.history.items.is_empty());
            assert_eq!(hw.scroll, 0);
        }
    }

    #[test]
    fn the_wheel_pans_a_tall_panel_before_it_scrolls_rows_and_never_leaves_the_content() {
        let fonts = Fonts::load().expect("test fonts");
        let sh = shared(12);
        let mut hw = panel(&fonts, 3, false); // surface 134 high, content taller
        let max_pan = hw.content_h - hw.h as f32;
        assert!(max_pan > 0.0);
        hw.ring = true;
        assert_eq!(hw.scroll_step(false, &sh, 1.0), HistOutcome::Redraw);
        assert_eq!(hw.pan, 30.0f32.min(max_pan));
        assert!(!hw.ring, "wheel input hides the focus ring");
        for _ in 0..20 {
            hw.scroll_step(false, &sh, 1.0);
        }
        assert_eq!(hw.pan, max_pan, "panning stops at the content");
        assert!(hw.scroll > 0 && hw.scroll <= sh.history.items.len() - hw.rows, "then rows scroll, never past the last full page");
        for _ in 0..40 {
            hw.scroll_step(true, &sh, 1.0);
        }
        assert_eq!((hw.pan, hw.scroll), (0.0, 0));
    }

    #[test]
    fn hover_tracks_rows_and_reports_only_changes() {
        let fonts = Fonts::load().expect("test fonts");
        let sh = shared(3);
        let mut hw = panel(&fonts, 3, true);
        let (_, _, rows) = hist_rects(&hw, &sh);
        assert!(hw.hover_at(5.0, rows[1].y + 3.0, &sh));
        assert_eq!(hw.hover, Some(1));
        assert!(!hw.hover_at(6.0, rows[1].y + 4.0, &sh), "same row: nothing to repaint");
        assert!(hw.hover_at(5.0, 2.0, &sh), "moving onto the header clears the highlight");
        assert_eq!(hw.hover, None);
        hw.hover = Some(0);
        assert!(hw.clear_hover());
        assert!(!hw.clear_hover());
    }

    #[test]
    fn relayout_keeps_focus_scroll_and_pan_inside_the_new_bounds() {
        let fonts = Fonts::load().expect("test fonts");
        let mut sh = shared(6);
        let mut hw = panel(&fonts, 3, true);
        hw.scroll = 3;
        hw.pan = 50.0;
        hw.focus = Some(HistFocus::Item(1));
        sh.history.remove(1);
        let m = Metrics { s: 1.0, x: 0, y: 0, w: 1920, h: 1080, top: 8, right_gap: 12 };
        let dims = history_dimensions(&fonts, &m, &sh, &|w, h| (w, h));
        let (w, h, rows) = (dims.w, dims.h, dims.rows);
        assert!(hw.relayout(dims, 100, 8, &sh, 1.0), "a new size and position are reported");
        assert_eq!((hw.w, hw.h, hw.x, hw.y, hw.rows), (w, h, 100, 8, rows));
        assert!(hw.scroll <= sh.history.items.len().saturating_sub(rows));
        assert!(hw.pan <= (hw.content_h - hw.h as f32).max(0.0));
        assert_eq!(hw.focus, Some(HistFocus::Dnd), "focus on a removed row falls back to a control");
        let again = history_dimensions(&fonts, &m, &sh, &|w, h| (w, h));
        assert!(!hw.relayout(again, 100, 8, &sh, 1.0), "unchanged geometry is not a change");
    }

    #[test]
    fn history_dimensions_fit_the_area_and_honour_the_snap() {
        let fonts = Fonts::load().expect("test fonts");
        let sh = shared(100);
        let m = Metrics { s: 2.0, x: 0, y: 0, w: 3840, h: 400, top: 16, right_gap: 24 };
        let d = history_dimensions(&fonts, &m, &sh, &|w, h| (w.div_ceil(2) * 2, h.div_ceil(2) * 2));
        assert!(d.h as f32 <= m.available_height() + 1.0);
        assert!(d.rows >= 1 && d.rows <= HIST_MAX_ROWS);
        assert_eq!((d.w % 2, d.h % 2), (0, 0));
        // an empty history still has room for its "No Notifications" row
        let empty = history_dimensions(&fonts, &Metrics { h: 1080, ..m }, &shared(0), &|w, h| (w, h));
        assert!(empty.h > 0 && empty.content_h > 0.0);
    }

    #[test]
    fn contains_is_half_open_in_panel_pixels() {
        let fonts = Fonts::load().expect("test fonts");
        let hw = panel(&fonts, 1, true);
        assert!(hw.contains(0.0, 0.0) && hw.contains(295.9, 1.0));
        assert!(!hw.contains(296.0, 1.0) && !hw.contains(-0.1, 1.0) && !hw.contains(1.0, hw.h as f32));
    }
}
