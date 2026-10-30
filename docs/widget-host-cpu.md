# Widget host CPU regression (decwidgets leaked one spinning thread per rebuild)

## Symptom
The installed `decwidgets daemon` used ~350-400% CPU for hours, even with no cards enabled. Read-only `/proc` sampling of the
running daemon showed the main thread asleep and **four separate worker threads each at 100%**, with almost no voluntary
context switches (they never block). They had been created at different times after start-up.

## Cause (proven, not guessed)
`widgets::host::run` rebuilds every window and drops its whole `Live` data source whenever the config stamp, selected theme,
widget file or wallpaper (`_XROOTPMAP_ID`) changes. `Live` owns an `dash::mpris::Mpris` worker when a card uses `media.*`
(e.g. *Now playing*). `Mpris` had no shutdown: dropping the handle closes the write end of its wake-up socketpair and drops the
command sender, but the worker kept looping. A closed socket is *always readable*, `read` returns 0 bytes and `try_recv` returns
`Disconnected`, so `poll` returned immediately every iteration: a permanent hot loop, one leaked thread per rebuild. gdb on a
private reproduction put every hot thread at `src/dash/mpris.rs:612` (the wake-socket drain loop) while the single live worker
sat correctly in `poll_readable` (line 607). The glass sampler (250 ms refresh, full-wallpaper capture) is **not** involved: the
same run without a media card stays at ~1% across rebuilds with the glass theme active.

## Fix
- `dash::mpris`: the worker stops when its handle is dropped (`drain_commands` detects EOF on the wake socket or a
  disconnected command channel).
- `widgets::host`: a rebuild keeps the running `Live` when the data groups and commands the cards need are unchanged
  (`Live::matches`); only a different set replaces it. This also keeps CPU/network history across theme or wallpaper changes
  instead of resetting it every time.
- `widgets::data::Live` now has a stop flag set on drop: user command runners and the rdwm tag watcher
  (`menubar::rdwm::spawn_watcher_until`, which wakes its blocking subscription read every second to look at the flag) end with
  their source instead of running for ever. The bar's own `spawn_watcher` behaves exactly as before.
- Not changed: the glass sampler/capture, event loop, card painting, collectors and actions. Known remaining leak: the
  `window.*` group's `wm::Tracker` threads are not stoppable on a genuine group change (blocked sockets, no CPU).

## Reproduce / verify (private Xvfb + picom + private session bus; nothing touches the desktop)
```
cargo build --bin decwidgets --example fake_mpris
WIDGETS_BIN=target/debug/decwidgets FAKE_MPRIS=target/debug/examples/fake_mpris \
  [THEME=nextkde OPACITY=0.4 THEMES_DIR=$PWD/themes] python3 tests/e2e/widgets-cpu-native.py /tmp/evidence
```
Unfixed `decwidgets` (same source minus the fix): idle 1.0%, then **+100% and +1 thread per rebuild** (4 rebuilds = 399%, 6
group changes = 1096%, 15 threads) and it never recovers when all cards are disabled (1294%). Fixed: 1.0-1.4% and a constant 3
threads through the same sequence, 2 threads with no cards; with the NextKDE glass theme 4.0-4.6% (10.0% while a live wallpaper
window changes 10x/s under the cards), glass backdrop repaints 3/3, real clicks on prev / play-pause / next reach `playerctl`
and a replaced player's title repaints the card.
