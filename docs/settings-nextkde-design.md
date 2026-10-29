# Settings visual design

Reference: NextKde/NextKde at commit `3fa1d7006869bbd3404a05d362c6e6028e0fe3da` (GNU GPL v3), inspected in `apps/settings/main.qml`, `apps/settings/components/SettingsCard.qml`, and `SettingsNavBar.qml`.

This is an original Rust rendering adaptation of observable visual conventions. No QML implementation or upstream assets are included. Decebal remains MIT licensed.

The settings window uses a quiet sidebar plane, lightly tinted persistent navigation selection, clearly separated page headings, sentence-case group labels, rounded inset cards, and neutral elevated segmented selection. Switches use the active palette accent. All surfaces and text use existing semantic theme tokens; settings keeps one visual language across desktop layouts and palettes.

At compact sizes the sidebar shrinks to 34% of the window (bounded by 156–236 logical pixels), long navigation labels truncate, content margins narrow, and card controls move below their labels. Control bounds and focus order remain part of the existing retained hit map and debug dump. Search, scrolling and pane semantics are unchanged.

Automated coverage checks compact control containment, focus order and hit testing at 1×, 1.5× and 2× alongside existing settings rendering tests. Full desktop rendering should also be checked with dark and light palettes on an isolated display; no live desktop changes are necessary.

## Render verification

Rendered the actual X11 settings binary with private HOME/config/runtime directories and Xvfb `:177` under rdwm. Checked Appearance, Widgets and General at 940×680 and the 480×360 minimum, explicitly selecting both dark and light modes in the isolated `decebal.json`. Captures live in `/tmp/decebal-settings-nextkde-qa/` with `<mode>-<pane>-<normal|narrow>.png` names; additional narrow scrolled captures check content below the fold.

This caught two real issues: sidebar group names crossed the narrow sidebar edge, and theme gallery thumbnails left insufficient room for names/actions. Group names now truncate within bounds. Gallery cards below 560 logical pixels place the preview above metadata and wrap action buttons; a test verifies containment, non-overlap and hit detection at three scales. Compact Appearance tabs avoid an empty label row and reduce text size to keep Themes/Palettes legible.

A screenshot immediately after an X11 resize can capture the background before repaint. The isolated capture script waits after geometry changes and moves the pointer into content before capturing. No live desktop settings were changed.
