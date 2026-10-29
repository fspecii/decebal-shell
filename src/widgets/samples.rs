//! Widgets shipped with decebal (`widgets/*.toml` in the source tree). A theme refers to them as `builtin:<id>`.

use super::dsl::{self, WidgetDef};
use super::Trust;

pub const SAMPLES: &[(&str, &str)] = &[
    ("next-clock", include_str!("../../widgets/next-clock.toml")),
    ("next-weather", include_str!("../../widgets/next-weather.toml")),
    ("next-system", include_str!("../../widgets/next-system.toml")),
    ("clock-card", include_str!("../../widgets/clock-card.toml")),
    ("cpu-ram-ring", include_str!("../../widgets/cpu-ram-ring.toml")),
    ("now-playing-mini", include_str!("../../widgets/now-playing-mini.toml")),
    ("workspace-dots", include_str!("../../widgets/workspace-dots.toml")),
    ("sysmon-graph", include_str!("../../widgets/sysmon-graph.toml")),
    ("battery-pill", include_str!("../../widgets/battery-pill.toml")),
];

pub fn source(id: &str) -> Option<&'static str> {
    SAMPLES.iter().find(|s| s.0 == id).map(|s| s.1)
}

pub fn get(id: &str) -> Option<WidgetDef> {
    // shipped samples are theme-safe by construction (a test parses them with `Trust::Theme`)
    source(id).and_then(|t| dsl::parse(t, Trust::Theme).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::data::sample;
    use crate::widgets::layout::{dump, layout, Ctx, Fixed};

    #[test]
    fn every_sample_is_valid_for_themes_and_has_matching_id() {
        for (id, text) in SAMPLES {
            let w = dsl::parse(text, Trust::Theme).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(w.id, *id);
            assert!(w.commands.is_empty() && w.actions.iter().all(|a| !matches!(a, super::super::actions::Action::Exec(_))));
            assert!(w.size.is_some() && !w.description.is_empty());
        }
        assert!(SAMPLES.len() >= 4);
    }

    /// Layout goldens: `UPDATE_GOLDEN=1 cargo test --lib widgets::samples` rewrites tests/golden/widgets/*.layout.
    #[test]
    fn layouts_match_the_golden_files() {
        let d = sample();
        for (id, _) in SAMPLES {
            let w = get(id).unwrap();
            let got = dump(&layout(&w, &Ctx { data: &d, metrics: &Fixed, theme_radius: 10.0 }, None));
            let path = format!("{}/tests/golden/widgets/{id}.layout", env!("CARGO_MANIFEST_DIR"));
            if std::env::var_os("UPDATE_GOLDEN").is_some() {
                std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
                std::fs::write(&path, &got).unwrap();
            }
            let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}; run with UPDATE_GOLDEN=1"));
            assert_eq!(got, want, "{id}: layout changed (UPDATE_GOLDEN=1 to accept)");
        }
    }
}
