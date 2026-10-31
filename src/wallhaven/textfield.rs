//! A single-line text field model (editing only; drawing lives in the UI).

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TextField {
    text: String,
    /// Cursor position in characters.
    cursor: usize,
    /// The other end of a selection (characters); `None` = nothing selected. Typing replaces the selection.
    anchor: Option<usize>,
}

impl TextField {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_string(), cursor: text.chars().count(), anchor: None }
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The selected range (start < end, characters), if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        let (s, e) = (a.min(self.cursor), a.max(self.cursor));
        (s < e).then_some((s, e))
    }

    /// Selects `start..end` (clamped) with the cursor at `end`; an empty range clears the selection.
    pub fn set_selection(&mut self, start: usize, end: usize) {
        let n = self.len();
        let (s, e) = (start.min(n), end.min(n));
        self.cursor = e;
        self.anchor = (s != e).then_some(s);
    }

    /// Moves the cursor (clamped) and drops any selection.
    pub fn set_cursor(&mut self, at: usize) {
        self.cursor = at.min(self.len());
        self.anchor = None;
    }

    /// Shift+arrow style: move the cursor to `to` (clamped), keeping the selection's anchor.
    pub fn extend(&mut self, to: usize) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = to.min(self.len());
    }

    pub fn selected_text(&self) -> Option<String> {
        let (s, e) = self.selection()?;
        Some(self.text.chars().skip(s).take(e - s).collect())
    }

    /// Deletes the selection; true when there was one.
    pub fn delete_selection(&mut self) -> bool {
        let Some((s, e)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let (a, b) = (self.byte_at(s), self.byte_at(e));
        self.text.replace_range(a..b, "");
        self.cursor = s;
        self.anchor = None;
        true
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.chars().count();
        self.anchor = None;
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    /// Inserts printable text at the cursor; control characters are dropped.
    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        self.delete_selection();
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let (a, b) = (self.byte_at(self.cursor - 1), self.byte_at(self.cursor));
        self.text.replace_range(a..b, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor >= self.text.chars().count() {
            return;
        }
        let (a, b) = (self.byte_at(self.cursor), self.byte_at(self.cursor + 1));
        self.text.replace_range(a..b, "");
    }

    pub fn left(&mut self) {
        self.cursor = match self.selection() {
            Some((s, _)) => s,
            None => self.cursor.saturating_sub(1),
        };
        self.anchor = None;
    }

    pub fn right(&mut self) {
        self.cursor = match self.selection() {
            Some((_, e)) => e,
            None => (self.cursor + 1).min(self.text.chars().count()),
        };
        self.anchor = None;
    }

    pub fn home(&mut self) {
        self.cursor = 0;
        self.anchor = None;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.chars().count();
        self.anchor = None;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
    }

    /// Text before the cursor (used to place the caret).
    pub fn before_cursor(&self) -> &str {
        &self.text[..self.byte_at(self.cursor)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_replaced_by_typing_and_collapsed_by_arrows() {
        let mut f = TextField::new("hello world");
        f.set_selection(6, 11);
        assert_eq!(f.selection(), Some((6, 11)));
        assert_eq!(f.selected_text().as_deref(), Some("world"));
        f.insert("there");
        assert_eq!((f.text(), f.cursor(), f.selection()), ("hello there", 11, None));
        f.set_selection(0, 5);
        f.left();
        assert_eq!((f.cursor(), f.selection()), (0, None), "Left collapses to the start");
        f.extend(3);
        assert_eq!(f.selection(), Some((0, 3)));
        f.backspace();
        assert_eq!(f.text(), "lo there");
        f.set_selection(2, 2);
        assert_eq!(f.selection(), None);
        f.set_cursor(99);
        assert_eq!(f.cursor(), 8);
    }

    #[test]
    fn typing_and_editing() {
        let mut f = TextField::default();
        f.insert("forest");
        f.left();
        f.left();
        f.insert("X");
        assert_eq!(f.text(), "foreXst");
        f.backspace();
        assert_eq!(f.text(), "forest");
        f.delete();
        assert_eq!(f.text(), "foret");
        f.home();
        f.backspace(); // no-op at start
        f.delete();
        assert_eq!(f.text(), "oret");
        f.end();
        assert_eq!(f.cursor(), 4);
    }

    #[test]
    fn handles_multibyte() {
        let mut f = TextField::new("café");
        f.left();
        f.backspace();
        assert_eq!(f.text(), "caé");
        f.insert("ñ");
        assert_eq!(f.text(), "cañé");
        assert_eq!(f.before_cursor(), "cañ");
    }

    #[test]
    fn control_chars_are_dropped() {
        let mut f = TextField::default();
        f.insert("a\nb\t\u{7}c");
        assert_eq!(f.text(), "abc");
        f.insert("");
        assert_eq!(f.text(), "abc");
    }

    #[test]
    fn set_and_clear() {
        let mut f = TextField::new("x");
        f.set("hello");
        assert_eq!(f.cursor(), 5);
        f.clear();
        assert_eq!(f.text(), "");
        assert_eq!(f.cursor(), 0);
    }
}
