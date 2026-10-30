//! Next-login accessibility preferences. These only alter the effective in-memory theme.
use crate::theme::model::{Rgb, Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Preferences {
    pub reduce_motion: bool,
    pub font_scale: Option<f32>,
    pub high_contrast: bool,
}

impl Preferences {
    /// Inject lookup for tests; malformed, non-finite and out-of-range values are ignored.
    pub fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Self {
        let reduce_motion = lookup("DECEBAL_REDUCE_MOTION").as_deref() == Some("1");
        let high_contrast = lookup("DECEBAL_HIGH_CONTRAST").as_deref() == Some("1");
        let font_scale = lookup("DECEBAL_FONT_SCALE").and_then(|s| s.parse::<f32>().ok()).filter(|s| s.is_finite() && (0.8..=2.0).contains(s));
        Self { reduce_motion, high_contrast, font_scale }
    }

    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Accessibility size takes precedence over the theme size, without changing the selection.
    pub fn apply(self, mut theme: Theme) -> Theme {
        if let Some(scale) = self.font_scale {
            theme.font_scale = scale;
        }
        if self.high_contrast {
            let (bg, fg) = if theme.dark { (Rgb(0, 0, 0), Rgb(255, 255, 255)) } else { (Rgb(255, 255, 255), Rgb(0, 0, 0)) };
            theme.background = bg;
            theme.sidebar = bg;
            theme.surface = bg;
            theme.bar = bg;
            theme.popup = bg;
            theme.dock = bg;
            theme.overlay = bg;
            theme.titlebar = bg;
            theme.titlebar_inactive = bg;
            theme.text = fg;
            theme.text_secondary = fg;
            theme.text_faint = fg;
            theme.bar_text = fg;
            theme.border = fg;
            // A selected control must stay distinguishable from its surroundings.
            theme.accent = if theme.dark { Rgb(255, 255, 0) } else { Rgb(0, 0, 128) };
            theme.accent_text = bg;
            theme.danger = if theme.dark { Rgb(255, 150, 150) } else { Rgb(128, 0, 0) };
            theme.ok = if theme.dark { Rgb(150, 255, 150) } else { Rgb(0, 90, 0) };
            theme.warn = theme.accent;
            // Contrast must not depend on the wallpaper under a translucent surface.
            theme.dock_alpha = 1.0;
            theme.popup_alpha = 1.0;
            theme.panel_alpha = 1.0;
            theme.bar_alpha = 1.0;
            theme.outline = 1.0;
            theme.glass_refraction = 0.0;
            theme.glass_highlight = 0.0;
            theme.glass_grain = 0.0;
        }
        theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{
        builtin,
        model::{contrast, Selection},
    };

    #[test]
    fn strict_bounded_preferences() {
        for input in ["", "nan", "NaN", "inf", "-1", "0.79", "2.01", " 1.2", "1.2 "] {
            assert_eq!(Preferences::from_lookup(|_| Some(input.into())).font_scale, None, "{input}");
        }
        for scale in [0.8, 1.0, 2.0] {
            assert_eq!(Preferences::from_lookup(|_| Some(scale.to_string())).font_scale, Some(scale));
        }
        for input in ["true", "yes", "01", " 1", "0"] {
            let p = Preferences::from_lookup(|_| Some(input.into()));
            assert!(!p.reduce_motion && !p.high_contrast);
        }
    }

    #[test]
    fn effective_theme_keeps_identity_and_has_opaque_readable_surfaces() {
        for dark in [false, true] {
            let base = builtin::def("nord").unwrap().resolve("nord", dark, &Selection::default());
            let effective = Preferences { high_contrast: true, font_scale: Some(2.0), ..Preferences::default() }.apply(base.clone());
            assert_eq!((&effective.id, &effective.name, effective.dark), (&base.id, &base.name, base.dark));
            assert_eq!(effective.font_scale, 2.0);
            assert_eq!(base.font_scale, 1.0);
            for bg in [effective.background, effective.sidebar, effective.surface, effective.bar, effective.popup, effective.dock, effective.overlay, effective.titlebar] {
                for fg in [effective.text, effective.text_secondary, effective.text_faint, effective.bar_text] {
                    assert!(contrast(fg, bg) >= 7.0);
                }
            }
            // Exercise the same CPU raster fills used by shell surfaces, and inspect the
            // composited pixels: an arbitrary wallpaper must not leak into the surface.
            let mut pm = tiny_skia::Pixmap::new(20, 10).unwrap();
            pm.fill(Rgb(200, 30, 90).color(1.0));
            crate::gfx::fill_rect(&mut pm, 0.0, 0.0, 20.0, 10.0, effective.popup_c());
            crate::gfx::fill_rect(&mut pm, 8.0, 2.0, 4.0, 6.0, effective.text_c());
            let bg = pm.pixel(1, 1).unwrap();
            let fg = pm.pixel(9, 3).unwrap();
            assert_eq!((bg.alpha(), fg.alpha()), (255, 255));
            assert!(contrast(Rgb(bg.red(), bg.green(), bg.blue()), Rgb(fg.red(), fg.green(), fg.blue())) >= 7.0);
            assert!(contrast(effective.accent_text, effective.accent) >= 7.0);
            assert_eq!((effective.dock_alpha, effective.popup_alpha, effective.panel_alpha, effective.bar_alpha), (1.0, 1.0, 1.0, 1.0));
        }
    }
}
