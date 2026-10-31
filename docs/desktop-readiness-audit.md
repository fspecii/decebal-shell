# Desktop readiness audit — current checkpoint

Current checkpoint: 2026-11-01. This report separates installed software,
qualified source and pending release acceptance. It does not establish KDE/GNOME
parity or physical hardware readiness. Earlier sections are historical and may
mention limitations since repaired; use the current implementation documents
linked below for feature coverage.

## Current installed and pending release

The installed shell is the qualified standard 23-binary `704c657` release in both
managed user binary directories. The running WM has been preserved; updating its
source or installed executable does not prove a live backend restart. Existing
Settings windows and drafts are preserved during shell handoffs.

The next shell candidate is `9d663b4`, **not installed** at this checkpoint:

- Full integrated library gate: **2,402 passed, 0 failed, 23 ignored**.
- All 24 optimized executables (23 runtime binaries plus `examples/launchapp`)
  built and were sealed after a successful cached retry. The original build's
  I/O-guard failure is retained, not counted as a success.
- Matching data bundle: **44 themes, 215 files**, with package-origin, integrity,
  user-override precedence and standard-XDG discovery checks. These counts do not
  imply full upstream visual fidelity.
- Seven optimized native phases passed: empty/reloaded Control Center, icon
  integration, one-click gallery opening, preview stress, XSETTINGS ownership,
  and GTK integration. GTK recorded **228/228 checks** and four live recolouring
  outcomes in the same GTK 3/GTK 4 processes.
- Launcher, rapid-toggle, dock lifecycle, UKUI matrix, Wayland popup and widget
  recovery optimized checks remain pending. Interrupted runs are not passes.
  The launcher fixture's missing private work directory was corrected before
  retry; subsequent testing was held after the I/O guard interrupted it.
- Widget-creator native coverage is also pending. A reviewed fixture has crop
  and cleanup repairs plus 16 passing helper tests; a resource-interrupted run
  did not reach creation and is not acceptance.

Local evidence is recorded in
`/var/tmp/decebal-9d-native-qualification-checkpoint.json`, the individual
`/var/tmp/decebal-9d-optimized-acceptance*-control/` directories, and the sealed
`9d663b4…-retry2` artifact with its matching `9d663b4….assets` bundle. These paths
are machine-local evidence, not repository-distributed test results.

## Current product gaps

Native Wayland widgets and compact HUD/dashboard surfaces are implemented; they
are not X11-only. Development-binary private compositor evidence exists, while
optimized and physical mixed-DPI/hotplug acceptance remains incomplete. See
[widget readiness](widgets-host-session-readiness.md) and
[compact popups](wayland-compact-popups.md). Native notification frontend work is
separate from capture/portal integration; neither should be inferred from the
widget implementation.

AT-SPI2 is implemented and Settings has a semantic adapter. The shell as a whole
is not yet accessible: launcher/bar/dock/notification adapters and real Orca
workflow acceptance remain work. See [accessibility](accessibility.md). Any
older “no AT-SPI” statement below is historical.

Explorer/Finder file-manager profiles provide appearance and integration around
existing file operations. They are not replacement interaction models, Finder
column view or an Explorer ribbon. See [file-manager profiles](file-manager-profiles.md).

The next product work therefore includes complete Wayland session integration,
launcher and shell accessibility, familiar file workflows, authentic theme
fidelity, and desktop-wide responsiveness qualification. Passing one workflow or
headless compositor test never substitutes for these broader requirements.

## Earlier accepted iteration — 0.3

The [sprint handoff](desktop-sprint-2026-10-31.md) records the exact artifacts,
original failures, native qualification and remaining requirements. The standard
23-bin release is `7837e1b`; the installed Settings-only follow-up is `0b8da82`
(SHA-256 `5bf4a43914b2cd717a82982f5ee8787a0b51bc77d74c0decd851d89d88567133`).
The other 22 binaries and example remain the standard release. Both managed user
locations have been updated, with matching 44-theme assets. Bar, HUD and dock
were refreshed without changing configuration, applications, workspace or focus.

The existing Settings process was preserved for its drafts; close and reopen it
to use the installed binary. The old live WM remains in place because it cannot
export a complete compatible layout. The updated WM and capability-gated
Super+arrow migration require a safe fresh session. The privileged system session
wrapper was not replaced.

The high-scale Settings crash and slider pointer-target overflow have optimized
private-X11 proofs. Extreme long-word wrapping, physical and screen-reader
acceptance, Wayland hardware paths, complete theme fidelity and desktop-wide
performance qualification remain open. KDE Breeze/Layan packages are still held;
their native pager fixture is not a shipped theme.

The full final library run recorded 2,173 passes, one schema cross-check failure
against a legacy system CLI, and 19 ignored tests. The same cross-check passed
separately against the pinned current CLI, without source changes. It is not
reported as a single clean full-suite run.

## Earlier accepted iteration — 0.2.1

- All 23 shell binaries were built from `bbe508f` with source/package fingerprint
  `995efd025ff4d9d06c3368967203a067cb98ec153084903d15eeccefd329f710`.
  A targeted `dechud` build from `4bdb5b1` adds its independent watcher lock.
- Source-backed Dux clock and Dux Lavender 1.3 passed the library gate
  (1,705 passed, 17 ignored), strict affected-target Clippy, native optimized
  Dashboard hosts and four-font compatibility. Twenty original-source component
  comparisons pass; these use the separate painter test binary, not a claim of
  pixel-identical optimized whole-Dashboard captures. Hot text-size regression
  compares changed scales and minute transitions against fresh preparation.
- Optimized launcher tests prove successful intentional toggles while genuine
  SIGTERM failures remain reported. Follow-up `58a21c6` serializes overlapping
  requests: a privately stopped holder reproduces coalescing before the fix;
  odd/even concurrent bursts preserve final open/closed state afterward. Both
  launchers passed the optimized lifecycle and concurrency fixtures and were
  installed in both user binary folders. Evidence:
  `/var/tmp/launcher-concurrent-optimized/result.json` and
  `/var/tmp/launcher-toggle-serial-optimized/result.json`; deployment backup:
  `~/.local/state/decebal/deployments/launcher-serial-20261031-030027/`.
  Notification icon-pack changes refresh
  banners/history while preserving image hints, deadlines and stored history.
- Workspace HUD watcher is single across concurrent starts and WM reconnects.
  Native regression passes; four old live watchers were replaced by one.
- Both user binary folders were updated with per-file atomic replacements and
  backups. Dux 1.3 was installed without applying it. Live bar, dock, widgets,
  notifier and HUD use the new code; config, notification history, managed app
  IDs, workspace and focus were preserved. Clipboard ownership was untouched.
  Deployment: `~/.local/state/decebal/deployments/coordinated-021-20261031-021902/`.
- rdwm `d3ed26c` frame-surface reuse passed optimized fallback/GLX and real
  Settings/widget/video gates. Installed, but the old `22eec653` WM remains live:
  its hidden tile ratio cannot be safely exported by the normal state API.
  Plugin-probe research was rejected after proving ownership stealing and timeout
  geometry changes. No forced live handoff was attempted.
- Wayland SSD shadows and capability-aware controls have nested software evidence;
  physical DRM/GLES, client decorations and hardware frame times remain unproved.
- Dedicated KDE/GNOME, universal-panel and launcher-style work remains in progress.
  Their new packages are not accepted as complete ports until authentic rendering,
  four-edge placement and every selectable applet work together.

## Earlier installed Settings — bounded disappearance check

The installed optimized Settings SHA
`8cac883d022a6de398d410717db470ecd42b0cdf31ec75b4eae6570a9e9cc02a`
passed private-X11 rapid browsing: 122 theme-gallery iterations, 135 wallpaper
iterations and 86 internal Appearance-tab switches (30 seconds each). One
Appearance click began preview painting; a later settled capture showed all
visible cards populated. A fresh process reused 24 visited PNG/reference files
with identical hashes, mtimes and sizes. Eight final visible fixture wallpaper
centres remained correct and idle redecodes were zero.

Peak Settings RSS was 54,540 KiB, with ten threads and 23 FDs; these are bounded
observations, not a general leak or crash absence guarantee. Parent verified
three pinned executable hashes against the installed files, two harness hashes,
and reviewed warm-theme and wallpaper captures. Evidence and reproducible
harness instructions: `/var/tmp/settings-current-stress/summary.json` and
`README.md`. Controlled fixture termination used SIGTERM; no unexpected exit
was observed. The user's historical disappearance remains **unconfirmed**.
Physical input/GPU, Wayland and wallpaper/theme replica fidelity are outside
this private test.

The repository's `tests/e2e/settings-preview-stress.py` now accepts
`SETTINGS_TAB_SECONDS` to exercise all four internal tabs with fresh visible hit
targets after scrolling back to the top. The extended fixture passed 40 switches
in ten seconds plus warm-cache and same-window/config-rejection reopen checks
against the same optimized Settings binary. Evidence:
`/var/tmp/settings-repo-tab-gate-final/result.json`. Its original three-click
preview diagnostic remains intentional; it does not replace the independent
one-click first-open proof above.

## Earlier accepted iteration

- Combined follow-up: all-bin optimized build passed (9m 30s),1,643 library tests
  passed/0 failed/11 ignored, strict all-target Clippy and formatting passed.
  Production inputs were frozen and 506 file hashes verified unchanged after build.
- Window Tracker ownership `446be5d` closes the remaining window-group worker
  leak. Eight native X11 cycles preserve surviving-clone UTF-8/legacy title
  updates and real activation; four private headless Wayland cycles release
  collectors. Optimized media-free widget integration returns to 2 threads/8 FDs
  after four enable/disable cycles, rather than leaking one collector per cycle.
  Initial protocol setup/in-flight replies keep their synchronous boundary; this
  does not add display-server reconnect or hardware Wayland control acceptance.
- Optional notification sounds `f9a03b0` are default-off. Named/local-file hints,
  suppress-sound, DND, muted apps and silent replacement updates are supported.
  One lazy worker, one pending event, short expiry/rate limiting, sealed bounded
  audio snapshots and five-second player cleanup prevent unbounded audible backlogs.
  Exact optimized private fake-player/bus regression 14/14 passed. Requires
  canberra-gtk-play plus a sound theme; no audible hardware test was performed.
  See [sound policy and limits](notification-sounds.md).
- Native popup startup focus repair `0773371` passes exact optimized startup,
  genuine foreign-focus close and Escape checks. Old optimized dashboard exited
  normally with status 0 before window discovery. This explains that reproduced
  apparent-crash case, not the user's still-unspecified flicker/crash report.
- Guarded explicit same-build WM reload now supports `--force`; without it an
  already-current binary is a no-op. Private tests cover both paths. Live reload
  completed with the same three app XIDs/geometries/view/focus restored, followed
  by new Settings/bar/widget/notifier processes. Current Glyph Rice and wallpaper
  stayed unchanged; dock remains disabled. Persistent notification history was
  byte-identical before/after and typed persistence status reports no errors.
- Latest durable release/reload evidence and backups:
  `~/.local/state/decebal/deployments/latest-20261030-202210/` and
  `~/.local/state/rdwm/restarts/20261030-202306-mn94ulb4/`.

## Previous accepted gallery/palette iteration

- Settings first-open repair: gallery discovery and generated-image completion
  now wake the native event loop. The old optimized binary failed after exactly
  one Appearance click; optimized Settings `fb48aacd…6333` passed with the final
  optimized renderer, no additional input, completed PNGs and later frames that
  painted them. Fresh live Settings on workspace 2 painted six previews after
  one click. Preview generation remains asynchronous and its PNGs persist.
- Twenty researched rice palettes bring the catalogue to 33. Fifteen source
  dark/light pairs and five explicitly adapted light variants have pinned
  primary-source data, role mappings, hashes and licence notices. See
  [palette research](rice-palettes.md); source palettes are not complete desktop
  theme replicas, and role readability adaptations are labelled.
- The widget CPU regression was reproduced: dropping an MPRIS owner left a
  permanently readable EOF wake socket, one 100% worker per rebuild. Commit
  `8074170` stops that worker and preserves compatible Live collectors across
  rebuilds. Exact optimized NextKDE/glass private test 18/18 passed, including
  repeated group changes, media interaction, changing title and backdrop pixels.
  The old live daemon was replaced; a one-second live sample dropped from
  approximately 400% to 2%CPU. Window-group Tracker worker shutdown is repaired in the follow-up above;
  there is no universal idle-power or performance claim.
- Notification font/viewport reflow, queue promotion/deadlines and keyboard
  actions passed the optimized 12-case native matrix: dark/light × 100/200% text
  ×normal/narrow/tiny screens. Existing IDs/history/focus survive ordinary
  font-only reflow. Extreme short layouts scroll/ellipsise rather than promising
  all content is visible. No screen-reader or native Wayland-notifier claim.
- Full optimized all-bin build passed. Strict affected-library/bin Clippy,
  formatting and focused integration tests passed. The combined pre-CPU library
  run was 1635 passed/0 failed/11 ignored; later CPU changes had separate focused
  integration and optimized native gates. These are distinct source checkpoints.
- Guarded X11 WM restart captured/restored the actual three floating clients'
  flags, geometry, workspace view and focus. Wrapper and client identities
  survived. Latest focus, XFCE decorations, workarea and gesture code is now
  running, not merely installed for next login. Hardware swipes remain untested.
  Restart helper `contrib/rdwm-restart-x11.py` in the sibling rdwm repository
  currently refuses tiled/fullscreen/snapped/scratch and multi-monitor state.
- Durable source/binary/backups and validation journals on this machine:
  `~/.local/state/decebal/deployments/latest-20261030-195735/` and
  `~/.local/state/rdwm/restarts/20261030-195402-qwxnjnxk/`.

## Earlier acceptance evidence (historical deployments)


- Latest Wayland Displays gate: **1,617 default-feature library tests passed,
  zero failed, ten ignored**, plus 25 focused Displays tests, the raw-editor guard,
  formatting and strict all-target Clippy. Typed worker IPC supports staged
  connector mode/rate, position, scale, transform and enabled drafts according
  to actual capabilities. Compositor-owned provisional Keep/Revert survives
  Settings close/crash; confirmation persists via the compositor. Instance and
  generation guards reject stale/reordered replies and drafts across restart;
  delayed errors remain visible without erasing newer state or extending timers.
- Exact release Settings `cef4fb9fbfbe71fb3835517d128db0e0562747baef77bab6e20a66996e83cbd5`
  passed real private nested Wayland input/pixels, Apply/Keep/Revert, crash timeout,
  stale reset and X11 legacy regression. Forced X11 rendering inside an actual
  Wayland session still exposes typed capabilities, with no xrandr/raw-output
  bypass. Independent QA also validated the versioned compositor release and
  restart persistence; physical DRM mode/power/hotplug remains untested hardware.
- Settings-only deployment journal:
  `~/.local/state/decebal/deployments/wayland-settings-20261030-165644/journal.json`.
  Two installed Settings paths match the reviewed hash, with atomic per-file
  replacements/backups and a pinned read-only inventory controller. Protected
  configuration hashes, processes and WM state were identical before/after.
  No Settings process was open or launched, and no compositor/output/app changes
  were made. Current session stays X11. New controls require matching rdwm output
  protocol v1; this Settings rollout did not install or restart a compositor.

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
| Choose panel edges, applets and launcher layouts across themes | Shared registry and distinct KDE/GNOME/launcher ports are being implemented in dedicated worktrees | Not yet deployed or complete: every renderer must support all four edges and each applet; renderer-based exclusions and hardcoded foreign desktop branding were rejected in contract review. |
| Change a coherent desktop design | Transactional themes, independent palettes, reviewed widget replacement and visible validation errors; native Terminal Minimal preserves radius 24 while composing legal SSD 20/20; packaged and compiled clock Keep/Replace pass | Full desktop parity unproven. Wallpaper stays independent unless the user enables the separate curated-photo follow switch. |
| Browse attractive wallpapers | Wallhaven browse/favourites and curated per-theme/mode photos; generation-guarded async follow worker; reviewed SHA+dimension cache import; 42 originals imported, three unknown entries skipped; post-photo Dynamic refresh preserves independent/fixed/disabled choices; [guide](wallpaper-follow.md) | Live API can return 403; failures and partial colour-refresh success remain visible. Reviewed hashes prove byte identity, not photographer identity, licence or absence of AI. Physical crop/readability checks remain. |
| Live wallpaper with real preview and fullscreen savings | DesktopHut/Commons search, muted video preview; per-output player and pause on visible fullscreen, retained frame; `src/livewall/{pane,preview,host,mod}.rs`, `tests/e2e/live-wallpaper.py` | Runtime needs mpv/ffmpeg, Wayland mpvpaper; provider availability can change. Current full provider/hardware run not established by this audit; no zero-GPU-cost promise for a paused process. |
| Apply themes to applications | Managed GTK blocks/settings, Qt5ct/Qt6ct colour schemes, terminal/Omarchy integration and rollback; `src/theme/system/`; native GTK/app suite 89 passed | GTK applications may require reopening. Qt needs correct platform-theme environment; Qt pixel/real-app restart verification remains missing. Third-party application custom UIs are not universally themed. |
| Recover application styling | Known legacy stylepack symlink is detached, shared target preserved/repaired by recorded hashes, off restores untouched link; `system/files.rs`, `system/gtk.rs` | Installed recovery code runs on the next apply; no forced migration of an already open live application. Edited files are preserved rather than blindly restored. |
| Create widgets and use small connected apps | No-code creator, shared Notes/Timer cards plus resource overview/focus hub, enlarged-text clock/media with unchanged 100% sizes, exact painter sample previews, private notes and conflict-safe saves; `settings/widgetbuilder.rs`, `notes/`, `timer.rs`; native release Notes/Timer log passed | Notes is local plaintext, not encrypted/synchronised. Headless login recovery is implemented and passed11unit/6realCLI checks; install its XDG entry to activate it. The crash-after-claim notification-loss tradeoff remains. Desktop widget host is X11-only. |
| Familiar file management | Follow-theme preference is the default when absent: Windows11/Frutiger Aero → Explorer, macOS/Graphite → Finder, others Native; explicit choices remain fixed. Private native list/grid/navigation/create-folder checks include dark Windows11 with independent Nord; [limits](file-manager-profiles.md) | **Visual Nautilus profiles, not complete Explorer/Finder replicas.** No ribbon, Finder column view, replacement navigation or new file-operation model. Existing GTK windows may require reopening. |
| Move, snap, tile and resize | rdwm implements Shift snapping, freely movable centred fullscreen exit, tiling formation; existing macOS/snap/resize suites and checkpoint evidence | User confirmed smoothness with Picom no-frame-pacing; repeatable NVIDIA first-drag/resize frame timing remains unproven. Do not infer 60fps from geometry or upload counters. |
| Premium window effects | Unified SSD X11 shadow carrier with correct group stacking/input transparency and bounded peripheral uploads; native debug/release 45 checks | Native shadow controls use cached live-parser capability and reject stale unsupported events. The current X11 WM retains accepted SSD shadows. New Wayland SSD shadows have nested software evidence; physical DRM/GLES and CSD/undecorated windows remain unproved. Mixed-DPI/resource/performance checks on real hardware remain. |
| Notifications and history | Bounded private atomic persistence, recovery/error/Retry, failed-clear durability and typed status; optimized font/viewport/keyboard matrix above; live new daemon activated | Legacy in-memory-only history reset during the explicitly announced reload. Default-off optional sounds passed the private optimized policy/lifecycle gate above; real audible hardware acceptance remains. Icon-generation refresh is now implemented and passed optimized native banner/history checks. No AT-SPI/native Wayland frontend. |
| Tray/background application controls | Bounded SNI/DBusMenu across bar/rail/taskbar, custom paths/overlays, dynamic submenus and typed input; actual OBS and private pixels/lifecycle tests pass. Worker refreshes icon/theme generations from cached sources without paint I/O or new GetAll/app NewIcon; A→B/tint/stable-key regression passed; [guide](app-indicators.md) | No XEmbed, attention movies, hover tooltips or menu-item icons. Wayland activation/global coordinates and native app matrix remain. Extremely short/custom rails can exhaust space. |
| Network, audio and Bluetooth | Native settings backed by nmcli, pactl/wpctl and bluetoothctl; `settings/system/{network,sound,bluetooth}.rs`, fake-command fixtures | Depends on installed services/tools. Parser/fake-runner coverage does not establish real Wi-Fi auth, VPN credentials, device hotplug, headset profile switching or service restart recovery. |
| Arrange displays and scale text | X11 xrandr workflow retained; native rdwm Wayland typed capability/query/apply/Keep/Revert, per-output scale/transform/position/modes/power where supported, stale epoch protection and persisted confirmation. Actual nested Wayland Settings and X11/forced-X11 release checks passed; [contract](wayland-displays.md) | Requires matching rdwm protocol v1. Nested host owns physical modes/power, so those controls are honestly disabled. Physical DRM enable/disable/mode, hotplug and mixed-DPI hardware matrix remain. |
| Accessibility and readable settings | Runtime scale, high contrast, reduced dock motion; reset-to-theme text size; wrapped rows and 200% Settings tests; `accessibility.rs`, `settings/system/access.rs` | No AccessKit/AT-SPI integration found. Complex editors/widgets and keyboard-only end-to-end operation need verification. Next-login exports on this machine still need the updated system wrapper installed. |
| Learn the desktop from inside the session | Spotlight Getting Started and Settings Welcome aliases/cards, same-instance deep links, draft preservation; `spotlight.rs`, `settings/panes.rs`, `bin/decsettings.rs` | Guide is reachable; no mandatory automatic first-login tour. Static shortcut labels can differ from custom bindings. Windows/macOS migration tasks are not all covered by geometry profiles. |
| Install, upgrade and recover session | Explicit config validation, supervised components, default PAM lock, atomic user-binary rollout/backup; rdwm session sources bounded known exports | `/usr/bin/rdwm-decebal-session` remains older until administrator installation of prepared `session-preferences-5c58c1c` bundle. Source tests passing do not repair that installed wrapper. Bare system CLI may differ from user session binaries. |
| Use Wayland as a complete daily desktop | rdwm Wayland backend and shell Wayland paths exist; live wallpaper uses mpvpaper | Native widgets unavailable; Wayland SSD shadows are implemented with nested software evidence; display management requires the tested rdwm protocol rather than arbitrary compositor support; focus tokens, screen capture/portals and complete app/toolkit workflow need private-compositor and hardware checks. X11 results do not establish Wayland readiness. |

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

[Concrete next work and acceptance gates](desktop-readiness-next.md) records earlier priorities. Window-tracker lifetimes and optional
notification sounds are accepted above; current work continues the section-by-section Dux replica; the latest notifier
and WM handoffs have completed, while hardware acceptance remains open.
These do not erase the larger missing file-manager fidelity, accessibility,
portal, toolkit and hardware requirements in the matrix.

Before any broader “desktop ready” claim, maintain a reproducible release matrix:
X11 and Wayland; a clean new-user session and an upgraded session; 100/125/150/200%
scale; single/mixed-DPI outputs; an app with custom/CSD decorations; missing
optional backend tools; failed persistence; and real keyboard-only newcomer tasks.
Every claimed supported row needs a matching test or recorded manual observation,
not merely a manifest, control label or large aggregate test count.

## Current live choices and pending work

At the accepted rollout, the user's current choice was Glyph Rice/dark with
Wallhaven `zpzqeg.jpg`; workspace 2 stayed visible. Settings, bar, notifier and
widget host run optimized source `f9a03b0`, and rdwm runs SHA`4b8091fa…d028`.
The dock is disabled for this rail theme: no hidden dock process was retained.
Picom and application processes were preserved. Later user selections must be
read from the running session, never restored from this audit snapshot.

The user now requests building and reloading finished iterations so they can test
while development proceeds. The guarded helper preserves supported X11 state and
refuses unsupported layouts; it must not be replaced by `action quit`, which ends
the session. The privileged system preference-export wrapper still needs its
separate installation. Existing active session binaries do not repair that file.

The structural Settings refinement is integrated. The rejected early Dux package
is not evidence of replica parity. Source-pinned typography and section-specific
bar/dashboard/control-center work continue in the assigned Dux thread. Candidate
selectors stay inactive until their actual native host gates pass. Full Files
Explorer/Finder layouts, AT-SPI, portals, Wayland widgets/shadows and the physical
hardware matrix remain substantial work.

The user reported intermittent wallpaper flicker and a crash while testing themes.
No corresponding live WM/Settings/Picom/Xorg core or OOM was found. The root pixmap
stayed stable over ten samples; one 200×200 wallpaper region was pixel-identical
for 60 frames. Those narrow observations do not prove an intermittent-flicker fix
or identify what disappeared. The report remains open pending reproduction or a
more specific failure observation. Do not attribute unrelated private test/core
events to the live desktop.

The overall desktop-readiness goal remains ACTIVE.


Small integration follow-ups for the next source batch: add Notification sounds
to the static Settings search index and CLI settings help, and refine the DND row
wording to acknowledge critical visual alerts. These do not invalidate the sound
policy/playback tests; they remain usability work, not claimed complete.
