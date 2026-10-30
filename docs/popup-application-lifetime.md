# Applications launched from temporary popups

The broad optimized Dux interaction gate found a real lifetime race: Control
Center closed normally on Settings, but its launch was queued on a background
probe thread. The process could exit before that thread spawned the program.
If it did spawn, the probe's parent-death signal and ten-second deadline could
kill a long-lived application. Geometry and the header target were correct.
This is separate from the unreproduced historical Settings preview disappearance.

`sys::spawn_application` now calls the existing detached application launcher
synchronously through spawn/exec before returning. The application has its own
session, no probe parent-death signal and no probe timeout. The launcher's usual
error handling and test dry-run logging remain available. Bounded probes,
watchers and ordinary setters retain their existing cleanup/deadline behavior.

Settings, notification history, network editor and recording launch paths use
the application path. Dux clipboard/history buttons no longer impose a probe's
ten-second lifetime. Appearance changes that intentionally close the popup also
launch their completion commands before it exits. Structured shell arguments
keep executable paths out of interpolated shell source.

## Evidence

- The original exact optimized source55c90da gate failed twice with stable
  geometry and no Settings fixture entry: `/tmp/decebal-review2-native/header-diagnostic`
  and `/tmp/decebal-review2-native/header-lifecycle`.
- A subprocess regression exits the caller immediately; its launched application
  must write a marker200ms later. This passed, as did177menubar tests.
- Fresh repaired debug bar SHA
  `d22a040353815e4f0c4ec0138e66212a96dcee7afbbe100999f620c828ec10ec`
  passed all four100/200light/dark Control Center cases with a deliberately
  delayed Settings fixture. Fixture completion was103–199ms after observed popup
  exit, eliminating the fast-fixture race. Evidence:
  `/tmp/decebal-popup-launch-after-matrix`.
- An actual private Settings instance (optimized SHA
  `e2cf1aaf39f4ba48913a3b44485c8f368df7477e99a24f7b120746626ef49135`)
  mapped and stayed alive for two seconds after its repaired-debug popup exited.
  Evidence: `/tmp/decebal-popup-launch-real-settings-final/real-settings.json`.

Final optimized acceptance is still required. These checks use private displays,
configuration and buses, never live user input, audio or settings. They do not
establish the cause of the earlier preview report.
