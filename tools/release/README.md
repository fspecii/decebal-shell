# Release preflight and controlled binary publication

`tools/release/decebal_release.py` (Python stdlib only) checks one sealed frozen-build bundle
(`decebal-frozen-release-manifest/1`, written by the release-engineering lane's `manifest.py`) and plans, publishes and
rolls back its binaries **per file**. It does not build, does not run artifact binaries, sends no signals, starts or
stops nothing, and never writes config. Regression tests: `python3 -I tests/release-preflight.py`.

```sh
T=tools/release/decebal_release.py
A=/var/tmp/decebal-release-engineering-20261031/artifacts/<sha>
python3 -I $T verify $A --expect-commit <sha> [--assets $A.assets]        # exit 0 = ok, 1 = problems listed
python3 -I $T plan $A --dest DIR [--dest DIR ...] [--only NAME ...] --out plan.json   # read-only
python3 -I $T publish --plan plan.json --private-root ROOT --journal NEWDIR --lock FILE  # private ROOT only
python3 -I $T rollback --journal DIR --lock FILE
```

## verify

* `MANIFEST.json.sha256` seals `MANIFEST.json`; top-level keys are exactly the known six (an unknown key such as a
  post-install hook is refused); every sealed record (`result.json`, `source-before/after.json`, `SHA256SUMS`, logs)
  matches its digest and is read-only.
* Provenance: 40-hex commit (optionally `--expect-commit`), clean source, before/after head, tree and production
  fingerprint agree with the manifest, `Cargo.toml`/`Cargo.lock` digests match, `result.json` is a passed clean build of
  that commit, clean locked offline release build record.
* Binaries: names match `^(examples/)?[a-z][a-z0-9_-]{0,63}$`; no symlink in any path component; regular, executable,
  not writable; sha256 and size equal the manifest and `SHA256SUMS`; no extra files under `bin/`; bounds 64 binaries,
  64 MiB each, 1 GiB total; files are hashed through `O_NOFOLLOW` descriptors and refused if they change while hashed.
* `--assets`: the data bundle's own `ASSETS.SHA256SUMS` and its `result.json` commit. It is **not** sealed by
  `MANIFEST.json`; the report says so (`sealed_by_manifest: false`).
* Diagnostics are paths, digests and identities; the build env is reported as variable names only.

## plan (dry run)

For each `--dest` (absolute, no symlink components, owned by you, not group/other writable) and each manifest binary
(default: all except `examples/*`): `create`, `replace` or `unchanged`, with the current file's identity
(dev, ino, size, mode, mtime_ns, nlink, uid, sha256). Symlinks or non-regular entries at a target are refused. Other
entries in the folder are listed under `untouched_entries` and never modified. `config_changes` and `runtime_actions`
are always empty; `config_watch` records sha256 digests (never content) of the config files root's deployment
journals track. `plan_sha256` covers the whole body.

`handoff` is read-only: for the theme-rendering components (decbar, decdock, decwidgets, dechud) it reads the
`$XDG_RUNTIME_DIR/<name>.lock` pid and accepts it as *owned* only when `/proc/<pid>` has your uid and its `exe` is one of
the planned destination paths (stale or reused pids are reported as such). It names each component's single-instance
protocol and how readiness is observed. It takes no locks (a probing `flock` could race a starting instance), matches
no process names, and proposes nothing for `decwm` (never restarted) or `decsettings` (open windows and drafts are never
closed). Theme *data* is reloaded by the running components' `theme::live` watcher; only binary changes need a new
process, and starting one is root's separately authorized step.

## publish (private by default)

**Pinned folders.** Every mutation goes through directory descriptors opened up front: the private root from `/` one
component at a time with `O_NOFOLLOW` (a symlink anywhere in its path is refused), each destination from that root
descriptor the same way (so it is inside the root by construction, not by string comparison), the artifact `bin/`, and
the journal folder. Files are addressed only by single-component names relative to those descriptors. Paths with
empty, `.` or `..` components are refused everywhere (plan entries, journal folder, journal `private_root`).

Refused unless: the plan's keys/schema/actions are known and `plan_sha256` matches; every entry's `path` is exactly
`dest_dir/name` with a flat binary name; no config or runtime change is requested; the bundle still verifies with the same
manifest and per-file digests; the private root is owned, mode 0700, and neither is, contains nor sits inside a live
folder (`~/.local/share/rdwm-{decebal,macshell}/bin`, `~/.local/bin`, `~/.config`, `/usr`, `/etc`, `/opt`); every
destination is below it; no process of yours executes from a destination; the journal folder is new and outside the
destinations; the `--lock` flock is free.

Before the first change every planned file is re-checked against its planned identity and digest, and the watched config
digests are re-checked; any drift ends as `aborted_before_changes` with nothing modified. Then per file:

1. `replace`: require the planned dev/ino, copy to `journal/backups/NNN-name` and verify the copy.
2. Record the staged name, create a hidden `O_EXCL` temp file in the pinned folder, **journal its dev/ino before writing**,
   write the artifact, keep the old mode, fsync, re-read it and require the manifest digest (catches partial writes).
3. Commit: `create` uses `renameat2(RENAME_NOREPLACE)`; `replace` uses `renameat2(RENAME_EXCHANGE)` and checks the
   *displaced* file is the planned inode and digest. If not, the exchange is reversed (the external file stays) and the
   publish rolls back. The displaced original is removed only through a claim (below).
4. Record the committed identity. After all files every target is re-hashed through the pinned folders, and every
   planned path is re-opened from `/` without symlinks: if it no longer leads to the pinned folder (renamed or replaced
   by a symlink during publish) the publish rolls back the pinned folders and reports `destination folder replaced
   during publish`. Nothing is written through the substituted path.

The journal (`journal.json`, schema `decebal-release-journal/1`) is rewritten whole in the pinned journal folder after
every step. It records `dest_dirs` (path → dev/ino of the pinned folder) and per file `path`, `dest_dir`, `name`, and
`staged`/`restore`/`backup` as **names** only.

## Explicit managed runtime publication

`--managed-runtime` is required independently on `plan`, `publish`, and `rollback`. The default private root guards
remain in force. Managed publication accepts **only** the exact owned, symlink-free directories
`~/.local/share/rdwm-decebal/bin` and `~/.local/share/rdwm-macshell/bin`, never their parents or children. It requires
the sealed standard manifest containing the 23 explicitly enumerated runtime binaries and `examples/launchapp`.
The example is never published; `--only` may select a runtime subset. Adding a new runtime name requires a deliberate
tool inventory update. `rdwm`, `rdwmctl`, and all other unselected entries remain untouched.

```sh
python3 -I $T plan $A --managed-runtime --dest "$HOME/.local/share/rdwm-decebal/bin" \
  --dest "$HOME/.local/share/rdwm-macshell/bin" --out managed-plan.json
python3 -I $T publish --managed-runtime --plan managed-plan.json --private-root ROOT \
  --journal ROOT/new-journal --lock ROOT/release.lock
python3 -I $T rollback --managed-runtime --journal ROOT/new-journal --lock ROOT/release.lock
```

The plan must be at most 15 minutes old (UTC), hash intact, and records HOME, destination folder identities and the
standard config watch's file identities plus hashes. Changed config, destination replacement, stale files, unknown
names or a different manifest refuse publication. Managed journal and lock paths must be strictly inside the separately
pinned owned 0700 private root; they cannot be config or live paths. The same held destination descriptors, backups,
atomic exchange, claims, foreign-file preservation and automatic rollback described above apply. Existing executables
may be in use: publication keeps the displaced inode in its original directory until unlink, so a running program's
sibling-directory lookup does not move to the journal. No process is executed, signalled, restarted or closed.

Managed journals require explicit rollback opt-in and revalidate the exact destination allowlist, HOME and recorded
folder identities, private root, journal and lock confinement, runtime names and all auxiliary entry names. A journal's
managed flag alone grants no authority. Existing private journals remain compatible and use the unchanged private
folder validation; `--managed-runtime` refuses a private journal.

## rollback

Runs automatically after any failure inside publish, or later via `rollback --journal`, including on a committed
publish and on a journal left mid-step by a crash. `rollback` refuses before touching anything if: the journal folder
path has `.`/`..`/symlink components; `private_root` is live, loose or reached through a symlink; any recorded folder
does not re-open (symlink-free, from the pinned root) to the **same dev/ino recorded at publish**; any entry's `path` is
not one of those folders + a flat name; or any `staged`/`restore`/`backup` value is not a single name of the expected
form. Files are then handled in reverse order through the pinned descriptors, decided by on-disk identity:

* **Claim before removal.** Nothing is unlinked by its well-known name. To remove a created file, the displaced original
  or a temp file, the tool first `renameat2(RENAME_NOREPLACE)`s it to a fresh random claim name, then checks the claimed
  entry is exactly the expected dev/ino (and digest). Only then is the claim name unlinked. A foreign entry is renamed
  back to its name (or, if that name was taken again meanwhile, preserved under the claim name and reported in the
  journal as `preserved`), and the file is `rollback_conflict`.
* A committed `replace` is checked (inode + digest of what this publish installed), then the verified backup copy (or
  the displaced original still at the staged name) is swapped in with `RENAME_EXCHANGE`; if what came out is not the
  installed inode and digest, the swap is reversed, so a racing replacement stays in place (`rollback_conflict`).
  Restores bring back content, mode and mtime, not the original inode.
* A `staged` entry whose target now has the staged inode was swapped but not journaled and is treated as committed. A
  staged temp whose identity was never journaled (crash between create and journal write) is preserved, not deleted.
* A failed restore (e.g. corrupt backup) is `rollback_failed`; the target keeps the verified new content; the run ends
  `rollback_incomplete` with `unresolved` paths and a non-zero exit. Journal write failures during rollback (ENOSPC)
  do not stop the restore; the last error surfaces; a later `rollback` is idempotent.
* Journals written before this format (no `dest_dirs`, full-path `staged`/`backup`) are refused, not interpreted.

## What is and is not guaranteed

* Each file's switch is one atomic rename in its folder; there is **no** atomicity across files or folders. Between
  the first and last commit (and during rollback) readers can see a mix of old and new binaries.
* **Same-inode writes are an accepted limit, not solved.** Identity checks are dev/ino; a writer that modifies the
  *same inode* in place between a digest check and the following rename/claim is not detected. Digests are re-checked
  at backup, on the displaced file and on claimed entries, which narrows but does not close that window.
* The claim narrows removal races to the random claim name. An actor that can write the destination folder (any process
  of yours) can still list it and swap the entry at the claim name between the check and the unlink.
* Folder pinning protects against renames and symlink substitution of the private root, destinations and journal after
  they are opened. It does not protect against bind mounts or a different user with write access to those folders
  (refused up front: owned, not group/other writable, root 0700).
* Durability depends on fsync of files and directories; a crash between a rename and the following journal write is
  recovered by the identity rules above (fixture-tested by injecting a process death right after the swap), not by the
  journal alone. A crash during a restore can leave a hidden `.<name>.decebal-restore-*` file (its name is journaled).
* Managed publication only replaces binary directory entries. Running processes keep their old executable inode and adopt no new code until a separately initiated process start.

## Exact child helper

`run_exact(path, args, expected_sha, env, timeout)` hashes the file through an open descriptor, refuses on mismatch or
group/other write bits, and executes that same descriptor (`/proc/self/fd/N`), so the verified inode is what runs. The
environment must be an explicit dict (nothing inherited) and only its keys are reported. The child gets a new session;
on timeout only that process group is killed (a descendant that calls `setsid` itself would escape it). The CLI does not
use it yet; it is for root's later ready-protocol probes in a private environment.
