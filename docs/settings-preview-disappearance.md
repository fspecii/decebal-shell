# Settings disappearance while browsing previews: diagnostic, 2026-10-30

The reported disappearance remains **unreproduced**. The user's trigger was browsing or generating theme previews, not Escape. The pending-widget-choice Escape fix and dashboard popup focus handling are separate issues and do not establish this report's cause.

## Evidence

The archived live Settings binary is:

`~/.local/state/decebal/deployments/settings-gallery-live-20261030/rdwm-decebal-decsettings`

SHA256 `534b6ed19471fe9adc9c3fc0b0a8334fce6de3ab961d6783a4eca43eba8aca9a` (source checkpoint `f598679`). The original stderr was empty. The last debug snapshot at 19:47:39 showed Appearance, Sidebar, no focused control, and frame 10. A replacement launcher was present at 19:47:42 with empty stderr. The parent investigation found no corresponding Settings core or OOM record in 19:35–19:52. Absence of a core does not establish a clean exit.

`tests/e2e/settings-preview-stress.py` uses an owned, PID-checked private Xvfb, private HOME/all XDG directories, inaccessible private bus endpoints, fake external mutating/network commands, and copies repository theme packages into its temporary fixture. It never sends Escape or Apply, or touches the live display. It scrolls across previews, samples RSS/threads/FDs/exit status, corrupts one private PNG when requested, and launches a fresh Settings against the same cache. It requires at least one generated PNG so an empty gallery cannot count as a successful preview test. Gallery re-entry bypasses the old known first-open wake bug.

| Binary | Bounded native result |
| --- | --- |
| Exact archived old SHA above | 20 seconds: 22 generated PNGs, 21–47 MiB RSS, 7–10 threads, 8–14 FDs; no unexpected exit. Corrupt-cache fresh launch also survived. |
| Available debug `target/debug/decsettings`, SHA `3c743ec489ad5ae3ae33fb6a08045a2a0debfb8bc6ee5abc9bf0f05db50572a3` | 40 seconds: 20 PNGs, 34–70 MiB RSS, 6–8 threads, 8–9 FDs; no unexpected exit. Separate 20-second run plus corrupt-cache relaunch also survived. |

Both used installed `decthemectl` SHA `ab48219b5692a539c567b72650c9f54e83ebbde3f0f6f7a42cea19db31ff4933` to preserve the old worker protocol. The debug binary is identified by hash and is not asserted to contain every uncommitted change. Stress stderr and warm-relaunch stderr were empty. A controlled SIGTERM between cold/warm phases returned -15; this is harness cleanup, not a crash.

Private evidence: `/tmp/settings-preview-old-final2`, `/tmp/settings-preview-old-final3`, `/tmp/settings-preview-new-fixture-stress`, `/tmp/settings-preview-new-final` (JSON samples, exact-XID screenshots, logs). Early runs without generated PNGs were invalid and excluded.

## Source findings and next diagnostic

At `f598679`, preview scrolling and gallery painting have no `Action::Close` route. Settings can still exit through explicit Ctrl+W, WM_DELETE, destruction of its own X window, a backend connection/server error, or an external signal. The unfiltered `DestroyNotify` arm is not evidence of a preview-child bug: this connection selects STRUCTURE_NOTIFY on its own window and does not select root SUBSTRUCTURE_NOTIFY.

A separate **candidate**, not an attributed cause: launching generic `decsettings` without a pane calls `single::acquire`, which sends SIGTERM to an existing instance. Explicit-pane launches use `hold` and navigation. A second generic launch therefore can cause exactly a no-core/no-stderr disappearance. There is no recorded signal sender tying this to the user's report.

Before claiming a fix, capture one recurrence with a supervised launch recording wait status and timestamps, plus opt-in backend exit-reason logging (WM_DELETE, own-window destruction, app Close, connection failure). Record generic-instance signal requests with sender/target PID. This distinguishes a signal/toggle from a render error without guessing or silently changing window lifetime behavior.

This bounded no-WM Xvfb exercise does not exclude interaction with rdwm, live compositor, user-installed packages, long-running resource growth, or a particular failing theme. No production changes were made for this investigation.
