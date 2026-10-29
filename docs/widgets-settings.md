# Desktop widgets in System Settings

Open **System Settings → Widgets** in the Appearance sidebar group, or run `decsettings widgets`.

Widgets are a desktop preference shared by every theme. Switching themes changes their colours and typography without replacing the user's enabled widgets, saved options or placement.

- **Show widgets** hides the whole desktop collection while retaining every enabled state and position.
- **Your widgets** shows saved instances, including disabled and unavailable entries. Each can be enabled, disabled or removed. Desktop widgets have a screen anchor and horizontal/vertical pixel offsets; positive offsets move right/down and negative offsets move left/up.
- **Available widgets** includes shared built-ins, user widgets and definitions supplied by installed themes. Adding an already configured but disabled widget enables the saved instance rather than creating a duplicate.

The page edits `decebal.json`'s `widgets` object, preserves unknown keys and per-instance options, and never changes the selected theme or palette. Changes use the widget host's live file watcher. Enabling the first widget starts the host when it is available. Native Wayland is reported as unsupported by the current desktop host rather than claiming the widget was displayed.

Definition errors appear beside the affected widget. Invalid JSON is left intact; repair it through Config & Presets. Removing a saved instance is explicit; the normal way to try hiding a widget is its enable switch.

Model regression checks:

```sh
cargo test --lib settings::widgetspane
```

The isolated desktop lifecycle regression starts its own Xvfb and rdwm with temporary HOME, configuration, data and runtime directories:

```sh
tests/e2e/widgets-settings.sh
```

It builds the three tested debug binaries by default (`PROFILE=release` selects release; `NO_BUILD=1` uses existing binaries). `RDWM_BIN_DIR` selects the rdwm build and `E2E_DISPLAY` selects an unused private display (default `:219`). Occupied displays and live displays `:0`/`:1` are refused.

The regression exercises actual Settings controls and checks X11 windows: master hiding destroys windows while the host remains alive, showing recreates them, individual toggles retain saved instances, and the catalog re-enables an existing disabled widget without duplication or losing options/placement. It also launches the daemon twice and applies two installed themes, comparing the complete canonical `widgets` JSON value across switches. All processes and files belong to the temporary test desktop and are cleaned up on exit.

## Adding a widget

New shared widgets belong in `widgets/` and are registered in `src/widgets/samples.rs`. Use semantic colour tokens (`surface`, `text`, `accent`, etc.) and the shared data sources; avoid active-theme checks and fixed palette colours. They then appear automatically in the Widgets collection for every desktop design. Theme packages may also supply definitions, which remain available through the shared collection while that package is installed.

Add the widget's layout golden, validate it under theme trust, and inspect dark/light renders under multiple designs. Keep unavailable data explicit. Theme manifests may suggest first-run defaults but must never replace an existing user's widget collection. Widget positions are retained across theme switches; the host keeps cards clear of pill bars when the bar changes edge.
