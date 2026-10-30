//! rdwm output protocol v1. Commands run only on Hub workers; paint consumes snapshots.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Capabilities {
    pub position: bool,
    pub scale: bool,
    pub transform: bool,
    pub mode: bool,
    pub enabled: bool,
    pub persist: bool,
    pub scale_min: f64,
    pub scale_max: f64,
    pub transforms: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputMode {
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Current {
    pub enabled: bool,
    pub mode: OutputMode,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Desired {
    pub name: String,
    #[serde(flatten)]
    pub current: Current,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Head {
    pub name: String,
    pub current: Current,
    pub modes: Vec<OutputMode>,
    pub capabilities: Capabilities,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pending {
    pub token: String,
    pub remaining_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Snapshot {
    pub protocol: u32,
    pub instance: String,
    pub backend: String,
    pub generation: u64,
    pub capabilities: Capabilities,
    pub outputs: Vec<Head>,
    pub pending: Option<Pending>,
    pub warning: Option<String>,
    #[serde(skip, default = "Instant::now")]
    pub received: Instant,
}
#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub generation: Option<u64>,
    pub instance: Option<String>,
    observed: Option<(String, u64)>,
    latest_request: Option<Instant>,
    retired_instances: Vec<String>,
    completed_tokens: Vec<String>,
    deadline: Option<(String, Instant)>,
    pub outputs: Vec<Desired>,
    pub dirty: bool,
    pub busy: bool,
}
impl Draft {
    /// Reordered worker results cannot restore stale state or extend a known deadline.
    pub fn accept(&mut self, s: &mut Snapshot) -> bool {
        if self.retired_instances.contains(&s.instance) || s.pending.as_ref().is_some_and(|p| self.completed_tokens.contains(&p.token)) {
            return false;
        }
        if let Some((epoch, generation)) = &self.observed {
            if epoch == &s.instance && (s.generation < *generation || (s.generation == *generation && self.latest_request.is_some_and(|at| s.received < at))) {
                return false;
            }
            if epoch != &s.instance {
                // A first-ever reply from the old process can also arrive late;
                // it may not yet have an epoch tombstone. Never let an older
                // request switch us away from the newly observed process.
                if self.latest_request.is_some_and(|at| s.received < at) {
                    return false;
                }
                self.retired_instances.push(epoch.clone());
                // Only one query and one action can be in flight (both bounded 6s).
                if self.retired_instances.len() > 32 {
                    self.retired_instances.remove(0);
                }
                self.deadline = None;
            }
        }
        if let Some((token, _)) = &self.deadline {
            if s.pending.as_ref().is_none_or(|p| &p.token != token) {
                self.completed_tokens.push(token.clone());
                if self.completed_tokens.len() > 32 {
                    self.completed_tokens.remove(0);
                }
            }
        }
        if let Some(p) = &mut s.pending {
            let proposed = s.received + Duration::from_millis(p.remaining_ms);
            let deadline = self.deadline.as_ref().filter(|(token, _)| token == &p.token).map(|(_, old)| (*old).min(proposed)).unwrap_or(proposed);
            p.remaining_ms = deadline.saturating_duration_since(s.received).as_millis().min(u64::MAX as u128) as u64;
            self.deadline = Some((p.token.clone(), deadline));
        } else {
            self.deadline = None;
        }
        self.latest_request = Some(self.latest_request.map(|at| at.max(s.received)).unwrap_or(s.received));
        self.observed = Some((s.instance.clone(), s.generation));
        true
    }
    pub fn accept_failure(&mut self, started: Instant) -> bool {
        if self.latest_request.is_some_and(|at| started < at) {
            return false;
        }
        self.latest_request = Some(started);
        true
    }
    pub fn reset(&mut self, s: &Snapshot) {
        self.generation = Some(s.generation);
        self.instance = Some(s.instance.clone());
        self.outputs = s.outputs.iter().map(|h| Desired { name: h.name.clone(), current: h.current.clone() }).collect();
        self.dirty = false;
    }
    pub fn receive(&mut self, s: &Snapshot) {
        if !self.dirty && !self.busy {
            self.reset(s);
        }
    }
}
fn checked_mode(m: &OutputMode) -> bool {
    (1..=32768).contains(&m.width) && (1..=32768).contains(&m.height) && (1..=1_000_000).contains(&m.refresh_mhz)
}
fn checked_caps(c: &Capabilities) -> bool {
    c.scale_min.is_finite()
        && c.scale_max.is_finite()
        && c.scale_min >= 0.25
        && c.scale_max <= 8.0
        && c.scale_min <= c.scale_max
        && c.transforms.len() <= 8
        && c.transforms.iter().all(|s| matches!(s.as_str(), "normal" | "90" | "180" | "270" | "flipped" | "flipped-90" | "flipped-180" | "flipped-270"))
}
pub fn parse(text: &str) -> Result<Snapshot, String> {
    if text.len() > 262144 {
        return Err("Display reply exceeds the safety limit".into());
    }
    let v: Value = serde_json::from_str(text).map_err(|_| "Invalid compositor display reply")?;
    if v["ok"] != true {
        return Err(v["error"].as_str().unwrap_or("Compositor does not support output management").chars().filter(|c| !c.is_control()).take(200).collect());
    }
    let mut s: Snapshot = serde_json::from_value(v).map_err(|_| "Incomplete compositor display capabilities")?;
    if s.instance.is_empty()
        || s.instance.len() > 128
        || s.instance.chars().any(char::is_control)
        || s.protocol != 1
        || !matches!(s.backend.as_str(), "nested-pixman" | "nested-gles" | "drm-pixman" | "drm-gles")
        || !checked_caps(&s.capabilities)
        || s.outputs.is_empty()
        || s.outputs.len() > 32
    {
        return Err("Unsupported compositor display protocol or capabilities".into());
    }
    let mut names = std::collections::HashSet::new();
    for h in &s.outputs {
        if h.name.is_empty()
            || h.name.len() > 128
            || h.name.chars().any(char::is_control)
            || !names.insert(&h.name)
            || h.modes.len() > 256
            || !h.modes.iter().all(checked_mode)
            || !checked_mode(&h.current.mode)
            || !checked_caps(&h.capabilities)
            || !h.current.scale.is_finite()
            || !(h.capabilities.scale_min..=h.capabilities.scale_max).contains(&h.current.scale)
            || !h.capabilities.transforms.contains(&h.current.transform)
        {
            return Err("Invalid connector state from compositor".into());
        }
    }
    if s.pending.as_ref().is_some_and(|p| p.token.is_empty() || p.token.len() > 256 || p.token.chars().any(char::is_control) || p.remaining_ms > 120_000) {
        return Err("Invalid display confirmation token".into());
    }
    s.warning = s.warning.map(|w| w.chars().filter(|c| !c.is_control()).take(200).collect());
    Ok(s)
}
fn ctl() -> String {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.join("rdwmctl"))).filter(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "rdwmctl".into())
}
pub fn request(r: &dyn Runner, v: Value) -> Result<Snapshot, String> {
    let started = Instant::now();
    let o = r.run(&Cmd::owned(&ctl(), vec!["raw".into(), v.to_string()]).timeout(6));
    if !o.ok {
        return Err(format!("Display control unavailable: {}", o.error_text("rdwmctl")));
    }
    let mut s = parse(&o.stdout)?;
    // A delayed process/socket reply must not restart the server countdown.
    s.received = started;
    Ok(s)
}
pub fn load(r: &dyn Runner) -> State {
    let started = Instant::now();
    match request(r, json!({"cmd":"output_query"})) {
        Ok(s) => State { available: true, wayland_query_started: Some(s.received), wayland: Some(s), ..Default::default() },
        Err(e) => State {
            wayland_query_started: Some(started),
            error: Some(format!("Wayland display management: {e}. Requires an rdwm compositor with output protocol v1; XWayland/xrandr cannot configure physical outputs.")),
            ..Default::default()
        },
    }
}
pub fn validate(s: &Snapshot, d: &Draft) -> Result<(), String> {
    if d.generation != Some(s.generation) || d.instance.as_deref() != Some(s.instance.as_str()) {
        return Err("Displays changed externally. Review and reset the draft before applying.".into());
    }
    if s.pending.is_some() {
        return Err("Keep or revert the pending layout first".into());
    }
    if d.outputs.len() != s.outputs.len() || !d.outputs.iter().any(|h| h.current.enabled) {
        return Err("At least one display must stay on".into());
    }
    let mut names = std::collections::HashSet::new();
    for h in &d.outputs {
        let old = s.outputs.iter().find(|o| o.name == h.name).ok_or("Connector disappeared")?;
        if !names.insert(&h.name) {
            return Err("Duplicate display".into());
        }
        let (a, b, c) = (&old.current, &h.current, &old.capabilities);
        if (!c.enabled && a.enabled != b.enabled)
            || (!c.mode && a.mode != b.mode)
            || (!c.scale && a.scale != b.scale)
            || (!c.transform && a.transform != b.transform)
            || (!c.position && (a.x, a.y) != (b.x, b.y))
        {
            return Err(format!("{} does not support that operation", h.name));
        }
        if !checked_mode(&b.mode)
            || (a.mode != b.mode && !old.modes.contains(&b.mode))
            || !b.scale.is_finite()
            || !(c.scale_min..=c.scale_max).contains(&b.scale)
            || !c.transforms.contains(&b.transform)
            || !(-131072..=131072).contains(&b.x)
            || !(-131072..=131072).contains(&b.y)
        {
            return Err(format!("Invalid output settings for {}", h.name));
        }
    }
    Ok(())
}
fn run(h: &mut Hub, value: Value, success: &str) {
    if h.ui_disp.busy {
        return;
    }
    h.ui_disp.busy = true;
    h.dirty = true;
    let msg = success.to_string();
    let started = h.sh.try_spawn(move |r| {
        let result = request(r, value);
        let (state, message, reset) = match result {
            Ok(s) => {
                let notice = s.warning.as_ref().map(|w| (format!("Display request committed with a warning: {w}"), true)).unwrap_or((msg, false));
                (State { available: true, wayland_query_started: Some(s.received), wayland: Some(s), ..Default::default() }, notice, true)
            }
            Err(e) => (load(r), (format!("Could not confirm the display request: {e}"), true), false),
        };
        Done { data: Some(super::super::Data::Disp(state)), msg: Some(message), display_job_done: true, display_reset: reset, ..Default::default() }
    });
    if !started {
        h.ui_disp.busy = false;
        h.say("Could not start display worker; no changes applied", true);
    }
}
pub fn apply(h: &mut Hub, act: &str, arg: &str, val: CtlVal) {
    let Some(s) = h.disp.data.as_ref().and_then(|s| s.wayland.as_ref()).cloned() else { return };
    if h.ui_disp.generation.is_none() {
        h.ui_disp.reset(&s);
    }
    if h.ui_disp.busy {
        return;
    }
    let result: Result<(), String> = (|| {
        if act == "wl-keep" || act == "wl-revert" {
            let p = s.pending.as_ref().ok_or("No pending display change")?;
            run(
                h,
                json!({"cmd":if act=="wl-keep"{"output_confirm"}else{"output_revert"},"token":p.token}),
                if act == "wl-keep" {
                    if s.capabilities.persist {
                        "Display layout kept and saved"
                    } else {
                        "Display layout kept for this session only"
                    }
                } else {
                    "Display layout reverted"
                },
            );
            return Ok(());
        }
        if s.pending.is_some() {
            return Err("Keep or revert the current display layout first".into());
        }
        if act == "wl-reset" {
            h.ui_disp.reset(&s);
            h.dirty = true;
            return Ok(());
        }
        if act == "wl-apply" {
            validate(&s, &h.ui_disp)?;
            run(
                h,
                json!({"cmd":"output_apply","instance":s.instance,"generation":s.generation,"outputs":h.ui_disp.outputs,"timeout_ms":15000}),
                "Provisional layout applied; keep it within 15 seconds",
            );
            return Ok(());
        }
        if h.ui_disp.generation != Some(s.generation) || h.ui_disp.instance.as_deref() != Some(s.instance.as_str()) {
            return Err("Displays changed externally; reset and review the draft".into());
        }
        let d = h.ui_disp.outputs.iter_mut().find(|d| d.name == arg).ok_or("Display is no longer connected")?;
        match (act, val) {
            ("wl-enabled", CtlVal::Bool(v)) => d.current.enabled = v,
            ("wl-mode", CtlVal::Str(v)) => {
                let i: usize = v.parse().map_err(|_| "Invalid mode")?;
                d.current.mode = s.outputs.iter().find(|o| o.name == arg).and_then(|o| o.modes.get(i)).ok_or("Mode is no longer available")?.clone();
            }
            ("wl-scale", CtlVal::Num(v)) if v.is_finite() && (25.0..=800.0).contains(&v) => d.current.scale = v / 100.,
            ("wl-transform", CtlVal::Str(v)) => d.current.transform = v,
            ("wl-x", CtlVal::Num(v)) if v.is_finite() && (-131072.0..=131072.0).contains(&v) => d.current.x = v as i32,
            ("wl-y", CtlVal::Num(v)) if v.is_finite() && (-131072.0..=131072.0).contains(&v) => d.current.y = v as i32,
            _ => return Err("Unsupported display control".into()),
        }
        h.ui_disp.dirty = true;
        h.dirty = true;
        Ok(())
    })();
    if let Err(e) = result {
        h.say(e, true);
    }
}
fn control(id: String, kind: Kind, enabled: bool) -> Ctl {
    let mut c = Ctl::new(id, kind);
    c.enabled = enabled;
    c
}
fn seconds_left(remaining_ms: u64, elapsed: Duration) -> u64 {
    remaining_ms.saturating_sub(elapsed.as_millis().min(u64::MAX as u128) as u64).div_ceil(1000)
}

pub fn items(h: &Hub, s: &Snapshot) -> Vec<Item> {
    let draft = &h.ui_disp;
    let desired: Vec<Desired> = if draft.generation.is_none() { s.outputs.iter().map(|o| Desired { name: o.name.clone(), current: o.current.clone() }).collect() } else { draft.outputs.clone() };
    let stale = draft.dirty && (draft.generation != Some(s.generation) || draft.instance.as_deref() != Some(s.instance.as_str()));
    let editable = !draft.busy && s.pending.is_none() && !stale;
    let mut v = vec![Item::Note {
        text: format!(
            "rdwm {} · changes are provisional until kept. {}",
            s.backend,
            if s.capabilities.persist { "Keep saves the output layout for next login." } else { "This compositor cannot save output layouts." }
        ),
        warn: false,
    }];
    if let Some(p) = &s.pending {
        let left = seconds_left(p.remaining_ms, s.received.elapsed());
        v.push(Item::Header("Keep these display settings?".into()));
        v.push(Item::Card(vec![Row::new(format!("Compositor will revert in {left} s"))
            .detail("Timeout protection remains active if Settings closes or crashes")
            .ctl(control("s:disp:wl-revert:".into(), Kind::Button { label: "Revert".into(), danger: true }, !draft.busy))
            .ctl(control("s:disp:wl-keep:".into(), Kind::Button { label: "Keep changes".into(), danger: false }, !draft.busy))]));
    }
    if stale {
        v.push(Item::Note { text: "Outputs changed while you were editing. Your draft is retained; reset it to review the new hardware state.".into(), warn: true });
    }
    for o in &s.outputs {
        let d = desired.iter().find(|d| d.name == o.name).map(|d| &d.current).unwrap_or(&o.current);
        let c = &o.capabilities;
        let id = |a: &str| format!("s:disp:wl-{a}:{}", o.name);
        v.push(Item::Header(o.name.clone()));
        let mut rows = vec![Row::new("Enabled").detail(if c.enabled { "At least one output must remain enabled" } else { "Output enable/disable is owned by this backend" }).ctl(control(
            id("enabled"),
            Kind::Toggle(d.enabled),
            editable && c.enabled,
        ))];
        let modes: Vec<Opt> = o.modes.iter().enumerate().map(|(i, m)| (format!("{} × {} · {:.3} Hz", m.width, m.height, m.refresh_mhz as f64 / 1000.), i.to_string())).collect();
        rows.push(
            Row::new("Resolution and refresh rate").detail(if c.mode { "Choose a mode advertised by this connector" } else { "Physical mode is controlled by the host compositor" }).ctl(control(
                id("mode"),
                Kind::Popup(modes, o.modes.iter().position(|m| m == &d.mode)),
                editable && c.mode && d.enabled,
            )),
        );
        rows.push(Row::new("Output scale").detail("Logical desktop size; does not change global Xft/DPI preferences").ctl(control(
            id("scale"),
            Kind::Stepper { v: d.scale * 100., min: c.scale_min * 100., max: c.scale_max * 100., step: 25., unit: "%".into() },
            editable && c.scale && d.enabled,
        )));
        rows.push(Row::new("Rotation / reflection").ctl(control(
            id("transform"),
            Kind::Popup(c.transforms.iter().map(|s| (s.clone(), s.clone())).collect(), c.transforms.iter().position(|s| s == &d.transform)),
            editable && c.transform && d.enabled,
        )));
        rows.push(Row::new("Horizontal position").detail(format!("Current logical position: {}, {}", o.current.x, o.current.y)).ctl(control(
            id("x"),
            Kind::Stepper { v: d.x as f64, min: -131072., max: 131072., step: 10., unit: " px".into() },
            editable && c.position && d.enabled,
        )));
        rows.push(Row::new("Vertical position").ctl(control(
            id("y"),
            Kind::Stepper { v: d.y as f64, min: -131072., max: 131072., step: 10., unit: " px".into() },
            editable && c.position && d.enabled,
        )));
        v.push(Item::Card(rows));
    }
    let error = validate(s, draft).err();
    v.push(Item::Card(vec![Row::new(if draft.busy { "Applying display request…" } else { "Display layout" })
        .detail(error.unwrap_or_else(|| "Changes stay in this draft until Apply".into()))
        .ctl(control("s:disp:wl-reset:".into(), Kind::Button { label: "Reset draft".into(), danger: false }, !draft.busy && s.pending.is_none()))
        .ctl(control("s:disp:wl-apply:".into(), Kind::Button { label: "Apply displays".into(), danger: false }, editable && draft.dirty && validate(s, draft).is_ok()))]));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::{FakeRunner, Out};
    struct Sandbox(std::path::PathBuf);
    impl Sandbox {
        fn new() -> Self {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "decebal-wl-display-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Value {
        serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/wayland-output-v1.json"))).unwrap()
    }
    #[test]
    fn compositor_countdown_uses_ceiling_and_expires_at_zero() {
        assert_eq!(seconds_left(2000, Duration::ZERO), 2);
        assert_eq!(seconds_left(2001, Duration::ZERO), 3);
        assert_eq!(seconds_left(2000, Duration::from_millis(1999)), 1);
        assert_eq!(seconds_left(2000, Duration::from_millis(2000)), 0);
        assert_eq!(seconds_left(0, Duration::ZERO), 0);
    }
    fn snapshot() -> Snapshot {
        parse(&fixture().to_string()).unwrap()
    }
    fn controls(v: &[Item]) -> Vec<&Ctl> {
        v.iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().flat_map(|r| &r.ctls).collect()
    }
    #[test]
    fn parser_gates_protocol_and_bounds_without_xrandr_fallback() {
        let s = snapshot();
        assert_eq!(s.outputs[0].current.mode.refresh_mhz, 60000);
        for value in [json!({"ok":false,"error":"Unsupported command"}), json!({"ok":true}), json!({"ok":false,"error":"Denied\nunsafe"})] {
            assert!(parse(&value.to_string()).is_err());
        }
        for (field, value) in [("protocol", json!(99)), ("backend", json!("unknown")), ("outputs", json!([]))] {
            let mut v = fixture();
            v[field] = value;
            assert!(parse(&v.to_string()).is_err());
        }
        let mut v = fixture();
        v["outputs"][0]["current"]["scale"] = json!(0);
        assert!(parse(&v.to_string()).is_err());
        let mut v = fixture();
        v["outputs"][0]["modes"] = json!(vec![json!({"width":1,"height":1,"refresh_mhz":1}); 257]);
        assert!(parse(&v.to_string()).is_err());
        let f = FakeRunner::new().ok("xrandr --query", "must never read");
        let state = load(&f);
        assert!(!state.available);
        assert!(f.lines().iter().all(|l| !l.contains("xrandr")));
    }
    #[test]
    fn full_inventory_last_output_and_connector_capabilities_are_validated() {
        let s = snapshot();
        let mut d = Draft::default();
        d.reset(&s);
        assert!(validate(&s, &d).is_ok());
        d.outputs[0].current.enabled = false;
        assert!(validate(&s, &d).unwrap_err().contains("At least one"));
        d.reset(&s);
        d.outputs[0].current.mode.refresh_mhz = 60001;
        assert!(validate(&s, &d).is_err());
        d.reset(&s);
        d.generation = Some(6);
        assert!(validate(&s, &d).unwrap_err().contains("changed externally"));
        d.reset(&s);
        let mut s = s;
        s.outputs[0].capabilities.scale = false;
        d.outputs[0].current.scale = 2.;
        assert!(validate(&s, &d).unwrap_err().contains("does not support"));
        d.reset(&s);
        d.outputs.push(d.outputs[0].clone());
        assert!(validate(&s, &d).is_err());
    }
    #[test]
    fn draft_survives_refresh_stale_hardware_and_reset() {
        let mut s = snapshot();
        let mut d = Draft::default();
        d.receive(&s);
        d.outputs[0].current.x = 300;
        d.dirty = true;
        d.receive(&s);
        assert_eq!(d.outputs[0].current.x, 300);
        s.generation += 1;
        s.outputs[0].current.x = 50;
        d.receive(&s);
        assert_eq!(d.outputs[0].current.x, 300);
        assert!(validate(&s, &d).is_err());
        d.reset(&s);
        assert_eq!(d.outputs[0].current.x, 50);
        assert!(!d.dirty);
    }
    #[test]
    fn pending_banner_and_backend_controls_use_cached_capabilities_only() {
        let r = Arc::new(FakeRunner::new());
        let sandbox = Sandbox::new();
        let mut h = Hub::with(r.clone(), sandbox.0.clone(), Arc::new(|| {}));
        let mut s = snapshot();
        s.outputs[0].capabilities.mode = false;
        s.outputs[0].capabilities.enabled = false;
        h.ui_disp.reset(&s);
        let v = items(&h, &s);
        let c = controls(&v);
        assert!(!c.iter().find(|c| c.id.contains("wl-mode:")).unwrap().enabled);
        assert!(!c.iter().find(|c| c.id.contains("wl-enabled:")).unwrap().enabled);
        assert!(!c.iter().any(|c| c.id == "s:disp:scale:"));
        assert!(r.lines().is_empty());
        s.pending = Some(Pending { token: "private-test-token".into(), remaining_ms: 15000 });
        let v = items(&h, &s);
        let c = controls(&v);
        assert!(c.iter().any(|c| c.id == "s:disp:wl-keep:"));
        assert!(c.iter().any(|c| c.id == "s:disp:wl-revert:"));
        assert!(!c.iter().find(|c| c.id.contains("wl-scale:")).unwrap().enabled);
    }
    #[test]
    fn apply_uses_typed_async_worker_and_compositor_returned_pending_state() {
        struct Script {
            main: std::thread::ThreadId,
            calls: std::sync::Mutex<Vec<Value>>,
        }
        impl Runner for Script {
            fn exists(&self, _: &str) -> bool {
                true
            }
            fn run(&self, c: &Cmd) -> Out {
                assert_ne!(std::thread::current().id(), self.main, "IPC must not run on UI thread");
                assert_eq!(c.args[0], "raw");
                let v: Value = serde_json::from_str(&c.args[1]).unwrap();
                self.calls.lock().unwrap().push(v.clone());
                let mut reply = fixture();
                reply["generation"] = json!(8);
                match v["cmd"].as_str().unwrap() {
                    "output_apply" => {
                        assert_eq!(v["outputs"][0]["scale"], 2.0);
                        assert_eq!(v["generation"], 7);
                        assert_eq!(v["timeout_ms"], 15000);
                        reply["pending"] = json!({"token":"own-token","remaining_ms":15000});
                        reply["outputs"][0]["current"]["scale"] = json!(2.0);
                    }
                    "output_confirm" => assert_eq!(v["token"], "own-token"),
                    _ => panic!("Unexpected request"),
                }
                Out::success(&reply.to_string())
            }
        }
        let r = Arc::new(Script { main: std::thread::current().id(), calls: Default::default() });
        let sandbox = Sandbox::new();
        let mut h = Hub::with(r.clone(), sandbox.0.clone(), Arc::new(|| {}));
        let s = snapshot();
        h.ui_disp.reset(&s);
        h.disp.data = Some(State { available: true, wayland: Some(s), ..Default::default() });
        apply(&mut h, "wl-scale", "nested-0", CtlVal::Num(200.));
        assert!(r.calls.lock().unwrap().is_empty());
        apply(&mut h, "wl-apply", "", CtlVal::Bool(true));
        let deadline = Instant::now() + Duration::from_secs(3);
        while h.ui_disp.busy {
            h.drain();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(h.disp.data.as_ref().unwrap().wayland.as_ref().unwrap().pending.is_some());
        assert!(!h.ui_disp.dirty);
        apply(&mut h, "wl-keep", "", CtlVal::Bool(true));
        while h.ui_disp.busy {
            h.drain();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(h.disp.data.as_ref().unwrap().wayland.as_ref().unwrap().pending.is_none());
        assert_eq!(r.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn refused_apply_retains_draft_and_reports_error_without_x11_or_config_writes() {
        struct Denied;
        impl Runner for Denied {
            fn exists(&self, _: &str) -> bool {
                true
            }
            fn run(&self, c: &Cmd) -> Out {
                assert_eq!(c.args[0], "raw");
                let v: Value = serde_json::from_str(&c.args[1]).unwrap();
                if v["cmd"] == "output_apply" {
                    Out::success("{\"ok\":false,\"code\":\"backend\",\"error\":\"Driver refused layout\"}")
                } else {
                    assert_eq!(v["cmd"], "output_query");
                    Out::success(&fixture().to_string())
                }
            }
        }
        let sandbox = Sandbox::new();
        let mut h = Hub::with(Arc::new(Denied), sandbox.0.clone(), Arc::new(|| {}));
        let s = snapshot();
        h.ui_disp.reset(&s);
        h.disp.data = Some(State { available: true, wayland: Some(s), ..Default::default() });
        apply(&mut h, "wl-scale", "nested-0", CtlVal::Num(f64::NAN));
        assert!(!h.ui_disp.dirty);
        assert!(h.ui_disp.outputs[0].current.scale.is_finite());
        apply(&mut h, "wl-scale", "nested-0", CtlVal::Num(200.));
        apply(&mut h, "wl-apply", "", CtlVal::Bool(true));
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut messages = Vec::new();
        while h.ui_disp.busy {
            messages.extend(h.drain().1);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(messages.iter().any(|(s, error)| *error && s.contains("Driver refused layout")));
        assert!(h.ui_disp.dirty);
        assert_eq!(h.ui_disp.outputs[0].current.scale, 2.);
        assert_eq!(h.disp.data.as_ref().unwrap().wayland.as_ref().unwrap().outputs[0].current.scale, 1.);
    }
    #[test]
    fn protocol_fixture_full_inventory_roundtrip_matches_server_endpoint_limits() {
        let mut v = fixture();
        v["outputs"][0]["current"]["x"] = json!(-131072);
        v["outputs"][0]["current"]["y"] = json!(131072);
        v["outputs"][0]["current"]["mode"] = json!({"width":32768,"height":1,"refresh_mhz":1000000});
        let mut disabled = v["outputs"][0].clone();
        disabled["name"] = json!("DP-disabled");
        disabled["current"]["enabled"] = json!(false);
        v["outputs"].as_array_mut().unwrap().push(disabled);
        let s = parse(&v.to_string()).unwrap();
        let mut d = Draft::default();
        d.reset(&s);
        assert!(validate(&s, &d).is_ok());
        let payload = json!({"cmd":"output_apply","instance":s.instance,"generation":s.generation,"outputs":d.outputs,"timeout_ms":15000});
        let roundtrip: Vec<Desired> = serde_json::from_value(payload["outputs"].clone()).unwrap();
        assert_eq!(roundtrip, d.outputs);
        assert_eq!(payload["outputs"][1]["enabled"], false);
        assert_eq!(payload["outputs"][1]["mode"]["width"], 32768);
        d.outputs[0].current.x = -131073;
        assert!(validate(&s, &d).is_err());
        let mut invalid = fixture();
        invalid["outputs"][0]["current"]["mode"]["refresh_mhz"] = json!(0);
        assert!(parse(&invalid.to_string()).is_err());
        let mut warning = fixture();
        warning["warning"] = json!("Directory sync uncertain\nlayout committed");
        let s = parse(&warning.to_string()).unwrap();
        assert_eq!(s.warning.as_deref(), Some("Directory sync uncertainlayout committed"));
    }
    #[test]
    fn reordered_jobs_preserve_latest_snapshot_cleanup_and_messages() {
        fn deliver(h: &mut Hub, s: Snapshot, job: bool) -> Vec<(String, bool)> {
            h.sh.results.lock().unwrap().push_back(Done {
                data: Some(super::super::super::Data::Disp(State { available: true, wayland: Some(s), ..Default::default() })),
                display_job_done: job,
                display_reset: job,
                msg: job.then(|| ("completed worker".into(), false)),
                ..Default::default()
            });
            h.drain().1
        }
        let sandbox = Sandbox::new();
        let mut h = Hub::with(Arc::new(FakeRunner::new()), sandbox.0.clone(), Arc::new(|| {}));
        let base = snapshot();
        deliver(&mut h, base.clone(), false);
        let mut provisional = base.clone();
        provisional.generation += 1;
        provisional.pending = Some(Pending { token: "transaction-1".into(), remaining_ms: 15000 });
        h.ui_disp.busy = true;
        assert!(!deliver(&mut h, provisional.clone(), true).is_empty());
        assert!(!h.ui_disp.busy);
        deliver(&mut h, base, false);
        assert_eq!(h.disp.data.as_ref().unwrap().wayland.as_ref().unwrap().generation, 8);
        assert!(h.sh.wayland_pending.load(std::sync::atomic::Ordering::Relaxed));
        // Same-generation completion is also safe: an already completed token is
        // never resurrected by a delayed query, independent of server counters.
        let mut confirmed = provisional.clone();
        confirmed.pending = None;
        deliver(&mut h, confirmed, false);
        h.ui_disp.busy = true;
        let messages = deliver(&mut h, provisional, true);
        assert!(!h.ui_disp.busy);
        assert_eq!(messages.len(), 1);
        assert!(h.disp.data.as_ref().unwrap().wayland.as_ref().unwrap().pending.is_none());
        assert!(!h.sh.wayland_pending.load(std::sync::atomic::Ordering::Relaxed));
    }
    #[test]
    fn restarted_compositor_retires_old_epoch_preserves_draft_and_never_extends_deadline() {
        let mut d = Draft::default();
        let mut a = snapshot();
        a.pending = Some(Pending { token: "old-token".into(), remaining_ms: 15000 });
        assert!(d.accept(&mut a));
        d.reset(&a);
        d.outputs[0].current.x = 123;
        d.dirty = true;
        let deadline = d.deadline.clone().unwrap().1;
        let mut delayed = a.clone();
        delayed.received += Duration::from_secs(3);
        assert!(d.accept(&mut delayed));
        assert_eq!(d.deadline.as_ref().unwrap().1, deadline);
        assert_eq!(delayed.pending.as_ref().unwrap().remaining_ms, 12000);
        let mut restarted = snapshot();
        restarted.instance = "new-compositor-instance".into();
        restarted.received = delayed.received + Duration::from_secs(1);
        restarted.pending = None;
        assert!(d.accept(&mut restarted));
        d.receive(&restarted);
        assert!(d.dirty);
        assert_eq!(d.outputs[0].current.x, 123);
        assert!(validate(&restarted, &d).is_err());
        assert!(!d.accept(&mut a));
        d.reset(&restarted);
        assert!(validate(&restarted, &d).is_ok());
        let mut initial = Draft::default();
        let mut first = restarted.clone();
        first.received += Duration::from_secs(1);
        assert!(initial.accept(&mut first));
        // Prior process was never seen before the new reply won the race.
        assert!(!initial.accept(&mut a));
    }
    #[test]
    fn delayed_failed_refresh_does_not_erase_apply_or_keep_state() {
        let sandbox = Sandbox::new();
        let mut h = Hub::with(Arc::new(FakeRunner::new()), sandbox.0.clone(), Arc::new(|| {}));
        let old_started = Instant::now();
        let mut new = snapshot();
        new.received = old_started + Duration::from_secs(1);
        new.generation = 8;
        new.pending = Some(Pending { token: "latest-apply".into(), remaining_ms: 15000 });
        for pending in [true, false] {
            if !pending {
                new.generation += 1;
                new.pending = None;
                new.received += Duration::from_secs(1);
            }
            h.sh.results
                .lock()
                .unwrap()
                .push_back(Done { data: Some(super::super::super::Data::Disp(State { available: true, wayland: Some(new.clone()), ..Default::default() })), ..Default::default() });
            h.drain();
            let at = h.disp.at;
            h.ui_disp.busy = true;
            h.sh.results.lock().unwrap().push_back(Done {
                data: Some(super::super::super::Data::Disp(State { wayland_query_started: Some(old_started), error: Some("older query timed out".into()), ..Default::default() })),
                display_job_done: true,
                msg: Some(("worker completed".into(), false)),
                ..Default::default()
            });
            let (_, messages) = h.drain();
            assert!(!h.ui_disp.busy);
            assert_eq!(h.disp.at, at, "discarded error must not freshen the state");
            assert_eq!(h.disp.data.as_ref().unwrap().wayland.as_ref().unwrap().generation, new.generation);
            assert_eq!(h.sh.wayland_pending.load(std::sync::atomic::Ordering::Relaxed), pending);
            assert!(messages.iter().any(|(text, warn)| *warn && text.contains("older display refresh failed")));
            assert!(messages.iter().any(|(text, _)| text == "worker completed"));
        }
        // A genuinely newer failure is not hidden behind the cached state.
        let fresh = new.received + Duration::from_secs(1);
        h.sh.results.lock().unwrap().push_back(Done {
            data: Some(super::super::super::Data::Disp(State { wayland_query_started: Some(fresh), error: Some("current query unavailable".into()), ..Default::default() })),
            ..Default::default()
        });
        h.drain();
        assert!(!h.disp.data.as_ref().unwrap().available);
        assert!(h.disp.data.as_ref().unwrap().error.as_ref().unwrap().contains("current query"));
    }
}
