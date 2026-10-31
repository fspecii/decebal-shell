# desktop-lab

`tools/desktop-lab` runs one bounded command in a private session. Exit 0 means that command and the requested checks finished. It does not mean full desktop, hardware, Wayland, or release acceptance. `full_acceptance` in every report is false.

The lane starts no VM and owns at most one Xvfb. The vm lane owns `tests/vm` (2 VMs, 2 vCPU, 1 GiB). This tool does not publish, push, call a model or T3 API, read the clipboard, or kill live helpers.

## Commands

Run from the repo root:

```bash
python3 tools/desktop-lab/desktop-lab run --out /tmp/desktop-lab-out --timeout 5 -- /usr/bin/true
python3 tools/desktop-lab/desktop-lab run --spec tools/desktop-lab/examples/job.json --out /tmp/desktop-lab-job
python3 tools/desktop-lab/desktop-lab gate --spec tools/desktop-lab/examples/gate.json --out /tmp/desktop-lab-gate
python3 tools/desktop-lab/desktop-lab commits check --repo .
python3 tools/desktop-lab/desktop-lab commits check-message --file /tmp/msg.txt
python3 tools/desktop-lab/desktop-lab index --repo . --since <rev> --evidence /tmp/evidence --out /tmp/desktop-lab-index
python3 tools/desktop-lab/desktop-lab catalog --out /tmp/desktop-lab-catalog \
  --panel /var/tmp/panel-wm-acceptance-final/summary.json \
  --settings /var/tmp/settings-current-stress/summary.json \
  --settings-bins /var/tmp/settings-current-stress/bin \
  --settings-inputs /var/tmp/settings-current-stress/inputs.json \
  --preview tests/e2e/settings-preview-stress.py \
  --control ~/.local/state/decebal/ten-hour-sprint-20261031/control.json
python3 tools/desktop-lab/desktop-lab sprint --control ~/.local/state/decebal/ten-hour-sprint-20261031/control.json --out /tmp/desktop-lab-sprint
python3 tools/desktop-lab/desktop-lab fingerprint --root . --path docs/desktop-lab.md --out /tmp/fp.json
```

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | The job or gate checks passed. Still not full acceptance. |
| 1 | The command, source freeze, or trailer check failed. |
| 2 | Policy refusal: deadline, live slot, host context, VM, publish, heavy compiler, or unreviewed fixture. |
| 3 | A required binary such as Xvfb or import is missing. |

Catalog, index, sprint, fingerprint, and a clean trailer check also exit 0. They print `full_acceptance: false` and do not print a false `passed` line.

`commits check` reads `tools/desktop-lab/BASELINE` when `--since` is omitted. A missing baseline refuses the scan instead of walking all history. The check does not rewrite commits.

## Isolation

Patterns followed, and not executed or replaced: `tests/e2e/env.sh`, `tests/e2e_spot.sh`, `tests/wm_tracker_lifetime.rs`, `tests/file-reveal.py`, and `tests/e2e/desktop-frame-settings.py`. The harness does not source `env.sh`.

Every unit and job gets a new 0700 HOME and the full XDG set (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_DIRS`, `XDG_DATA_DIRS`). The child environment is an allowlist. These host values are scrubbed: `DISPLAY`, `WAYLAND_DISPLAY`, `XAUTHORITY`, `RDWM_*`, `SESSION_MANAGER`, `GTK_*`, `GDK_*`, `GSK_*`, `QT_*`, both D-Bus addresses, and secret-prefixed variables. A private `XDG_RUNTIME_DIR` alone is not isolation. `DISPLAY=:0` with the real HOME is a failed fixture mode, including the decbar-rdwm unit case, and this lane does not edit that fixture.

There is no flag that keeps the host display, home, or buses. `host_context` and `inherit_host` are refused. The CLI does not pass `--session-bus` unless asked. A spec that omits `session_bus` starts a private session bus. The system bus address is `unix:path=<root>/no-system-bus`, and that file is not created. The session bus config has no systemd activation and no host service directories.

Owned Xvfb jobs reserve an explicit display from 700 through 999. A number is skipped when `/tmp/.XN-lock` exists, when `lstat` of `/tmp/.X11-unix/XN` succeeds, or when `/proc/net/unix` lists the abstract name `@/tmp/.X11-unix/XN`. The live display `:0` can still have that abstract socket after the filesystem `X0` path is absent. The cause of the missing path is unconfirmed. Root owns the x11rb client fix for abstract transport. This harness leaves that source, and any already-running display, alone.

The argv is `Xvfb :N -screen 0 <screen> -nolisten tcp -noreset -ac`. `-displayfd` is absent, so startup does not probe from `:0`. Before the job command or a screenshot, the owned server must still be alive, the lock text must be its pid, the filesystem path must be a socket and not a symlink, and the abstract name must be present. The listening inodes come from `/proc/net/unix` lines whose path is exactly `/tmp/.X11-unix/XN` or `@/tmp/.X11-unix/XN`, with Flags bit `SO_ACCEPTCON` (`0x10000`), type `SOCK_STREAM` (`1`), and state `01`. A bound datagram is state `01` and is left out of that set. Both sets must be present in `/proc/<server-pid>/fd` as `socket:[inode]`. Those inodes are not the directory entry's `st_ino`. An extra listener on the same path fails the proof. `display_transport` records `filesystem_inode`, `abstract_inode`, the listener inode lists, `inodes_owned_by_server`, and `displayfd_probe: false`. A display below 700 is refused. If an owned server in this process is on `:0` or was started with `-displayfd`, only that spawned process is stopped, before any input. The live server and other applications are not signaled. Foreign X sockets and locks are not unlinked. Only one server runs at a time. The lane holds one Xvfb lock. Wayland jobs run only the requested compositor with `WLR_BACKENDS=headless`; those variables are not copied into the job command. Screenshots are real PNGs from `/usr/bin/import -window root` on the owned Xvfb. A missing image is a failure.

## Build slots

Plain `flock file command` passes the lock fd into the child. A daemonized `xclip` inherited fd 3 and held slot 1 after a decbar unit job. Root retired that inode to `slot1-retired-0200.lock` and cancelled only blocked, childless flock waiters on the old inode. Slot 2 was not restarted. This lane does not open, lock, write, or signal those files, does not read clipboard contents, and does not kill that helper.

Give a job a private absolute `slot` path. The parent opens it without following symlinks. Each path component is opened with `O_NOFOLLOW`. The immediate parent must be a directory owned by this user and mode 0700. The lock fd is `O_CLOEXEC` and non-inheritable. Before `flock`, and again before any truncate or write, `fstat` must show a regular file owned by this user with `nlink == 1`. A symlink or hardlink is refused and the stand-in is not modified. The three live slot paths are refused before `open`. Tests of aliases use private temp stand-ins.

The parent holds that `flock` until the contained process tree has been verified dead, then releases it. An unverified drain does not release it, and a report of the error is not that verification. The job command does not receive the fd. A guardian in its own session inherits a duplicate of the same open file description and does not set a parent-death signal. Closing one duplicate leaves the lock held while the other stays open. The guardian closes its duplicate only when the coordinator asks for release or the coordinator's pid and start time are gone, and every contained track has its own proof. Each contained track is pre-registered pending before its process is spawned. The watch record maps that track in `tracks` as `id`, `drain`, `invocation`, `pending`, `host` (`pid`, `start`), and `inits` (the same `pid` and `start` fields). `pending` stays true until the host and every init on that track have integer pids and starts. Every `run_job` chooses a new invocation nonce. That run's build and command drains are `out/invocations/<nonce>/containment-build.json` and `out/invocations/<nonce>/containment-command.json`. A track's drain proof is its own file, readable JSON whose `namespace_empty` value is true and whose `invocation` equals that track's nonce. A drain file already in the output directory from an earlier run, including `containment-command.json` with `namespace_empty` true, stays in place and is not proof for the new track. The other proof for that same track is a completed init record: `pending` is false, the host pid and start are integers, and every init pid and start are integers for processes that are gone. When `tracks` is non-empty, those rows are the proof; a flat list of known-dead inits is not. A missing, truncated, or malformed drain stays unverified. An empty init list, a pending track, an unbound drain, or an unknown init start keeps the lock. The coordinator then closes its own fd.

The contained command is started by a fresh single-threaded launcher. That launcher sets `PR_SET_PDEATHSIG` to SIGKILL, checks that its parent is still the coordinator, and only then `exec`s. It is not a Python `preexec_fn`. SIGTERM and SIGHUP in the coordinator run this cleanup. SIGKILL has no handler: the parent-death signal kills the unshare process, `--kill-child` ends the namespace, and the guardian keeps the slot until that tree is gone.

The recorded wrapper is:

```text
flock --close <slot> nice -n 10 -- <command>
```

A command whose basename is `flock` without `--close` is refused.

The command and the build run inside a private pid namespace. The probe tries `unshare --pid --fork --mount-proc --kill-child --forward-signals` and, when that is not permitted, a user namespace that maps the current user (`--user --map-current-user`, or `--map-root-user` if that is the form that exits 0). The report records the prefix that worked. A supervisor is pid 1: it forwards SIGTERM, waits a short grace, then SIGKILL, and the kernel kills whatever remains when pid 1 exits. That covers double-fork, nested, late, and cleared-marker processes. A host `/proc` marker scan is not containment and is not complete cleanup. A transient user scope is not used, because stopping one left a double-fork daemon alive. If the probe fails, the command does not start and the report says containment is unsupported.

SIGKILL and exit 137 set `kill_cause` to `unknown`. They are not coordinator queue cleanup and they are not a compiler failure. OOM, a self-kill, and `exit 137` share that status. This harness does not invent coordinator provenance and does not rerun the command. A build timeout is captured before the process is stopped, so the later signal does not clear `timed_out`.

The job process is a child subreaper only for the duration of `run_job`. That is an extra, not the cleanup claim. Direct children are eligible for a marker signal only when their parent is this process and their environment still contains the exact `DECEBAL_DESKTOP_LAB_JOB` token. Anything without that marker is left alone, including an unrelated sibling.

`catalog` adds a read-only inode/size/mtime record for the three slot files. `locked_by_this_lane`, `clipboard_read`, `helpers_killed_by_this_lane`, and `slot2_restarted` are false.

## Stubs

Safety stubs are always first on `PATH` and exit 127: `xclip`, `wl-copy`, `wl-paste`, `xsel`, `xdg-open`, `systemctl`, `loginctl`, `reboot`, `shutdown`, `poweroff`, `halt`, `hyprctl`, `swaymsg`, `gsettings`, `omarchy-theme-set`. The recorder stores the basename and argv. That record is not protocol success. A stub cannot be overridden to exit 0.

## Reports

Each run writes `report.json` and `report.md`.

- `job_passed` / `gate_passed`: the owned checks finished.
- `full_acceptance`: always false.
- `hardware_acceptance`: always false. RSS, CPU percent, fd count, and thread count are process counters. `gpu` is null.
- `source.before` / `source.after`: SHA-256 of the named paths only. `frozen` requires those digests to match.
- `build.argv`, `build.features`, and `binaries[].sha256`: the command that ran and the files hashed after it.
- `build_slot`: CLOEXEC metadata and the `flock --close` argv. Present only when `slot` was set. The lock is held through the verified drain. `released` stays false when the drain was not verified. `guardian_pid` is the process that holds the duplicate.
- `display_transport`: display number, filesystem socket, abstract socket, lock pid match, both listening inodes, `inodes_owned_by_server`, and `displayfd_probe: false`. Present after an owned Xvfb proves both namespaces.
- `containment`: `pid-namespace` or `unsupported`, the argv prefix, `kill_child`, `verified_drain`, and `claims_marker_scan_is_sufficient: false`.
- `kill_cause`: `unknown` when the command ends with SIGKILL or exit 137. Absent for a timeout.
- `result_class`: `killed` for that unknown signal, `failed` when checks fail, `ok` when they pass.
- `isolation.host_home_reused`, `host_display_reused`, `host_bus_reused`: false after a successful scrub. `runtime_dir_alone_is_isolation` is false.
- `escaped_children_signaled`: direct marked children this process signaled. It is not proof that the namespace was empty.

Lines that look like secrets are redacted in the markdown report and in log files. Host environment values are not stored.

## Gate and commits

`gate` requires `source_paths`. Declared `build.features` must match `--features` in the argv. Empty features and no flag is allowed. A mismatch is recorded and the compiler is not started. `cargo` and `rustc` are refused unless `allow_heavy` is set, and this lane does not set it for a whole-suite build. Relative `bins` are hashed from `source_root` after the producing command.

`commits check` rejects any `Co-authored-by` trailer on commits after the baseline, and names Claude Code when that identity is present. Existing history is not rewritten.

## Catalog limits

Panel evidence at `/var/tmp/panel-wm-acceptance-final/` is a dev candidate: 21 workarea cases plus 4 short cases (25) and 8 popup cases. The source binding is an author smoke name, not an independent frozen fingerprint. There is no top-level acceptance. X11 candidates only; no physical GPU, input, or Wayland acceptance.

Settings evidence at `/var/tmp/settings-current-stress/` may contain `passed: true` and pinned binary hashes. That is a bounded observation. The historical crash cause stays unconfirmed. Harness corrections recorded there are fixture errors. Peak RSS is not hardware acceptance. Rehash reads the named copies under that directory, not a live bindir. The catalog does not run the stress.

`tests/e2e/settings-preview-stress.py` is read, not executed. `SETTINGS_TAB_SECONDS` switches `theme`, `palettes`, `customise`, and `apps`. A root report of 40 switches is stored with `verified_by_this_lane: false` and `executed: false`. A hash mismatch against `inputs.json` is the expected new script revision. This lane does not call that 40-switch root report verified.

Sprint deadline used by the gate is `2026-10-31T11:36:32+00:00`. Past that instant, `run` and `gate` refuse unless `allow_after_deadline` is set. `sprint` copies deadline, `max_vms`, and the no-coauthor flag. Thread ids in the control file are ignored and are not written out.

Unreviewed markers refused unless `allow_unreviewed` is set: `settings-preview-stress.py`, `settings-current-stress/run.py`, `settings-current-stress/tabs.py`, `panel-wm-acceptance`, `panel-wm-popup`. VMs, `gh`, model CLIs, host power tools, and git `push` / `rebase` / `reset` / `amend` are refused.

## Tests

```bash
python3 -m unittest discover -s tools/desktop-lab/tests -t tools/desktop-lab -v
```

The suite uses fake children, one real private Xvfb smoke, and temp-file flock checks. The smoke reserves one display at or above 700, requires both socket namespaces and both listening inodes on the spawned server, and uses an argv without `-displayfd`. Reservation tests inject busy sets and temp sockets. They do not create or remove live X sockets. A private temp socket and a unique abstract name, plus a lock file that names a different pid, is refused before input and left in place. The suite proves a double-fork, a nested cleared-marker daemon, churn, and SIGTERM followed by SIGKILL die with the namespace while an unrelated sibling stays alive. Coordinator SIGTERM, SIGHUP, and SIGKILL are delivered with a pidfd to the desktop-lab process while a grandchild has called `setsid` and exec'd with an empty environment. The temp lock stays held until that progeny is gone. A patched unverified drain, and a guardian whose drain file says the namespace is occupied, both keep the lock. Missing, truncated, and malformed drain files, and an unrecorded or unknown-start namespace init, keep the lock after coordinator SIGKILL while a separate progeny is still alive. A completed build whose inits are already dead keeps the lock while a later command track is still pending, for both a missing drain and a malformed drain, until that command track itself has a known-start init death. A reused output directory that already holds `containment-build.json` and `containment-command.json` with `namespace_empty` true keeps those files, and a new pending command keeps the lock after coordinator SIGKILL until that command has its own invocation-bound drain or its own known-start init death. A private temp datagram socket in state `01` is excluded from the listener set; the stream listener on that host is kept. Alias tests point at temp stand-ins and do not open the live slot files. It does not run cargo, the settings stress, the panel fixture, or `settings-preview-stress.py`. The plain-flock negative test uses a private temp file and pidfd-kills only the fake daemon it spawned.
