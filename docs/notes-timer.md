# Notes and Timer

Open **Notes**, **Quick Note**, or **Timer** from Super+Space or your app launcher.
They use the current Decebal palette and native window decorations.

Notes autosaves as you type. Use Tab to move between title and body, Ctrl+N for
another note, Ctrl+S to retry saving, Ctrl+Shift+S to save a conflicting draft as a
copy, and Ctrl+C/X/V for the clipboard. Ctrl+PageUp / Ctrl+PageDown select the
previous / next note in the sidebar and scroll it into view, stopping at the ends.
Selecting another note first saves the
current draft. An external change never gets silently overwritten. A failed save
keeps the draft and prevents an ordinary window close; force-killing a process
can still lose an unsaved draft. Notes are local plaintext, not encrypted or
cloud-synchronised. Files live under `$XDG_DATA_HOME/decebal/notes` (normally
`~/.local/share/decebal/notes`), with private directory/file permissions.

Click **Focus timer** in Notes (or Ctrl+Shift+T) to prepare a countdown for the
current note. The note saves first; a save failure keeps the draft in Notes.
Only its title is passed to Timer, bounded to 120 UTF-8 bytes, never the body.
Choose a duration and press Start when ready. Opening this view does not start,
reset or replace a running/paused countdown. Reset or finish the existing timer
before starting the requested task. Its title then appears in the shared timer
widget and completion notification. Timer's **Notes** button returns to Notes;
both apps have **Widgets…** to open the widget catalog and creator.

Timer accepts seconds or MM:SS, with clickable 5/15/25-minute presets. Click the
duration to replace it, then Start; Pause/Resume and Reset share state with its
widget. Reopening restores its saved duration; a duration change from another
window or widget updates the input, while normal countdown refreshes preserve a
duration you are editing. Reset restores the shared duration. Enter/Space controls
the timer, R resets it, and Escape closes its window.
A single lightweight completion worker stays alive only for an active countdown.
Closing the window does not stop the countdown. Time asleep counts; manual clock
changes affect the remaining time. Desktop notifications require `notify-send`.
A notification failure appears when the app is reopened. Delivery is at most once:
a crash between a durable completion claim and notification delivery can lose the
notification. The packaged XDG login entry runs `dectimer autostart` without
opening a window, including after switching desktop sessions.
It resumes future deadlines or delivers an elapsed, unclaimed timer after login.
Missing, idle, paused and already delivered state exits immediately without a
resident helper. Duplicate login/service/UI starts share the same worker lock.
Corrupt state is left intact and returns a nonzero exit with a stderr diagnostic.
Terminal/service launches retain that diagnostic; an XDG launcher may discard it. An optional
`decebal-timer.service` provides the same recovery under `graphical-session.target`;
it is not enabled by installation and does not restart an idle timer. Install the
login entry (or enable that service) for reboot recovery; a binary-only update does
not itself create an autostart entry.

In **Settings → Widgets**, add **Quick note** and **Timer** from the built-in
catalog. The note card shows a bounded excerpt of the most recently saved note;
click it to open Notes, or New note for a blank draft. Place it only where you are
comfortable displaying private content. The timer card reads the same countdown
as the app. Neither widget needs `allow_exec`. Gallery previews use fictional
sample data and never read private notes. Existing widget layouts remain intact.

CLI: `decnotes [--new]`, `dectimer [--task TITLE]`, `dectimer autostart`, `decwidgets add builtin:quick-note`,
`decwidgets add builtin:timer-card`. Both apps support `--help` and `--version`.

## Widget data snapshot for automation

`decwidgets data --json [--group notes|timer]...` prints the `notes.*` and `timer.*`
values the Quick note and Timer widgets would show, resolved once through the same
data path the widget painter uses. Without `--group`, both groups are included.

- **Snapshot, not live.** It reads storage once and exits. It starts no widget host,
  watcher, worker, sampler or command, and never claims, completes or notifies an
  expired timer.
- **Read-only.** Notes are listed and read, and the timer state file is read.
  Nothing is created, locked, repaired or migrated, and nothing is written to the
  log: this command skips the usual start/exit log line and the one-time
  `macshell`→`decebal` data migration.
- **Private content only on request.** `notes.title` and `notes.excerpt` come from
  your latest note, so ask for `--group timer` alone if you do not want them. The
  output goes only to stdout.
- **Bounded.** Exactly the 8 fields below (fewer with `--group`). Text uses the
  painter's own bounds (note title 38 and excerpt 110 characters, whitespace
  collapsed), with a defensive 256-character cap per value. Values are literal text,
  so `{cpu.usage}` in a note stays as written.
- **Errors.** An unknown group or argument exits with status 1 and a message on
  stderr. Unreadable or unsafe stores report the same fallback values the widgets
  show, such as `Notes unavailable` or `Timer unavailable`, and exit 0.

Schema `decebal-widget-data/1`:

```json
{"schema":"decebal-widget-data/1","snapshot":true,"live":false,"generated_unix":1793440619,
 "groups":["notes","timer"],
 "fields":{"notes.count":2,"notes.title":"A useful thought","notes.excerpt":"…","notes.status":"Saved on this device",
           "timer.remaining":"00:04:59","timer.title":"Focus","timer.status":"Paused","timer.progress":0.003}}
```

| Field | Value |
| --- | --- |
| `notes.count` | Number of readable notes |
| `notes.title`, `notes.excerpt` | The latest note (`Untitled note` when blank; placeholders when there is none) |
| `notes.status` | `Saved on this device`, `Some unreadable notes preserved` or `Notes unavailable` |
| `timer.remaining` | `HH:MM:SS` (`--:--` when unavailable) |
| `timer.title` | The countdown label |
| `timer.status` | `Ready`, `Running`, `Paused`, `Complete` or `Timer unavailable` |
| `timer.progress` | Elapsed fraction, 0 to 1 |

A field that cannot be represented is `null`. Other widget groups (system,
media, weather and so on) are not supported by `--json`, because sampling them
would need background workers. The private fixtures are in
`tests/widget-data-json.py`.
