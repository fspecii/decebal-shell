# Familiar file-manager workflows

Status: implementation plan, not a completed Explorer or Finder port. The user
requires familiar file-manager behaviour as well as Windows/macOS appearance.
The current [Files profiles](file-manager-profiles.md) style Nautilus; they do
not implement a ribbon, Finder columns, navigation model or file operations.
Those requirements remain open even when the profile's native tests pass.

## Current evidence and integration points

Inspected on 2026-10-30:

- `src/theme/system/file_manager.rs` produces scoped GTK4 Nautilus CSS;
  `src/theme/system/files.rs` manages theme files and backups, not file browsing.
- `src/settings/systempane.rs` exposes appearance profiles. Keep this separate
  from default application selection and functional navigation preferences.
- `src/settings/system/defaults.rs` already selects the `inode/directory` handler.
  A new application must remain opt-in rather than changing that handler when a
  user applies a colour theme.
- The initial audit found `src/spotlight.rs::file_item` losing the exact item in
  `Act::Reveal` and opening only its parent. Phase 1 now preserves that path and
  requests selection. Ordinary Open also uses `launch::open_argv`.
- `src/launch.rs::open_argv` supports the configured `DECEBAL_OPEN` override;
  `spawn_argv` supplies the existing detached launch path.
- Installed executable discovery found Nautilus, `gio`, `xdg-open`, `gdbus` and
  `busctl`, but not Dolphin, Thunar, Nemo or PCManFM. This is a machine snapshot,
  not a packaging guarantee or a universal compatibility claim.
- Local `pkg-config` reports GTK4 4.24.1, libadwaita 1.10.0 and GIO 2.90.1.
  Cargo currently has no GTK/GIO Rust bindings. The existing UI uses tiny-skia
  and native X11/Wayland window backends.

Local protocol evidence, read without activating the live file manager:

- `/usr/share/dbus-1/services/org.freedesktop.FileManager1.service` registers
  Nautilus as the `org.freedesktop.FileManager1` activation service.
- The installed `org.freedesktop.portal.OpenURI.xml` describes OpenDirectory
  using FileManager1 ShowItems, falling back to opening the containing folder.
- Installed `KF6/KIOGui/kio/openfilemanagerwindowjob.h` documents highlighting
  files or folders with FileManager1 and falling back to the first parent.
  Revealing a directory highlights it in its parent; opening it is a different
  action. These installed references support the proposed workflow, not proof
  that Decebal already implements it.

## Phase 1: reliable reveal from desktop search

Implementation checkpoint: `src/reveal.rs` and `decutil reveal -- PATH` now
provide this operation; Spotlight preserves the item path and launches the
detached utility. Private typed-service and real Nautilus selection tests cover
the implemented scope; see [run instructions and limits](file-manager-profiles.md).
The native browser in later phases remains unimplemented.

Preserve the exact selected path in `Act::Reveal`. Add one reusable reveal
operation under `launch` (or a focused module called by it), and route ordinary
Open through the existing opener helper. Label the alternative action **Show in
file manager**. Opening a directory enters it; revealing it selects it in its
parent. Keep that distinction in the help and tests.

The reveal request must outlive Spotlight, which closes immediately after an
activation. A worker thread owned by Spotlight is insufficient. Use a detached
helper process, for example a narrow `decutil reveal -- <path>` command, started
through `launch::spawn_argv`. The helper sends the typed FileManager1 request and
owns its reply/fallback. `src/bin/decutil.rs` dispatches this focused utility
command before the general parser.

Use correctly encoded local file URIs, no shell interpolation. Retain the
selected symlink identity rather than silently canonicalising to another folder.
The existing `dbus::Conn` and `Writer` support typed calls and arrays; connection
and blocking calls use five-second socket timeouts. Keep all connection/protocol
work outside painting and input dispatch, and establish an overall bounded
completion contract rather than assuming one socket timeout bounds every step.
Verify the exact ShowItems object path, signature and activation behaviour in a
private bus fixture before relying on it.

On absent/unsupported service or bounded failure, open the containing directory
once using `open_argv`. Do not claim the item is selected after fallback. A custom
opener override must have an explicit tested policy: respect it by opening the
parent when no compatible selection mechanism is configured, rather than
silently launching another file manager. A vanished item may still reveal its
existing parent; invalid/no-parent inputs must report a useful failure. Avoid
persistent workers, retries that create duplicate windows and file execution.

This increment improves a familiar workflow. It does not close the full
Explorer/Finder implementation requirement.

## Phase 2: choose the functional browser foundation

Recommended direction to evaluate: a separate native file-manager application
with GTK4/GIO, sharing Decebal palette/icon selection and profile vocabulary.
Keep the existing default handler until the new application passes operational
acceptance and the user explicitly selects it. Keep the shell's tiny-skia build
independent, using a separate package/crate or an explicitly optional component.

A GTK4 prototype can use established focus, text input, accessibility, list/tree
and drag/drop infrastructure. GIO is a candidate for asynchronous enumeration,
monitoring and operations, but its bindings, dependency versions, cancellation,
remote backends and error semantics still need explicit evaluation. Installed
libraries are not evidence that a Rust binding is cached or a new dependency has
been approved/integrated. Libadwaita should be optional to the design decision:
its adaptive widgets may help, but must not force both profiles into an
unmodified GNOME layout.

Building the full browser in the current tiny-skia toolkit would share existing
painting and theme code, but requires implementing accessible multi-selection,
editable paths, drag/drop, clipboard MIME interoperability, menus and complex
focus semantics. It must not ship a visually accurate browser whose file
operations or screen-reader access are inferior. No current evidence establishes
that infrastructure is ready.

Do not fork Nautilus or adopt another file manager just to rename its theme.
An upstream extension/patch route needs a separate feasibility and maintenance
assessment: the current scoped CSS cannot rearrange its application structure.

The first browser prototype should exercise one common model with distinct
layouts, rather than duplicate operation engines:

- Windows profile: familiar sidebar, breadcrumb/editable address location,
  list/details and icon views, predictable context actions and visible operation
  controls. A ribbon/command bar is a real interactive surface if advertised.
- macOS profile: familiar sidebar and toolbar, list/icon views and a real
  hierarchical column browser if advertised. Columns must navigate and preserve
  selection, not be decorative panels.
- Both: back/forward/up history, tabs, home and common folders, mounted locations,
  sorting and search; keyboard operation and clear errors at every step.

Theme selection may choose presentation defaults, never rewrite user bookmarks,
view/sort choices, default app, file contents or clipboard state. Explicit layout
choices stay fixed across palette changes.

## Phase 3: operational acceptance and release boundary

Run destructive operations only on isolated fixture trees. Record before/after
contents, selection, focus and operation outcome; screenshots alone do not prove
correct file handling. Preserve current Nautilus profile regression coverage.

| Area | Required behaviour and evidence |
| --- | --- |
| Reveal increment | Exact item highlighted in private Nautilus; file and directory reveal differ from Open. Test spaces, Unicode, `#`, `%`, quotes, symlinks and leading hyphens. Private fake service verifies typed URI/body, timeout/error fallback once and custom opener policy. Kill/close Spotlight before delayed reply; helper still completes. |
| Navigation | Address typing/paste, breadcrumbs, back/forward/up and tabs reach the expected real paths. Invalid/unreadable paths preserve the prior location and show recoverable errors. |
| Familiar layouts | Actual Windows details/icon controls and macOS hierarchical columns operate on the same model; selection survives appropriate view changes. Labels cannot promise absent views. |
| Selection and opening | Pointer, keyboard ranges/toggles, rename editing and multi-selection agree. Enter/open-with launches the chosen handler once; folders and executables follow explicit policies. |
| File operations | Create, rename, copy, move, trash, restore and explicit permanent delete produce verified bytes/metadata in fixture trees. Handle conflicts, partial failure, permissions, read-only mounts and cancellation without false success. No overwrite without the stated conflict choice. |
| Progress and undo | Long operations remain responsive, report progress/errors and support the advertised cancellation/undo semantics. Undo after external changes cannot silently destroy newer data. |
| Clipboard and drag/drop | Interoperate with installed external file managers for copy/move and dropped file URI lists. Preserve names and distinguish copy from move; no arbitrary shell parsing. |
| Search and scale | Search cancellation/stale results, large directories and slow backends do not block input. Measure bounded memory and incremental updates on a documented fixture size. |
| Accessibility | Keyboard-only complete workflows, visible focus, assistive-technology roles/names/actions and real screen-reader operation. Test 100/125/200% UI/text, narrow/short windows, light/dark/high contrast and reduced motion. |
| Desktop integration | Correct default-handler opt-in, theme/palette/icons, launch/reveal from shell, notification actions and window management on X11 and actual Wayland. Test missing services and application restart. |
| Recovery and release | Clean login/upgrade/uninstall, interrupted operation recovery, independent package dependencies and no regression to existing default file manager or user settings. |

The staged plan can land useful increments before the complete browser, but the
full request remains open until its advertised navigation, layouts and operations
are implemented and verified. GTK availability, passing CSS tests or a polished
mockup are not substitutes for these gates.
