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

## Root integration blocks

The bar files belong to the panel owner; these are the narrow hooks the integration needs.

1. `src/menubar/active.rs`: give `ActiveWindow` the native focus (default keeps every
   existing implementation compiling):

   ```rust
   /// The focused window as the app-menu worker's input (X11 id or Wayland), if known.
   fn active_focus(&self) -> crate::appmenu::Focus {
       crate::appmenu::Focus::None
   }
   ```

   and in `impl ActiveWindow for WmActive`:

   ```rust
   fn active_focus(&self) -> crate::appmenu::Focus {
       let Some(t) = &self.tracker else { return crate::appmenu::Focus::None };
       crate::appmenu::Focus::from_window(t.snapshot().active(), crate::appmenu::tracker_protocol())
   }
   ```

   Better long term: `wm::Tracker` exposes the protocol it actually started
   (`Tracker::protocol() -> Option<Protocol>`) so `tracker_protocol()`'s mirror of
   `Tracker::start`'s choice can go.
2. `src/menubar/menus.rs`: drop the static `file`/`edit`/`view` menus from `build()` (keep
   `apple`, `app`, `window`, `help`, which run real shell/WM commands) and append one `Menu` per
   `view::titles()` entry, carrying the dbusmenu id instead of a `&'static str` action.
3. `src/menubar/mod.rs`: own an `Option<AppMenus>` started from `start_watchers` when the
   applet is enabled; on each tick feed `active.active_focus()`; rebuild `self.menus` when
   `snapshot().generation` changes; route row clicks on exported menus to
   `AppMenus::activate(token, row.id, 0)` and open/close to `opened`/`closed`; draw nested
   submenus (`Row::submenu`) as cascades or drill-down; show `State::reason()` as one disabled
   row when the app exported nothing.

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
* The wrapper runs cargo with a private HOME/XDG tree, no host DISPLAY/WAYLAND/XAUTHORITY/
  rdwm/session variables, an invalid session bus address and a recorder PATH, under
  `flock --close` on the build slot.

## Gaps

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
