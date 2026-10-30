//! Visual familiarity for modern GTK 4 Nautilus, never a replacement file manager.
//! Upstream defines `.nautilus-window` in nautilus-window.c, `placessidebar`
//! in gtk/nautilusgtkplacessidebar.c, and view selectors in resources/style.css.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Native,
    Explorer,
    Finder,
}

impl Style {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
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
}

/// Geometry only: all colours remain provided by the current GTK palette.
/// Avoid changing column padding/margins: Nautilus uses these for hit testing.
pub fn css(style: Style) -> String {
    let (radius, sidebar_radius, row_gap, cell_padding, header_padding, grid_inset) = match style {
        Style::Native => return String::new(),
        Style::Explorer => (2, 2, 1, 3, 4, 8),
        Style::Finder => (7, 8, 3, 5, 6, 12),
    };
    format!(
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
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for sheet in [explorer, finder] {
            for line in sheet.lines().filter(|line| line.contains('{')) {
                assert!(line.starts_with(".nautilus-window "), "unscoped rule: {line}");
            }
            for forbidden in ["color:", "background:", "button {", "opacity:", "visibility:", "display:", "min-height: 0", "url("] {
                assert!(!sheet.contains(forbidden), "{forbidden}");
            }
        }
    }
}
