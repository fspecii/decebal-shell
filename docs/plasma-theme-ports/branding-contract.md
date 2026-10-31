# Plasma panel branding contract (for the universal-panel owner, relayed by the parent)

**Status: requirements with upstream evidence, now partly implemented.** The shared contract at commit `71e343413fdafcc6fde638d701b63c183512f1c8` (`[branding]` logo/logo_tint/launcher_label/desktop_label, `[bar] applets`) is declared by both manifests; the items below say what the native previews at that commit show. Remaining genuine gaps are listed at the end. The two packages stay
**NOT READY** until a native four-edge, all-applet, launcher-fidelity acceptance exists. Stock Breeze's Windows logo and Search
box, and Layan's Apple glyph and "Finder", are real fidelity blockers, not cosmetic notes.

Sources are the pinned clones named in `../plasma-theme-ports.md` (plasma-desktop `c68d33a9`, plasma-workspace `dec158c0`,
breeze-icons `e3c7154b`, Layan-kde `a0b6a495`). Field names below are proposals in the same shape as the universal-panel
draft (`[bar] applets`, keys like `launcher`, `search`, `app_menu`); the owner may rename them.

## 1. Launcher button

| Item | Upstream evidence | Current native behaviour | Required |
| --- | --- | --- | --- |
| Glyph | `plasma-desktop/applets/kickoff/main.xml`: `icon` default `start-here-kde-symbolic`; `metadata.json` `Icon` = `start-here-kde`. Breeze icons ship it: `icons/places/{16,22}/start-here-kde-symbolic.svg` (sha256 prefixes `20fa724ca0a6da6a`, `5847e804ed68a770`); the installed `breeze` theme here has the same names. Layan's layout script gives Kickoff no icon override, so its icon comes from the active icon theme (Layan recommends Tela) | Windows taskbar paints a four-square Windows logo; macOS bar paints an Apple glyph | A per-theme launcher glyph chosen by **icon-theme name** (e.g. `launcher.icon = "start-here-kde-symbolic"`), resolved through the existing icon resolver and tinted like symbolic icons; fallback to a neutral generic glyph, never a Windows/Apple mark, when the name is missing |
| Label | `kickoff/main.xml`: `menuLabel` default empty | none | no text label by default (`launcher.label = ""`); icon only |
| Vertical panels | `kickoff/main.qml` lines 204-326: shouldHaveIcon is forced on vertical, width fills, height follows icon aspect, label suppressed | n/a (no vertical panel) | square icon centred in the panel thickness, upright |
| Popup | `kickoff/FullRepresentation.qml`, `Header.qml`, `Footer.qml`, `LeaveButtons.qml`: search field in the header, Favorites/All Applications/Places navigation, leave buttons in the footer (Sleep, Hibernate, Restart, Shut Down in `fullscreenpreview.jpg`) | existing launcher | not a panel concern; record the Kickoff look as a separate launcher-surface gap |

## 2. Search

| Theme | Evidence | Required |
| --- | --- | --- |
| Breeze | The default panel script (`layout-templates/org.kde.plasma.desktop.defaultPanel/contents/layout.js`) adds kickoff, pager, icontasks, marginsseparator, systemtray, digitalclock, showdesktop and, for some locales, kimpanel. There is no search applet or box; `fullscreenpreview.jpg` shows none | `search` applet **off** by default and the Windows-style panel's search box must be hideable per theme (`search` is in the proposed applet registry; the box is today a dock-settings option the manifest cannot set) |
| Layan | `layouts/org.kde.plasma.desktop-layout.js` adds `org.kde.milou` (KRunner search) at the right end; the preview shows a magnifier **icon** in the top right, not an input box | `search` **on**, rendered as an icon button that opens the existing search surface, not a text field |

## 3. Layan application menu, spacer and the Apple/Finder fallback

| Item | Evidence | Current | Required |
| --- | --- | --- | --- |
| App menu | Layout adds `org.kde.plasma.appmenu` after Kickoff. `plasma-workspace/applets/appmenu/main.xml`: `compactView` default false, `allScreens` true; `qml/main.qml` shows the focused window's menu titles (File, Edit, View ... via DBus menu), `application-menu` icon as placeholder/compact. Preview shows `File(F) Edit(E) View(V) Go(G) Tools(T) Settings(S) Help(H)` | the macOS bar paints "Finder", File/Edit/View/Window/Help placeholders | `app_menu` must be a real consumer of the focused app's DBus menu, or be **unavailable with a stated missing dependency**. `src/sni.rs` has a `com.canonical.dbusmenu` client for tray context menus, but no `com.canonical.AppMenu.Registrar` service or focused-window menu consumer was found in `src`, so this is an engine feature, not a theme choice. Never paint another desktop's fixed names |
| Panel spacer | layout adds `org.kde.plasma.panelspacer` between menu and tray | flat bar uses fixed layout | a flexible spacer element, not an applet toggle |
| Width | layout `maximumLength = minimumLength = 75.9` grid units (about 1366 px on the export screen) | full width | full-width top panel; the number is a screen artefact |
| Workspace dots | not in Layan's layout | the macOS bar shows workspace dots | `workspaces` applet **off** by default for Layan |

## 4. Clock

`plasma-workspace/applets/digital-clock/main.xml`: `showDate` default true, `dateFormat` `shortDate`, `use24hFormat` 1 (follow
locale), `showLocalTimezone` false. Horizontal panel (upstream preview): time over short date, right aligned. Vertical panel
(`DigitalClock.qml` state `verticalPanel`): two rows, time above date, text fitted to the panel width, never rotated, no
one-line mode. Layan's layout sets `use24hFormat = 2` (force 24 h) and `displayTimezoneAsCode = false` for its
`splitdigitalclock` (a third-party plasmoid not in the pinned repos; the stock digital clock is the nearest). Required: a clock
applet option set `{time, date: short|off, hour_format}` with upright two-row layout on left/right edges.

## 5. Show desktop, pager, tasks, tray

- Show desktop: `showdesktop/main.xml` icon default `user-desktop-symbolic`, hit target `Kirigami.Units.iconSizes.medium`
  square (`main.qml` lines 30-37); toggles `desktop-symbolic` / peeks. The Windows taskbar paints a thin sliver instead.
- Pager (Breeze only): `pager/qml/main.qml` min/max swap by orientation, fills panel width when vertical.
- Tasks: `taskmanager/main.xml` default launchers `applications:systemsettings.desktop`, `applications:org.kde.discover.desktop`,
  `preferred://filemanager`, `preferred://browser`; `separateLaunchers` true. These are seeds for a user-owned pin list, never
  hard-coded commands in a theme. Indicator frames `normal-/north-/east-/west-*` in libplasma `breeze/widgets/tasks.svg`.
- Tray: `systemtray/main.xml` `showAllItems` false (extra items collapse behind an arrow), `extraItems/hiddenItems/shownItems`
  empty by default; the expander arrow flips on vertical panels (`ExpanderArrow.qml`).

## 5b. Panel geometry facts for the panel owner

Floating gap 8 px when the Plasma theme has no `floating-*` prefix (Breeze has none; `views/Panel.qml`); panel fill alpha 0.85
(Breeze) / 0.6 (Layan); corner radius 5 (Breeze) / about 8 (Layan, from its preview); thickness 44 in the Breeze preview
(script formula `2*ceil(gridUnit*2.5/2)`, 46 at gridUnit 18); Layan 1.78 grid units (about 32 px). Keep the same values on every edge.

## Acceptance for removing NOT READY (all required)

1. Native captures of both packages on top, bottom, left and right, light and dark, with the Kickoff glyph (no Windows logo, no
   Apple glyph, no "Finder"), no search box for Breeze, a search icon for Layan.
2. Each registry applet toggled alone, all on and all off, per theme and edge; vertical clock/tasks/tray upright.
3. Popups open inward from the real panel position; work area, hit targets and keyboard focus checked.
4. `app_menu` either live (DBus menu) or shown as unavailable with its dependency named.
5. The capability-adaptive tests in `tests/plasma_theme_ports.rs` already switch to positive edge/applet/native assertions when the contract exists; the human `NOT READY` gate is removed with this acceptance.

## Status at shared commit 71e3434 (native previews, fixed sample data)

| Item | Result | Evidence |
| --- | --- | --- |
| Kickoff glyph instead of Windows squares / Apple | **Done**: `icon:start-here-kde-symbolic`, tinted (`logo_tint = true`) | `plasma-breeze-bottom-dark.png`, `plasma-layan-top-dark.png` |
| No Search box on Breeze | **Done**: `search` is not in the Breeze applet set | `plasma-breeze-bottom-dark.png` |
| Search as an icon on Layan | **Done** (magnifier at the right end) | `plasma-layan-top-dark.png` |
| "Finder" fallback | **Done**: the bar says a neutral "Desktop" (Plasma's own placeholder, "Global Menu", is not used because it would claim a feature Decebal lacks) | declared in both manifests; only an `app_menu` bar shows it, which Layan no longer enables by default, so no committed preview renders it |
| Layan application menu | **Not shipped**: `app_menu` is omitted from Layan's default set because its titles are Decebal's fixed `File Edit View Window Help`; `window_title` stands in; exporter API requested in `../plasma-theme-ports.md` gap 1 | `plasma-layan-top-dark.png` |
| Single logo on Layan's bar | **Done in the package** (no `app_menu`); renderer should not repeat the logo if both are enabled | same |
| Vertical Breeze taskbar | **Not done**: collapsed cluster at the foot of the strip in the 71e3434 preview | `plasma-breeze-left-dark.png` |
| Clock / show-desktop forms | partly: two-line time/date on horizontal edges, stacked digits on rails; Windows-style sliver, not Plasma's square icon | previews |

## Unusable logo at run time (shared commit b28cca5)

Only an absent `[branding] logo` keeps a style's own glyph. An explicit logo that cannot be had (the icon theme lacks
`start-here-kde-symbolic`, e.g. Breeze not installed) is drawn as `builtin:grid` in every renderer including the taskbar's Start; the
labels stay. Both Plasma manifests set an explicit logo and `desktop_label = "Desktop"` and no `launcher_label`, so the neutral
grid plus the label behaviour above applies without change. Evidence: `plasma-theme-ports.md`, "Logo fallback". Residual nit: the taskbar's
neutral grid still reads as four squares next to the Windows mark.
