//! Visual familiarity for modern GTK 4 Nautilus, never a replacement file manager.
//! Upstream defines `.nautilus-window` in nautilus-window.c, `placessidebar`
//! in gtk/nautilusgtkplacessidebar.c, and view selectors in resources/style.css.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    FollowTheme,
    Native,
    Explorer,
    Finder,
}

impl Style {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "theme" | "follow-theme" => Some(Self::FollowTheme),
            "native" => Some(Self::Native),
            "explorer" => Some(Self::Explorer),
            "finder" => Some(Self::Finder),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::FollowTheme => "theme",
            Self::Native => "native",
            Self::Explorer => "explorer",
            Self::Finder => "finder",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::FollowTheme => "Follow theme",
            Self::Native => "Native Files",
            Self::Explorer => "Windows Explorer inspired",
            Self::Finder => "macOS Finder inspired",
        }
    }

    /// Resolve an automatic preference against the applied theme identity, never its palette
    /// or button placement: unrelated rice themes often share macOS window buttons.
    pub fn resolve(self, theme_id: &str) -> Self {
        if self != Self::FollowTheme {
            return self;
        }
        match theme_id {
            "windows11" | "frutiger-aero" => Self::Explorer,
            "macos" | "graphite" => Self::Finder,
            _ => Self::Native,
        }
    }
}

/// Profile geometry and surface hierarchy, using only the current GTK palette.
/// Avoid changing column padding/margins: Nautilus uses these for hit testing.
pub fn css(style: Style) -> String {
    let (radius, sidebar_radius, row_gap, cell_padding, header_padding, grid_inset) = match style {
        Style::Native | Style::FollowTheme => return String::new(),
        Style::Explorer => (2, 2, 1, 3, 4, 8),
        Style::Finder => (7, 8, 3, 5, 6, 12),
    };
    let mut sheet = format!(
        "/* Files visual profile: {} (native file operations) */\n\
.nautilus-window headerbar {{ padding-top: {header_padding}px; padding-bottom: {header_padding}px; }}\n\
.nautilus-window placessidebar row {{ border-radius: {sidebar_radius}px; margin-top: {row_gap}px; margin-bottom: {row_gap}px; }}\n\
.nautilus-window .nautilus-list-view columnview > listview > row {{ border-radius: {radius}px; margin-top: {row_gap}px; margin-bottom: {row_gap}px; }}\n\
.nautilus-window .nautilus-list-view #NautilusViewCell {{ padding-top: {cell_padding}px; padding-bottom: {cell_padding}px; }}\n\
.nautilus-window .nautilus-grid-view gridview {{ padding: {grid_inset}px; }}\n\
.nautilus-window .nautilus-grid-view gridview > child {{ border-radius: {radius}px; }}\n\
.nautilus-window .nautilus-grid-view #NautilusViewCell {{ border-radius: {radius}px; }}\n\
.nautilus-window #NautilusPathBar {{ border-radius: {radius}px; }}\n\
.nautilus-window #NautilusPathButton {{ border-radius: {radius}px; }}\n",
        style.id()
    );
    // Do not adjust columnview's horizontal padding or its rows/cells: Nautilus
    // coordinates those values with rubberband selection and drop hit testing.
    // Header separators are inset shadows, so the native control allocation stays
    // intact. Colours follow the palette, including high-contrast theme colours.
    sheet.push_str(match style {
        Style::Native | Style::FollowTheme => unreachable!(),
        Style::Explorer => concat!(
            ".nautilus-window headerbar { background-color: @headerbar_bg_color; box-shadow: inset 0 -1px @borders; }\n",
            ".nautilus-window placessidebar { background-color: @sidebar_bg_color; }\n",
            ".nautilus-window placessidebar row { padding-top: 2px; padding-bottom: 2px; }\n",
            ".nautilus-window placessidebar row label { font-weight: 500; }\n",
            ".nautilus-window #NautilusPathBar { background-color: @view_bg_color; box-shadow: inset 0 0 0 1px @borders; }\n",
            ".nautilus-window .nautilus-list-view columnview > header { background-color: @sidebar_bg_color; box-shadow: inset 0 -1px @borders; }\n",
            ".nautilus-window .nautilus-list-view columnview > header button { border-radius: 0; }\n",
            ".nautilus-window .floating-bar { border-radius: 2px; }\n",
        ),
        Style::Finder => concat!(
            ".nautilus-window headerbar { background-color: @headerbar_bg_color; box-shadow: inset 0 -1px alpha(@view_fg_color, 0.12); }\n",
            ".nautilus-window placessidebar { background-color: @sidebar_bg_color; }\n",
            ".nautilus-window placessidebar row { padding-top: 4px; padding-bottom: 4px; }\n",
            ".nautilus-window placessidebar row label { font-weight: 600; }\n",
            ".nautilus-window #NautilusPathBar { background-color: alpha(@view_fg_color, 0.07); box-shadow: none; }\n",
            ".nautilus-window .nautilus-list-view columnview > header { background-color: @view_bg_color; box-shadow: inset 0 -1px alpha(@view_fg_color, 0.10); }\n",
            ".nautilus-window .nautilus-list-view columnview > listview > row:nth-child(even):not(:selected):not(:hover):not(:drop(active)) { background-color: alpha(@view_fg_color, 0.035); }\n",
            ".nautilus-window .floating-bar { border-radius: 8px; }\n",
        ),
    });
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_profiles_follow_only_known_desktop_families_and_keep_explicit_choices() {
        for (theme, expected) in [
            ("windows11", Style::Explorer),
            ("frutiger-aero", Style::Explorer),
            ("macos", Style::Finder),
            ("graphite", Style::Finder),
            ("terminal-minimal", Style::Native),
            ("nextkde", Style::Native),
            ("nord", Style::Native),
            ("custom-macos-buttons", Style::Native),
        ] {
            assert_eq!(Style::FollowTheme.resolve(theme), expected, "{theme}");
            for fixed in [Style::Native, Style::Explorer, Style::Finder] {
                assert_eq!(fixed.resolve(theme), fixed);
            }
        }
        assert_eq!(Style::default(), Style::FollowTheme);
        assert_eq!(Style::parse("theme"), Some(Style::FollowTheme));
        assert_eq!(Style::parse("follow-theme"), Some(Style::FollowTheme));
        assert!(css(Style::FollowTheme).is_empty(), "unresolved automatic style must not leak profile geometry");
    }

    #[test]
    fn grid_and_breadcrumb_geometry_follow_the_profile_without_shrinking_controls() {
        for (style, radius, inset) in [(Style::Explorer, 2, 8), (Style::Finder, 7, 12)] {
            let sheet = css(style);
            assert!(sheet.contains(&format!(".nautilus-window .nautilus-grid-view gridview {{ padding: {inset}px; }}")));
            assert!(sheet.contains(&format!(".nautilus-window #NautilusPathButton {{ border-radius: {radius}px; }}")));
            // Keep native click targets, breadcrumb overflow and column hit testing.
            for forbidden in ["min-width:", "min-height:", "padding-left:", "padding-right:", "margin-left:", "margin-right:", "overflow:"] {
                assert!(!sheet.contains(forbidden), "{forbidden}");
            }
        }
    }

    #[test]
    fn profiles_are_distinct_scoped_geometry_with_native_palette() {
        assert!(css(Style::Native).is_empty());
        let explorer = css(Style::Explorer);
        let finder = css(Style::Finder);
        assert_ne!(explorer, finder);
        assert!(explorer.contains("border-radius: 2px"));
        assert!(finder.contains("border-radius: 8px"));
        for sheet in [&explorer, &finder] {
            for line in sheet.lines().filter(|line| line.contains('{')) {
                assert!(line.starts_with(".nautilus-window "), "unscoped rule: {line}");
            }
            for forbidden in ["#ffffff", "#000000", "opacity:", "visibility:", "display:", "min-height: 0", "url("] {
                assert!(!sheet.contains(forbidden), "{forbidden}");
            }
            assert!(sheet.contains("placessidebar row { padding-top:"));
            assert!(sheet.contains("columnview > header {"));
            assert!(sheet.contains("@view_bg_color"));
            assert!(sheet.contains("@sidebar_bg_color"));
        }
        assert!(explorer.contains("box-shadow: inset 0 0 0 1px @borders"));
        assert!(finder.contains("row:nth-child(even):not(:selected):not(:hover):not(:drop(active))"));
    }
}
