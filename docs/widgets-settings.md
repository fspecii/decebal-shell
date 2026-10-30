# Desktop widgets in System Settings

Open **System Settings → Widgets** in the Appearance sidebar group, or run `decsettings widgets`.

Widgets follow the selected theme. A theme with active widgets supplies its layout; a theme with no active widgets hides the collection while retaining the saved instances. If you have customised the saved layout, applying a theme with widgets asks **Keep my layout**, **Use theme widgets**, or **Cancel** before changing anything. Palette and light/dark changes within the same theme preserve the layout.

- **Show widgets** hides the whole desktop collection while retaining every enabled state and position.
- **Your widgets** shows saved instances, including disabled and unavailable entries. Each can be enabled, disabled or removed. Desktop widgets have a screen anchor and horizontal/vertical pixel offsets; positive offsets move right/down and negative offsets move left/up.
- **Available widgets** includes shared built-ins, user widgets and definitions supplied by installed themes. Adding an already configured but disabled widget enables the saved instance rather than creating a duplicate.

The page edits `decebal.json`'s `widgets` object, preserves unknown keys and per-instance options, and never changes the selected theme or palette. Changes use the widget host's live file watcher. Enabling the first widget starts the host when it is available. Native Wayland is reported as unsupported by the current desktop host rather than claiming the widget was displayed.

Definition errors appear beside the affected widget. Invalid JSON is left intact; repair it through Config & Presets. Removing a saved instance is explicit; the normal way to try hiding a widget is its enable switch.

## Creating your own widget

**Create a widget** (second card of the page) is the no-code way to make a card. **Create widget…** opens a form with a live preview on top:

| Type | What it shows | Options |
| --- | --- | --- |
| Digital clock | Large time and optional date | 24-hour or 12-hour time, show the date |
| Analog clock | A round clock face | Seconds hand |
| System metrics | Titled card with bars and live values | Processor, memory, root disk, network speed (at least one) |
| Static note | A title and your text, wrapped onto up to six lines | The note itself (up to 200 characters) |
| Quick note · Notes app | Latest saved note, with Open and New note actions | Shared with Notes; no static text required |
| Timer · shared countdown | Remaining time, status and progress | Click to open Timer controls |

Quick note and Timer previews use fictional data. On the desktop, Quick note can reveal private content; its text is edited in Notes. The timer uses the app’s persisted countdown and continues while the app is closed. Neither template runs shell commands.

Every type also has a **Name** (1 to 40 characters; the title of metrics and note cards), a **Size** (Small, Medium or Large scales the whole card), and a **Position** (screen anchor plus horizontal and vertical offsets, like any other card). The suggested position sits clear of the cards already on the desktop until you type an offset yourself.

The preview is the shipped widget painter drawing the exact widget file that Create would save, with the theme currently in use and fixed, labelled sample data (Friday 23 October, 14:07, CPU 37%, memory 62%). It never reads live data, and it never starts the widget host. **Create widget** stays disabled, with the reason shown, until the name, note text and chosen metrics are valid.

Create does the following, and nothing else:

1. Writes `<name>.toml` into the user widget folder (`$DECEBAL_WIDGETS`, else `~/.config/decebal/widgets`). The file name is the name lower-cased to ASCII letters, digits and dashes (`Desk Clock!` becomes `desk-clock`). A name already used by a file, a symlink (even a dangling one), a shipped widget or a saved card gets `-2`, `-3`, ... instead. The file is created exclusively, so an existing widget file is never replaced, and an unsaved settings file never leaves a stray widget behind.
2. Adds an enabled desktop card for it to `decebal.json` through the same save path as the catalog and turns **Show widgets** on. Other cards, per-card options, unknown keys, `allow_exec` and the theme are preserved. Because the layout now differs from the theme's, applying a theme with widgets asks **Keep my layout / Use theme widgets / Cancel** first, as for any customised layout.
3. Starts the widget host if it is not running (X11 only), exactly like **Add**.

Templates only produce data-only widgets: no `exec`, no `[commands]`, no click actions other than the allowlisted **open dashboard** on the metrics card, so they work with `allow_exec` off. Text you type is stored literally (braces and quotes cannot start a data reference or break the file), and invalid or oversized input is refused rather than truncated.

The result is a normal user widget. It appears under **Your widgets** and **Available widgets**, can be disabled, moved or removed there, and its first lines say so. To change the content later, edit the file with any text editor (the format is in docs/theme-engine.md); to delete it, remove the card and then the file. Settings does not rewrite or delete widget files on its own, so hand edits are never lost; creating a similar widget with the form is the way to start over.

Model regression checks:

```sh
cargo test --lib settings::widgetspane
cargo test --lib settings::widgetchoice
cargo test --lib settings::widgetbuilder
cargo test --lib settings::app::key_tests::create_widget_form_works_through_real_controls
```

The creator tests run entirely in temporary directories (a private `decebal.json` and `DECEBAL_WIDGETS`), draw to in-memory pixmaps and never start the widget host or touch D-Bus. `WIDGET_EVIDENCE_DIR=/some/dir` keeps PNGs of the form and of every template at every size.

The isolated desktop lifecycle regression starts its own Xvfb and rdwm with temporary HOME, configuration, data and runtime directories:

```sh
tests/e2e/widgets-settings.sh
tests/e2e/theme-widget-choice.sh
```

It builds the three tested debug binaries by default (`PROFILE=release` selects release; `NO_BUILD=1` uses existing binaries). `RDWM_BIN_DIR` selects the rdwm build and `E2E_DISPLAY` selects an unused private display (default `:219`). Occupied displays and live displays `:0`/`:1` are refused.

The regression exercises actual Settings controls and checks X11 windows: master hiding destroys windows while the host remains alive, showing recreates them, individual toggles retain saved instances, and the catalog re-enables an existing disabled widget without duplication or losing options/placement. It also launches the daemon twice. Theme replacement and cancellation are covered by `cargo test --lib settings::widgetchoice` and `cargo test --lib theme::engine::apply`. All processes and files belong to the temporary test desktop and are cleaned up on exit.

## Adding a widget

New shared widgets belong in `widgets/` and are registered in `src/widgets/samples.rs`. Use semantic colour tokens (`surface`, `text`, `accent`, etc.) and the shared data sources; avoid active-theme checks and fixed palette colours. They then appear automatically in the Widgets collection for every desktop design. Theme packages may also supply definitions, which remain available through the shared collection while that package is installed.

Add the widget's layout golden, validate it under theme trust, and inspect dark/light renders under multiple designs. Keep unavailable data explicit. Theme manifests supply their widget defaults. Untouched theme layouts switch automatically; custom layouts require a replacement choice. `decthemectl theme apply ID` requires `--keep-widgets` or `--replace-widgets` when a custom layout exists. Themes without active widgets hide saved cards; the host keeps cards clear of pill bars when the bar changes edge.

Adding or re-enabling a card through **Add/Enable** also turns on the desktop collection, so an apparently successful Add cannot leave it hidden by the master switch. The normal master switch still preserves every individual enabled state and placement.

`decwidgets status --json` reports whether the host on the current X display completed its first render, the selected config path, master visibility and definition errors for saved desktop cards. Host startup waits for this ready signal rather than only checking that a process acquired a lock.

Desktop cards sit above mapped `DESKTOP` wallpaper windows and below managed applications. With rdwm's root-pixmap wallpaper, they remain below applications and shell panels. Non-composited translucent cards sample the wallpaper pixmap rather than capturing app contents, and refresh when that pixmap changes.

The stronger visibility regression checks real root pixels and real click behavior on private Xvfb with rdwm and generated wallpapers:

```sh
CARGO_TARGET_DIR=/home/ambsd/Projects/macshell-appearance/target tests/e2e/widgets-visible.sh
```

It starts with an empty, hidden collection and uses the actual Settings Add button to start the host. It proves visible pixels above wallpaper, click delivery, coverage by fullscreen apps, palette/theme/restart persistence, duplicate prevention, support for a late desktop window, and refreshed translucent wallpaper sampling. This complements the saved-state lifecycle checks; mapped-window counts alone do not prove a working desktop widget.

Login integration uses `decwidgets autostart`: it quietly exits in Wayland/Xwayland and headless contexts, and runs the persistent watcher on X11 even when the collection is empty or hidden. A compositor arriving later or restarting causes the host to recreate surfaces with the appropriate visual. `decwidgets daemon` remains the explicit foreground daemon command.

The host advertises readiness only after its first rendered frame. Adding a widget also reveals a hidden collection. X11 startup uses `decwidgets autostart` (a safe no-op outside X11) or the packaged `decebal-widgets.service`; rdwm's session supervisor starts and monitors the shared daemon. No data workers are started while the collection is empty or hidden. The visibility regression `tests/e2e/widgets-visible.sh` checks actual pixels, clicks and stacking with a real window manager rather than relying on window counts alone.

### Network and storage cards

The shared catalog also includes **Network traffic** (`builtin:network-traffic`) and
**Root storage** (`builtin:storage-card`). Both open the existing dashboard when
clicked, use semantic palette colours, and reuse the shared one-second system
sampler without commands or extra services. They do not change theme defaults.

Network traffic shows aggregate receive/send rates from the existing sampler
(which excludes loopback and common container bridges/veth pairs), and two independently scaled
60-sample histories (normally 60 seconds while the host is active). It is a
throughput monitor, not a connectivity test. Root storage shows the filesystem
mounted at `/`, with free/used/total capacity and a usage bar; it does not enumerate
other mounts. Missing values say **Unavailable**; legitimate zero values remain
visible. Missing graphs are empty and a missing storage value leaves an empty bar.

Reproduce layout goldens and actual dark/light renders with installed fonts:

```sh
cargo test --lib widgets::samples
WIDGET_EVIDENCE_DIR="$PWD/docs/widget-evidence" cargo test --lib widgets::samples::tests::shared_cards_render_in_dark_and_light_designs
```

The committed PNGs in `docs/widget-evidence/` are actual tiny-skia widget output,
using fixed sample data and missing-data states in Nord and macOS dark/light
variants. No live desktop, configuration or theme application is needed.

### Rice and mobile-inspired collection

These shared widgets are available in Settings → Widgets on every theme. Their
shapes and layout borrow from familiar desktop rice and mobile design styles;
all colours still come from the current semantic palette.

| Widget ID | Design and live data | Interaction |
| --- | --- | --- |
| `terminal-session` | Square prompt frame, user/host, uptime, CPU and RAM meters | Open dashboard |
| `retro-meters` | Hi-fi inspired twin resource dials, load and core count | Open dashboard |
| `ios-day` | Rounded date tile with current cached weather | Display only |
| `berry-hub` | Compact clock, focused app/title, workspace and angular shortcuts | Search, apps, notification history |
| `android-player` | Rounded media tile, track/artist, progress and transport controls | Previous, play/pause, next |

The hub does not invent message counts or connect to phone accounts. Weather uses
the existing cached source; playback uses the existing MPRIS source. Missing
weather and absent players show explicit empty states; playback controls disappear
when no player is present. Long app/track text is truncated. These additions do
not select or replace a theme's widget layout.

### Fetch summary and terminal wave

- `fetch-summary`: the real distro ASCII logo from Fastfetch beside its JSON
  OS, kernel, CPU model, memory, uptime and WM data. Requires `fastfetch`; click
  to open Dashboard. The logo follows the active semantic accent colour.
- `terminal-wave`: terminal-style music controls and a waveform-shaped **track
  progress** meter. Segments fill from left to right using MPRIS track position.
  This is not an audio spectrum and does not capture desktop audio. Controls hide
  when no player is present.

The compact `now-playing-mini` reserves space for all three transport controls,
even with cover art and wide track titles. Text is fitted to its allocated pixel
width with an ellipsis, so the Next button stays inside the card's hit region.


### Palette and material integration

Every built-in uses semantic theme colours. Choosing a separate colour scheme
recolours cards and their controls without changing their layout. Material
backgrounds now inherit theme surface opacity: popup/surface backgrounds use
`popup_alpha`, dock backgrounds use `dock_alpha`, bar backgrounds use `bar_alpha`,
and overlay backgrounds use `panel_alpha`. Explicit token alpha multiplies that
value. Theme opacity changes therefore affect existing cards too.

An optional saved instance option applies additional background transparency:

```json
{"use":"builtin:fetch-summary", "options":{"background_opacity":0.6}}
```

`background_opacity` ranges from 0 (no background fill) to 1 (theme material,
the default). It multiplies material fill opacity; text, icons, meter data and
button backgrounds retain their foreground contrast. Outlines are retained.
Zero disables native backdrop glass and external blur on that widget. These are
per-instance options, preserved by Settings; there is no new opacity slider.
On X11 without a compositor the host blends translucent cards over rdwm's
wallpaper pixmap; with a compositor it uses ARGB and the existing glass path.
Native Wayland widget hosting remains unsupported.

Fastfetch runs as a built-in data provider only when a visible card requests
`fetch.*`. Its command is fixed to JSON output and OS/Kernel/CPU/Memory/Uptime/WM,
with `--config none`; theme files cannot supply Fastfetch arguments or load user
Fastfetch custom commands. A second fixed invocation uses `--structure Break`,
`--logo-type builtin` and `--pipe true` to return just the real distro logo.
The pair shares a five-second budget; each output is capped at 64 KiB. The worker
refreshes at most once a minute. Missing Fastfetch, malformed output
and module errors produce explicit unavailable states. Failed refreshes clear
previous values. No package installation or external configuration is performed.

Validate the installed backend explicitly (the normal suite skips this optional
dependency probe):

```sh
cargo test --lib widgets::fetch::tests::installed_fastfetch_returns_real_system_data -- --ignored
WIDGET_EVIDENCE_DIR="$PWD/docs/widget-evidence" cargo test --lib widgets::samples
```


The `ascii` DSL leaf displays multiline data in a fixed monospace grid:

```toml
[root]
type = "ascii"
text = "{fetch.logo}"
size = 12
color = "accent"
```

ASCII art keeps its leading spaces, line breaks and block characters and scales
down as a whole to fit its slot. It never wraps or adds ellipses to a logo.
Rendering is bounded to 40 rows × 80 columns. Fastfetch logos containing terminal
escape sequences or control characters are rejected. Full-block characters fill
pixel-snapped cells without font-dependent seams; other characters use the
configured monospace role. If logo extraction fails, system data remains visible
with an explicit logo-unavailable status; no fabricated fallback logo is used.

### Compact resource and focus cards

`builtin:resource-overview` (320 × 248 logical pixels) groups CPU/RAM meters,
root free space and receive/send rates; clicking it opens Dashboard. Values come
from the existing shared system sampler. A missing value says Unavailable; a
measured zero remains zero. It deliberately makes no GPU, temperature or process
list promise.

`builtin:focus-hub` (320 × 292) joins the existing shared countdown with the latest
note title. Timer opens the existing countdown app; New note opens a blank note;
the muted latest-note panel opens Notes. It never starts/replaces a countdown or
reads note bodies for display. Countdown recovery and note storage are unchanged.
No shell commands, network requests, cover art or extra collectors are introduced.
Both cards are available in the shared Widgets catalog for every theme.

These are independently implemented native adaptations of Dux's compact metric
groups and Pomodoro hierarchy; see `dux-adaptation-handoff.md` for pinned upstream
sources and license boundaries. Semantic colours follow the selected palette.

The optional widget-level `auto_height = true` makes `size`'s height a minimum:
layout grows to measured content (up to 2000 pixels) when accessibility text is
larger. Existing files keep their fixed geometry, and an explicit layout fit
still wins. It does not scale fonts down. New cards use this opt-in and fit a
480-pixel-high display at 100% and 200% text size; custom placements and multiple
cards still need enough desktop space. Native Wayland widget hosting remains
unsupported. Visual evidence can be reproduced with the isolated renderer test:

```sh
WIDGET_EVIDENCE_DIR=/tmp/dux-widgets cargo test --lib overview_and_focus_native_scale_evidence -- --ignored --test-threads=1
```
