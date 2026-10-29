# Isolated X11 glass verification

Run `tests/e2e/glass.sh`.

The test builds decbar, decwidgets and decthemectl with two build jobs, then starts private Xvfb and Picom XRender with the scoped rules from `contrib/picom/decebal-glass.conf` and a compatible kernel blur override. It uses temporary HOME, config, data and runtime directories. Display `:231` is the default; `E2E_DISPLAY` selects another unused private display. Occupied displays and `:0`/`:1` are refused. `PROFILE=release` selects release builds, `NO_BUILD=1` reuses binaries, and `KEEP=1` retains temporary logs and screenshots.

Checks cover:

- Resetting from Nord and its palette restores NextKDE's own colours.
- NextKDE bar and desktop analog clock report `_DECEBAL_GLASS_NATIVE=1` after successful backdrop capture, disable external Picom blur to avoid double filtering, and the live clock has a 32-bit ARGB visual.
- The bar's bounding shape excludes unused popup canvas over the desktop clock, keeping Picom from blurring widgets beneath that transparent canvas.
- Root screenshot pixels inside both surfaces change when the wallpaper changes between two solid colours. The pixels differ from the bare wallpaper as well, proving that an actual tinted surface is composited over the background.
- A generated checkerboard changes through the native card bezel compared with straight alpha compositing, proving a captured/transformed backdrop rather than a marker alone.
- Switching to Nord clears native and blur hints on both shared surfaces while Picom remains alive.

This proves captured native glass, per-pixel composition and the Picom fallback boundary. It does not claim parity with NextKDE's KWin liquid-glass refraction effects. The test uses screenshots of its own generated desktop only.

Picom enables default background blur when a blur method is configured. The first, unconditional `blur-background = false` rule is required; the following `_DECEBAL_BLUR = 1` rule selectively enables it. Without this default-off rule, even a zero-hint bar's unused transparent popup canvas blurs ordinary windows beneath it. The checkerboard regression proves that fine desktop detail stays sharp after switching to a non-glass theme.

The default test compositor is XRender because this machine's private Xvfb/GLX path sometimes leaves the composited root stale while the actual bar client pixels update correctly. The test records both client and root pixels; it retains strict root freshness assertions. `PICOM_BACKEND=glx` reproduces that environment-specific artifact. The native material renderer is independent of Picom's blur/shader backend. Production GLX behavior is a separate live-session check.

The suite also stops and restarts its private compositor, verifying the widget watcher recreates desktop surfaces as 24-bit fallback windows and then restores 32-bit ARGB windows. This covers compositor/login startup ordering.
