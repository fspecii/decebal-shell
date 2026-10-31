# Exported application menus (`src/appmenu`)

The bar's application menus come from the focused window's **own exported menu**, never from
a built-in list of File/Edit/View entries. When the window exported nothing, the bar says so
honestly or hides the application menus; the shell's system menu (logo menu, window commands)
is separate and stays.

## Protocols

| Protocol | Status |
| --- | --- |
| `com.canonical.AppMenu.Registrar` (window id → service + object path) | Implemented: observe an existing owner, or serve the name when it is free |
| `com.canonical.dbusmenu` (`GetLayout`, `Event`, `AboutToShow`, `LayoutUpdated`, `ItemsPropertiesUpdated`, `ItemActivationRequested`) | Implemented (client side) |
| `org.gtk.Menus` / `org.gtk.Actions` (GTK `GMenuModel` export, found via `_GTK_UNIQUE_BUS_NAME` / `_GTK_MENUBAR_OBJECT_PATH` X11 properties) | **Not implemented** (see gaps) |
| Wayland (`org_kde_kwin_appmenu`, KWin-only, no rdwm or wlr equivalent) | **Not supported**: Wayland focus reports `Unavailable::Wayland` |

Sources used: libdbusmenu-glib 0.4 headers on this machine
(`/usr/include/libdbusmenu-glib-0.4/libdbusmenu-glib/menuitem.h`: property names `type`,
`label`, `enabled`, `visible`, `icon-name`, `icon-data`, `accessible-desc`, `toggle-type`
(`checkmark`/`radio`), `toggle-state` (0/1/other), `shortcut` (`aas`), `children-display`
(`submenu`), `disposition`; events `clicked`/`opened`/`closed`), the dbusmenu D-Bus interface
(`GetLayout(i parentId, i depth, as props) → (u revision, (ia{sv}av))`, `Event(i, s, v, u)`,
`AboutToShow(i) → b`, signals `LayoutUpdated(u, i)`, `ItemsPropertiesUpdated(a(ia{sv}), a(ias))`,
`ItemActivationRequested(i, u)`), and the registrar interface (`RegisterWindow(u, o)`,
`UnregisterWindow(u)`, `GetMenuForWindow(u) → (s, o)`, `GetMenus() → a(uso)`, signals
`WindowRegistered(u, s, o)`, `WindowUnregistered(u)`). Qt 6.12 on this machine registers
`/MenuBar/<n>` through `com.canonical.AppMenu.Registrar` (strings in `libQt6Gui.so.6`); its
Wayland path uses `org_kde_kwin_appmenu` (in `libQt6WaylandClient.so.6`).

**Who actually exports:** Qt 5/6 widget apps on X11 (only when a registrar exists **when the app
starts**), GTK 2/3 apps with `appmenu-gtk-module` loaded (not installed here), libdbusmenu users,
and some Electron/Chromium builds on X11. Plain GTK3/GTK4/libadwaita apps, Firefox and Chrome
by default export **nothing** over dbusmenu; the bar must show "This application does not export
a menu." for them rather than a fake menu.

## Behaviour

* **Registrar ownership.** `GetNameOwner` first. With an owner: observe it (`GetMenuForWindow`,
  its `WindowRegistered`/`WindowUnregistered` signals, only from that owner). Without one (and
  `RegistrarPolicy::ObserveOrOwn`): `RequestName` with `DO_NOT_QUEUE | ALLOW_REPLACEMENT`, never
  `REPLACE_EXISTING`. A desktop registrar that later asks with `REPLACE_EXISTING` takes the name
  and is then observed. When an observed owner leaves, the worker requests the free name. On
  stop it sends `ReleaseName`. The served table refuses window 0, invalid paths, hijacking a
  window another live client registered, and more than 512 windows (64 per client); a client's
  windows are dropped (with `WindowUnregistered`) when it leaves the bus.
* **No activation.** Every call carries `NO_AUTO_START`.
* **Focus input.** `AppMenus::set_focus(Focus)` is a one-slot latest-value channel; the module
  starts no window tracker. `Focus::X11 { window }` is the EWMH `_NET_ACTIVE_WINDOW` client id
  (exactly what Qt/GTK/Chromium register). `Focus::Wayland` → `Unavailable::Wayland`.
* **Commands.** `opened(token, id)` → `Event(id, "opened")` + `AboutToShow(id)` (refetch when it
  answers true); `closed(token, id)` → `Event(id, "closed")`; `activate(token, id, timestamp)` →
  `Event(id, "clicked", <int32 0>, timestamp)` only when `id` is reachable through visible,
  enabled parents and is a plain item. Commands go through a bounded queue (`try_send`; a full
  queue drops the click and returns `false`). Each focus change makes a new `MenuToken`;
  commands with an old token are ignored, and replies for an old focus are dropped.
* **Updates.** `LayoutUpdated` storms are debounced (25 ms) and coalesced with in-flight fetches;
  `ItemsPropertiesUpdated` builds a new immutable tree (`Arc` swap) without refetching.
* **Caps** (`Limits`): depth 8, 1024 items, 256 children per item, 256 chars per string,
  PNG icons 64 KiB each / 1 MiB per tree (PNG signature required), 4 shortcuts × 6 keys,
  4 MiB reply bodies, 20 MiB of buffered input, 32 calls in flight, 32 queued commands,
  3 s call timeout, 2 s connect budget, reconnect backoff up to 30 s. Malformed nodes,
  negative and duplicate ids are dropped and counted; wrong-typed properties keep defaults;
  control characters are stripped from labels.
* **Threads/fds.** One `appmenu` thread per `AppMenus`, holding one bus socket and one wake
  socket pair. `stop(timeout)` joins it; dropping the handle stops it without blocking. Both
  release every fd (`tests/appmenu_lifecycle.rs`).

## Stable API (for the bar integration)

```rust
use decebal::appmenu::{self, AppMenus, Config, Focus, Protocol, State, view};

// Start only while the application-menu applet is enabled.
let mut menus = AppMenus::start(Config::session(), Arc::new(move || waker.wake()))?;

// Every time the existing tracker reports a focus change (no second tracker):
menus.set_focus(Focus::from_window(tracker.snapshot().active(), appmenu::tracker_protocol()));

// Drawing (never blocks; a mutex + Arc clone):
let snap = menus.snapshot();
match &snap.state {
    State::Ready(menu) => {
        for t in view::titles(&menu.tree) { /* t.id, t.label, t.mnemonic, t.enabled, t.submenu */ }
        let rows = view::rows(&menu.tree, title_id); // Row { id, label, shortcut, enabled, separator, toggle, state, submenu, icon_name }
    }
    other => { let _why: Option<String> = other.reason(); /* show or hide app menus */ }
}

// Input:
menus.opened(menu.token, title_id);              // dropdown shown
menus.activate(menu.token, row.id, event_time);  // row clicked (row.activatable())
menus.closed(menu.token, title_id);              // dropdown hidden

// Applet turned off:
menus.stop(Duration::from_millis(200));          // or just drop(menus)
```

`Snapshot { generation, state, registrar, notice, activation_request, events_delivered }`;
`State::{Starting, Unavailable(Unavailable), NoFocus, NotExported{window}, Loading{window},
Ready(Arc<Exported>), Failed{window, reason}, Stopped}`; `Exported { token, window, service,
owner, path, tree: Arc<MenuTree> }`.

## Bar integration (commit "Show the focused window's exported menus …")

Kept narrow so it can be rebased around the panel owner's scaling work:

* `src/menubar/active.rs`: `ActiveWindow::active_focus() -> appmenu::Focus` (default
  `Focus::None`); `WmActive` derives it from its existing tracker with `Focus::from_window`.
  Better long term: `wm::Tracker` exposes the protocol it actually started, replacing
  `appmenu::tracker_protocol()`'s mirror of `Tracker::start`.
* `src/menubar/menus.rs`: `build()` returns only `apple`, `app`, `window`, `help`; the static
  File/Edit/View lists are gone.
* `src/menubar/exported.rs` (new): `Exported` glue: `set_enabled`, `set_focus`, `poll`,
  `compose` (splices exported titles after the app menu, or puts the honest reason as an inert
  first row of the app menu), `click` (`Sent`/`Moved`/`Ignored`; submenus drill down in place
  with a `‹ Parent` row), `opened`/`closed`, `available`/`shortcut` per row, `identity`.
* `src/menubar/mod.rs`: one `app_menu` field; `refresh_app_name` starts/stops the worker
  (live bar only: a waker exists, not embedded, classic horizontal layout, not Dux, and
  `DECBAR_NO_APP_MENUS` unset), feeds focus, rebuilds menus on change and closes an open
  exported menu whose identity changed; `open_menu`/`close` notify the app;
  `run_menu_item` routes exported rows; `draw_dropdown` takes availability/shortcut from them.
* Root may prefer a real applet/config switch instead of `DECBAR_NO_APP_MENUS`.

## Tests

* `tests/e2e/appmenu-cargo.sh test -j2 --offline --lib appmenu`: tree, view, registrar table,
  bus helpers, and the real worker against a private `dbus-daemon` (own config, no service
  dirs) with a scriptable fake exporter/registrar that records every `Event`/`AboutToShow`/
  `GetLayout`: own/observe/replace/take-over of the registrar, real item ids and timestamps on
  clicks, refused separators/disabled/hidden/submenu/unknown ids, property and layout updates,
  storm coalescing, stale replies and tokens across windows, malformed/oversized/silent
  exporters, hijack refusal, re-entrant calls during a click, no bus, Wayland, reconnect.
* `tests/e2e/appmenu-cargo.sh test -j2 --offline --test appmenu_lifecycle`: no thread, fd or
  bus name left after `stop()` or drop, three rounds.
* `src/menubar/exported.rs` test: a real `DecBar` with pointer clicks on titles and rows against the
  private bus + recording exporter: honest reason row with no exporter, exported titles after the
  app menu, AboutToShow on open, disabled rows inert, clicks delivered as ids 10 and 160 (nested
  submenu through drill-down and back), sliding to another title closes the first on the app side,
  menus disappear and the open one closes when the app leaves, disabling removes the reason row.
* `tests/e2e/appmenu-real.sh`: real exporters. A Qt 6 `QMainWindow` (Qt's own `QDBusMenuBar`,
  built from `tests/e2e/appmenu/qt-fixture.cpp` into the lane's target dir) on a private Xvfb,
  focused through the real X11 `wm::Tracker` (EWMH properties set on the private root), and a
  libdbusmenu-glib exporter (`tests/e2e/appmenu/dbusmenu-fixture.py`). Verified: Qt registers its
  real X11 window id; mnemonics, `⌃O`, check/radio states, hidden item, submenu; clicks trigger
  exactly the intended `QAction`s (recorded by the app), Qt's toggled state comes back as a
  property update, Qt's `aboutToShow` adds an item that arrives through `LayoutUpdated` and can be
  clicked, disabled Undo never triggers, Qt exiting drops its menu; libdbusmenu delivers
  `item-activated` for the clicked ids with the click timestamp. Alt+F typed into the Qt window
  (XTEST on the verified private display) makes Qt send `ItemActivationRequested` for File; the
  bar opens the requested menu once per request (covered by the native bar test).
  Display rule: reserved numbers 700-899 only, never `-displayfd`; the lock file, the socket path
  and the abstract socket must all belong to the started Xvfb before any client connects, with a
  private Xauthority cookie. Nothing foreign is ever unlinked.
* The wrapper runs cargo with a private HOME/XDG tree, no host DISPLAY/WAYLAND/XAUTHORITY/
  rdwm/session variables, an invalid session bus address and a recorder PATH, under
  `flock --close` on the build slot.

## Gaps
* Keyboard navigation and mnemonics of exported menus are not wired (pointer only); icons
  (`icon-name`/`icon-data`) are parsed but not drawn.
* Clicks carry timestamp 0 (the bar's pointer events have no X server time).
* Dropdown width is measured from labels only; long exported shortcuts may crowd labels.

* `org.gtk.Menus` export (pure GTK3/4 `GApplication` menubars) is not read.
* Wayland: no supported protocol; Qt's `org_kde_kwin_appmenu` would need compositor support.
* Transient dialogs: a focused dialog that registered no menu shows "does not export"; its
  parent's menu (`WM_TRANSIENT_FOR`) is not looked up.
* Registrations made with this process are lost if a real registrar replaces it; apps that do
  not re-register (Qt registers once per window) keep no menu until reopened.
* Qt and appmenu-gtk-module decide at startup: apps started before any registrar existed never
  export.
* `crate::dbus::Reader` recurses on nested variants; a real `dbus-daemon` validates bodies
  (nesting ≤ 64), but a hostile peer on a non-validating bus could exhaust the stack.
