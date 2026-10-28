//! Test helper for `tests/e2e/dash.sh`: a fake MPRIS player on the session bus.
//!
//! Usage: `fake_mpris [NAME] [TITLE]` registers `org.mpris.MediaPlayer2.NAME` (default `test`) and plays
//! TITLE (default `Fake Song`) by Fake Artist. Every method call it receives is appended to the file named by
//! `FAKE_MPRIS_LOG` (`PlayPause`, `Next`, `Previous`, `SetPosition <us>`, `Volume <v>`); PlayPause toggles the
//! status and is announced with PropertiesChanged like a real player does. `FAKE_MPRIS_ART` names a cover image
//! (a file path) published as `mpris:artUrl`. Built on decebal's own D-Bus client.

use decebal::dbus::{Conn, Message, Value, Writer, CALL};
use std::io::Write;
use std::time::Instant;

const OBJ: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const LENGTH_US: i64 = 200_000_000;

struct State {
    title: String,
    identity: String,
    playing: bool,
    volume: f64,
    started: Instant,
    base_us: i64,
}

impl State {
    fn position(&self) -> i64 {
        let extra = if self.playing { self.started.elapsed().as_micros() as i64 } else { 0 };
        (self.base_us + extra).min(LENGTH_US)
    }
}

type Writ = Box<dyn Fn(&mut Writer)>;

fn entry(w: &mut Writer, key: &str, sig: &str, f: &dyn Fn(&mut Writer)) {
    w.align(8);
    w.string(key);
    w.sig(sig);
    f(w);
}

fn player_props(st: &State) -> Vec<(&'static str, &'static str, Writ)> {
    let (title, playing, volume, pos) = (st.title.clone(), st.playing, st.volume, st.position());
    vec![
        ("PlaybackStatus", "s", Box::new(move |w: &mut Writer| w.string(if playing { "Playing" } else { "Paused" }))),
        ("Volume", "d", Box::new(move |w: &mut Writer| w.f64(volume))),
        ("Position", "x", Box::new(move |w: &mut Writer| w.i64(pos))),
        ("CanControl", "b", Box::new(|w: &mut Writer| w.u32(1))),
        ("CanGoNext", "b", Box::new(|w: &mut Writer| w.u32(1))),
        ("CanGoPrevious", "b", Box::new(|w: &mut Writer| w.u32(1))),
        ("CanSeek", "b", Box::new(|w: &mut Writer| w.u32(1))),
        (
            "Metadata",
            "a{sv}",
            Box::new(move |w: &mut Writer| {
                w.array(8, |w| {
                    entry(w, "mpris:trackid", "o", &|w| w.string("/fake/track/1"));
                    entry(w, "mpris:length", "x", &|w| w.i64(LENGTH_US));
                    entry(w, "xesam:title", "s", &|w| w.string(&title));
                    entry(w, "xesam:artist", "as", &|w| w.array(4, |w| w.string("Fake Artist")));
                    entry(w, "xesam:album", "s", &|w| w.string("Fake Album"));
                    if let Some(art) = std::env::var_os("FAKE_MPRIS_ART") {
                        entry(w, "mpris:artUrl", "s", &|w| w.string(&format!("file://{}", art.to_string_lossy())));
                    }
                })
            }),
        ),
    ]
}

fn root_props(st: &State) -> Vec<(&'static str, &'static str, Writ)> {
    let id = st.identity.clone();
    vec![("Identity", "s", Box::new(move |w: &mut Writer| w.string(&id))), ("CanQuit", "b", Box::new(|w: &mut Writer| w.u32(0)))]
}

fn props_for(st: &State, iface: &str) -> Vec<(&'static str, &'static str, Writ)> {
    match iface {
        PLAYER => player_props(st),
        ROOT => root_props(st),
        _ => Vec::new(),
    }
}

fn log(line: &str) {
    if let Some(p) = std::env::var_os("FAKE_MPRIS_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn changed_signal(conn: &mut Conn, st: &State, names: &[&str]) {
    let mut w = Writer::default();
    w.string(PLAYER);
    w.array(8, |w| {
        for (k, sig, f) in props_for(st, PLAYER).iter().filter(|(k, _, _)| names.contains(k)) {
            entry(w, k, sig, f);
        }
    });
    w.array(4, |_| {});
    let _ = conn.send(&Message::signal(OBJ, PROPS, "PropertiesChanged", "sa{sv}as", w.buf));
}

fn handle(conn: &mut Conn, st: &mut State, m: &Message) {
    let args = m.args().unwrap_or_default();
    let reply = match (m.iface.as_str(), m.member.as_str()) {
        (PROPS, "GetAll") => {
            let iface = args.first().and_then(Value::as_str).unwrap_or("");
            let mut w = Writer::default();
            w.array(8, |w| {
                for (k, sig, f) in props_for(st, iface) {
                    entry(w, k, sig, &*f);
                }
            });
            Some(Message::reply_to(m, "a{sv}", w.buf))
        }
        (PROPS, "Get") => {
            let (iface, name) = (args.first().and_then(Value::as_str).unwrap_or(""), args.get(1).and_then(Value::as_str).unwrap_or(""));
            props_for(st, iface).into_iter().find(|(k, _, _)| *k == name).map(|(_, sig, f)| {
                let mut w = Writer::default();
                w.sig(sig);
                f(&mut w);
                Message::reply_to(m, "v", w.buf)
            })
        }
        (PROPS, "Set") => {
            if let (Some("Volume"), Some(v)) = (args.get(1).and_then(Value::as_str), args.get(2).and_then(Value::as_f64)) {
                st.volume = v;
                log(&format!("Volume {v:.2}"));
                changed_signal(conn, st, &["Volume"]);
            }
            Some(Message::reply_to(m, "", Vec::new()))
        }
        (PLAYER, "PlayPause") => {
            st.base_us = st.position();
            st.started = Instant::now();
            st.playing = !st.playing;
            log("PlayPause");
            changed_signal(conn, st, &["PlaybackStatus"]);
            Some(Message::reply_to(m, "", Vec::new()))
        }
        (PLAYER, "Next" | "Previous") => {
            log(&m.member);
            Some(Message::reply_to(m, "", Vec::new()))
        }
        (PLAYER, "SetPosition") => {
            let pos = args.get(1).and_then(Value::as_int).unwrap_or(0);
            st.base_us = pos;
            st.started = Instant::now();
            log(&format!("SetPosition {pos}"));
            Some(Message::reply_to(m, "", Vec::new()))
        }
        _ => Some(Message::error_to(m, "org.freedesktop.DBus.Error.UnknownMethod", &m.member)),
    };
    if let Some(r) = reply {
        let _ = conn.send(&r);
    }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let name = a.next().unwrap_or_else(|| "test".into());
    let title = a.next().unwrap_or_else(|| "Fake Song".into());
    let mut conn = Conn::session().expect("session bus");
    let bus = format!("org.mpris.MediaPlayer2.{name}");
    if !conn.request_name(&bus, false).expect("RequestName") {
        eprintln!("fake_mpris: could not own {bus}");
        std::process::exit(1);
    }
    conn.set_nonblocking(true).expect("nonblocking");
    let mut st = State { title, identity: format!("Fake {name}"), playing: true, volume: 0.7, started: Instant::now(), base_us: 30_000_000 };
    log("ready");
    loop {
        let mut p = libc::pollfd { fd: conn.fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd, length 1.
        unsafe { libc::poll(&mut p, 1, -1) };
        match conn.fill() {
            Ok(true) => {}
            _ => return,
        }
        while let Ok(Some(m)) = conn.next_message() {
            if m.kind == CALL {
                handle(&mut conn, &mut st, &m);
            }
        }
    }
}
