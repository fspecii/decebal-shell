# Kickoff Places: asynchronous delivery through the shared launcher host

Status: root wired the shared hook; 36 focused tests passed, with 2 existing ignores. Formatting and strict all-target lint pass after root test-only cleanup. Native launcher and package acceptance remain pending.

## What Kickoff's Places tab does now (all in `src/launcher/styled/kde/places.rs` and `kde.rs`)

* Building the launcher and painting read nothing. The first time the Places tab opens, **one** worker thread reads `user-dirs.dirs`, the places, removable
  media and `recently-used.xbel`. The interface never waits: the tab shows at once what needs no file system (Show KRunner, Home, the applications of the
  history) and a "Reading places..." note.
* Single flight: while a worker has not finished, even one stuck in a stalled mount or a blocked `stat`, `places::request` starts no second one, however often
  the tab is opened and closed. Thread count is therefore at most one for the launcher's lifetime, plus one more only after an earlier one finished.
* Leaving the tab (`TAB 0`) calls `places::cancel()`: the worker checks the flag between steps and returns early (a system call that never returns cannot be
  interrupted; it simply stays the one worker), and an answer produced after a cancel is discarded. Coming back before the worker finished keeps its answer.
* A worker panic is caught: the flight ends, nothing is delivered, the next opening may start one new worker.
* The answer lands only in a thread-local store in `places.rs`. Pages, queries, selections and zones are always derived from the current state at layout
  time (`computer_rows()` / `document_rows()` read the store), so a late answer cannot be applied to a stale page. The store is taken in by `places::poll()`,
  which `layout()` runs (layout follows every input event) and `kde::tick()` runs for the host.
* Reads are bounded before anything is allocated: regular files only (a FIFO is never opened), `user-dirs.dirs` at most 64 KiB, `recently-used.xbel` at most
  512 KiB from each end, 400 bookmarks, 120 existence checks, 40 documents, 16 devices; private items (`<bookmark:private/>`) are skipped; entities, percent
  encoding, calendar fields and timestamp arithmetic are validated or checked.

## Shared event-loop hook (now wired)

`Launcher::set_waker` now forwards the callback through `Styled` to the Places store. `Launcher::tick` asks the style to harvest completed work and redraws it without another input event. The regression uses channels to finish a held worker, receives its wake-up, then verifies the shared style tick changes its visible rows and requests exactly one redraw.

The implemented hook is:

```rust
// src/launcher/styled.rs
impl Styled {
    /// The host's waker: a style may call it from a worker thread when something it started has finished.
    pub fn set_waker(&mut self, waker: crate::window_app::Waker) {
        if self.style == LauncherStyle::Kde {
            kde::set_waker(waker);
        }
    }

    /// Host tick after the waker fired: takes finished background answers in.
    pub fn tick(&mut self) -> Response {
        if self.style == LauncherStyle::Kde && kde::tick() {
            self.relayout();
            return Response::REDRAW;
        }
        Response::NONE
    }
}

// src/launcher.rs, impl WindowApp for Launcher
fn set_waker(&mut self, waker: Waker) {
    if let Some(st) = self.styled.as_mut() {
        st.set_waker(waker.clone());
    }
    crate::theme::live::start_watcher(move || waker());
}

fn tick(&mut self) -> Response {
    if let Some(st) = self.styled.as_mut() {
        let answered = st.tick();
        if crate::theme::take_changed() {
            st.relayout();
            return Response::REDRAW;
        }
        return answered;
    }
    /* unchanged below */
}
```

Other styles (Deepin, UKUI) can use the same two entry points for their own workers; a style that has none returns `Nothing` / `NONE`.

## Tests and evidence

`kde/places.rs`: one thread however many `request`/`cancel` cycles; the waker handshake and the answer landing in the store only; cancel stops the worker and
drops its answer, then exactly one new worker may start; coming back keeps the answer; a panicking worker frees the flight; a reader is never asked for more than
the cap; 256 MiB sparse `user-dirs.dirs` and history files; FIFOs skipped; stop requests end the reads between steps; hostile XBEL (60 MB, junk, entities,
private items, malformed and oversized bookmarks, 50 000 distinct paths); timestamp validation and random-input no-panic.
`kde/tests.rs` (real controls): opening Places with a stuck worker returns at once and shows Show KRunner and Home; sidebar, footer and keyboard keep working and
Enter opens Home while the rest is unread; 300 open/close cycles add no thread; a late answer changes the store and not the page, the query, the focus or the
zones; leaving Places discards the answer and the next opening starts one new worker. All handshakes use channels (the worker is released by the test, the
waker signals back); timeouts exist only to turn a hang into a failure.


Root evidence: `/var/tmp/decebal-kde-places-root-gate-v3/result.json` (36 passed, 2 ignored; the subsequent lint phase found a redundant closure) and `/var/tmp/decebal-kde-places-root-lint-final-v2/result.json` (strict all-target lint and formatting pass at `9aaff89`). Earlier failed fixture assertions and lint logs are retained. Root's first new wake-up assertion incorrectly assumed loaded data preceded the fixed Show KRunner row; it was corrected to verify delivery without changing that product order. No native capture, installation or full Plasma-package acceptance is implied. The shared heavy-job pause was restored after each run.
