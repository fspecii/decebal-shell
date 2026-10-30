# Desktop readiness audit — current checkpoint

Refreshed 2026-10-30 from Decebal runtime source `eabb41b` and rdwm `ef9d660`,
current source, existing test logs, and the Obsidian project checkpoint. Parent
source now includes visible/hidden generated-art cleanup (`668c481`, `984ba09`), native shadow controls and Timer recovery. Parent integration checks are recorded below; this report is not a hardware validation claim.

This is a workflow/readiness assessment, not proof of KDE/GNOME parity. The
original audit is preserved in git at `a895427:docs/desktop-readiness-audit.md`
(inspected Decebal `d45f075`, rdwm `64a31e2`). Its original claims must not be read
as the current state. No tests, GUI, live configuration changes or installations
were performed during this refresh.

## Evidence boundaries

- Current source confirms the implementations listed below.
- Existing logs inspected: `/tmp/decebal-shadow-full-lib.log` (1,533 passed,
  6 ignored), `/tmp/rdwm-shadow-workspace-final.log` (workspace results),
  `/tmp/rdwm-shadow-release-native.log` (45 native assertions),
  `/tmp/decebal-native-recovery-e2e.log` (89 passed), and release picker,
  Notes/Timer and preview-cache logs. These are previous runs, not new results.
- The project checkpoint records source/deployment hashes and preserved live
  configuration/workspaces. It explicitly distinguishes updated user binaries
  from the older WM still running and the privileged session wrapper not installed.
- A green unit count does not establish hardware performance, mixed-DPI behavior,
  successful Wi-Fi pairing, accessibility interoperability or a Wayland workflow.

## Requirement matrix

| User workflow | Present implementation/evidence | Remaining limit or acceptance evidence |
|---|---|---|
| Change a coherent desktop design | Transactional theme engine, widget replacement review, independent palettes/icons/wallpaper; `src/theme/engine/apply.rs`, `tests/e2e/theme-widget-choice.sh` | Full toolkit/desktop parity unproven; all34 shipped manifests omit wallpaper defaults. Managed local cleanup retired47 exact-known files with backups; the selected image remains protected pending the replacement choice. |
| Browse attractive wallpapers | Separate Wallhaven browse/favourites and per-theme dark/light recommendations; live SFW/AI-tag checks before downloads; `src/settings/wallhaven/{curated,parse}.rs`; native release picker log passed | Attribution/licence remain unknown for catalogue photos. Visual appearance and tags cannot certify non-AI capture. Monitor-specific crop/readability checks remain. |
| Live wallpaper with real preview and fullscreen savings | DesktopHut/Commons search, muted video preview; per-output player and pause on visible fullscreen, retained frame; `src/livewall/{pane,preview,host,mod}.rs`, `tests/e2e/live-wallpaper.py` | Runtime needs mpv/ffmpeg, Wayland mpvpaper; provider availability can change. Current full provider/hardware run not established by this audit; no zero-GPU-cost promise for a paused process. |
| Apply themes to applications | Managed GTK blocks/settings, Qt5ct/Qt6ct colour schemes, terminal/Omarchy integration and rollback; `src/theme/system/`; native GTK/app suite 89 passed | GTK applications may require reopening. Qt needs correct platform-theme environment; Qt pixel/real-app restart verification remains missing. Third-party application custom UIs are not universally themed. |
| Recover application styling | Known legacy stylepack symlink is detached, shared target preserved/repaired by recorded hashes, off restores untouched link; `system/files.rs`, `system/gtk.rs` | Installed recovery code runs on the next apply; no forced migration of an already open live application. Edited files are preserved rather than blindly restored. |
| Create widgets and use small connected apps | No-code creator, shared Notes/Timer cards, exact painter sample previews, private notes and conflict-safe saves; `settings/widgetbuilder.rs`, `notes/`, `timer.rs`; native release Notes/Timer log passed | Notes is local plaintext, not encrypted/synchronised. Headless login recovery is implemented and passed11unit/6realCLI checks; install its XDG entry to activate it. The crash-after-claim notification-loss tradeoff remains. Desktop widget host is X11-only. |
| Familiar file management | Explorer/Finder-inspired Nautilus geometry profiles with independent preference; `theme/system/file_manager.rs`; [limits](file-manager-profiles.md) | **Not the requested complete Explorer/Finder layout experience.** No ribbon, Finder column view, new file-operation model or custom navigation. Native Nautilus still does operations/navigation. |
| Move, snap, tile and resize | rdwm implements Shift snapping, freely movable centred fullscreen exit, tiling formation; existing macOS/snap/resize suites and checkpoint evidence | User confirmed smoothness with Picom no-frame-pacing; repeatable NVIDIA first-drag/resize frame timing remains unproven. Do not infer 60fps from geometry or upload counters. |
| Premium window effects | Unified SSD X11 shadow carrier with correct group stacking/input transparency and bounded peripheral uploads; native debug/release 45 checks | Native shadow controls use cached live-parser capability and reject stale unsupported events. New WM session and theme reapply required; current live WM is older. CSD/undecorated windows and Wayland shadows absent. Mixed-DPI/resource/performance checks on real hardware remain. |
| Notifications and history | Freedesktop notification server, actions, DND, per-app mute, bounded persisted history; `notify/{mod,model,ui}.rs`, `tests/e2e/notify.sh` | This audit did not rerun the native suite. `History::save` ignores persistence errors; durable private-history permissions/error handling need review. No notification-sound consumer found in notify modules. |
| Tray/background application controls | Built-in controls plus a dormant SNI/DBusMenu worker foundation in `src/sni.rs`; real private-bus owned/external watcher checks pass | **Third-party icons are not wired into the bar/taskbar UI yet.** Custom IconThemePath, overlay composition, external host unregistration and initial-property retries remain. No XEmbed manager. Backend checks do not establish visible app-indicator support. |
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
| Dock reconcile lost lock | Bounded reconcile retry/lifecycle implementation present in `src/dock_lifecycle.rs`; previous native 9/9 recorded. |
| Preview persistence untested | Release restart log confirms stored PNG/reference reuse without rewriting. |

## Next implementation batch

[Concrete next work and acceptance gates](desktop-readiness-next.md) prioritises
third-party app indicators, Timer login recovery and Wayland display management.
These do not erase the larger missing file-manager fidelity, accessibility,
portal, toolkit and hardware requirements in the matrix.

Before any broader “desktop ready” claim, maintain a reproducible release matrix:
X11 and Wayland; a clean new-user session and an upgraded session; 100/125/150/200%
scale; single/mixed-DPI outputs; an app with custom/CSD decorations; missing
optional backend tools; failed persistence; and real keyboard-only newcomer tasks.
Every claimed supported row needs a matching test or recorded manual observation,
not merely a manifest, control label or large aggregate test count.
