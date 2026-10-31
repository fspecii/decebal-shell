# Quiescent Wayland first frame

`wayland_first_frame.py` checks a first paint without any event arriving after the backend's two startup roundtrips. It requires Linux, Python 3, Sway, swaymsg, dbus-daemon, and two builds of `examples/qa_wayland_first_frame.rs`: one linked against the backend before the fix, and one linked against the fixed backend. These are QA fixtures, not installed applications. The harness starts its own headless pixman Sway, private bus, HOME, XDG directories, and socketpair. It never connects to an existing display or sends input.

Build the identical example against two source directories using external Cargo projects. The before source must remain immutable. Each manifest has this shape (change the package/bin name for each fixture):

```toml
[package]
name = "quiescent-frame-before"
version = "0.1.0"
edition = "2021"
[[bin]]
name = "quiescent-frame-before"
path = "/absolute/current/source/examples/qa_wayland_first_frame.rs"
[dependencies]
decebal = { path = "/absolute/before/source", default-features = false, features = ["wayland"] }
tiny-skia = "0.12"
```

Use a new private `CARGO_TARGET_DIR`, generate each manifest's lockfile offline, then build sequentially with the established global build-slot lock, `flock --close`, `nice -n 10`, and `cargo build --offline --locked -j2`. Do not use production release caches. Retain the Cargo.lock files, commands, logs, backend source snapshots, and source/binary SHA256 hashes with the evidence. The root example can also be built directly with `cargo build --example qa_wayland_first_frame --no-default-features --features wayland` using the same build-slot constraints.

Run with an evidence path that does not exist:

```sh
python3 tests/e2e/wayland_first_frame.py \
  --before /absolute/private/target/debug/quiescent-frame-before \
  --after /absolute/private/target/debug/quiescent-frame-after \
  --evidence /var/tmp/quiescent-frame-unique-run
```

The fixture calls `backend::wayland_window::run` directly, avoiding the separate backend-support probe. It retains its waker without invoking it, and `a11y_name` remains `None`. Retention matters: dropping the last sender generates a client-side `ChannelEvent::Closed`, which would wake the old backend and invalidate the quiescent control. The proxy parses Wayland messages and forwards SCM_RIGHTS file descriptors. It identifies the three `wl_display.sync` requests: registry initialization, first startup roundtrip, and second startup roundtrip. It requires an `xdg_surface.configure` and corresponding client `ack_configure` before forwarding the third callback's `done`. It then permanently withholds every later server message, including a `delete_id` sharing the same socket read. The proxy keeps the connection open and forwards client requests, so the app can create its shm buffer and commit without receiving another server event.

Assertions require the configure handler marker, the marker written after both roundtrips returned, a live app process, and zero delivered server messages after the gate. With the old backend, no paint occurs during a one-second observation after the gate; the main thread's wait channel is captured. With the fixed backend, the paint marker appears before blocking dispatch, and the proxy observes a buffer attach followed by a surface commit while the server gate remains closed. Five seconds bounds fixture setup; ten seconds bounds compositor readiness. Timeouts are failure bounds, not a workaround for delayed rendering.

`results.json` records actual PIDs, owned IPC/socket paths, active outputs, binary hashes, app events, gate serials, withheld packet counts, and wait channel. Each fixture has `wire.json`, `client.log`, `app-events.txt`, and `cleanup.json`; Sway and bus logs are retained. `finally` terminates and waits for each owned process, closes proxy sockets and received descriptors, and records cleanup even if a prerequisite fails. A pass proves the isolated initial-paint regression; it does not establish the root cause of any historical run without protocol evidence.

Validated on 2026-10-31 with three independent owned compositor/bus runs. The frozen `f12e959` backend failed to paint in all three runs; the fixed backend snapshot (SHA256 `ac98b306ae8e4d1d0f55d1c400a9545227b503c7f36129c7f567390904ef0e74`) painted and committed in all three. Evidence: `/var/tmp/decebal-quiescent-frame-20261031/summary.json`, `source.json`, and `proof-{1,2,3}/`. The earlier `control-old-old/` used a discarded waker, so both old fixtures painted; it is retained with `fixture-discarded-waker-control.rs` as an explicit non-proving control. Every final proof records zero post-gate server messages delivered, matching configure/ack serials, both initial roundtrips returned, and all owned PIDs reaped and absent from `/proc`.
