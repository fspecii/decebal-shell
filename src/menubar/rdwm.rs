//! rdwm integration for the bar's layout switcher: reads the focused monitor's layout from rdwm's
//! IPC socket (`$XDG_RUNTIME_DIR/rdwm.sock`, JSON lines) and sends layout actions to it.
//!
//! A single watcher thread blocks on an event subscription (no polling while rdwm is up); while
//! rdwm is not running it only looks for the socket every few seconds. Without rdwm (Hyprland, ...)
//! the state stays `None` and the bar hides the item.

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Built-in layouts: (ipc name, menu title, bar symbol).
pub const LAYOUTS: &[(&str, &str, &str)] = &[
    ("tile", "Tile", "[]="),
    ("rtile", "Tile, master right", "=[]"),
    ("monocle", "Monocle", "[M]"),
    ("floating", "Floating", "><>"),
    ("bottomstack", "Bottom stack", "TTT"),
    ("bstackhoriz", "Bottom stack, bands", "==="),
    ("grid", "Grid", "HHH"),
    ("gaplessgrid", "Gapless grid", ":::"),
    ("horizgrid", "Two rows", "---"),
    ("nrowgrid", "N-row grid", "###"),
    ("deck", "Deck", "[D]"),
    ("centeredmaster", "Centered master", "|M|"),
    ("centeredfloatingmaster", "Floating master", ">M>"),
    ("dwindle", "Dwindle", "[\\]"),
    ("spiral", "Spiral", "[@]"),
    ("tatami", "Tatami", "|+|"),
    ("columns", "Columns", "|||"),
    ("rows", "Rows", "=-="),
    ("threecol", "Three columns", "[||]"),
    ("bsp", "BSP", "[+]"),
    ("scroll", "Scrolling strip", "[<>]"),
    ("tabbed", "Tabbed", "[T]"),
    ("stacked", "Stacked", "[S]"),
];

/// Title for an ipc layout name (aliases and unknown plugin layouts map to themselves).
pub fn title_of(name: &str) -> String {
    let canon = match name {
        "bstack" => "bottomstack",
        "col" => "columns",
        "scrolling" => "scroll",
        n => n,
    };
    LAYOUTS.iter().find(|l| l.0 == canon).map(|l| l.1.to_string()).unwrap_or_else(|| name.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutState {
    pub name: String,
    pub symbol: String,
}

impl LayoutState {
    /// Canonical built-in name (`bstack` -> `bottomstack`), for the checkmark.
    pub fn canonical(&self) -> &str {
        match self.name.as_str() {
            "bstack" => "bottomstack",
            "col" => "columns",
            "scrolling" => "scroll",
            n => n,
        }
    }
}

pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty()) {
        Some(d) => PathBuf::from(d).join("rdwm.sock"),
        None => PathBuf::from(format!("/tmp/rdwm-{}/rdwm.sock", unsafe { libc::getuid() })),
    }
}

/// The selected monitor's layout from a `get_state` reply.
pub fn parse_state(line: &str) -> Option<LayoutState> {
    let v: Value = serde_json::from_str(line).ok()?;
    let st = v.get("state")?;
    let mons = st.get("monitors")?.as_array()?;
    let sel = st.get("selected_mon").and_then(Value::as_i64).unwrap_or(0);
    let m = mons.iter().find(|m| m.get("id").and_then(Value::as_i64) == Some(sel)).or_else(|| mons.first())?;
    Some(LayoutState { name: m.get("layout")?.as_str()?.to_string(), symbol: m.get("symbol").and_then(Value::as_str).unwrap_or("").to_string() })
}

/// JSON line for an rdwm action.
pub fn action_request(action: &str, args: &[&str]) -> String {
    serde_json::json!({ "action": action, "args": args }).to_string()
}

/// Sends one request and returns the first reply line. Bounded by short timeouts: never hangs.
pub fn request(line: &str) -> Option<String> {
    let mut s = UnixStream::connect(socket_path()).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.set_write_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(line.as_bytes()).ok()?;
    s.write_all(b"\n").ok()?;
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).ok()?;
    Some(out)
}

/// Fire-and-forget action on a worker thread (the UI thread never touches the socket).
pub fn send_action(action: &str, args: &[&str]) {
    if super::super::controlcenter::sys::is_dry_run() {
        super::super::controlcenter::sys::spawn(&["rdwmctl", "action", action].iter().copied().chain(args.iter().copied()).collect::<Vec<_>>());
        return;
    }
    let line = action_request(action, args);
    std::thread::spawn(move || {
        let _ = request(&line);
    });
}

pub type Shared = Arc<Mutex<Option<LayoutState>>>;

/// Starts the watcher. `on_change` runs on the watcher thread whenever the shown state changed.
pub fn spawn_watcher<F: Fn() + Send + 'static>(shared: Shared, on_change: F) {
    std::thread::spawn(move || {
        let set = |v: Option<LayoutState>| {
            let mut g = shared.lock().unwrap_or_else(|p| p.into_inner());
            if *g != v {
                *g = v;
                drop(g);
                on_change();
            }
        };
        loop {
            let path = socket_path();
            if !path.exists() {
                set(None);
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
            let Ok(mut s) = UnixStream::connect(&path) else {
                set(None);
                std::thread::sleep(Duration::from_secs(3));
                continue;
            };
            set(request(r#"{"cmd":"get_state"}"#).and_then(|l| parse_state(&l)));
            if s.write_all(b"{\"cmd\":\"subscribe\",\"events\":[\"layout\",\"tag_view\",\"focus\"]}\n").is_err() {
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
            // Blocks until rdwm sends something; EOF means rdwm went away.
            for line in BufReader::new(&mut s).lines() {
                let Ok(line) = line else { break };
                if line.contains("\"event\"") {
                    set(request(r#"{"cmd":"get_state"}"#).and_then(|l| parse_state(&l)));
                }
            }
            set(None);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// Mini wireframe of a layout as cells in the unit square (x, y, w, h), for the menu icons.
pub fn layout_cells(name: &str) -> Vec<(f32, f32, f32, f32)> {
    let g = 0.09;
    let col = |n: usize, x0: f32, w: f32| -> Vec<(f32, f32, f32, f32)> {
        let h = (1.0 - g * (n as f32 - 1.0)) / n as f32;
        (0..n).map(|i| (x0, i as f32 * (h + g), w, h)).collect()
    };
    let row = |n: usize, y0: f32, h: f32| -> Vec<(f32, f32, f32, f32)> {
        let w = (1.0 - g * (n as f32 - 1.0)) / n as f32;
        (0..n).map(|i| (i as f32 * (w + g), y0, w, h)).collect()
    };
    let mut v;
    match name {
        "monocle" => v = vec![(0.0, 0.0, 1.0, 1.0)],
        "floating" => v = vec![(0.0, 0.0, 0.62, 0.6), (0.38, 0.4, 0.62, 0.6)],
        "tile" | "deck" => {
            v = vec![(0.0, 0.0, 0.55, 1.0)];
            v.extend(if name == "deck" { vec![(0.61, 0.0, 0.39, 1.0)] } else { col(2, 0.61, 0.39) });
        }
        "rtile" => {
            v = vec![(0.45, 0.0, 0.55, 1.0)];
            v.extend(col(2, 0.0, 0.39));
        }
        "bottomstack" => {
            v = vec![(0.0, 0.0, 1.0, 0.55)];
            v.extend(row(3, 0.61, 0.39));
        }
        "bstackhoriz" => {
            v = vec![(0.0, 0.0, 1.0, 0.43)];
            v.extend([(0.0, 0.49, 1.0, 0.24), (0.0, 0.79, 1.0, 0.21)]);
        }
        "grid" | "gaplessgrid" => {
            v = row(2, 0.0, 0.47);
            v.extend(row(if name == "grid" { 2 } else { 3 }, 0.53, 0.47));
        }
        "horizgrid" => {
            v = row(2, 0.0, 0.47);
            v.extend(row(3, 0.53, 0.47));
        }
        "nrowgrid" => {
            v = Vec::new();
            for i in 0..3 {
                v.extend(row(2, i as f32 * 0.35, 0.3));
            }
        }
        "centeredmaster" => {
            v = col(2, 0.0, 0.26);
            v.push((0.32, 0.0, 0.36, 1.0));
            v.extend(col(2, 0.74, 0.26));
        }
        "centeredfloatingmaster" => v = vec![(0.0, 0.0, 1.0, 1.0), (0.2, 0.15, 0.6, 0.7)],
        "dwindle" | "spiral" | "bsp" => {
            v = vec![(0.0, 0.0, 0.5, 1.0), (0.56, 0.0, 0.44, 0.5), (0.56, 0.56, 0.22, 0.44), (0.84, 0.56, 0.16, 0.44)];
            if name == "spiral" {
                v[2] = (0.78, 0.56, 0.22, 0.44);
                v[3] = (0.56, 0.56, 0.16, 0.44);
            }
        }
        "tatami" => v = vec![(0.0, 0.0, 0.5, 0.47), (0.56, 0.0, 0.44, 0.47), (0.0, 0.53, 0.3, 0.47), (0.36, 0.53, 0.28, 0.47), (0.7, 0.53, 0.3, 0.47)],
        "columns" => v = row(3, 0.0, 1.0),
        "rows" => {
            v = Vec::new();
            for i in 0..3 {
                v.push((0.0, i as f32 * 0.35, 1.0, 0.3));
            }
        }
        "threecol" => {
            v = vec![(0.0, 0.0, 0.4, 1.0)];
            v.extend(col(2, 0.46, 0.26));
            v.extend(col(2, 0.78, 0.22));
        }
        "scroll" => v = vec![(-0.1, 0.0, 0.3, 1.0), (0.26, 0.0, 0.48, 1.0), (0.8, 0.0, 0.3, 1.0)],
        "tabbed" => {
            v = row(3, 0.0, 0.18);
            v.push((0.0, 0.24, 1.0, 0.76));
        }
        "stacked" => v = vec![(0.0, 0.0, 1.0, 0.16), (0.0, 0.22, 1.0, 0.16), (0.0, 0.44, 1.0, 0.56)],
        _ => v = vec![(0.0, 0.0, 0.55, 1.0), (0.61, 0.0, 0.39, 1.0)],
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = r#"{"ok":true,"state":{"focused":null,"monitors":[{"id":0,"layout":"tile","symbol":"[]="},{"id":1,"layout":"bsp","symbol":"[+]"}],"selected_mon":1}}"#;

    #[test]
    fn state_uses_the_selected_monitor() {
        assert_eq!(parse_state(STATE), Some(LayoutState { name: "bsp".into(), symbol: "[+]".into() }));
        let first = r#"{"ok":true,"state":{"monitors":[{"id":0,"layout":"grid","symbol":"HHH"}]}}"#;
        assert_eq!(parse_state(first).map(|s| s.name), Some("grid".into()));
    }

    #[test]
    fn bad_state_is_none() {
        assert_eq!(parse_state(""), None);
        assert_eq!(parse_state(r#"{"ok":false,"error":"x"}"#), None);
        assert_eq!(parse_state(r#"{"ok":true,"state":{"monitors":[]}}"#), None);
    }

    #[test]
    fn action_json_matches_the_ipc_syntax() {
        assert_eq!(action_request("setlayout", &["bsp"]), r#"{"action":"setlayout","args":["bsp"]}"#);
        assert_eq!(action_request("togglegaps", &[]), r#"{"action":"togglegaps","args":[]}"#);
    }

    #[test]
    fn every_layout_has_a_unique_name_and_cells_inside_the_unit_box() {
        let mut seen = std::collections::HashSet::new();
        for (name, title, sym) in LAYOUTS {
            assert!(seen.insert(*name) && !title.is_empty() && !sym.is_empty());
            let cells = layout_cells(name);
            assert!(!cells.is_empty(), "{name}");
            if *name != "scroll" {
                for (x, y, w, h) in cells {
                    assert!(x >= -0.001 && y >= -0.001 && x + w <= 1.001 && y + h <= 1.001 && w > 0.0 && h > 0.0, "{name} cell out of box");
                }
            }
        }
        assert_eq!(LAYOUTS.len(), 23);
    }

    #[test]
    fn aliases_resolve_to_the_checked_entry() {
        let s = LayoutState { name: "bstack".into(), symbol: "TTT".into() };
        assert_eq!(s.canonical(), "bottomstack");
        assert_eq!(title_of("bstack"), "Bottom stack");
        assert_eq!(title_of("myplugin"), "myplugin");
    }

    #[test]
    fn watcher_reads_state_from_a_fake_socket() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("macbar-rdwm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("rdwm.sock");
        let l = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut r = BufReader::new(c.try_clone().unwrap());
                    let mut line = String::new();
                    while r.read_line(&mut line).unwrap_or(0) > 0 {
                        let mut w = &c;
                        if line.contains("get_state") {
                            let _ = writeln!(w, "{STATE}");
                        } else {
                            let _ = writeln!(w, r#"{{"ok":true}}"#);
                        }
                        line.clear();
                    }
                });
            }
        });
        // SAFETY of env mutation: only this test touches XDG_RUNTIME_DIR in this module.
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        assert_eq!(request(r#"{"cmd":"get_state"}"#).and_then(|l| parse_state(&l)).map(|s| s.name), Some("bsp".into()));
        let shared: Shared = Arc::new(Mutex::new(None));
        spawn_watcher(shared.clone(), || {});
        for _ in 0..50 {
            if shared.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        assert_eq!(shared.lock().unwrap().as_ref().map(|s| s.symbol.clone()), Some("[+]".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
