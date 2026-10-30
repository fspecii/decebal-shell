# Universal panels and applets — acceptance contract

User requirement, 2026-10-31: all Decebal themes support top, bottom, left and
right placement. Each supported applet can be enabled or disabled independently
through Settings. KDE Plasma and GNOME ports share the same architecture.

## Required behavior

- Apply to every shipped theme and renderer, including classic, macOS, Windows,
  Caelestia, Dux, Glyph and rail layouts. Preserve each theme's visual identity.
- Text and icons stay upright on vertical panels. Placement changes update work
  areas, popup anchoring, hit targets, focus order and autohide consistently.
- Inventory all actual applets and expose each through the common registry and
  Settings: clock/date, volume, search, weather, workspaces, window title/menu,
  media, network, battery, brightness, notifications, tray and power, plus any
  other existing controls. Missing dependencies must be explained; don't replace
  functions with static visual placeholders or silently omit controls.
- Turning off a component stops its unnecessary timers/subscriptions. Shared
  services keep functioning for remaining consumers. Avoid duplicate dock/bar
  processes when the selected theme uses only one surface.
- User placement and enabled-app preferences survive palette/mode changes.
  Theme defaults and explicit reset behavior must be clear and reversible.
- Preserve old configurations and manifests. Package authors use one shared
  declarative contract rather than per-theme special-case settings.
- Native generated previews reflect the effective edge and applets, keep correct
  proportions, and invalidate their stored cache only when relevant inputs change.

## Evidence required

Maintain an explicit matrix of every renderer against all four edges. Exercise
all-on, all-off and each applet independently, pointer and keyboard activation,
real popup placement, multiple scale factors and both colour modes. Check work
areas, overflow at short dimensions, persisted changes, worker lifetime and
native preview geometry. Record the exact binary/source for native captures.
Passing one generic renderer does not establish coverage for other styles.

KDE/GNOME ports require primary-source research, attribution, authentic native
rendering and a clear record of remaining fidelity gaps. New package owners
must coordinate engine needs with the universal-panel owner. Parent reviews,
builds and deploys accepted results; worker threads do not change the live session.

This document records the intended result, not a claim that it is implemented.
