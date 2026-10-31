# Preserved failures of the native evidence runs

Every failing run is kept. None was a product defect that survived; each led to a fix of the harness or the code, and the final run (`../*.log`, all `done:0`) is on the final binaries.

* `attempt1-launcher.log`: the 24 bottom-edge cases passed; the every-edge stage failed at `top`: the diff against the baseline included the taskbar clock (the minute ticked between the two screenshots). Fixed: the clock/tray end of the strip is ignored when measuring the card.
* `attempt1-launcher-expand.log`: "nothing is shown": the harness clicked at (300, 300), outside the card at 100 %, which dismissed the launcher. Fixed: click inside the search field.
* `attempt1-start-click.log`: `IndexError`: the harness cropped the baseline and the shot differently. Fixed.
* `attempt2-*`: the harness's panel-colour assertion sampled the strip where the opened card (or its shadow) was. Fixed: sample the far end of the strip when a card is open.
* `earlier-input-*`: the first input runs: the 200 % `account` click read the recorded-program log before the asynchronous spawn wrote it, then Escape was read before the window unmapped. Fixed: poll for the effect (up to 6 s).
* Code defects the evidence found and fixed along the way: start-menu tabs spilling out of a 768 px card at 200 %; the card too short at 400 x 300 / 200 % (list below the card); the expand button overlapping the stack on a short sidebar; `writer.rs` not lifting `launcher_style` (every builtin package with `[launcher] style` failed the write/re-parse test).
