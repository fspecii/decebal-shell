//! Panels (Desktop & Dock pane): where the bar and the dock / taskbar sit, and every applet on or off, for whatever
//! theme is applied. Nothing is preselected from the style: each control says what the theme would do ("Theme
//! default: left") and whether the current value is the user's own choice. Writes only the user keys of
//! docs/universal-panels.md (`decebal.json` `bar.edge` / `bar.split` / `bar.applets`, `dock-settings.json` `edge`);
//! "Use theme defaults" removes exactly those. Running bars follow the files within half a second.

use super::model::{CtlVal, Model};
use super::ui::{Ctl, Item, Kind, Opt, Row};
use crate::panel::{self, AppletId, Edge, Effective, Need, Origin, Renderer, UserPanel};
use serde_json::Value;

/// The panel inputs as the files on disk say them, with the live theme's package defaults.
fn inputs(m: &Model) -> (Renderer, UserPanel, panel::ThemePanel, Option<crate::config::BarPosition>, Option<bool>, crate::config::BarLayout) {
    let th = crate::theme::th();
    let bar = &m.shell["bar"];
    let legacy = bar.get("position").and_then(Value::as_str).and_then(crate::config::BarPosition::parse);
    let ws = m.shell.get("workspaces").and_then(|w| serde_json::from_value(w.clone()).ok()).unwrap_or(crate::config::Toggle::Word(crate::config::Word::Auto));
    (th.bar_renderer(), UserPanel::from_bar_json(bar), th.panel_theme.clone(), legacy, panel::legacy_workspaces(ws), crate::config::BarLayout::from_json(bar))
}

fn shell_settings(m: &Model) -> crate::config::ShellSettings {
    serde_json::from_value(m.shell.clone()).unwrap_or_default()
}

fn taskbar_visible(m: &Model) -> bool {
    crate::theme::style::dock_style(&m.dock) == crate::config::DockStyle::Windows && shell_settings(m).dock_visible(crate::theme::th().bar_vertical())
}

fn applet_renderer(m: &Model, bar: Renderer) -> Renderer {
    if taskbar_visible(m) && !shell_settings(m).bar_visible(crate::config::DockStyle::Windows) {
        Renderer::Taskbar
    } else {
        bar
    }
}

fn dock_edge(m: &Model) -> Option<Edge> {
    m.dock.get("edge").and_then(Value::as_str).and_then(Edge::parse)
}

/// The bar's effective panel and how each part was decided.
pub fn effective(m: &Model) -> (Effective, Origin) {
    let (renderer, user, theme, legacy, ws, dux) = inputs(m);
    let i = panel::Inputs { renderer, user: &user, theme: &theme, legacy_position: legacy, legacy_workspaces: ws, dux: Some(&dux), dock_user_edge: None };
    (panel::resolve(&i), panel::resolve_edge(&i).2)
}

/// Whether the machine has what an applet needs (cheap file-system checks; `None` = cannot tell from here).
fn detected(need: Need) -> Option<bool> {
    let any_dir = |p: &str| std::fs::read_dir(p).map(|mut d| d.next().is_some()).unwrap_or(false);
    let have = |t: &str| crate::controlcenter::sys::have(t) || std::path::Path::new(&crate::launch::sibling_or_path(t)).is_file();
    Some(match need {
        Need::Nothing | Need::ActiveState | Need::WeatherData | Need::MediaPlayer | Need::TrayApps => return None,
        Need::WindowManager | Need::WindowList => std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some(),
        Need::Rdwm => crate::wm::is_rdwm(),
        Need::Battery => {
            std::fs::read_dir("/sys/class/power_supply").map(|d| d.flatten().any(|e| std::fs::read_to_string(e.path().join("type")).is_ok_and(|t| t.trim() == "Battery"))).unwrap_or(false)
        }
        Need::Backlight => any_dir("/sys/class/backlight"),
        Need::Bluetooth => have("bluetoothctl"),
        Need::AudioSink => have("wpctl") || have("pactl"),
        Need::NetworkManager => any_dir("/sys/class/net"),
        Need::Program(p) => have(p),
    })
}

fn edge_options(split: bool, default: &str) -> Vec<Opt> {
    let mut o: Vec<Opt> = vec![(format!("Theme ({default})"), "theme".into())];
    o.extend(Edge::ALL.iter().map(|e| (e.title().to_string(), e.as_str().to_string())));
    if split {
        o.push(("Split".into(), "split".into()));
    }
    o
}

/// The header, the placement card, the applets card and a note, ready to append to a pane.
pub fn items(m: &Model) -> Vec<Item> {
    let th = crate::theme::th();
    let (eff, origin) = effective(m);
    let (renderer, user, theme, legacy, ws, dux) = inputs(m);
    let none = UserPanel::default();
    let default_inputs = panel::Inputs { renderer, user: &none, theme: &theme, legacy_position: legacy, legacy_workspaces: ws, dux: Some(&dux), dock_user_edge: None };
    let (default_edge, default_split, _) = panel::resolve_edge(&default_inputs);
    let default_word = if default_split { "split".to_string() } else { default_edge.as_str().to_string() };
    let split = renderer.supports_split();
    let bar_opts = edge_options(split, &default_word);
    let bar_sel = if user.edge.is_none() && user.split.is_none() { Some(0) } else { bar_opts.iter().position(|o| o.1 == if eff.split { "split" } else { eff.edge.as_str() }) };
    let dock_style = crate::theme::style::dock_style(&m.dock);
    let dock_name = if dock_style == crate::config::DockStyle::Windows { "Taskbar" } else { "Dock" };
    let dock_default = theme.dock_edge.unwrap_or(if m.dock.get("windowsPosition").and_then(Value::as_str) == Some("top") { Edge::Top } else { Edge::Bottom });
    let dock_opts = edge_options(false, dock_default.as_str());
    let dock_sel = match dock_edge(m) {
        None => Some(0),
        Some(e) => dock_opts.iter().position(|o| o.1 == e.as_str()),
    };
    let bar_on = shell_settings(m).bar_visible(dock_style);
    let mut place = vec![
        Row::new(format!("{} position", renderer.title()))
            .detail(format!("Now on the {} edge ({}). Every bar style draws all four edges in its own look", if eff.split { "top and bottom" } else { eff.edge.as_str() }, origin.title()))
            .ctl(Ctl::new("pn:edge", Kind::Seg(bar_opts, bar_sel))),
        Row::new(format!("{dock_name} position")).detail("The dock or taskbar keeps clear of the bar when both use one edge").ctl(Ctl::new("pn:dock", Kind::Seg(dock_opts, dock_sel))),
    ];
    if !bar_on {
        place.push(
            Row::new("Show the bar")
                .detail("The separate bar is off. Applets use the taskbar when it is visible; otherwise turning one on also enables the bar")
                .ctl(Ctl::new("pn:bar", Kind::Toggle(false))),
        );
    }
    place.push(
        Row::new("Theme defaults")
            .detail("Forget your position and applet choices; the theme decides again")
            .ctl(Ctl::new("pn:reset", Kind::Button { label: "Use theme defaults".into(), danger: false })),
    );
    let applet_renderer = applet_renderer(m, renderer);
    let applet_inputs = panel::Inputs { renderer: applet_renderer, legacy_workspaces: if applet_renderer == Renderer::Taskbar { None } else { ws }, ..default_inputs };
    let mut applets = Vec::new();
    for id in AppletId::all() {
        let info = id.info();
        let (on, from) = panel::resolve_applet(&panel::Inputs { user: &user, ..applet_inputs }, id);
        let (theme_on, _) = panel::resolve_applet(&applet_inputs, id);
        let mut detail = format!(
            "{}. {} ({})",
            cap(info.action),
            if on { "On" } else { "Off" },
            if from == Origin::User { format!("your choice; theme default {}", if theme_on { "on" } else { "off" }) } else { from.title().to_string() }
        );
        match detected(info.need) {
            Some(false) => detail.push_str(&format!(". Not on this machine: {}", info.need.explain())),
            None if info.need != Need::Nothing => detail.push_str(&format!(". {}", cap(info.need.explain()))),
            _ => {}
        }
        if id == AppletId::ControlCenter && !th.control_center.enabled {
            detail.push_str(". The Control Center itself is off (below)");
        }
        applets.push(Row::new(info.title).detail(detail).ctl(Ctl::new(format!("pn:applet:{}", info.key), Kind::Toggle(on))));
    }
    vec![
        Item::Header("Panels".into()),
        Item::Card(place),
        Item::Header("Applets".into()),
        Item::Card(applets),
        Item::Note {
            text: "Choices here are yours: palette, mode and theme changes keep them. Right-click a switch to give that applet back to the theme. Applets that do not fit go to the panel's overflow (…) list.".into(),
            warn: false,
        },
    ]
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Handles `pn:*` controls.
pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    let mut shell = m.shell.clone();
    let mut dock_edge_value: Option<Option<Value>> = None;
    let said: String = match (parts, v) {
        (["pn", "edge"], CtlVal::Str(w)) => match w.as_str() {
            "theme" => {
                panel::write_edge(&mut shell, None, None);
                "The bar follows the theme's position".into()
            }
            "split" => {
                panel::write_edge(&mut shell, Some(Edge::Top), Some(true));
                "Bar split across top and bottom".into()
            }
            w => match Edge::parse(w) {
                Some(e) => {
                    panel::write_edge(&mut shell, Some(e), Some(false));
                    format!("Bar on the {} edge", e.as_str())
                }
                None => return,
            },
        },
        (["pn", "edge"], CtlVal::Reset) => {
            panel::write_edge(&mut shell, None, None);
            "The bar follows the theme's position".into()
        }
        (["pn", "dock"], CtlVal::Str(w)) => {
            dock_edge_value = Some(Edge::parse(&w).map(|e| Value::from(e.as_str())));
            match Edge::parse(&w) {
                Some(e) => format!("Dock on the {} edge", e.as_str()),
                None => "The dock follows the theme's position".into(),
            }
        }
        (["pn", "dock"], CtlVal::Reset) => {
            dock_edge_value = Some(None);
            "The dock follows the theme's position".into()
        }
        (["pn", "bar"], CtlVal::Bool(on)) => {
            shell["bar"]["enabled"] = Value::from(on);
            if on { "Bar on" } else { "Bar off" }.into()
        }
        (["pn", "applet", key], CtlVal::Bool(on)) => {
            let Some(id) = AppletId::parse(key) else { return };
            panel::write_applet(&mut shell, id, Some(on));
            let bar_off = !shell_settings(m).bar_visible(crate::theme::style::dock_style(&m.dock));
            if on && bar_off && !taskbar_visible(m) {
                // the applet must be somewhere: the theme's bar comes on (the single decbar process)
                shell["bar"]["enabled"] = Value::from(true);
                format!("{} on; the bar is on to show it", id.title())
            } else {
                format!("{} {}", id.title(), if on { "on" } else { "off" })
            }
        }
        (["pn", "applet", key], CtlVal::Reset) => {
            let Some(id) = AppletId::parse(key) else { return };
            panel::write_applet(&mut shell, id, None);
            format!("{} follows the theme", id.title())
        }
        (["pn", "reset"], CtlVal::Click) => {
            panel::reset(&mut shell);
            dock_edge_value = Some(None);
            "Panels follow the theme again".into()
        }
        _ => return,
    };
    // Both keys go through the persistence worker, in order; the notice follows once they are on disk.
    let bar_changed = shell.get("bar") != m.shell.get("bar");
    if bar_changed {
        let last = dock_edge_value.is_none();
        m.edit_json_said("shell", "bar", shell.get("bar").cloned(), last.then(|| said.clone()));
    }
    if let Some(v) = dock_edge_value {
        m.edit_json_said("dock", "edge", v, Some(said.clone()));
    }
    if !bar_changed && m.notice.as_ref().is_none_or(|n| !n.error) && !m.saving() {
        m.say(said, false);
    }
    m.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ui::Item;

    fn model(name: &str) -> (std::path::PathBuf, Model) {
        use crate::settings::io::{Paths, Queue};
        let d = std::env::temp_dir().join(format!("decebal-panelpane-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        let queue: Queue = std::sync::Arc::new(std::sync::Mutex::new(Default::default()));
        let m = Model::new(paths, crate::settings::schema::Schema::snapshot(), queue, std::sync::Arc::new(|| {}));
        (d, m)
    }

    fn rows(items: &[Item]) -> Vec<&Row> {
        items.iter().filter_map(|i| if let Item::Card(r) = i { Some(r) } else { None }).flatten().collect()
    }

    #[test]
    fn every_applet_has_an_accessible_switch_and_nothing_is_preselected() {
        let (dir, m) = model("rows");
        let items = items(&m);
        let rs = rows(&items);
        for id in AppletId::all() {
            let row = rs.iter().find(|r| r.ctls.iter().any(|c| c.id == format!("pn:applet:{}", id.key()))).unwrap_or_else(|| panic!("{} has no switch", id.key()));
            assert_eq!(row.label, id.title(), "the switch is labelled");
            assert!(!row.detail.is_empty(), "{}: says what it does and where its value comes from", id.key());
        }
        let edge = rs.iter().find(|r| r.ctls.iter().any(|c| c.id == "pn:edge")).unwrap();
        match &edge.ctls[0].kind {
            Kind::Seg(opts, sel) => {
                assert_eq!(*sel, Some(0), "without a choice the selector says Theme, not an edge");
                for e in Edge::ALL {
                    assert!(opts.iter().any(|o| o.1 == e.as_str()), "{} offered", e.as_str());
                }
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn choices_are_written_as_user_keys_and_reset_removes_only_them() {
        let (dir, mut m) = model("write");
        std::fs::write(m.paths.shell_json(), r#"{"bar": {"enabled": true, "position": "left", "left": ["clock"]}, "theme": {"name": "nord"}}"#).unwrap();
        m.shell = super::super::io::json_load(&m.paths.shell_json());
        apply(&mut m, &["pn", "edge"], CtlVal::Str("bottom".into()));
        apply(&mut m, &["pn", "applet", "weather"], CtlVal::Bool(false));
        apply(&mut m, &["pn", "applet", "media"], CtlVal::Bool(true));
        apply(&mut m, &["pn", "dock"], CtlVal::Str("left".into()));
        let doc = super::super::io::json_load(&m.paths.shell_json());
        assert_eq!(doc["bar"]["edge"], "bottom");
        assert_eq!(doc["bar"]["applets"], serde_json::json!({"weather": false, "media": true}));
        assert_eq!(super::super::io::json_load(&m.paths.dock_settings())["edge"], "left");
        assert_eq!(effective(&m).0.edge, Edge::Bottom);
        apply(&mut m, &["pn", "applet", "media"], CtlVal::Reset);
        assert_eq!(super::super::io::json_load(&m.paths.shell_json())["bar"]["applets"], serde_json::json!({"weather": false}));
        apply(&mut m, &["pn", "reset"], CtlVal::Click);
        let doc = super::super::io::json_load(&m.paths.shell_json());
        assert!(doc["bar"].get("edge").is_none() && doc["bar"].get("applets").is_none() && doc["bar"].get("split").is_none());
        assert_eq!(doc["bar"]["left"], serde_json::json!(["clock"]), "other bar keys stay");
        assert_eq!(doc["theme"]["name"], "nord");
        assert!(super::super::io::json_load(&m.paths.dock_settings()).get("edge").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_visible_taskbar_supplies_applet_defaults_without_spawning_a_bar() {
        let (dir, mut m) = model("taskbar-defaults");
        let shell = serde_json::json!({"bar": {"enabled": "auto"}, "dock": {"enabled": true}});
        std::fs::write(m.paths.shell_json(), shell.to_string()).unwrap();
        m.shell = shell;
        m.dock = serde_json::json!({"style": "windows"});
        assert_eq!(applet_renderer(&m, Renderer::MenuBar), Renderer::Taskbar);
        let rs_items = items(&m);
        for key in ["launcher", "tasks", "show_desktop"] {
            let id = format!("pn:applet:{key}");
            let row = rows(&rs_items).into_iter().find(|r| r.ctls.iter().any(|c| c.id == id)).unwrap();
            assert!(matches!(row.ctls[0].kind, Kind::Toggle(true)), "{key} follows the visible taskbar defaults");
        }
        apply(&mut m, &["pn", "applet", "launcher"], CtlVal::Bool(true));
        assert_eq!(m.shell["bar"]["enabled"], "auto", "the taskbar already displays the applet");
        m.shell["dock"]["enabled"] = Value::Bool(false);
        std::fs::write(m.paths.shell_json(), m.shell.to_string()).unwrap();
        apply(&mut m, &["pn", "applet", "launcher"], CtlVal::Bool(true));
        assert_eq!(m.shell["bar"]["enabled"], true, "with no visible taskbar the applet needs the bar");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn turning_an_applet_on_under_a_bar_less_theme_turns_the_bar_on() {
        let (dir, mut m) = model("barless");
        std::fs::write(m.paths.shell_json(), r#"{"bar": {"enabled": false}}"#).unwrap();
        m.shell = super::super::io::json_load(&m.paths.shell_json());
        apply(&mut m, &["pn", "applet", "clock"], CtlVal::Bool(true));
        let doc = super::super::io::json_load(&m.paths.shell_json());
        assert_eq!((doc["bar"]["enabled"].clone(), doc["bar"]["applets"]["clock"].clone()), (Value::Bool(true), Value::Bool(true)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
