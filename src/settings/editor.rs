//! A small multi-line text editor widget for the Configuration pane: a pure model ([`Editor`]: lines, cursor, selection,
//! undo/redo, word and page movement), a cheap line lexer for JSON and TOML colouring ([`tokens`]) and the painter
//! ([`draw`]) with line numbers, error markers, a status strip and scrollbars.
//!
//! The model has no windowing in it, so everything but the painter is unit tested. Columns are in characters.

use super::ui::{pal, Painter, Placed, Rect};
use crate::gfx::{color, fill_rounded, stroke_rounded};
use crate::text::Weight;
use std::rc::Rc;
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Pos {
        Pos { line, col }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Json,
    Toml,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Typing,
    Backspace,
    DeleteFwd,
    Other,
}

#[derive(Debug, Clone)]
struct Edit {
    at: Pos,
    removed: String,
    inserted: String,
    before: (Pos, Option<Pos>),
    after: Pos,
    kind: Kind,
}

const MAX_UNDO: usize = 1000;
pub const INDENT: &str = "  ";

pub struct Editor {
    lines: Rc<Vec<String>>,
    pub cursor: Pos,
    pub anchor: Option<Pos>,
    /// Column kept while moving up and down over short lines.
    want_col: Option<usize>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// `undo.len()` when the text was last saved or loaded; `None` once that state is unreachable.
    saved_at: Option<usize>,
    no_merge: bool,
    /// First visible line and horizontal scroll (unscaled px), kept up to date by the painter.
    pub top: usize,
    pub left: f32,
    /// Ask the painter to scroll the cursor into view.
    pub reveal: bool,
    /// Lines per page, set by the painter (used by PageUp/PageDown).
    pub page: usize,
    pub read_only: bool,
    pub lang: Lang,
    /// Text copied inside the editor (used when no system clipboard tool answers).
    pub clip: String,
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `text` inserted at `at` ends.
fn advance(at: Pos, text: &str) -> Pos {
    let n = text.matches('\n').count();
    if n == 0 {
        Pos::new(at.line, at.col + char_len(text))
    } else {
        Pos::new(at.line + n, char_len(text.rsplit('\n').next().unwrap_or("")))
    }
}

impl Editor {
    pub fn new(text: &str, lang: Lang) -> Editor {
        let mut e = Editor {
            lines: Rc::new(vec![String::new()]),
            cursor: Pos::default(),
            anchor: None,
            want_col: None,
            undo: Vec::new(),
            redo: Vec::new(),
            saved_at: Some(0),
            no_merge: false,
            top: 0,
            left: 0.0,
            reveal: false,
            page: 20,
            read_only: false,
            lang,
            clip: String::new(),
        };
        e.set_text(text);
        e
    }

    /// Replaces everything; clears history and marks the text as saved.
    pub fn set_text(&mut self, text: &str) {
        let t = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut lines: Vec<String> = t.split('\n').map(String::from).collect();
        // a trailing newline is not an extra empty editable line... keep it: the file really ends with one
        if lines.is_empty() {
            lines.push(String::new());
        }
        self.lines = Rc::new(lines);
        self.cursor = Pos::default();
        self.anchor = None;
        self.want_col = None;
        self.undo.clear();
        self.redo.clear();
        self.saved_at = Some(0);
        self.no_merge = false;
        self.top = 0;
        self.left = 0.0;
        self.reveal = true;
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn lines(&self) -> &Rc<Vec<String>> {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.undo.len());
        self.no_merge = true;
    }

    pub fn is_modified(&self) -> bool {
        self.saved_at != Some(self.undo.len())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ---------------------------------------------------------------- selection

    /// Ordered selection, `None` when empty.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        Some(if a < self.cursor { (a, self.cursor) } else { (self.cursor, a) })
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.text_in(a, b))
    }

    fn text_in(&self, a: Pos, b: Pos) -> String {
        if a.line == b.line {
            let l = self.line(a.line);
            return l[byte_at(l, a.col)..byte_at(l, b.col)].to_string();
        }
        let mut out = String::new();
        let first = self.line(a.line);
        out.push_str(&first[byte_at(first, a.col)..]);
        for i in a.line + 1..b.line {
            out.push('\n');
            out.push_str(self.line(i));
        }
        out.push('\n');
        let last = self.line(b.line);
        out.push_str(&last[..byte_at(last, b.col)]);
        out
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        let last = self.lines.len() - 1;
        self.cursor = Pos::new(last, char_len(self.line(last)));
        self.want_col = None;
        self.reveal = true;
    }

    fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos::new(line, p.col.min(char_len(self.line(line))))
    }

    /// Puts the cursor at `p` (extending the selection when `extend`).
    pub fn set_cursor(&mut self, p: Pos, extend: bool) {
        let p = self.clamp(p);
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = p;
        self.want_col = None;
        self.reveal = true;
    }

    // ---------------------------------------------------------------- movement

    fn move_to(&mut self, p: Pos, extend: bool, keep_want: bool) {
        let want = self.want_col;
        self.set_cursor(p, extend);
        self.want_col = if keep_want { want } else { None };
    }

    pub fn left(&mut self, extend: bool) {
        if !extend {
            if let Some((a, _)) = self.selection() {
                return self.set_cursor(a, false);
            }
        }
        let c = self.cursor;
        let p = if c.col > 0 {
            Pos::new(c.line, c.col - 1)
        } else if c.line > 0 {
            Pos::new(c.line - 1, char_len(self.line(c.line - 1)))
        } else {
            c
        };
        self.move_to(p, extend, false);
    }

    pub fn right(&mut self, extend: bool) {
        if !extend {
            if let Some((_, b)) = self.selection() {
                return self.set_cursor(b, false);
            }
        }
        let c = self.cursor;
        let p = if c.col < char_len(self.line(c.line)) {
            Pos::new(c.line, c.col + 1)
        } else if c.line + 1 < self.lines.len() {
            Pos::new(c.line + 1, 0)
        } else {
            c
        };
        self.move_to(p, extend, false);
    }

    fn vertical(&mut self, delta: isize, extend: bool) {
        let c = self.cursor;
        let want = self.want_col.unwrap_or(c.col);
        let line = (c.line as isize + delta).clamp(0, self.lines.len() as isize - 1) as usize;
        if line == c.line && delta != 0 {
            // at the first/last line: jump to the line's end/start like most editors
            let p = if delta < 0 { Pos::new(0, 0) } else { Pos::new(line, char_len(self.line(line))) };
            return self.move_to(p, extend, false);
        }
        let p = Pos::new(line, want.min(char_len(self.line(line))));
        self.move_to(p, extend, true);
        self.want_col = Some(want);
    }

    pub fn up(&mut self, extend: bool) {
        self.vertical(-1, extend);
    }

    pub fn down(&mut self, extend: bool) {
        self.vertical(1, extend);
    }

    pub fn page_up(&mut self, extend: bool) {
        self.vertical(-(self.page.max(2) as isize - 1), extend);
        self.top = self.top.saturating_sub(self.page.saturating_sub(1));
    }

    pub fn page_down(&mut self, extend: bool) {
        self.vertical(self.page.max(2) as isize - 1, extend);
        self.top = (self.top + self.page.saturating_sub(1)).min(self.lines.len().saturating_sub(1));
    }

    /// Smart Home: first non-blank, then column 0.
    pub fn home(&mut self, extend: bool) {
        let c = self.cursor;
        let l = self.line(c.line);
        let first = l.chars().take_while(|ch| ch.is_whitespace()).count();
        let col = if c.col == first || first == char_len(l) { 0 } else { first };
        self.move_to(Pos::new(c.line, col), extend, false);
    }

    pub fn end(&mut self, extend: bool) {
        let c = self.cursor;
        self.move_to(Pos::new(c.line, char_len(self.line(c.line))), extend, false);
    }

    pub fn doc_start(&mut self, extend: bool) {
        self.move_to(Pos::default(), extend, false);
    }

    pub fn doc_end(&mut self, extend: bool) {
        let last = self.lines.len() - 1;
        self.move_to(Pos::new(last, char_len(self.line(last))), extend, false);
    }

    fn word_left_pos(&self) -> Pos {
        let c = self.cursor;
        if c.col == 0 {
            return if c.line > 0 { Pos::new(c.line - 1, char_len(self.line(c.line - 1))) } else { c };
        }
        let chars: Vec<char> = self.line(c.line).chars().collect();
        let mut i = c.col;
        while i > 0 && !is_word(chars[i - 1]) && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        if i == c.col || (i > 0 && is_word(chars[i - 1])) {
            while i > 0 && is_word(chars[i - 1]) {
                i -= 1;
            }
        }
        Pos::new(c.line, i)
    }

    fn word_right_pos(&self) -> Pos {
        let c = self.cursor;
        let chars: Vec<char> = self.line(c.line).chars().collect();
        if c.col >= chars.len() {
            return if c.line + 1 < self.lines.len() { Pos::new(c.line + 1, 0) } else { c };
        }
        let mut i = c.col;
        if is_word(chars[i]) {
            while i < chars.len() && is_word(chars[i]) {
                i += 1;
            }
        } else if !chars[i].is_whitespace() {
            while i < chars.len() && !is_word(chars[i]) && !chars[i].is_whitespace() {
                i += 1;
            }
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        Pos::new(c.line, i)
    }

    pub fn word_left(&mut self, extend: bool) {
        let p = self.word_left_pos();
        self.move_to(p, extend, false);
    }

    pub fn word_right(&mut self, extend: bool) {
        let p = self.word_right_pos();
        self.move_to(p, extend, false);
    }

    /// Double click: selects the word under the cursor.
    pub fn select_word(&mut self) {
        let c = self.cursor;
        let chars: Vec<char> = self.line(c.line).chars().collect();
        if chars.is_empty() {
            return;
        }
        let i = c.col.min(chars.len() - 1);
        let class = |ch: char| {
            if is_word(ch) {
                0
            } else if ch.is_whitespace() {
                1
            } else {
                2
            }
        };
        let k = class(chars[i]);
        let (mut a, mut b) = (i, i + 1);
        while a > 0 && class(chars[a - 1]) == k {
            a -= 1;
        }
        while b < chars.len() && class(chars[b]) == k {
            b += 1;
        }
        self.anchor = Some(Pos::new(c.line, a));
        self.cursor = Pos::new(c.line, b);
    }

    pub fn select_line(&mut self) {
        let l = self.cursor.line;
        self.anchor = Some(Pos::new(l, 0));
        self.cursor = if l + 1 < self.lines.len() { Pos::new(l + 1, 0) } else { Pos::new(l, char_len(self.line(l))) };
    }

    // ---------------------------------------------------------------- editing

    /// Replaces `a..b` with `text` and records the edit. `a <= b`.
    fn replace(&mut self, a: Pos, b: Pos, text: &str, kind: Kind) {
        let removed = self.text_in(a, b);
        if removed.is_empty() && text.is_empty() {
            return;
        }
        let before = (self.cursor, self.anchor);
        self.splice(a, b, text);
        let after = advance(a, text);
        self.cursor = after;
        self.anchor = None;
        self.want_col = None;
        self.reveal = true;
        self.redo.clear();
        if self.saved_at.is_some_and(|s| s > self.undo.len()) {
            self.saved_at = None;
        }
        let edit = Edit { at: a, removed, inserted: text.to_string(), before, after, kind };
        let can_merge = !self.no_merge && self.saved_at != Some(self.undo.len());
        if let Some(last) = self.undo.last_mut() {
            if can_merge && last.kind == kind && merge(last, &edit) {
                return;
            }
        }
        self.no_merge = false;
        self.undo.push(edit);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
            self.saved_at = self.saved_at.and_then(|s| s.checked_sub(1));
        }
    }

    /// The raw text change, no history.
    fn splice(&mut self, a: Pos, b: Pos, text: &str) {
        let lines = Rc::make_mut(&mut self.lines);
        let head = lines[a.line][..byte_at(&lines[a.line], a.col)].to_string();
        let tail = lines[b.line][byte_at(&lines[b.line], b.col)..].to_string();
        let mut mid: Vec<String> = text.split('\n').map(String::from).collect();
        mid[0] = format!("{head}{}", mid[0]);
        let last = mid.len() - 1;
        mid[last].push_str(&tail);
        lines.splice(a.line..=b.line, mid);
    }

    fn selection_or(&self, f: impl Fn(&Editor) -> (Pos, Pos)) -> (Pos, Pos) {
        self.selection().unwrap_or_else(|| f(self))
    }

    /// Typed or pasted text replaces the selection. Control characters other than newline and tab are dropped.
    pub fn insert_str(&mut self, s: &str) {
        if self.read_only {
            return;
        }
        let clean: String = s.replace("\r\n", "\n").replace('\r', "\n").chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        let (a, b) = self.selection_or(|e| (e.cursor, e.cursor));
        let kind = if clean.chars().count() == 1 && clean != "\n" { Kind::Typing } else { Kind::Other };
        let kind = if a != b { Kind::Other } else { kind };
        self.replace(a, b, &clean, kind);
    }

    /// Enter: new line with the current line's indentation (one level deeper after `{`, `[` or `:`).
    pub fn newline(&mut self) {
        if self.read_only {
            return;
        }
        let (a, b) = self.selection_or(|e| (e.cursor, e.cursor));
        let line = self.line(a.line);
        let before = &line[..byte_at(line, a.col)];
        let mut indent: String = before.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        if before.trim_end().ends_with(['{', '[']) {
            indent.push_str(INDENT);
        }
        self.replace(a, b, &format!("\n{indent}"), Kind::Other);
    }

    pub fn backspace(&mut self) {
        if self.read_only {
            return;
        }
        if let Some((a, b)) = self.selection() {
            return self.replace(a, b, "", Kind::Other);
        }
        let c = self.cursor;
        if c.col > 0 {
            // inside leading spaces delete back to the previous indent stop
            let l = self.line(c.line);
            let lead = l.chars().take_while(|ch| *ch == ' ').count();
            let n = if lead >= c.col && c.col >= INDENT.len() && c.col % INDENT.len() == 0 && l.chars().take(c.col).all(|ch| ch == ' ') { INDENT.len() } else { 1 };
            self.replace(Pos::new(c.line, c.col - n), c, "", Kind::Backspace);
        } else if c.line > 0 {
            let p = Pos::new(c.line - 1, char_len(self.line(c.line - 1)));
            self.replace(p, c, "", Kind::Backspace);
        }
    }

    pub fn delete(&mut self) {
        if self.read_only {
            return;
        }
        if let Some((a, b)) = self.selection() {
            return self.replace(a, b, "", Kind::Other);
        }
        let c = self.cursor;
        if c.col < char_len(self.line(c.line)) {
            self.replace(c, Pos::new(c.line, c.col + 1), "", Kind::DeleteFwd);
        } else if c.line + 1 < self.lines.len() {
            self.replace(c, Pos::new(c.line + 1, 0), "", Kind::DeleteFwd);
        }
    }

    pub fn delete_word_back(&mut self) {
        if self.read_only {
            return;
        }
        if self.selection().is_some() {
            return self.backspace();
        }
        let p = self.word_left_pos();
        let c = self.cursor;
        self.replace(p, c, "", Kind::Other);
    }

    pub fn delete_word_fwd(&mut self) {
        if self.read_only {
            return;
        }
        if self.selection().is_some() {
            return self.delete();
        }
        let p = self.word_right_pos();
        let c = self.cursor;
        self.replace(c, p, "", Kind::Other);
    }

    /// Ctrl+K: deletes to the end of the line (the newline when already there).
    pub fn kill_line(&mut self) {
        if self.read_only {
            return;
        }
        let c = self.cursor;
        let end = Pos::new(c.line, char_len(self.line(c.line)));
        if c == end {
            self.delete();
        } else {
            self.replace(c, end, "", Kind::Other);
        }
    }

    /// Tab: indent the selected lines, or insert spaces up to the next indent stop.
    pub fn indent(&mut self) {
        if self.read_only {
            return;
        }
        match self.selection() {
            Some((a, b)) if a.line != b.line => {
                let (mut sel_a, mut sel_b) = (a, b);
                let last = if b.col == 0 { b.line - 1 } else { b.line };
                for l in a.line..=last {
                    self.replace(Pos::new(l, 0), Pos::new(l, 0), INDENT, Kind::Other);
                }
                sel_a.col += INDENT.len();
                if b.col > 0 {
                    sel_b.col += INDENT.len();
                }
                self.anchor = Some(sel_a);
                self.cursor = sel_b;
            }
            _ => {
                let c = self.selection().map(|(a, _)| a).unwrap_or(self.cursor);
                let n = INDENT.len() - c.col % INDENT.len();
                self.insert_str(&" ".repeat(n));
            }
        }
    }

    /// Shift+Tab: removes one indent level from the selected (or current) lines.
    pub fn dedent(&mut self) {
        if self.read_only {
            return;
        }
        let (a, b) = self.selection().unwrap_or((self.cursor, self.cursor));
        let last = if b.line > a.line && b.col == 0 { b.line - 1 } else { b.line };
        let (mut ca, mut cb) = (self.anchor, self.cursor);
        for l in a.line..=last {
            let n = self.line(l).chars().take(INDENT.len()).take_while(|c| *c == ' ').count();
            if n > 0 {
                self.replace(Pos::new(l, 0), Pos::new(l, n), "", Kind::Other);
                if let Some(a) = ca.as_mut() {
                    if a.line == l {
                        a.col = a.col.saturating_sub(n);
                    }
                }
                if cb.line == l {
                    cb.col = cb.col.saturating_sub(n);
                }
            }
        }
        self.anchor = ca.filter(|a| *a != cb);
        self.cursor = cb;
    }

    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_text()?;
        if !self.read_only {
            let (a, b) = self.selection()?;
            self.replace(a, b, "", Kind::Other);
        }
        Some(t)
    }

    pub fn paste(&mut self, s: &str) {
        let n = s.chars().filter(|c| *c == '\n').count();
        self.insert_str(s);
        if n > 0 {
            self.no_merge = true;
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(e) = self.undo.pop() else { return false };
        let end = advance(e.at, &e.inserted);
        self.splice(e.at, end, &e.removed);
        self.cursor = e.before.0;
        self.anchor = e.before.1;
        self.redo.push(e);
        self.want_col = None;
        self.reveal = true;
        self.no_merge = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(e) = self.redo.pop() else { return false };
        let end = advance(e.at, &e.removed);
        self.splice(e.at, end, &e.inserted);
        self.cursor = e.after;
        self.anchor = None;
        self.undo.push(e);
        self.want_col = None;
        self.reveal = true;
        self.no_merge = true;
        true
    }

    /// Replaces the text as one undoable step (Format).
    pub fn replace_all(&mut self, text: &str) {
        let last = self.lines.len() - 1;
        let end = Pos::new(last, char_len(self.line(last)));
        let cur = self.cursor;
        self.replace(Pos::default(), end, text, Kind::Other);
        self.cursor = self.clamp(cur);
        self.no_merge = true;
    }
}

/// Folds `next` into `last` when both are one run of typing or deleting. True when merged.
fn merge(last: &mut Edit, next: &Edit) -> bool {
    match last.kind {
        Kind::Typing => {
            let end = advance(last.at, &last.inserted);
            if last.removed.is_empty() && next.removed.is_empty() && next.at == end && last.inserted.chars().count() < 80 && !next.inserted.contains('\n') {
                // a word boundary starts a new undo step, like most editors
                if next.inserted.trim().is_empty() != last.inserted.trim().is_empty() && !last.inserted.is_empty() {
                    return false;
                }
                last.inserted.push_str(&next.inserted);
                last.after = next.after;
                return true;
            }
            false
        }
        Kind::Backspace => {
            if last.inserted.is_empty() && next.inserted.is_empty() && advance(next.at, &next.removed) == last.at && !next.removed.contains('\n') {
                last.removed = format!("{}{}", next.removed, last.removed);
                last.at = next.at;
                last.after = next.after;
                return true;
            }
            false
        }
        Kind::DeleteFwd => {
            if last.inserted.is_empty() && next.inserted.is_empty() && next.at == last.at && !next.removed.contains('\n') {
                last.removed.push_str(&next.removed);
                return true;
            }
            false
        }
        Kind::Other => false,
    }
}

// ---------------------------------------------------------------- colouring

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Key,
    Str,
    Num,
    Kw,
    Punct,
    Comment,
    Header,
}

/// Token runs of one line: `(start, end, class)` in characters, covering the whole line without gaps.
pub fn tokens(lang: Lang, line: &str) -> Vec<(usize, usize, Tok)> {
    let chars: Vec<char> = line.chars().collect();
    let mut cls = vec![Tok::Plain; chars.len()];
    match lang {
        Lang::Json => lex_json(&chars, &mut cls),
        Lang::Toml => lex_toml(&chars, &mut cls),
        Lang::Plain => {}
    }
    let mut out: Vec<(usize, usize, Tok)> = Vec::new();
    for (i, c) in cls.iter().enumerate() {
        match out.last_mut() {
            Some(last) if last.2 == *c => last.1 = i + 1,
            _ => out.push((i, i + 1, *c)),
        }
    }
    out
}

fn lex_string(chars: &[char], start: usize) -> usize {
    let q = chars[start];
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '\\' && q == '"' {
            i += 2;
            continue;
        }
        if chars[i] == q {
            return i + 1;
        }
        i += 1;
    }
    chars.len()
}

fn paint(cls: &mut [Tok], a: usize, b: usize, t: Tok) {
    for c in cls.iter_mut().take(b.min(a.max(b))).skip(a) {
        *c = t;
    }
}

fn lex_value(chars: &[char], cls: &mut [Tok], mut i: usize) {
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let e = lex_string(chars, i);
            paint(cls, i, e, Tok::Str);
            i = e;
        } else if c == '#' {
            paint(cls, i, chars.len(), Tok::Comment);
            return;
        } else if c.is_ascii_digit() || ((c == '-' || c == '+') && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())) {
            let mut e = i + 1;
            while e < chars.len() && (chars[e].is_ascii_alphanumeric() || matches!(chars[e], '.' | '_' | '-' | ':' | '+')) {
                e += 1;
            }
            paint(cls, i, e, Tok::Num);
            i = e;
        } else if c.is_alphabetic() {
            let mut e = i;
            while e < chars.len() && (chars[e].is_alphanumeric() || chars[e] == '_') {
                e += 1;
            }
            let w: String = chars[i..e].iter().collect();
            if matches!(w.as_str(), "true" | "false" | "null") {
                paint(cls, i, e, Tok::Kw);
            }
            i = e;
        } else if matches!(c, '{' | '}' | '[' | ']' | ',' | ':' | '=') {
            cls[i] = Tok::Punct;
            i += 1;
        } else {
            i += 1;
        }
    }
}

fn lex_json(chars: &[char], cls: &mut [Tok]) {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            let e = lex_string(chars, i);
            let mut k = e;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            paint(cls, i, e, if chars.get(k) == Some(&':') { Tok::Key } else { Tok::Str });
            i = e;
        } else {
            let mut e = i;
            while e < chars.len() && chars[e] != '"' {
                e += 1;
            }
            lex_value(&chars[i..e], &mut cls[i..e], 0);
            i = e;
        }
    }
}

fn lex_toml(chars: &[char], cls: &mut [Tok]) {
    let start = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(chars.len());
    if start >= chars.len() {
        return;
    }
    match chars[start] {
        '#' => paint(cls, start, chars.len(), Tok::Comment),
        '[' => {
            let mut e = start;
            while e < chars.len() && chars[e] != '#' {
                e += 1;
            }
            let hdr_end = chars[start..e].iter().rposition(|c| *c == ']').map(|p| start + p + 1).unwrap_or(e);
            paint(cls, start, hdr_end, Tok::Header);
            lex_value(chars, cls, hdr_end);
        }
        _ => {
            // key = value: the key ends at the first `=` outside quotes
            let mut i = start;
            let mut eq = None;
            while i < chars.len() {
                match chars[i] {
                    '"' | '\'' => i = lex_string(chars, i),
                    '=' => {
                        eq = Some(i);
                        break;
                    }
                    '#' => break,
                    _ => i += 1,
                }
            }
            match eq {
                Some(e) => {
                    let mut k = e;
                    while k > start && chars[k - 1].is_whitespace() {
                        k -= 1;
                    }
                    paint(cls, start, k, Tok::Key);
                    cls[e] = Tok::Punct;
                    lex_value(chars, cls, e + 1);
                }
                None => lex_value(chars, cls, start),
            }
        }
    }
}

// ---------------------------------------------------------------- the painted view

/// Everything the painter needs for one frame (a snapshot; the model stays in the app).
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// Control id of the whole editor (focusable, Enter starts typing).
    pub id: String,
    pub lang: Lang,
    pub lines: Rc<Vec<String>>,
    pub cursor: Pos,
    pub anchor: Option<Pos>,
    pub top: usize,
    pub left: f32,
    pub reveal: bool,
    /// 0-based line and message.
    pub errors: Vec<(usize, String)>,
    pub read_only: bool,
    /// Keyboard focus is in the editor.
    pub active: bool,
    pub modified: bool,
    /// Unscaled height of the whole widget.
    pub height: f32,
    pub status: String,
}

impl Editor {
    pub fn view(&self, id: &str, errors: &[(usize, String)], active: bool, height: f32, status: String) -> View {
        View {
            id: id.into(),
            lang: self.lang,
            lines: self.lines.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
            top: self.top,
            left: self.left,
            reveal: self.reveal,
            errors: errors.to_vec(),
            read_only: self.read_only,
            active,
            modified: self.is_modified(),
            height,
            status,
        }
    }

    /// Takes over what the painter settled on (scroll position, page size).
    pub fn sync(&mut self, g: &Geo) {
        self.top = g.top;
        self.left = g.left;
        self.page = g.page;
        self.reveal = false;
    }

    /// Document position under a point (window coordinates).
    pub fn hit(&self, p: &Painter, g: &Geo, x: f32, y: f32) -> Pos {
        let line = (g.top + ((y - g.text.1) / g.line_h).floor().max(0.0) as usize).min(self.lines.len() - 1);
        let rel = x - g.text.0 + g.left * p.s - PAD * p.s;
        let l = self.line(line);
        let mut acc = 0.0;
        let mut col = 0;
        for ch in l.chars() {
            let w = p.measure(&disp_char(ch), g.font_px, Weight::Regular);
            if acc + w / 2.0 > rel {
                break;
            }
            acc += w;
            col += 1;
        }
        Pos::new(line, col)
    }
}

fn disp_char(c: char) -> String {
    if c == '\t' {
        "    ".into()
    } else {
        c.to_string()
    }
}

fn disp(s: &str) -> String {
    s.replace('\t', "    ")
}

const PAD: f32 = 8.0;

/// What the painter measured, for hit testing and scroll bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geo {
    pub rect: Rect,
    /// The text area (without gutter and status strip).
    pub text: Rect,
    pub line_h: f32,
    pub font_px: f32,
    pub page: usize,
    pub top: usize,
    /// Unscaled horizontal scroll.
    pub left: f32,
    pub lines: usize,
}

pub fn gutter_digits(lines: usize) -> usize {
    lines.max(1).to_string().len().max(2)
}

fn tok_color(t: Tok) -> Color {
    let th = crate::theme::th();
    match t {
        Tok::Plain => pal::text(),
        Tok::Key => th.accent.over(th.text, 0.8).color(1.0),
        Tok::Str => th.ok.over(th.text, 0.85).color(1.0),
        Tok::Num => th.warn.color(1.0),
        Tok::Kw => th.danger.over(th.text, 0.55).color(1.0),
        Tok::Punct => pal::sub(),
        Tok::Comment => pal::faint(),
        Tok::Header => th.accent.color(1.0),
    }
}

/// Paints the editor into `r` and returns its hit-test entry and measurements.
pub fn draw(pm: &mut Pixmap, p: &mut Painter, v: &View, r: Rect, hover: bool) -> (Placed, Geo) {
    let s = p.s;
    let u = |x: f32| x * s;
    let (x, y, w, h) = r;
    let status_h = u(24.0);
    let line_h = u(18.0);
    let font_px = u(12.5);
    let digits = gutter_digits(v.lines.len());
    let gutter_w = p.measure(&"0".repeat(digits), u(11.5), Weight::Regular) + u(22.0);
    let text = (x + gutter_w, y + u(1.0), w - gutter_w - u(1.0), h - status_h - u(1.0));
    let page = ((text.3 / line_h).floor() as usize).max(1);
    let n = v.lines.len();
    let mut top = v.top.min(n.saturating_sub(1));
    let mut left_px = v.left * s;

    // frame
    fill_rounded(pm, x, y, w, h, u(8.0), pal::window());
    let (cur, sel) = (v.cursor, v.anchor.filter(|a| *a != v.cursor).map(|a| if a < v.cursor { (a, v.cursor) } else { (v.cursor, a) }));

    // scroll so the cursor shows, when asked
    let caret_px = |p: &Painter, pos: Pos| -> f32 {
        let l = v.lines.get(pos.line).map(String::as_str).unwrap_or("");
        let pre: String = l.chars().take(pos.col).collect();
        p.measure(&disp(&pre), font_px, Weight::Regular)
    };
    if v.reveal {
        if cur.line < top {
            top = cur.line;
        } else if cur.line >= top + page {
            top = cur.line + 1 - page;
        }
        let cx = caret_px(p, cur);
        let avail = text.2 - u(PAD) * 2.0;
        if cx < left_px {
            left_px = (cx - u(40.0)).max(0.0);
        } else if cx > left_px + avail - u(6.0) {
            left_px = cx - avail + u(40.0);
        }
    }
    top = top.min(n.saturating_sub(page));
    if left_px < 0.0 {
        left_px = 0.0;
    }

    // gutter
    fill_rounded(pm, x, y, gutter_w, h - status_h, u(8.0), crate::theme::th().ink(0.045));
    let have_err = |line: usize| v.errors.iter().any(|(l, _)| *l == line);

    // text layer (clipped by drawing it into its own pixmap)
    let (tw, th) = (text.2.max(1.0) as u32, text.3.max(1.0) as u32);
    if let Some(mut sub) = Pixmap::new(tw, th) {
        for row in 0..=page {
            let li = top + row;
            if li >= n {
                break;
            }
            let ly = row as f32 * line_h;
            if have_err(li) {
                crate::gfx::fill_rect(&mut sub, 0.0, ly, tw as f32, line_h, crate::theme::th().danger.color(0.16));
            } else if li == cur.line && v.active && sel.is_none() {
                crate::gfx::fill_rect(&mut sub, 0.0, ly, tw as f32, line_h, crate::theme::th().ink(0.05));
            }
            let line = &v.lines[li];
            // selection
            if let Some((a, b)) = sel {
                if li >= a.line && li <= b.line {
                    let len = line.chars().count();
                    let c0 = if li == a.line { a.col } else { 0 };
                    let c1 = if li == b.line { b.col } else { len };
                    let pre = |c: usize| -> f32 {
                        let t: String = line.chars().take(c).collect();
                        p.measure(&disp(&t), font_px, Weight::Regular)
                    };
                    let x0 = u(PAD) + pre(c0) - left_px;
                    let mut x1 = u(PAD) + pre(c1) - left_px;
                    if li < b.line {
                        x1 += u(6.0);
                    }
                    if x1 > x0 {
                        crate::gfx::fill_rect(&mut sub, x0, ly, x1 - x0, line_h, crate::theme::th().accent.color(0.38));
                    }
                }
            }
            // text in coloured runs
            let mut px = u(PAD) - left_px;
            let chars: Vec<char> = line.chars().collect();
            for (a, b, t) in tokens(v.lang, line) {
                let seg: String = chars[a..b].iter().collect();
                let seg = disp(&seg);
                let wseg = p.measure(&seg, font_px, Weight::Regular);
                if px + wseg >= 0.0 && px <= tw as f32 {
                    p.text(&mut sub, &seg, px, ly + line_h / 2.0, font_px, if t == Tok::Header { Weight::Medium } else { Weight::Regular }, tok_color(t));
                }
                px += wseg;
            }
            // wavy-ish underline for errors: a solid red line under the text
            if have_err(li) {
                crate::gfx::fill_rect(&mut sub, u(PAD) - left_px, ly + line_h - u(2.0), (px - (u(PAD) - left_px)).max(u(12.0)), u(1.4).max(1.0), pal::red());
            }
        }
        // caret
        if v.active && cur.line >= top && cur.line <= top + page {
            let cx = u(PAD) + caret_px(p, cur) - left_px;
            let cy = (cur.line - top) as f32 * line_h;
            crate::gfx::fill_rect(&mut sub, cx.round(), cy + u(2.0), u(1.5).max(1.0), line_h - u(4.0), pal::accent());
        }
        pm.draw_pixmap(text.0 as i32, text.1 as i32, sub.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }

    // line numbers
    for row in 0..=page {
        let li = top + row;
        if li >= n {
            break;
        }
        let cy = text.1 + row as f32 * line_h + line_h / 2.0;
        if cy > y + h - status_h {
            break;
        }
        let c = if have_err(li) {
            pal::red()
        } else if li == cur.line {
            pal::text()
        } else {
            pal::faint()
        };
        p.text_right(pm, &(li + 1).to_string(), x + gutter_w - u(10.0), cy, u(11.5), if li == cur.line { Weight::Medium } else { Weight::Regular }, c);
        if have_err(li) {
            crate::gfx::fill_circle(pm, x + u(7.0), cy, u(2.6), pal::red());
        }
    }

    // scrollbar
    if n > page {
        let track = text.3 - u(8.0);
        let th_ = (track * page as f32 / n as f32).max(u(24.0));
        let ty = text.1 + u(4.0) + (track - th_) * (top as f32 / (n - page) as f32);
        fill_rounded(pm, x + w - u(8.0), ty, u(4.0), th_, u(2.0), crate::theme::th().ink(0.25));
    }

    // status strip
    let sy = y + h - status_h;
    crate::gfx::fill_rect(pm, x + u(1.0), sy, w - u(2.0), u(1.0).max(1.0), pal::sep());
    let first_err = v.errors.first();
    let left_txt = format!("Ln {}, Col {}", cur.line + 1, cur.col + 1);
    p.text(pm, &left_txt, x + u(12.0), sy + status_h / 2.0, u(11.0), Weight::Regular, pal::sub());
    let mut rx = x + w - u(12.0);
    let right = if v.read_only {
        "read only".to_string()
    } else if v.modified {
        "modified".to_string()
    } else {
        "saved".to_string()
    };
    let col = if v.modified && !v.read_only { pal::orange() } else { pal::faint() };
    p.text_right(pm, &right, rx, sy + status_h / 2.0, u(11.0), Weight::Regular, col);
    rx -= p.measure(&right, u(11.0), Weight::Regular) + u(14.0);
    if !v.status.is_empty() {
        p.text_right(pm, &v.status, rx, sy + status_h / 2.0, u(11.0), Weight::Regular, pal::faint());
        rx -= p.measure(&v.status, u(11.0), Weight::Regular) + u(14.0);
    }
    if let Some((l, msg)) = first_err {
        let room = (rx - (x + u(120.0))).max(u(40.0));
        let t = p.fit(&format!("Line {}: {msg}", l + 1), room, u(11.0), Weight::Regular);
        p.text_right(pm, &t, rx, sy + status_h / 2.0, u(11.0), Weight::Regular, pal::red());
    }

    // border
    if v.active {
        stroke_rounded(pm, x, y, w, h, u(8.0), pal::accent(), u(1.5));
    } else {
        stroke_rounded(pm, x + 0.5, y + 0.5, w - 1.0, h - 1.0, u(8.0), if hover { crate::theme::th().ink(0.28) } else { crate::theme::th().ink(0.14) }, 1.0);
    }
    let _ = color;

    let geo = Geo { rect: r, text, line_h, font_px, page, top, left: left_px / s, lines: n };
    let placed = Placed { rect: r, ctl: super::ui::Ctl::new(v.id.clone(), super::ui::Kind::Button { label: "Editor".into(), danger: false }), parts: vec![], row_rect: r, label: "Editor".into() };
    (placed, geo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(t: &str) -> Editor {
        Editor::new(t, Lang::Json)
    }

    fn type_str(e: &mut Editor, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                e.newline();
            } else {
                e.insert_str(&c.to_string());
            }
        }
    }

    #[test]
    fn insert_delete_and_lines() {
        let mut e = ed("abc\ndef");
        assert_eq!(e.line_count(), 2);
        e.set_cursor(Pos::new(0, 3), false);
        e.insert_str("X");
        assert_eq!(e.text(), "abcX\ndef");
        e.insert_str("1\n2");
        assert_eq!(e.text(), "abcX1\n2\ndef");
        assert_eq!(e.cursor, Pos::new(1, 1));
        assert_eq!(e.line_count(), 3);
        e.backspace();
        e.backspace();
        assert_eq!(e.text(), "abcX1\ndef", "the second backspace joins the lines");
        e.delete();
        assert_eq!(e.text(), "abcX1def");
        e.set_cursor(Pos::new(0, 99), false);
        assert_eq!(e.cursor, Pos::new(0, 8), "the cursor is clamped");
        e.delete();
        assert_eq!(e.text(), "abcX1def");
        e.insert_str("\u{7}\r\nz");
        assert_eq!(e.text(), "abcX1def\nz", "control characters dropped, CRLF normalised");
        assert_eq!(Editor::new("a\r\nb", Lang::Plain).text(), "a\nb");
        assert_eq!(Editor::new("", Lang::Plain).line_count(), 1);
        assert_eq!(Editor::new("a\n", Lang::Plain).line_count(), 2, "a trailing newline gives an empty last line");
        assert_eq!(Editor::new("a\n", Lang::Plain).text(), "a\n");
    }

    #[test]
    fn multibyte_text() {
        let mut e = ed("café ñ");
        e.set_cursor(Pos::new(0, 4), false);
        e.backspace();
        assert_eq!(e.text(), "caf ñ");
        e.end(false);
        assert_eq!(e.cursor.col, 5);
        e.insert_str("日本");
        assert_eq!(e.text(), "caf ñ日本");
        e.left(true);
        assert_eq!(e.selected_text().as_deref(), Some("本"));
    }

    #[test]
    fn selection_and_replacing_it() {
        let mut e = ed("one two three");
        e.set_cursor(Pos::new(0, 4), false);
        e.word_right(true);
        assert_eq!(e.selected_text().as_deref(), Some("two "));
        e.insert_str("2 ");
        assert_eq!(e.text(), "one 2 three");
        e.select_all();
        assert_eq!(e.selected_text().as_deref(), Some("one 2 three"));
        assert_eq!(e.cut().as_deref(), Some("one 2 three"));
        assert_eq!(e.text(), "");
        e.undo();
        assert_eq!(e.text(), "one 2 three");
        assert_eq!(e.selection(), Some((Pos::new(0, 0), Pos::new(0, 11))), "undo restores the selection");
        // multi-line selection
        let mut e = ed("aa\nbb\ncc");
        e.set_cursor(Pos::new(0, 1), false);
        e.set_cursor(Pos::new(2, 1), true);
        assert_eq!(e.selected_text().as_deref(), Some("a\nbb\nc"));
        e.backspace();
        assert_eq!(e.text(), "ac");
        // left/right collapse the selection to its edges
        let mut e = ed("hello");
        e.set_cursor(Pos::new(0, 1), false);
        e.set_cursor(Pos::new(0, 4), true);
        e.left(false);
        assert_eq!((e.cursor, e.selection()), (Pos::new(0, 1), None));
        e.set_cursor(Pos::new(0, 4), true);
        e.right(false);
        assert_eq!(e.cursor, Pos::new(0, 4));
        // double click and triple click
        let mut e = ed("foo_bar = 12");
        e.set_cursor(Pos::new(0, 2), false);
        e.select_word();
        assert_eq!(e.selected_text().as_deref(), Some("foo_bar"));
        e.select_line();
        assert_eq!(e.selected_text().as_deref(), Some("foo_bar = 12"));
    }

    #[test]
    fn undo_and_redo_group_typing() {
        let mut e = ed("");
        assert!(!e.is_modified());
        type_str(&mut e, "hello world");
        assert!(e.is_modified());
        assert_eq!(e.text(), "hello world");
        assert!(e.undo(), "undo removes the last word run");
        assert_eq!(e.text(), "hello ");
        e.undo();
        assert_eq!(e.text(), "hello");
        e.undo();
        assert_eq!(e.text(), "");
        assert!(!e.can_undo() && !e.is_modified());
        assert!(e.redo());
        assert_eq!(e.text(), "hello");
        e.redo();
        e.redo();
        assert_eq!(e.text(), "hello world");
        assert!(!e.redo());
        // a new edit drops the redo history
        e.undo();
        e.insert_str("!");
        assert!(!e.can_redo());
        // backspacing a run is one step
        let mut e = ed("abcdef");
        e.end(false);
        for _ in 0..3 {
            e.backspace();
        }
        assert_eq!(e.text(), "abc");
        e.undo();
        assert_eq!(e.text(), "abcdef");
        assert_eq!(e.cursor, Pos::new(0, 6));
        // forward deletes too
        let mut e = ed("abcdef");
        e.delete();
        e.delete();
        assert_eq!(e.text(), "cdef");
        e.undo();
        assert_eq!(e.text(), "abcdef");
        // undo across lines
        let mut e = ed("a");
        e.end(false);
        e.newline();
        e.insert_str("b");
        e.undo();
        e.undo();
        assert_eq!(e.text(), "a");
    }

    #[test]
    fn modified_flag_follows_the_saved_state() {
        let mut e = ed("x");
        e.end(false);
        e.insert_str("y");
        assert!(e.is_modified());
        e.mark_saved();
        assert!(!e.is_modified());
        e.insert_str("z");
        assert!(e.is_modified(), "typing after a save starts a new undo step");
        e.undo();
        assert!(!e.is_modified(), "undo back to the saved text");
        e.undo();
        assert!(e.is_modified());
        e.redo();
        assert!(!e.is_modified());
        e.set_text("fresh");
        assert!(!e.is_modified() && !e.can_undo());
    }

    #[test]
    fn newline_keeps_and_adds_indentation() {
        let mut e = ed("{\n  \"a\": 1");
        e.doc_end(false);
        e.newline();
        assert_eq!(e.cursor, Pos::new(2, 2));
        let mut e = ed("{");
        e.end(false);
        e.newline();
        assert_eq!(e.text(), "{\n  ");
        let mut e = ed("[");
        e.end(false);
        e.newline();
        assert_eq!(e.cursor, Pos::new(1, 2));
    }

    #[test]
    fn tab_indents_and_shift_tab_dedents() {
        let mut e = ed("a\nb\nc");
        e.indent();
        assert_eq!(e.text(), "  a\nb\nc");
        e.set_cursor(Pos::new(0, 0), false);
        e.set_cursor(Pos::new(2, 1), true);
        e.indent();
        assert_eq!(e.text(), "    a\n  b\n  c");
        e.dedent();
        assert_eq!(e.text(), "  a\nb\nc");
        // backspace in leading spaces removes a whole indent level
        let mut e = ed("    x");
        e.set_cursor(Pos::new(0, 4), false);
        e.backspace();
        assert_eq!(e.text(), "  x");
        // cut/paste
        let mut e = ed("abc");
        e.select_all();
        let t = e.cut().unwrap();
        e.paste(&format!("{t}{t}"));
        assert_eq!(e.text(), "abcabc");
    }

    #[test]
    fn cursor_movement_and_word_jumps() {
        let mut e = ed("alpha beta\n\nz");
        e.word_right(false);
        assert_eq!(e.cursor, Pos::new(0, 6));
        e.word_right(false);
        assert_eq!(e.cursor, Pos::new(0, 10));
        e.word_right(false);
        assert_eq!(e.cursor, Pos::new(1, 0));
        e.word_left(false);
        assert_eq!(e.cursor, Pos::new(0, 10));
        e.word_left(false);
        assert_eq!(e.cursor, Pos::new(0, 6));
        // vertical keeps the wanted column over a short line
        let mut e = ed("abcdef\nab\nabcdef");
        e.set_cursor(Pos::new(0, 5), false);
        e.down(false);
        assert_eq!(e.cursor, Pos::new(1, 2));
        e.down(false);
        assert_eq!(e.cursor, Pos::new(2, 5));
        e.down(false);
        assert_eq!(e.cursor, Pos::new(2, 6), "the last line's end");
        e.up(false);
        e.up(false);
        e.up(false);
        e.up(false);
        assert_eq!(e.cursor, Pos::new(0, 0));
        // smart home
        let mut e = ed("    x");
        e.end(false);
        e.home(false);
        assert_eq!(e.cursor.col, 4);
        e.home(false);
        assert_eq!(e.cursor.col, 0);
        // paging
        let text = (0..100).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let mut e = ed(&text);
        e.page = 10;
        e.page_down(false);
        assert_eq!(e.cursor.line, 9);
        e.page_up(false);
        assert_eq!(e.cursor.line, 0);
        e.doc_end(true);
        assert_eq!(e.selection().unwrap().1.line, 99);
    }

    #[test]
    fn read_only_ignores_edits_but_selects() {
        let mut e = ed("abc");
        e.read_only = true;
        e.insert_str("x");
        e.end(false);
        e.backspace();
        e.newline();
        assert_eq!(e.text(), "abc");
        e.select_all();
        assert_eq!(e.cut().as_deref(), Some("abc"));
        assert_eq!(e.text(), "abc");
    }

    #[test]
    fn replace_all_is_one_undo_step() {
        let mut e = ed("{\"a\":1}");
        e.replace_all("{\n  \"a\": 1\n}");
        assert_eq!(e.line_count(), 3);
        e.undo();
        assert_eq!(e.text(), "{\"a\":1}");
        assert_eq!(e.line_count(), 1);
    }

    #[test]
    fn line_numbers_width() {
        assert_eq!(gutter_digits(1), 2);
        assert_eq!(gutter_digits(99), 2);
        assert_eq!(gutter_digits(100), 3);
        assert_eq!(gutter_digits(12345), 5);
    }

    #[test]
    fn json_and_toml_colouring() {
        let t = tokens(Lang::Json, "  \"name\": \"x\", \"n\": -1.5e3, \"ok\": true");
        let kinds: Vec<Tok> = t.iter().map(|x| x.2).collect();
        assert!(kinds.contains(&Tok::Key) && kinds.contains(&Tok::Str) && kinds.contains(&Tok::Num) && kinds.contains(&Tok::Kw));
        let key = t.iter().find(|x| x.2 == Tok::Key).unwrap();
        assert_eq!((key.0, key.1), (2, 8));
        // runs cover the whole line without gaps
        let line = "  \"a\": [1, null],";
        let t = tokens(Lang::Json, line);
        assert_eq!(t.first().unwrap().0, 0);
        assert_eq!(t.last().unwrap().1, line.chars().count());
        assert!(t.windows(2).all(|w| w[0].1 == w[1].0));
        let toml = tokens(Lang::Toml, "[[bind]] # keys");
        assert_eq!(toml[0].2, Tok::Header);
        assert_eq!(toml.last().unwrap().2, Tok::Comment);
        let toml = tokens(Lang::Toml, "border_px = 2  # px");
        assert_eq!(toml[0], (0, 9, Tok::Key));
        assert!(toml.iter().any(|x| x.2 == Tok::Num) && toml.last().unwrap().2 == Tok::Comment);
        assert_eq!(tokens(Lang::Toml, "# only a comment"), vec![(0, 16, Tok::Comment)]);
        assert_eq!(tokens(Lang::Toml, "image = \"a # b\"").iter().filter(|x| x.2 == Tok::Comment).count(), 0, "a # inside a string is not a comment");
        assert!(tokens(Lang::Toml, "key = 'lit'").iter().any(|x| x.2 == Tok::Str));
        assert!(tokens(Lang::Json, "").is_empty() && tokens(Lang::Toml, "   ").is_empty());
        // an unterminated string does not panic
        let _ = tokens(Lang::Json, "\"abc");
        let _ = tokens(Lang::Toml, "k = \"abc");
        let _ = tokens(Lang::Toml, "[table");
        assert_eq!(tokens(Lang::Plain, "x = 1").len(), 1);
    }

    #[test]
    fn painting_and_hit_testing() {
        let mut e = ed(&(0..60).map(|i| format!("  \"key{i}\": {i},")).collect::<Vec<_>>().join("\n"));
        e.set_cursor(Pos::new(40, 5), false);
        let v = e.view("cfg:editor", &[(3, "bad".into())], true, 300.0, "JSON".into());
        let mut pm = Pixmap::new(700, 400).unwrap();
        let mut p = Painter { fonts: None, s: 1.0 };
        let (placed, g) = draw(&mut pm, &mut p, &v, (20.0, 20.0, 640.0, 300.0), false);
        assert_eq!(placed.ctl.id, "cfg:editor");
        assert!(g.page > 10 && g.page < 20, "{g:?}");
        assert!(g.top + g.page > 40 && g.top <= 40, "the cursor was scrolled into view: {g:?}");
        assert!(pm.data().chunks_exact(4).any(|px| px[3] > 0));
        e.sync(&g);
        assert_eq!(e.top, g.top);
        assert!(!e.reveal);
        // clicking the first visible line's start puts the cursor there
        let pos = e.hit(&p, &g, g.text.0 + 1.0, g.text.1 + 2.0);
        assert_eq!(pos, Pos::new(g.top, 0));
        let pos = e.hit(&p, &g, g.text.0 + 5000.0, g.text.1 + g.line_h * 2.5);
        assert_eq!(pos.line, g.top + 2);
        assert_eq!(pos.col, e.line(g.top + 2).chars().count(), "past the end of a line");
        let pos = e.hit(&p, &g, g.text.0 + 10.0, g.text.1 + 99999.0);
        assert_eq!(pos.line, e.line_count() - 1, "below the text");
    }
}
