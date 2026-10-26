//! X11/xkb keysym helpers shared by the X11 and Wayland window backends (keysym numbering is the
//! same in both).

use crate::window_app::{Key, KeyEvent};

/// Printable character for a keysym (Latin-1 range, keypad and Unicode keysyms).
pub fn keysym_char(sym: u32) -> Option<char> {
    match sym {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(sym),
        0xffb0..=0xffb9 => char::from_u32(u32::from(b'0') + (sym - 0xffb0)), // keypad digits
        0xffaa => Some('*'),
        0xffab => Some('+'),
        0xffad => Some('-'),
        0xffae => Some('.'),
        0xffaf => Some('/'),
        0x100_0100..=0x110_ffff => char::from_u32(sym - 0x100_0000),
        _ => None,
    }
}

/// The keysym that produces `c`, if it has one.
pub fn char_keysym(c: char) -> Option<u32> {
    let cp = c as u32;
    match cp {
        0x20..=0x7e | 0xa0..=0xff => Some(cp),
        0x100..=0x10ffff => Some(0x100_0000 + cp),
        _ => None,
    }
}

/// Maps a keysym to a [`Key`]; `None` for modifier keys and anything unrecognised.
pub fn keysym_key(sym: u32) -> Option<Key> {
    Some(match sym {
        0xff1b => Key::Escape,
        0xff0d | 0xff8d => Key::Enter,
        0xff08 => Key::Backspace,
        0xffff | 0xff9f => Key::Delete,
        0xff51 | 0xff96 => Key::Left,
        0xff52 | 0xff97 => Key::Up,
        0xff53 | 0xff98 => Key::Right,
        0xff54 | 0xff99 => Key::Down,
        0xff55 | 0xff9a => Key::PageUp,
        0xff56 | 0xff9b => Key::PageDown,
        0xff50 | 0xff95 => Key::Home,
        0xff57 | 0xff9c => Key::End,
        0xff09 | 0xfe20 => Key::Tab,
        s if keysym_char(s).is_some() => Key::Char,
        _ => return None,
    })
}

/// Builds a [`KeyEvent`].
///
/// * `sym` – the keysym with modifiers applied (what the user typed),
/// * `plain_sym` – the unmodified keysym (for Ctrl shortcuts),
/// * `utf8` – the layout/compose-aware text when the platform provides it, else derived from `sym`,
/// * `shift`/`ctrl`/`alt` – modifier state.
pub fn key_event(
    sym: u32,
    plain_sym: u32,
    utf8: Option<&str>,
    shift: bool,
    ctrl: bool,
    alt: bool,
) -> Option<KeyEvent> {
    let key = keysym_key(sym)?;
    let ch = keysym_char(plain_sym).map(|c| c.to_ascii_lowercase());
    let text = if key == Key::Char && !ctrl && !alt {
        match utf8 {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => keysym_char(sym).map(String::from).unwrap_or_default(),
        }
    } else {
        String::new()
    };
    Some(KeyEvent {
        key,
        text,
        ch,
        ctrl,
        shift,
        alt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysym_to_char() {
        assert_eq!(keysym_char(0x61), Some('a'));
        assert_eq!(keysym_char(0xe9), Some('é'));
        assert_eq!(keysym_char(0x100_20ac), Some('€'));
        assert_eq!(keysym_char(0xffb5), Some('5'), "keypad digit");
        assert_eq!(keysym_char(0xff0d), None);
        assert_eq!(keysym_char(0xffe1), None, "Shift_L has no character");
    }

    #[test]
    fn char_and_keysym_roundtrip() {
        for c in ['a', 'Z', '5', 'é', '€', 'ñ'] {
            assert_eq!(keysym_char(char_keysym(c).unwrap()), Some(c));
        }
        assert_eq!(char_keysym('\n'), None);
    }

    #[test]
    fn navigation_keys() {
        assert_eq!(keysym_key(0xff1b), Some(Key::Escape));
        assert_eq!(keysym_key(0xff0d), Some(Key::Enter));
        assert_eq!(keysym_key(0xff52), Some(Key::Up));
        assert_eq!(keysym_key(0xff56), Some(Key::PageDown));
        assert_eq!(keysym_key(0xffe1), None);
        assert_eq!(keysym_key(0x41), Some(Key::Char));
    }

    #[test]
    fn typed_text_and_shortcuts() {
        let e = key_event(0x41, 0x61, None, true, false, false).unwrap(); // Shift+a
        assert_eq!((e.text.as_str(), e.ch, e.shift), ("A", Some('a'), true));
        let e = key_event(0x61, 0x61, Some("\u{1}"), false, true, false).unwrap(); // Ctrl+a: shortcut, no text
        assert_eq!((e.text.as_str(), e.ch, e.ctrl), ("", Some('a'), true));
        let e = key_event(0xff08, 0xff08, None, false, false, false).unwrap();
        assert_eq!((e.key, e.text.as_str()), (Key::Backspace, ""));
        assert!(key_event(0xffe1, 0xffe1, None, false, false, false).is_none());
    }

    #[test]
    fn composed_text_wins_over_keysym() {
        let e = key_event(0x65, 0x65, Some("é"), false, false, false).unwrap();
        assert_eq!(e.text, "é");
    }
}
