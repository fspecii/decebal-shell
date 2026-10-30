# Desktop readiness audit — current checkpoint

Refreshed 2026-10-30 from accepted production code/layout `1d0d4b9`,
independent QA `2f8fd5d` and final Dux evidence `c372f45`, with production deployment evidence below. This records actual
source, private native tests and bounded live rollout; it is not proof of
KDE/GNOME parity or a hardware validation claim. The original audit is preserved
at `a895427:docs/desktop-readiness-audit.md`; its claims are historical.

## Evidence boundaries

- Latest Dux/notification integration: **1,604 default-feature library tests passed,
  zero failed, ten ignored**; formatting and strict all-target Clippy passed.
  Four release binaries (`decsettings`, `decwidgets`, `decnotify`, `decthemectl`)
  built with default X11/Wayland features and no test-auth. Exact release native
  Settings six-case checks include light/dark, 100/200% UI/text, short viewports,
  keyboard navigation and retained drafts. Widget host input (eight safe app
  clicks), combined 1280×720 readable four-card layout, notification theme/history,
  wallpaper-follow/old-producer compatibility and private Dux install all passed.
  Dux/curated package tests pass 8/8, strengthened final Dux tests 4/4 and the
  complete art policy 4/4. These are private synthetic-data checks, not live app
  telemetry or hardware/Wayland widget evidence.
- Rollout `~/.local/state/decebal/deployments/dux-20261030-160404/journal.json`
  records all eight binary paths/hashes/backups, validated Dux no-apply package
  install and pidfd handoff of the manual widget host to PID2869626 at nice0.
  Source guard retains exact accepted documentation-only README equivalence;
  production source/data/widgets/manifests did not change during build. Whole
  protected config hashes and WM state were identical before/after. Open Settings,
  legacy notification daemon, bar/dock, WM and Picom identities were preserved.

- Full default-feature production baseline `019797a`: formatting and strict
  all-target Clippy passed; **1,591 library tests passed, zero failed, nine
  ignored**. Six release binaries used default X11/Wayland features, no
  `test-auth`. Actual private native checks passed Terminal Apply/error/widget
  choices and compiled clock, Files light/dark+Nord, notification recovery, SNI
  horizontal/rail/taskbar/OBS/overflow and missing-bus/reconnect/icon refresh.
- Final `08c07a2` source gate: **1,597 library tests passed, zero failed, nine
  ignored**, formatting and strict all-target Clippy passed. The affected
  `decthemectl`/`decsettings` release pair passed 11 independent production
  wallpaper groups, including unchanged earlier-producer JSON into the new
  worker, derived-colour failures/retry, fixed sources, concurrent choices and
  one-generation light/dark refresh. NextKDE switch/reapply and reduced-motion
  regressions passed; production export includes magnification 1.4.
- Persistent evidence/backups/hashes: user-state deployment directories
  `~/.local/state/decebal/deployments/20261030-source-freeze-f5982d5/` and
  `20261030-follow-hook-08c07a2/`. Each records exact sources, feature flags,
  executable hashes and rollout. Replacements in both user bindirs were atomic
  per file with a journal and rollback; this was not a single cross-directory
  atomic rename. Full native baseline and affected-pair follow-up are distinct.
- Running bar/widgets were updated at normal nice 0; no WM/Picom/Nautilus restart.
  The open Settings process was preserved when its on-disk binary was replaced.
  Legacy notification daemon was deliberately retained: new durable-history
  code on disk does not mean that old process uses it. The privileged system
  session wrapper also remains an independent installation boundary.
- Colour refresh uses a bounded latest-selection reread before its existing
  transaction; this is not a cross-process compare-and-swap. The controlled late
  choice regressions pass, but arbitrary external edits after the final reread
  remain a race boundary.
- A green test count does not establish hardware presentation FPS, mixed-DPI,
  real Wi-Fi pairing, screen-reader interoperability or a complete Wayland flow.

## Requirement matrix

| User workflow | Present implementation/evidence | Remaining limit or acceptance evidence |
|---|---|---|
| Change a coherent desktop design | Transactional themes, independent palettes, reviewed widget replacement and visible validation errors; native Terminal Minimal preserves radius 24 while composing legal SSD 20/20; packaged and compiled clock Keep/Replace pass | Full desktop parity unproven. Wallpaper stays independent unless the user enables the separate curated-photo follow switch. |
| Browse attractive wallpapers | Wallhaven browse/favourites and curated per-theme/mode photos; generation-guarded async follow worker; reviewed SHA+dimension cache import; 42 originals imported, three unknown entries skipped; post-photo Dynamic refresh preserves independent/fixed/disabled choices; [guide](wallpaper-follow.md) | Live API can return 403; failures and partial colour-refresh success remain visible. Reviewed hashes prove byte identity, not photographer identity, licence or absence of AI. Physical crop/readability checks remain. |
| Live wallpaper with real preview and fullscreen savings | DesktopHut/Commons search, muted video preview; per-output player and pause on visible fullscreen, retained frame; `src/livewall/{pane,preview,host,mod}.rs`, `tests/e2e/live-wallpaper.py` | Runtime needs mpv/ffmpeg, Wayland mpvpaper; provider availability can change. Current full provider/hardware run not established by this audit; no zero-GPU-cost promise for a paused process. |
| Apply themes to applications | Managed GTK blocks/settings, Qt5ct/Qt6ct colour schemes, terminal/Omarchy integration and rollback; `src/theme/system/`; native GTK/app suite 89 passed | GTK applications may require reopening. Qt needs correct platform-theme environment; Qt pixel/real-app restart verification remains missing. Third-party application custom UIs are not universally themed. |
| Recover application styling | Known legacy stylepack symlink is detached, shared target preserved/repaired by recorded hashes, off restores untouched link; `system/files.rs`, `system/gtk.rs` | Installed recovery code runs on the next apply; no forced migration of an already open live application. Edited files are preserved rather than blindly restored. |
| Create widgets and use small connected apps | No-code creator, shared Notes/Timer cards plus resource overview/focus hub, enlarged-text clock/media with unchanged 100% sizes, exact painter sample previews, private notes and conflict-safe saves; `settings/widgetbuilder.rs`, `notes/`, `timer.rs`; native release Notes/Timer log passed | Notes is local plaintext, not encrypted/synchronised. Headless login recovery is implemented and passed11unit/6realCLI checks; install its XDG entry to activate it. The crash-after-claim notification-loss tradeoff remains. Desktop widget host is X11-only. |
| Familiar file management | Follow-theme preference is the default when absent: Windows11/Frutiger Aero → Explorer, macOS/Graphite → Finder, others Native; explicit choices remain fixed. Private native list/grid/navigation/create-folder checks include dark Windows11 with independent Nord; [limits](file-manager-profiles.md) | **Visual Nautilus profiles, not complete Explorer/Finder replicas.** No ribbon, Finder column view, replacement navigation or new file-operation model. Existing GTK windows may require reopening. |
| Move, snap, tile and resize | rdwm implements Shift snapping, freely movable centred fullscreen exit, tiling formation; existing macOS/snap/resize suites and checkpoint evidence | User confirmed smoothness with Picom no-frame-pacing; repeatable NVIDIA first-drag/resize frame timing remains unproven. Do not infer 60fps from geometry or upload counters. |
| Premium window effects | Unified SSD X11 shadow carrier with correct group stacking/input transparency and bounded peripheral uploads; native debug/release 45 checks | Native shadow controls use cached live-parser capability and reject stale unsupported events. New WM session and theme reapply required; current live WM is older. CSD/undecorated windows and Wayland shadows absent. Mixed-DPI/resource/performance checks on real hardware remain. |
| Notifications and history | Source/new binaries provide bounded private atomic history, symlink/permission/lock safety, surfaced error/Retry, failed-clear durability, merge/rekey and typed CLI status; 31 focused notification tests plus exact-release native theme changes, EACCES/corrupt/symlink/restart checks passed; popup/accent/text follow cached live theme without resetting history/DND; [guide](notification-history.md) | **Old live daemon still runs.** Its persisted 38 records were backed up; no active banner at observation. RAM-only state is unknown and no forced handoff occurred. New permissions/error handling takes effect only after a separately reviewed restart/session. Notification sounds remain unimplemented; fixed row metrics do not fully reflow at 200%, and cached app icons do not refresh on icon-pack changes. |
| Tray/background application controls | Bounded SNI/DBusMenu across bar/rail/taskbar, custom paths/overlays, dynamic submenus and typed input; actual OBS and private pixels/lifecycle tests pass. Worker refreshes icon/theme generations from cached sources without paint I/O or new GetAll/app NewIcon; A→B/tint/stable-key regression passed; [guide](app-indicators.md) | No XEmbed, attention movies, hover tooltips or menu-item icons. Wayland activation/global coordinates and native app matrix remain. Extremely short/custom rails can exhaust space. |
| Network, audio and Bluetooth | Native settings backed by nmcli, pactl/wpctl and bluetoothctl; `settings/system/{network,sound,bluetooth}.rs`, fake-command fixtures | Depends on installed services/tools. Parser/fake-runner coverage does not establish real Wi-Fi auth, VPN credentials, device hotplug, headset profile switching or service restart recovery. |
| Arrange displays and scale text | X11 xrandr resolution/rate/rotation/placement/mirroring plus timed revert; Xft/deco scale; `settings/system/display.rs` | No per-output fractional-DPI user workflow demonstrated. Wayland `load_env` explicitly returns unavailable; no native output-management adapter here. Hotplug/mixed-DPI hardware matrix missing. |
| Accessibility and readable settings | Runtime scale, high contrast, reduced dock motion; reset-to-theme text size; wrapped rows and 200% Settings tests; `accessibility.rs`, `settings/system/access.rs` | No AccessKit/AT-SPI integration found. Complex editors/widgets and keyboard-only end-to-end operation need verification. Next-login exports on this machine still need the updated system wrapper installed. |
| Learn the desktop from inside the session | Spotlight Getting Started and Settings Welcome aliases/cards, same-instance deep links, draft preservation; `spotlight.rs`, `settings/panes.rs`, `bin/decsettings.rs` | Guide is reachable; no mandatory automatic first-login tour. Static shortcut labels can differ from custom bindings. Windows/macOS migration tasks are not all covered by geometry profiles. |
| Install, upgrade and recover session | Explicit config validation, supervised components, default PAM lock, atomic user-binary rollout/backup; rdwm session sources bounded known exports | `/usr/bin/rdwm-decebal-session` remains older until administrator installation of prepared `session-preferences-5c58c1c` bundle. Source tests passing do not repair that installed wrapper. Bare system CLI may differ from user session binaries. |
| Use Wayland as a complete daily desktop | rdwm Wayland backend and shell Wayland paths exist; live wallpaper uses mpvpaper | Native widgets unavailable; unified shadows X11-only; display controls unavailable; focus tokens, screen capture/portals and complete app/toolkit workflow need private-compositor and hardware checks. X11 results do not establish Wayland readiness. |

## Original findings reconciled

| Original finding | Current state |
|---|---|
| Session exports die in child | Fixed in rdwm source (`be5f6e0` / `5c58c1c`), private session checks recorded. Privileged installed wrapper still pending. |
| Accessibility controls have no consumers | Runtime consumers and real geometry checks now exist. Reset-to-theme added. Screen-reader support remains absent. |
| Screenshot sample chords bypass decutil | Source/default chords now use decutil; private chord and launcher checks recorded. Installed system sample follows wrapper installation boundary. |
| btop not running creates warning | Source checks running state; no-op absence and genuine failure separated. |
| Legacy GTK link mutates shared pack | Recovery implementation and real private symlink/on/off checks passed. |
| Guide exists only in repository | Native Welcome and Spotlight entry implemented, with real navigation/draft checks. |
| Dock reconcile lost lock | Bounded reconcile retry/lifecycle remains. NextKDE now explicitly restores magnification 1.4 after flat themes; source and installed override repaired, runtime field hot-reloaded without restart. Reduced motion remains respected. |
| Preview persistence untested | Release restart log confirms stored PNG/reference reuse without rewriting. |

## Next implementation batch

[Concrete next work and acceptance gates](desktop-readiness-next.md) prioritises
remaining indicator compatibility, legacy-daemon handoff and Wayland display management.
These do not erase the larger missing file-manager fidelity, accessibility,
portal, toolkit and hardware requirements in the matrix.

Before any broader “desktop ready” claim, maintain a reproducible release matrix:
X11 and Wayland; a clean new-user session and an upgraded session; 100/125/150/200%
scale; single/mixed-DPI outputs; an app with custom/CSD decorations; missing
optional backend tools; failed persistence; and real keyboard-only newcomer tasks.
Every claimed supported row needs a matching test or recorded manual observation,
not merely a manifest, control label or large aggregate test count.

## Current live choices and pending work

The user explicitly enabled automatic curated-photo following. The earlier
Caelestia/dark rollout selected Wallhaven p82qrj; subsequent **user selection** is
NextKDE/dark with runtime `wallhaven-57o619.png`, following on and live-video mode
off. Video selection/provider/power preferences were retained. Final pair install
preserved the user's then-current shell/dock bytes and open Settings rather than
restoring the older Caelestia snapshot. Independent palette/widget choices remain
user-owned. The colour-hook follow-up was tested and installed without forcing a
new live photo, theme or mode.

A generated/upscaled Tidal Loom image is an explicitly permitted, isolated
experiment, not an applied wallpaper or catalogue/default asset. Its README and
provenance remain in the user trial directory; no theme artwork was replaced.

Dux Settings hierarchy, native resource/focus cards and the Theme Specialist's
Dux Lavender package are integrated and installed. The package is available in
the gallery, **not applied**: NextKDE/dark, radius24, photo/follow settings and
custom widget layout remained unchanged. The new widget host understands the new
DSL/builtins. Open Settings retains its old in-memory image/drafts; reopen later
to see the new styling. Notification colour coherence is installed on disk but
legacy daemon670409 remains pending a separately safe handoff. User-reported dock overlap
with floating apps has accepted rdwm source `9788af9`: ordinary free floats fit a
new reserved workarea only; explicit resize/drag clears that limit and fullscreen
is excluded. Core 504 passed/two ignored and exact release panel7/fullscreen11/
snap10 passed. Both user WM binaries are installed for **next login** (SHA256
`bbff6f1077f2faee443f89457cc30d2195f47a0fd8aafe951b955f07d5ac629b`);
journal: `~/.local/state/rdwm/deployments/workarea-20261030-153244/journal.json`.
Running WM1714917 is unchanged: this general fix is **not active yet**. The
separately approved T3-only correction to client553,27/2147×1328 is not evidence
that the entire live WM was upgraded. The broader
readiness goal remains ACTIVE.
