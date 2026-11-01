//! Actual package resolution, not screenshots or a claim of full upstream replicas.
use decebal::theme::{builtin, engine::decoration_compat, model::Selection};

#[test]
fn shipped_family_packages_select_the_native_launcher() {
    for (id, style) in [("windows11", "windows"), ("nextkde", "kde"), ("gnome-adwaita", "gnome")] {
        let def = builtin::def(id).unwrap();
        for dark in [false, true] {
            assert_eq!(def.resolve(id, dark, &Selection::default()).launcher_style, style, "{id}, dark={dark}");
        }
    }
}

#[test]
fn windows_and_gnome_choose_mode_specific_caption_styles_with_a_classic_fallback() {
    for (id, light, dark) in [("windows11", "windows-light", "windows"), ("gnome-adwaita", "adwaita-light", "adwaita")] {
        let def = builtin::def(id).unwrap();
        for (is_dark, expected) in [(false, light), (true, dark)] {
            let want = decoration_compat::wanted(&def.extra, is_dark).expect("package declares mode-specific styles");
            assert_eq!(want.desired, expected);
            assert_eq!(want.fallback.as_deref(), Some("classic"));
            let theme = def.resolve(id, is_dark, &Selection::default());
            assert!(theme.window_buttons_right, "{id} toolkit controls follow its native right-side style");
        }
    }
}
