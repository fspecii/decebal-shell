//! Follow the shell's existing GTK app integration without reading Decebal config:
//! the shell writes the palette (named colours and CSS variables) and, for Files, a
//! `/* Files visual profile: <id> (native file operations) */` marker into the user's
//! `gtk-4.0/gtk.css`. decfiles reads that file (asynchronously, read-only) to pick its
//! profile; explicit choices (`--profile`, an explicit layout) always win.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Profile {
    #[default]
    Native,
    Explorer,
    Finder,
}

const MARKER: &str = "/* Files visual profile: ";

impl Profile {
    /// The shell's `file_manager::Style::id` vocabulary.
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "native" => Some(Self::Native),
            "explorer" => Some(Self::Explorer),
            "finder" => Some(Self::Finder),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Explorer => "explorer",
            Self::Finder => "finder",
        }
    }

    /// The last profile marker in the generated stylesheet; none means Native.
    pub fn from_generated_css(text: &str) -> Self {
        text.match_indices(MARKER)
            .filter_map(|(index, _)| {
                let rest = &text[index + MARKER.len()..];
                let id = rest.split(|c: char| c.is_whitespace() || c == '*').next()?;
                Self::parse(id)
            })
            .last()
            .unwrap_or_default()
    }

    /// Default layout when the user has not chosen one: Finder opens hierarchical columns.
    pub fn prefers_columns(self) -> bool {
        self == Self::Finder
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Native => "decfiles-native",
            Self::Explorer => "decfiles-explorer",
            Self::Finder => "decfiles-finder",
        }
    }

    /// Column titles in each platform's own vocabulary.
    pub fn titles(self) -> [&'static str; 4] {
        match self {
            Self::Finder => ["Name", "Kind", "Size", "Date Modified"],
            _ => ["Name", "Type", "Size", "Date modified"],
        }
    }
}

/// Profile surfaces scoped to the window class. Colours come only from the palette's
/// CSS variables (with neutral fallbacks); no padding/margin changes, so hit targets
/// and layout stay identical across profiles. The user's gtk.css (higher priority) wins.
pub fn css() -> &'static str {
    concat!(
        "window.decfiles-explorer .decfiles-sidebar { background-color: var(--sidebar-bg-color, alpha(currentColor, 0.04)); color: var(--sidebar-fg-color, currentColor); }\n",
        "window.decfiles-explorer .decfiles-sidebar button { border-radius: 2px; }\n",
        "window.decfiles-explorer columnview > header > button { border-radius: 0; background-color: var(--sidebar-bg-color, alpha(currentColor, 0.04)); color: var(--sidebar-fg-color, currentColor); }\n",
        "window.decfiles-explorer columnview > listview > row { border-radius: 2px; }\n",
        "window.decfiles-explorer .decfiles-tabbar button { border-radius: 2px; }\n",
        "window.decfiles-explorer .decfiles-commandbar { box-shadow: inset 0 1px alpha(currentColor, 0.12); }\n",
        "window.decfiles-finder .decfiles-sidebar { background-color: var(--sidebar-bg-color, alpha(currentColor, 0.05)); color: var(--sidebar-fg-color, currentColor); }\n",
        "window.decfiles-finder .decfiles-sidebar button { border-radius: 7px; font-weight: 600; }\n",
        "window.decfiles-finder columnview > listview > row:nth-child(even):not(:selected) { background-color: alpha(currentColor, 0.035); }\n",
        "window.decfiles-finder listview > row { border-radius: 6px; }\n",
        "window.decfiles-finder .decfiles-tabbar button { border-radius: 7px; }\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_profile_marker_and_defaults_to_native() {
        let generated = "/* Graphite: dark */\n@define-color window_bg_color #20242b;\n/* Files visual profile: finder (native file operations) */\n.nautilus-window headerbar {}\n";
        assert_eq!(Profile::from_generated_css(generated), Profile::Finder);
        assert_eq!(Profile::from_generated_css("/* Files visual profile: explorer (native file operations) */"), Profile::Explorer);
        assert_eq!(Profile::from_generated_css("@define-color window_bg_color #fff;"), Profile::Native);
        assert_eq!(Profile::from_generated_css("/* Files visual profile: something-else */"), Profile::Native);
        assert!(Profile::Finder.prefers_columns() && !Profile::Explorer.prefers_columns() && !Profile::Native.prefers_columns());
        for profile in [Profile::Native, Profile::Explorer, Profile::Finder] {
            assert_eq!(Profile::parse(profile.id()), Some(profile));
        }
        for forbidden in ["padding", "margin", "min-width", "min-height", "#"] {
            assert!(!css().contains(forbidden), "{forbidden}");
        }
    }

    /// The marker is the shell's contract: its generator must still emit exactly this
    /// form with the same profile ids, or this test fails instead of drifting silently.
    #[test]
    fn marker_matches_the_shell_generator_source() {
        let shell = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/theme/system/file_manager.rs");
        let Ok(source) = std::fs::read_to_string(&shell) else {
            eprintln!("shell source not present; skipping contract check");
            return;
        };
        assert!(source.contains("/* Files visual profile: {} (native file operations) */"), "shell marker format changed");
        for id in ["\"explorer\"", "\"finder\"", "\"native\""] {
            assert!(source.contains(id), "shell profile id {id} missing");
        }
        let generated = format!("/* Files visual profile: {} (native file operations) */", "finder");
        assert_eq!(Profile::from_generated_css(&generated), Profile::Finder);
    }
}
