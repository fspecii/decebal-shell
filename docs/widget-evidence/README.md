# Widget integration evidence

All PNGs use the production widget layout and tiny-skia painter with installed
fonts. They are actual raster output, not browser mockups. Browser evidence only
places these PNGs over a checkerboard to expose their alpha channels.

## Every built-in

All 18 shared widgets have Nord and macOS dark/light renders with sample and
unavailable data: 144 PNGs named `<id>-<design>-<mode>-<state>.png`.
Fixtures use a fixed clock, and unavailable data never masquerades as live data.
Layout bounds are checked with actual font metrics; deterministic layout goldens
remain the portable baseline because font rasterization varies between machines.

All 18 also have `<id>-catppuccin-<mode>-transparent.png` renders. These use the
real palette overlay path: Nord geometry with the built-in Catppuccin colour
scheme, theme opacity 0.7 and saved instance `background_opacity` 0.6. Tests verify
that palette changes alter every widget's pixels, preserve geometry, and that
opaque/partial/zero material fill produce decreasing alpha coverage. Every
rendered built-in colour is asserted to be a semantic token.

## Real Fastfetch backend

`fetch-summary-fastfetch-live-dark.png` and `...-light.png` contain actual values
collected from installed Fastfetch 2.70.0 using the compiled adapter. They use
Nord geometry, Catppuccin colours and the transparency settings above.
`fetch-summary-fastfetch-cli-live.png` exercises the registered live source through
`decwidgets render --live` on the private test configuration. It uses that probe's
colours and background. These files contain real OS/kernel/CPU/RAM/uptime/WM data.
The other fetch-summary PNGs intentionally use clearly labelled sample data.

The adapter uses fixed JSON modules and `--config none`, runs off the drawing
thread, and caps execution at five seconds and stdout at 64 KiB. Tests cover
malformed/partial JSON, absent modules, control characters, missing binaries,
timeouts, output overflow, clearing stale data, and refresh throttling.

## Rendering and interaction regressions

The compact player additionally has `long` and `cover-long` renders. Tests check
real font widths, in-bounds controls and the Next button's actual hit target.
The cover is a generated solid placeholder, not private media art. Hub shortcuts,
playing/paused/absent states, waveform-shaped track progress, unavailable weather,
and narrow-bar paint bounds also have regressions.

Material tests verify theme/token/instance alpha multiplication and opaque
foreground controls. A zero-background instance skips native backdrop glass and
external blur; outlines remain. Normal glass and non-composited wallpaper
sampling retain the existing host paths.

Reproduce:

```sh
WIDGET_EVIDENCE_DIR="$PWD/docs/widget-evidence" cargo test --lib widgets::samples
WIDGET_EVIDENCE_DIR="$PWD/docs/widget-evidence" cargo test --lib widgets::fetch::tests::installed_fastfetch_returns_real_system_data -- --ignored
RDWM_BIN_DIR=/path/to/rdwm/target/debug tests/e2e/widget-materials.sh
PROFILE=debug RDWM_BIN_DIR=/path/to/rdwm/target/debug tests/e2e/widgets-visible.sh
```

Validation: 45 widget tests passed, with the optional Fastfetch test passed
separately. Full library: 1,441 passed, five ignored, current-user PAM-start probe
skipped. Clippy all targets/features with warnings denied and formatting passed.
Private Xvfb: all five material pixel checks and all 35 visibility/click checks
passed. The material checks cover opacity, an independent palette, light mode and
zero fill over a real wallpaper pixmap.

The earlier live bug capture remains in ignored
`target/widget-debug/now-playing-live.png`; desktop content is not committed.
No live configuration, binaries or processes were changed. Theme widget ownership,
Keep/Replace/Cancel, production PAM and live-wallpaper code were preserved.
