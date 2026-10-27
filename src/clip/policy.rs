//! What the daemon refuses to record: nothing that looks like a secret, nothing blank, nothing huge.

use super::history::MAX_BYTES;

/// A selection owner (or the focused window) whose `WM_CLASS` contains one of these is a password manager.
pub const SECRET_CLASSES: [&str; 4] = ["1password", "bitwarden", "keepassxc", "keepass"];

/// Selection target that password managers offer next to the text to say "do not keep this" (KDE convention,
/// set by KeePassXC, Bitwarden and others).
pub const HINT_TARGET: &str = "x-kde-passwordManagerHint";

/// True when any of the class / instance names belongs to a password manager (case-insensitive substring).
pub fn class_is_secret<S: AsRef<str>>(names: &[S]) -> bool {
    names.iter().any(|n| {
        let n = n.as_ref().to_lowercase();
        SECRET_CLASSES.iter().any(|c| n.contains(c))
    })
}

/// True when the selection's target list carries the password-manager hint.
pub fn targets_have_hint<S: AsRef<str>>(names: &[S]) -> bool {
    names.iter().any(|n| n.as_ref().eq_ignore_ascii_case(HINT_TARGET))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Store,
    /// Whitespace only (or empty).
    Blank,
    /// Larger than [`MAX_BYTES`].
    TooBig,
    /// Came from a password manager.
    Secret,
}

/// Decides whether a clipboard text is recorded. `classes` are the owner's (and the focused window's) class names,
/// `targets` the names the selection offered.
pub fn judge<S: AsRef<str>, T: AsRef<str>>(text: &str, classes: &[S], targets: &[T]) -> Verdict {
    if class_is_secret(classes) || targets_have_hint(targets) {
        Verdict::Secret
    } else if text.trim().is_empty() {
        Verdict::Blank
    } else if text.len() > MAX_BYTES {
        Verdict::TooBig
    } else {
        Verdict::Store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NONE: [&str; 0] = [];

    #[test]
    fn password_manager_classes_are_recognised() {
        for c in ["1Password", "1password", "Bitwarden", "KeePassXC", "keepassxc", "KeePass2", "org.keepassxc.KeePassXC"] {
            assert!(class_is_secret(&[c]), "{c}");
        }
        assert!(class_is_secret(&["Navigator", "1password"]), "any of the names");
        for c in ["firefox", "Alacritty", "", "pass"] {
            assert!(!class_is_secret(&[c]), "{c}");
        }
        assert!(!class_is_secret(&NONE));
    }

    #[test]
    fn the_password_manager_hint_target_is_recognised() {
        assert!(targets_have_hint(&["TARGETS", "UTF8_STRING", "x-kde-passwordManagerHint"]));
        assert!(!targets_have_hint(&["TARGETS", "UTF8_STRING"]));
    }

    #[test]
    fn verdicts() {
        assert_eq!(judge("hello", &["firefox"], &["UTF8_STRING"]), Verdict::Store);
        assert_eq!(judge("hello", &["KeePassXC"], &NONE), Verdict::Secret);
        assert_eq!(judge("hello", &NONE, &["x-kde-passwordManagerHint"]), Verdict::Secret);
        assert_eq!(judge("  \n\t ", &NONE, &NONE), Verdict::Blank);
        assert_eq!(judge("", &NONE, &NONE), Verdict::Blank);
        assert_eq!(judge(&"x".repeat(MAX_BYTES), &NONE, &NONE), Verdict::Store);
        assert_eq!(judge(&"x".repeat(MAX_BYTES + 1), &NONE, &NONE), Verdict::TooBig);
        assert_eq!(judge("   ", &["1password"], &NONE), Verdict::Secret, "secret wins");
    }
}
