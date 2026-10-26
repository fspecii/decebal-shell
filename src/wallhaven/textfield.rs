//! A single-line text field model (editing only; drawing lives in the UI).

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TextField {
    text: String,
    /// Cursor position in characters.
    cursor: usize,
}

impl TextField {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: text.chars().count(),
        }
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
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    /// Inserts printable text at the cursor; control characters are dropped.
    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let (a, b) = (self.byte_at(self.cursor - 1), self.byte_at(self.cursor));
        self.text.replace_range(a..b, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.text.chars().count() {
            return;
        }
        let (a, b) = (self.byte_at(self.cursor), self.byte_at(self.cursor + 1));
        self.text.replace_range(a..b, "");
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
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
