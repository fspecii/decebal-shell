# Shared resource widget render evidence

These PNGs were produced by `widgets::samples::tests::shared_cards_render_in_dark_and_light_designs`
using the production widget layout and tiny-skia painter, installed fonts, and fixed data.
They are card renders, not captures of a running desktop. No live services are required.

Each widget has Nord and macOS renders, each in dark and light mode, with both
normal sample data and absent data (16 PNGs total). The normal sample is
412 kB/s receive, 88 kB/s send, 71% root usage, and 270 GiB free.
The histories scale independently. Layout assertions cover every node's bounds
with actual font metrics; separate layout goldens use deterministic fixed metrics.

Reproduce from the repository root:

```sh
WIDGET_EVIDENCE_DIR="$PWD/docs/widget-evidence" cargo test --lib widgets::samples::tests::shared_cards_render_in_dark_and_light_designs
cargo test --lib widgets::samples
```

Visually reviewed normal and unavailable states for legibility, complete labels,
full-width graphs/bars, rounded corners, and semantic colours in both modes.
Font rasterization can vary between machines; layout goldens are the portable
regression baseline.

Validation on this checkout: 40 widget tests passed; the full default-feature
library suite passed 1,436 tests with four ignored and the current-user PAM-start
probe explicitly skipped. All-target/all-feature Clippy with warnings denied,
formatting, and whitespace checks passed. Theme ownership and Keep/Replace/Cancel
code, production PAM, live wallpaper code and live configuration were unchanged.

## Rice and mobile-inspired additions

Five additional widgets (`terminal-session`, `retro-meters`, `ios-day`, `berry-hub`,
`android-player`) have the same two-design/two-mode/sample-or-unavailable coverage:
40 additional PNGs. Their colours follow the selected theme, with fixed radii
used deliberately to preserve square terminal, angular hub and rounded mobile
silhouettes. The shared render test above now covers all seven new cards.

The interaction regression tests the hub's actual hit targets and allowlisted
actions, player controls and playing/paused/absent states, long-title truncation,
and missing weather. Rendering uses the actual painter, not browser mockups.

## Fetch, terminal wave and compact-player correction

`fetch-summary`, `terminal-wave` and `now-playing-mini` are included in the shared
sample/unavailable dark/light rendering test. The compact-player regression also
saves `long` and `cover-long` variants, checks actual font widths, and verifies
that the Next button's center is inside the root hit region and invokes the
allowlisted Next action. The waveform progress regression checks empty, half and
full meters and the absent-player state. Cover art in this test is a generated
solid placeholder; no private media files are included.

The live bug capture remains in ignored `target/widget-debug/now-playing-live.png`
on this checkout, outside committed evidence because it includes the desktop.
