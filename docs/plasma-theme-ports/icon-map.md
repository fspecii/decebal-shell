# Plasma applet icons: names to match in the icon pack integration

For the assets owner. Which Breeze icon names each Plasma panel applet uses, with the source of the name and whether the installed
`breeze` theme on this machine contains it (`/usr/share/icons/breeze`, checked 2026-10-31). Names marked "source" come from the pinned
Plasma sources (plasma-desktop `c68d33a9`, plasma-workspace `dec158c0`); names marked "not inspected" belong to repositories that were not
pinned (plasma-pa, plasma-nm, bluedevil, powerdevil) and are only checked for presence, **not** confirmed as what those applets use.
The packages bundle no icons: `[icons] theme = "breeze"` (Breeze) or `"Tela"` with `breeze` behind it (Layan) plus
`[branding] logo = "icon:start-here-kde-symbolic"` are the only icon dependencies they declare.

| Registry applet | Icon name(s) | Source | In installed breeze |
| --- | --- | --- | --- |
| `launcher` (Kickoff) | `start-here-kde-symbolic` (compact icon default); picker icon `start-here-kde` | `kickoff/main.xml`, `metadata.json` | yes (16/22 symbolic) |
| `show_desktop` | `user-desktop-symbolic`; peek states `desktop-symbolic` / `window-symbolic`; minimise-all `window-minimize-symbolic`/`window-restore-symbolic` | `showdesktop/main.xml`, `main.qml:184-191` | yes (not verified for the window-* pair) |
| `app_menu` (Plasma appmenu) | `application-menu` (compact/placeholder); picker icon `show-menu` | `appmenu/qml/main.qml:47`, `metadata.json` | yes |
| `notifications` | `notification-active-symbolic`, `notification-inactive-symbolic`, `notifications-disabled-symbolic`, `notification-progress-inactive`; picker `preferences-desktop-notification-bell` | `notifications/CompactRepresentation.qml` | yes (first three) |
| `clipboard` | `klipper-symbolic` (compact), `edit-clear-history`, `configure`; picker `klipper` | `clipboard` qml | yes |
| `media` | `media-playback-start`, `media-playback-pause`, `media-skip-backward`, `media-skip-forward`, `media-playlist-repeat`, `media-playlist-shuffle`, `media-album-cover` | `mediacontroller/*.qml` | yes (first four) |
| `power` (lock/logout) | `system-shutdown` (picker), `system-log-out` per `primaryActions` | `lock_logout/metadata.json`, `LeaveButtons.qml` | yes |
| `tasks` | picker `preferences-system-windows` (task icons are the applications' own) | `taskmanager/metadata.json` | yes |
| `workspaces` (pager) | picker `user-desktop`; cells are drawn from the `widgets/pager` SVG, not icons | `pager/metadata.json` | yes |
| `clock` | picker `preferences-system-time`; text, no icon | `digital-clock/metadata.json` | n/a |
| `search` | milou/KRunner: not inspected; presence only: `search`, `system-search`, `edit-find` | n/a | yes |
| `volume` | not inspected; presence only: `audio-volume-high(-symbolic)`, `audio-volume-muted` | n/a | yes |
| `network` | not inspected; presence only: `network-wireless-symbolic`, `network-wireless-signal-good-symbolic` | n/a | yes |
| `bluetooth` | not inspected; no `bluetooth*-symbolic` file exists in the installed breeze (checked by file name search); `bluetooth-active-symbolic` is absent | n/a | partly |
| `battery` | not inspected; presence only: `battery-full-symbolic` | n/a | yes |
| `brightness` | not inspected; presence only: `brightness-high(-symbolic)` | n/a | yes |
| `lock_keys` | `plugin/` of keyboardindicator was not found to name icons; `caps-on`/`num-on` are absent from installed breeze | n/a | no |

Rules the asset integration should follow for these packages:

1. Resolve by **icon-theme name** through the user's theme (the contract's `icon:<name>`), never by a bundled copy of Breeze (LGPL-2.1+
   artwork; the packages ship colour values only).
2. Symbolic names are `currentColor`/`ColorScheme-Text` SVGs, so they must be tinted with the bar's text colour (`logo_tint`); the
   taskbar's Start currently tints with the accent instead (engine-requests.md R5).
3. A missing name falls back to the neutral `builtin:grid` for the logo, never a Windows or Apple mark (shared commit `b28cca5`).
4. Tela (Layan's recommended icons) is not installed here, so none of its names were checked; Layan falls back to Breeze.
