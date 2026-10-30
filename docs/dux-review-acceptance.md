# Combined Dux review acceptance gates

Independent inspection of ac7a4b7 confirmed the focused regression harnesses and
source contracts below. This inspection is not execution acceptance of a newer
combined binary. Root owns the subsequent complete tests and optimized build;
previous debug evidence must not be substituted for that candidate.

## Exact candidate provenance

Pin the candidate binaries and record their SHA-256 values with the source
revision and build log. The harnesses record the hashes they actually execute.
A timestamp or a matching version string alone does not identify a revision.
In particular, `/tmp/dux-bar-review-frozen-bin/decbar` and
`/tmp/dux-bar-cc-final-lib-tests` predate ac7a4b7.

## Private native gates

Run from the shell repository, using the same pinned candidate directory:

```sh
BIN_DIR=/absolute/candidate /usr/bin/python3 tests/e2e/dux-placement-native.py /tmp/dux-accept-placement
BAR_BINARY=/absolute/candidate/decbar /usr/bin/python3 tests/e2e/dux-title-redraw.py themes/dux-lavender /tmp/dux-accept-title
BAR_BINARY=/absolute/candidate/decbar /usr/bin/python3 tests/e2e/dux-cc-fallback.py /tmp/dux-accept-fallback
```

The placement harness needs `decbar`, `decnotify`, and `decmon`. It measures the
actual painted band, banner and monitor positions and requires banner clearance
of eight pixels and monitor clearance of four. The title harness changes only
`_NET_WM_NAME` on a fake EWMH application, without resource widgets or pointer
input; the bar must repaint within three seconds. The fallback harness selects
classic macOS with the independent Dux Control Center look and no package fonts.
It verifies actual person/settings/network glyph pixels, inert unfocused Return,
and an explicitly focused Settings action routed to a private fixture.

These harnesses create private HOME/XDG directories, owned Xvfb displays and
private or nonexistent buses. They must never be pointed at the live display.

## Focused source and painter gates

Use the exact test executable built from the same source candidate, running each
filter serially:

- `comfortable_source_grid_matches_pinned_dashmetrics_equations`
- `source_grid_and_large_text_reflow_keep_cards_separate_and_in_bounds`
- `slider_targets_cover_the_full_row_and_icons_use_source_names`
- `dux_symbols_have_semantic_fallback_without_package_fonts`
- `control_center_wheel_keeps_highlight_and_repaint_consistent`

The Dash equation gate asserts the 128px columns, eight-pixel gaps, 96px row
unit and exact card coordinates at 832px. The reflow gate checks separate cards
at narrow widths and 100/200% text. Slider tests check the full 48px row,
including its top and bottom hit regions, rather than only the painted track.

Run `native_dux_surface_evidence --ignored --test-threads=1 --nocapture` alone,
with explicit private `DECEBAL_CONFIG`, `DECEBAL_THEMES`, copied Dux package fonts,
private HOME/all XDG/buses, stubbed external commands and
`DUX_SURFACE_EVIDENCE_DIR`. Its forty PNGs cover light/dark, 100/200% text,
available/missing data, Control Center and Dash pages. It also exercises shared
hit geometry, pointer commits and explicit keyboard-focus safety assertions.

Offscreen painter assertions do not prove real X wheel/drag interactions at
every scale, Wayland popup placement, hardware frame rate or accessibility APIs.
A successful native no-font keyboard gate does not establish all those claims.

## Inspection outcome

Harness Python syntax and shared diff whitespace checks passed. A minor wheel
highlight edge was reported to root: scrolling outside the panel cleared state
without requesting repaint. Root subsequently corrected it and added the focused
regression named above. No live input/configuration, installation, restart or
release build was performed by the independent reviewer.
