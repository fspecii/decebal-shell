//! One muted player per output. Pausing keeps its last frame, without further decoding.
use super::{covered, Config};
use crate::settings::io::{self, Paths};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

pub fn status_path() -> PathBuf {
    crate::single::runtime_file("decwall-status.json")
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Output {
    id: u64,
    name: String,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}
struct Player {
    child: Child,
    socket: PathBuf,
    output: Output,
    paused: Option<bool>,
    #[cfg_attr(not(feature = "x11"), allow(dead_code))]
    window: u32,
    dead: bool,
}
impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}
fn pause(player: &mut Player, value: bool) -> Result<(), String> {
    if player.paused == Some(value) {
        return Ok(());
    }
    let mut socket = UnixStream::connect(&player.socket).map_err(|e| e.to_string())?;
    socket.set_write_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
    writeln!(socket, "{}", json!({"command":["set_property","pause",value]})).map_err(|e| e.to_string())?;
    player.paused = Some(value);
    Ok(())
}
fn is_wayland() -> bool {
    match std::env::var("DECEBAL_BACKEND").ok().as_deref() {
        Some("wayland") => true,
        Some("x11") => false,
        _ => std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") || std::env::var_os("WAYLAND_DISPLAY").is_some(),
    }
}
fn state_outputs(state: &Value) -> Vec<Output> {
    state["monitors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let r = &m["rect"];
            Some(Output { id: m["id"].as_u64()?, name: m["name"].as_str()?.into(), x: r["x"].as_i64()? as i32, y: r["y"].as_i64()? as i32, w: r["w"].as_u64()? as u32, h: r["h"].as_u64()? as u32 })
        })
        .filter(|o| o.w > 0 && o.h > 0 && !o.name.is_empty())
        .collect()
}
fn start(cfg: &Config, o: &Output, window: u32, wayland: bool) -> Result<Player, String> {
    let socket = crate::single::runtime_file(&format!("decwall-{}-{}.sock", std::process::id(), o.id));
    let _ = std::fs::remove_file(&socket);
    let options = vec![
        "config=no".to_string(),
        "load-scripts=no".into(),
        "ytdl=no".into(),
        "audio=no".into(),
        "sub=no".into(),
        "loop-file=inf".into(),
        "pause=yes".into(),
        "hwdec=auto-safe".into(),
        "panscan=1".into(),
        "osc=no".into(),
        "osd-level=0".into(),
        "input-default-bindings=no".into(),
        "input-vo-keyboard=no".into(),
        "stop-screensaver=no".into(),
        "cache=no".into(),
        "demuxer-max-bytes=8388608".into(),
        "demuxer-max-back-bytes=0".into(),
        "vd-lavc-threads=2".into(),
        format!("vf=lavfi=[fps={}]", cfg.fps.clamp(1, 60)),
        format!("input-ipc-server={}", socket.display()),
    ];
    let mut cmd = if wayland {
        let mut c = Command::new("mpvpaper");
        c.arg("-o").arg(options.join(" ")).arg(&o.name).arg(&cfg.video);
        c
    } else {
        let mut c = Command::new("mpv");
        c.args(options.iter().map(|s| format!("--{s}"))).arg("--vo=xv,x11,gpu").arg(format!("--wid={window}")).arg("--").arg(&cfg.video);
        c
    };
    use std::os::unix::process::CommandExt;
    // The player is disposable; it must not survive its host or inherit blocked SIGTERM.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            libc::setpriority(libc::PRIO_PROCESS, 0, 10);
            Ok(())
        });
    }
    let child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("{}: {e}", if wayland { "mpvpaper" } else { "mpv" }))?;
    Ok(Player { child, socket, output: o.clone(), paused: None, window, dead: false })
}

#[cfg(feature = "x11")]
mod x11 {
    use super::*;
    use x11rb::{
        connection::Connection,
        protocol::{randr::ConnectionExt as _, shape, xproto::*},
        rust_connection::RustConnection,
        wrapper::ConnectionExt as _,
    };
    pub struct Desktop {
        pub conn: RustConnection,
        root: u32,
    }
    fn e<T: std::fmt::Display>(err: T) -> String {
        err.to_string()
    }
    impl Desktop {
        pub fn new() -> Result<Self, String> {
            let (conn, n) = x11rb::connect(None).map_err(e)?;
            let root = conn.setup().roots[n].root;
            Ok(Self { conn, root })
        }
        pub fn outputs(&self, state: &Value) -> Result<Vec<Output>, String> {
            let monitors = self.conn.randr_get_monitors(self.root, true).map_err(e)?.reply().map_err(e)?;
            let mut outputs = Vec::new();
            for (i, m) in monitors.monitors.iter().enumerate() {
                let name = self.conn.get_atom_name(m.name).map_err(e)?.reply().map_err(e)?.name;
                // Older rdwm exposes workareas only. Match physical RandR monitors by overlap.
                let matched = state["monitors"].as_array().and_then(|ms| {
                    ms.iter().max_by_key(|s| {
                        let r = &s["workarea"];
                        let (x, y, w, h) = (r["x"].as_i64().unwrap_or(0), r["y"].as_i64().unwrap_or(0), r["w"].as_i64().unwrap_or(0), r["h"].as_i64().unwrap_or(0));
                        ((x + w).min(i64::from(m.x) + i64::from(m.width)) - x.max(i64::from(m.x))).max(0) * ((y + h).min(i64::from(m.y) + i64::from(m.height)) - y.max(i64::from(m.y))).max(0)
                    })
                });
                outputs.push(Output {
                    id: matched.and_then(|s| s["id"].as_u64()).unwrap_or(i as u64),
                    name: String::from_utf8_lossy(&name).into_owned(),
                    x: i32::from(m.x),
                    y: i32::from(m.y),
                    w: u32::from(m.width),
                    h: u32::from(m.height),
                });
            }
            Ok(outputs)
        }
        pub fn create(&self, o: &Output) -> Result<u32, String> {
            let id = self.conn.generate_id().map_err(e)?;
            self.conn
                .create_window(
                    x11rb::COPY_DEPTH_FROM_PARENT,
                    id,
                    self.root,
                    o.x as i16,
                    o.y as i16,
                    o.w as u16,
                    o.h as u16,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &CreateWindowAux::new().override_redirect(1).background_pixel(0),
                )
                .map_err(e)?
                .check()
                .map_err(e)?;
            self.conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_CLASS, AtomEnum::STRING, b"decwall\0Decwall\0").map_err(e)?;
            let kind = self.conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE").map_err(e)?.reply().map_err(e)?.atom;
            let desktop = self.conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DESKTOP").map_err(e)?.reply().map_err(e)?.atom;
            self.conn.change_property32(PropMode::REPLACE, id, kind, AtomEnum::ATOM, &[desktop]).map_err(e)?;
            self.conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, b"Decebal live wallpaper").map_err(e)?;
            // Empty input shape passes clicks through to the root, including through embedded children.
            shape::rectangles(&self.conn, shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, id, 0, 0, &[]).map_err(e)?.check().map_err(e)?;
            self.conn.map_window(id).map_err(e)?;
            self.conn.configure_window(id, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW)).map_err(e)?;
            self.conn.flush().map_err(e)?;
            Ok(id)
        }
        pub fn maintain(&self, players: &[Player]) -> Result<(), String> {
            while self.conn.poll_for_event().map_err(e)?.is_some() {}
            for p in players.iter().filter(|p| !p.dead) {
                self.conn.configure_window(p.window, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW)).map_err(e)?;
            }
            self.conn.flush().map_err(e)
        }
        pub fn destroy(&self, id: u32) {
            let _ = self.conn.destroy_window(id);
            let _ = self.conn.flush();
        }
    }
}

pub fn run() -> Result<(), String> {
    if !Config::load().enabled {
        return Ok(());
    }
    let Some(_guard) = crate::single::hold("decwall") else { return Ok(()) };
    crate::single::block_signals(&[libc::SIGTERM, libc::SIGINT]);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::spawn(move || {
        crate::single::wait_signal(&[libc::SIGTERM, libc::SIGINT]);
        stopped.store(true, Ordering::Relaxed);
    });
    let wayland = is_wayland();
    #[cfg(feature = "x11")]
    let desktop = if !wayland { Some(x11::Desktop::new()?) } else { None };
    #[cfg(not(feature = "x11"))]
    if !wayland {
        return Err("This build has no X11 support".into());
    }
    let paths = Paths::discover();
    let mut players: Vec<Player> = Vec::new();
    let mut previous = None;
    let mut errors = Vec::<String>::new();
    let mut tick = 0u32;
    let mut stable_since = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let mut cfg = Config::load();
        if previous.as_ref().is_some_and(|(old, _)| old != &cfg) {
            stable_since = Instant::now();
        }
        let state = io::ipc_call(&paths.socket, &json!({"cmd":"get_state"})).ok().map(|s| s["state"].clone());
        // Static selection or a theme wallpaper supersedes the video, including after login.
        let desired = std::fs::read_to_string(&paths.rdwm_config).ok().and_then(|s| Cfg::parse(&s).ok()).map(|c| match c.get("wallpaper", "image") {
            Some(Val::Str(s)) => Some(PathBuf::from(crate::settings::panes::expand_tilde(&s))),
            _ => None,
        });
        if cfg.enabled
            && (desired.as_ref().is_some_and(|p| p.as_ref() != Some(&cfg.poster))
                || (stable_since.elapsed() > Duration::from_secs(2)
                    && state.as_ref().filter(|s| s["wallpaper"].is_object()).is_some_and(|s| s["wallpaper"]["image"].as_str().map(std::path::Path::new) != Some(cfg.poster.as_path()))))
        {
            cfg.enabled = false;
            cfg.save()?;
        }
        if !cfg.enabled {
            break;
        }
        let current = state.as_ref().unwrap_or(&Value::Null);
        let outputs = state_outputs(current);
        #[cfg(feature = "x11")]
        let outputs = if let Some(d) = &desktop { d.outputs(current)? } else { outputs };
        if previous.as_ref() != Some(&(cfg.clone(), outputs.clone())) {
            #[cfg(feature = "x11")]
            if let Some(d) = &desktop {
                for p in &players {
                    d.destroy(p.window);
                }
            }
            players.clear();
            errors.clear();
            stable_since = Instant::now();
            if cfg.enabled {
                if outputs.is_empty() {
                    errors.push("No outputs reported by rdwm; update rdwm for Wayland output names".into());
                }
                for o in &outputs {
                    #[cfg(feature = "x11")]
                    let window = if let Some(d) = &desktop { d.create(o)? } else { 0 };
                    #[cfg(not(feature = "x11"))]
                    let window = 0;
                    match start(&cfg, o, window, wayland) {
                        Ok(p) => players.push(p),
                        Err(e) => {
                            #[cfg(feature = "x11")]
                            if let Some(d) = &desktop {
                                d.destroy(window);
                            }
                            errors.push(e);
                        }
                    }
                }
            }
            previous = Some((cfg.clone(), outputs));
        }
        for p in &mut players {
            if p.dead {
                continue;
            }
            if p.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                errors.push(format!("Player exited on {}; check mpv/mpvpaper installation. Still wallpaper remains", p.output.name));
                #[cfg(feature = "x11")]
                if let Some(d) = &desktop {
                    d.destroy(p.window);
                }
                p.dead = true;
                p.paused = None;
                continue;
            }
            let paused = state.is_none() || (cfg.pause_fullscreen && covered(current, p.output.id));
            let _ = pause(p, paused); // IPC becomes ready asynchronously; retried on next tick.
        }
        #[cfg(feature = "x11")]
        if let Some(d) = &desktop {
            d.maintain(&players)?;
        }
        if tick % 2 == 0 {
            let status = json!({"enabled":cfg.enabled,"backend":if wayland {"wayland"}else{"x11"},"players":players.iter().map(|p|json!({"output":p.output.name,"pid":p.child.id(),"paused":p.paused,"alive":!p.dead})).collect::<Vec<_>>(),"errors":errors});
            let text = serde_json::to_string_pretty(&status).map_err(|e| e.to_string())?;
            // Runtime state is disposable; avoid config backups on each status update.
            let part = status_path().with_extension("tmp");
            std::fs::write(&part, text).map_err(|e| e.to_string())?;
            std::fs::rename(part, status_path()).map_err(|e| e.to_string())?;
        }
        tick = tick.wrapping_add(1);
        std::thread::sleep(Duration::from_millis(500));
    }
    drop(players);
    let _ = std::fs::remove_file(status_path());
    Ok(())
}
use crate::settings::tomlcfg::{Cfg, Val};
