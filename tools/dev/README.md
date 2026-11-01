# Developer jobs on a desktop in active use

Background compilation and native test matrices caused severe I/O stalls on the development desktop even while
the NVMe was not continuously saturated. Use this runner for non-interactive jobs on that machine, including
commands from agent worktrees. Keep agents stopped whenever the user requests a resource pause.

```sh
python3 tools/dev/desktop_job.py -- cargo test --offline --lib settings::
python3 tools/dev/desktop_job.py --timeout 900 -- python3 tests/release-preflight.py
python3 tools/dev/desktop_job.py --pause
python3 tools/dev/desktop_job.py --resume
```

Use the same pattern for an existing native harness, with its explicit binary and evidence paths. Do not use
this wrapper to launch the user's normal desktop applications or persistent services.

The runner takes **both** existing `~/.cache/decebal-build-slots/slot{1,2}.lock` files, so it serializes against
older callers too. It never holds one while waiting for the other. Locks stay with the supervisor, not the child.
The default uses the OS account's home, so a private fixture's `HOME` or `XDG_CACHE_HOME` cannot split the budget.
`DECEBAL_BUILD_SLOTS` or `--slots-dir` can select an explicitly shared alternative; isolated unit tests use the latter.
Every checkout must use the same slots directory. This is cooperative coordination, not a cgroup limit or a
security boundary: direct commands that bypass the runner can still consume resources.

Each command gets nice 19, idle I/O priority, `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`. Explicit Cargo job
counts above one are refused. Developer/test debug info defaults to zero unless the caller explicitly selected
it. An existing `CARGO_TARGET_DIR` is preserved; otherwise a persistent cache outside the checkout is selected
from its absolute path. Do not launch parallel builds from inside one wrapper or override the thread limits in
the test executable. Build evidence must record any explicit profile changes.

`--pause` creates the shared pause flag. Waiting jobs refuse to start, and active wrappers terminate their own
command group. `--resume` only removes that flag: it does **not** restart agents, repeat failed commands or
resume old processes. Do not remove a user-requested pause until the user has agreed to the resumption scope.
Never delete lock files to bypass a running job.

The runner samples Linux `/proc/pressure/io` full-stall time over measured intervals of at least one second.
Above 10% it ends only its owned command group, returns 130 and leaves the shared queue paused. This is a
responsiveness budget, not a diagnosis of a faulty disk or proof of which process caused the stalls. Use
`--max-io-stall PERCENT` for an explicit finite budget in `(0, 100]`; unavailable pressure counters refuse the
job before it starts. A pressure-aborted test is not a pass. Inspect the system before explicitly resuming.

Queue wait defaults to 60 seconds (exit 75 if unavailable); execution defaults to 30 minutes (exit 124 on timeout).
`--queue-timeout` and `--timeout` accept finite seconds. Interrupts and an active pause return 130. The command
has no stdin; output is inherited, with no environment dump or separate log. Redirect noisy output to a private
evidence file. The owned process group is cleaned up before locks are released; test harnesses must still reap
any deliberately detached `setsid` processes themselves. A timed-out or interrupted result is not a passed test.

Lightweight regression checks (no Cargo, desktop or network):

```sh
python3 tests/desktop-job.py
```

## Read-only pressure observation

When a guarded job stops, collect a finite, low-priority observation before deciding whether to resume:

```sh
python3 tools/dev/desktop_observe.py --seconds 10 --out /absolute/fresh-report.json
python3 tests/desktop-observe.py
```

This tool sets **its own** CPU priority to nice 19 and I/O priority to idle. It does not stress the disk,
start/stop developer jobs, remove the queue pause, signal other processes or change desktop configuration.
Reports are new 0600 files; existing files and final-component symlinks are refused. It reads Linux I/O PSI
and same-UID process `stat`, `comm` and `io` metadata, never arguments, environments or open-file paths.
CPU percentages are relative to one core and use the measured interval. PID reuse and decreasing counters
are excluded from process deltas. Observation is limited to 60 seconds; default sampling is once per second.

Global PSI is **not** per-job attribution. Only processes present at both sample boundaries have deltas;
short-lived processes and kernel writeback can be missing. A quiet sample is not proof the intermittent
problem is solved, and a pressure spike is not a disk-health diagnosis. Keep the failed gate's evidence.

Initial checkpoint: five synthetic identity/counter/privacy/output tests passed, then a ten-second live
observation completed with a maximum measured full stall of 0.026%. The preceding quiet-period sample had
contained a 44.921% one-second spike, so heavy jobs remain coordinated rather than broadly resumed.
Evidence: `/var/tmp/decebal-observer-first-baseline.json`.

Cancellation gives the direct fixture up to ten seconds to finish its own
cleanup before forcing the command group down. Both machine slots remain held
through that interval. Multi-phase drivers must also handle SIGTERM and wait
for their active fixture; an immediately exiting driver cannot preserve a
fixture's cleanup window. Detached private compositors still require their
fixture's owned-handle shutdown and must not be killed by process name.

For native release gates, pass `--quiet-seconds 5` before `--`. The runner holds
both slots and measures the existing I/O budget before creating the command.
An over-budget observation refuses admission with exit 75, leaves jobs paused,
and starts no fixture. It does not retry automatically or change the runtime
pressure threshold. The observation accepts 1–30 seconds (0 disables it).

Use `desktop_observe.py --disks --seconds 10 --out <fresh.json>` to include
whole-device read/write/flush completion latency, active time and average queue
depth alongside PSI and process activity. This only reads counters and creates
no I/O workload. Partition rows are excluded; device-mapper and physical devices
may describe the same I/O and must not be summed. Counter resets/new devices are
skipped. Active time on modern parallel devices is not a saturation metric or a
disk-health test. Field definitions follow the
[Linux kernel I/O statistics documentation](https://docs.kernel.org/admin-guide/iostats.html).
