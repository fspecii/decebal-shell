# Screen readers and other assistive technologies (AT-SPI2)

Decebal windows built on `WindowApp` can publish a semantic accessibility tree over AT-SPI2, the protocol Orca and
other Linux assistive technologies use. System Settings (`decsettings`) is the first complete adapter: a screen
reader can read, navigate and operate every page of it. The launcher (`declaunch`) is the second, in every style it
ships. Bars, docks and other `App` surfaces do not publish a tree yet; nothing here makes the whole desktop
accessible.

## Turning it on

Nothing happens until accessibility is enabled for the session. Each window watches `org.a11y.Status.IsEnabled` on the
session bus (set by Orca, by GNOME's accessibility switch, or with
`busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true`) and connects the moment it
becomes true; it disconnects again when it becomes false. It never starts the bus launcher itself.

- `DECEBAL_A11Y=0` keeps the bridge off; `DECEBAL_A11Y=1` connects without waiting for `IsEnabled`.
- `AT_SPI_BUS_ADDRESS` names the accessibility bus directly (instead of `org.a11y.Bus.GetAddress`); without a session
  bus it also counts as "enabled".

While no assistive technology listens, a window builds no tree; the only cost is one idle worker thread asleep in
`poll`.

## What Settings exposes

| Part | Roles and interfaces |
| --- | --- |
| Window | `frame` (active while it has keyboard focus) in an `application` named "System Settings" |
| Sidebar | `panel` with the search `entry` (Text, EditableText, caret, selection; "Clear search" button) and the page `list` (Selection; one `list item` per page with its section, or per search result) |
| Page | `scroll pane` named after the page; `heading` (title, level 1; section headers, level 2); notes as `label`, errors as `alert` (announced) |
| Cards | `panel` per card; a row with one control names the control after the row and describes it with the row's detail; a row with several controls is a `grouping` |
| Toggle | `switch` (checkable, checked; action `toggle`) |
| Slider / Stepper | `slider` / `spin button` with Value (min, max, step, current, text); actions `increment`, `decrement` |
| Segmented choice | `grouping` of `radio button`s (Selection) |
| Tabs and sections | `page tab list` of `page tab`s (Selection) |
| List choice | `combo box` (has-popup, expanded/collapsed, action `press`) holding its `menu` of `menu item`s (Selection). While the list is open it is showing and modal, and keyboard focus moves through its items |
| Colour | `button` (has-popup, colour attribute) that opens the modal colour `dialog`: swatch buttons, a hex `entry`, Default |
| Text | `entry`, or `password text` for secrets; drafts until activated (attribute `draft=true`), exactly like typing then Enter |
| Shortcut | `button` that starts recording (pressed while recording) |
| Buttons, chips | `button` (destructive attribute for dangerous ones), `toggle button` |
| Galleries | wallpapers, layouts, themes, accent swatches and icon themes as `list`s of selectable `list item`s |
| Theme cards | `panel` per theme with its buttons; previews as `image` |
| Widget layout choice | `dialog` holding the question and its three buttons |
| Config & Presets editor | multi-line `text` (Text, EditableText, caret, selection; errors in the description, invalid-entry state) |
| Toast | `notification` (announced) |

Every control whose setting has a default also offers `reset` (the right-click "back to default").
`AccessibleId` is a stable key (`ctl:<control id>`, `pane:<page>`, `search`, ...): it survives redraws, scrolling and
rebuilds, and the numeric object paths do too for the life of the window.

## What the launcher exposes

`declaunch` publishes one tree for the Launchpad, Dux, Windows, GNOME, KDE, Xfce, BlackBerry and UKUI styles
(application "Launcher", `frame` named after the style). The Launchpad and Dux are described from their own grid and
list layout; the six styled launchers from the style engine's zones, i.e. the geometry the style really laid out and
painted, so each style shows its own tabs, lists and buttons and a style that adds or moves a control is described
without a change to the adapter. Only what is on screen is described: a row scrolled off, a page that is not showing
or a list that is cut at the card has no node and so no action.

| Part | Roles and interfaces |
| --- | --- |
| Search | `entry` (Text, EditableText, caret, selection, placeholder as in the style; action `activate` opens the selected app, or the first result). Its extents are exact in the Launchpad and Dux; the style engine does not publish where a styled launcher puts its search box, so there it has none |
| Status | `status bar` "N results" / "No matching apps" while searching; a `notification` "No matching apps" appears once when a search reaches no match (announced). Outside a search, an empty list that the style paints a text over (Kickoff's "Nothing here yet...", Whisker's "Empty: right-click...", BlackBerry's "Nothing here yet") is the `status bar` with those very words plus one polite `notification` (`announce:nothing`) while it lasts. Typing, hovering, key moves and result counts that merely change are not announced |
| App lists | `list` (Selection) of `list item`s, one per app, named after the app, described by its desktop `Comment=` (and "Pinned"), `posinset`/`setsize` attributes, Selected for the highlighted one. Actions `activate` (open), and in the styled launchers `pin` / `unpin`. An app that cannot start (missing `TryExec`) is disabled |
| Letter headings | `heading` (level 2) in alphabetical lists |
| Categories, views, widget tabs, page dots | `page tab list` of `page tab`s (Selection); the current view or page is Selected; action `click` shows it |
| Buttons | `button`s for what the style draws: All apps / Back, Settings, Power, Account, Computer, Expand / Restore, Clear search, Close / Show Apps, and the best-match Open and Pin buttons (Windows) |

`AccessibleId`s are `search`, `status`, `zone:<zone>`, `<zone>/app:<desktop id>`, `<zone>/view:<view>`,
`<zone>/page:<n>`, `detail/open:<desktop id>`, ...: apps are keyed by desktop-file id, never by list position, so
a node survives searching (the results list shows the same app as the same object), reordering by pinning,
scrolling and paging. An app that shares its id with another gets `#n` in catalog order.

Focus: while the last key typed text, the focus is the search entry; after an arrow, Tab or Page key it is the
highlighted app (or button) the launcher will act on. Enter, a click and the accessibility `activate` all open the
selected app. Keyboard navigation itself is unchanged and stays style-consistent: the adapter only follows it.

Actions are the launcher's own: `activate` on an app starts exactly that app through the same launch as Enter or a
click (detached, so the launch is asynchronous), records it in the usage history (styled launchers) and closes the launcher like the
keyboard does; if the launch fails the request is refused with the reason and the launcher stays open. `pin` /
`unpin` toggle the same favourites list (and file) as a right click. Choosing a tab or page, clearing the search and
the footer buttons run the engine's own `activate` arms; a test compares each of them with a real click on the same
control and requires the same response and the same state. Editing the search text through EditableText gives the
same results, selection and focus zone as typing it. A request that raced a keystroke (the app is no longer in the
list, already pinned, ...) is refused, never applied to whatever moved into its place.

Style-owned controls: the controls a style draws itself (`Target::Style`) are described by the style
(`Styled::semantics`: stable key, role, name, description, enabled, current) and activated by the style engine's own
`activate`, so they do exactly what a click does. Kickoff (KDE) is the one style that has any:

| Kickoff control | Role and behaviour |
| --- | --- |
| Footer tabs Applications / Places | `page tab`s in a `page tab list`; the current one is Selected; `click` switches (Places starts its background read) |
| Places sidebar (Computer, History, Frequently Used) | `page tab`s in the sidebar's `page tab list` (Selection); the current place is Selected |
| Places rows (Show KRunner, computer places, history documents) | `list item`s named and described by the row; `click` opens it (disabled when it has nothing to open) |
| Footer system buttons (Sleep, Restart, Shut Down, ...) | `button`s; Restart, Shut Down and Log Out ask for a second `click`: the name becomes "Restart?" and the description "Press again to confirm", exactly as the caption does |
| Session button | `button` with has-popup, Expanded / Collapsed; `click` opens and closes the menu |
| Session menu | modal `menu` (Selection) of `menu item`s, operable by `click` or by choosing the row; while it is open every other request is refused except the Session button that closes it |
| Header avatar and configure | `button`s (Account settings, Configure launcher) |

While Kickoff reads its places in the background, the tree has a busy `status bar` "Reading places..." and one polite
announcement; the lists fill in when the read finishes. A style that adds controls of its own must describe them in
`Styled::semantics` (and may start `Styled::busy` work): the source tests fail for any style-owned control left
undescribed, instead of leaving it out of the tree. Controls the engine draws but no style owns (Windows' All apps /
Back and best-match buttons, GNOME's Show Apps, UKUI's sidebar) are described by the adapter as before.

Still missing: the extents of a styled launcher's search box (the engine does not publish them, so the field is
described without a rectangle; the Launchpad and Dux have exact extents). Bars and docks are not covered, and nothing
here claims physical Orca acceptance.

## How actions run

Requests from the assistive technology are typed operations (an action name, a value, a focus or scroll request, a
text edit, a child selection). They reach the window's own thread through a bounded queue and run through the same
code as the keyboard: `Model::apply`, opening a list, starting and committing an edit, moving focus, scrolling into
view. Nothing synthesises pointer or key events. A request is rejected with a D-Bus error that says why when:

- the object no longer exists (`...Error.UnknownNode`),
- it changed after the request was made, e.g. a second activation racing the first, or a file reload in between
  (`...Error.Stale`),
- the control is disabled (`...Error.Disabled`), or does not offer the operation (`...Error.Unsupported`),
- the value or offsets are invalid (`org.freedesktop.DBus.Error.InvalidArgs`),
- the settings model refused it (`...Error.Refused` with the same text as the error shown on the page),
- a list or the colour dialog is open (modal) and the request is for something else (`...Refused`),
- 32 requests are already waiting (`org.freedesktop.DBus.Error.LimitsExceeded`, at once),
- the window did not get to it within 5 s (`...Error.Timeout`; the request is then dropped, never applied late;
  `DECEBAL_A11Y_REPLY_TIMEOUT_MS` changes the deadline, 100 to 60000).

Settings writes its files (with `fsync`) on its UI thread. Under heavy disk load one save can take seconds; while it
runs, the window cannot answer, and an assistive technology with a short call timeout (libatspi's default is far below
5 s) reports a missing reply even though the change is applied. Moving saves off the UI thread is outside the
accessibility bridge.

An accepted action is answered after the window has repainted and published the new tree, so a client that reads
right after its call returns sees the result (state-change and value events are sent before the reply).

Passwords: a secret field exposes one bullet per character, never its characters; it cannot be copied or cut.

## Coordinates

- **X11**: extents in screen coordinates are real root-window coordinates (the window's origin, tracked through
  configure, map and focus events, plus the painted rectangle). Window and parent coordinates are also supported.
  Hit-testing (`GetAccessibleAtPoint`) uses the same rectangles.
- **Wayland**: a client cannot learn its global position, so screen coordinates are reported as window-relative
  (screen = window coordinates, origin 0,0). Window activation follows `wl_keyboard` enter/leave; whether a window
  can actually take focus (`GrabFocus` moves Settings' own keyboard focus, not the compositor's) is up to the
  compositor. Magnifiers that need global positions cannot follow Decebal windows on Wayland.

Rectangles are physical pixels at the window's UI scale. Text ranges and characters report the field's rectangle
(glyph positions are not exported).

## Protocol coverage

Implemented from the at-spi2-core **2.62.1** interface XML (tag `2.62.1`; SHA-256 of the files used):

| File | SHA-256 |
| --- | --- |
| Accessible.xml | `8099f87a97ee4086baa73fba2b0d8755a33e9216ede0aa12617b0db52c55e786` |
| Application.xml | `e5ac0e3c197b9a657e8347c8d0b5e7cd73ae4ce5b0a409b3a875a3aa7476ec08` |
| Action.xml | `ae8df12ef63c6c50284e172ecc904f74b6431e4d4b961cf6da9c9207480fb588` |
| Cache.xml | `dd3b7429733cb896574183d05aa4862568d27c426016bc39b637db64d2ba0dc7` |
| Component.xml | `ae87d87a4db76f0ab2f0aa8357a0392f71f1fa58b719990165cecb2bfa45d138` |
| EditableText.xml | `2ea1b94822f19b0b00c80b918b89833cfb67d1eeef99d69b8421d0e6f40920ff` |
| Event.xml | `d4661d2bbcf5c6d03ba7a5d6d81c481eae803421796e69acebe1a277bbb19948` |
| Selection.xml | `63eb9c10977192bb7cfb3e17684680101132b26c0b6277b2246e21eba27b1f8b` |
| Socket.xml | `70c3fe7dc29d65d9671ce96b183a65accd7bca5d1d2546a412f9b01921b63ee7` |
| Text.xml | `5c2d5049d2e427d630ca1ae288d0abe321f39c683336cb8a1373f41c4414d614` |
| Value.xml | `3fd8623364f0d8ac8bd2b7f5b23a055f74c35e8af0bb29076c8be10466f7a737` |
| atspi-constants.h (role/state numbers) | `50e4604cef992f9a1bf6917d5548583a808e80405ef416cc87f3f9666f350501` |
| bus/at-spi-bus-launcher.c (org.a11y.Bus / org.a11y.Status) | see `SHA256SUMS` in the evidence folder |

- Accessible (all methods and properties), Application (Id set by the registry during `Embed`), Component (extents,
  position, size, layer, hit-testing, contains, `GrabFocus`, `ScrollTo`; move/resize requests return false), Action,
  Value (settable `CurrentValue`), Text (text, characters, words, sentences, lines, caret, one selection; empty
  attribute runs), EditableText (set, insert, delete, cut, copy, paste), Selection (single choice: deselecting
  returns false), Cache (`GetItems`), Properties, Introspectable, Peer.
- Events: `PropertyChange` (name, description, value, role, parent), `StateChanged`, `ChildrenChanged`,
  `TextChanged`, `TextCaretMoved`, `TextSelectionChanged`, `SelectionChanged`, `Announcement` (new alerts and
  notifications), window `Activate`/`Deactivate`. Focus events go first; at most 1024 signals per change.
- Not implemented: Collection, Table, Hypertext, Image, Document, `BoundsChanged`, Cache `AddAccessible`/
  `RemoveAccessible`, peer-to-peer connections (`GetApplicationBusAddress` returns ""), the X root window `AT_SPI_BUS`
  property as an address source, and listener-based event filtering (all events are sent).

## Limits

A tree holds at most 4096 objects (it says so when cut), strings 4 KiB, text 256 KiB; a list exports its first 500
choices (the rest stay reachable by type-ahead in the open list).

## Testing it privately

Launcher: `src/launcher/a11y/tests.rs` has source tests that build the real launcher in every style, describe it and
dispatch requests the way the host does (stable ids across search / reorder / scrolling, the exact app launched, stale
/ disabled / offpage refusal, the modal menu, editable search semantics, focus and selection, bounds against each
style's zones, parity with real clicks). `tests/e2e/a11y_launcher.py` is a drafted private native probe for the same
(declaunch per style in a private bus, Xvfb and tree, fixture apps whose `Exec=` writes a marker file); it has not been
run.

`tests/e2e/a11y_settings.py` runs the real `decsettings` inside a private HOME/XDG tree, a private `dbus-run-session`
(real `at-spi-bus-launcher`; the real `at-spi2-registryd` is started on the private accessibility bus because its
dbus-broker would ask systemd to activate it), a private Xvfb and, when installed, a private headless sway, and drives
it with GI Atspi and raw Gio calls:

    cargo build --bin decsettings
    python3 -I tests/e2e/a11y_settings.py --bin target/debug/decsettings --evidence ~/.cache/decebal-a11y-evidence/run

It never touches the live session bus, display, input, configuration or processes.

### Review repair awaiting execution (2026-11-01)

Search activation in the launcher is now restricted to application targets.
Moving accessibility focus to search, editing it, or activating its default
action cancels a pending Kickoff leave-action confirmation; it cannot confirm
Restart/Shutdown left armed elsewhere. The explicit second press on the system
button remains the confirmation route. SetText now filters control characters
like keyboard input. New tests cover direct search activation, focus followed
by activation, returning to the explicit two-press flow, and single-line text.
These repairs have passed rustfmt/whitespace checks only; build/test and native
AT-SPI/Orca acceptance are pending. Modal disabled states were repaired in
`1ad235a`; non-search empty announcements are described below; launchability
caching remains open.

The repaired candidate `0ab32ba` passed `cargo check --locked --offline -j1
--all-targets` with unchanged source fingerprints and no resource interruption.
Evidence: `/var/tmp/decebal-launcher-a11y-source-check/result.json`. Regression
tests are type-checked but have not executed; native accessibility acceptance
and the remaining source findings are still open.

Modal-tree follow-up: while Session is open, obscured focusable controls and
selection containers are now advertised disabled with no actions. Session and
its visible menu rows remain enabled. Existing modal tests now require disabled
states and validation errors, rather than enabled nodes whose dispatch refuses.
This follow-up is formatted but has not yet compiled or executed.

### Empty lists outside a search (awaiting execution)

The words a style paints over a list with nothing in it come from one function
per style (`empty_text` in `kde.rs`, `xfce.rs`, `blackberry.rs`) that both the
painting and the accessibility tree call (`Styled::empty_note`), so the screen
reader is told exactly what is shown and a change of wording cannot leave the two
apart. The tree has an inert `status bar` with those words and one polite
`notification` while the list is empty; both persist across redraws, hovering and
key moves, so nothing is announced twice, and the announcement is made again only
after the list had been non-empty. While Kickoff reads its places (`Styled::busy`)
there is no empty note: the busy status and "Reading places..." announcement stand
in, and the empty words appear only when the read has finished with nothing. A
search with no match keeps its own status and announcement ("No matching apps").

Not covered: Windows paints two hints (no pinned apps; no recent apps) only when
its Recommended block fits, a decision made from geometry private to that style, so
Windows, GNOME and UKUI (which list every app and paint no such text) have no empty
note. Source tests for all of this are written and have not been built or run.

Validation correction: a shared Cargo target reused a different worktree's test
executable and ran zero launcher tests. That run is explicitly invalidated in
`/var/tmp/decebal-launcher-a11y-completed-tests/result.json`; it is not a pass.
A dedicated checkout-owned target, seeded only with dependency outputs and no
root-crate fingerprints/incremental state, then compiled and executed 44 tests:
43 passed and the modal-focus test expected the wrong error category. Focus is
non-mutating in the common validator, so a non-focusable background node correctly
returns Unsupported rather than Disabled. The test now checks that refusal and
that menu focus remains unchanged. Evidence from the failed qualified run is
retained in `/var/tmp/decebal-launcher-a11y-qualified-tests/`. A fresh passing run
is still required. Earlier shared-target compilation records are superseded by
this explicit-source/required-test-evidence process.
