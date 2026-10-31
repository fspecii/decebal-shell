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
`XDG_CACHE_HOME` selects the cache root; `--slots-dir` is for isolated tests or another explicitly shared budget.
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

Queue wait defaults to 60 seconds (exit 75 if unavailable); execution defaults to 30 minutes (exit 124 on timeout).
`--queue-timeout` and `--timeout` accept finite seconds. Interrupts and an active pause return 130. The command
has no stdin; output is inherited, with no environment dump or separate log. Redirect noisy output to a private
evidence file. The owned process group is cleaned up before locks are released; test harnesses must still reap
any deliberately detached `setsid` processes themselves. A timed-out or interrupted result is not a passed test.

Lightweight regression checks (no Cargo, desktop or network):

```sh
python3 tests/desktop-job.py
```
