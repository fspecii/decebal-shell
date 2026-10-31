# Coordinated desktop development window — 2026-10-31

User-authorized window: **01:36:32–11:36:32 UTC**, measured against system time.
This is an implementation and acceptance window, not a claim of KDE/GNOME parity.
The coordinator owns integration, publishing and deployment.

## Execution rules

- Ceilings: 20 Claude, 10 Grok and 4 Codex agents concurrently. Isolated worktrees
  and explicit file ownership prevent workers overwriting one another.
- Top-level worker checkpoints every ten minutes. A systemd user timer records
  clock ticks; a separate deadline timer records expiry. The bound coordinator
  schedule first runs at 01:56:16 UTC. Private control/worker registry lives under
  `~/.local/state/decebal/ten-hour-sprint-20261031/`.
- Two heavy Cargo build slots, private targets, source frozen during builds,
  `-j2` and low scheduling priority. At most two disposable test VMs, each up to
  two vCPUs and 1 GiB RAM; never attach host disks or expose public services.
- Each accepted feature needs concrete source/binary identity, appropriate tests
  and native evidence. Push reviewed/tested increments. Do not add Claude Code
  co-author trailers or rewrite existing published history.
- Worker processes never install into the live session. Deployment preserves
  user preferences and passes the existing guarded WM handoff policy.

## Implementation lanes

| Lane | Provider | Scope |
| --- | --- | --- |
| Super+Arrow snapping | Claude Opus | Focused window to left/right/top/bottom workarea halves; preserve existing action IDs and modified bindings |
| OpenKylin desktop | Claude Sonnet | Source-backed design, theme package, widgets, icon/cursor assets and decoration contract |
| Deepin desktop | Claude Sonnet | Source-backed design, theme package, widgets, icon/cursor assets and decoration contract |
| Lingmo desktop | Claude Sonnet | Source-backed design, theme package, widgets, icon/cursor assets and decoration contract |
| Asset pack integration | Claude Opus | Verified independently selectable icon/cursor packs, staged import, rollback and safe metadata |
| Window decorations | Claude Opus | Native OpenKylin/Deepin/Lingmo style IDs, states, geometry and private backend evidence |
| Native Files | Claude Opus | Copy/move/clipboard and progress after verified create/rename/trash; then tabs/search/theme integration |
| Test/debug tooling | Grok 4.7 | Isolated test jobs, source/binary manifests, resource sampling, reports and publishing checks |
| Disposable VMs | Grok 4.7 | Real boot/lifecycle acceptance automation; explicit dependency blockers where boot cannot be established |

Existing universal-panel, launcher, KDE, GNOME and accessibility owners continue
their implementation. New family owners do not edit shared engine modules or
another family's assets. Icons/cursors remain independently selectable; theme
changes preserve explicit overrides and global tiling behavior.

## Current integration gates

Independent source review of panel candidate `b28cca5` found reset retaining a
legacy edge, a missing taskbar applet preview fingerprint, disabled-provider
probes continuing through collective snapshots, and array-form applet overrides
being lost on one edit. These were sent to the implementation owner. Display
scale, taskbar keyboard mode and summoned side-panel placement remain required.

Independent launcher review of `676a4aa` found Windows search actions outside
short high-scale cards and ambiguous Recent identity for duplicate app names.
The owner is implementing those fixes and distinct second-result action tests.
Root's app-cache writer isolation must survive integration.

The private panel/WM candidate gate passes 25 workarea cases and eight inward
Control Center popup cases, including two RandR monitors and same-edge bar/dock.
Artifacts match the author's smoke hashes but lack an independent frozen build
input fingerprint. This is useful dev-candidate evidence, not optimized release
acceptance or physical hardware/Wayland proof.
