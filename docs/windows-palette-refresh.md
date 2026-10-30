# Windows taskbar palette refresh

Two independent bugs kept the Windows taskbar from following the selected palette:

1. `DockApp::tick` returned into the Windows tick before processing shared theme-generation changes. The cached Windows colors/metrics therefore survived palette changes until another settings change or a process restart.
2. A forced `windowsTheme=light|dark` that differed from the active mode rebuilt the built-in Windows 11 defaults with an empty selection, losing the selected palette, custom accent and dynamic colors.

The common tick now polls configuration and refreshes theme-dependent state before choosing its Windows/macOS path. Reloaded settings use the ordinary style-transition path; redraw flags carry into the Windows tick. Fonts already synchronize against theme generation. Forced modes resolve the current design and selection in the requested mode only when the settings/theme cache is refreshed. Accessibility overrides are applied, and wallpaper brightness following is disabled only in a local cloned selection, preserving the user's saved preferences. Auto mode uses the already-resolved active theme.

## Regression evidence

`tests/e2e/windows-palette-live.py` runs one dock process/window on an owned private Xvfb with private HOME/all XDG directories and inaccessible bus endpoints. It changes Catppuccin → Nord → Gruvbox in Auto, forced Light and forced Dark, reading actual taskbar background pixels after each change. It performs no live input/configuration changes and no restart between cases. It needs Xvfb, xdotool, ImageMagick `import`, and Python Pillow.

```sh
DOCK_BIN=/absolute/decdock PALETTE_EVIDENCE_DIR=/tmp/windows-palette-proof \
  python3 tests/e2e/windows-palette-live.py
```

The pre-fix archived optimized binary SHA `4f37e39910dbc252f97c2da1594f6abb742c96d27a0eaf6b74301ceac144f400` fails this gate: each mode retains identical pixels for all three palettes. Evidence is `/tmp/windows-palette-before-final`.

Unit coverage checks live theme-generation refresh with the same Windows state, Mac↔Windows transitions, selected palette colors in both forced modes, custom accent/dynamic settings, accessibility high contrast, and unchanged input preferences. The generation test runs in a subprocess to isolate publication of process-global theme state from parallel tests.

This fix affects shell/taskbar colors. GTK/file-manager/Chrome integrations have separate application and restart behavior; the existing Windows Explorer profile/palette test passes and is not the cause of this taskbar regression.

After-fix debug SHA `72752f279bb83afba8fe4b3860b892a10070782ae75679e494fb2ad17ebad4c0` passes all nine native cases in one PID/window (`/tmp/windows-palette-after-final2`). Start-logo accents match the selected palettes: Auto/Dark Catppuccin `(203,166,247)`, Nord `(136,192,208)`, Gruvbox `(250,189,47)`; forced Light `(136,57,239)`, `(94,129,172)`, `(181,118,20)`. The gate checks background and accent together because the nearly identical Nord/Catppuccin light backgrounds quantize to the same pixel under taskbar alpha. Six taskbar tests, 31 dock-app tests, and lib/decdock clippy with warnings denied pass. No live session was modified.
