//! Widgets shipped with decebal (`widgets/*.toml` in the source tree). A theme refers to them as `builtin:<id>`.

use super::dsl::{self, WidgetDef};
use super::Trust;

pub const SAMPLES: &[(&str, &str)] = &[
    ("network-traffic", include_str!("../../widgets/network-traffic.toml")),
    ("storage-card", include_str!("../../widgets/storage-card.toml")),
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

    #[test]
    fn resource_cards_distinguish_missing_data_from_zero() {
        for (id, paths, expected) in [("network-traffic", vec!["net.rx", "net.tx"], "Receive   0 B/s"), ("storage-card", vec!["disk.free", "disk.used", "disk.total", "disk.usage"], "0% used")] {
            let w = get(id).unwrap();
            let mut d = crate::widgets::data::Data::empty();
            let draw = |d: &crate::widgets::data::Data| dump(&layout(&w, &Ctx { data: d, metrics: &Fixed, theme_radius: 10.0 }, None));
            assert!(draw(&d).contains("Unavailable"), "{id}");
            for path in paths {
                d.set(path, crate::widgets::data::Val::Num(0.0));
            }
            let zero = draw(&d);
            assert!(!zero.contains("Unavailable"), "{id}: {zero}");
            assert!(zero.contains(expected), "{id}: {zero}");
        }
    }

    /// Set WIDGET_EVIDENCE_DIR to retain the actual renderer output for visual review.
    #[test]
    fn resource_cards_render_in_dark_and_light_designs() {
        use crate::theme::model::Selection;
        use crate::widgets::{layout::Item, paint};
        use std::path::Path;

        fn contained(it: &Item, width: f32, height: f32) {
            assert!(it.rect.x >= 0.0 && it.rect.y >= 0.0);
            assert!(it.rect.x + it.rect.w <= width + 0.01 && it.rect.y + it.rect.h <= height + 0.01, "{}: {:?}", it.kind, it.rect);
            for child in &it.children {
                contained(child, width, height);
            }
        }

        let mut fonts = crate::text::Fonts::load().expect("render verification requires fonts");
        for design in ["nord", "macos"] {
            for dark in [true, false] {
                let th = crate::theme::builtin::def(design).unwrap().resolve(design, dark, &Selection::default());
                for id in ["network-traffic", "storage-card"] {
                    let def = get(id).unwrap();
                    for (state, data) in [("sample", sample()), ("unavailable", crate::widgets::data::Data::empty())] {
                        let it = layout(&def, &Ctx { data: &data, metrics: &paint::FontMetrics(&fonts), theme_radius: th.radius }, None);
                        contained(&it, it.rect.w, it.rect.h);
                        let pm = paint::render(&def, &data, &th, &mut fonts, Path::new("."), 16).unwrap();
                        assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0));
                        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
                        if let Some(dir) = std::env::var_os("WIDGET_EVIDENCE_DIR") {
                            std::fs::create_dir_all(&dir).unwrap();
                            let mode = if dark { "dark" } else { "light" };
                            pm.save_png(Path::new(&dir).join(format!("{id}-{design}-{mode}-{state}.png"))).unwrap();
                        }
                    }
                }
            }
        }
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
