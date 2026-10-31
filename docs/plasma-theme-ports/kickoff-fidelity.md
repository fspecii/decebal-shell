# Kickoff: native launcher against the original (KDE Plasma 6)

Status: **section 1 of the Kickoff port is done (the footer and the Places tab); the rest of the card is still the earlier approximation.** The packages stay
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

## Full port gap list (still open; the packages stay NOT READY)

1. **Card size.** The native card now opens at the original's 655 x 516 (`placement.rs`, one shared line, the natural size of the Kde style); in the original the size comes from Kickoff's implicit-size rules (grid units, scrollbar presence), which are not reproduced, so a different font size or panel would size it differently there.
2. **Header.** Avatar image (the user's face, `~/.face`) instead of an initial; the search field's geometry; the configure (sliders) and pin buttons (pin keeps the popup open when another window activates; the gear here opens Decebal Settings).
3. **Sidebar.** Row style (the current row has Breeze's accent-outlined frame; the original hover/focus frames), Breeze category icons (`applications-development`... not the freedesktop menu icons), the category list and order (the original shows Development, Education, Games, Graphics, Help, Internet, Multimedia, Office, System, Utilities; native adds Settings, Science, Other), the thin scrollbar, hover-to-switch-category (off by default).
4. **Content.** Grid cell size and label wrapping (favourites at 64 px icons with centred two-line labels), the list delegate (name / description layout, compact mode), All Applications with section headers and the separator rows, drag and drop to reorder favourites, the per-application context menu (add/remove favourite, "Add to Panel", "Show in menu editor"), alphabetical sort setting.
5. **Search.** KRunner's runner results and categories (applications, files, bookmarks, recent documents, system settings) and their keyboard behaviour; native searches installed applications only.
6. **Material.** Plasma's dialog frame (`dialogs/background` SVG with its corner radius and shadow, translucency and blur), focus rings of Breeze controls, animations (page transitions, the footer height change).
7. **Behaviour.** Opening animation, `hideOnWindowDeactivate` and pin, vertical-panel and top-panel variants (the card's anchor and the Session popup direction), RTL mirroring, accessibility roles.
8. **Platform.** X11 only here; Wayland untested.

## Evidence and verification

`cargo test --lib launcher::` 55 passed (placement tests for every style, edge and scale included); `cargo test --test plasma_theme_ports` 13 passed (including the `[launcher] style = "kde"` assertion that had been
waiting for a build slot); `cargo clippy --lib --tests -- -D warnings` clean. Debug build of this branch's commit on root `7272017` plus the changes
described in `kickoff-contract.md`.
