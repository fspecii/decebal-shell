# Native Wayland Displays

Settings → Displays uses rdwm output protocol v1 in a Wayland session. It does
not run xrandr against XWayland. Physical-session detection uses either a
nonempty WAYLAND_DISPLAY or XDG_SESSION_TYPE=wayland; forcing this window to
render with X11 or setting the generic X11-tools override does not bypass it. Other compositors, older rdwm builds and missing
IPC report an unavailable explanation instead of exposing controls that cannot
manage the real outputs. The existing X11 xrandr workflow is separate.

Each connector reports its actual mode, refresh rate, logical position, scale and
transform. Controls are gated by the compositor's per-connector capabilities;
nested outputs cannot change the host compositor's physical mode or power state.
Unsupported controls remain disabled with an explanation. Reflection transforms
are shown only when advertised. Primary-output selection and mirroring are not
invented when the backend does not advertise them.

Edits form a local draft. Apply displays sends the complete queried connector
inventory, compositor instance and generation, not a partial set that silently disables omitted
outputs. At least one output must remain enabled. External changes/hot-unplug
retain an edited draft but disable Apply until Reset draft reloads the hardware
state. A compositor restart changes the instance, so a numerically identical
generation cannot silently validate a draft from the previous process. Unrelated Settings drafts and global Xft/DPI preferences are untouched.

A successful Apply displays the compositor's provisional state and reveals the
Keep/Revert banner. Its countdown is derived from the compositor's remaining
time and refreshed asynchronously. The compositor owns the deadline, so closing
or crashing Settings cannot leave an unconfirmed layout permanent. Keep requests
confirmation/persistence through rdwm; it does not separately rewrite output
configuration from the shell. Revert and expiry restore the compositor's prior
supported state. A persistence or backend error is shown; no success is claimed
from merely sending a command. The compositor must atomically merge confirmed
output settings with unrelated configuration, and reject stale/conflicting writes.

All IPC and process waits run on bounded worker jobs. Painting and input dispatch
use immutable cached snapshots. No shell command interpolation, network, polling
process on the UI thread or wallpaper decode is involved. Pending layouts poll
once a second even if the pane is hidden; ordinary display queries keep the
existing slower cadence. A new confirmation scrolls into view once; countdown
refreshes do not continuously reset the user's scroll position. Reordered worker
results cannot replace a newer generation or revive a retired process/token.
A delayed reply never extends the known deadline for the same pending token.

## Wire contract

The existing `rdwmctl raw` transport sends one typed JSON request. Queries use
`{"cmd":"output_query"}` and require `ok`, `protocol:1`, a supported backend,
a required process `instance`, `generation`, global/per-head `capabilities`, complete `outputs` and optional
`pending:{token,remaining_ms}`. An optional successful `warning` reports a committed
layout with uncertain directory-sync durability; it is not described as a rollback.
Modes use exact integer `refresh_mhz`, avoiding
rounded-rate selection. Apply uses `output_apply`, the queried instance and generation, all
`{name,...current}` fields and `timeout_ms:15000`. Keep/Revert use
`output_confirm`/`output_revert` with the returned token. The shell rejects
malformed/oversized inventories and does not fall back to X11 after failure.
Protocol bounds are 32 heads, 256 modes per head, dimensions 1..32768,
refresh 1..1000000 mHz, logical positions ±131072 and advertised scales within
0.25..8. Disabled heads retain meaningful selected mode/transform/scale fields.

Parser, draft, full-inventory/capability/last-output guards, cached UI and actual
worker-thread requests have focused tests under
`settings::system::display::wayland`. Independent native tests must use a private
nested compositor/socket and actual Wayland client/input/configure observation;
a mocked reply alone does not establish output management or presentation.
Hardware DRM disable/enable, hotplug, persistence and rollback acceptance remain
tied to the compositor's independently recorded backend tests.

## Verification checkpoint

The debug integration passed 1,617 default-feature library tests (zero failures,
ten ignored), 25 focused Displays tests, the raw-output-editor guard, formatting
and strict all-target Clippy. Independent private nested Wayland tests exercised
real Settings pixels/input, standard-US Tab/Enter/Escape, staged draft navigation,
actual Apply/Keep/Revert with visible confirmation, Settings SIGKILL followed by
compositor timeout rollback, external-generation rejection/reset and capability
gates. A separate private X11 test retained its legacy output/wallpaper controls.
Evidence: `/tmp/rdwm-output-settings-epoch-final/settings-checks.json` and
`/tmp/rdwm-output-settings-x11-final/`; the independent native fixture lives in
the rdwm repository. These are debug-artifact results, not release/hardware claims.
No live session, compositor, configuration or installed binary was changed for
this verification. Physical DRM mode/power/hotplug testing still requires hardware.
