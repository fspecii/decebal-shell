# Contributing

## Layout

```
src/bin/        one file per program (decbar, decdock, decnotify, decsettings, decthemectl, ...)
src/theme/      theme model, engine (data-only theme packages), fonts, dynamic wallpaper colours, system-wide theming
src/settings/   System Settings panes
src/widgets/    desktop widgets and the safe widget DSL
src/menubar/    bar, Control Center, vertical (Caelestia) layouts
docs/           user documentation (theming, theme engine, presets, icons, wallpapers, Caelestia)
tests/e2e/      end-to-end scripts that run against a private Xvfb + rdwm; tests/*.rs are cargo tests
packaging/      .deb builder; PKGBUILD in the root
```

## Build and test

```sh
cargo build --release --locked
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
bash tests/e2e/theme.sh      # needs Xvfb and an rdwm build (RDWM_BIN_DIR); set MS_BIN_DIR and PROFILE for release builds
```

Keep the daemons light: `notify.sh` fails if `decnotify` idles above 10 MB RSS. Themes are data only (no code in
packages); see docs/theme-engine.md before changing the format, and bump `format` for incompatible changes.

Commits: short imperative subject, explain why in the body. Open an issue before a large change.
