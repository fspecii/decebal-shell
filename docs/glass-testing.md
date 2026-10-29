# Isolated X11 glass verification

Run `tests/e2e/glass.sh`.

The test builds decbar, decwidgets and decthemectl with two build jobs, then starts private Xvfb and Picom GLX with `contrib/picom/decebal-glass.conf`. It uses temporary HOME, config, data and runtime directories. Display `:231` is the default; `E2E_DISPLAY` selects another unused private display. Occupied displays and `:0`/`:1` are refused. `PROFILE=release` selects release builds, `NO_BUILD=1` reuses binaries, and `KEEP=1` retains temporary logs and screenshots.

Checks cover:

- Resetting from Nord and its palette restores NextKDE's own colours.
- NextKDE bar and desktop analog clock request scoped blur, and the live clock has a 32-bit ARGB visual.
- The bar's bounding shape excludes unused popup canvas over the desktop clock, keeping Picom from blurring widgets beneath that transparent canvas.
- Root screenshot pixels inside both surfaces change when the wallpaper changes between two solid colours. The pixels differ from the bare wallpaper as well, proving that an actual tinted surface is composited over the background.
- Switching to Nord clears the blur hint on both shared surfaces while Picom remains alive.

This proves per-pixel transparency and scoped blur integration with Picom GLX. It does not claim parity with NextKDE's KWin liquid-glass refraction effects. The test uses screenshots of its own generated desktop only.

Picom enables default background blur when a blur method is configured. The first, unconditional `blur-background = false` rule is required; the following `_DECEBAL_BLUR = 1` rule selectively enables it. Without this default-off rule, even a zero-hint bar's unused transparent popup canvas blurs ordinary windows beneath it. The checkerboard regression proves that fine desktop detail stays sharp after switching to a non-glass theme.
