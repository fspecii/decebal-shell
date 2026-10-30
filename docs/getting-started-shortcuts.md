# Your shortcuts in Getting Started

Getting Started uses the existing Settings background watcher's effective rdwm
`bind list` snapshot. That list already includes the running configuration,
defaults and runtime registrations. The guide does not merge unapplied file
edits into a running list. It performs no IPC, filesystem access or command
execution while creating or painting rows.

Recognized window actions and direct application argv bindings appear with their
actual modifiers, sequences and named modes. A simple `sh -c 'exec decspot'` can
be identified, but arbitrary shell scripts remain **Custom launch commands**.
The guide never evaluates a script or assumes a mention of an app means it runs.
It does not promise an installed executable or infer what environment variables
and external scripts resolve to. **Not identified** therefore does not mean
unbound: the complete Shortcuts pane remains the place to inspect custom scripts.

Disconnected rdwm, unavailable effective-list support and an authoritative empty
list have distinct states. Disconnection hides stale cached keys. Duplicate key
labels are removed; at most three per action are shown with a count pointing to
Shortcuts for the rest. Existing row-to-wrapped-note rendering keeps sequences
readable at enlarged text sizes. Welcome routing and default General startup are
unchanged. No binding or configuration is changed by the guide.
