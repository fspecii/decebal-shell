# Dux bar and Control Center review repairs

The bar now propagates a title-only active-window refresh into its repaint flag.
A real private EWMH regression changes `_NET_WM_NAME` on the same fake app,
without pointer/key input, minute rollover or resource widgets. The old optimized
bar stayed stale for three seconds; the repaired debug bar repainted in 0.38s.
Run `BAR_BINARY=/absolute/decbar python3 tests/e2e/dux-title-redraw.py
/absolute/dux-package /tmp/evidence`. `EXPECT_STALE=1` records the unfixed
baseline. The script owns Xvfb, isolates HOME/all XDG/both buses, and stubs tools;
it never uses the live display or launches real desktop actions.

Source WidgetModel icons are distinct: `do_not_disturb_on` for DND and `contrast`
for Appearance. Material mapping and cmap/GSUB evidence are handled by the font
repair. Independent Dux-style Control Centers in other themes use existing
semantic vector symbols if package Material faces are unavailable. The chooser
explains this fallback; it does not promise source font parity without assets.

Slider targets now retain the full source 48px row height (growing with text),
while painting a centered 14px track. Paint and input retain the same horizontal
track span. Label/unavailable text is vertically centered. Focus/hover rings
remain visible for enabled toggles. Unit checks cover top and bottom of targets
and distinct nonblank symbols without prepared Material fonts.

Bar hover clears on entering popups; Control Center hover clears on leaving.
Keyboard Enter requires an explicit keyboard focus and never borrows stale
pointer hover or implicitly activates the first control. Pointer input clears
keyboard focus; wheel scrolling clears offscreen focus. Both Control Center
hosts use a 40px-per-delta scroll step and expose a thumb when clipped. The
isolated native surface assertion checks inert unfocused Enter, stale capture
hover, pointer-leave clearing, and slider Enter not issuing midpoint setters.

Resource wake-ups occur every two seconds only when CPU/RAM widgets are actually
configured, and a drop guard releases a sampler's busy flag even if it panics.
Long-label fitting uses a Unicode-safe binary search instead of removing one
character per measurement. Repeated layout snapshot/player-list cloning remains
a separate measured-optimization opportunity; it was not broadly rewritten.

Classic bar and Control Center layout/slider drawing remain unchanged. Parent
must run final optimized native input/pixel gates, source geometry/font gates,
and the complete integrated validation before installation. Debug and offscreen
checks here do not establish hardware FPS, Wayland popup coordinates or AT-SPI.

## Accepted debug evidence

- Menubar:176passed/2ignored; Control Center:47passed/1ignored;
  CC Settings:2passed. Strict lib/decbar Clippy passed.
- Exact private 40-image surface painter gate passed, including the source-sized
  slider targets and the keyboard safety assertions at100/200% with real package
  fonts: `/tmp/dux-bar-cc-review-painters/result.json`.
- Final debug bar SHA`6d5d059a0a38b72f628ec3c693f45fd7fb913180c170d38dec188881a709411a`
  repaints title-only change in0.31s: `/tmp/dux-title-frozen/result.json`.
  Baseline SHA`fe1b902a…62d0` stays stale3s; before/after images contain actual
  A/Z titles. These are actual EWMH events, not synthetic App unit state.
- `/tmp/dux-cc-fallback-keyboard-final/` proves visible person/settings/network
  glyph pixels on classic macOS with independent Dux look and no package fonts,
  unfocused Return is inert, then Down/Return invokes only a private Settings
  fixture. `tests/e2e/dux-cc-fallback.py` reproduces it. Capture waits for actual
  presentation, not just the earlier debug-layout file write.

No installation, live input/audio/configuration, restart or release build was
performed by this agent. Source production is frozen after this validation.
