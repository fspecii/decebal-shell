//! The clipboard picker: a Spotlight-style popup with a search field on top and the history below.

use super::history::{self, History};
use crate::app::Button;
use crate::gfx::{blit_scaled, fill_rect, fill_rounded, stroke_rounded};
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use tiny_skia::{Color, Pixmap};

const CARD_W: u32 = 640;
const HEADER_H: f32 = 60.0;
const ROW_H: f32 = 40.0;
const MAX_ROWS: usize = 9;
const PAD: f32 = 12.0;
/// Longest stretch of a line that is measured and drawn (the rest could never fit).
const LINE_CHARS: usize = 240;

/// Width of the drawn return symbol (see [`draw_return_icon`]).
const RETURN_ICON: f32 = 10.0;

/// The return symbol (down, then left with an arrowhead) with its left edge at `x` and middle at `cy`.
fn draw_return_icon(pm: &mut Pixmap, x: f32, cy: f32, c: Color) {
    let (s, top) = (RETURN_ICON, cy - 4.5);
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x + s, top);
    pb.line_to(x + s, top + 6.0);
    pb.line_to(x + 1.0, top + 6.0);
    pb.move_to(x + 4.0, top + 3.0);
    pb.line_to(x + 1.0, top + 6.0);
    pb.line_to(x + 4.0, top + 9.0);
    if let Some(path) = pb.finish() {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke { width: 1.3, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..tiny_skia::Stroke::default() };
        pm.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
    }
}

/// What the picker decided; read by `main` after the popup is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Closed without a choice (Escape, click outside).
    Pending,
    /// Make it the clipboard, then press Ctrl+V in the window that had the focus.
    Paste(String),
    /// Make it the clipboard only.
    Copy(String),
}

pub struct Picker {
    history: History,
    /// The history file: a deleted entry is written back at once.
    path: Option<PathBuf>,
    /// Called after the file changed (tells a running daemon to re-read it).
    on_change: Box<dyn Fn()>,
    field: TextField,
    /// Indices into the history that match the search, newest first.
    shown: Vec<usize>,
    sel: usize,
    scroll: usize,
    now: u64,
    size: (u32, u32),
    alpha: bool,
    fonts: Option<Fonts>,
    symbols: SymbolCache,
    outcome: Rc<RefCell<Outcome>>,
    /// Where the pointer was when the popup appeared: hovering selects only after it really moves, so a pointer that
    /// happens to rest over the list does not take the selection away from the first entry.
    pointer_home: Option<(f32, f32)>,
    pointer_moved: bool,
}

impl Picker {
    pub fn new(history: History, path: Option<PathBuf>, on_change: Box<dyn Fn()>, outcome: Rc<RefCell<Outcome>>) -> Picker {
        let mut p = Picker {
            history,
            path,
            on_change,
            field: TextField::default(),
            shown: Vec::new(),
            sel: 0,
            scroll: 0,
            now: crate::fuzzy::now(),
            size: (CARD_W, HEADER_H as u32),
            alpha: false,
            fonts: Fonts::load(),
            symbols: SymbolCache::default(),
            outcome,
            pointer_home: None,
            pointer_moved: false,
        };
        p.refilter();
        p
    }

    fn refilter(&mut self) {
        self.shown = history::filter(self.history.entries(), self.field.text());
        self.sel = 0;
        self.scroll = 0;
    }

    fn rows(&self) -> usize {
        self.shown.len().clamp(1, MAX_ROWS)
    }

    fn card_h(&self) -> f32 {
        HEADER_H + self.rows() as f32 * ROW_H + PAD
    }

    fn move_sel(&mut self, delta: isize) -> Response {
        if self.shown.is_empty() {
            return Response::NONE;
        }
        let n = self.shown.len() as isize;
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + MAX_ROWS {
            self.scroll = self.sel + 1 - MAX_ROWS;
        }
        Response::REDRAW
    }

    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < 0.0 || x >= self.size.0 as f32 || y < HEADER_H {
            return None;
        }
        let i = ((y - HEADER_H) / ROW_H) as usize + self.scroll;
        (i < self.shown.len() && i < self.scroll + MAX_ROWS).then_some(i)
    }

    /// Decides for the entry at position `pos` of the shown list and closes.
    fn pick(&mut self, pos: usize, copy_only: bool) -> Response {
        let Some(entry) = self.shown.get(pos).and_then(|&i| self.history.entries().get(i)) else { return Response::NONE };
        let text = entry.text.clone();
        *self.outcome.borrow_mut() = if copy_only { Outcome::Copy(text) } else { Outcome::Paste(text) };
        Response::act(Action::Close)
    }

    fn delete_selected(&mut self) -> Response {
        let Some(&idx) = self.shown.get(self.sel) else { return Response::NONE };
        self.history.remove(idx);
        if let Some(p) = &self.path {
            if let Err(e) = self.history.save(p) {
                eprintln!("macclip: cannot write {}: {e}", p.display());
            }
        }
        (self.on_change)();
        let keep = self.sel;
        self.refilter();
        self.sel = keep.min(self.shown.len().saturating_sub(1));
        self.scroll = self.scroll.min(self.sel).max((self.sel + 1).saturating_sub(MAX_ROWS));
        Response::REDRAW
    }

    fn measure(&self, s: &str, px: f32) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, Weight::Regular))
    }

    /// `s` cut with an ellipsis so it is at most `max_w` wide.
    fn fit(&self, s: &str, px: f32, max_w: f32) -> String {
        let mut t: String = s.chars().take(LINE_CHARS).collect();
        let cut = t.len() < s.len();
        if !cut && self.measure(&t, px) <= max_w {
            return t;
        }
        while !t.is_empty() && self.measure(&format!("{t}…"), px) > max_w {
            t.pop();
        }
        format!("{}…", t.trim_end())
    }

    fn text_at(&mut self, pm: &mut Pixmap, s: &str, x: f32, cy: f32, px: f32, c: Color) {
        if let Some(f) = &mut self.fonts {
            f.draw(pm, s, x, cy - px * 0.62, px, Weight::Regular, c);
        }
    }

    fn draw_card(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        pm.fill(Color::TRANSPARENT);
        let radius = th.r(1.6);
        let base = if self.alpha { th.overlay_c() } else { th.overlay.color(1.0) };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, base);

        // Search line.
        if let Some(sym) = self.symbols.get("magnifyingglass", th.text_faint.t(), 44) {
            blit_scaled(pm, sym, PAD + 10.0, HEADER_H / 2.0 - 10.0, 20.0, 20.0, 1.0);
        }
        let (tx, cy) = (PAD + 44.0, HEADER_H / 2.0);
        if self.field.text().is_empty() {
            self.text_at(pm, "Search clipboard history", tx, cy, 19.0, th.ink(0.55));
        } else {
            let t = self.field.text().to_string();
            self.text_at(pm, &t, tx, cy, 19.0, th.text_c());
            let cx = (tx + self.measure(self.field.before_cursor(), 19.0)).round();
            fill_rect(pm, cx, cy - 11.0, 1.5, 22.0, th.text_c());
        }
        fill_rect(pm, 0.0, HEADER_H - 1.0, w, 1.0, th.ink(0.10));

        if self.shown.is_empty() {
            let msg = if self.history.is_empty() { "Nothing copied yet" } else { "No matches" };
            let mw = self.measure(msg, 14.0);
            self.text_at(pm, msg, ((w - mw) / 2.0).round(), HEADER_H + ROW_H / 2.0, 14.0, th.ink(0.55));
        }
        let end = (self.scroll + MAX_ROWS).min(self.shown.len());
        for pos in self.scroll..end {
            let y = HEADER_H + (pos - self.scroll) as f32 * ROW_H;
            let entry = &self.history.entries()[self.shown[pos]];
            let selected = pos == self.sel;
            if selected {
                fill_rounded(pm, 8.0, y + 2.0, w - 16.0, ROW_H - 4.0, th.r(0.9), th.accent_c());
            }
            let (main, sub) = if selected { (th.accent_text_c(), th.accent_text.color(0.75)) } else { (th.text_c(), th.ink(0.55)) };
            let (first, lines) = history::summarize(&entry.text);
            let age = history::age(self.now, entry.time);
            // "⏎ N lines": the return symbol is drawn, since many fonts (Inter among them) have no glyph for U+23CE.
            let hint = if lines > 1 { format!("{lines} lines") } else { String::new() };
            let age_w = self.measure(&age, 12.0);
            let hint_w = if hint.is_empty() { 0.0 } else { self.measure(&hint, 12.0) + RETURN_ICON + 6.0 + 14.0 };
            let text_x = 52.0;
            if pos < 9 {
                self.text_at(pm, &(pos + 1).to_string(), 24.0, y + ROW_H / 2.0, 13.0, sub);
            }
            let max_w = w - text_x - 24.0 - age_w - hint_w - 12.0;
            let line = self.fit(&first, 15.0, max_w);
            self.text_at(pm, &line, text_x, y + ROW_H / 2.0, 15.0, main);
            self.text_at(pm, &age, w - 20.0 - age_w, y + ROW_H / 2.0, 12.0, sub);
            if !hint.is_empty() {
                let hx = w - 20.0 - age_w - hint_w;
                draw_return_icon(pm, hx, y + ROW_H / 2.0, sub);
                self.text_at(pm, &hint, hx + RETURN_ICON + 6.0, y + ROW_H / 2.0, 12.0, sub);
            }
        }
        stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), th.outline_c(), 1.0);
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }
}

impl WindowApp for Picker {
    fn title(&self) -> String {
        "Clipboard".into()
    }

    fn app_id(&self) -> &'static str {
        "macclip"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn popup_translucent(&self) -> bool {
        true
    }

    fn set_popup_alpha(&mut self, alpha: bool) {
        self.alpha = alpha;
    }

    /// The grab is on purpose: a window manager moving the focus (focus follows mouse) must not close the picker.
    fn popup_closes_on_focus_loss(&self) -> bool {
        false
    }

    fn input_grabbed(&mut self, mods: Option<u16>) -> Response {
        // Without the keyboard nothing can be typed or chosen.
        if mods.is_none() {
            return Response::act(Action::Close);
        }
        Response::NONE
    }

    fn initial_size(&self) -> (u32, u32) {
        (CARD_W, self.card_h().ceil() as u32)
    }

    fn content_size(&self) -> Option<(u32, u32)> {
        Some((CARD_W, self.card_h().ceil() as u32))
    }

    fn set_waker(&mut self, waker: Waker) {
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w());
    }

    fn set_screen(&mut self, _: u32, _: u32) {}

    fn resize(&mut self, w: u32, h: u32) {
        self.size = (w, h);
    }

    fn set_rounded(&mut self, _: bool) {}

    fn draw(&mut self, pm: &mut Pixmap) {
        self.draw_card(pm);
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        let home = *self.pointer_home.get_or_insert((x, y));
        if !self.pointer_moved {
            if (x - home.0).abs() + (y - home.1).abs() < 4.0 {
                return Response::NONE;
            }
            self.pointer_moved = true;
        }
        match self.row_at(x, y) {
            Some(i) if i != self.sel => {
                self.sel = i;
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed || button != Button::Left {
            return Response::NONE;
        }
        match self.row_at(x, y) {
            Some(i) => self.pick(i, false),
            None => Response::NONE,
        }
    }

    fn scroll(&mut self, _: f32, _: f32, dy: f32) -> Response {
        if dy > 0.0 {
            self.move_sel(1)
        } else if dy < 0.0 {
            self.move_sel(-1)
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Escape => Response::act(Action::Close),
            Key::Enter => self.pick(self.sel, ev.shift),
            Key::Down => self.move_sel(1),
            Key::Tab => self.move_sel(if ev.shift { -1 } else { 1 }),
            Key::Up => self.move_sel(-1),
            Key::PageDown => self.move_sel(MAX_ROWS as isize),
            Key::PageUp => self.move_sel(-(MAX_ROWS as isize)),
            Key::Delete => self.delete_selected(),
            Key::Left => {
                self.field.left();
                Response::REDRAW
            }
            Key::Right => {
                self.field.right();
                Response::REDRAW
            }
            Key::Home => {
                self.field.home();
                Response::REDRAW
            }
            Key::End => {
                self.field.end();
                Response::REDRAW
            }
            Key::Backspace => {
                self.field.backspace();
                self.refilter();
                Response::REDRAW
            }
            // Alt+1 .. Alt+9: the entry at that position (Shift: copy only).
            Key::Char if ev.alt && !ev.ctrl && matches!(ev.ch, Some('1'..='9')) => {
                let n = ev.ch.and_then(|c| c.to_digit(10)).unwrap_or(1) as usize;
                self.pick(n - 1, ev.shift)
            }
            Key::Char if ev.ctrl && ev.ch == Some('u') => {
                self.field.clear();
                self.refilter();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && ev.ch == Some('a') => {
                self.field.home();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && ev.ch == Some('e') => {
                self.field.end();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && matches!(ev.ch, Some('n' | 'j')) => self.move_sel(1),
            Key::Char if ev.ctrl && matches!(ev.ch, Some('p' | 'k')) => self.move_sel(-1),
            Key::Char if !ev.ctrl && !ev.alt && !ev.text.is_empty() => {
                // The space of the hotkey that opened us (auto-repeat, a late release) must not start the search.
                if self.field.text().is_empty() && ev.text.trim().is_empty() {
                    return Response::NONE;
                }
                self.field.insert(&ev.text);
                self.refilter();
                Response::REDRAW
            }
            Key::Char => Response::NONE,
        }
    }

    fn tick(&mut self) -> Response {
        // A theme change: repaint.
        if crate::theme::take_changed() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key, text: &str) -> KeyEvent {
        KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift: false, alt: false }
    }

    fn picker(texts: &[&str]) -> (Picker, Rc<RefCell<Outcome>>) {
        let mut h = History::default();
        for (i, t) in texts.iter().enumerate() {
            h.push(t, 1000 + i as u64);
        }
        let out = Rc::new(RefCell::new(Outcome::Pending));
        (Picker::new(h, None, Box::new(|| {}), out.clone()), out)
    }

    fn type_text(p: &mut Picker, s: &str) {
        for c in s.chars() {
            p.key(&key(Key::Char, &c.to_string()));
        }
    }

    fn closes(r: Response) -> bool {
        r.action == Some(Action::Close)
    }

    fn texts(p: &Picker) -> Vec<String> {
        p.shown.iter().map(|&i| p.history.entries()[i].text.clone()).collect()
    }

    #[test]
    fn lists_newest_first_and_filters_while_typing() {
        let (mut p, _) = picker(&["alpha one", "Beta two", "alpha three"]);
        assert_eq!(texts(&p), ["alpha three", "Beta two", "alpha one"]);
        type_text(&mut p, "ALPHA");
        assert_eq!(texts(&p), ["alpha three", "alpha one"]);
        type_text(&mut p, " o");
        assert_eq!(texts(&p), ["alpha one"]);
        p.key(&key(Key::Backspace, ""));
        p.key(&key(Key::Backspace, ""));
        assert_eq!(p.shown.len(), 2);
        p.key(&KeyEvent { ctrl: true, ch: Some('u'), ..key(Key::Char, "") });
        assert_eq!(p.shown.len(), 3);
    }

    #[test]
    fn navigation_wraps_and_tab_goes_forward_shift_tab_back() {
        let (mut p, _) = picker(&["a", "b", "c"]);
        assert_eq!(p.sel, 0);
        p.key(&key(Key::Down, ""));
        p.key(&key(Key::Tab, ""));
        assert_eq!(p.sel, 2);
        p.key(&key(Key::Down, ""));
        assert_eq!(p.sel, 0, "wraps");
        p.key(&key(Key::Up, ""));
        assert_eq!(p.sel, 2);
        p.key(&KeyEvent { shift: true, ..key(Key::Tab, "") });
        assert_eq!(p.sel, 1);
    }

    #[test]
    fn return_pastes_the_selected_entry_and_shift_return_only_copies() {
        let (mut p, out) = picker(&["first", "second"]);
        p.key(&key(Key::Down, ""));
        assert!(closes(p.key(&key(Key::Enter, ""))));
        assert_eq!(*out.borrow(), Outcome::Paste("first".into()));
        let (mut p, out) = picker(&["first", "second"]);
        assert!(closes(p.key(&KeyEvent { shift: true, ..key(Key::Enter, "") })));
        assert_eq!(*out.borrow(), Outcome::Copy("second".into()));
    }

    #[test]
    fn return_on_the_filtered_list_picks_from_the_filtered_list() {
        let (mut p, out) = picker(&["apple", "banana", "apricot"]);
        type_text(&mut p, "ban");
        p.key(&key(Key::Enter, ""));
        assert_eq!(*out.borrow(), Outcome::Paste("banana".into()));
    }

    #[test]
    fn escape_and_an_empty_list_choose_nothing() {
        let (mut p, out) = picker(&["a"]);
        assert!(closes(p.key(&key(Key::Escape, ""))));
        assert_eq!(*out.borrow(), Outcome::Pending);
        let (mut p, out) = picker(&[]);
        assert!(!closes(p.key(&key(Key::Enter, ""))), "nothing to pick");
        assert_eq!(*out.borrow(), Outcome::Pending);
        let (mut p, _) = picker(&["a"]);
        type_text(&mut p, "zzz");
        assert!(p.shown.is_empty());
        assert!(!closes(p.key(&key(Key::Enter, ""))));
    }

    #[test]
    fn alt_digits_quick_pick() {
        let (mut p, out) = picker(&["one", "two", "three"]);
        assert!(closes(p.key(&KeyEvent { alt: true, ch: Some('2'), ..key(Key::Char, "") })));
        assert_eq!(*out.borrow(), Outcome::Paste("two".into()), "Alt+2 is the second entry (newest first is 1)");
        let (mut p, _) = picker(&["one"]);
        assert!(!closes(p.key(&KeyEvent { alt: true, ch: Some('5'), ..key(Key::Char, "") })), "no fifth entry");
        let (mut p, out) = picker(&["one", "two"]);
        assert!(closes(p.key(&KeyEvent { alt: true, shift: true, ch: Some('1'), ..key(Key::Char, "") })));
        assert_eq!(*out.borrow(), Outcome::Copy("two".into()));
        let (mut p, _) = picker(&["x"]);
        type_text(&mut p, "1");
        assert_eq!(p.field.text(), "1", "a plain digit is search text");
    }

    #[test]
    fn delete_removes_the_entry_and_writes_the_file() {
        let dir = std::env::temp_dir().join(format!("macclip-picker-test-{}", std::process::id()));
        let path = dir.join("clipboard-history");
        let mut h = History::default();
        for t in ["a", "b", "c"] {
            h.push(t, 1);
        }
        let changed = Rc::new(RefCell::new(0));
        let c2 = changed.clone();
        let mut p = Picker::new(h, Some(path.clone()), Box::new(move || *c2.borrow_mut() += 1), Rc::new(RefCell::new(Outcome::Pending)));
        p.key(&key(Key::Down, "")); // "b"
        let r = p.key(&key(Key::Delete, ""));
        assert!(r.redraw && !closes(r));
        assert_eq!(texts(&p), ["c", "a"]);
        assert_eq!(p.sel, 1, "the selection stays on the neighbour");
        assert_eq!(History::load(&path).entries().iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["c", "a"]);
        assert_eq!(*changed.borrow(), 1, "the daemon is told");
        p.key(&key(Key::Delete, ""));
        p.key(&key(Key::Delete, ""));
        assert!(p.shown.is_empty());
        p.key(&key(Key::Delete, ""));
        assert!(History::load(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clicking_a_row_pastes_it() {
        let (mut p, out) = picker(&["a", "b", "c"]);
        p.resize(CARD_W, p.card_h() as u32);
        assert!(!closes(p.pointer_button(10.0, 10.0, Button::Left, true)), "the search field is not a row");
        assert!(!p.pointer_motion(10.0, HEADER_H + ROW_H * 1.5).redraw, "the first position is only remembered");
        assert_eq!(p.sel, 0, "a pointer resting over the list does not select");
        assert!(p.pointer_motion(10.0, HEADER_H + ROW_H * 1.5 + 6.0).redraw);
        assert_eq!(p.sel, 1);
        assert!(closes(p.pointer_button(10.0, HEADER_H + ROW_H * 2.5, Button::Left, true)));
        assert_eq!(*out.borrow(), Outcome::Paste("a".into()));
    }

    #[test]
    fn the_card_grows_with_the_list_up_to_nine_rows_and_scrolls() {
        let (mut p, _) = picker(&[]);
        let empty_h = p.card_h();
        let names: Vec<String> = (0..20).map(|i| format!("entry {i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let (mut full, _) = picker(&refs);
        assert_eq!(full.card_h(), HEADER_H + 9.0 * ROW_H + PAD);
        assert_eq!(empty_h, HEADER_H + ROW_H + PAD, "one row of room for the empty message");
        for _ in 0..12 {
            full.key(&key(Key::Down, ""));
        }
        assert_eq!(full.sel, 12);
        assert_eq!(full.scroll, 4, "the selection stays inside the nine visible rows");
        assert_eq!(full.row_at(5.0, HEADER_H + 1.0), Some(4));
        assert_eq!(p.content_size(), Some((CARD_W, empty_h.ceil() as u32)));
        p.key(&key(Key::Escape, ""));
    }

    #[test]
    fn a_leading_space_does_not_start_the_search() {
        let (mut p, _) = picker(&["a b"]);
        p.key(&key(Key::Char, " "));
        assert_eq!(p.field.text(), "");
        type_text(&mut p, "a b");
        assert_eq!(p.shown.len(), 1);
    }

    #[test]
    fn it_draws_something_for_a_long_multiline_entry() {
        let (mut p, _) = picker(&["first line is quite long ".repeat(40).as_str(), "x\ny\nz"]);
        let (w, h) = (CARD_W, p.card_h().ceil() as u32);
        p.resize(w, h);
        let mut pm = Pixmap::new(w, h).unwrap();
        p.draw(&mut pm);
        assert!(pm.pixels().iter().any(|px| px.alpha() == 255), "the card is painted");
    }
}
