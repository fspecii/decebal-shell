# Desktop performance recorder

`tools/desktop-performance.py` records a short, repeatable sample of processes the caller names. It is a standalone Python standard-library tool. It does not install anything, does not change the session, and does not talk to rdwm, a panel, or a desktop service.

It is not a GNOME, KDE, or other desktop-environment benchmark. A comparison is only a difference between two recordings this tool produced, and only when those runs used the same duration and period.

## Run

```sh
python3 tools/desktop-performance.py \
  --pid PID [--pid PID ...] \
  --duration SECONDS \
  --period-ms MILLISECONDS \
  --output recording.json \
  [--force] \
  [--compare earlier.json]
```

`--pid`, `--duration`, and `--period-ms` are required. There is no default process list and no default length.

The JSON file is the recording. A short summary is printed on stdout. CPU in that summary is always "percent of one core".

## Bounds

| Input | Bound |
| --- | --- |
| PIDs | 1 to 32 distinct pids, each in `1 .. 4194304` |
| Duration | whole seconds, 1 to 60 |
| Period | whole milliseconds, 100 to 2000 |
| Observations | at most 600 |

An observation is one snapshot of every nominated PID, including the sample at the start and the sample at the requested duration. The count is `floor(duration_ms / period_ms) + 1`.

60 seconds at 100 ms would be 601 observations, so that pair is rejected. 60 seconds at 101 ms (595 observations) and 59 seconds at 100 ms are inside the cap. The tool does not drop samples silently to make an over-cap request succeed.

PIDs are not discovered by scanning `/proc`. A pid that is not on the command line is not opened.

## CPU

100% means one core was busy for the measured monotonic time between two alive snapshots. The percentage is:

```text
(delta utime + delta stime) / CLK_TCK / elapsed_seconds * 100
```

`elapsed_seconds` is the actual monotonic interval, not the requested period. `utime` and `stime` come from `/proc/<pid>/stat` and already sum the process's threads. A process using two cores can read about 200. The recorder does not divide by the number of online CPUs. Online CPU count is stored as `online_cpus` with role `informational_only_not_a_divisor`. `machine_share_percent` is always JSON `null` and its status is `not_reported`.

Worked example, `CLK_TCK = 100`:

| utime delta | stime delta | elapsed | percent of one core |
| ---: | ---: | ---: | ---: |
| 50 | 25 | 1.0 s | 75 |
| 50 | 25 | 0.5 s | 150 |
| 200 | 50 | 1.0 s | 250 |

The summary `cpu_percent_one_core` is the same formula over the sum of tick deltas and the sum of elapsed time of intervals whose status is `ok`. It is not an average of the per-interval percentages. The unweighted mean is a separate field.

A backwards `utime` or `stime` is `counter_regressed`. The tool does not add a guessed `2**32` or `2**64` modulus: that would publish a made-up rate. The interval's percent fields are JSON `null`.

An interval shorter than 1 ms, or a machine that does not report `CLK_TCK`, has no percent. A real measured zero (the counters did not move) is a number with status `ok`. Denied, missing, zombie, reused, and malformed results are not zeros.

## What is read

For each nominated pid, and only while that pid is still the same task:

| Path | Used for |
| --- | --- |
| `/proc/sys/kernel/random/boot_id` | boot identity |
| `/proc/<pid>/stat` | state, comm fallback, utime, stime, threads, starttime, rss |
| `/proc/<pid>/comm` | comm, at most 64 bytes, when the file is readable |
| `/proc/<pid>/fd` | how many names are in the directory |
| `/proc/<pid>/smaps_rollup` | the `Pss:` line, in bytes (`kB * 1024`), when permitted |

Final path components are opened with `O_NOFOLLOW`. Descriptor targets are not read. `Pss_Dirty` is not treated as `Pss`.

Not read: `cmdline`, `environ`, `cwd`, `exe`, `root`, `mem`, `maps`, `smaps`, `io`, and the caller's other files. The recorder does not send signals, attach, read the clipboard, run a shell, or query a desktop backend. `git rev-parse HEAD` is attempted once so the recording can name its source commit; if that fails the hash is JSON `null` and the status is `unavailable`.

Comm from the `comm` file wins. If that file is unreadable, the comm already parsed from `stat` is kept and marked `fallback_stat`. Nothing else is used as a name.

## Identity

A task is `(boot_id, pid, starttime)` plus a pidfd when the kernel provides one.

- `pidfd_open` is used only on the real `/proc` recorder, and only for a nominated pid. The fd is never signalled. It is closed when the recording ends.
- If the pidfd is readable, the pinned task has exited. A `/proc/<pid>` entry with a different `starttime` is a reused pid: status `reused`, and its counters are not stored.
- If pidfd is unavailable, later snapshots still have to match `starttime`. A mismatch is `reused`.
- State `Z` is `zombie`. Zombie samples do not carry CPU, RSS, thread, fd, or PSS numbers.
- After `missing`, `exited`, `reused`, `zombie`, `denied`, or `malformed`, later observations repeat that status and do not open the pid again. A recycled pid cannot attach to the rest of the window.
- The confirming `stat` read after fd and PSS collection has to match. If it does not, those extras are dropped.

`pidfd` can be `open`, `unsupported`, `denied`, `missing`, or `skipped` (fixture mode). Unsupported is reported. It is not described as a successful pin.

## Status vocabulary

Process `outcome` is `measured` only when at least one interval has status `ok`. Otherwise it is the specific failure: `no_interval`, `missing`, `denied`, `malformed`, `unsupported`, `zombie`, `exited`, `reused`, `cancelled`, or `unmeasured`.

Interval status is `ok`, `not_alive`, `counter_regressed`, `elapsed_too_small`, `unsupported_clock`, or `malformed`.

PSS and fd summaries use `ok`, `partial`, `denied`, `unsupported`, `malformed`, `capped`, `symlink`, or `no_measurement`. `capped` means the fd directory was larger than 65536 names; the count is null rather than a short count presented as exact. A missing `smaps_rollup` is `unsupported` (older kernels), not zero.

## Output

`--output` has to be a final regular path. The parent directory must already exist. The tool refuses:

- a symlink, including with `--force` (the link is not followed and the target is not replaced)
- a directory, fifo, or other non-regular node
- an existing regular file, unless `--force`

The bytes are written to a temporary file in the same directory, fsynced, then published. Without `--force` the publish is `link`, which fails if the name now exists. With `--force`, `rename` replaces a regular file. A failed replace leaves the previous regular file in place. The mode is `0600`.

`--compare` reads one existing regular JSON file, at most 8 MiB, and not a symlink. It has to be a different path from `--output`. If the file is not this tool's schema, or the duration or period differs, `matched_run` is false, `processes` is empty, and no per-process deltas are printed. A missing CPU or RSS value on one side has delta `null` and status `not_measured`. It is not compared as zero. The comparison object always has `"desktop_environment_benchmark": false`.

## Cancel and exit

SIGINT and SIGTERM set a flag. The sampler stops on the next check (sleeps are at most 50 ms), closes pidfds, and writes one complete JSON document with `cancelled: true` and `cancel_reason` of `SIGINT` or `SIGTERM`. Nominated processes are not signalled. A cancel mid-window does not fill the remaining samples with zeros.

| Code | Meaning |
| --- | --- |
| 0 | every nominated pid has a measured interval |
| 2 | the request is outside the bounds |
| 3 | the output or comparison path cannot be used |
| 4 | cancelled; a JSON file was still published when the path was accepted |
| 5 | the window finished, but at least one pid was not measured |

Exit 5 is a finished recording with an honest per-pid outcome, not a failed file.

## Tests

```sh
python3 tests/desktop_performance.py
```

The fixture tests parse `stat` (including the awkward comm used by the Activity Monitor parser), reject malformed and over-wide numbers, refuse to invent RSS, check one-core rates and actual elapsed time, reject counter regression, enforce the pid/duration/period/600-observation limits, and walk a private proc tree through reuse, zombies, permission errors, symlink refusal, PSS zero versus denial, fd-count caps, and output boundaries.

`Procfs(allow_pidfd=False)` does not call `pidfd_open`. Fixture pid numbers are not live process reads.

The last two tests spawn helpers the test owns:

- SIGTERM to the recorder (two idle helpers). The recorder exits cancelled and does not kill the helpers. The test then reaps them and checks they are not zombies.
- One busy helper and one idle helper for a 2 second recording at 200 ms. The busy loop is one thread and stops itself after 5 seconds. The test's wait is 4 seconds and kills the recorder if that expires. Both helpers are reaped on the way out.

Those tests pass only the helper PIDs. They assert the sentinel in the helper argv and environment is absent from the recording.

Private evidence, when `DESKTOP_PERF_EVIDENCE_DIR` is set, is `probe.json`, `probe.csv`, and `meta.json` in that directory. The meta file carries the git hash, the one-core CPU figures, and the limits. It is one run of this tool. It is not a hardware claim and not a desktop comparison.

## Limits

- Linux `/proc` only. No install, no extra packages, no root elevation.
- At most 32 pids, 60 seconds, 600 observations, comm truncated at 64 bytes.
- No command lines, environments, or descriptor targets.
- No process control. A reused pid is dropped, not attributed.
- Host CPU aggregate and per-core breakdowns are not sampled. Process CPU is only the one-core total described above.
- PSS depends on `smaps_rollup` and permissions. Denial is a status.
- Without pidfd, reuse is caught on the next `starttime` mismatch, including the confirming read in the same snapshot. It is not a guarantee about a pid that was already recycled before the first read; that case is `reused` or `exited` when a pidfd was opened, and otherwise the first `starttime` is what gets pinned.
- `--compare` will not rank desktops. Unmatched duration or period produces no deltas.
- The recorder is not wired into the shell. Shipping, review, and any live configuration stay with the integration owner.
