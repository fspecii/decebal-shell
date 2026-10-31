# Kickoff: native launcher against the original (KDE Plasma 6)

Status: **the footer/Places and header/sidebar increments have dev-native acceptance for the scope below; the full card replica remains incomplete.** The packages stay
**NOT READY**. This follows `docs/desktop-ports/replica-acceptance.md` (root, `65d55ca`): original references, matching conditions, source measurements and
original pixels kept apart, and no inert controls.

## Identity

| What | Value |
| --- | --- |
| Source: Kickoff | plasma-desktop `c68d33a9c7cd8e3d32c989a9eadf95e25cc6e79c` (2026-10-21), `applets/kickoff/`: `Footer.qml` (blob `593b5ebac4d8`), `LeaveButtons.qml` (`bec3126b650e`), `PlacesPage.qml` (`8730d3bc9c47`), `ApplicationsPage.qml` (`0e0fe9ec6c91`), `main.xml` (`8f6a44e2a497`), `Header.qml`, `FullRepresentation.qml`, `KickoffSingleton.qml` |
| Source: system actions, places | plasma-workspace `dec158c08fbda8f1dbfd3a86449977dde6347146` (2026-09-04), `applets/kicker/systemmodel.cpp` (`7ea35b4bd5b8`), `systementry.cpp` (`892f4fae9351`), `computermodel.cpp` (`b941205e43a1`) |
| Original capture | the official Breeze Dark and Breeze look-and-feel previews in the same plasma-workspace commit: `lookandfeel/org.kde.breezedark/contents/previews/fullscreenpreview.jpg` (sha256 `3b06913d44c165e69a7dabd669a5fc2e4df1e0c76329cdbad58bcb733852aa05`) and `lookandfeel/org.kde.breeze/contents/previews/fullscreenpreview.jpg` (`020281704596ca6c78410fd36b090e8c5baa078795e71a3c5945b42cbe53d3c6`), 1920 x 1080, Kickoff open at the bottom left of a bottom panel, taskbar clock "30 Sep 2025". They are real Plasma renderings (not Decebal output) but still screenshots chosen by KDE for a preview. They are **not copied into this repository** (they carry upstream licences and Konqi artwork); the path, commit and hash are the reference, and `tools/plasma-kickoff-compare.py` re-measures them from the pinned clone |
| Release gap | the capture is dated September 2025 (Plasma 6.4 / 6.5 era), the pinned source October 2026. The pinned clones are depth 1, so what changed in between cannot be listed offline. The footer geometry agreeing with the capture to 1 px is the evidence that nothing visible moved in the part measured |
| Not available | an original capture of the **Places** tab, of the Session menu, of a pending confirmation, of the card in light mode other than Breeze Light. Those parts are source-derived only |
| Design mode | the default (Breeze) look; Kickoff's applet configuration at its defaults: `systemFavorites = suspend,hibernate,reboot,shutdown`, `primaryActions = 0` (Power), captions on, `applicationsDisplay = 1`, `favoritesDisplay = 0` |

## Matching conditions

Logical size 655 x 516 px at 100 % (the card in the original: 8, 504 to 663, 1020 of the 1920 x 1080 capture), Breeze Dark and Breeze (light), Konqi as
the user, three favourites, an icon theme of Breeze (`breeze-dark` for the dark card, as Plasma picks), the colours of `themes/plasma-breeze` resolved
by the same engine that paints the real launcher. Native captures: `native/kickoff/native-{dark,light}-{applications,places}.png`, written by
`cargo test --lib kickoff_capture_dark -- --ignored` (and `_light`). They render the engine's own card at the original size; the real `declaunch`
window opens at the placement size, which is now the same 655 x 516 (before this section it was 620 x 560, narrow enough that Kickoff's own collapse rule folded every power button into the Session menu). The renderer used
its own UI font (the original uses Noto Sans 10 pt); that is not matched and is listed as a difference.

## Section 1: footer and Places (done)

Original composition reproduced: the Applications and Places tabs on the left (tab bar footer, icon + caption, `applications-all-symbolic`, `compass`,
the active tab marked by Breeze's accent line along the top of the footer), Sleep / Hibernate / Restart / Shut Down as captioned flat buttons
(`systemFavorites`), the Session overflow button (icon `system-log-out`, icon-only when its caption does not fit, `LeaveButtons.qml`'s layout
rules) with its menu of the remaining actions (Lock, Log Out), and the Places page: a sidebar of Computer, History and Frequently Used with a list for
the one chosen. The Applications sidebar no longer lists "Recently Used" (it moved into Places, `showRecentApps: false`) and shows a separator after
All Applications.

**Original-versus-native, measured** (`native/kickoff/footer-comparison.json`; native start/end minus original, px, along the footer row, seven content
groups: Applications tab, Places tab, Sleep, Hibernate, Restart, Shut Down, Session): dark `[0,+1] [0,0] [-1,-1] [0,+1] [-1,0] [0,0] [0,0]`, light
`[0,+1] [0,0] [-1,-1] [0,+1] [-1,-1] [0,0] [0,0]`. Every group is within 1 px; the group count is equal in both modes. The footer is 45 px high, its
top edge and the accent line on the same rows as the original.

**Behaviour, all through the same geometry for drawing, hit testing and keyboard** (`src/launcher/styled/kde/tests.rs`): tabs switch pages (pointer,
keyboard) and reset the list; Sleep acts at once and closes; Restart, Shut Down and Log Out arm on the first press (caption "Restart?", accent frame),
act on the second and close, Escape or a press elsewhere or a tab switch takes the arming back; Hibernate appears only where `/sys/power/state` offers
disk; the Session menu opens upward inside the card, takes the keyboard focus, Escape closes it before the launcher, a press outside only closes it,
Lock runs, Log Out confirms; when the card is too narrow for the buttons every action moves into the menu (`collapseActionButtons`); every footer item
stays inside the card at 100 / 125 / 150 / 200 % and three card sizes; Home / Show KRunner / documents open through `xdg-open` / `decspot`; History
orders applications and recent documents by time, Frequently Used by launch count. System actions run through the same commands as `decsession`
(`crate::session`); the unit tests record them and never run one.

**Differences in section 1, recorded and not averaged away:**

- Restart / Shut Down / Log Out take a second press where Plasma opens its leave screen (a full-screen confirmation with a countdown).
- Save Session and Switch User are not offered (Plasma hides them too when the session manager or display manager cannot do them; Decebal has neither).
- The Session menu always opens upward inside the card; Plasma's menu is a separate popup that is bottom-posed on top and side panels.
- The Computer list lacks Network (`remote:/`) and Recent Files / Locations (`recentlyused:/`), which are KIO locations; "System Settings" is listed only when installed; devices are the removable media mounted under `/run/media/$USER`, not Solid's list.
- Tooltips of the icon-only Session button and the Places icons are not drawn.
- The Places lists are read by one background worker when the tab first opens (nothing at start-up, nothing while painting); until it answers the tab shows Show KRunner, Home and a "Reading places..." note. Without the host's waker hook (`kickoff-places-hook.md`) the answer appears at the next input event.
- The History list has no timestamps for applications launched outside Decebal (KActivitiesStats is not available); only launches from this launcher and Spotlight count.
- Fonts, text rendering (no subpixel/hinting match), and the exact icon rasterisation differ from Qt's.

**Real window, real window manager** (`tests/e2e/plasma-packages-native.py --cases kickoff`; `native/kickoff/real-window-matrix-report.json`, `.log`, `real-bottom-100-kickoff-*.png`): the real `declaunch` (kde style) opened from the taskbar's Start under the pinned rdwm `ab1b588` on an owned private Xvfb, with root's immutable `decdock` `ec0106d5…` (Start-anchor fix `fbddaf8`); `systemctl`, `loginctl`, `rdwm-lock`, `poweroff` and `reboot` are recording stand-ins first on `PATH`, so nothing reaches the host. Per case: Places tab click moves the accent line to the Places tab and changes the page, Applications brings it back; Sleep runs `systemctl suspend` and closes Kickoff; Restart's first press runs nothing, Escape keeps Kickoff open and takes the arming back, two presses run `systemctl reboot` and close; Shut Down two presses run `systemctl poweroff`; the Session button opens a menu, Escape closes the menu and not Kickoff, Lock runs (`rdwm-lock`). **24 / 24 pass** (4 edges x 100 / 125 / 200 % x dark / light). An earlier run of the same gate had 8 failures at 200 % (a fixed delay shorter than the debug build's repaint of the larger card); the harness now polls, and that run's log is kept (`real-window-matrix-run1-8-timing-failures.log`). Log Out and Hibernate were not exercised natively (Log Out runs `rdwmctl action quit`, Hibernate depends on the host's `/sys/power/state`); both are covered by the unit tests.

## Section 2: header and sidebar (dev-validated, full replica incomplete)

Written without a build (no Cargo, no GUI, no native run was allowed for this increment): `rustfmt --check` is clean, nothing has been compiled, and the
regressions below have **not been run**. The packages stay NOT READY. The source/date discrepancy of the Identity section still applies (capture dated
30 Sep 2025, pinned source from Oct 2026, depth-1 clones): measurements come from the capture, behaviour from the pinned QML.

**Measured on the capture** (655 x 516 card at 100 %, logical px relative to the card; the original screenshot is not in the repo, only its path/sha256 in Identity):

| Part | Original | Native (`kde.rs` `frame()`) |
| --- | --- | --- |
| header band | 46 px, separator on row 45 | `HEADER_H = 46` |
| avatar button | 28 px, centre (23, 23) | 28 x 28 at x = 9, vertically centred |
| user name | x = 42, regular, 14 px | same, `hs`-scaled |
| column divider | x = 235 (the sidebar's right edge) | 1 px at `side_w - 1`, from the header to the footer |
| search field | x = 239, y = 8, 338 x 30 | `side_w + 4`, y + 8, ends 5 px before the configure button |
| configure button | centre 57 px from the right edge (the Keep Open slot sits right of it) | centre `w - 57`; a card narrower than 480 px moves it into the right-hand slot (`w - 22`) |
| sidebar frame | x = 4, y = 52, 208 wide | `side_w - 27` wide at x + 4 |
| sidebar rows | 35 px pitch, icon box at x = 11, caption at x = 44 | `ROW_PITCH = 35`, icon at row x + 7, caption at row x + 40 |
| row shape | Breeze frame radius 5 | `ROW_RADIUS = 5`; current/hovered row: accent 0.17 fill with a 0.55 accent outline; keyboard focus: 2 px accent ring |

**Behaviour taken from the pinned plasma-desktop `Header.qml`/`main.qml`/`rootmodel.cpp`** (c68d33a9… / dec158c0…): the avatar button opens the user's account
settings (`KCMLauncher.openSystemSettings("kcm_users")`), the configure button the applet configuration; here they run `decsettings users` and
`decsettings launcher` (Decebal Settings' Users & Login and Launcher panes) and close the launcher. Both are real controls (pointer and keyboard Right/Enter);
the search field gets a clear button while there is a query (the QML SearchField's clear action). Sidebar: Favorites (`bookmarks`), All Applications
(`applications-all-symbolic`), then the menu categories sorted by their title as the root model does, with the themed `applications-*` icon when the icon
theme has one. The old separator row is gone (it was a non-focusable fake item); the divider is painted on the boundary under All Applications.

**Deliberately not done (deferred, no decorative inert control was added):**

* **Keep Open (pin) button.** It binds `hideOnWindowDeactivate`; Decebal's launcher closes on focus loss in the shared window host, so a working pin needs a
  host option (not in this module). The slot is left empty rather than drawing a dead button.
* The name's hover swap to `login@host`, the avatar's face image (`~/.face`; an initial is drawn), tooltips on the header buttons.
* The extra 9 px of spacing around the original's separator (the shared lists have one row height), Help category, the original's scrollbar style (a thin
  neutral bar is drawn beside the sidebar when it scrolls), exact icon artwork (icons come from the user's icon theme, no artwork is copied).
* The category set still differs (native adds Settings, Science, Other; the original lists Development, Education, Games, Graphics, Help, Internet,
  Multimedia, Office, System, Utilities).

**Tests added (now passed in the root gates below)** (`src/launcher/styled/kde/tests.rs`, last section): geometry against the numbers above (±2.5 px); sidebar contents/order/no separator;
current row painted accent-tinted by pixel sampling; header items are exactly avatar + configure (no pin) and each runs its `decsettings` pane and closes;
keyboard Right/Enter on the header; clear button only while searching and it clears; header controls inside the card, non-overlapping and hit-testable on
320 x 240 and 400 x 300 screens at 100 / 125 / 200 % on all four edges.

## Full port gap list (still open; the packages stay NOT READY)

1. **Card size.** The native card now opens at the original's 655 x 516 (`placement.rs`, one shared line, the natural size of the Kde style); in the original the size comes from Kickoff's implicit-size rules (grid units, scrollbar presence), which are not reproduced, so a different font size or panel would size it differently there.
2. **Header.** Geometry and the avatar/configure/clear controls are Section 2 (dev-validated). Still open: avatar image (the user's face, `~/.face`) instead of an initial, the Keep Open (pin) button (needs a host option), name hover swap, tooltips.
3. **Sidebar.** Row geometry, order (alphabetical) and semantic icons are Section 2 (dev-validated). Still open: the category set (the original shows Development, Education, Games, Graphics, Help, Internet, Multimedia, Office, System, Utilities; native adds Settings, Science, Other), the original's separator spacing and scrollbar style, hover-to-switch-category (off by default), exact icon fidelity per theme.
4. **Content.** Grid cell size and label wrapping (favourites at 64 px icons with centred two-line labels), the list delegate (name / description layout, compact mode), All Applications with section headers and the separator rows, drag and drop to reorder favourites, the per-application context menu (add/remove favourite, "Add to Panel", "Show in menu editor"), alphabetical sort setting.
5. **Search.** KRunner's runner results and categories (applications, files, bookmarks, recent documents, system settings) and their keyboard behaviour; native searches installed applications only.
6. **Material.** Plasma's dialog frame (`dialogs/background` SVG with its corner radius and shadow, translucency and blur), focus rings of Breeze controls, animations (page transitions, the footer height change).
7. **Behaviour.** Opening animation, `hideOnWindowDeactivate` and pin, vertical-panel and top-panel variants (the card's anchor and the Session popup direction), RTL mirroring, accessibility roles.
8. **Platform.** X11 only here; Wayland untested.

## Evidence and verification

`cargo test --lib launcher::` 55 passed (placement tests for every style, edge and scale included); `cargo test --test plasma_theme_ports` 13 passed (including the `[launcher] style = "kde"` assertion that had been
waiting for a build slot); `cargo clippy --lib --tests -- -D warnings` clean. Debug build of this branch's commit on root `7272017` plus the changes
described in `kickoff-contract.md`.

## Main integration acceptance (2026-10-31)

The footer, session controls and asynchronous Places loader are integrated into the shell; the full Plasma package remains NOT READY for the gaps above. Main integration `2c81d18` passed 84 launcher/fuzzy tests (4 ignored), formatting, all-target Clippy with warnings denied and a dev launcher build. Evidence: `/var/tmp/decebal-kickoff-main-integration-gate/result.json`.

A missing icon pack now uses semantic session/navigation glyphs after exact theme lookup and before generic fallback (`a7b6ca8`). Fifteen icon-related tests, formatting and the dev launcher build passed: `/var/tmp/decebal-launcher-semantic-icons-gate/result.json`. This is a fallback, not bundled Breeze artwork.

The immutable dev artifact was then exercised in private Xvfb: 40 captures over dark/light, 100/200% UI scale, all four panel edges and unanchored placement, two screen sizes, and four two-monitor placement cases. Keyboard launch, Escape, hover, outside-click and pointer launch passed; wheel scrolling was not established with the short fixture list. The dark 100% footer capture was visually inspected. Evidence: `/var/tmp/decebal-kickoff-native-control-icons/result.json` and `/var/tmp/decebal-kickoff-native-evidence-icons/manifest.json`; no rescued/leftover fixture processes and no I/O-pressure abort. Optimized artifact acceptance and live installation remain outstanding.

## Header/sidebar main acceptance (2026-10-31)

Main integration `4d82b4d` passed 113 focused launcher/fuzzy/icon/symbol tests (5 ignored), formatting, strict all-target Clippy and a dev launcher build. Evidence: `/var/tmp/decebal-kickoff-header-main-source-gate/result.json`. Visual review found a missing Configure icon when no icon pack was available; root added theme-first semantic Favorites and Configure fallbacks. Final `40e6e92` passed 48 focused KDE/icon/symbol tests (3 ignored), formatting, strict all-target Clippy and the launcher build: `/var/tmp/decebal-kickoff-header-configure-source-gate/result.json`. These test sets overlap and must not be summed as distinct coverage.

The immutable final dev launcher passed 40 native captures plus keyboard/pointer, monitor placement and account/configuration/clear-query header actions. The two Settings actions invoked fixture siblings and closed the launcher; no real Settings window was opened. The clear-query check replaced an invalid search with Atlas and launched the expected fixture app. No leftover/rescued process or I/O-pressure abort: `/var/tmp/decebal-kickoff-header-native-control-v2/result.json`, `/var/tmp/decebal-kickoff-header-native-evidence-v2/manifest.json`. The dark 100% capture was visually inspected after the Configure fallback fix. Wheel scrolling remains unproved with the short fixture list.

The matrix uses macOS palette tokens to exercise this launcher's independence from theme colour selection. It proves the measured geometry/behaviour, not a matched Breeze artwork/palette pixel replica. Exact original assets, the remaining header/sidebar gaps, full package acceptance, optimized artifacts and live installation remain open. The running desktop is unchanged by this increment.
