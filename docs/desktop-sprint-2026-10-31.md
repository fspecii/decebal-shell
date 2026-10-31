# Desktop sprint: 31 October 2026

This ten-hour sprint improved the native desktop and its release tooling. It does not establish GNOME/KDE feature parity or hardware performance superiority. Implementations below are native adaptations; upstream desktop names do not imply exact replicas or complete ports.

## Delivered

- **Window management:** Super+Left/Right/Up/Down half-workarea snapping, panel-aware geometry, preserved snap/maximize geometry against client resize requests, and exact runtime-layout export/restore for supporting WM versions. Existing configurations keep explicit shortcuts until migrated.
- **Panels:** shared applet configuration on all four edges; constrained-output fitting and overflow; responsive Control Center; task attention/minimize states and real-window-geometry workspace pagers; keyboard interaction and display-scale rendering for the macOS-style dock.
- **Launchers:** selectable native Windows, GNOME, KDE, XFCE, BlackBerry, Dux and macOS directions, with serialized toggle/activate lifecycle handling and private repeated-toggle acceptance.
- **Settings:** responsive persistence workers, retained drafts on save failure, bounded supervised asset installation, schema-gated gesture controls and a native AT-SPI2 adapter. Accessibility protocol tests do not establish screen-reader or hardware acceptance.
- **Theme packages:** reviewed GNOME, openKylin/UKUI, Deepin and Lingmo adaptations; source-derived decorations and centred UKUI island geometry; palettes and semantic icon fallbacks. KDE pager infrastructure is delivered; the separate Breeze/Layan package candidates are not shipped.
- **Icons/cursors:** optional pinned catalogues and a bounded shared installer, atomic ownership/selection writes, explicit text/directory alias contracts, alias-cycle rejection and bounded diagnostics. Availability depends on each catalogue's verified source and licence. Lingmo cursors remain unavailable because upstream licensing was not established.
- **Ecosystem:** optional native Files workflows, Notes/Timer integration and read-only `decwidgets data --json` snapshots using the same widget-data resolver. Notes failures retain the draft; timer completion survives closing its window without duplicate notifications.
- **Engineering:** immutable release manifests, read-only verification/deployment plans, private transactional rollback tools, scoped performance recording, an isolated desktop lab and disposable KVM shell acceptance.

The shell release contains 44 theme packages. Curated photograph metadata covers 60 photographs across 31 visible packages; unknown authorship/licensing is retained explicitly, and artwork is not redistributed under an invented licence.

## Qualification

The standard optimized shell release is `7837e1b6cebc2e892d0cd618720dbd51adf2667d`, manifest SHA-256 `cb0a50b361b0d3680629abda321cd16915640ad0ae8ea3e56d401a981b5cecf3`. It includes 23 application binaries plus the test example, with separately sealed matching assets. QA-only fault binaries were never installed.

| Gate | Result | Scope |
| --- | --- | --- |
| Full library run | 2,173 passed; 1 failed; 19 ignored | Sole failure selected an older system CLI for a schema cross-check |
| Same schema test with pinned current CLI | 1 passed | No source change; original full-run failure retained |
| Focused integration batch | 132 passed, 1 ignored; 24 integration checks passed | Later source changes separately qualified |
| Strict all-target Clippy | Passed | Production-equivalent pre-release source |
| Optimized macOS dock matrix | 588/588, 52 scenarios | Private X11, exact WM artifact identities |
| Optimized Windows panels | 16/16 | Published struts/software rendering, not a physical WM session |
| Optimized workspace pager | 24/24 | Private exact-artifact WM; non-shipped KDE fixture |
| Optimized widget JSON | 5/5 | Read-only isolated state fixtures |
| Notes/Timer/widgets | 8/8 | All three binaries from the same standard release |
| Fault-enabled Settings | 79/79 assertions | Separate QA-only binary; 66 baseline + 13 new assertions |
| OpenKylin island geometry | 48 native cases | Four edges, scale/mode/size matrix; 24 horizontal measurements |
| Installed WM private X11 | 404/404 | Exact optimized artifacts; does not authorize forced live replacement |
| Disposable KVM shell guest | 15/15 required steps | Earlier standard `af22367` shell; full WM/hardware flags remain false |

The fault wrapper originally expected 81 assertions by double-counting two shared checks. Its original failed wrapper record remains; root compared the complete 66-check baseline and all 13 new gesture assertions before accepting actual coverage. No checks were deleted or fabricated. The full library result is likewise not described as a single clean run.

For schema tests, point `RDWMCTL` at the matching built CLI when `/usr/bin/rdwmctl` belongs to an older release. Preserve the assertion; do not replace it with a skip.

## Activation and remaining work

The standard shell binaries and matching assets were installed to both managed user locations. Bar, HUD and dock were refreshed with exact process-identity checks. Configuration, open applications, workspace and focus were preserved. A ten-second observation showed no new busy loop; it is not a desktop-wide performance benchmark.

The old Settings window was left open to preserve drafts. Close and reopen it to use the installed version. The old live WM cannot export a complete compatible layout, so it was not forcibly restarted. The updated WM is installed for a safe fresh login. The privileged system session wrapper was not replaced.

After starting a compatible new WM, migrate existing shortcuts using the installed `rdwm-super-arrows-migrate` helper. It first verifies the *running* server's actions, preserves custom chords, validates the candidate, backs up the configuration and can roll back a failed reload. Geometric focus moves to Super+Ctrl+Shift+arrows. A staged configuration was validated, but not applied against the incompatible old session.

Final high-scale Settings acceptance discovered a reproducible small-window crash at 640×540 with 200% UI and text. Source follow-up `d67b39f` caps the sidebar to preserve page space and rejects invalid rounded-rectangle sizes. Four root regression tests, focused strict Clippy, owner Settings tests and independent source review passed. The optimized follow-up stayed alive in General and Keyboard at 640×540 and Keyboard at 320×240; light/dark keyboard-focus and unchanged-config checks passed. Its manifest SHA-256 is `8e9003016f8810c7cbf4db76aff704a9663706b424f5527e656c2d23a0a9d466`, binary SHA-256 `49ca07c3ff11a9f2d4723b74545cd2f17e30b44e7e7cac639d7792aa807487ab`. It was installed at 11:18 UTC into both managed locations, without closing the old Settings window. This is a **Settings-only follow-up**: shared `gfx.rs` changed in source, but the other 22 application binaries and example remain the standard 783 build. The capture wrapper accidentally truncated its earlier exit-code line; its complete JSON/PNG/config/cleanup evidence was independently verified and the original truncated log retained. The separate reproduction exited 0. At the extreme small-window/high-scale combination, some labels and part of the threshold slider still extend beyond available space; crash safety does not establish full small-window usability. The earlier historical preview-related disappearance remains unconfirmed and must not be claimed fixed by this finding without evidence.

Outstanding acceptance includes physical multi-monitor/HiDPI input, gestures, suspend/lock/login recovery, screen readers, Wayland DRM/GLES/shadow timing and gesture forwarding, and broader GTK/Qt/Files newcomer workflows. Source comparisons, software tests and private VMs do not substitute for those checks. Theme fidelity, licensed asset completeness and sustained memory/frame-time benchmarks remain ongoing work.

## Continuing the work

Start from the committed contracts rather than screenshot-only approximations: [panel and theme docs](settings-design.md), [desktop assets](desktop-asset-packs.md), [Notes/Timer](notes-timer.md), [isolated desktop lab](desktop-lab.md), and [scoped performance recorder](desktop-performance.md). The WM repository documents snapping migration, exact-layout activation and VM acceptance separately.

Prioritize physical login/recovery acceptance and a usable small-window/high-text-size Settings layout before adding another theme family. Next, qualify the complete screen-reader workflow and Wayland hardware paths. Keep theme source/licence records, exact artifact identities and original failure evidence with every accepted increment.
