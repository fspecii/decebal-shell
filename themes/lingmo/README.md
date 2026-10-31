# Lingmo for Decebal

The look and layout of **Lingmo OS's current Wayland desktop** (`lingmo-compositor` + `lingmo-sdk`, pinned in
[`docs/desktop-ports/lingmo/sources.json`](../../docs/desktop-ports/lingmo/sources.json)): one package, light and dark.

**Status: NOT READY.** Palette, metrics, panel layout and branding are measured from source and validate against the
shared panel contract (`min_shell_version = "0.3.0"`). It is not a replica. Known differences, in
[`docs/desktop-ports/lingmo/README.md`](../../docs/desktop-ports/lingmo/README.md):

- the real status bar picks light or dark ink from the wallpaper behind it; this one follows the mode;
- the window decoration uses the nearest existing native style (`adwaita`); Lingmo's close button has no red state and its
  corners are continuous, which needs a native `lingmo` style (contract written, not implemented here);
- the dock's 16 px icon gap has no theme key; glass values are global metrics, not per-surface.

Icons (`Lingmo`), cursors (`lingmo-light`/`lingmo-dark`) and wallpapers are **not bundled**. The manifest has no `[icons]` section and writes no wallpaper. The
`Lingmo` icon pack is described for the shared asset installer in `assets/desktop-ports/lingmo/asset-packs.toml` and installed only on request; its `[[recommend]]` entry
lets the shared contract use the pack only when it is installed and the user has not chosen icons explicitly. No cursor pack is catalogued (no upstream licence file). Matching photographs are recorded as
curated recommendations (`docs/desktop-ports/lingmo/wallhaven-lingmo-records.json`, authorship and licence unknown): if the user has turned on **Follow theme** in the Wallpaper pane,
a theme or mode change prepares the matching photograph; otherwise the wallpaper stays independent, and nothing downloads on its own. Layouts, mfact, nmaster
and smart_gaps are the user's global behaviour and are not set here.
