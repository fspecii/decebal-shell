//! Notifications: do-not-disturb, banner duration, per-app mute list.
//!
//! Stored in `~/.config/decebal/notifications.toml` (read by the notification daemon, decnotify):
//!
//! ```toml
//! dnd = false                 # do not disturb: no banners (notifications are still kept in the history)
//! banner_seconds = 5          # how long a banner stays: 1..=60, 0 = until clicked
//! muted_apps = ["Slack"]      # app names (the notification's app_name, matched case-insensitively): no banner, no sound
//! ```
//! Unknown keys and comments are preserved. A missing file means the defaults above.

use super::run::Cmd;
use super::session::write_atomic;
use super::Hub;
use crate::settings::model::CtlVal;
use crate::settings::ui::{Ctl, Item, Kind, Row};
use std::path::Path;
use toml_edit::{Array, DocumentMut, Item as TItem, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct NotifCfg {
    pub dnd: bool,
    pub sounds: bool,
    pub banner_seconds: u32,
    pub muted_apps: Vec<String>,
}

impl Default for NotifCfg {
    fn default() -> Self {
        NotifCfg { dnd: false, sounds: false, banner_seconds: 5, muted_apps: vec![] }
    }
}

pub const FILE: &str = "notifications.toml";

impl NotifCfg {
    pub fn parse(text: &str) -> NotifCfg {
        let d = NotifCfg::default();
        let Ok(doc) = text.parse::<DocumentMut>() else { return d };
        NotifCfg {
            dnd: doc.get("dnd").and_then(|v| v.as_bool()).unwrap_or(d.dnd),
            sounds: doc.get("sounds").and_then(|v| v.as_bool()).unwrap_or(false),
            banner_seconds: doc.get("banner_seconds").and_then(|v| v.as_integer()).map(|n| n.clamp(0, 60) as u32).unwrap_or(d.banner_seconds),
            muted_apps: doc.get("muted_apps").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        }
    }

    pub fn load(dir: &Path) -> NotifCfg {
        std::fs::read_to_string(dir.join(FILE)).map(|t| NotifCfg::parse(&t)).unwrap_or_default()
    }

    /// Edits the existing text (comments and unknown keys survive).
    pub fn render(&self, old: &str) -> String {
        let mut doc = old.parse::<DocumentMut>().unwrap_or_default();
        if old.trim().is_empty() {
            doc.decor_mut().set_prefix(
                "# Notification settings, written by decsettings and read by decnotify.\n# dnd: do not disturb. banner_seconds: 1..60 (0 = until clicked). muted_apps: app names, case-insensitive.\n",
            );
        }
        let mut arr = Array::new();
        for a in &self.muted_apps {
            arr.push(a.as_str());
        }
        set_value(&mut doc, "dnd", Value::from(self.dnd));
        set_value(&mut doc, "sounds", Value::from(self.sounds));
        set_value(&mut doc, "banner_seconds", Value::from(self.banner_seconds as i64));
        set_value(&mut doc, "muted_apps", Value::Array(arr));
        doc.to_string()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let old = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
        write_atomic(&dir.join(FILE), &self.render(&old))
    }
}

/// Replaces a value but keeps its comments and spacing.
fn set_value(doc: &mut DocumentMut, key: &str, mut v: Value) {
    match doc.get_mut(key).and_then(|i| i.as_value_mut()) {
        Some(old) => {
            *v.decor_mut() = old.decor().clone();
            *old = v;
        }
        None => doc[key] = TItem::Value(v),
    }
}

pub fn valid_app(s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("Type the application's name".into());
    }
    if t.chars().count() > 60 || t.chars().any(|c| c.is_control()) {
        return Err("That is not a valid application name".into());
    }
    Ok(t.to_string())
}

#[derive(Default)]
pub struct Ui {
    pub new_app: String,
}

pub fn items(h: &Hub) -> Vec<Item> {
    let c = &h.notif;
    let mut v = vec![Item::Header("Do Not Disturb".into())];
    v.push(Item::Card(vec![Row::new("Do Not Disturb").detail("Silence banners and sounds; notifications are still kept").ctl(Ctl::new("s:not:dnd:", Kind::Toggle(c.dnd)))]));
    v.push(Item::Header("Banners".into()));
    v.push(Item::Card(vec![
        Row::new("Banner duration")
            .detail(if c.banner_seconds == 0 { "Banners stay until you click them".to_string() } else { "How long a banner stays on screen".to_string() })
            .ctl(Ctl::new("s:not:secs:", Kind::Stepper { v: c.banner_seconds as f64, min: 0.0, max: 60.0, step: 1.0, unit: " s".into() })),
        Row::new("Notification sounds").detail("Play event sounds with canberra-gtk-play; silent during Do Not Disturb").ctl(Ctl::new("s:not:sounds:", Kind::Toggle(c.sounds))),
        Row::new("Send a test notification").detail("Needs notify-send and a running notification daemon").ctl(Ctl::new("s:not:test:", Kind::Button { label: "Send test".into(), danger: false })),
    ]));
    v.push(Item::Header("Muted apps".into()));
    let mut rows: Vec<Row> =
        c.muted_apps.iter().enumerate().map(|(i, a)| Row::new(a.clone()).ctl(Ctl::new(format!("s:not:unmute:{i}"), Kind::Button { label: "Unmute".into(), danger: false }))).collect();
    if rows.is_empty() {
        rows.push(Row::new("No muted apps").detail("Muted apps never show banners"));
    }
    rows.push(
        Row::new("Mute an app")
            .detail("The name the app uses in its notifications")
            .ctl(Ctl::new("s:not:newapp:", Kind::Text { val: h.ui_not.new_app.clone(), hint: "e.g. Slack".into() }))
            .ctl(Ctl::new("s:not:mute:", Kind::Button { label: "Mute".into(), danger: false })),
    );
    v.push(Item::Card(rows));
    v.push(Item::Note { text: format!("Saved in {}. The notification daemon (decnotify) reads it.", h.sh.dir.join(FILE).display()), warn: false });
    v
}

pub fn apply(h: &mut Hub, act: &str, arg: &str, val: CtlVal) {
    let mut n = h.notif.clone();
    let r: Result<(), String> = (|| {
        match (act, &val) {
            ("dnd", CtlVal::Bool(b)) => n.dnd = *b,
            ("sounds", CtlVal::Bool(b)) => n.sounds = *b,
            ("secs", CtlVal::Num(s)) => {
                if !(0.0..=60.0).contains(s) {
                    return Err("Banner duration must be between 0 and 60 seconds".into());
                }
                n.banner_seconds = *s as u32;
            }
            ("newapp", CtlVal::Str(s)) => {
                h.ui_not.new_app = s.clone();
                h.dirty = true;
                return Ok(());
            }
            ("mute", _) => {
                let a = valid_app(&h.ui_not.new_app)?;
                if n.muted_apps.iter().any(|x| x.eq_ignore_ascii_case(&a)) {
                    return Err(format!("{a} is already muted"));
                }
                n.muted_apps.push(a);
                h.ui_not.new_app.clear();
            }
            ("unmute", _) => {
                let i: usize = arg.parse().map_err(|_| "Unknown entry")?;
                if i >= n.muted_apps.len() {
                    return Err("That entry is gone".into());
                }
                n.muted_apps.remove(i);
            }
            ("test", _) => {
                if !h.sh.runner.exists("notify-send") {
                    return Err("notify-send is not installed".into());
                }
                h.sh.runner.spawn(&Cmd::new("notify-send", &["-a", "System Settings", "Test notification", "Banners look like this."]));
                return Ok(());
            }
            _ => return Err("Unsupported notification change".into()),
        }
        n.save(&h.sh.dir)?;
        h.notif = n;
        h.dirty = true;
        Ok(())
    })();
    if let Err(e) = r {
        h.say(e, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::system::run::FakeRunner;
    use std::sync::Arc;

    fn hub(n: &str) -> (Hub, std::path::PathBuf, Arc<FakeRunner>) {
        let d = std::env::temp_dir().join(format!("ms-not-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let fr = Arc::new(FakeRunner::lenient().have("notify-send"));
        (Hub::with(fr.clone(), d.clone(), Arc::new(|| {})), d, fr)
    }

    #[test]
    fn parse_defaults_and_values() {
        assert_eq!(NotifCfg::parse(""), NotifCfg::default());
        assert_eq!(NotifCfg::parse("garbage ["), NotifCfg::default());
        let c = NotifCfg::parse("dnd = true\nbanner_seconds = 99\nmuted_apps = [\"Slack\", \"spotify\"]\nextra = 1\n");
        assert!(c.dnd);
        assert_eq!(c.banner_seconds, 60, "clamped");
        assert_eq!(c.muted_apps, ["Slack", "spotify"]);
    }

    #[test]
    fn render_keeps_comments_and_unknown_keys() {
        let old = "# my notes\ndnd = false # inline\nfuture_key = \"x\"\n";
        let c = NotifCfg { dnd: true, sounds: true, banner_seconds: 8, muted_apps: vec!["A".into()] };
        let t = c.render(old);
        assert!(t.contains("# my notes") && t.contains("future_key = \"x\"") && t.contains("# inline"));
        assert_eq!(NotifCfg::parse(&t), c);
        let fresh = NotifCfg::default().render("");
        assert!(fresh.starts_with("# Notification settings") && fresh.contains("muted_apps = []"));
    }

    #[test]
    fn toggles_and_lists_persist() {
        let (mut h, d, _) = hub("a");
        apply(&mut h, "dnd", "", CtlVal::Bool(true));
        apply(&mut h, "secs", "", CtlVal::Num(12.0));
        apply(&mut h, "newapp", "", CtlVal::Str("  Slack ".into()));
        apply(&mut h, "mute", "", CtlVal::Click);
        apply(&mut h, "newapp", "", CtlVal::Str("slack".into()));
        apply(&mut h, "mute", "", CtlVal::Click);
        assert!(h.msgs.iter().any(|m| m.1 && m.0.contains("already")));
        assert_eq!(NotifCfg::load(&d), NotifCfg { dnd: true, sounds: false, banner_seconds: 12, muted_apps: vec!["Slack".into()] });
        apply(&mut h, "unmute", "0", CtlVal::Click);
        assert!(NotifCfg::load(&d).muted_apps.is_empty());
        apply(&mut h, "sounds", "", CtlVal::Bool(true));
        assert!(NotifCfg::load(&d).sounds);
        assert!(items(&h).iter().any(|item| matches!(item, Item::Card(rows) if rows.iter().any(|r| r.label == "Notification sounds"))));
        apply(&mut h, "unmute", "5", CtlVal::Click);
        apply(&mut h, "secs", "", CtlVal::Num(999.0));
        assert_eq!(h.msgs.iter().filter(|m| m.1).count(), 3);
        apply(&mut h, "newapp", "", CtlVal::Str("".into()));
        apply(&mut h, "mute", "", CtlVal::Click);
        assert!(h.msgs.last().unwrap().0.contains("name"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_notification_uses_notify_send() {
        let (mut h, _, fr) = hub("b");
        apply(&mut h, "test", "", CtlVal::Click);
        assert!(fr.lines()[0].starts_with("notify-send -a System Settings"));
        let fr2 = Arc::new(FakeRunner::new());
        let mut h2 = Hub::with(fr2, std::env::temp_dir().join("ms-not-c"), Arc::new(|| {}));
        apply(&mut h2, "test", "", CtlVal::Click);
        assert!(h2.msgs[0].0.contains("not installed"));
    }

    #[test]
    fn items_list_muted_apps() {
        let (mut h, _, _) = hub("d");
        h.notif.muted_apps = vec!["Slack".into()];
        let t = format!("{:?}", items(&h));
        assert!(t.contains("s:not:unmute:0") && t.contains("s:not:dnd:") && t.contains("s:not:newapp:"));
    }
}
