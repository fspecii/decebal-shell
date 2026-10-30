# Native desktop-frame Settings acceptance

The Wallpaper pane exposes **Rounded desktop frame**, corner radius, thickness
and colour when the installed schema supports them and the running WM reports
`state.wallpaper.frame`. An upgraded offline CLI does not enable these controls
for an older running WM. The frame is independent of the theme and wallpaper
selection; resetting its colour follows the wallpaper background colour.

## Reproduce the end-to-end gate

```sh
SETTINGS_BIN=/absolute/shell/bin/decsettings \
RDWM_BIN_DIR=/absolute/wm/bin \
OLD_RDWM_BIN=/absolute/older/rdwm \
python3 tests/e2e/desktop-frame-settings.py /tmp/frame-settings-evidence
```

`OLD_RDWM_BIN` is optional. The test runs actual WM, Settings, widgets, live
wallpaper host, ffmpeg and mpv binaries. WM/schema/state responses are not
fabricated. Every case owns its Xvfb and uses private HOME/all XDG directories,
separate RDWM socket, inaccessible session/system buses and external-probe
stubs. It does not operate the live display or user configuration. The tiny
silent generated video is a test fixture, not a shipped/user wallpaper asset.

The first phase clicks the real control coordinates from
`DECSETTINGS_DEBUG_LAYOUT`. Enabling must create four native strips and update
WM state/config; disabling must remove them. Wallpaper image/mode/background,
config comments and byte-identical theme/unknown shell preferences must survive.
The optional older-WM case uses the new real CLI with an actual old WM and
requires the unsupported controls to disappear after runtime discovery.

The second phase places a data-only solid widget at desktop position0,0 and
starts the real video host/embedded mpv. The user toggle enables the frame while
both hosts run. Actual root stacking must settle as video < frame < widgets <
ordinary apps. Three workspace changes exercise native WM restacks. Twenty
pixel/stack samples must retain the entire edge widget, and bounded CPU samples
reject a feedback loop. Disabling must leave the real hosts/player running.
Executed binary/fixture hashes, live player status, UI rectangles, native window
IDs, stack samples, CPU ticks, screenshots and failures are saved in result JSON.
Children stop before their private display to keep cleanup diagnostics meaningful.

## Regression and validation boundary

The first optimized WM candidate897c308 (SHA5e18c25e...) failed the new real-host
phase: widget placement immediately above a DESKTOP wallpaper put widgets below
frame strips. Independent debug repairs mark the owned strips as DESKTOP layer
components and lower them before mapping. The same unchanged optimized Shell
then passed actual UI, host layering and older-session guard checks.

Final optimized repaired-WM acceptance is recorded below only after executing
the pinned artifacts. This gate does not establish Wayland/real hardware/mixed-DPI
visual parity, animation FPS, or long-running zero-CPU behaviour.

## Accepted optimized artifacts

Producer WM source `efa8f1f`, binary SHA256
`22eec653b2299f804999e258fb44548f8b5d125b93dbeb7175ea1446ec331236`;
CLI `ab2b2999be136454116bdb593d505850c0103238335e56806a42d6b7fcc7049a`.
Shell source `0e2fd63`, Settings SHA256
`96e25b700af19ab8659244f2b80500afda7ca6c3d85316b7156583d82a9ae607`.
Both actual native UI/host and older-WM/new-CLI cases passed:
`/tmp/rdwm-frame-settings-optimized-repaired/result.json`.

The real widget host SHA is `bababbd5877cd2ab210af4de8dd100d4f3bf27e46026f6b8abaa8fc3b1fe5917`;
real video host SHA is `4086b78f0ed10c49cff046d69c2ec4277a656fa682597bfe7ca93de817d2fb0b`.
The actual embedded mpv player remained alive, unpaused and error-free. Twenty
edge-pixel/stack samples passed after three workspace restacks; the bounded
2.42-second sample measured zero WM CPU ticks and one widget-host tick.
The 0,0 widget stayed whole, above every frame strip. Screenshots were visually
checked; the new frame card is present only with a supporting running WM.

The matching repaired optimized WM also passed its standalone backend gate,
including relative order/overlap pixels for two raised desktop surfaces and
an actual external `xsetroot` background retained through frame-only startup,
output resize and disabling:
`/tmp/rdwm-desktop-frame-optimized-repaired/result.json`.

These were private native tests. No installation, live configuration changes,
user input injection, live audio, daemon replacement or WM restart occurred.
