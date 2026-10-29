# Debug logs

Every Decebal program (decsettings, decbar, decdock, decwidgets, decthemectl, ...) writes its own log, so a problem in one window can be
followed without strace or a terminal. Code: `src/dlog.rs`.

```
decthemectl log                  # level, folder and the programs that have a log
decthemectl log tail 50          # last 50 lines of all programs, merged by time
decthemectl log tail 100 decsettings
decthemectl log level debug      # keep debug on (decebal.json {"debug": {"level": "debug"}}); applies to programs started afterwards
decthemectl log clear
DECEBAL_LOG=trace decsettings    # one run only; DECEBAL_LOG_STDERR=1 also prints to the terminal
```

* Files: `~/.local/state/decebal/logs/<program>.log` (`$XDG_STATE_HOME`, or `$DECEBAL_LOG_DIR`), rotated to `.log.1` at 2 MiB. Nothing is
  created until a record passes the level, so a quiet program leaves no file.
* Levels: `off error warn info debug trace`; the default is `info` (start, exit and crash lines, panics, warnings); `info` also records user-visible events (theme applies and undo, settings panes,
  the notices decsettings shows, saved rdwm config); `debug` adds every file write, spawned command and watcher event, and a start record with
  arguments and display variables; `trace` adds each request to rdwm's socket.
* A panic is always recorded with its place and a backtrace (unless the level is `off`), as is a fatal error that ends a program.
* Line format: `2026-10-28 15:20:01.123 INFO  4242 decsettings decebal::settings::app: pane: theme`.
* Logs hold names, paths, counts and results, never clipboard text, passwords or file contents. Review a log before posting it anyway.

For code: `decebal::log_error!`, `log_warn!`, `log_info!`, `log_debug!`, `log_trace!` take `format!` arguments, which are only evaluated when the
level is on. `compat::init()` (the first call in every `main`) sets the level and the panic hook, so a new program gets logging for free.
