//! Sound: output/input devices, default selection, volume + mute, per-app streams.
//! Backend: `pactl` (JSON output, stable names) when present, else `wpctl` (WirePlumber ids).

use super::run::{Cmd, Runner};
use super::{Action, Dom, Hub};
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Row};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Pactl,
    Wpctl,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dev {
    /// pactl: device name; wpctl: node id.
    pub id: String,
    pub desc: String,
    /// 0.0 .. 1.5 (1.0 = 100 %).
    pub vol: f32,
    pub mute: bool,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub backend: Backend,
    pub outputs: Vec<Dev>,
    pub inputs: Vec<Dev>,
    pub streams: Vec<Dev>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------- parsers

fn pct(s: &str) -> Option<f32> {
    s.trim().trim_end_matches('%').parse::<f32>().ok().map(|v| v / 100.0)
}

fn json_vol(v: &Value) -> f32 {
    let Some(o) = v.as_object() else { return 0.0 };
    let vals: Vec<f32> = o.values().filter_map(|c| c["value_percent"].as_str().and_then(pct)).collect();
    if vals.is_empty() {
        0.0
    } else {
        vals.iter().sum::<f32>() / vals.len() as f32
    }
}

/// `pactl -f json list sinks|sources`. Monitor sources are skipped.
pub fn parse_pactl_devices(json: &str, default: &str) -> Result<Vec<Dev>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("unexpected pactl output: {e}"))?;
    let arr = v.as_array().ok_or("unexpected pactl output")?;
    let mut out = Vec::new();
    for d in arr {
        let name = d["name"].as_str().unwrap_or("");
        if name.is_empty() || name.ends_with(".monitor") || d["properties"]["device.class"].as_str() == Some("monitor") {
            continue;
        }
        let desc = d["description"].as_str().filter(|s| !s.is_empty()).unwrap_or(name);
        out.push(Dev { id: name.into(), desc: desc.into(), vol: json_vol(&d["volume"]), mute: d["mute"].as_bool().unwrap_or(false), default: name == default.trim() });
    }
    Ok(out)
}

/// `pactl -f json list sink-inputs`: one entry per playing app.
pub fn parse_pactl_streams(json: &str) -> Result<Vec<Dev>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("unexpected pactl output: {e}"))?;
    let arr = v.as_array().ok_or("unexpected pactl output")?;
    let mut out = Vec::new();
    for d in arr {
        let idx = d["index"].as_u64().map(|i| i.to_string()).unwrap_or_default();
        if idx.is_empty() {
            continue;
        }
        let p = &d["properties"];
        let app = p["application.name"].as_str().or(p["media.name"].as_str()).or(p["application.process.binary"].as_str()).unwrap_or("Unknown app");
        out.push(Dev { id: idx, desc: app.into(), vol: json_vol(&d["volume"]), mute: d["mute"].as_bool().unwrap_or(false), default: false });
    }
    Ok(out)
}

/// `wpctl status`: sinks, sources and streams of the Audio section (volumes for streams are read separately).
pub fn parse_wpctl_status(text: &str) -> State {
    let mut st = State { backend: Backend::Wpctl, outputs: vec![], inputs: vec![], streams: vec![], error: None };
    let mut in_audio = false;
    let mut sect = "";
    for line in text.lines() {
        let plain = line.trim_start_matches(|c: char| c == ' ' || "│├└─".contains(c));
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_audio = line.trim() == "Audio";
            sect = "";
            continue;
        }
        if !in_audio {
            continue;
        }
        match plain.trim() {
            "Sinks:" => sect = "sinks",
            "Sources:" => sect = "sources",
            "Streams:" => sect = "streams",
            "Devices:" | "Filters:" => sect = "",
            _ => {
                if sect.is_empty() {
                    continue;
                }
                let (default, rest) = match plain.strip_prefix('*') {
                    Some(r) => (true, r.trim_start()),
                    None => (false, plain.trim_start()),
                };
                let Some((num, name)) = rest.split_once(". ") else { continue };
                if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let mut name = name.trim().to_string();
                let (mut vol, mut mute) = (0.0, false);
                if let Some(i) = name.rfind("[vol:") {
                    let tail = name[i + 5..].trim_end_matches(']').to_string();
                    mute = tail.contains("MUTED");
                    vol = tail.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    name = name[..i].trim().to_string();
                }
                if sect == "streams" && (name.contains(" > ") || name.contains(" < ") || name.starts_with("output_") || name.starts_with("input_")) {
                    continue; // port lines under a stream
                }
                let dev = Dev { id: num.into(), desc: name, vol, mute, default };
                match sect {
                    "sinks" => st.outputs.push(dev),
                    "sources" => st.inputs.push(dev),
                    _ => st.streams.push(dev),
                }
            }
        }
    }
    st
}

/// `wpctl get-volume ID` -> (volume, muted).
pub fn parse_wpctl_volume(text: &str) -> Option<(f32, bool)> {
    let rest = text.trim().strip_prefix("Volume:")?;
    let v = rest.split_whitespace().next()?.parse().ok()?;
    Some((v, rest.contains("MUTED")))
}

// ---------------------------------------------------------------- loading

pub fn load(r: &dyn Runner) -> State {
    let none = |e: &str| State { backend: Backend::None, outputs: vec![], inputs: vec![], streams: vec![], error: Some(e.into()) };
    if r.exists("pactl") {
        let sinks = r.run(&Cmd::new("pactl", &["-f", "json", "list", "sinks"]));
        if sinks.ok {
            let dsink = r.run(&Cmd::new("pactl", &["get-default-sink"])).stdout;
            let dsrc = r.run(&Cmd::new("pactl", &["get-default-source"])).stdout;
            let sources = r.run(&Cmd::new("pactl", &["-f", "json", "list", "sources"]));
            let inputs = r.run(&Cmd::new("pactl", &["-f", "json", "list", "sink-inputs"]));
            let mut st = State { backend: Backend::Pactl, outputs: vec![], inputs: vec![], streams: vec![], error: None };
            match parse_pactl_devices(&sinks.stdout, &dsink) {
                Ok(v) => st.outputs = v,
                Err(e) => st.error = Some(e),
            }
            if sources.ok {
                st.inputs = parse_pactl_devices(&sources.stdout, &dsrc).unwrap_or_default();
            }
            if inputs.ok {
                st.streams = parse_pactl_streams(&inputs.stdout).unwrap_or_default();
            }
            return st;
        }
        if !r.exists("wpctl") {
            return none(&format!("The sound server did not answer: {}", sinks.error_text("pactl")));
        }
    }
    if r.exists("wpctl") {
        let o = r.run(&Cmd::new("wpctl", &["status"]));
        if !o.ok {
            return none(&format!("The sound server did not answer: {}", o.error_text("wpctl")));
        }
        let mut st = parse_wpctl_status(&o.stdout);
        for d in st.streams.iter_mut() {
            if let Some((v, m)) = parse_wpctl_volume(&r.run(&Cmd::new("wpctl", &["get-volume", &d.id])).stdout) {
                d.vol = v;
                d.mute = m;
            }
        }
        return st;
    }
    none("Neither pactl nor wpctl is installed, so sound cannot be controlled from here.")
}

// ---------------------------------------------------------------- setters (command builders)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind3 {
    Out,
    In,
    Stream,
}

pub fn set_volume(b: Backend, k: Kind3, id: &str, vol: f32) -> Result<Vec<Cmd>, String> {
    if !vol.is_finite() || !(0.0..=1.5).contains(&vol) {
        return Err("Volume must be between 0 and 150 %".into());
    }
    if id.is_empty() {
        return Err("No device selected".into());
    }
    Ok(vec![match (b, k) {
        (Backend::Pactl, k) => {
            let sub = match k {
                Kind3::Out => "set-sink-volume",
                Kind3::In => "set-source-volume",
                Kind3::Stream => "set-sink-input-volume",
            };
            Cmd::owned("pactl", vec![sub.into(), id.into(), format!("{}%", (vol * 100.0).round() as u32)])
        }
        (Backend::Wpctl, _) => Cmd::owned("wpctl", vec!["set-volume".into(), id.into(), format!("{vol:.2}")]),
        (Backend::None, _) => return Err("No sound backend available".into()),
    }])
}

pub fn set_mute(b: Backend, k: Kind3, id: &str, mute: bool) -> Result<Vec<Cmd>, String> {
    if id.is_empty() {
        return Err("No device selected".into());
    }
    let m = if mute { "1" } else { "0" };
    Ok(vec![match (b, k) {
        (Backend::Pactl, k) => {
            let sub = match k {
                Kind3::Out => "set-sink-mute",
                Kind3::In => "set-source-mute",
                Kind3::Stream => "set-sink-input-mute",
            };
            Cmd::new("pactl", &[sub, id, m])
        }
        (Backend::Wpctl, _) => Cmd::new("wpctl", &["set-mute", id, m]),
        (Backend::None, _) => return Err("No sound backend available".into()),
    }])
}

pub fn set_default(b: Backend, k: Kind3, id: &str) -> Result<Vec<Cmd>, String> {
    if id.is_empty() {
        return Err("No device selected".into());
    }
    Ok(vec![match (b, k) {
        (Backend::Pactl, Kind3::Out) => Cmd::new("pactl", &["set-default-sink", id]),
        (Backend::Pactl, Kind3::In) => Cmd::new("pactl", &["set-default-source", id]),
        (Backend::Wpctl, Kind3::Out | Kind3::In) => Cmd::new("wpctl", &["set-default", id]),
        _ => return Err("Streams have no default".into()),
    }])
}

// ---------------------------------------------------------------- pane

fn find<'a>(st: &'a State, id: &str) -> Option<(&'a Dev, Kind3)> {
    st.outputs.iter().map(|d| (d, Kind3::Out)).chain(st.inputs.iter().map(|d| (d, Kind3::In))).chain(st.streams.iter().map(|d| (d, Kind3::Stream))).find(|(d, k)| {
        let _ = k;
        d.id == id
    })
}

fn dev_row(prefix: &str, d: &Dev, label: String, detail: &str) -> Row {
    Row::new(label)
        .detail(detail)
        .ctl(Ctl::new(format!("s:snd:vol{prefix}:{}", d.id), Kind::Slider { v: (d.vol * 100.0).round() as f64, min: 0.0, max: 100.0, step: 1.0, unit: "%".into() }))
        .ctl(Ctl::new(format!("s:snd:mute{prefix}:{}", d.id), Kind::Chip { label: "Mute".into(), on: d.mute }))
}

pub fn unavailable(what: &str, why: &str) -> Vec<Item> {
    vec![Item::Header(what.into()), Item::Card(vec![Row::new("Not available").detail(why)])]
}

pub fn items(h: &Hub) -> Vec<Item> {
    let Some(st) = &h.sound.data else { return vec![Item::Note { text: "Reading sound devices...".into(), warn: false }] };
    if st.backend == Backend::None {
        return unavailable("Sound", st.error.as_deref().unwrap_or("No sound server was found."));
    }
    let mut v = Vec::new();
    if let Some(e) = &st.error {
        v.push(Item::Note { text: e.clone(), warn: true });
    }
    let section = |title: &str, empty: &str, id: &str, p: &str, devs: &[Dev]| -> Vec<Item> {
        let mut out = vec![Item::Header(title.into())];
        if devs.is_empty() {
            out.push(Item::Card(vec![Row::new(empty)]));
            return out;
        }
        let opts: Vec<(String, String)> = devs.iter().map(|d| (d.desc.clone(), d.id.clone())).collect();
        let sel = devs.iter().position(|d| d.default);
        let mut rows = vec![Row::new(if id == "out" { "Output device" } else { "Input device" })
            .detail("Used by apps that do not choose their own")
            .ctl(Ctl::new(format!("s:snd:default{p}:{id}"), Kind::Popup(opts, sel)))];
        for d in devs {
            rows.push(dev_row(p, d, d.desc.clone(), if d.default { "Default" } else { "" }));
        }
        out.push(Item::Card(rows));
        out
    };
    v.extend(section("Output", "No output devices", "out", "", &st.outputs));
    v.extend(section("Input", "No input devices", "in", "-in", &st.inputs));
    v.push(Item::Header("Applications".into()));
    if st.streams.is_empty() {
        v.push(Item::Card(vec![Row::new("No application is playing sound")]));
    } else {
        v.push(Item::Card(st.streams.iter().map(|d| dev_row("-app", d, d.desc.clone(), "")).collect()));
    }
    v
}

pub fn apply(h: &mut Hub, act: &str, arg: &str, v: CtlVal) {
    let Some(st) = h.sound.data.clone() else { return };
    let (k, base) = match act {
        a if a.ends_with("-in") => (Kind3::In, a.trim_end_matches("-in")),
        a if a.ends_with("-app") => (Kind3::Stream, a.trim_end_matches("-app")),
        a => (Kind3::Out, a),
    };
    let b = st.backend;
    let res: Result<Action, String> = (|| match (base, &v) {
        ("vol", CtlVal::Num(n)) => {
            let (d, _) = find(&st, arg).ok_or("That device is gone")?;
            let cmds = set_volume(b, k, arg, (*n as f32 / 100.0).clamp(0.0, 1.5))?;
            let undo = set_volume(b, k, arg, d.vol)?;
            Ok(Action::new(Dom::Sound, format!("{} volume", d.desc), cmds).undo(undo))
        }
        ("mute", CtlVal::Click) | ("mute", CtlVal::Bool(_)) => {
            let (d, _) = find(&st, arg).ok_or("That device is gone")?;
            let to = !d.mute;
            let cmds = set_mute(b, k, arg, to)?;
            let undo = set_mute(b, k, arg, d.mute)?;
            Ok(Action::new(Dom::Sound, format!("{} {}", if to { "Mute" } else { "Unmute" }, d.desc), cmds).undo(undo))
        }
        ("default", CtlVal::Str(id)) => {
            let old = if k == Kind3::In { &st.inputs } else { &st.outputs }.iter().find(|d| d.default).map(|d| d.id.clone());
            let cmds = set_default(b, k, id)?;
            let undo = match old {
                Some(o) => set_default(b, k, &o)?,
                None => vec![],
            };
            Ok(Action::new(Dom::Sound, "Default device", cmds).undo(undo))
        }
        _ => Err("Unsupported sound change".into()),
    })();
    match res {
        Ok(a) => {
            // optimistic update so sliders do not jump back while the command runs
            if let Some(s) = h.sound.data.as_mut() {
                for d in s.outputs.iter_mut().chain(s.inputs.iter_mut()).chain(s.streams.iter_mut()) {
                    if d.id == arg {
                        match (base, &v) {
                            ("vol", CtlVal::Num(n)) => d.vol = *n as f32 / 100.0,
                            ("mute", _) => d.mute = !d.mute,
                            _ => {}
                        }
                    }
                }
            }
            h.run(a);
        }
        Err(e) => h.say(e, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::FakeRunner;
    use crate::settings::system::{exec_action, Hub};
    use std::sync::Arc;

    const SINKS: &str = r#"[{"index":134,"state":"SUSPENDED","name":"alsa_output.usb-HyperX.analog-stereo","description":"HyperX Cloud II","mute":false,"volume":{"front-left":{"value":32768,"value_percent":"50%"},"front-right":{"value":32768,"value_percent":"50%"}},"properties":{"device.class":"sound"}},
{"index":135,"name":"alsa_output.hdmi-stereo","description":"HDMI Monitor","mute":true,"volume":{"mono":{"value":26214,"value_percent":"40%"}},"properties":{}}]"#;
    const SOURCES: &str = r#"[{"index":1,"name":"alsa_output.usb.analog-stereo.monitor","description":"Monitor of HyperX","mute":false,"volume":{},"properties":{"device.class":"monitor"}},
{"index":2,"name":"alsa_input.usb-mic","description":"HyperX Mic","mute":false,"volume":{"mono":{"value_percent":"69%"}},"properties":{}}]"#;
    const INPUTS: &str = r#"[{"index":77,"mute":false,"volume":{"front-left":{"value_percent":"100%"}},"properties":{"application.name":"Firefox","media.name":"Playback"}},
{"index":78,"mute":true,"volume":{"front-left":{"value_percent":"30%"}},"properties":{"media.name":"track.ogg"}}]"#;

    const WPCTL: &str = "PipeWire 'pipewire-0' [1.6.9, a@b, cookie:1]\n └─ Clients:\n        32. WirePlumber                         [1.6.9, a@b, pid:1167]\n\nAudio\n ├─ Devices:\n │      48. Starship [alsa]\n │  \n ├─ Sinks:\n │  *   56. HyperX Cloud II Core Wireless Analog Stereo [vol: 0.50]\n │      63. GA106 HDMI (HDMI) [CU34G2XP] [vol: 0.40 MUTED]\n │  \n ├─ Sources:\n │  *   54. HyperX Mono  [vol: 0.69]\n │  \n ├─ Filters:\n │  \n └─ Streams:\n        80. Firefox\n             102. output_FL  > alsa_output:playback_FL   [active]\n             103. output_FR  > alsa_output:playback_FR   [active]\n\nVideo\n ├─ Devices:\n │  \n ├─ Sinks:\n │      99. Should Not Appear [vol: 1.0]\n";

    #[test]
    fn pactl_sinks_default_mute_and_average_volume() {
        let v = parse_pactl_devices(SINKS, "alsa_output.usb-HyperX.analog-stereo\n").unwrap();
        assert_eq!(v.len(), 2);
        assert!(v[0].default && !v[1].default);
        assert_eq!(v[0].vol, 0.5);
        assert!(v[1].mute);
        assert!((v[1].vol - 0.4).abs() < 1e-6);
        assert_eq!(v[0].desc, "HyperX Cloud II");
    }

    #[test]
    fn pactl_sources_skip_monitors() {
        let v = parse_pactl_devices(SOURCES, "").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "alsa_input.usb-mic");
    }

    #[test]
    fn pactl_streams_use_app_name_then_media_name() {
        let v = parse_pactl_streams(INPUTS).unwrap();
        assert_eq!(v[0].desc, "Firefox");
        assert_eq!(v[0].id, "77");
        assert_eq!(v[1].desc, "track.ogg");
        assert!(v[1].mute);
        assert!(parse_pactl_streams("not json").is_err());
        assert_eq!(parse_pactl_streams("[]").unwrap().len(), 0);
    }

    #[test]
    fn wpctl_status_sections() {
        let st = parse_wpctl_status(WPCTL);
        assert_eq!(st.outputs.len(), 2);
        assert_eq!(st.outputs[0].id, "56");
        assert!(st.outputs[0].default);
        assert_eq!(st.outputs[0].vol, 0.5);
        assert!(st.outputs[1].mute);
        assert_eq!(st.outputs[1].desc, "GA106 HDMI (HDMI) [CU34G2XP]");
        assert_eq!(st.inputs.len(), 1);
        assert_eq!(st.inputs[0].desc, "HyperX Mono");
        assert_eq!(st.streams.len(), 1);
        assert_eq!(st.streams[0].desc, "Firefox");
        assert_eq!(st.streams[0].id, "80");
    }

    #[test]
    fn wpctl_volume_line() {
        assert_eq!(parse_wpctl_volume("Volume: 0.50\n"), Some((0.5, false)));
        assert_eq!(parse_wpctl_volume("Volume: 0.40 [MUTED]"), Some((0.4, true)));
        assert_eq!(parse_wpctl_volume("garbage"), None);
    }

    #[test]
    fn setters_build_the_right_command_lines() {
        let l = |r: Result<Vec<Cmd>, String>| r.unwrap().iter().map(|c| c.line()).collect::<Vec<_>>();
        assert_eq!(l(set_volume(Backend::Pactl, Kind3::Out, "sinkA", 0.5)), ["pactl set-sink-volume sinkA 50%"]);
        assert_eq!(l(set_volume(Backend::Pactl, Kind3::In, "srcA", 0.333)), ["pactl set-source-volume srcA 33%"]);
        assert_eq!(l(set_volume(Backend::Pactl, Kind3::Stream, "77", 1.0)), ["pactl set-sink-input-volume 77 100%"]);
        assert_eq!(l(set_volume(Backend::Wpctl, Kind3::Out, "56", 0.4)), ["wpctl set-volume 56 0.40"]);
        assert_eq!(l(set_mute(Backend::Pactl, Kind3::Out, "s", true)), ["pactl set-sink-mute s 1"]);
        assert_eq!(l(set_mute(Backend::Wpctl, Kind3::In, "54", false)), ["wpctl set-mute 54 0"]);
        assert_eq!(l(set_default(Backend::Pactl, Kind3::Out, "s")), ["pactl set-default-sink s"]);
        assert_eq!(l(set_default(Backend::Pactl, Kind3::In, "s")), ["pactl set-default-source s"]);
        assert_eq!(l(set_default(Backend::Wpctl, Kind3::Out, "56")), ["wpctl set-default 56"]);
    }

    #[test]
    fn setters_validate() {
        assert!(set_volume(Backend::Pactl, Kind3::Out, "s", 2.0).is_err());
        assert!(set_volume(Backend::Pactl, Kind3::Out, "s", f32::NAN).is_err());
        assert!(set_volume(Backend::Pactl, Kind3::Out, "", 0.5).is_err());
        assert!(set_volume(Backend::None, Kind3::Out, "s", 0.5).is_err());
        assert!(set_default(Backend::Pactl, Kind3::Stream, "1").is_err());
    }

    #[test]
    fn load_prefers_pactl_and_degrades() {
        let r = FakeRunner::new()
            .ok("pactl -f json list sinks", SINKS)
            .ok("pactl get-default-sink", "alsa_output.hdmi-stereo\n")
            .ok("pactl get-default-source", "alsa_input.usb-mic\n")
            .ok("pactl -f json list sources", SOURCES)
            .ok("pactl -f json list sink-inputs", INPUTS);
        let st = load(&r);
        assert_eq!(st.backend, Backend::Pactl);
        assert!(st.outputs[1].default && st.inputs[0].default);
        assert_eq!(st.streams.len(), 2);
        // nothing installed
        let st = load(&FakeRunner::new());
        assert_eq!(st.backend, Backend::None);
        assert!(st.error.unwrap().contains("Neither"));
        // wpctl only
        let r = FakeRunner::new().ok("wpctl status", WPCTL).ok("wpctl get-volume 80", "Volume: 0.75\n");
        let st = load(&r);
        assert_eq!(st.backend, Backend::Wpctl);
        assert_eq!(st.streams[0].vol, 0.75);
        // server down
        let r = FakeRunner::new().on("pactl -f json list sinks", crate::settings::system::run::Out::fail("Connection refused"));
        assert!(load(&r).error.unwrap().contains("did not answer"));
    }

    fn hub_with(st: State) -> (Hub, Arc<FakeRunner>) {
        let fr = Arc::new(FakeRunner::lenient());
        let mut h = Hub::with(fr.clone(), std::env::temp_dir().join("ms-snd-test"), Arc::new(|| {}));
        h.sound.data = Some(st);
        (h, fr)
    }

    #[test]
    fn apply_builds_actions_with_undo() {
        let st = State { backend: Backend::Pactl, outputs: parse_pactl_devices(SINKS, "alsa_output.usb-HyperX.analog-stereo").unwrap(), inputs: vec![], streams: vec![], error: None };
        let (mut h, fr) = hub_with(st);
        sound_apply(&mut h, "vol", "alsa_output.usb-HyperX.analog-stereo", CtlVal::Num(80.0));
        // wait for the worker
        for _ in 0..100 {
            if !fr.lines().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(fr.lines(), ["pactl set-sink-volume alsa_output.usb-HyperX.analog-stereo 80%"]);
        assert_eq!(h.sound.data.as_ref().unwrap().outputs[0].vol, 0.8);
        // the action carries the undo
        let a = Action::new(Dom::Sound, "x", vec![]).undo(set_volume(Backend::Pactl, Kind3::Out, "s", 0.5).unwrap());
        let d = exec_action(&FakeRunner::lenient(), &std::env::temp_dir(), &a);
        assert_eq!(d.undo.unwrap().cmds[0].line(), "pactl set-sink-volume s 50%");
    }

    fn sound_apply(h: &mut Hub, act: &str, arg: &str, v: CtlVal) {
        apply(h, act, arg, v)
    }

    #[test]
    fn unknown_device_is_an_error_not_a_command() {
        let st = State { backend: Backend::Pactl, outputs: vec![], inputs: vec![], streams: vec![], error: None };
        let (mut h, fr) = hub_with(st);
        apply(&mut h, "vol", "ghost", CtlVal::Num(10.0));
        assert!(fr.lines().is_empty());
        assert_eq!(h.msgs.len(), 1);
        assert!(h.msgs[0].1);
    }

    #[test]
    fn failing_command_reports_stderr() {
        let a = Action::new(Dom::Sound, "Volume", vec![Cmd::new("pactl", &["set-sink-volume", "s", "5%"])]);
        let r = FakeRunner::new().on("pactl set-sink-volume s 5%", crate::settings::system::run::Out::fail("Failure: No such entity"));
        let d = exec_action(&r, &std::env::temp_dir(), &a);
        assert_eq!(d.msg, Some(("Volume: Failure: No such entity".into(), true)));
        assert!(d.undo.is_none());
    }

    #[test]
    fn items_for_each_state() {
        let (h, _) = hub_with(State { backend: Backend::None, outputs: vec![], inputs: vec![], streams: vec![], error: Some("nope".into()) });
        let it = items(&h);
        assert!(matches!(&it[1], Item::Card(r) if r[0].label == "Not available"));
        let st = State { backend: Backend::Pactl, outputs: parse_pactl_devices(SINKS, "alsa_output.hdmi-stereo").unwrap(), inputs: vec![], streams: parse_pactl_streams(INPUTS).unwrap(), error: None };
        let (h, _) = hub_with(st);
        let it = items(&h);
        let ids: Vec<String> = it.iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().flat_map(|r| r.ctls.iter().map(|c| c.id.clone())).collect();
        assert!(ids.contains(&"s:snd:vol:alsa_output.hdmi-stereo".to_string()));
        assert!(ids.contains(&"s:snd:mute-app:77".to_string()));
        assert!(ids.contains(&"s:snd:default:out".to_string()));
    }
}
