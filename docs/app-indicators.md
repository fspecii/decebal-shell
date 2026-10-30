# Background app indicators

Apps using StatusNotifierItem can show their icons in the horizontal menu bar,
vertical rail, and Windows taskbar. A rail-only theme keeps the dock disabled;
it does not need an extra dock process to host indicators.

- Click an icon to activate its app. Apps marked as menus open their menu instead.
- Middle-click sends the app's secondary action; wheel sends its scroll action.
- Right-click opens the exported menu, with nested, disabled and checked items.
  Dynamic submenus load when opened. Apps without an exported menu receive their
  native context-menu request instead.
- An attention request adds a small accent dot. Passive items leave the strip.
- When space is tight, **…** opens Background apps. More/Previous and the wheel
  page through long lists. App titles and tooltip descriptions appear in menus.

The worker discovers existing items, handles owner changes, and reconnects after
bus loss. It hosts its own watcher only if no other watcher exists; another
watcher is retained. Concurrent bar/taskbar hosts do not steal ownership. No SNI
worker is kept for the unused head of a split bar. On a crowded 600px rail,
optional shortcuts can be folded into corresponding enabled Control Center
controls to leave room for Background apps. Extremely short rails with many
workspaces and disabled/custom controls can still exhaust physical space.

Icons are decoded and cached on the bus worker, not during painting. Supported
sources include named theme icons, bounded custom IconThemePath directories,
ARGB pixmaps, attention icons and overlays. Custom PNG/XPM dimensions are capped
at512px, custom files at2MiB; custom SVGs cannot embed external/raster images.
Menus use typed protocol calls and item IDs, never shell commands. Limits include
32 items,64 queued actions,128 pending calls,256 menu nodes and8 nesting levels;
long menus are paged. Missing bus or invalid data leaves built-in controls usable.

This is SNI/DBusMenu support, not an XEmbed legacy tray manager. Wayland global
coordinates and activation tokens are not yet compositor-integrated; native
Wayland interactive compatibility still needs validation. Hover tooltips and
menu-item icon decorations remain future polish; tooltip text is available in
the context menu. Attention movies are not animated. Individual apps may expose
only part of the protocol.

Validation uses all-private HOME/XDG, Xvfb and D-Bus fixtures, including actual
painted pixels/input, nested/dynamic menu actions, overflow, multiple hosts,
malformed data, missing bus and reconnect. Installed OBS is additionally checked
for a real native icon, exported menu, safe projector submenu and exit lifecycle.
Tests do not invoke streaming or recording actions.

```
bash tests/e2e/sni-fixture.sh
bash tests/e2e/sni-backend.sh
PROFILE=debug bash tests/e2e/sni-ui.sh
PROFILE=release bash tests/e2e/sni-ui.sh
PROFILE=release bash tests/e2e/sni-ui.sh --tiny-rail
```

`DECBAR_DEBUG_INDICATORS=/private/path.json` enables opt-in test geometry/state
output, including when the status engine is embedded in the taskbar. It is off
normally; no application titles are logged unless explicitly enabled.

Protocol reference: [KDE StatusNotifierItem interface](https://raw.githubusercontent.com/KDE/kstatusnotifieritem/master/src/org.kde.StatusNotifierItem.xml).
