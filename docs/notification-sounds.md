# Optional notification sounds

Settings → Notifications → **Notification sounds** enables `sounds = true` in
`~/.config/decebal/notifications.toml`. The default is **off**; updating Decebal
never opts you in. Existing comments, unknown keys and muted apps are preserved.

Playback uses optional `canberra-gtk-play` and the installed freedesktop sound
theme (the normal system sound volume). No audio asset is bundled, no network
fetch happens and no package is installed automatically. Missing player/theme,
unsupported audio or playback failure leaves the visual notification working.
`GetCapabilities` includes `sound` only when the player is discoverable. There is
no independent volume slider: this uses the system event-sound route.

The implementation follows the [freedesktop Desktop Notifications Specification,
Hints and Capabilities](https://specifications.freedesktop.org/notification/latest-single/):

- `suppress-sound = true` always wins.
- A valid `sound-name` wins over `sound-file`. That precedence is Decebal policy;
  the specification describes both hints without specifying their precedence.
- Without a hint, the default event is `message-new-instant`.
- Do Not Disturb silences **all** sounds, including critical notifications. The
  existing policy allowing a critical *banner* through DND is unchanged.
- Muted apps and replacement updates are silent. Sounds are arrival feedback;
  history reopening and queued-banner promotion do not replay them.
- Sounds expire after at most 750ms in the worker queue, or the notification's
  shorter lifetime. At most one pending event and one player exist. Attempts are
  limited to one per second; excess events are dropped rather than delayed into
  an audible backlog. Expiry/dismissal does not replay anything.

Names are bounded ASCII event identifiers; arguments never go through a shell.
File hints must be absolute local paths to non-symlink regular WAV/Ogg/FLAC files,
at most 8MiB, with the appropriate header. The worker passes a bounded sealed snapshot
file descriptor to the player, so swapping the pathname cannot redirect playback
to another inode, and later writes to the original cannot grow/change the decoder input. File URIs and remote URLs are not supported. Actual format
validity and sound-theme lookup remain the audio backend's responsibility.

The player worker starts lazily, only after an enabled notification asks for
sound. It blocks while idle. Disabling sounds or enabling DND releases it when
the daemon next processes the configuration/call/UI change. Playback is capped
at 5seconds, and normal daemon return or Player drop kills the owned subprocess group and reaps
the direct player; it never kills a process by name. Kernel filesystem stalls
remain an OS I/O boundary; validation performs only a regular-file metadata and
bounded (at most 8MiB+1) copy on the worker, never in the UI paint loop.

## Verification

`NOTIFY_BINARY=/absolute/decnotify /usr/bin/python3 tests/e2e/notification-sounds.py /tmp/new-evidence`
uses a private Xvfb, session bus, HOME and all XDG paths, an unreachable system bus,
synthetic notifications and a fake player. It never plays sound. It checks the
real bus parser/dispatch, disabled/suppressed/DND/muted/replacement behavior,
name/file precedence and descriptor contents, flood limits, invalid file hints,
player failure recovery, timeout/group cleanup/direct-child reaping and queued
expiry. Unit tests cover hint/deadline validation, file limits, missing backend,
worker drop and Settings preservation/controls. This is not an audible hardware
or sound-theme installation test.
