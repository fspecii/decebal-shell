# Notes and Timer

Open **Notes**, **Quick Note**, or **Timer** from Super+Space or your app launcher.
They use the current Decebal palette and native window decorations.

Notes autosaves as you type. Use Tab to move between title and body, Ctrl+N for
another note, Ctrl+S to retry saving, Ctrl+Shift+S to save a conflicting draft as a
copy, and Ctrl+C/X/V for the clipboard. Selecting another note first saves the
current draft. An external change never gets silently overwritten. A failed save
keeps the draft and prevents an ordinary window close; force-killing a process
can still lose an unsaved draft. Notes are local plaintext, not encrypted or
cloud-synchronised. Files live under `$XDG_DATA_HOME/decebal/notes` (normally
`~/.local/share/decebal/notes`), with private directory/file permissions.

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
notification. A machine restart resumes the saved deadline when Timer is reopened;
there is no login autostart service yet.

In **Settings → Widgets**, add **Quick note** and **Timer** from the built-in
catalog. The note card shows a bounded excerpt of the most recently saved note;
click it to open Notes, or New note for a blank draft. Place it only where you are
comfortable displaying private content. The timer card reads the same countdown
as the app. Neither widget needs `allow_exec`. Gallery previews use fictional
sample data and never read private notes. Existing widget layouts remain intact.

CLI: `decnotes [--new]`, `dectimer`, `decwidgets add builtin:quick-note`,
`decwidgets add builtin:timer-card`. Both apps support `--help` and `--version`.
