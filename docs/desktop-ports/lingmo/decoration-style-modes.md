# Mode-specific window styles with a proven fallback (contract for every theme)

Code: `src/theme/engine/decoration_compat.rs`. Hooks: `schema.rs` (keys), `apply.rs` (resolution, record, rejection), `writer.rs` (version floor),
`preview.rs` (explicit preview mode), `src/settings/{io,decorations,panes}.rs` (running-WM style list, picker, persistent warning).
No theme or family id appears in the engine: the contract is generic and any theme may use it.

## Why

A static `[decoration] style` cannot pick the right hover, pressed, shadow and header row for light and dark: the native measured-port styles ship as
pairs (`openkylin`/`openkylin-dark`, `deepin`/`deepin-dark`, `lingmo`/`lingmo-dark`). And a theme must not claim a style the *running* window manager
cannot draw. The installed `rdwmctl schema --offline` describes the installed CLI, not the compositor that is running (the live one can be older).

## Manifest keys (all optional, identifiers; need `min_shell_version = "0.3.0"`, written automatically by `decthemectl theme pack/export`)

```toml
[decoration]
style = "adwaita"                 # older key: wanted style for a mode that has no style_<mode>; the only key engines without this contract read
style_light = "lingmo"            # wanted in light mode
style_dark = "lingmo-dark"        # wanted in dark mode
fallback_style_light = "adwaita-light"   # used in light mode if style_light cannot be proven
fallback_style_dark = "adwaita"          # used in dark mode if style_dark cannot be proven
fallback_style = "adwaita"        # either mode, after the mode-specific fallback key
```

Rules: desired = `style_<mode>`, else `style`. Fallback = `fallback_style_<mode>`, else `fallback_style`, else none. A theme with none of the five new keys
is untouched (the old `style` path and its notes are unchanged). Invalid identifiers are ignored. `height`/`radius` written by a theme override the style's native
geometry, so a theme that wants the native geometry of a pair leaves them out.

The mode is the applied mode (light, dark, or the system's when automatic). A mode switch, a palette or accent change and a full apply all go through the same
transaction, so the style follows the mode every time.

## What counts as proof of support (never the offline table of a running WM's CLI)

| answer | when | result |
|---|---|---|
| `Running` | the socket accepts and the running rdwm's `schema` reply lists `decoration.style` choices (and `decoration_styles` records, intersected) | its list is the proof |
| `Offline` | the socket is missing or nothing listens behind a stale file: no rdwm is running; the explicit local target `rdwmctl schema --offline` lists styles | proof, labelled "rdwm is not running… takes effect when rdwm starts" |
| `Preview` | only the native preview worker sets `DECEBAL_DECO_CAPS=preview`; it draws with its own decoration helper and never talks to a WM | wanted style, no gate, no warning |
| `Unknown` | a socket is there but the answer is an error, garbage, silent, an older rdwm without `schema`, or has no style list; or no usable target CLI while none runs | **never proof and never offline**: the apply stops *before any write* with a complete message |

### The probe is bounded (no thread, one deadline, no leftovers)

Everything happens on the calling thread under one absolute deadline (`Budget::DEFAULT`: 3 s for a running rdwm, 8 s for the offline helper), so a stalled window manager,
a full socket backlog or a misbehaving helper cannot hold theme planning longer than that:

* **Running rdwm:** one raw non-blocking Unix socket per question: connect (retried every few ms while the accept queue is full, which a non-blocking connect reports as `EAGAIN`),
  send the request line, read to the newline (cap 2 MiB), close. There is **no separate probe connection**, so nothing changes between "is it there" and "ask it". A missing socket or a
  refused connection (a stale file) is the only way to "not running"; every other failure is `Unknown`.
* **Offline helper:** `rdwmctl schema --offline` runs in its **own process group** with its output pipe non-blocking. The output is drained *while it runs* under a 4 MiB cap and the same
  deadline (a helper that writes more than a pipe holds can no longer block, and a parent that waited for exit before reading can no longer deadlock it). After end of output the helper must still
  exit within the deadline. On every path out (success, deadline, oversize, error, panic) the whole group is killed with `SIGKILL` while the leader is still unreaped, and the child is reaped, so a
  descendant that keeps stdout open neither hangs the probe nor outlives it. A freshly written helper script that is briefly "text file busy" is retried a few times.

Outcomes of `resolve`: the wanted style if listed; else the fallback **if the same source lists it** (the style is applied and a persistent warning is recorded); else an
error and nothing is written. A style proven this way is not vetoed by the installed CLI's older offline table; if `rdwmctl check-config` then rejects a style the WM was
proven to list, the apply stops with an error rather than dropping the style silently.

## Persistent warning

While a fallback is in use `decebal.json` holds `decoration_compat = {theme, mode, desired, applied, proof, message}`; Settings → Appearance → Window style shows the
message ("…needs a newer rdwm… Upgrade rdwm and log in again (or restart rdwm), then apply the theme again."). The record is removed by the next apply in which the wanted style
applies, and by applying any theme that does not use the contract. Nothing restarts or reloads the window manager because of it.

## Settings picker

`decorations::choices_for(schema, runtime)`: a running rdwm that listed its styles decides; one that is running but could not list them is offered only the older set (never the
measured ports); the offline table is used only while none runs. The six measured-port styles (`openkylin`, `openkylin-dark`, `deepin`, `deepin-dark`, `lingmo`, `lingmo-dark`) are in
the picker and the native-metrics catalogue (rows taken from the renderer's own records). `rdwm-schema.json` carries the 24 style names and the six records; no other key of the bundled
snapshot changed, so the snapshot-versus-installed-CLI key comparison keeps its meaning.

## Adopting it in another package (own files only)

Deepin:
```toml
[decoration]
style = "adwaita"
style_light = "deepin"
style_dark = "deepin-dark"
fallback_style_light = "adwaita-light"
fallback_style_dark = "adwaita"
```
openKylin:
```toml
[decoration]
style = "adwaita"
style_light = "openkylin"
style_dark = "openkylin-dark"
fallback_style_light = "adwaita-light"
fallback_style_dark = "adwaita"
```
Set `min_shell_version = "0.3.0"`, drop any `height`/`radius` you only copied from the old fallback, and add a test that `decoration_compat::wanted` returns the pair for both modes.
Native geometry of the pairs from the running renderer: openkylin 38 px / 12, deepin 50 / 8, lingmo 30 / 11.
