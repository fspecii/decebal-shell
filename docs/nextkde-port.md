# NextKDE design adaptation

Reference: [NextKde/NextKde](https://github.com/NextKde/NextKde), commit `3fa1d7006869bbd3404a05d362c6e6028e0fe3da`, inspected on 2026-10-29. The upstream repository's root `LICENSE` is GNU GPL version 3. Its desktop screenshot (`docs/images/desktop.png`), `apps/settings/pages/ThemeSettingsPage.qml`, `apps/settings/components/SettingsCard.qml`, `apps/settings/pages/WidgetAppearanceSection.qml`, `shell/desktop/modules/deskcenter/DeskWidgetCard.qml`, `shell/desktop/modules/weather/DockWeatherWidget.qml` and `shell/desktop/modules/dock/DockTemperatureWidget.qml` were design references.

This is an original Decebal implementation inspired by that design, rather than a transplant of the Qt/Quickshell implementation. No upstream QML, JavaScript, fonts, icons, wallpapers or other assets are distributed here. The theme manifest, shared widget definitions and geometric wallpaper artwork are original MIT-licensed Decebal content. `themes/nextkde/LICENSE.md` contains the license; `ARTWORK.md` contains the complete original SVG source for both wallpaper renders. NextKDE remains the name of the independent upstream project; this theme does not imply endorsement.

## Design mapping

The reference combines a thin translucent global top bar, centered floating dock, generous rounded glass cards and restrained blue selections. Decebal maps those ideas to its existing macOS decoration/dock engine, 26-pixel top bar, 18-pixel surface radius and translucent dock/popup metrics. These are supported controls, not unused shader settings. Refraction shaders, Quickshell animations, KDE integrations, dock carousels and spatial wallpaper effects are not implemented by this data theme.

The original wallpaper uses broad, quiet overlapping glass ribbons so widgets remain legible. `glass-dark.png` is the default; `glass-light.png` is an additional manually selectable asset. The current theme engine selects the first wallpaper image, so switching appearance mode does not automatically switch wallpapers. Wallpaper matching is off by default to keep the user's palette independent. The theme's dark/light colors supply defaults; a separately selected global palette continues to recolor all tokenized surfaces.

Settings design guidance for the separate app redesign: a muted sidebar, grouped navigation, blue selected capsules, white/tonal rounded content cards, clear section labels and broad gutters. Desktop structure and palette should remain separate choices.

## Shared widget concepts

All three definitions live in the repository-level `widgets/` directory. They use theme tokens exclusively and never test the active theme ID. They contain no commands, executable hooks, asset paths or third-party services of their own. Each can be enabled independently in the global widget settings and runs on every theme. The theme deliberately has no widget placements, so selecting it cannot replace global widget choices.

- `next-clock`: digital time, weekday and full date from the shared local clock.
- `next-weather`: current temperature and symbolic condition from Decebal's existing weather source. An explicit waiting state appears when data is unavailable. No forecasts, location or condition descriptions are fabricated; the source currently provides none of those strings.
- `next-system`: CPU history and load plus memory usage from the existing shared system sampler. It does not start extra polling processes.

The reference's analog clock/weather/resource-card concepts are adapted to the supported widget DSL. Widget actions only open the existing dashboard. Clock/date formatting follows the shared formatter; weather is currently Celsius, matching the shared data source.

Integration registers `nextkde` in `src/theme/builtin.rs` and the three `next-*` definitions in `src/widgets/samples.rs`. The install target recursively copies theme packages, including both wallpapers, artwork sources and license. No user configuration or running desktop has been changed by this work.

## Validation

Using `/home/ambsd/Projects/macshell-appearance/target/debug/` binaries:

```sh
decthemectl theme validate themes/nextkde
for f in widgets/next-*.toml; do decwidgets validate "$f"; done
```

All pass under the strict theme-data parser. Both 3840×2160 PNG wallpaper assets were generated with `rsvg-convert` from the SVG source embedded in `ARTWORK.md`; the dark wallpaper and upstream reference screenshot were visually inspected. All three widgets were rendered and inspected with fixed sample data under the default active palette and isolated NextKDE dark/light configuration:

```sh
DECEBAL_CONFIG=/tmp/nextkde-render-dark DECEBAL_THEMES="$PWD/themes" \
  decwidgets render widgets/next-system.toml --png /tmp/next-system-dark.png
```

The isolated config folder contains `decebal.json` with `{"theme":{"name":"nextkde","mode":"dark"}}` (or `light`). System card layout was corrected after the first render so the CPU/memory values align to the right. A contrast calculation on uncomposited theme tokens checked primary, secondary and faint text against popup plus accent text against accent: minimum 6.69:1 dark / 5.03:1 light. Actual compositing varies with wallpaper; the popup is 94% opaque. No complete desktop runtime or fresh weather-provider/network check was performed in this bounded package task; parent integration handles built-in registry coverage and the live desktop build.

### Integration review

The integrated package in `decebal-theme-studio` passed `decthemectl theme validate themes/nextkde` with SHA-256 `af5e008f353b4a9ecd1648ab9ccb8415482b74565341752f2e6cfb9ff6463973`. Eighteen isolated file-definition renders passed: three widgets × NextKDE/Windows 11/Caelestia × dark/light. NextKDE dark system, Windows light clock and Caelestia dark weather renders were visually inspected; cards adapt their colors and radii without depending on the active desktop style. Outputs are private temporary QA artifacts, not installed theme content. The then-current `target/debug/decwidgets` predates registration, so these were file-based renderer checks; the parent integration build verifies compiled built-in registration and runs the full suite.


Parent integration completed a fresh compiled registry build, the 30-check private desktop theme application run, and dark/light gallery captures (`docs/themes/nextkde*.png`). The package now also includes an actual desktop screenshot at `screenshots/main.png`; this changes the package content hash from the earlier bounded review. Settings was visually checked at normal and narrow sizes. The release and combined test results are recorded in the project handoff.
