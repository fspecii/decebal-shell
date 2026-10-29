# Settings visual design

Reference: NextKde/NextKde at commit `3fa1d7006869bbd3404a05d362c6e6028e0fe3da` (GNU GPL v3), inspected in `apps/settings/main.qml`, `apps/settings/components/SettingsCard.qml`, and `SettingsNavBar.qml`.

This is an original Rust rendering adaptation of observable visual conventions. No QML implementation or upstream assets are included. Decebal remains MIT licensed.

The settings window uses a quiet sidebar plane, lightly tinted persistent navigation selection, clearly separated page headings, sentence-case group labels, rounded inset cards, and neutral elevated segmented selection. Switches use the active palette accent. All surfaces and text use existing semantic theme tokens; settings keeps one visual language across desktop layouts and palettes.

At compact sizes the sidebar shrinks to 34% of the window (bounded by 156–236 logical pixels), long navigation labels truncate, content margins narrow, and card controls move below their labels. Control bounds and focus order remain part of the existing retained hit map and debug dump. Search, scrolling and pane semantics are unchanged.

Automated coverage checks compact control containment, focus order and hit testing at 1×, 1.5× and 2× alongside existing settings rendering tests. Full desktop rendering should also be checked with dark and light palettes on an isolated display; no live desktop changes are necessary.

## Render verification

Rendered the actual X11 settings binary with private HOME/config/runtime directories and Xvfb `:177` under rdwm. Checked Appearance, Widgets and General at 940×680 and the 480×360 minimum, explicitly selecting both dark and light modes in the isolated `decebal.json`. Captures live in `/tmp/decebal-settings-nextkde-qa/` with `<mode>-<pane>-<normal|narrow>.png` names; additional narrow scrolled captures check content below the fold.

This caught two real issues: sidebar group names crossed the narrow sidebar edge, and theme gallery thumbnails left insufficient room for names/actions. Group names now truncate within bounds. Gallery cards below 560 logical pixels place the preview above metadata and wrap action buttons; a test verifies containment, non-overlap and hit detection at three scales. Compact Appearance tabs avoid an empty label row and reduce text size to keep Themes/Palettes legible.

Native geometry presentation is covered by `tests/e2e/settings-repaint.sh`: float, resize, same-size moves and repeated geometry requests, with no synthetic pointer/key input. It captures both the client drawable and the corresponding root region. Optional `WITH_COMPOSITOR=1` distinguishes application painting from compositor presentation. In one private GLX/Picom reproduction the client drawable contained the complete Settings UI while the composited root was black, including before geometry changes; that is compositor presentation evidence, not a confirmed Settings redraw defect. No speculative native backend changes were retained. No live desktop settings were changed.

## Fidelity refinement

Compared the actual upstream `docs/images/desktop.png` directly. Its Settings window has an explicit Settings heading, white inset navigation groups on a neutral sidebar, and generously sized two-column appearance cards. The adaptation now follows that composition: title and search remain above the scrolling groups; theme cards form two columns when space permits, with large previews and actions beneath their descriptions, and become one column at compact widths.

NextKDE/macOS fallback previews are original illustrations of a whole desktop arrangement: gradient background, translucent analog-clock/weather cards, a split-plane Settings window and a broad glass dock with distinct colourful icons. Valid package screenshots remain preferred. These illustrations communicate layout; they do not claim to be captured desktop screenshots. Settings itself keeps opaque neutral planes, consistent with the reference; desktop transparency belongs to the shell surfaces and widgets.
