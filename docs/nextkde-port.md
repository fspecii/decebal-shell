# NextKDE reference adaptation

> Wallpaper policy update (2026-10-30): the generated wallpaper/SVG/package-shot
> experiments described below are historical and have been removed from visible
> packages. Use **Wallpaper → For this theme** and `data/wallhaven-curated.json`.
> Theme apply does not own wallpaper. Layout, palette and native widget references
> remain useful; old art paths/commands below are not current reproduction steps.


Reference: [NextKde/NextKde](https://github.com/NextKde/NextKde), commit `3fa1d7006869bbd3404a05d362c6e6028e0fe3da`, inspected on 2026-10-29. The upstream root `LICENSE` is GNU GPL version 3. Reference files inspected include `docs/images/desktop.png`, the Theme Settings page and Settings Card, `WidgetAppearanceSection.qml`, `AppearanceTokens.qml`, `DeskWidgetCard.qml`, `DeskClockFace.qml`, `DockWeatherWidget.qml`, `DockTemperatureWidget.qml` and the shared glass README.

This is an original Decebal adaptation of the reference, not an exact port of its compositor implementation. No upstream QML, JavaScript, fonts, icons, wallpapers or other assets are distributed here. The manifest, shared widget definitions are original MIT-licensed Decebal content. `themes/nextkde/LICENSE.md` contains the license. The former gradient masters and PNGs are removed. NextKDE is the independent upstream project's name; this theme does not imply endorsement.

## Appearance

The actual reference screenshot combines a quiet slate-to-aqua gradient, a nearly invisible top bar, centered floating glass dock, white monochrome widget ink and pale hairline card contours. Version 1.1 follows those features with an explicit low top-bar alpha, translucent dock, 22-pixel rounded surfaces and glass widget cards. It avoids opaque popup-colored widget plates and unrelated decorative wallpaper shapes.

Upstream `AppearanceTokens.qml` declares zero painted bar/widget opacity for glass forms and white widget ink at 78% opacity. `DeskWidgetCard.qml` delegates its glass surface to KWin with a subtle scrim. Decebal adapts that appearance using supported alpha/scrim controls: the theme's bar alpha is 6%, dock alpha 18%; widget fill is a 14% dock-token scrim with a 32% foreground hairline. Popups/settings retain more opaque surfaces for reading controls. These are material defaults, not a claim of KWin shader equivalence.

Actual liquid refraction, adaptive contrast scrims, continuous-corner geometry and independent per-card blur/refraction remain compositor capabilities beyond this package. The root backend handles alpha/compositor support. A package cannot recreate those effects by naming a theme or changing RGB colors.

The package declares explicit `wallpaper.dark` and `wallpaper.light` artwork. A full theme apply selects the effective appearance variant. Later mode changes swap package-owned artwork; palette/customisation changes preserve user wallpaper choices. Wallpaper color matching is disabled by default so the user's separately selected palette remains independent. Widget surfaces and ink use theme tokens; changing palette recolors them everywhere. Glass legibility depends on the actual backdrop and needs desktop/compositor verification, not only uncomposited token contrast calculations.

## Shared widgets

These repository-level definitions contain no active-theme checks, command hooks or copied upstream code. Each can be enabled independently on every theme. The theme has no `[[widgets]]` placements and therefore cannot replace global widget choices.

- `next-clock`: 192×192 transparent glass square with a live monochrome analog dial, hour numerals and second hand. Uses the shared `analog-clock` DSL leaf and local `Data.time`, rather than static artwork.
- `next-weather`: 400×192 wide glass card with real temperature and condition symbol, date and Celsius unit label. Missing weather data produces an explicit waiting state. Forecast, city and condition descriptions visible upstream are omitted because Decebal's shared weather source does not provide them.
- `next-system`: 192×192 glass square with live CPU resource ring and memory bar. Uses the existing shared sampler without separate polling processes.

All widget glass ink uses `bar_text`, including the NextKDE light variant, while fill uses `dock`; both follow independent global palettes. These foreground tokens make the shared definitions usable with other desktop structures. Clicking a card opens the existing dashboard.

Integration registers `nextkde` in `src/theme/builtin.rs` and all three `next-*` definitions in `src/widgets/samples.rs`. Installation recursively copies theme packages, including original artwork, wallpapers and license. No running desktop, user preferences or live theme was modified by this bounded task.

## Validation

Run against the current integration build:

```sh
decthemectl theme validate themes/nextkde
for f in widgets/next-*.toml; do decwidgets validate "$f"; done
```

Version 1.0 passed package validation and eighteen isolated renders across NextKDE, Windows 11 and Caelestia in dark/light modes. Version 1.1 passed current-build theme validation (package SHA-256 `dfd602a47ecb25d273cb9fc29d29bc9fa8a5403219146aa71fed09711f562318`) after alpha-range and analog-clock additions. All eighteen revised file-based widget renders passed across NextKDE, Windows 11 and Caelestia in dark/light modes. NextKDE dark analog and light weather renders over the actual gradient were visually inspected; all-builtin contrast verification passes using the renderer's dedicated bar-text role. Dark dock base color remains dark enough for ordinary text tokens; light dock base remains pale.

A 1920×1080 private Xvfb + actual rdwm desktop displayed the running analog/weather/system cards, top bar, floating dock and full Settings window at 940×680. The Picom XRender backend with scoped Gaussian blur produced the accepted fully composited root screenshot, not a manually combined image. Its log was clean. The equivalent GLX backend on Xvfb produced a black Settings client inside its otherwise-painted frame, even with blur disabled and `use-damage=false`; direct client captures remained fully painted. Disabling the compositor and using XRender both restored the visible client. This isolates an Xvfb/GLX composition incompatibility rather than a Settings repaint or theme failure. It does not establish that hardware GLX fails on the live desktop.

Private matrix scripts and evidence were retained under `/tmp/nextkde-matrix-*`; the accepted Gaussian screenshot is `/tmp/nextkde-matrix-floating-xrender-gaussian.zHn1m4/root.png`. The private Picom config used `backend="xrender"`, `use-damage=false`, Gaussian size 12/deviation 5 and the same `_DECEBAL_BLUR` scoped rule as the repository configuration. Only the screenshot test configuration changed; live Picom configuration was untouched. Parent integration owns final live compositor verification.

A private render configuration uses a temporary folder containing `decebal.json` with `{"theme":{"name":"nextkde","mode":"dark"}}`, plus `DECEBAL_CONFIG=<temporary-folder>` and `DECEBAL_THEMES=<checkout>/themes`. Render with `decwidgets render widgets/next-clock.toml --png <output>`; select a photograph separately for photographic QA. This does not apply a live theme.

The X11 glass path now tags translucent shell surfaces with `_DECEBAL_BLUR = 1` when `metrics.blur` is nonzero. `contrib/picom/decebal-glass.conf` enables background blur only for these tagged windows. Bounding shapes exclude the bar's unused transparent popup canvas, so it cannot blur desktop widgets behind that canvas. This implements transparent tint and frosted backgrounds; upstream KWin's refractive/distortion shader remains a separate effect and is not reproduced.


## Native material production pass

The shared original Decebal material implementation exposes separate refraction, edge-width, highlight, grain and background-blur controls. NextKDE requests 10-pixel refraction, 12-pixel bezel, 0.35 highlight, 0.003 grain and 4-pixel blur. This is no longer described as plain alpha plus compositor blur. Dock metadata follows the actual magnified plate; bar/menu/widget metadata describes actual rounded material rather than unused canvas or pointer bridges. The root native backend samples window/root-pixmap content underneath the surface with a screen-registered cache refreshed at approximately 4 Hz. This is an independently implemented edge-lens material and does not claim frame-exact KWin refraction or copied upstream shaders. Final actual desktop captures must follow native-renderer integration, including a textured backdrop check that exposes edge displacement.

Selection-only mode/palette/customisation commits now use the complete atomic package transaction while preserving selected palette/accent, custom wallpaper and global widget JSON. This prevents a color change from replacing package window-decoration/dock structure with legacy defaults. Full design application intentionally resets colors to that theme's defaults.


Final native-renderer desktop captures were generated and visually inspected with current debug binaries on isolated 1920×1080 rdwm/Picom desktops: light `/tmp/nextkde-matrix-nextkde-native-light.FsQbng/root.png`, dark `/tmp/nextkde-matrix-nextkde-native-dark.2aGgv4/root.png`. The theme's package screenshot now uses the actual native light capture. Foreground remains crisp; these actual gradient-desktop images establish whole-desktop composition and layout. Dedicated textured-backdrop checks are owned by the renderer integration and establish refraction rather than relying on a smooth wallpaper to make it look plausible.
