# Lock screen rendering performance

The X11 `rdwm-lock` wrapper resolves `declock` first. On the audited installation that is `/home/ambsd/.local/share/rdwm-macshell/bin/declock`.

The old renderer trimmed parsed font faces after each frame. Text measurement then reparsed faces on the next keypress, even when glyph images were cached. It also repeatedly ran the general affine backdrop scaler for the panel and every X11 upload band.

The lock painter now retains its fonts until it is dropped. Clock, avatar and name are cached separately from the password/status area, refreshing when the displayed clock/date changes. A separable fixed-point bilinear pass expands the already blurred backdrop once; redraws copy cached pixels. Typing uploads only the password/status band. Expose and monitor-resize paths still restore full covered regions.

Private Xvfb at 3440×1440, same debug build profile and render/upload benchmark:

| Measurement | Before | After |
|---|---:|---:|
| Capture through first completed full upload | 5868.72 ms | 1638.58 ms |
| Key redraw/upload median, 20 samples | 1147.69 ms | 21.89 ms |
| Key redraw/upload p95 | 1206.13 ms | 22.99 ms |

These are debug rendering timings, excluding PAM and keyboard grabbing; production timing depends on hardware and build profile. Caching adds approximately one full-size RGBA background (about 20 MB at this resolution), panel snapshots and retained font metrics for the lock's lifetime. They are released when the UI is dropped.

Run the isolated regression:

```sh
tests/e2e/lock-perf.sh
```

It starts an unused private Xvfb (`:237` by default), uses temporary HOME/config/runtime directories, and invokes only an ignored rendering unit test. It never runs the locker, acquires grabs, creates an authenticator or submits credentials. It compares cached rendering against a complete redraw, checks actual X11 pixels after partial uploads, and requires median key rendering below 250 ms (`DECLOCK_PERF_MAX_KEY_MS` can adjust this on slower test machines).

The auth, password buffer, backoff, worker, keyboard/pointer grab and unlock paths are unchanged. Lock tests can run without even loading the current user's PAM configuration:

```sh
cargo test --lib lock:: -- --skip pam_starts_for_the_current_user
```
